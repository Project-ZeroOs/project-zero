# ADR-0013: Kernel Event Notification and Completion Primitive (Stage 3D — Increment 5)

## Status
**ACCEPTED & FROZEN** (Stage 3D Increment 5 Architecture Frozen)

## Date
2026-09-15

---

## 1. Context & Architectural Motivation

Project Zero Stage 3D Increments 1–4 established the core blocking and synchronization foundation:
- **Increment 1**: Zero-allocation intrusive `WaitQueue` and `commit_block_and_switch`.
- **Increment 2**: Single-owner kernel `Mutex` with direct ownership handoff.
- **Increment 3**: Predicate-coupled `Condvar` with atomic wait registration and mutex release.
- **Increment 4**: Timer-driven voluntary delay `sleep_ms` backed by `SleepTable`.

However, two fundamental synchronization capabilities remained missing in the execution substrate:
1. **Ownerless Asynchronous Signaling**: Mutex unlock is strictly bound to the owner thread (`owner == caller_id`), and Condvar requires an associated Mutex protecting the predicate. The kernel requires a synchronization primitive where the waker has no ownership restrictions (allowing worker threads, timer callbacks, or hardware ISRs to signal) and where signaling has persistent state (avoiding lost wakeups if the signal occurs before the waiter blocks).
2. **Broadcast Completion Latching**: A mechanism allowing one or more threads to wait on the occurrence of an event (such as thread completion or hardware I/O completion) where, once signaled, all current and future waiters are unblocked until an explicit reset.

---

## 2. Architectural Decision

Stage 3D Increment 5 introduces the **`Event`** primitive: a 32-byte, dual-mode, zero-allocation kernel event notification and completion primitive.

### 2.1 Mode Definitions
```rust
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
```

### 2.2 Data Structure Layout (32 Bytes, 8-Byte Aligned)
```rust
#[repr(C)]
pub struct Event {
    signaled: bool,              // Offset 0: 1 byte
    event_type: EventType,        // Offset 1: 1 byte
    _pad: [u8; 6],               // Offset 2..8: 6 bytes alignment padding
    waiters: WaitQueue,          // Offset 8..32: 24 bytes (head, tail, count)
}
```

---

## 3. Execution Context Boundaries & Invariants

### 3.1 Strict Context Boundaries
To prevent race conditions and invalid stack transitions, operations on `Event` are strictly partitioned:

* **Thread Context Only (`nested_irq_count == 0`)**:
  - `wait(&mut self) -> bool`: Blocks current thread if unsignaled.
  - `try_wait(&mut self) -> bool`: Non-blocking check/consumption.
  - `is_signaled(&self) -> bool`: Read-only state inspection.
  - `waiter_count(&self) -> usize`: Waiter count inspection.
  *Violation*: Panics deterministically if invoked from an interrupt handler (`nested_irq_count > 0`).

* **ISR-Safe & Thread-Safe (`nested_irq_count >= 0`)**:
  - `signal(&mut self)`: Unblocks waiting thread(s) or latches signaled state.
  - `reset(&mut self)`: Clears signaled state to false.
  *Contract*: Operates strictly under `SCHEDULER.lock` with `IF=0`. Does not block and does not initiate a context transfer.

### 3.2 Permanent Cross-Cutting Invariants
1. **INV-E1 (Lock Serialization)**: All state transitions (`signaled`, `event_type`, `waiters`) are serialized strictly under `SCHEDULER.lock` with `IF=0`.
2. **INV-E2 (Saved-Frame Invariant)**: In `wait()`, `(*current).frame_type = SavedFrameType::Cooperative` is explicitly stamped under `SCHEDULER.lock` before calling `commit_block_and_switch()`.
3. **INV-E3 (Detachment Invariant)**: Threads popped from `event.waiters` during `signal()` have `thread.next_waiter == NULL` guaranteed prior to calling `wake_thread_locked()`.
4. **INV-E4 (Auto-Reset Single Consumer)**: An `AutoReset` event unblocks at most one thread per `signal()`. If no threads are waiting, it latches at most one future `wait()` or `try_wait()`.
5. **INV-E5 (Manual-Reset Broadcast)**: A `ManualReset` event unblocks all current waiters upon `signal()` and allows all subsequent callers of `wait()` to proceed until explicitly reset.
6. **INV-E6 (Zero Dynamic Allocation)**: `Event` requires zero heap allocation.
7. **INV-E7 (Deferred Preemption)**: When `signal()` wakes a higher-priority thread, it enqueues it into `SCHEDULER.runqueues` and asserts `need_resched = 1`. Preemption occurs exclusively at permitted scheduling points (voluntary yield, block, or timer interrupt).
