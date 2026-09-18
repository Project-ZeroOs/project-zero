//! Project Zero - Stage 3E: Thread Lifecycle & Resource Reclamation
//!
//! Authoritative Contract: ADR-0014 & Stage 3E Architecture Rev3.
//!
//! Enforces:
//! - Complete thread lifecycle: Initializing -> Ready <-> Running <-> Blocked -> Zombie -> Reclaiming -> Free.
//! - Hybrid reclamation: synchronous JoinHandle joining + asynchronous Reaper scavenging.
//! - Strict single-membership invariant in ZOMBIE_QUEUE via immediate child-list unlinking.
//! - Atomic Zombie -> Reclaiming state claim serialized under SCHEDULER.lock with IF=0.
//! - Zero dynamic heap allocation throughout all lifecycle paths.
//! - Exact PMM frame neutrality across creation, termination, and reclamation.

use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;
use crate::task::percpu::current_thread_from_gs;
use crate::task::scheduler::{cpu_if_bit, SCHEDULER};
pub use crate::task::thread::ThreadId;
use crate::task::thread::{
    create_thread, destroy_thread_stack, free_descriptor, KernelThread, Priority, ThreadError,
    ThreadState, MAX_THREADS, THREAD_TABLE,
};

// ====================================================================
// Intrusive Zombie Queue for Detached / Abandoned Reaping
// ====================================================================

static mut ZOMBIE_QUEUE_HEAD: *mut KernelThread = core::ptr::null_mut();
static mut ZOMBIE_QUEUE_TAIL: *mut KernelThread = core::ptr::null_mut();
static mut ZOMBIE_COUNT: usize = 0;

