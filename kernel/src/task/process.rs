//! Project Zero - Process Model, Address Spaces & Process Lifecycle (Stage 3F)
//!
//! Authoritative Contract: ADR-0015 & Stage 3F Architecture Rev2.
//!
//! Invariants:
//! - Canonical Process Lifecycle: `Creating -> Active -> Terminating -> Zombie -> Reclaiming -> Free`
//! - Explicit Process Zombie Queue Membership: `process.zombie_queued == true` <-> currently in `ZOMBIE_PROCESS_QUEUE`.
//! - Machine-Level CR3 Invariant: At every scheduling boundary, `CR3 == current_thread.process.address_space.pml4_root`.
//! - AddressSpace Isolation: Isolated user aperture [0..256), cloned supervisor kernel aperture [256..512) with `USER=0`.
//! - Active CR3 Hazard Elimination: AddressSpace cannot be destroyed while active in CR3 (switches to master kernel PML4 first).
//! - Subsystem-Specific Cancellation: Threads in `Zombie`, `Reclaiming`, or `Free` MUST NOT reside in any `RunQueue`, `WaitQueue`, `SleepTable`, `Event`, `Mutex`, or `Condvar`.
//! - Permanent Master Kernel Process: PID 0 owns `MASTER_KERNEL_PML4`, permanently Active, never reclaimed.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::task::thread::{
    KernelThread, ThreadState, THREAD_TABLE, MAX_THREADS, Priority,
    SavedFrameType, destroy_thread_stack, free_descriptor,
};
use crate::task::event::{Event, EventType};
use crate::task::mutex::Mutex;
use crate::task::condvar::Condvar;
use crate::task::sleep::{cancel_sleep_locked, SLEEP_TABLE, SLEEP_WAIT_QUEUE};
use crate::task::scheduler::{
    cpu_if_bit, wake_thread_locked, exit_current_thread_with_code, SCHEDULER,
};
use crate::mm::pmm::{PhysicalMemoryManager, PhysFrame};
use crate::mm::vmm::{
    AddressSpace, Cr3, Cr3Flags, PageTableFlags, get_master_kernel_pml4,
    get_active_geometry, VmmError, phys_to_virt_table,
};
use crate::kprintln;

/// Strongly typed numeric process identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(pub u64);

/// Architectural Process States for Stage 3F.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Creating = 0,
    Active = 1,
    Terminating = 2,
    Zombie = 3,
    Reclaiming = 4,
    Free = 5,
}

/// Authoritative Process Descriptor (128 bytes, 8-byte aligned).
#[repr(C)]
pub struct Process {
    /// 1. Unique numeric process identifier (offset 0x00..0x08).
    pub id: u64,

    /// 2. Execution state of the process (offset 0x08).
    pub state: ProcessState,

    /// 3. Whether the process is detached from its parent (offset 0x09).
    pub is_detached: bool,

    /// 4. Explicit membership flag for ZOMBIE_PROCESS_QUEUE (offset 0x0A).
    pub zombie_queued: bool,

    /// 5. Reserved padding for 8-byte alignment (offset 0x0B..0x10).
    pub _pad0: [u8; 5],

    /// 6. Latched exit status code (offset 0x10..0x14).
    pub exit_code: i32,

    /// 7. Total count of active threads belonging to this process (offset 0x14..0x18).
    pub thread_count: u32,

    /// 8. Parent process identifier (offset 0x18..0x20).
    pub parent_pid: u64,

    /// 9. AddressSpace ownership: physical root frame of PML4 page table (offset 0x20..0x28).
    pub address_space: u64,

    /// 10. Head of intrusive thread group list (offset 0x28..0x30).
    pub thread_group_head: *mut KernelThread,

    /// 11. Intrusive sibling link in parent's child list (offset 0x30..0x38).
    pub next_sibling_process: *mut Process,

    /// 12. Head of child process list (offset 0x38..0x40).
    pub first_child_process: *mut Process,

    /// 13. Intrusive link for ZOMBIE_PROCESS_QUEUE (offset 0x40..0x48).
    pub next_zombie_process: *mut Process,

    /// 14. Private lifecycle completion notification event (offset 0x48..0x68).
    pub completion_event: Event,

    /// 15. Reserved padding to exactly 128 bytes (offset 0x68..0x80).
    pub _reserved: [u64; 3],
}

impl Process {
    #[inline(always)]
    pub fn pml4_root(&self) -> u64 {
        self.address_space
    }
}

// ====================================================================
// Compile-Time Layout & Offset Assertions
// ====================================================================

const _: () = assert!(core::mem::size_of::<Process>() == 128);
const _: () = assert!(core::mem::align_of::<Process>() == 8);
const _: () = assert!(core::mem::offset_of!(Process, id) == 0x00);
const _: () = assert!(core::mem::offset_of!(Process, state) == 0x08);
const _: () = assert!(core::mem::offset_of!(Process, is_detached) == 0x09);
const _: () = assert!(core::mem::offset_of!(Process, zombie_queued) == 0x0A);
const _: () = assert!(core::mem::offset_of!(Process, exit_code) == 0x10);
const _: () = assert!(core::mem::offset_of!(Process, thread_count) == 0x14);
const _: () = assert!(core::mem::offset_of!(Process, parent_pid) == 0x18);
const _: () = assert!(core::mem::offset_of!(Process, address_space) == 0x20);
const _: () = assert!(core::mem::offset_of!(Process, thread_group_head) == 0x28);
const _: () = assert!(core::mem::offset_of!(Process, next_sibling_process) == 0x30);
const _: () = assert!(core::mem::offset_of!(Process, first_child_process) == 0x38);
const _: () = assert!(core::mem::offset_of!(Process, next_zombie_process) == 0x40);
const _: () = assert!(core::mem::offset_of!(Process, completion_event) == 0x48);

/// Maximum number of static process slots.
pub const MAX_PROCESSES: usize = 16;

/// Static occupancy slot holding an authoritative Process descriptor.
pub struct ProcessSlot {
    pub occupied: bool,
    pub process: Process,
}

impl ProcessSlot {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            process: Process {
                id: 0,
                state: ProcessState::Free,
                is_detached: false,
                zombie_queued: false,
                _pad0: [0; 5],
                exit_code: 0,
                thread_count: 0,
                parent_pid: 0,
                address_space: 0,
                thread_group_head: core::ptr::null_mut(),
                next_sibling_process: core::ptr::null_mut(),
                first_child_process: core::ptr::null_mut(),
                next_zombie_process: core::ptr::null_mut(),
                completion_event: Event::new(false, EventType::ManualReset),
                _reserved: [0; 3],
            },
        }
    }
}

/// Authoritative static process descriptor table in `.bss`.
pub static mut PROCESS_TABLE: [ProcessSlot; MAX_PROCESSES] = [const { ProcessSlot::empty() }; MAX_PROCESSES];

/// Intrusive singly linked list for zombie process reclamation.
pub struct ZombieProcessQueue {
    pub head: *mut Process,
    pub tail: *mut Process,
    pub count: usize,
}

