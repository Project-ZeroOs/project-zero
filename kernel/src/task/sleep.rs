//! Stage 3D Increment 4: Timer Sleep Subsystem (`sleep_ms` & `SleepTable`).
//!
//! Authoritative Contract: ADR-0012 Revision 3 & Stage 3D Increment 4 Architecture Revision.
//!
//! Invariants:
//! - Sleeping threads are modeled strictly as `ThreadState::Blocked` on `SLEEP_WAIT_QUEUE`.
//! - Bounded static storage `SLEEP_TABLE[MAX_THREADS]` with zero dynamic heap allocation.
//! - `SCHEDULER.lock` authoritatively serializes `SLEEP_TABLE` and `SLEEP_WAIT_QUEUE`.
//! - Authoritative clock: LAPIC periodic timer at calibrated 100 Hz (1 tick = 10 ms).
//! - Deadline calculation: `TICKS` sampled under `SCHEDULER.lock`, `ceil(ms / 10)` overflow-safe division, `saturating_add`.
//! - Expiration occurs strictly under `SCHEDULER.lock` with clean `WaitQueue` detachment before `wake_thread_locked()`.

use core::sync::atomic::Ordering;
use crate::task::scheduler::{
    commit_block_and_switch, cpu_if_bit, prepare_block_locked,
    wake_thread_locked, yield_now, SCHEDULER,
};
use crate::task::thread::{KernelThread, ThreadState, MAX_THREADS};
use crate::task::waitqueue::WaitQueue;
use crate::task::percpu::{self, current_thread_from_gs};

/// Entry in the static sleep table tracking an active sleeping thread and its deadline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SleepEntry {
    pub thread: *mut KernelThread,
    pub deadline_tick: u64,
}

pub const MAX_SLEEP_ENTRIES: usize = MAX_THREADS; // 16

/// Statically bounded table of active sleeping threads.
pub static mut SLEEP_TABLE: [Option<SleepEntry>; MAX_SLEEP_ENTRIES] = [None; MAX_SLEEP_ENTRIES];

/// Wait queue holding all currently blocked sleeping threads.
pub static mut SLEEP_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// Voluntarily sleeps the current running thread for at least `ms` milliseconds.
///
/// Semantics:
/// - `sleep_ms(0)`: Immediately yields timeslice via `yield_now()` without registering in `SLEEP_TABLE` or `WaitQueue`. Returns `true`.
/// - `ms > 0`: Blocks the calling thread until `TICKS >= deadline`, where `deadline = now + ceil(ms / 10)`.
///   - Authoritative `TICKS` is sampled strictly under `SCHEDULER.lock` with `IF == 0`.
///   - Conversion uses overflow-safe ceiling division: `(ms / 10) + if ms % 10 != 0 { 1 } else { 0 }`.
///   - Deadline is computed using saturating arithmetic: `now.saturating_add(duration_ticks)`.
///   - If `SLEEP_TABLE` is exhausted (all 16 slots full), returns `false` deterministically without blocking or overwriting.
///   - If duplicate registration is detected, returns `false`.
///   - Resumes execution after wakeup and returns `true`.
#[inline(never)]
pub fn sleep_ms(ms: u64) -> bool {
    if ms == 0 {
        yield_now();
        return true;
    }

    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };

    let current = current_thread_from_gs();
    assert!(!current.is_null(), "Cannot sleep null thread");
    assert_eq!(
        unsafe { (*current).state },
        ThreadState::Running,
        "Only Running thread can sleep"
    );
    assert_eq!(
        unsafe { percpu::BSP_PERCPU.nested_irq_count },
        0,
        "Cannot sleep inside an ISR"
    );

    // 1. Duplicate registration check: verify current thread is not already in SLEEP_TABLE
    for slot in unsafe { SLEEP_TABLE.iter() } {
        if let Some(entry) = slot {
            if entry.thread == current {
                unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
                return false;
            }
        }
    }

    // 2. Find free slot in SLEEP_TABLE
    let mut free_slot_idx = None;
    for (i, slot) in unsafe { SLEEP_TABLE.iter().enumerate() } {
        if slot.is_none() {
            free_slot_idx = Some(i);
            break;
        }
    }

    let slot_idx = match free_slot_idx {
        Some(idx) => idx,
        None => {
            // SleepTable exhausted: fail deterministically without modifying table or thread state
            unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
            return false;
        }
    };

    // 3. Sample authoritative TICKS under SCHEDULER.lock and compute deadline
    let now = crate::hal::arch::x86_64::timer::TICKS.load(Ordering::Relaxed);
    let duration_ticks = (ms / 10) + if ms % 10 != 0 { 1 } else { 0 };
    let deadline = now.saturating_add(duration_ticks);

    // 4. Register SleepTable entry
    unsafe {
        SLEEP_TABLE[slot_idx] = Some(SleepEntry {
            thread: current,
            deadline_tick: deadline,
        });
    }

    // 5. Prepare block and link to SLEEP_WAIT_QUEUE
    let (outgoing, prev_rsp, next_rsp, target_frame) = unsafe {
        prepare_block_locked(&mut SLEEP_WAIT_QUEUE)
    };

    // 6. Terminal context switch (releases SCHEDULER.lock keeping IF=0)
    unsafe {
        commit_block_and_switch(outgoing, prev_rsp, next_rsp, orig_rflags, target_frame);
    }

    // Resumed after expiration and reschedule
    true
}

