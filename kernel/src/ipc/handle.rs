//! Project Zero - Stage 3G Handle Management Subsystem
//!
//! Authoritative Contract: Stage 3G Architecture Rev10 (Approved & Frozen).
//!
//! Handles are 32-bit tokens composed of:
//!   - bits 0..15:  slot_index in the process's HandleTable (0..31)
//!   - bits 16..31: handle_generation (matches HandleEntry.handle_generation)

use crate::ipc::types::IpcError;
use crate::ipc::object::{KERNEL_OBJECT_TABLE, KERNEL_OBJECT_TABLE_LOCK, MAX_KERNEL_OBJECTS};

pub const MAX_HANDLES_PER_PROCESS: usize = 32;

/// Strongly typed 32-bit handle token presented by user space.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle(pub u32);

impl Handle {
    #[inline(always)]
    pub const fn new(slot_index: u16, generation: u16) -> Self {
        Self(((generation as u32) << 16) | (slot_index as u32))
    }

    #[inline(always)]
    pub const fn slot_index(&self) -> usize {
        (self.0 & 0xFFFF) as usize
    }

    #[inline(always)]
    pub const fn generation(&self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }
}

#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct HandleEntry {
    /// Index into KERNEL_OBJECT_TABLE (0..255).
    pub object_index: u16,
    /// Generation of this handle slot (for validating user Handles).
    pub handle_generation: u16,
    /// Generation of the KernelObject when this handle was created.
    pub object_generation: u16,
    /// Access rights bitmask for this handle.
    pub rights: u16,
    /// Endpoint discriminator for Channel objects (0 or 1).
    pub endpoint: u8,
    /// Occupancy flag for this handle slot.
    pub occupied: bool,
    /// Explicit padding for 8-byte alignment.
    pub _reserved: [u8; 6],
}

const _: () = assert!(core::mem::size_of::<HandleEntry>() == 16);
const _: () = assert!(core::mem::align_of::<HandleEntry>() == 8);

impl HandleEntry {
    pub const fn empty() -> Self {
        Self {
            object_index: 0,
            handle_generation: 1, // Start with generation 1
            object_generation: 0,
            rights: 0,
            endpoint: 0,
            occupied: false,
            _reserved: [0u8; 6],
        }
    }
}

#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct HandleTable {
    pub entries: [HandleEntry; MAX_HANDLES_PER_PROCESS], // 32 * 16 = 512 bytes
    pub count: u16,                                      // 2 bytes
    pub _reserved: [u8; 6],                              // 6 bytes padding for 8-byte alignment
}

const _: () = assert!(core::mem::size_of::<HandleTable>() == 520);
const _: () = assert!(core::mem::align_of::<HandleTable>() == 8);


impl HandleTable {
    pub const fn empty() -> Self {
        Self {
            entries: [const { HandleEntry::empty() }; MAX_HANDLES_PER_PROCESS],
            count: 0,
            _reserved: [0u8; 6],
        }
    }
}

/// Authoritative external handle-table registry in .bss (8 * 520 = 4,160 bytes).
pub static mut PROCESS_HANDLE_TABLES: [HandleTable; crate::task::process::MAX_PROCESSES] =
    [const { HandleTable::empty() }; crate::task::process::MAX_PROCESSES];

/// Resolves the process_slot_index for the calling thread's ProcessId.
///
/// SAFETY & SYNCHRONIZATION (Invariant I-PROC-4):
/// - Lockless read of PROCESS_TABLE is 100% race-free because the calling thread
///   is actively executing, guaranteeing that its process has thread_count >= 1.
/// - The process-table slot address and ProcessId association remain stable while
///   the calling thread is live; the slot cannot transition to Free while that thread
///   keeps the process thread_count nonzero.
/// - Complexity: Bounded scan of <= 8 entries; bounded constant-time O(1).
#[inline(always)]
pub fn resolve_current_process_slot(current_pid: u64) -> Result<usize, IpcError> {
    unsafe {
        for i in 0..crate::task::process::MAX_PROCESSES {
            let slot = &crate::task::process::PROCESS_TABLE[i];
            if slot.occupied && slot.process.id == current_pid {
                return Ok(i);
            }
        }
    }
    Err(IpcError::InvalidProcess)
}

/// Allocates a handle slot in the specified process's HandleTable under KERNEL_OBJECT_TABLE_LOCK.
///
/// Increments object's `handle_refs` count.
pub unsafe fn allocate_handle_entry_locked(
    process_slot_index: usize,
    object_index: usize,
    rights: u16,
    endpoint: u8,
) -> Result<Handle, IpcError> {
    let slot_idx = crate::cap::ops::allocate_handle_slot_locked(process_slot_index)?;
    crate::cap::ops::install_root_capability_locked(
        process_slot_index,
        slot_idx,
        object_index,
        rights,
        endpoint,
    )
}

/// Validates a user-supplied Handle for the current process under KERNEL_OBJECT_TABLE_LOCK.
///
/// Returns (object_index, endpoint, rights).
pub unsafe fn validate_handle_locked(
    process_slot_index: usize,
    handle: Handle,
    required_rights: u16,
) -> Result<(usize, u8, u16), IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked(), "validate_handle_locked requires lock");
    assert!(process_slot_index < crate::task::process::MAX_PROCESSES);

    let slot_idx = handle.slot_index();
    if slot_idx >= MAX_HANDLES_PER_PROCESS {
        return Err(IpcError::InvalidHandle);
    }

    let htable = &PROCESS_HANDLE_TABLES[process_slot_index];
    let entry = &htable.entries[slot_idx];

    if entry.handle_generation != handle.generation() {
        return Err(IpcError::BadHandleGeneration);
    }
    if !entry.occupied {
        return Err(IpcError::InvalidHandle);
    }

    let node = &crate::cap::node::CAPABILITY_NODE_TABLE[process_slot_index][slot_idx];
    if node.is_revoked {
        return Err(IpcError::CapabilityRevoked);
    }

    let obj_idx = entry.object_index as usize;
    if obj_idx >= MAX_KERNEL_OBJECTS {
        return Err(IpcError::InvalidHandle);
    }

    let obj_slot = &KERNEL_OBJECT_TABLE[obj_idx];
    if !obj_slot.occupied || obj_slot.generation != entry.object_generation {
        return Err(IpcError::BadHandleGeneration);
    }

    if (entry.rights & required_rights) != required_rights {
        return Err(IpcError::PermissionDenied);
    }

    Ok((obj_idx, entry.endpoint, entry.rights))
}

/// Closes a handle in the specified process's HandleTable under KERNEL_OBJECT_TABLE_LOCK.
///
/// Advances handle_generation and decrements object's `handle_refs`.
/// Returns (object_index, endpoint).
pub unsafe fn close_handle_locked(
    process_slot_index: usize,
    handle: Handle,
) -> Result<(usize, u8), IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked(), "close_handle_locked requires lock");
    assert!(process_slot_index < crate::task::process::MAX_PROCESSES);

    let slot_idx = handle.slot_index();
    if slot_idx >= MAX_HANDLES_PER_PROCESS {
        return Err(IpcError::InvalidHandle);
    }

    let htable = &PROCESS_HANDLE_TABLES[process_slot_index];
    let entry = &htable.entries[slot_idx];

    if entry.handle_generation != handle.generation() {
        return Err(IpcError::BadHandleGeneration);
    }
    if !entry.occupied {
        return Err(IpcError::InvalidHandle);
    }

    let pmm = &mut *(&raw mut crate::mm::pmm::PMM);
    crate::cap::ops::capability_close_locked(process_slot_index, slot_idx, pmm)
}