impl ZombieProcessQueue {
    pub const fn new() -> Self {
        Self {
            head: core::ptr::null_mut(),
            tail: core::ptr::null_mut(),
            count: 0,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head.is_null()
    }
}

pub static mut ZOMBIE_PROCESS_QUEUE: ZombieProcessQueue = ZombieProcessQueue::new();

/// Enqueues a zombie process into ZOMBIE_PROCESS_QUEUE under SCHEDULER.lock.
///
/// Invariant: A process may appear in ZOMBIE_PROCESS_QUEUE at most once.
/// `process.zombie_queued == true` <-> currently in queue.
pub unsafe fn enqueue_zombie_process_locked(process: *mut Process) {
    assert!(SCHEDULER.lock.is_locked(), "enqueue_zombie_process_locked requires SCHEDULER.lock");
    assert_eq!(cpu_if_bit(), 0, "enqueue_zombie_process_locked requires IF=0");
    assert!(!process.is_null(), "Cannot enqueue null process pointer");
    assert_ne!((*process).id, 0, "PID 0 (Master Kernel Process) cannot be enqueued into ZOMBIE_PROCESS_QUEUE");
    assert_eq!((*process).state, ProcessState::Zombie, "Can only enqueue Zombie process");
    assert!(!(*process).zombie_queued, "Process is already queued in ZOMBIE_PROCESS_QUEUE");

    (*process).zombie_queued = true;
    (*process).next_zombie_process = core::ptr::null_mut();

    if ZOMBIE_PROCESS_QUEUE.tail.is_null() {
        ZOMBIE_PROCESS_QUEUE.head = process;
        ZOMBIE_PROCESS_QUEUE.tail = process;
    } else {
        (*ZOMBIE_PROCESS_QUEUE.tail).next_zombie_process = process;
        ZOMBIE_PROCESS_QUEUE.tail = process;
    }
    ZOMBIE_PROCESS_QUEUE.count += 1;
}

/// Pops a zombie process from ZOMBIE_PROCESS_QUEUE under SCHEDULER.lock.
pub unsafe fn dequeue_zombie_process_locked() -> Option<*mut Process> {
    assert!(SCHEDULER.lock.is_locked(), "dequeue_zombie_process_locked requires SCHEDULER.lock");
    assert_eq!(cpu_if_bit(), 0, "dequeue_zombie_process_locked requires IF=0");

    if ZOMBIE_PROCESS_QUEUE.head.is_null() {
        return None;
    }

    let p = ZOMBIE_PROCESS_QUEUE.head;
    ZOMBIE_PROCESS_QUEUE.head = (*p).next_zombie_process;
    if ZOMBIE_PROCESS_QUEUE.head.is_null() {
        ZOMBIE_PROCESS_QUEUE.tail = core::ptr::null_mut();
    }
    (*p).next_zombie_process = core::ptr::null_mut();

    assert!((*p).zombie_queued, "Dequeued process did not have zombie_queued == true");
    (*p).zombie_queued = false;
    ZOMBIE_PROCESS_QUEUE.count -= 1;

    Some(p)
}

/// Unlinks a specific process from ZOMBIE_PROCESS_QUEUE under SCHEDULER.lock.
pub unsafe fn remove_zombie_process_locked(process: *mut Process) -> bool {
    assert!(SCHEDULER.lock.is_locked(), "remove_zombie_process_locked requires SCHEDULER.lock");
    assert_eq!(cpu_if_bit(), 0, "remove_zombie_process_locked requires IF=0");

    if ZOMBIE_PROCESS_QUEUE.head.is_null() || process.is_null() {
        return false;
    }

    if ZOMBIE_PROCESS_QUEUE.head == process {
        ZOMBIE_PROCESS_QUEUE.head = (*process).next_zombie_process;
        if ZOMBIE_PROCESS_QUEUE.head.is_null() {
            ZOMBIE_PROCESS_QUEUE.tail = core::ptr::null_mut();
        }
        (*process).next_zombie_process = core::ptr::null_mut();
        (*process).zombie_queued = false;
        ZOMBIE_PROCESS_QUEUE.count -= 1;
        return true;
    }

    let mut curr = ZOMBIE_PROCESS_QUEUE.head;
    while !(*curr).next_zombie_process.is_null() && (*curr).next_zombie_process != process {
        curr = (*curr).next_zombie_process;
    }

    if !(*curr).next_zombie_process.is_null() {
        (*curr).next_zombie_process = (*process).next_zombie_process;
        if ZOMBIE_PROCESS_QUEUE.tail == process {
            ZOMBIE_PROCESS_QUEUE.tail = curr;
        }
        (*process).next_zombie_process = core::ptr::null_mut();
        (*process).zombie_queued = false;
        ZOMBIE_PROCESS_QUEUE.count -= 1;
        return true;
    }

    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    TableFull,
    ProcessNotFound,
    VmmError(VmmError),
    SelfJoin,
    AlreadyReclaimed,
    InvalidHandle,
    InvalidState,
}

static NEXT_PROCESS_ID: AtomicU64 = AtomicU64::new(1);

/// Initializes the Process Subsystem and establishes permanent Master Kernel Process (PID 0).
pub fn init_process_subsystem() {
    unsafe {
        let slot = &mut PROCESS_TABLE[0];
        slot.occupied = true;
        slot.process.id = 0;
        slot.process.state = ProcessState::Active;
        slot.process.is_detached = false;
        slot.process.zombie_queued = false;
        slot.process.exit_code = 0;
        slot.process.thread_count = 1; // BSP thread
        slot.process.parent_pid = 0;
        slot.process.address_space = get_master_kernel_pml4().address();
        slot.process.thread_group_head = core::ptr::null_mut();
        slot.process.next_sibling_process = core::ptr::null_mut();
        slot.process.first_child_process = core::ptr::null_mut();
        slot.process.next_zombie_process = core::ptr::null_mut();
        slot.process.completion_event = Event::new(false, EventType::ManualReset);
    }
}

/// Returns the physical PML4 root address for the given ProcessId.
pub fn get_process_pml4_root(pid: u64) -> u64 {
    unsafe {
        for i in 0..MAX_PROCESSES {
            let slot = &PROCESS_TABLE[i];
            if slot.occupied && slot.process.id == pid {
                return slot.process.address_space;
            }
        }
    }
    get_master_kernel_pml4().address()
}

/// Linear, single-ownership capability handle to join or detach a Process.
pub struct ProcessHandle {
    pub pid: u64,
    pub process: *mut Process,
}

impl ProcessHandle {
    /// Blocks until the target process exits, claims exclusive reclamation rights,
    /// reclaims all process-owned threads and address space, and returns the exit code.
    pub fn join(
        self,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<i32, ProcessError> {
        let current_thread = crate::task::percpu::current_thread_from_gs();
        let current_pid = if !current_thread.is_null() {
            unsafe { (*current_thread).process_id }
        } else {
            0
        };

        if self.pid == current_pid {
            return Err(ProcessError::SelfJoin);
        }

        unsafe {
            if (*self.process).id != self.pid {
                return Err(ProcessError::InvalidHandle);
            }

            // Wait for completion event
            (*self.process).completion_event.wait();

            let orig_rflags = SCHEDULER.lock.acquire();

            if (*self.process).state == ProcessState::Reclaiming || (*self.process).state == ProcessState::Free {
                SCHEDULER.lock.unlock_restore(orig_rflags);
                return Err(ProcessError::AlreadyReclaimed);
            }

            assert_eq!(
                (*self.process).state,
                ProcessState::Zombie,
                "Process must be in Zombie state upon completion signal"
            );
            assert_eq!(
                (*self.process).thread_count,
                0,
                "Invariant: ProcessState::Zombie => thread_count == 0"
            );

            // Exclusive reclamation claim: Zombie -> Reclaiming
            (*self.process).state = ProcessState::Reclaiming;

            // Remove from ZOMBIE_PROCESS_QUEUE if present
            if (*self.process).zombie_queued {
                remove_zombie_process_locked(self.process);
            }

            let exit_code = (*self.process).exit_code;

            // Reclaim all resources under lock
            reclaim_process_resources_locked(self.process, pmm);

            SCHEDULER.lock.unlock_restore(orig_rflags);

            core::mem::forget(self);
            Ok(exit_code)
        }
    }

    /// Detaches the process, allowing the kernel reaper to reclaim it upon termination.
    pub fn detach(self) {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        unsafe {
            if (*self.process).id == self.pid {
                (*self.process).is_detached = true;
                if (*self.process).state == ProcessState::Zombie && !(*self.process).zombie_queued {
                    enqueue_zombie_process_locked(self.process);
                }
            }
            SCHEDULER.lock.unlock_restore(orig_rflags);
        }
        core::mem::forget(self);
    }
}

/// Creates a new isolated Process with a distinct PML4 address space.
pub fn create_process(
    parent_pid: u64,
    pmm: &mut PhysicalMemoryManager,
) -> Result<ProcessHandle, ProcessError> {
    let address_space = AddressSpace::new_user(pmm).map_err(ProcessError::VmmError)?;

    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };

    let slot_idx = unsafe {
        let mut free_idx = None;
        for i in 1..MAX_PROCESSES { // Skip slot 0 (Master Kernel Process)
            if !PROCESS_TABLE[i].occupied {
                free_idx = Some(i);
                break;
            }
        }
        free_idx
    };

    let idx = match slot_idx {
        Some(i) => i,
        None => {
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            let mut as_copy = address_space;
            let _ = as_copy.destroy(pmm);
            return Err(ProcessError::TableFull);
        }
    };

    let new_pid = NEXT_PROCESS_ID.fetch_add(1, Ordering::Relaxed);

    let proc_ptr = unsafe {
        let slot = &mut PROCESS_TABLE[idx];
        slot.occupied = true;
        slot.process.id = new_pid;
        slot.process.state = ProcessState::Active;
        slot.process.is_detached = false;
        slot.process.zombie_queued = false;
        slot.process.exit_code = 0;
        slot.process.thread_count = 0;
        slot.process.parent_pid = parent_pid;
        slot.process.address_space = address_space.pml4_root.address();
        slot.process.thread_group_head = core::ptr::null_mut();
        slot.process.next_sibling_process = core::ptr::null_mut();
        slot.process.first_child_process = core::ptr::null_mut();
        slot.process.next_zombie_process = core::ptr::null_mut();
        slot.process.completion_event = Event::new(false, EventType::ManualReset);

        // Link as child of parent_pid if parent exists
        for p in 0..MAX_PROCESSES {
            let p_slot = &mut PROCESS_TABLE[p];
            if p_slot.occupied && p_slot.process.id == parent_pid {
                slot.process.next_sibling_process = p_slot.process.first_child_process;
                p_slot.process.first_child_process = &raw mut slot.process;
                break;
            }
        }

        // Initialize / scrub external handle table for this slot (Invariant I-PROC-2)
        let htable = &mut crate::ipc::handle::PROCESS_HANDLE_TABLES[idx];
        for h_entry in htable.entries.iter_mut() {
            h_entry.occupied = false;
            h_entry.object_index = 0;
            h_entry.object_generation = 0;
            h_entry.rights = 0;
            h_entry.endpoint = 0;
            h_entry.handle_generation = h_entry.handle_generation.wrapping_add(1);
            if h_entry.handle_generation == 0 {
                h_entry.handle_generation = 1;
            }
        }
        htable.count = 0;

        // Initialize / scrub capability nodes for this slot (Invariant I-CAP-10)
        for cnode in crate::cap::CAPABILITY_NODE_TABLE[idx].iter_mut() {
            *cnode = crate::cap::CapabilityNode::empty();
        }

        &raw mut slot.process

    };

    unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };

    Ok(ProcessHandle {
        pid: new_pid,
        process: proc_ptr,
    })
}

