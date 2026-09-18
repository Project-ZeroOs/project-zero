//! Project Zero - KernelThread Execution Descriptor & Static Table (Stage 3A)
//!
//! Provides the concrete execution entity primitive, guaranteed stable-address
//! descriptor table, and forged initial cooperative activation frames.

use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager};
use crate::mm::vmm::ActivePageTable;
use super::stack::{StackAllocation, StackError};
use core::sync::atomic::{AtomicU64, Ordering};

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

/// Strongly typed numeric thread identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadId(pub u64);

/// Legacy Thread structure retained for backward compatibility with unactivated prototypes.
#[derive(Debug, Clone, Copy)]
pub struct Thread {
    pub id: ThreadId,
    pub name: &'static str,
    pub state: ThreadState,
    pub stack_memory: *mut u8,
    pub stack_size: usize,
    pub rsp: *mut u8,
}

/// Maximum number of static thread slots in Stage 3A.
pub const MAX_THREADS: usize = 16;

/// Default scheduling quantum in timer ticks (50 ms at 100 Hz).
pub const DEFAULT_QUANTUM: u32 = 5;

use crate::task::event::{Event, EventType};

/// Architectural Thread States for Stage 3.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Initializing = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
    Reclaiming = 5,
    Free = 6,
}

impl ThreadState {
    pub const Terminated: ThreadState = ThreadState::Zombie;
}

/// Deterministic Priority Classes for Stage 3.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Idle = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Discriminator identifying the type of saved context.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedFrameType {
    Cooperative = 0,
    Preemptive = 1,
}

/// Authoritative KernelThread Descriptor (176 bytes, 8-byte aligned).
#[repr(C)]
pub struct KernelThread {
    /// Saved stack pointer when thread is suspended (offset 0).
    pub saved_rsp: u64,

    /// Unique numeric thread identifier (offset 8).
    pub id: u64,

    /// Execution state of the thread (offset 16).
    pub state: ThreadState,

    /// Static priority level for deterministic round-robin (offset 17).
    pub priority: Priority,

    /// Type of frame saved at saved_rsp (offset 18).
    pub frame_type: SavedFrameType,

    /// Detached flag: true if abandoned or detached (offset 19).
    pub is_detached: bool,

    /// Latched exit status (offset 20..24).
    pub exit_code: i32,

    /// Virtual address of the top of the stack (offset 24).
    pub stack_top: u64,

    /// Virtual address of the unmapped guard page below the stack (offset 32).
    pub guard_page: u64,

    /// Physical frames allocated for stack pages (offset 40..72).
    pub stack_frames: [PhysFrame; 4],

    /// Remaining time slice in timer ticks (offset 72..76).
    pub quantum_remaining: u32,

    /// Explicit membership flag for ZOMBIE_QUEUE (offset 76).
    pub zombie_queued: bool,

    /// Reserved padding for 8-byte alignment (offset 77..80).
    pub _pad2: [u8; 3],

    /// Total ticks consumed over thread lifetime (offset 80..88).
    pub total_ticks: u64,

    /// Intrusive link for zero-allocation runqueue scheduling (offset 88..96).
    pub next_runnable: *mut KernelThread,

    /// Intrusive link for zero-allocation wait-channel blocking (offset 96..104).
    pub next_waiter: *mut KernelThread,

    /// Intrusive link for Reaper ZOMBIE_QUEUE (offset 104..112).
    pub next_zombie: *mut KernelThread,

    /// Intrusive child list head (offset 112..120).
    pub first_child: *mut KernelThread,

    /// Intrusive sibling link (offset 120..128).
    pub next_sibling: *mut KernelThread,

    /// Parent/creator thread ID (offset 128..136).
    pub parent_id: u64,

    /// Owning process identifier (offset 136..144).
    pub process_id: u64,

    /// Private lifecycle completion latch (offset 144..176).
    pub(crate) completion_event: Event,
}

// ====================================================================
// Compile-Time Layout & Offset Assertions
// ====================================================================

