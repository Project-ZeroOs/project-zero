//! Project Zero - Stage 3D Increment 2: Kernel Mutex Primitive
//!
//! Implements an authoritative single-core kernel Mutex serialized strictly
//! by `SCHEDULER.lock`:
//! - Direct ownership handoff on unlock.
//! - Priority FIFO waiter queuing via `WaitQueue`.
//! - Strict ownership validation (non-owner unlock panics).
//! - Non-recursive lock enforcement (recursive acquisition panics).
//! - Zero dynamic heap allocation.

use crate::task::waitqueue::WaitQueue;
use crate::task::percpu::current_thread_from_gs;
use crate::task::scheduler::{
    commit_block_and_switch, cpu_if_bit, prepare_block_locked, wake_thread_locked, SCHEDULER,
};

/// Error types for deterministic failure semantics validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockError {
    RecursiveLock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockError {
    NotLocked,
    NotOwner,
}

/// Authoritative single-core kernel Mutex.
pub struct Mutex {
    pub(crate) owner: u64, // ThreadId of current owner, 0 = unlocked
    pub(crate) waiters: WaitQueue,
}

unsafe impl Send for Mutex {}
unsafe impl Sync for Mutex {}

impl Mutex {
    /// Creates a new unlocked Mutex with an empty waiter queue.
    pub const fn new() -> Self {
        Self {
            owner: 0,
            waiters: WaitQueue::new(),
        }
    }

    /// Returns the thread ID of the current owner, or 0 if unlocked.
    #[inline(always)]
    pub fn owner(&self) -> u64 {
        self.owner
    }

