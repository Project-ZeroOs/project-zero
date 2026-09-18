//! Project Zero - Stage 3D Increment 3: Condition Variable Primitive
//!
//! Implements zero-allocation Condition Variables backed intrusively by `WaitQueue`.
//! - Atomically registers the waiting thread on the condvar wait queue and releases
//!   the associated Mutex via `Mutex::unlock_locked()` under `SCHEDULER.lock`.
//! - Prepares block via `prepare_block_detached_locked()` and context transfers via `commit_block_and_switch()`.
//! - On wakeup, re-acquires the Mutex via `mutex.lock()` before returning to caller.
//! - Signaling via `signal()` (wakes highest-priority waiter) and `broadcast()` (wakes all waiters).
//! - Zero dynamic heap allocation.

use crate::task::mutex::Mutex;
use crate::task::percpu::current_thread_from_gs;
use crate::task::scheduler::{
    commit_block_and_switch, cpu_if_bit, prepare_block_detached_locked, SCHEDULER,
};
use crate::task::waitqueue::WaitQueue;

/// Error types for Condvar validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CondvarError {
    MutexNotHeld,
}

/// Zero-allocation kernel Condition Variable.
pub struct Condvar {
    pub(crate) waiters: WaitQueue,
}

unsafe impl Send for Condvar {}
unsafe impl Sync for Condvar {}

impl Condvar {
    /// Creates a new Condition Variable with an empty waiter queue.
    pub const fn new() -> Self {
        Self {
            waiters: WaitQueue::new(),
        }
    }

    /// Validates whether the caller can wait on this condvar with the given mutex.
    #[inline(always)]
    pub fn validate_wait(&self, mutex: &Mutex, caller_id: u64) -> Result<(), CondvarError> {
        if mutex.owner() == 0 || mutex.owner() != caller_id {
            Err(CondvarError::MutexNotHeld)
        } else {
            Ok(())
        }
    }

    /// Returns the number of blocked threads currently waiting on this condition variable.
    #[inline(always)]
    pub fn waiter_count(&self) -> usize {
        self.waiters.len()
    }

    /// Returns true if there are no waiters currently on this condition variable.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.waiters.is_empty()
    }

    /// Atomically releases the held mutex, blocks the current thread on this condition variable,
    /// and re-acquires the mutex upon waking before returning to caller.
    ///
    /// REQUIRES:
    ///   - Caller MUST currently own `mutex`. Calling without holding the mutex panics.
    ///
    /// GUARANTEES:
    ///   - Atomic waiter registration and mutex release under `SCHEDULER.lock` preventing lost wakeups.
    ///   - Does NOT grant mutex ownership upon waking; the thread re-acquires `mutex` via `mutex.lock()`.
    #[inline(never)]
    pub fn wait(&mut self, mutex: &mut Mutex) {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let current = current_thread_from_gs();
        let current_id = unsafe { (*current).id };

        // 1. Mandatory ownership check: caller must own mutex
        if mutex.owner() != current_id {
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            panic!(
                "Condvar wait violation: caller does not own mutex (owner={}, caller={})",
                mutex.owner(), current_id
            );
        }

        unsafe {
            // 2. Atomically enqueue current thread onto condvar wait queue
            self.waiters.push_priority_locked(current);

            // 3. Atomically release mutex via _locked primitive (handoff to next mutex waiter if present)
            mutex.unlock_locked();

            // 4. Prepare block and context switch
            let (out_thread, prev_rsp, next_rsp, frame_type) = prepare_block_detached_locked();
            commit_block_and_switch(out_thread, prev_rsp, next_rsp, orig_rflags, frame_type);

            // ==================== POST-RESUMPTION ====================
            // Execution resumes here after another thread wakes this thread via signal() or broadcast().
            // Waking from Condvar places the thread into Ready/Running, but does NOT grant mutex ownership.
        }

        // 5. Re-acquire mutex before returning to caller.
        // If another thread holds the mutex, this call cleanly blocks until ownership is granted!
        mutex.lock();
    }

    /// Wakes the highest-priority waiter on this condition variable, if any.
    ///
    /// Does NOT require the caller to hold the associated Mutex, although the state
    /// predicate being signaled must be protected by the Mutex.
    ///
    /// Returns true if a waiter was woken, false if the queue was empty.
    #[inline(never)]
    pub fn signal(&mut self) -> bool {
        self.waiters.wake_one().is_some()
    }

    /// Wakes all waiters currently blocked on this condition variable.
    ///
    /// Woken threads transition `Blocked -> Ready` and compete for the Mutex via `mutex.lock()`.
    ///
    /// Returns the number of threads woken.
    #[inline(never)]
    pub fn broadcast(&mut self) -> usize {
        self.waiters.wake_all()
    }

    /// Subsystem-specific cancellation: cleanly detaches a waiter under SCHEDULER.lock.
    pub unsafe fn cancel_waiter_locked(&mut self, thread: *mut crate::task::thread::KernelThread) -> bool {
        assert!(SCHEDULER.lock.is_locked(), "cancel_waiter_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "cancel_waiter_locked requires IF=0");
        self.waiters.remove_locked(thread)
    }
}