const _: () = assert!(core::mem::offset_of!(KernelThread, saved_rsp) == 0);
const _: () = assert!(core::mem::offset_of!(KernelThread, id) == 8);
const _: () = assert!(core::mem::offset_of!(KernelThread, state) == 16);
const _: () = assert!(core::mem::offset_of!(KernelThread, priority) == 17);
const _: () = assert!(core::mem::offset_of!(KernelThread, frame_type) == 18);
const _: () = assert!(core::mem::offset_of!(KernelThread, is_detached) == 19);
const _: () = assert!(core::mem::offset_of!(KernelThread, exit_code) == 20);
const _: () = assert!(core::mem::offset_of!(KernelThread, stack_top) == 24);
const _: () = assert!(core::mem::offset_of!(KernelThread, guard_page) == 32);
const _: () = assert!(core::mem::offset_of!(KernelThread, stack_frames) == 40);
const _: () = assert!(core::mem::offset_of!(KernelThread, quantum_remaining) == 72);
const _: () = assert!(core::mem::offset_of!(KernelThread, zombie_queued) == 76);
const _: () = assert!(core::mem::offset_of!(KernelThread, _pad2) == 77);
const _: () = assert!(core::mem::offset_of!(KernelThread, total_ticks) == 80);
const _: () = assert!(core::mem::offset_of!(KernelThread, next_runnable) == 88);
const _: () = assert!(core::mem::offset_of!(KernelThread, next_waiter) == 96);
const _: () = assert!(core::mem::offset_of!(KernelThread, next_zombie) == 104);
const _: () = assert!(core::mem::offset_of!(KernelThread, first_child) == 112);
const _: () = assert!(core::mem::offset_of!(KernelThread, next_sibling) == 120);
const _: () = assert!(core::mem::offset_of!(KernelThread, parent_id) == 128);
const _: () = assert!(core::mem::offset_of!(KernelThread, process_id) == 136);
const _: () = assert!(core::mem::offset_of!(KernelThread, completion_event) == 144);
const _: () = assert!(core::mem::size_of::<KernelThread>() == 176);
const _: () = assert!(core::mem::align_of::<KernelThread>() == 8);

/// Static occupancy slot holding an authoritative KernelThread descriptor.
pub struct ThreadSlot {
    pub occupied: bool,
    pub thread: KernelThread,
}

impl ThreadSlot {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            thread: KernelThread {
                saved_rsp: 0,
                id: 0,
                state: ThreadState::Free,
                priority: Priority::Normal,
                frame_type: SavedFrameType::Cooperative,
                is_detached: false,
                exit_code: 0,
                stack_top: 0,
                guard_page: 0,
                stack_frames: [PhysFrame(0); 4],
                quantum_remaining: DEFAULT_QUANTUM,
                zombie_queued: false,
                _pad2: [0; 3],
                total_ticks: 0,
                next_runnable: core::ptr::null_mut(),
                next_waiter: core::ptr::null_mut(),
                next_zombie: core::ptr::null_mut(),
                first_child: core::ptr::null_mut(),
                next_sibling: core::ptr::null_mut(),
                parent_id: 0,
                process_id: 0,
                completion_event: Event::new(false, EventType::ManualReset),
            },
        }
    }
}

/// Authoritative static descriptor table.
/// Guarantees that descriptor addresses remain immutable for their entire lifetime.
pub static mut THREAD_TABLE: [ThreadSlot; MAX_THREADS] = [const { ThreadSlot::empty() }; MAX_THREADS];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadError {
    TableFull,
    StackAllocationFailed(StackError),
    InvalidPointer,
    SelfJoin,
    AlreadyReclaimed,
    InvalidHandle,
}

/// Allocates an unoccupied thread slot from the static table.
/// Returns a raw pointer with a guaranteed stable virtual address.
pub fn allocate_descriptor() -> Result<*mut KernelThread, ThreadError> {
    unsafe {
        let table = &raw mut THREAD_TABLE;
        for i in 0..MAX_THREADS {
            let slot = &mut (*table)[i];
            if !slot.occupied {
                slot.occupied = true;
                slot.thread.id = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
                slot.thread.state = ThreadState::Initializing;
                slot.thread.is_detached = false;
                slot.thread.exit_code = 0;
                slot.thread.zombie_queued = false;
                slot.thread._pad2 = [0; 3];
                slot.thread.next_runnable = core::ptr::null_mut();
                slot.thread.next_waiter = core::ptr::null_mut();
                slot.thread.next_zombie = core::ptr::null_mut();
                slot.thread.first_child = core::ptr::null_mut();
                slot.thread.next_sibling = core::ptr::null_mut();
                slot.thread.parent_id = 0;
                slot.thread.process_id = 0;
                slot.thread.completion_event = Event::new(false, EventType::ManualReset);
                return Ok(&raw mut slot.thread);
            }
        }
    }
    Err(ThreadError::TableFull)
}