/// Enqueues a Zombie thread into the intrusive ZOMBIE_QUEUE.
///
/// Precondition: SCHEDULER.lock held, IF=0.
/// Invariant: A thread descriptor may be enqueued into ZOMBIE_QUEUE at most once.
pub(crate) unsafe fn enqueue_zombie_locked(thread: *mut KernelThread) {
    assert!(SCHEDULER.lock.is_locked(), "enqueue_zombie_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "enqueue_zombie_locked requires IF=0");
    assert!(!thread.is_null(), "Cannot enqueue null thread as zombie");
    assert_eq!((*thread).state, ThreadState::Zombie, "Only Zombie threads can enter ZOMBIE_QUEUE");
    assert!(!(*thread).zombie_queued, "Thread already marked as zombie_queued");
    assert!((*thread).next_zombie.is_null(), "Thread already linked in ZOMBIE_QUEUE");

    (*thread).zombie_queued = true;
    if ZOMBIE_QUEUE_TAIL.is_null() {
        ZOMBIE_QUEUE_HEAD = thread;
        ZOMBIE_QUEUE_TAIL = thread;
    } else {
        (*ZOMBIE_QUEUE_TAIL).next_zombie = thread;
        ZOMBIE_QUEUE_TAIL = thread;
    }
    (*thread).next_zombie = core::ptr::null_mut();
    ZOMBIE_COUNT += 1;
}

/// Dequeues the next Zombie thread from ZOMBIE_QUEUE.
///
/// Precondition: SCHEDULER.lock held, IF=0.
pub(crate) unsafe fn dequeue_zombie_locked() -> Option<*mut KernelThread> {
    assert!(SCHEDULER.lock.is_locked(), "dequeue_zombie_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "dequeue_zombie_locked requires IF=0");

    if ZOMBIE_QUEUE_HEAD.is_null() {
        return None;
    }

    let thread = ZOMBIE_QUEUE_HEAD;
    assert!((*thread).zombie_queued, "Dequeued thread must have zombie_queued == true");
    (*thread).zombie_queued = false;

    ZOMBIE_QUEUE_HEAD = (*thread).next_zombie;
    if ZOMBIE_QUEUE_HEAD.is_null() {
        ZOMBIE_QUEUE_TAIL = core::ptr::null_mut();
    }
    (*thread).next_zombie = core::ptr::null_mut();
    ZOMBIE_COUNT -= 1;
    Some(thread)
}

/// Returns the number of threads currently waiting in ZOMBIE_QUEUE.
pub fn zombie_queue_count() -> usize {
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let count = unsafe { ZOMBIE_COUNT };
    unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
    count
}

/// Unlinks a specific Zombie thread from ZOMBIE_QUEUE under SCHEDULER.lock.
pub(crate) unsafe fn remove_zombie_locked(thread: *mut KernelThread) -> bool {
    assert!(SCHEDULER.lock.is_locked(), "remove_zombie_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "remove_zombie_locked requires IF=0");

    if ZOMBIE_QUEUE_HEAD.is_null() || thread.is_null() {
        return false;
    }

    if ZOMBIE_QUEUE_HEAD == thread {
        ZOMBIE_QUEUE_HEAD = (*thread).next_zombie;
        if ZOMBIE_QUEUE_HEAD.is_null() {
            ZOMBIE_QUEUE_TAIL = core::ptr::null_mut();
        }
        (*thread).next_zombie = core::ptr::null_mut();
        (*thread).zombie_queued = false;
        ZOMBIE_COUNT -= 1;
        return true;
    }

    let mut curr = ZOMBIE_QUEUE_HEAD;
    while !(*curr).next_zombie.is_null() && (*curr).next_zombie != thread {
        curr = (*curr).next_zombie;
    }

    if !(*curr).next_zombie.is_null() {
        (*curr).next_zombie = (*thread).next_zombie;
        if ZOMBIE_QUEUE_TAIL == thread {
            ZOMBIE_QUEUE_TAIL = curr;
        }
        (*thread).next_zombie = core::ptr::null_mut();
        (*thread).zombie_queued = false;
        ZOMBIE_COUNT -= 1;
        return true;
    }

    false
}

// ====================================================================
// Atomic Reclamation Claim Primitive
// ====================================================================

/// Atomically transitions a thread from Zombie to Reclaiming.
///
/// Serialized strictly by SCHEDULER.lock with IF=0.
/// Returns true if this caller won the race to transition Zombie -> Reclaiming.
/// Returns false if another context already claimed it.
pub(crate) unsafe fn claim_reap_locked(thread: *mut KernelThread) -> bool {
    assert!(SCHEDULER.lock.is_locked(), "claim_reap_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "claim_reap_locked requires IF=0");

    if (*thread).state == ThreadState::Zombie {
        (*thread).state = ThreadState::Reclaiming;
        true
    } else {
        false
    }
}

// ====================================================================
// Parent-Child Intrusive Tracking Helpers
// ====================================================================

/// Adds `child` into `parent`'s intrusive child list.
/// Precondition: SCHEDULER.lock held, IF=0.
pub(crate) unsafe fn add_child_locked(parent: *mut KernelThread, child: *mut KernelThread) {
    assert!(SCHEDULER.lock.is_locked());
    assert_eq!(cpu_if_bit(), 0);

    (*child).parent_id = (*parent).id;
    (*child).next_sibling = (*parent).first_child;
    (*parent).first_child = child;
}

/// Removes `child` from `parent`'s intrusive child list.
/// Precondition: SCHEDULER.lock held, IF=0.
pub(crate) unsafe fn remove_child_locked(parent: *mut KernelThread, child: *mut KernelThread) {
    assert!(SCHEDULER.lock.is_locked());
    assert_eq!(cpu_if_bit(), 0);

    let mut curr = (*parent).first_child;
    let mut prev: *mut KernelThread = core::ptr::null_mut();

    while !curr.is_null() {
        if curr == child {
            if prev.is_null() {
                (*parent).first_child = (*curr).next_sibling;
            } else {
                (*prev).next_sibling = (*curr).next_sibling;
            }
            (*curr).parent_id = 0;
            (*curr).next_sibling = core::ptr::null_mut();
            return;
        }
        prev = curr;
        curr = (*curr).next_sibling;
    }
}

/// Looks up an active thread descriptor by its unique monotonic ThreadId under lock.
pub(crate) unsafe fn lookup_thread_by_id_locked(id: u64) -> Option<*mut KernelThread> {
    assert!(SCHEDULER.lock.is_locked());
    let table = &raw mut THREAD_TABLE;
    for i in 0..MAX_THREADS {
        let slot = &mut (*table)[i];
        if slot.occupied && slot.thread.id == id {
            return Some(&raw mut slot.thread);
        }
    }
    None
}

/// Detaches a thread under lock: unlinks it from parent and enqueues to ZOMBIE_QUEUE if already Zombie.
pub(crate) unsafe fn detach_thread_locked(slot_idx: usize, expected_id: ThreadId) {
    assert!(SCHEDULER.lock.is_locked());
    assert_eq!(cpu_if_bit(), 0);

    if slot_idx >= MAX_THREADS {
        return;
    }

    let table = &raw mut THREAD_TABLE;
    let slot = &mut (*table)[slot_idx];
    if !slot.occupied || slot.thread.id != expected_id.0 {
        return;
    }

    let target = &raw mut slot.thread;
    (*target).is_detached = true;

    // Unlink from parent's child list if attached
    let parent_id = (*target).parent_id;
    if parent_id != 0 {
        if let Some(parent) = lookup_thread_by_id_locked(parent_id) {
            remove_child_locked(parent, target);
        }
        (*target).parent_id = 0;
    }

    // If already Zombie, enqueue exactly once into ZOMBIE_QUEUE
    if (*target).state == ThreadState::Zombie && !(*target).zombie_queued {
        enqueue_zombie_locked(target);
    }
}

// ====================================================================
// Public Capability-Compatible JoinHandle
// ====================================================================

/// Linear, non-copyable capability representing exclusive ownership of a joinable thread.
pub struct JoinHandle {
    pub(crate) target_id: ThreadId,
    pub(crate) target_slot: usize,
}

impl JoinHandle {
    /// Returns the public ThreadId of the target thread.
    pub fn thread_id(&self) -> ThreadId {
        self.target_id
    }

    /// Synchronously waits for the target thread to terminate, claims exclusive reclamation rights,
    /// deallocates its guarded stack, frees its descriptor slot, and returns its exit code.
    ///
    /// Fails with:
    /// - `ThreadError::SelfJoin` if calling thread attempts to join itself.
    /// - `ThreadError::InvalidHandle` if slot was recycled or ID mismatch.
    /// - `ThreadError::AlreadyReclaimed` if another context claimed reclamation.
    pub fn join(
        mut self,
        pmm: &mut PhysicalMemoryManager,
        vmm: &mut ActivePageTable,
    ) -> Result<i32, ThreadError> {
        let current = unsafe { current_thread_from_gs() };
        if unsafe { (*current).id } == self.target_id.0 {
            return Err(ThreadError::SelfJoin);
        }

        if self.target_slot >= MAX_THREADS {
            return Err(ThreadError::InvalidHandle);
        }

        let table = &raw mut THREAD_TABLE;
        let slot = unsafe { &mut (*table)[self.target_slot] };
        let target = &raw mut slot.thread;

        // Verify slot validity before blocking
        {
            let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
            if !slot.occupied || unsafe { (*target).id } != self.target_id.0 {
                unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
                return Err(ThreadError::InvalidHandle);
            }
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        }

        // 1. Await completion latch (blocks via Event::wait() if target is still running)
        unsafe { (*target).completion_event.wait() };

        // 2. Acquire lock and atomically claim reclamation rights
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };

        if !slot.occupied || unsafe { (*target).id } != self.target_id.0 {
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            return Err(ThreadError::InvalidHandle);
        }

        let won_claim = unsafe { claim_reap_locked(target) };
        if !won_claim {
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            return Err(ThreadError::AlreadyReclaimed);
        }

        // Defuse Drop handler since this handle is successfully joining
        self.target_slot = usize::MAX;

        // Unlink from parent's child list if attached
        unsafe {
            let parent_id = (*target).parent_id;
            if parent_id != 0 {
                if let Some(parent) = lookup_thread_by_id_locked(parent_id) {
                    remove_child_locked(parent, target);
                }
                (*target).parent_id = 0;
            }
        }

        let exit_code = unsafe { (*target).exit_code };
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };

        // 3. Destroy quiescent stack (unmaps 4 pages, frees 4 frames, clears arena bit)
        destroy_thread_stack(target, pmm, vmm);

        // 4. Free descriptor (zeroes slot, sets occupied = false, state = Free)
        unsafe { free_descriptor(target).map_err(|_| ThreadError::InvalidPointer)? };

        Ok(exit_code)
    }

    /// Detaches the thread, relinquishing join authority and delegating reclamation
    /// to the background Zombie Reaper.
    pub fn detach(mut self) {
        let slot_idx = self.target_slot;
        let target_id = self.target_id;
        self.target_slot = usize::MAX; // Defuse Drop

        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        unsafe { detach_thread_locked(slot_idx, target_id) };
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
    }
}

impl Drop for JoinHandle {
    /// JoinHandle::Drop is best-effort convenience only.
    /// Kernel lifecycle correctness MUST NOT depend on Rust Drop execution (e.g. if the owner thread terminates).
    /// Kernel-level parent abandonment remains authoritative.
    fn drop(&mut self) {
        if self.target_slot < MAX_THREADS {
            let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
            unsafe { detach_thread_locked(self.target_slot, self.target_id) };
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        }
    }
}

// ====================================================================
// Public Creation & Reaper APIs
// ====================================================================

/// Spawns a new kernel thread under the cooperative/preemptive scheduler.
/// Returns its public ThreadId and exclusive JoinHandle.
pub fn spawn_thread(
    entry_fn: extern "C" fn(u64),
    arg: u64,
    priority: Priority,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) -> Result<(ThreadId, JoinHandle), ThreadError> {
    let thread_ptr = create_thread(entry_fn, arg, priority, pmm, vmm)?;
    let thread_id = unsafe { (*thread_ptr).id };

    // Find slot index
    let mut slot_idx = usize::MAX;
    unsafe {
        let table = &raw mut THREAD_TABLE;
        for i in 0..MAX_THREADS {
            if (&raw mut (*table)[i].thread) == thread_ptr {
                slot_idx = i;
                break;
            }
        }
    }
    assert!(slot_idx < MAX_THREADS, "Thread slot not found in THREAD_TABLE");

    // Under lock: link child into parent and enqueue into scheduler runqueue
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    unsafe {
        let parent = current_thread_from_gs();
        if !parent.is_null() && (*parent).state == ThreadState::Running {
            add_child_locked(parent, thread_ptr);
        }
        (*thread_ptr).state = ThreadState::Ready;
        SCHEDULER.enqueue(thread_ptr);
    }
    unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };

    Ok((
        ThreadId(thread_id),
        JoinHandle {
            target_id: ThreadId(thread_id),
            target_slot: slot_idx,
        },
    ))
}

/// Scavenges and reclaims all detached threads currently waiting in ZOMBIE_QUEUE.
///
/// Returns the number of zombie threads successfully reaped.
pub fn reap_zombies(pmm: &mut PhysicalMemoryManager, vmm: &mut ActivePageTable) -> usize {
    let mut reaped_count = 0;

    loop {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let candidate = unsafe { dequeue_zombie_locked() };

        let thread_to_destroy = match candidate {
            Some(t) => {
                let won = unsafe { claim_reap_locked(t) };
                if won {
                    Some(t)
                } else {
                    None
                }
            }
            None => None,
        };
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };

        match thread_to_destroy {
            Some(t) => {
                destroy_thread_stack(t, pmm, vmm);
                free_descriptor(t).expect("free_descriptor failed during reap");
                reaped_count += 1;
            }
            None => break,
        }
    }

    reaped_count
}