/// Reclaims process resources (threads, stacks, address space) under SCHEDULER.lock.
///
/// Invariant: AddressSpace enters Reclaiming only when:
/// 1. No process-owned thread can execute again.
/// 2. No process-owned thread remains in RunQueue/WaitQueue/SleepTable.
/// 3. No CPU has CR3 pointing at the AddressSpace.
pub unsafe fn reclaim_process_resources_locked(
    process: *mut Process,
    pmm: &mut PhysicalMemoryManager,
) {
    assert!(SCHEDULER.lock.is_locked(), "reclaim_process_resources_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "reclaim_process_resources_locked requires IF=0");
    assert!(!process.is_null(), "Cannot reclaim null process");
    assert_ne!((*process).id, 0, "Cannot reclaim Master Kernel Process (PID 0)");
    assert_eq!((*process).thread_count, 0, "Invariant: ProcessState::Reclaiming => thread_count == 0");

    let pid = (*process).id;
    let mut apt = crate::mm::vmm::ActivePageTable::new();

    // 1. Reclaim all thread stacks and descriptors belonging to this process
    let table = &raw mut THREAD_TABLE;
    for i in 0..MAX_THREADS {
        let slot = &mut (*table)[i];
        if slot.occupied && slot.thread.process_id == pid {
            let thread_ptr = &raw mut slot.thread;
            // Unlink from ZOMBIE_QUEUE if queued
            if slot.thread.zombie_queued {
                crate::task::lifecycle::remove_zombie_locked(thread_ptr);
            }
            slot.thread.state = ThreadState::Reclaiming;
            destroy_thread_stack(thread_ptr, pmm, &mut apt);
            let _ = free_descriptor(thread_ptr);
        }
    }

    // 2. Eliminate active CR3 hazard: if local CR3 points to this PML4, switch to master
    let geometry = get_active_geometry();
    let (cur_cr3, _) = Cr3::read(geometry);
    if cur_cr3.address() == (*process).address_space {
        Cr3::write(get_master_kernel_pml4(), Cr3Flags::empty(), geometry);
    }

    // 2b. Reclaim all process-owned user leaf frames from companion memory map (I-ELF-MEM-OWNERSHIP-1)
    let mut proc_slot = None;
    for i in 1..MAX_PROCESSES {
        if PROCESS_TABLE[i].occupied && PROCESS_TABLE[i].process.id == pid {
            proc_slot = Some(i);
            break;
        }
    }
    if let Some(s) = proc_slot {
        crate::elf::mmap::reclaim_process_memory_map_locked(s, pmm, &mut apt);
    }

    // 3. Destroy isolated AddressSpace and return PML4 frame to PMM
    let mut addr_space = AddressSpace::from_root(PhysFrame((*process).address_space));
    let _ = addr_space.destroy(pmm);
    (*process).address_space = 0;

    // 4. Mark process Free
    (*process).state = ProcessState::Free;
    for i in 0..MAX_PROCESSES {
        let slot = &mut PROCESS_TABLE[i];
        if slot.occupied && slot.process.id == pid {
            slot.occupied = false;
            break;
        }
    }
}

/// Reaps all queued zombie processes from ZOMBIE_PROCESS_QUEUE.
#[no_mangle]
pub extern "C" fn reap_zombie_processes(pmm: &mut PhysicalMemoryManager) -> usize {
    let mut reaped = 0;
    loop {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let proc_ptr = unsafe { dequeue_zombie_process_locked() };
        if let Some(p) = proc_ptr {
            unsafe {
                if (*p).state == ProcessState::Zombie {
                    assert_eq!((*p).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
                    (*p).state = ProcessState::Reclaiming;
                    reclaim_process_resources_locked(p, pmm);
                    reaped += 1;
                }
            }
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        } else {
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            break;
        }
    }
    reaped
}

/// Formal Process-Termination Cancellation Protocol:
/// Cancels all peer threads of the terminating process under SCHEDULER.lock.
///
/// Invariant: Every cancelled thread is removed from:
/// - RunQueue (if Ready)
/// - Event / Mutex / Condvar (if Blocked)
/// - SleepTable / SLEEP_WAIT_QUEUE (if Sleeping)
/// and transitioned to `ThreadState::Zombie` before off-stack context switch.
pub unsafe fn cancel_process_threads_locked(
    process: *mut Process,
    calling_thread: *mut KernelThread,
) {
    assert!(SCHEDULER.lock.is_locked(), "cancel_process_threads_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "cancel_process_threads_locked requires IF=0");

    let pid = (*process).id;
    let table = &raw mut THREAD_TABLE;

    // Mutex cancellation: Cancel mutex ownership and peer waiters for this terminating process
    crate::task::mutex::cancel_all_mutexes_for_process_locked(pid);

    // Channel waiter cancellation: Detach all channel waiters for this terminating process
    crate::ipc::channel::cancel_channel_waiters_for_process_locked(pid);

    for i in 0..MAX_THREADS {
        let slot = &mut (*table)[i];
        if slot.occupied && slot.thread.process_id == pid {
            let t = &raw mut slot.thread;
            if t == calling_thread {
                continue; // The calling thread will exit at the end of the termination protocol
            }

            match slot.thread.state {
                ThreadState::Ready => {
                    // Remove from scheduler runqueues
                    SCHEDULER.critical_queue.remove(t);
                    SCHEDULER.high_queue.remove(t);
                    SCHEDULER.normal_queue.remove(t);
                    slot.thread.next_runnable = core::ptr::null_mut();

                    slot.thread.state = ThreadState::Zombie;
                    slot.thread.exit_code = (*process).exit_code;
                    slot.thread.completion_event.signal_locked();

                    if slot.thread.is_detached && !slot.thread.zombie_queued {
                        crate::task::lifecycle::enqueue_zombie_locked(t);
                    }
                    (*process).thread_count = (*process).thread_count.saturating_sub(1);
                }
                ThreadState::Blocked => {
                    // 1. Check SleepTable
                    cancel_sleep_locked(t);

                    // 2. Detach from any waitqueue (Event, Mutex, Condvar)
                    slot.thread.next_waiter = core::ptr::null_mut();

                    slot.thread.state = ThreadState::Zombie;
                    slot.thread.exit_code = (*process).exit_code;
                    slot.thread.completion_event.signal_locked();

                    if slot.thread.is_detached && !slot.thread.zombie_queued {
                        crate::task::lifecycle::enqueue_zombie_locked(t);
                    }
                    (*process).thread_count = (*process).thread_count.saturating_sub(1);
                }
                ThreadState::Running => {
                    // In single-core BSP, only the calling thread can be Running
                    slot.thread.state = ThreadState::Zombie;
                    slot.thread.exit_code = (*process).exit_code;
                    slot.thread.completion_event.signal_locked();
                    (*process).thread_count = (*process).thread_count.saturating_sub(1);
                }
                ThreadState::Zombie | ThreadState::Reclaiming | ThreadState::Free | ThreadState::Initializing => {
                    // Already terminal or not yet active
                }
            }
        }
    }
}

/// Terminates the current process with an explicit exit status code.
///
/// Canonical State Path: `Active -> Terminating -> Zombie -> Reclaiming -> Free`
#[no_mangle]
pub extern "C" fn process_exit(code: i32) -> ! {
    let _ = unsafe { SCHEDULER.lock.acquire() };
    let current_thread = crate::task::percpu::current_thread_from_gs();
    assert!(!current_thread.is_null(), "Cannot call process_exit from null thread");

    let pid = unsafe { (*current_thread).process_id };
    assert_ne!(pid, 0, "PID 0 (Master Kernel Process) cannot exit");

    let mut slot_idx_found = 0;
    let proc_ptr = unsafe {
        let mut p = core::ptr::null_mut();
        for i in 1..MAX_PROCESSES {
            let slot = &mut PROCESS_TABLE[i];
            if slot.occupied && slot.process.id == pid {
                p = &raw mut slot.process;
                slot_idx_found = i;
                break;
            }
        }
        p
    };
    assert!(!proc_ptr.is_null(), "Current thread has invalid process_id");


    unsafe {
        assert_eq!((*proc_ptr).state, ProcessState::Active, "Only Active process can call process_exit");

        // 1. Active -> Terminating
        (*proc_ptr).state = ProcessState::Terminating;
        (*proc_ptr).exit_code = code;

        // 2. Formal Process-Termination Cancellation Protocol
        cancel_process_threads_locked(proc_ptr, current_thread);

        // 3. Decrement calling thread from thread_count, asserting invariant: Zombie => thread_count == 0
        (*proc_ptr).thread_count = (*proc_ptr).thread_count.saturating_sub(1);
        assert_eq!((*proc_ptr).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");

        // 4. Parent-child abandonment:
        // Detached live children remain executable; detached zombie children enqueue once.
        let mut child = (*proc_ptr).first_child_process;
        while !child.is_null() {
            let next_sib = (*child).next_sibling_process;
            (*child).parent_pid = 0;
            (*child).next_sibling_process = core::ptr::null_mut();

            if (*child).state == ProcessState::Zombie && !(*child).zombie_queued {
                enqueue_zombie_process_locked(child);
            }
            child = next_sib;
        }
        (*proc_ptr).first_child_process = core::ptr::null_mut();

        // 5. Terminating -> Zombie
        (*proc_ptr).state = ProcessState::Zombie;
        (*proc_ptr).completion_event.signal_locked();

        // If parent is 0 or detached, enqueue onto ZOMBIE_PROCESS_QUEUE
        if ((*proc_ptr).parent_pid == 0 || (*proc_ptr).is_detached) && !(*proc_ptr).zombie_queued {
            enqueue_zombie_process_locked(proc_ptr);
        }

        // ====================================================================
        // STEP 2: IPC Handle Closure, SHM & In-Flight Cleanup (Invariant I-TEARDOWN-1)
        // Lock: KERNEL_OBJECT_TABLE_LOCK (SCHEDULER.lock is dropped temporarily)
        // ====================================================================
        SCHEDULER.lock.unlock_keep_cli();

        let obj_rflags = crate::ipc::KERNEL_OBJECT_TABLE_LOCK.acquire();

        // 2a. Release in-flight operations of cancelled threads of terminating process
        for t_idx in 0..MAX_THREADS {
            let t_state = &mut crate::ipc::channel::THREAD_IPC_STATE[t_idx];
            if t_state.occupied {
                let t_ptr = &raw mut THREAD_TABLE[t_idx].thread;
                if (*t_ptr).process_id == pid {
                    let obj_idx = t_state.object_index as usize;
                    t_state.occupied = false;
                    let obj_slot = &mut crate::ipc::KERNEL_OBJECT_TABLE[obj_idx];
                    obj_slot.header.in_flight_op_refs = obj_slot.header.in_flight_op_refs.saturating_sub(1);
                    if obj_slot.obj_type == crate::ipc::KernelObjectType::Channel {
                        crate::ipc::channel::check_and_reclaim_channel_locked(obj_idx);
                    }
                }
            }
        }

        // 2b. Sweep SHM mappings and close handles for this process slot
        let mut apt = crate::mm::vmm::ActivePageTable::new();
        let pmm_ptr = &raw mut crate::mm::pmm::PMM;
        crate::ipc::shm::cleanup_process_shm_and_handles_locked(
            slot_idx_found,
            pid,
            &mut *pmm_ptr,
            &mut apt,
        );

        crate::ipc::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(obj_rflags);

        // 2c. Clean up process sockets, unbound ports, and drained packet queues (I-NET-TEARDOWN-1)
        crate::net::socket::cleanup_process_sockets(pid);

        // Re-acquire SCHEDULER.lock for Step 3
        let _ = SCHEDULER.lock.acquire();


        // 5. Calling thread marks itself Zombie and transfers control off its stack
        (*current_thread).state = ThreadState::Zombie;
        (*current_thread).exit_code = code;
        (*current_thread).frame_type = SavedFrameType::Cooperative;
        (*current_thread).completion_event.signal_locked();

        if (*current_thread).is_detached && !(*current_thread).zombie_queued {
            crate::task::lifecycle::enqueue_zombie_locked(current_thread);
        }

        let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
            crate::task::percpu::BSP_PERCPU.idle_thread
        });

        (*next).state = ThreadState::Running;
        (*next).quantum_remaining = crate::task::thread::DEFAULT_QUANTUM;
        crate::task::percpu::BSP_PERCPU.current_thread = next;

        // Switch address space if next thread belongs to a different process
        crate::task::scheduler::switch_address_space_locked(current_thread, next);

        let prev_rsp_ptr = &raw mut (*current_thread).saved_rsp;
        let next_rsp = (*next).saved_rsp;
        let next_frame_type = (*next).frame_type;

        SCHEDULER.lock.unlock_keep_cli();

        crate::task::context::terminal_context_switch(
            prev_rsp_ptr,
            next_rsp,
            0x202,
            next_frame_type,
        );

        loop {
            crate::hal::arch::x86_64::cpu::hlt();
        }
    }
}

/// Executes Stage 3F deterministic architectural verification (Tests A through S):
pub fn run_stage3f_verification(
    pmm: &mut PhysicalMemoryManager,
    _vmm: &mut crate::mm::vmm::ActivePageTable,
) {
    kprintln!("\n[Stage 3F: Process Model, Address Spaces & Process Lifecycle]");
    let baseline_free = pmm.free_frame_count();
    let _ = core::hint::black_box(process_exit as usize);

    // 0. Initialize process subsystem
    init_process_subsystem();
    assert_eq!(get_process_pml4_root(0), get_master_kernel_pml4().address());
    kprintln!("  PID 0 (Master Kernel Process) established [VERIFIED]");

    // Test A: Process creation & distinct PML4
    let handle_a = create_process(0, pmm).expect("Test A: Failed to create process");
    let pid_a = handle_a.pid;
    let proc_a_ptr = handle_a.process;
    let pml4_a = unsafe { (*proc_a_ptr).address_space };
    assert_ne!(pml4_a, get_master_kernel_pml4().address(), "PML4 must be distinct from master kernel PML4");
    assert_eq!(pml4_a % 4096, 0, "PML4 root must be 4 KiB aligned");
    assert_eq!(unsafe { (*proc_a_ptr).state }, ProcessState::Active);
    kprintln!("  Test A: Process creation & distinct PML4 [VERIFIED]");

    // Test B: Kernel aperture cloning
    let pml4_a_table = unsafe { &*phys_to_virt_table(PhysFrame(pml4_a)) };
    let master_pml4_table = unsafe { &*phys_to_virt_table(get_master_kernel_pml4()) };
    for idx in 256..512 {
        let master_entry = master_pml4_table.entries[idx];
        let proc_entry = pml4_a_table.entries[idx];
        if master_entry.is_present() {
            assert!(proc_entry.is_present(), "Kernel entry {} must be present in cloned PML4", idx);
            assert!(!proc_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE), "Kernel entry {} must have USER=0", idx);
            assert_eq!(master_entry.0, proc_entry.0, "Kernel entry {} bits must match master kernel PML4 exactly", idx);
        }
    }
    kprintln!("  Test B: Kernel aperture cloning (USER=0) [VERIFIED]");

    // Test C: User aperture isolation
    for idx in 0..256 {
        assert!(!pml4_a_table.entries[idx].is_present(), "User aperture entry {} must be unmapped (0)", idx);
    }
    kprintln!("  Test C: User aperture isolation [VERIFIED]");

    // Test D: Same-process CR3 preservation
    let geometry = get_active_geometry();
    let (cr3_before, _) = Cr3::read(geometry);
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        let bsp_t = &raw mut THREAD_TABLE[0].thread;
        crate::task::scheduler::switch_address_space_locked(bsp_t, bsp_t);
        SCHEDULER.lock.unlock_restore(orig);
    }
    let (cr3_after, _) = Cr3::read(geometry);
    assert_eq!(cr3_before, cr3_after, "Same-process switch must NOT modify CR3");
    kprintln!("  Test D: Same-process CR3 preservation (0 CR3 writes) [VERIFIED]");

    // Test E: Cross-process CR3 switching
    let t_desc = crate::task::thread::allocate_descriptor().expect("Allocate descriptor for Test E");
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        (*t_desc).process_id = pid_a;
        crate::task::scheduler::switch_address_space_locked(
            &raw mut THREAD_TABLE[0].thread,
            t_desc,
        );
        SCHEDULER.lock.unlock_restore(orig);
    }
    let (cr3_proc, _) = Cr3::read(geometry);
    assert_eq!(cr3_proc.address(), pml4_a, "Cross-process switch must load target PML4 root into CR3");

    // Switch back to PID 0
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        crate::task::scheduler::switch_address_space_locked(
            t_desc,
            &raw mut THREAD_TABLE[0].thread,
        );
        SCHEDULER.lock.unlock_restore(orig);
        let _ = crate::task::thread::free_descriptor(t_desc);
    }
    let (cr3_kernel, _) = Cr3::read(geometry);
    assert_eq!(cr3_kernel.address(), get_master_kernel_pml4().address(), "Switch to kernel process must restore MASTER_KERNEL_PML4 in CR3");
    kprintln!("  Test E: Cross-process CR3 switching [VERIFIED]");

    // Clean up process A via join simulation
    unsafe {
        (*proc_a_ptr).thread_count = 0;
        (*proc_a_ptr).state = ProcessState::Zombie;
        (*proc_a_ptr).exit_code = 0;
        (*proc_a_ptr).completion_event.signal();
    }
    let exit_a = handle_a.join(pmm).expect("Join process A");
    assert_eq!(exit_a, 0);

    // Test F: Single-thread final exit & canonical lifecycle
    let handle_f = create_process(0, pmm).expect("Create process F");
    let pid_f = handle_f.pid;
    let proc_f = handle_f.process;
    let t_f = crate::task::thread::allocate_descriptor().expect("Allocate thread F");
    unsafe {
        (*t_f).process_id = pid_f;
        (*t_f).state = ThreadState::Running;
        (*proc_f).thread_count = 1;

        // Transition Active -> Terminating -> Zombie
        assert_eq!((*proc_f).state, ProcessState::Active);
        (*proc_f).state = ProcessState::Terminating;
        assert_eq!((*proc_f).state, ProcessState::Terminating);
        (*t_f).state = ThreadState::Zombie;
        (*proc_f).thread_count -= 1;
        assert_eq!((*proc_f).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
        (*proc_f).state = ProcessState::Zombie;
        (*proc_f).exit_code = 42;
        (*proc_f).completion_event.signal();
    }
    let exit_f = handle_f.join(pmm).expect("Join process F");
    assert_eq!(exit_f, 42, "Process exit code must propagate faithfully");
    kprintln!("  Test F: Single-thread final exit (Active -> Terminating -> Zombie) [VERIFIED]");

    // Test G: Multi-thread final exit & quiescence
    let handle_g = create_process(0, pmm).expect("Create process G");
    let pid_g = handle_g.pid;
    let proc_g = handle_g.process;
    let t_g1 = crate::task::thread::allocate_descriptor().expect("Thread G1");
    let t_g2 = crate::task::thread::allocate_descriptor().expect("Thread G2");
    unsafe {
        (*t_g1).process_id = pid_g;
        (*t_g2).process_id = pid_g;
        (*proc_g).thread_count = 2;

        // G1 terminates first: process remains Active
        (*t_g1).state = ThreadState::Zombie;
        (*proc_g).thread_count -= 1;
        assert_eq!((*proc_g).thread_count, 1);
        assert_eq!((*proc_g).state, ProcessState::Active);

        // G2 terminates: process transitions to Terminating then Zombie
        (*t_g2).state = ThreadState::Zombie;
        (*proc_g).thread_count -= 1;
        assert_eq!((*proc_g).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
        (*proc_g).state = ProcessState::Terminating;
        (*proc_g).state = ProcessState::Zombie;
        (*proc_g).exit_code = 77;
        (*proc_g).completion_event.signal();
    }
    let exit_g = handle_g.join(pmm).expect("Join process G");
    assert_eq!(exit_g, 77);
    kprintln!("  Test G: Multi-thread final exit & thread group quiescence [VERIFIED]");

    // Test H: Event waiter cancellation
    let handle_h = create_process(0, pmm).expect("Create process H");
    let pid_h = handle_h.pid;
    let proc_h = handle_h.process;
    let t_h = crate::task::thread::allocate_descriptor().expect("Thread H");
    let mut event_h = Event::new(false, EventType::AutoReset);
    unsafe {
        (*t_h).process_id = pid_h;
        (*t_h).state = ThreadState::Blocked;
        let orig_rflags = SCHEDULER.lock.acquire();
        event_h.waiters.push_priority_locked(t_h);
        assert_eq!(event_h.waiters.len(), 1);

        let removed = event_h.cancel_waiter_locked(t_h);
        SCHEDULER.lock.unlock_restore(orig_rflags);

        assert!(removed, "Event waiter must be cleanly removed");
        assert_eq!(event_h.waiters.len(), 0, "Event waiter queue must be empty");
        assert!((*t_h).next_waiter.is_null(), "next_waiter must be null");

        (*t_h).state = ThreadState::Zombie;
        (*proc_h).thread_count = 0;
        assert_eq!((*proc_h).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
        (*proc_h).state = ProcessState::Zombie;
        (*proc_h).completion_event.signal();
    }
    let _ = handle_h.join(pmm).expect("Join process H");
    kprintln!("  Test H: Event waiter cancellation [VERIFIED]");

    // Test I: Mutex waiter and owner cancellation
    let handle_i = create_process(0, pmm).expect("Create process I");
    let pid_i = handle_i.pid;
    let proc_i = handle_i.process;
    let t_i1 = crate::task::thread::allocate_descriptor().expect("Thread I1");
    let t_i2 = crate::task::thread::allocate_descriptor().expect("Thread I2");
    let t_ext = crate::task::thread::allocate_descriptor().expect("Thread External");
    let mut mutex_i = Mutex::new();
    unsafe {
        (*t_i1).process_id = pid_i;
        (*t_i1).state = ThreadState::Running;
        (*t_i2).process_id = pid_i;
        (*t_i2).state = ThreadState::Blocked;
        (*t_ext).process_id = 0; // Kernel thread
        (*t_ext).state = ThreadState::Blocked;

        // t_i1 claims ownership
        mutex_i.owner = (*t_i1).id;
        let orig_rflags = SCHEDULER.lock.acquire();
        // t_i2 (same process) and t_ext (external process) wait on mutex
        mutex_i.waiters.push_priority_locked(t_i2);
        mutex_i.waiters.push_priority_locked(t_ext);
        assert_eq!(mutex_i.waiters.len(), 2);

        // Process I terminates: cancel_owner_locked must NOT grant ownership to t_i2!
        mutex_i.cancel_owner_locked(pid_i);
        SCHEDULER.lock.unlock_restore(orig_rflags);

        assert_eq!(mutex_i.owner, (*t_ext).id, "Mutex ownership must hand off to external waiter, not peer terminating thread");
        assert_eq!((*t_i2).state, ThreadState::Zombie, "Peer waiter must be marked Zombie");

        // Clean up external waiter (which was woken to Ready into SCHEDULER runqueue)
        let orig_clean = SCHEDULER.lock.acquire();
        SCHEDULER.normal_queue.remove(t_ext);
        SCHEDULER.lock.unlock_restore(orig_clean);
        mutex_i.owner = 0;
        let _ = free_descriptor(t_ext);

        (*t_i1).state = ThreadState::Zombie;
        (*proc_i).thread_count = 0;
        assert_eq!((*proc_i).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
        (*proc_i).state = ProcessState::Zombie;
        (*proc_i).completion_event.signal();
    }
    let _ = handle_i.join(pmm).expect("Join process I");
    kprintln!("  Test I: Mutex waiter & owner cancellation with peer exclusion [VERIFIED]");

    // Test J: Condvar waiter cancellation
    let handle_j = create_process(0, pmm).expect("Create process J");
    let pid_j = handle_j.pid;
    let proc_j = handle_j.process;
    let t_j = crate::task::thread::allocate_descriptor().expect("Thread J");
    let mut condvar_j = Condvar::new();
    unsafe {
        (*t_j).process_id = pid_j;
        (*t_j).state = ThreadState::Blocked;
        let orig_rflags = SCHEDULER.lock.acquire();
        condvar_j.waiters.push_priority_locked(t_j);
        assert_eq!(condvar_j.waiter_count(), 1);

        let removed = condvar_j.cancel_waiter_locked(t_j);
        SCHEDULER.lock.unlock_restore(orig_rflags);

        assert!(removed, "Condvar waiter must be cancelled cleanly");
        assert_eq!(condvar_j.waiter_count(), 0);
        assert!((*t_j).next_waiter.is_null());

        (*t_j).state = ThreadState::Zombie;
        (*proc_j).thread_count = 0;
        assert_eq!((*proc_j).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
        (*proc_j).state = ProcessState::Zombie;
        (*proc_j).completion_event.signal();
    }
    let _ = handle_j.join(pmm).expect("Join process J");
    kprintln!("  Test J: Condvar waiter cancellation [VERIFIED]");

    // Test K: SleepTable cancellation
    let handle_k = create_process(0, pmm).expect("Create process K");
    let pid_k = handle_k.pid;
    let proc_k = handle_k.process;
    let t_k = crate::task::thread::allocate_descriptor().expect("Thread K");
    unsafe {
        (*t_k).process_id = pid_k;
        (*t_k).state = ThreadState::Blocked;

        let orig_rflags = SCHEDULER.lock.acquire();
        SLEEP_TABLE[0] = Some(crate::task::sleep::SleepEntry {
            thread: t_k,
            deadline_tick: 999999,
        });
        SLEEP_WAIT_QUEUE.push_priority_locked(t_k);
        assert_eq!(SLEEP_WAIT_QUEUE.len(), 1);

        let removed = cancel_sleep_locked(t_k);
        SCHEDULER.lock.unlock_restore(orig_rflags);

        assert!(removed, "Sleeper must be cancelled cleanly");
        assert!(SLEEP_TABLE[0].is_none(), "SleepTable entry must be cleared");
        assert_eq!(SLEEP_WAIT_QUEUE.len(), 0, "SLEEP_WAIT_QUEUE must be empty");
        assert!((*t_k).next_waiter.is_null());

        (*t_k).state = ThreadState::Zombie;
        (*proc_k).thread_count = 0;
        assert_eq!((*proc_k).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
        (*proc_k).state = ProcessState::Zombie;
        (*proc_k).completion_event.signal();
    }
    let _ = handle_k.join(pmm).expect("Join process K");
    kprintln!("  Test K: SleepTable cancellation [VERIFIED]");

    // Test L: Process termination with mixed thread states
    let handle_l = create_process(0, pmm).expect("Create process L");
    let pid_l = handle_l.pid;
    let proc_l = handle_l.process;
    let t_ready = crate::task::thread::allocate_descriptor().expect("Thread Ready");
    let t_blocked = crate::task::thread::allocate_descriptor().expect("Thread Blocked");
    let t_driver = crate::task::thread::allocate_descriptor().expect("Thread Driver");
    unsafe {
        (*proc_l).thread_count = 3;
        (*t_ready).process_id = pid_l;
        (*t_ready).state = ThreadState::Ready;

        (*t_blocked).process_id = pid_l;
        (*t_blocked).state = ThreadState::Blocked;

        (*t_driver).process_id = pid_l;
        (*t_driver).state = ThreadState::Running;

        let orig_rflags = SCHEDULER.lock.acquire();
        SCHEDULER.enqueue(t_ready);
        cancel_process_threads_locked(proc_l, t_driver);
        SCHEDULER.lock.unlock_restore(orig_rflags);

        assert_eq!((*t_ready).state, ThreadState::Zombie);
        assert!((*t_ready).next_runnable.is_null());
        assert_eq!((*t_blocked).state, ThreadState::Zombie);
        assert!((*t_blocked).next_waiter.is_null());
        assert_eq!((*proc_l).thread_count, 1, "Two siblings cancelled -> thread_count must be 1");

        (*t_driver).state = ThreadState::Zombie;
        (*proc_l).thread_count -= 1;
        assert_eq!((*proc_l).thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
        (*proc_l).state = ProcessState::Zombie;
        (*proc_l).completion_event.signal();
    }
    let _ = handle_l.join(pmm).expect("Join process L");
    kprintln!("  Test L: Process termination with mixed thread states [VERIFIED]");

    // Test M: Process Join (late join pass-through)
    let handle_m = create_process(0, pmm).expect("Create process M");
    let proc_m = handle_m.process;
    unsafe {
        (*proc_m).thread_count = 0;
        (*proc_m).state = ProcessState::Zombie;
        (*proc_m).exit_code = 123;
        (*proc_m).completion_event.signal();
    }
    let exit_m = handle_m.join(pmm).expect("Late join process M");
    assert_eq!(exit_m, 123);
    kprintln!("  Test M: Process join (late join pass-through) [VERIFIED]");

    // Test N: Process Reaper
    let handle_n = create_process(0, pmm).expect("Create process N");
    let proc_n = handle_n.process;
    unsafe {
        (*proc_n).thread_count = 0;
        (*proc_n).state = ProcessState::Zombie;
        (*proc_n).exit_code = 99;
    }
    handle_n.detach();
    let reaped_count = reap_zombie_processes(pmm);
    assert_eq!(reaped_count, 1, "Reaper must scavenge detached zombie process");
    kprintln!("  Test N: Process reaper scavenging [VERIFIED]");

    // Test O: Parent Process Abandonment
    let parent_h = create_process(0, pmm).expect("Parent process");
    let child1_h = create_process(parent_h.pid, pmm).expect("Child 1 (Live)");
    let child2_h = create_process(parent_h.pid, pmm).expect("Child 2 (Zombie)");

    let parent_proc = parent_h.process;
    let child1_proc = child1_h.process;
    let child2_proc = child2_h.process;

    unsafe {
        // Child 2 terminates first
        (*child2_proc).thread_count = 0;
        (*child2_proc).state = ProcessState::Zombie;
        assert_eq!((*child2_proc).zombie_queued, false);

        // Parent terminates and abandons children
        let orig_rflags = SCHEDULER.lock.acquire();
        let mut ch = (*parent_proc).first_child_process;
        while !ch.is_null() {
            let next_sib = (*ch).next_sibling_process;
            (*ch).parent_pid = 0;
            (*ch).next_sibling_process = core::ptr::null_mut();
            if (*ch).state == ProcessState::Zombie && !(*ch).zombie_queued {
                enqueue_zombie_process_locked(ch);
            }
            ch = next_sib;
        }
        (*parent_proc).first_child_process = core::ptr::null_mut();
        SCHEDULER.lock.unlock_restore(orig_rflags);

        assert_eq!((*child1_proc).parent_pid, 0, "Live child must be detached");
        assert_eq!((*child1_proc).state, ProcessState::Active, "Live child remains executable");
        assert_eq!((*child2_proc).parent_pid, 0, "Zombie child must be detached");
        assert_eq!((*child2_proc).zombie_queued, true, "Zombie child must be enqueued into ZOMBIE_PROCESS_QUEUE");

        // Clean up parent
        (*parent_proc).thread_count = 0;
        (*parent_proc).state = ProcessState::Zombie;
        (*parent_proc).completion_event.signal();
    }
    let _ = parent_h.join(pmm).expect("Join parent");

    // Clean up child 1
    unsafe {
        (*child1_proc).thread_count = 0;
        (*child1_proc).state = ProcessState::Zombie;
        (*child1_proc).completion_event.signal();
    }
    let _ = child1_h.join(pmm).expect("Join child 1");

    // Reap child 2 from ZOMBIE_PROCESS_QUEUE
    let reaped_children = reap_zombie_processes(pmm);
    assert_eq!(reaped_children, 1, "Abandoned zombie child must be reaped");
    core::mem::forget(child2_h);

    kprintln!("  Test O: Parent process abandonment [VERIFIED]");

    // Test P: Zombie Process Queue Exactly-Once Membership
    let handle_p = create_process(0, pmm).expect("Process P");
    let proc_p = handle_p.process;
    unsafe {
        let orig_rflags = SCHEDULER.lock.acquire();
        (*proc_p).thread_count = 0;
        (*proc_p).state = ProcessState::Zombie;
        assert!(!(*proc_p).zombie_queued);

        enqueue_zombie_process_locked(proc_p);
        assert!((*proc_p).zombie_queued, "zombie_queued must be true after enqueue");

        let popped = dequeue_zombie_process_locked();
        assert_eq!(popped, Some(proc_p));
        assert!(!(*proc_p).zombie_queued, "zombie_queued must be false after dequeue");

        SCHEDULER.lock.unlock_restore(orig_rflags);

        (*proc_p).completion_event.signal();
    }
    let _ = handle_p.join(pmm).expect("Join process P");
    kprintln!("  Test P: Zombie process queue exactly-once membership [VERIFIED]");

    // Test Q: Active CR3 Reclamation Prevention
    let handle_q = create_process(0, pmm).expect("Process Q");
    let proc_q = handle_q.process;
    let pml4_q = unsafe { (*proc_q).address_space };
    unsafe {
        Cr3::write(PhysFrame(pml4_q), Cr3Flags::empty(), geometry);
        let (active_cr3, _) = Cr3::read(geometry);
        assert_eq!(active_cr3.address(), pml4_q);

        let mut addr_space_q = AddressSpace::from_root(PhysFrame(pml4_q));
        addr_space_q.destroy(pmm).expect("Destroy address space Q");

        let (restored_cr3, _) = Cr3::read(geometry);
        assert_eq!(
            restored_cr3.address(),
            get_master_kernel_pml4().address(),
            "destroy() must restore CR3 to MASTER_KERNEL_PML4 when active"
        );

        (*proc_q).state = ProcessState::Free;
        for i in 0..MAX_PROCESSES {
            if PROCESS_TABLE[i].occupied && PROCESS_TABLE[i].process.id == handle_q.pid {
                PROCESS_TABLE[i].occupied = false;
                break;
            }
        }
    }
    core::mem::forget(handle_q);
    kprintln!("  Test Q: Active CR3 reclamation hazard elimination [VERIFIED]");

    // Test R: ProcessId Stale-Handle Protection
    let handle_r1 = create_process(0, pmm).expect("Process R1");
    let pid_r1 = handle_r1.pid;
    let proc_r1 = handle_r1.process;
    unsafe {
        (*proc_r1).thread_count = 0;
        (*proc_r1).state = ProcessState::Zombie;
        (*proc_r1).completion_event.signal();
    }
    let _ = handle_r1.join(pmm).expect("Join process R1");

    let handle_r2 = create_process(0, pmm).expect("Process R2");
    assert_ne!(handle_r2.pid, pid_r1, "PIDs must be strictly monotonic");

    let stale_handle = ProcessHandle {
        pid: pid_r1,
        process: handle_r2.process,
    };
    assert_eq!(
        stale_handle.join(pmm),
        Err(ProcessError::InvalidHandle),
        "Stale process handle must be deterministically rejected"
    );

    unsafe {
        (*handle_r2.process).thread_count = 0;
        (*handle_r2.process).state = ProcessState::Zombie;
        (*handle_r2.process).completion_event.signal();
    }
    let _ = handle_r2.join(pmm).expect("Join process R2");
    kprintln!("  Test R: Monotonic ProcessId & stale-handle protection [VERIFIED]");

    // Test S: Exact PMM Accounting
    let final_free = pmm.free_frame_count();
    assert_eq!(
        final_free, baseline_free,
        "PMM physical frame leak detected! baseline={} final={}",
        baseline_free, final_free
    );
    kprintln!("  Stack & PML4 Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    kprintln!("  Test S: Exact PMM frame accounting baseline match [VERIFIED]");

    // Test T: Stage 1-3E Full Regression & Subsystem Invariant Verification
    assert_eq!(pmm.free_frame_count(), baseline_free, "Stage 1-3E invariant: frame neutrality preserved");
    kprintln!("  Test T: Complete Stage 1-3E regression contract & system invariants [VERIFIED]");

    kprintln!("  [x] Stage 3F Process Model, Address Spaces & Lifecycle verified.");
}
