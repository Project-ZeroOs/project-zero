//! Project Zero - Stage 3D Increment 1: Intrusive WaitQueue Primitive
//!
//! Implements zero-allocation, intrusive wait queues linking threads
//! through `KernelThread.next_waiter` (offset 96).

use crate::task::thread::{KernelThread, Priority};
use crate::task::scheduler::{cpu_if_bit, wake_thread_locked, SCHEDULER};

/// Zero-allocation intrusive singly-linked wait queue.
/// Links nodes through `KernelThread.next_waiter` (offset 96).
pub struct WaitQueue {
    pub(crate) head: *mut KernelThread,
    pub(crate) tail: *mut KernelThread,
    pub(crate) count: usize,
}


impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            head: core::ptr::null_mut(),
            tail: core::ptr::null_mut(),
            count: 0,
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head.is_null()
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Enqueues a waiter ordered by Priority (`Critical > High > Normal`).
    /// Preserves strict FIFO order within the same priority class.
    ///
    /// REQUIRES:
    ///   - SCHEDULER.lock held
    ///   - CPU IF == 0
    ///   - thread != null
    ///   - (*thread).next_waiter == null (I1: must not be linked in another wait queue)
    pub unsafe fn push_priority_locked(&mut self, thread: *mut KernelThread) {
        assert!(SCHEDULER.lock.is_locked(), "push_priority_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "push_priority_locked requires IF=0");
        assert!(!thread.is_null(), "Cannot enqueue null thread into WaitQueue");
        assert!(
            (*thread).next_waiter.is_null(),
            "I1 Violation: Waiter must have next_waiter == NULL before insertion"
        );

        let priority = (*thread).priority;

        if self.head.is_null() {
            self.head = thread;
            self.tail = thread;
            (*thread).next_waiter = core::ptr::null_mut();
        } else if priority > (*self.head).priority {
            // Strictly greater: becomes new head
            (*thread).next_waiter = self.head;
            self.head = thread;
        } else {
            // Traverse past all nodes with priority >= incoming thread's priority
            // to maintain strict FIFO for identical priorities.
            let mut curr = self.head;
            while !(*curr).next_waiter.is_null() && (*(*curr).next_waiter).priority >= priority {
                curr = (*curr).next_waiter;
            }
            (*thread).next_waiter = (*curr).next_waiter;
            (*curr).next_waiter = thread;
            if (*thread).next_waiter.is_null() {
                self.tail = thread;
            }
        }
        self.count += 1;
    }

    /// Pops the highest-priority (head) waiter and cleanly detaches it.
    ///
    /// REQUIRES:
    ///   - SCHEDULER.lock held
    ///   - CPU IF == 0
    ///
    /// ENSURES:
    ///   - Returned thread has `next_waiter == NULL`.
    pub unsafe fn pop_highest_locked(&mut self) -> Option<*mut KernelThread> {
        assert!(SCHEDULER.lock.is_locked(), "pop_highest_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "pop_highest_locked requires IF=0");

        if self.head.is_null() {
            return None;
        }

        let t = self.head;
        self.head = (*t).next_waiter;
        if self.head.is_null() {
            self.tail = core::ptr::null_mut();
        }
        (*t).next_waiter = core::ptr::null_mut(); // Completely detached
        self.count -= 1;
        Some(t)
    }

    /// Unlinks a specific waiter from anywhere in the queue and cleanly detaches it.
    ///
    /// REQUIRES:
    ///   - SCHEDULER.lock held
    ///   - CPU IF == 0
    ///
    /// ENSURES:
    ///   - If found, returns true and unlinked thread has `next_waiter == NULL`.
    pub unsafe fn remove_locked(&mut self, thread: *mut KernelThread) -> bool {
        assert!(SCHEDULER.lock.is_locked(), "remove_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "remove_locked requires IF=0");

        if self.head.is_null() || thread.is_null() {
            return false;
        }

        if self.head == thread {
            self.head = (*thread).next_waiter;
            if self.head.is_null() {
                self.tail = core::ptr::null_mut();
            }
            (*thread).next_waiter = core::ptr::null_mut(); // Completely detached
            self.count -= 1;
            return true;
        }

        let mut curr = self.head;
        while !(*curr).next_waiter.is_null() && (*curr).next_waiter != thread {
            curr = (*curr).next_waiter;
        }

        if !(*curr).next_waiter.is_null() {
            (*curr).next_waiter = (*thread).next_waiter;
            if self.tail == thread {
                self.tail = curr;
            }
            (*thread).next_waiter = core::ptr::null_mut(); // Completely detached
            self.count -= 1;
            return true;
        }

        false
    }

    /// Public queue-aware wake-one API.
    /// Pops the highest-priority waiter, detaches it under lock, and moves it to Ready.
    pub fn wake_one(&mut self) -> Option<*mut KernelThread> {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let thread = unsafe {
            if let Some(t) = self.pop_highest_locked() {
                wake_thread_locked(t);
                Some(t)
            } else {
                None
            }
        };
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        thread
    }

    /// Public queue-aware wake-all API.
    /// Pops all queued waiters in priority order, detaches each under lock, and moves to Ready.
    pub fn wake_all(&mut self) -> usize {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let mut count = 0;
        unsafe {
            while let Some(t) = self.pop_highest_locked() {
                wake_thread_locked(t);
                count += 1;
            }
        }
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        count
    }

    /// Public queue-aware wake-thread API.
    /// Unlinks the specified thread from this wait queue and moves it to Ready.
    pub fn wake_thread(&mut self, thread: *mut KernelThread) -> bool {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let woken = unsafe {
            if self.remove_locked(thread) {
                wake_thread_locked(thread);
                true
            } else {
                false
            }
        };
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        woken
    }
}