/// Scans SLEEP_TABLE in table order and wakes any entry whose deadline has arrived.
///
/// Preconditions:
///   - SCHEDULER.lock HELD
///   - CPU IF == 0
///
/// Postconditions:
///   - SCHEDULER.lock remains HELD
///   - CPU IF remains 0
///   - Expired threads cleanly detached from `SLEEP_WAIT_QUEUE` before `wake_thread_locked()`
///   - Expired slots cleared to `None` in `SLEEP_TABLE`
pub(crate) unsafe fn expire_sleepers_locked(now_ticks: u64) -> usize {
    assert!(
        SCHEDULER.lock.is_locked(),
        "expire_sleepers_locked requires SCHEDULER.lock held"
    );
    assert_eq!(cpu_if_bit(), 0, "expire_sleepers_locked requires IF=0");

    let mut woken_count = 0;
    for slot in SLEEP_TABLE.iter_mut() {
        if let Some(entry) = *slot {
            if now_ticks >= entry.deadline_tick {
                let thread = entry.thread;
                assert!(!thread.is_null());
                assert_eq!((*thread).state, ThreadState::Blocked);

                // 1. Detach from SLEEP_WAIT_QUEUE first (WaitQueue Detachment Contract)
                let detached = SLEEP_WAIT_QUEUE.remove_locked(thread);
                assert!(
                    detached,
                    "Sleeping thread must be present in SLEEP_WAIT_QUEUE"
                );
                assert!(
                    (*thread).next_waiter.is_null(),
                    "Detached sleeper must have null next_waiter"
                );

                // 2. Clear SLEEP_TABLE slot
                *slot = None;

                // 3. Wake thread: transitions Blocked -> Ready, enqueues to RunQueue,
                // and sets percpu.need_resched = 1 if thread.priority > current.priority.
                wake_thread_locked(thread);
                woken_count += 1;
            }
        }
    }
    woken_count
}

/// Subsystem-specific cancellation: cleanly removes a sleeping thread from SLEEP_TABLE
/// and SLEEP_WAIT_QUEUE under SCHEDULER.lock without waking it to Ready.
pub unsafe fn cancel_sleep_locked(thread: *mut KernelThread) -> bool {
    assert!(
        SCHEDULER.lock.is_locked(),
        "cancel_sleep_locked requires SCHEDULER.lock held"
    );
    assert_eq!(cpu_if_bit(), 0, "cancel_sleep_locked requires IF=0");

    let mut found = false;
    for slot in SLEEP_TABLE.iter_mut() {
        if let Some(entry) = slot {
            if entry.thread == thread {
                *slot = None;
                found = true;
                break;
            }
        }
    }
    if found {
        SLEEP_WAIT_QUEUE.remove_locked(thread);
    }
    found
}
