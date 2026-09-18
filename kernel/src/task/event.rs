//! Project Zero - Stage 3D Increment 5: Kernel Event Notification & Completion Primitive
//!
//! Authoritative Contract: ADR-0013 & Stage 3D Increment 5 Architecture Rev2.
//!
//! Invariants:
//! - Dual-mode semantics: AutoReset (Synchronization / Doorbell) and ManualReset (Notification / Completion Latch).
//! - Statically bounded, zero dynamic heap allocation. Size = 32 bytes, 8-byte aligned.
//! - Strict execution context boundaries: wait(), try_wait(), is_signaled(), waiter_count() are thread-only;
//!   signal() and reset() are ISR-safe and thread-safe.
//! - SAVED-FRAME INVARIANT: wait() explicitly stamps `SavedFrameType::Cooperative` via `prepare_block_locked()`.
//! - Clean detachment invariant: popped waiters have `next_waiter == NULL` before `wake_thread_locked()`.
//! - Preemption preservation: woken higher-priority waiters assert `need_resched` and dispatch at permitted scheduling points.

use crate::task::waitqueue::WaitQueue;
use crate::task::percpu::{self, current_thread_from_gs};
use crate::task::scheduler::{
    commit_block_and_switch, cpu_if_bit, prepare_block_locked, wake_thread_locked, SCHEDULER,
};

/// Operating mode of the Event primitive.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    /// Auto-Reset (Synchronization Event / Doorbell):
    /// When signaled, atomically wakes exactly ONE waiter and resets to Unsignaled.
    /// If no waiters are present, remains Signaled until consumed by the next wait() or try_wait().
    AutoReset = 0,

    /// Manual-Reset (Notification / Completion Latch):
    /// When signaled, wakes ALL current waiters and remains Signaled.
    /// Future wait() calls pass through immediately without blocking until explicitly reset().
    ManualReset = 1,
}

/// Zero-allocation, dual-mode kernel Event primitive (32 bytes, 8-byte aligned).
#[repr(C)]
pub struct Event {
    pub(crate) signaled: bool,
    pub(crate) event_type: EventType,
    _pad: [u8; 6],
    pub(crate) waiters: WaitQueue,
}

unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    /// Creates a new Event with specified initial state and reset mode.
    pub const fn new(signaled: bool, event_type: EventType) -> Self {
        Self {
            signaled,
            event_type,
            _pad: [0; 6],
            waiters: WaitQueue::new(),
        }
    }

    /// Blocks the current thread until the event becomes signaled.
    ///
    /// Execution Context: THREAD CONTEXT ONLY (`nested_irq_count == 0`).
    /// Fails closed (panics) if called inside an ISR.
    ///
    /// Invariants:
    /// - Serialized strictly under `SCHEDULER.lock`.
    /// - Preserves `SAVED-FRAME INVARIANT`: sets `frame_type = SavedFrameType::Cooperative`.
    #[inline(never)]
    pub fn wait(&mut self) -> bool {
        assert_eq!(
            unsafe { percpu::BSP_PERCPU.nested_irq_count },
            0,
            "Cannot wait on Event inside an ISR"
        );

        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };

        if self.signaled {
            if self.event_type == EventType::AutoReset {
                self.signaled = false;
            }
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            return true;
        }

        unsafe {
            // Atomically enqueue on wait queue, set state to Blocked, and stamp frame_type = Cooperative
            let (out_thread, prev_rsp, next_rsp, target_frame) = prepare_block_locked(&mut self.waiters);

            // Execute terminal context switch (releases SCHEDULER.lock keeping IF=0)
            commit_block_and_switch(out_thread, prev_rsp, next_rsp, orig_rflags, target_frame);

            // Resumes here after another context signals the event and wakes this thread
        }

        true
    }

    /// Non-blocking check of the event state.
    /// If signaled, consumes the signal (if AutoReset) and returns true.
    /// If unsignaled, returns false immediately without blocking.
    ///
    /// Execution Context: THREAD CONTEXT ONLY (`nested_irq_count == 0`).
    #[inline(never)]
    pub fn try_wait(&mut self) -> bool {
        assert_eq!(
            unsafe { percpu::BSP_PERCPU.nested_irq_count },
            0,
            "Cannot try_wait on Event inside an ISR"
        );

        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };

        if self.signaled {
            if self.event_type == EventType::AutoReset {
                self.signaled = false;
            }
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            true
        } else {
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            false
        }
    }

    /// Internal signal primitive executing under an ALREADY HELD `SCHEDULER.lock`.
    ///
    /// Preconditions:
    /// - `SCHEDULER.lock` is held by the calling context.
    /// - CPU IF == 0.
    ///
    /// Semantics:
    /// - Executes full signal state transition and waiter wake logic.
    /// - Wakes highest-priority waiter (AutoReset) or all waiters (ManualReset).
    /// - Asserts waiter detachment invariant (`next_waiter == NULL`) before `wake_thread_locked()`.
    /// - Does NOT acquire, release, or restore `SCHEDULER.lock`.
    #[inline(never)]
    pub(crate) unsafe fn signal_locked(&mut self) {
        assert!(SCHEDULER.lock.is_locked(), "signal_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "signal_locked requires IF=0");

        match self.event_type {
            EventType::AutoReset => {
                // If waiters exist, wake exactly ONE (highest priority) and keep event unsignaled
                if let Some(waiter) = self.waiters.pop_highest_locked() {
                    assert!(
                        (*waiter).next_waiter.is_null(),
                        "Popped waiter must be cleanly detached"
                    );
                    wake_thread_locked(waiter);
                    self.signaled = false;
                } else {
                    // No waiters: latch signaled state for the next single consumer
                    self.signaled = true;
                }
            }
            EventType::ManualReset => {
                // Latch signaled state
                self.signaled = true;
                // Wake ALL currently blocked waiters in priority order
                while let Some(waiter) = self.waiters.pop_highest_locked() {
                    assert!(
                        (*waiter).next_waiter.is_null(),
                        "Popped waiter must be cleanly detached"
                    );
                    wake_thread_locked(waiter);
                }
            }
        }
    }

    /// Signals the event, unblocking waiting thread(s).
    ///
    /// Execution Context: ISR-SAFE & THREAD-SAFE.
    /// Operates under `SCHEDULER.lock` with `IF=0`. Does not block and does not switch context.
    /// Woken threads enter the scheduler runqueues; if a woken thread has higher priority
    /// than current, `need_resched` is asserted for dispatch at the next permitted scheduling point.
    #[inline(never)]
    pub fn signal(&mut self) {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        unsafe { self.signal_locked() };
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
    }

    /// Manually resets the event to unsignaled.
    ///
    /// Execution Context: ISR-SAFE & THREAD-SAFE.
    #[inline(never)]
    pub fn reset(&mut self) {
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        self.signaled = false;
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
    }

    /// Read-only inspection of the signaled state.
    ///
    /// Execution Context: THREAD CONTEXT ONLY (`nested_irq_count == 0`).
    #[inline(never)]
    pub fn is_signaled(&self) -> bool {
        assert_eq!(
            unsafe { percpu::BSP_PERCPU.nested_irq_count },
            0,
            "Cannot call is_signaled on Event inside an ISR"
        );
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let state = self.signaled;
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        state
    }

    /// Returns the number of threads currently blocked on this event.
    ///
    /// Execution Context: THREAD CONTEXT ONLY (`nested_irq_count == 0`).
    #[inline(never)]
    pub fn waiter_count(&self) -> usize {
        assert_eq!(
            unsafe { percpu::BSP_PERCPU.nested_irq_count },
            0,
            "Cannot call waiter_count on Event inside an ISR"
        );
        let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
        let count = self.waiters.len();
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        count
    }

    /// Subsystem-specific cancellation: cleanly detaches a waiter under SCHEDULER.lock.
    pub unsafe fn cancel_waiter_locked(&mut self, thread: *mut crate::task::thread::KernelThread) -> bool {
        assert!(SCHEDULER.lock.is_locked(), "cancel_waiter_locked requires SCHEDULER.lock held");
        assert_eq!(cpu_if_bit(), 0, "cancel_waiter_locked requires IF=0");
        self.waiters.remove_locked(thread)
    }
}