    /// Returns true if the mutex is currently locked.
    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.owner != 0
    }

    /// Returns the number of blocked threads currently waiting on this mutex.
    #[inline(always)]
    pub fn waiter_count(&self) -> usize {
        self.waiters.len()
    }

    /// Validates whether the given thread ID can lock the mutex without deadlock.
    #[inline(always)]
    pub fn validate_lock(&self, caller_id: u64) -> Result<(), LockError> {
        if self.owner != 0 && self.owner == caller_id {
            Err(LockError::RecursiveLock)
        } else {
            Ok(())
        }
    }

    /// Validates whether the given thread ID has permission to unlock.
    #[inline(always)]
    pub fn validate_unlock(&self, caller_id: u64) -> Result<(), UnlockError> {
        if self.owner == 0 {
            Err(UnlockError::NotLocked)
        } else if self.owner != caller_id {
            Err(UnlockError::NotOwner)
        } else {
            Ok(())
        }
    }

    /// Validates whether the given thread ID has permission to unlock.
    #[inline(always)]
    pub fn can_unlock_by(&self, thread_id: u64) -> bool {
        self.validate_unlock(thread_id).is_ok()
    }

    /// Acquires the mutex, blocking if currently owned by another thread.
    ///
    /// Invariants:
    /// - Non-recursive: panics if current thread already owns the mutex.
    /// - Fast path: if free (owner == 0), claims ownership immediately and returns.
    /// - Contended path: blocks via `prepare_block_locked` and `commit_block_and_switch`.
    ///   Upon resumption, ownership has already been handed off directly by `unlock()`,
    ///   so the thread returns immediately without re-calling lock() or re-checking owner.
    #[inline(never)]
    pub fn lock(&mut self) {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let current = current_thread_from_gs();
        let current_id = unsafe { (*current).id };

        // M1 / Failure Semantics: Recursive lock detection
        if let Err(err) = self.validate_lock(current_id) {
            match err {
                LockError::RecursiveLock => {
                    panic!("Deadlock: recursive mutex lock attempted by thread {}", current_id);
                }
            }
        }

        // Fast path: mutex is unlocked
        if self.owner == 0 {
            self.owner = current_id;
            unsafe {
                register_active_mutex_locked(self as *mut Mutex);
                SCHEDULER.lock.unlock_restore(orig_rflags);
            };
            return;
        }

        // Contended path: block on mutex waiter queue
        unsafe {
            let (out_thread, prev_rsp, next_rsp, frame_type) = prepare_block_locked(&mut self.waiters);
            commit_block_and_switch(out_thread, prev_rsp, next_rsp, orig_rflags, frame_type);

            // DIRECT HANDOFF INVARIANT:
            // When execution resumes here, self.owner == current_id has ALREADY been
            // established by the unlocking thread inside unlock().
            assert_eq!(
                self.owner, current_id,
                "Direct handoff violation: resumed waiter does not own mutex"
            );
            register_active_mutex_locked(self as *mut Mutex);
            // The thread returns directly to caller without re-acquiring.
        }
    }

    /// Internal primitive to release the mutex while holding SCHEDULER.lock.
    ///
    /// REQUIRES:
    ///   - SCHEDULER.lock held
    ///   - CPU IF == 0
    ///
    /// ENSURES:
    ///   - SCHEDULER.lock remains HELD
    ///   - CPU IF remains 0
    ///   - Direct ownership handoff performed if waiter exists, else owner = 0
    pub(crate) unsafe fn unlock_locked(&mut self) {
        assert!(SCHEDULER.lock.is_locked(), "unlock_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "unlock_locked requires IF=0");

        if let Some(waiter) = self.waiters.pop_highest_locked() {
            // Invariant M5: waiter detached completely
            assert!((*waiter).next_waiter.is_null(), "M5: Pop from waiters must detach cleanly");

            // Direct ownership handoff:
            // New owner becomes the awakened waiter BEFORE wake_thread_locked transitions it to Ready.
            let waiter_id = (*waiter).id;
            self.owner = waiter_id;
            wake_thread_locked(waiter);
        } else {
            self.owner = 0;
            unregister_active_mutex_locked(self as *mut Mutex);
        }
    }

    /// Releases the mutex, transferring ownership directly to the highest-priority waiter
    /// if one exists, or marking the mutex free (owner = 0).
    ///
    /// Invariants:
    /// - M3: Caller must be the current owner. Non-owner unlock panics.
    /// - Unlocked unlock: Panics if mutex is not locked.
    /// - Direct handoff: If waiters exist, pops highest-priority waiter, sets owner = waiter.id,
    ///   and wakes waiter into Ready state.
    #[inline(never)]
    pub fn unlock(&mut self) {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let current = current_thread_from_gs();
        let current_id = unsafe { (*current).id };

        // Failure Semantics validation
        if let Err(err) = self.validate_unlock(current_id) {
            match err {
                UnlockError::NotLocked => {
                    panic!("Invalid operation: attempted to unlock an unlocked mutex");
                }
                UnlockError::NotOwner => {
                    panic!(
                        "Security violation: mutex unlock attempted by non-owner (owner={}, caller={})",
                        self.owner, current_id
                    );
                }
            }
        }

        unsafe {
            self.unlock_locked();
            SCHEDULER.lock.unlock_restore(orig_rflags);
        }
    }

    /// Subsystem-specific cancellation: cleanly detaches a waiter under SCHEDULER.lock.
    pub unsafe fn cancel_waiter_locked(&mut self, thread: *mut crate::task::thread::KernelThread) -> bool {
        assert!(SCHEDULER.lock.is_locked(), "cancel_waiter_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "cancel_waiter_locked requires IF=0");
        self.waiters.remove_locked(thread)
    }

    /// Subsystem-specific cancellation: handles terminating mutex owner under SCHEDULER.lock.
    /// Ensures that a waiter receiving ownership cannot simultaneously be cancelled
    /// as part of the same process-termination sweep.
    pub unsafe fn cancel_owner_locked(&mut self, terminating_pid: u64) {
        assert!(SCHEDULER.lock.is_locked(), "cancel_owner_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "cancel_owner_locked requires IF=0");
        if self.owner == 0 {
            return;
        }

        let owner_thread = crate::task::thread::get_thread_by_id(self.owner);
        if let Some(t) = owner_thread {
            if (*t).process_id == terminating_pid {
                let mut new_owner = 0;
                while let Some(waiter) = self.waiters.pop_highest_locked() {
                    if (*waiter).process_id == terminating_pid {
                        // This waiter belongs to the terminating process as well; cancel it and do not grant ownership
                        (*waiter).state = crate::task::thread::ThreadState::Zombie;
                        (*waiter).completion_event.signal_locked();
                        if (*waiter).is_detached && !(*waiter).zombie_queued {
                            crate::task::lifecycle::enqueue_zombie_locked(waiter);
                        }
                    } else {
                        // Valid external waiter found: hand off ownership directly
                        new_owner = (*waiter).id;
                        self.owner = new_owner;
                        wake_thread_locked(waiter);
                        break;
                    }
                }
                if new_owner == 0 {
                    self.owner = 0;
                    unregister_active_mutex_locked(self as *mut Mutex);
                }
            }
        }
    }
}

pub static mut ACTIVE_MUTEXES: [*mut Mutex; 16] = [core::ptr::null_mut(); 16];

pub unsafe fn register_active_mutex_locked(m: *mut Mutex) {
    for slot in ACTIVE_MUTEXES.iter_mut() {
        if *slot == m {
            return;
        }
    }
    for slot in ACTIVE_MUTEXES.iter_mut() {
        if slot.is_null() {
            *slot = m;
            return;
        }
    }
}

pub unsafe fn unregister_active_mutex_locked(m: *mut Mutex) {
    for slot in ACTIVE_MUTEXES.iter_mut() {
        if *slot == m {
            *slot = core::ptr::null_mut();
            return;
        }
    }
}

pub unsafe fn cancel_all_mutexes_for_process_locked(terminating_pid: u64) {
    assert!(SCHEDULER.lock.is_locked(), "cancel_all_mutexes_for_process_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "cancel_all_mutexes_for_process_locked requires IF=0");

    for slot in ACTIVE_MUTEXES.iter_mut() {
        if !slot.is_null() {
            let m = *slot;
            (*m).cancel_owner_locked(terminating_pid);
            if (*m).owner == 0 {
                *slot = core::ptr::null_mut();
            }
        }
    }
}