/// Releases a previously allocated thread slot.
pub fn free_descriptor(ptr: *mut KernelThread) -> Result<(), ThreadError> {
    unsafe {
        let table = &raw mut THREAD_TABLE;
        for i in 0..MAX_THREADS {
            let slot = &mut (*table)[i];
            if (&raw mut slot.thread) == ptr {
                slot.occupied = false;
                slot.thread.state = ThreadState::Free;
                slot.thread.id = 0;
                slot.thread.is_detached = false;
                slot.thread.zombie_queued = false;
                slot.thread.first_child = core::ptr::null_mut();
                slot.thread.next_sibling = core::ptr::null_mut();
                slot.thread.next_zombie = core::ptr::null_mut();
                return Ok(());
            }
        }
    }
    Err(ThreadError::InvalidPointer)
}

/// Finds an active thread descriptor by its unique ID.
pub fn get_thread_by_id(id: u64) -> Option<*mut KernelThread> {
    unsafe {
        let table = &raw mut THREAD_TABLE;
        for i in 0..MAX_THREADS {
            let slot = &mut (*table)[i];
            if slot.occupied && slot.thread.id == id {
                return Some(&raw mut slot.thread);
            }
        }
    }
    None
}

/// Destroys a thread's guarded stack without freeing the descriptor.
pub fn destroy_thread_stack(
    thread_ptr: *mut KernelThread,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    unsafe {
        let thread = &mut *thread_ptr;
        if thread.guard_page != 0 {
            let slot_idx = ((thread.guard_page - super::stack::STACK_ARENA_START) / super::stack::STACK_SLOT_SIZE) as usize;
            let stack = super::stack::StackAllocation {
                slot_idx,
                guard_start: thread.guard_page,
                guard_end: thread.guard_page + super::stack::STACK_GUARD_SIZE,
                stack_start: thread.guard_page + super::stack::STACK_GUARD_SIZE,
                stack_end: thread.stack_top,
                stack_top: thread.stack_top,
                frames: thread.stack_frames,
            };
            let _ = stack.destroy(pmm, vmm);
            thread.guard_page = 0;
            thread.stack_top = 0;
        }
    }
}

/// Destroys a thread's guarded stack and frees its descriptor.
pub fn destroy_thread(
    thread_ptr: *mut KernelThread,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) -> Result<(), ThreadError> {
    destroy_thread_stack(thread_ptr, pmm, vmm);
    free_descriptor(thread_ptr)
}

/// Registers the existing bootstrap execution flow as the BSP initial thread (ID 0).
pub fn create_bootstrap_thread() -> Result<*mut KernelThread, ThreadError> {
    unsafe {
        let table = &raw mut THREAD_TABLE;
        let slot = &mut (*table)[0];
        slot.occupied = true;
        slot.thread.id = 0;
        slot.thread.state = ThreadState::Running;
        slot.thread.priority = Priority::Normal;
        slot.thread.frame_type = SavedFrameType::Cooperative;
        slot.thread.is_detached = false;
        slot.thread.exit_code = 0;
        slot.thread.saved_rsp = 0;
        slot.thread.stack_top = 0;
        slot.thread.guard_page = 0;
        slot.thread.stack_frames = [PhysFrame(0); 4];
        slot.thread.quantum_remaining = DEFAULT_QUANTUM;
        slot.thread.zombie_queued = false;
        slot.thread._pad2 = [0; 3];
        slot.thread.total_ticks = 0;
        slot.thread.next_runnable = core::ptr::null_mut();
        slot.thread.next_waiter = core::ptr::null_mut();
        slot.thread.next_zombie = core::ptr::null_mut();
        slot.thread.first_child = core::ptr::null_mut();
        slot.thread.next_sibling = core::ptr::null_mut();
        slot.thread.parent_id = 0;
        slot.thread.process_id = 0;
        slot.thread.completion_event = Event::new(false, EventType::ManualReset);
        Ok(&raw mut slot.thread)
    }
}

