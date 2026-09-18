//! Project Zero - Stage 3G Kernel Object Subsystem
//!
//! Authoritative Contract: Stage 3G Architecture Rev10 (Approved & Frozen).
//!
//! Monotonic Lock Order:
//!   KERNEL_OBJECT_TABLE_LOCK < SCHEDULER.lock < CPU (IF=0)

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::ipc::types::{IpcError, ObjectId};

pub const MAX_KERNEL_OBJECTS: usize = 256;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelObjectType {
    Free = 0,
    Channel = 1,
    ShmObject = 2,
    Event = 3,
    Process = 4,
    StorageObject = 5,
    Device = 6,
    Socket = 7,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectLifecycleState {
    Free = 0,        // Slot available for allocation (occupied == false)
    Active = 1,      // ref_count > 0; fully operational (occupied == true)
    PeerClosed = 2,  // ref_count > 0; one endpoint closed, peer alive (occupied == true)
    Quiescent = 3,   // ref_count == 0; awaiting cleanup sweep (occupied == true)
    Reclaiming = 4,  // Resources being cleared (occupied == true)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KernelObjectHeader {
    /// Monotonic unique 64-bit object identifier across kernel lifetime.
    pub object_id: u64,
    /// Process ID of creator process.
    pub creator_pid: u64,
    /// Total active handle references across all processes.
    pub handle_refs: u16,
    /// Number of active page-table mappings in any AddressSpace.
    pub mapping_refs: u16,
    /// Number of active in-flight operations referencing this object.
    pub in_flight_op_refs: u16,
    /// High-level lifecycle state.
    pub state: ObjectLifecycleState,
    /// Reserved padding byte.
    pub _reserved1: u8,
    /// Active signal mask (bits defined per object type).
    pub signals: u32,
    /// Explicit padding for 8-byte alignment.
    pub _reserved2: u32,
}

const _: () = assert!(core::mem::size_of::<KernelObjectHeader>() == 32);
const _: () = assert!(core::mem::align_of::<KernelObjectHeader>() == 8);

impl KernelObjectHeader {
    pub const fn empty() -> Self {
        Self {
            object_id: 0,
            creator_pid: 0,
            handle_refs: 0,
            mapping_refs: 0,
            in_flight_op_refs: 0,
            state: ObjectLifecycleState::Free,
            _reserved1: 0,
            signals: 0,
            _reserved2: 0,
        }
    }

    #[inline(always)]
    pub fn ref_count(&self) -> usize {
        (self.handle_refs as usize) + (self.mapping_refs as usize) + (self.in_flight_op_refs as usize)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KernelObjectSlot {
    pub occupied: bool,
    pub obj_type: KernelObjectType,
    /// Authoritative 16-bit generation counter for this object slot.
    pub generation: u16,
    /// Index into typed storage pool (CHANNEL_TABLE or SHM_TABLE).
    pub pool_index: u16,
    pub _pad: u16,
    /// Common object header.
    pub header: KernelObjectHeader,
}

const _: () = assert!(core::mem::size_of::<KernelObjectSlot>() == 40);
const _: () = assert!(core::mem::align_of::<KernelObjectSlot>() == 8);

impl KernelObjectSlot {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            obj_type: KernelObjectType::Free,
            generation: 1, // Start with generation 1
            pool_index: 0,
            _pad: 0,
            header: KernelObjectHeader::empty(),
        }
    }
}

/// Global spinlock protecting KERNEL_OBJECT_TABLE and related IPC structures.
///
/// Lock order: KERNEL_OBJECT_TABLE_LOCK < SCHEDULER.lock < CPU (IF=0)
pub struct KernelObjectTableLock {
    lock: AtomicBool,
}

impl KernelObjectTableLock {
    pub const fn new() -> Self {
        Self {
            lock: AtomicBool::new(false),
        }
    }

    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.lock.load(Ordering::Relaxed)
    }

    /// Acquires the lock, disabling interrupts and returning previous RFLAGS.
    #[inline(always)]
    pub fn acquire(&self) -> u64 {
        let rflags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                "cli",
                out(reg) rflags,
                options(nomem)
            );
        }
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        rflags
    }


    /// Releases the lock and restores previous interrupt state.
    #[inline(always)]
    pub fn unlock_restore(&self, rflags: u64) {
        self.lock.store(false, Ordering::Release);
        if (rflags & (1 << 9)) != 0 {
            unsafe {
                crate::hal::arch::x86_64::cpu::sti();
            }
        }
    }
}

pub static KERNEL_OBJECT_TABLE_LOCK: KernelObjectTableLock = KernelObjectTableLock::new();

/// Authoritative 256-slot static kernel object table in .bss (256 * 40 = 10 KiB).
pub static mut KERNEL_OBJECT_TABLE: [KernelObjectSlot; MAX_KERNEL_OBJECTS] =
    [const { KernelObjectSlot::empty() }; MAX_KERNEL_OBJECTS];

/// Monotonic atomic object-ID allocator.
pub static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);

/// Allocates a unique monotonic 64-bit Object ID with a deterministic terminal state.
pub fn allocate_object_id() -> Result<u64, IpcError> {
    let mut current = NEXT_OBJECT_ID.load(Ordering::Relaxed);
    loop {
        if current == u64::MAX {
            return Err(IpcError::ObjectIdExhausted);
        }
        let next = current + 1;
        match NEXT_OBJECT_ID.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(allocated_id) => return Ok(allocated_id),
            Err(actual) => current = actual,
        }
    }
}

/// Allocates a slot in KERNEL_OBJECT_TABLE under KERNEL_OBJECT_TABLE_LOCK.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF=0).
pub unsafe fn allocate_object_slot_locked(
    obj_type: KernelObjectType,
    pool_index: u16,
    creator_pid: u64,
) -> Result<usize, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked(), "allocate_object_slot_locked requires lock");
    let object_id = allocate_object_id()?;

    for i in 0..MAX_KERNEL_OBJECTS {
        let slot = &mut KERNEL_OBJECT_TABLE[i];
        if !slot.occupied {
            slot.occupied = true;
            slot.obj_type = obj_type;
            slot.pool_index = pool_index;
            // Generation remains monotonic across slot reuse
            slot.header = KernelObjectHeader {
                object_id,
                creator_pid,
                handle_refs: 0,
                mapping_refs: 0,
                in_flight_op_refs: 0,
                state: ObjectLifecycleState::Active,
                _reserved1: 0,
                signals: 0,
                _reserved2: 0,
            };
            return Ok(i);
        }
    }
    Err(IpcError::ObjectTableFull)
}

/// Frees an object slot in KERNEL_OBJECT_TABLE under KERNEL_OBJECT_TABLE_LOCK.
///
/// Advances slot generation to invalidate stale handles.
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF=0).
pub unsafe fn free_object_slot_locked(object_index: usize) {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked(), "free_object_slot_locked requires lock");
    assert!(object_index < MAX_KERNEL_OBJECTS);
    let slot = &mut KERNEL_OBJECT_TABLE[object_index];
    assert!(slot.occupied);
    slot.generation = slot.generation.wrapping_add(1);
    if slot.generation == 0 {
        slot.generation = 1; // Reserve generation 0
    }
    slot.occupied = false;
    slot.obj_type = KernelObjectType::Free;
    slot.header = KernelObjectHeader::empty();
}