/// Creates a new KernelThread with a dedicated 16 KiB guarded stack in the Kernel Stack Arena.
/// Forges an initial cooperative activation frame matching the exact `boot/context.asm` ABI.
///
/// Note: Stage 3A only constructs the execution object and initial stack context.
/// It does NOT insert the thread into any scheduler runqueue.
pub fn create_thread(
    entry_fn: extern "C" fn(u64),
    arg: u64,
    priority: Priority,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) -> Result<*mut KernelThread, ThreadError> {
    // 1. Allocate dedicated guarded stack from Kernel Stack Arena
    let stack = StackAllocation::allocate(pmm, vmm)
        .map_err(ThreadError::StackAllocationFailed)?;

    // 2. Allocate stable descriptor from static table
    let thread_ptr = match allocate_descriptor() {
        Ok(p) => p,
        Err(e) => {
            let _ = stack.destroy(pmm, vmm);
            return Err(e);
        }
    };

    let thread = unsafe { &mut *thread_ptr };
    thread.priority = priority;
    thread.frame_type = SavedFrameType::Cooperative;
    thread.stack_top = stack.stack_top;
    thread.guard_page = stack.guard_start;
    thread.stack_frames = stack.frames;
    thread.quantum_remaining = DEFAULT_QUANTUM;
    thread.total_ticks = 0;

    // 3. Forge initial cooperative context frame on the thread's stack.
    // Exact switch_context pop sequence:
    //   popfq        -> pops [saved_rsp + 0]  (RFLAGS)
    //   pop r15      -> pops [saved_rsp + 8]  (R15)
    //   pop r14      -> pops [saved_rsp + 16] (R14)
    //   pop r13      -> pops [saved_rsp + 24] (R13)
    //   pop r12      -> pops [saved_rsp + 32] (R12 = arg)
    //   pop rbp      -> pops [saved_rsp + 40] (RBP)
    //   pop rbx      -> pops [saved_rsp + 48] (RBX = entry_fn)
    //   ret          -> pops [saved_rsp + 56] (RIP = thread_bootstrap_entry)
    //
    // Total forged frame = 64 bytes (8 quadwords).
    // Initial saved_rsp = stack_top - 64.
    let saved_rsp = stack.stack_top - 64;
    unsafe {
        let frame = saved_rsp as *mut u64;
        *frame.add(0) = 0x0202; // RFLAGS: IF=1, reserved bit 1=1
        *frame.add(1) = 0;      // R15
        *frame.add(2) = 0;      // R14
        *frame.add(3) = 0;      // R13
        *frame.add(4) = arg;    // R12 = thread argument
        *frame.add(5) = 0;      // RBP
        *frame.add(6) = entry_fn as usize as u64; // RBX = entry function pointer
        *frame.add(7) = thread_bootstrap_entry as *const () as usize as u64; // Target RIP for 'ret'
    }

    thread.saved_rsp = saved_rsp;
    thread.state = ThreadState::Ready;

    Ok(thread_ptr)
}

/// Creates a new user thread bound to a Process, forging an initial cooperative frame
/// that transitions into Ring 3 via `user_thread_bootstrap_trampoline` and `iretq` (I-SYSCALL-11).
pub fn create_user_thread(
    process: *mut crate::task::process::Process,
    user_rip: u64,
    user_rsp: u64,
    priority: Priority,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) -> Result<*mut KernelThread, ThreadError> {
    assert!(!process.is_null(), "Cannot create thread for null process");

    // 1. Allocate dedicated guarded stack from Kernel Stack Arena (16 KiB)
    let stack = StackAllocation::allocate(pmm, vmm)
        .map_err(ThreadError::StackAllocationFailed)?;

    // 2. Allocate stable descriptor from static table
    let thread_ptr = match allocate_descriptor() {
        Ok(p) => p,
        Err(e) => {
            let _ = stack.destroy(pmm, vmm);
            return Err(e);
        }
    };

    let thread = unsafe { &mut *thread_ptr };
    thread.priority = priority;
    thread.frame_type = SavedFrameType::Cooperative;
    thread.stack_top = stack.stack_top;
    thread.guard_page = stack.guard_start;
    thread.stack_frames = stack.frames;
    thread.quantum_remaining = DEFAULT_QUANTUM;
    thread.total_ticks = 0;
    thread.process_id = unsafe { (*process).id };

    // 3. Forge initial cooperative context frame on kernel stack (64 bytes).
    let saved_rsp = stack.stack_top - 64;
    unsafe {
        let frame = saved_rsp as *mut u64;
        *frame.add(0) = 0x0202; // RFLAGS: IF=1, reserved bit 1=1
        *frame.add(1) = 0;      // R15
        *frame.add(2) = 0;      // R14
        *frame.add(3) = user_rsp; // R13
        *frame.add(4) = user_rip; // R12
        *frame.add(5) = 0;      // RBP
        *frame.add(6) = 0;      // RBX
        *frame.add(7) = crate::syscall::entry::user_thread_bootstrap_trampoline as *const () as usize as u64;
    }

    thread.saved_rsp = saved_rsp;
    thread.state = ThreadState::Ready;

    let rflags = unsafe { crate::task::scheduler::SCHEDULER.lock.acquire() };
    unsafe {
        (*process).thread_count += 1;
        crate::task::scheduler::SCHEDULER.lock.unlock_restore(rflags);
    }

    Ok(thread_ptr)
}

extern "C" {
    /// Bootstrap trampoline executed upon first cooperative activation (implemented in `boot/context.asm`).
    pub fn thread_bootstrap_entry();
}

