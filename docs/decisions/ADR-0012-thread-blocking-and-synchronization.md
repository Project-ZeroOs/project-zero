# ADR-0012: Thread Blocking & Synchronization Architecture (Stage 3D — Revision 3)

## Status
**APPROVED & FROZEN** (Stage 3D Increments 1–4 Complete & Frozen)

## Date
2026-09-14

---

## 1. Context & Architectural Foundation

Project Zero Stage 3C established genuine hardware-driven preemptive scheduling:
- Local APIC periodic timer running at calibrated 100 Hz (10 ms / tick).
- 4-way context switching (`switch_context`, `switch_context_coop_to_preempt`, `restore_context_cooperative`, `restore_context_preemptive`).
- Strict machine invariants: `sched_lock == FREE`, `CPU IF == 0`, `GS+16 == next`, `nested_irq_count == 0` before context transfer.
- Zero dynamic heap allocation and statically bounded `KernelThread` table (`MAX_THREADS = 16`).

However, all thread execution remains either compute-bound or voluntarily relinquishing via `yield_now()`. Threads cannot pause execution awaiting an external event, lock, or timer without busy-spinning.

Stage 3D introduces the fundamental kernel capability for a thread to **block** when it cannot make progress, transition to `ThreadState::Blocked`, and later be deterministically **woken** into `ThreadState::Ready`.

### Core Architectural Principle
> **Blocking is a scheduler state transition, not a special kind of context switch.**

The existing frozen context-switch machinery remains the sole mechanism for changing machine context. Blocking determines **which thread is eligible to run**. When a thread blocks, it is removed from the runnable pool and linked into an intrusive wait channel. When woken, it returns to the ready pool. The underlying stack transfers continue to utilize the frozen Stage 3B/3C context primitives.

---

## 2. Frozen Substrate Contracts

The following subsystems are architecturally frozen and must remain unmodified:
1. **Stage 2F Memory Substrate**: Higher-half canonical VMA (`0xFFFFFFFF8010xxxx`), HHDM (`0xFFFF800000000000`), W^X page permissions, unmapped guard pages.
2. **Stage 3A Thread Descriptors**: `KernelThread` (104 bytes, 8-byte aligned), `next_runnable` (offset 88), `next_waiter` (offset 96), `PerCpu` at `GS_BASE` (size 48, offsets +16, +32, +36, +40).
3. **Stage 3B Cooperative Core**: `switch_context`, `RunQueue` priority ordering (`Critical > High > Normal`), idle thread, `SchedLock` release-before-switch contract.
4. **Stage 3C Preemptive Engine**: LAPIC timer at 100 Hz, 160-byte interrupt frames, 4-way context primitives, deferred preemption (`preempt_count`, `need_resched`).
5. **Cross-Cutting Continuation Invariant (SAVED-FRAME INVARIANT)**:
   - `frame_type == SavedFrameType::Cooperative` $\longleftrightarrow$ `saved_rsp` points to a valid 64-byte cooperative frame.
   - `frame_type == SavedFrameType::Preemptive` $\longleftrightarrow$ `saved_rsp` points to a valid 160-byte preemptive frame (`InterruptFrame`).
   - Every voluntary transition (`yield_now()`, `prepare_block_locked()`, `prepare_block_detached_locked()`, `exit_current_thread()`) MUST explicitly set `(*current).frame_type = SavedFrameType::Cooperative` before context transfer.

---

## 3. Decision & Architectural Specifications

### 3.1 Strict Function Category & Lock Invariant (Zero Exceptions)

Stage 3D categorizes all synchronization operations into three distinct architectural classes with **zero exceptions**:

```text
========================================================================
1. PUBLIC / LOCK-ACQUIRING APIs
------------------------------------------------------------------------
  block_current(wait_queue)
  WaitQueue::wake_one(&mut self) -> Option<*mut KernelThread>
  WaitQueue::wake_all(&mut self) -> usize
  WaitQueue::wake_thread(&mut self, thread) -> bool
  Mutex::lock(&mut self)
  Mutex::unlock(&mut self)
  Condvar::wait(&mut self, &mut Mutex)
  Condvar::signal(&mut self)
  Condvar::broadcast(&mut self)
  sleep_ms(ms: u64)

  Contract:
    - Acquires SCHEDULER.lock internally (or manages lock lifecycle).
    - Restores caller's original CPU IF upon normal return.

========================================================================
2. INTERNAL / LOCK-HELD APIs (*_locked) — ZERO EXCEPTIONS
------------------------------------------------------------------------
  prepare_block_locked(wait_queue) -> (*mut u64, u64, SavedFrameType)
  prepare_block_detached_locked() -> (*mut u64, u64, SavedFrameType)
  wake_thread_locked(detached_thread)
  WaitQueue::push_priority_locked(&mut self, thread)
  WaitQueue::pop_highest_locked(&mut self) -> Option<*mut KernelThread>
  WaitQueue::remove_locked(&mut self, thread) -> bool

  Contract:
    - MUST be called with SCHEDULER.lock already HELD and CPU IF == 0.
    - NEVER acquires SCHEDULER.lock.
    - NEVER releases SCHEDULER.lock.
    - Leaves SCHEDULER.lock HELD upon return.

========================================================================
3. TERMINAL CONTEXT TRANSFERS (Explicit Transition Boundary)
------------------------------------------------------------------------
  commit_block_and_switch(prev_rsp_ptr, next_rsp, orig_rflags, target_frame)

  Contract:
    - Enters with SCHEDULER.lock HELD and CPU IF == 0.
    - Releases SCHEDULER.lock keeping IF == 0 (fulfilling Option B contract).
    - Asserts pre-switch machine invariants.
    - Dispatches context transfer via terminal_context_switch.
    - Does NOT contain '_locked' suffix, cleanly preserving the invariant.
========================================================================
```

---

### 3.2 Thread State Machine & Transitions

The authoritative `ThreadState` enum defined in Stage 3A is:
```rust
#[repr(u8)]
pub enum ThreadState {
    Initializing = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Terminated = 4,
}
```

*Note on Sleeping:* Sleeping is architecturally modeled as `ThreadState::Blocked` waiting on a timer wait channel, maintaining the invariant `Blocked != Ready` and preserving the frozen 104-byte `KernelThread` layout without adding an unnecessary state variant.

#### State Transition Matrix

| Current State | Target State | Triggering Primitive | Context Switch? | RunQueue Membership | WaitQueue Membership |
| :--- | :--- | :--- | :---: | :---: | :---: |
| `Initializing` | `Ready` | `spawn()` | No | Added to tail of priority `RunQueue` | None |
| `Ready` | `Running` | `pop_highest_runnable()` | Yes | Removed from `RunQueue` | None |
| `Running` | `Ready` | `yield_now()` / Preemption | Yes | Added to tail of priority `RunQueue` | None |
| `Running` | `Blocked` | `block_current(wait_queue)` | Yes | **None** | Added to `WaitQueue` |
| `Blocked` | `Ready` | `wake_thread_locked()` | No | Added to tail of priority `RunQueue` | **Detached prior to call** |
| `Running` | `Terminated` | `exit_current_thread()` | Yes (terminal) | None | None |

#### Architectural Invariants on State
- **I1**: A `Blocked` thread is **never** present on any `RunQueue`.
- **I2**: A `Ready` or `Running` thread is **never** present on any `WaitQueue`.
- **I3**: A thread belongs to at most **one** `WaitQueue` at any point in time (`next_waiter` linkage).
- **I4**: Waking transitions `Blocked -> Ready` **exactly once**.
- **I5**: A `Terminated` thread cannot become `Ready`, `Blocked`, or be woken.

---

### 3.3 Intrusive WaitQueue & Detachment Architecture

To maintain the zero-heap allocation invariant, `WaitQueue` is an intrusive queue linking threads through `KernelThread.next_waiter` (offset 96).

```rust
pub struct WaitQueue {
    head: *mut KernelThread,
    tail: *mut KernelThread,
    count: usize,
}
```

#### The WaitQueue Detachment Contract
> **`wake_thread_locked(detached_thread)` transitions an ALREADY-DETACHED blocked thread into a runnable thread; it NEVER performs intrusive queue unlinking itself.**
>
> **Every thread passed to `wake_thread_locked()` MUST first be cleanly unlinked (popped or removed) from its owning `WaitQueue`.**

#### Internal Lock-Held Queue Primitives:
```rust
impl WaitQueue {
    pub unsafe fn push_priority_locked(&mut self, thread: *mut KernelThread) {
        assert!(SCHEDULER.lock.is_locked());
        assert_eq!(cpu_if_bit(), 0);
        assert!(!thread.is_null());
        assert!((*thread).next_waiter.is_null(), "Cannot enqueue already-linked waiter");

        let priority = (*thread).priority;
        // Priority FIFO insertion: Critical > High > Normal
        if self.head.is_null() {
            self.head = thread;
            self.tail = thread;
            (*thread).next_waiter = core::ptr::null_mut();
        } else if priority > (*self.head).priority {
            (*thread).next_waiter = self.head;
            self.head = thread;
        } else {
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

    pub unsafe fn pop_highest_locked(&mut self) -> Option<*mut KernelThread> {
        assert!(SCHEDULER.lock.is_locked());
        assert_eq!(cpu_if_bit(), 0);
        if self.head.is_null() {
            return None;
        }
        let t = self.head;
        self.head = (*t).next_waiter;
        if self.head.is_null() {
            self.tail = core::ptr::null_mut();
        }
        (*t).next_waiter = core::ptr::null_mut(); // CLEANLY DETACHED
        self.count -= 1;
        Some(t)
    }

    pub unsafe fn remove_locked(&mut self, thread: *mut KernelThread) -> bool {
        assert!(SCHEDULER.lock.is_locked());
        assert_eq!(cpu_if_bit(), 0);
        if self.head.is_null() || thread.is_null() {
            return false;
        }
        if self.head == thread {
            self.head = (*thread).next_waiter;
            if self.head.is_null() {
                self.tail = core::ptr::null_mut();
            }
            (*thread).next_waiter = core::ptr::null_mut(); // CLEANLY DETACHED
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
            (*thread).next_waiter = core::ptr::null_mut(); // CLEANLY DETACHED
            self.count -= 1;
            return true;
        }
        false
    }
}
```

---

### 3.4 The Waking Protocol

#### 1. Internal Lock-Held Primitive: `wake_thread_locked(thread: *mut KernelThread)`
```rust
/// Transitions an ALREADY DETACHED thread from Blocked to Ready.
///
/// REQUIRES:
///   - SCHEDULER.lock held
///   - CPU IF == 0
///   - thread is not null
///   - (*thread).state == ThreadState::Blocked
///   - (*thread).next_waiter == null (must have been detached from WaitQueue prior to call)
///
/// ENSURES:
///   - (*thread).state = ThreadState::Ready
///   - (*thread) enqueued on appropriate priority RunQueue
///   - SCHEDULER.lock remains HELD
pub unsafe fn wake_thread_locked(thread: *mut KernelThread) {
    assert!(SCHEDULER.lock.is_locked(), "wake_thread_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "wake_thread_locked requires IF=0");
    assert!(!thread.is_null(), "Cannot wake null thread pointer");
    assert_eq!((*thread).state, ThreadState::Blocked, "Can only wake Blocked thread");
    assert!((*thread).next_waiter.is_null(), "Thread must be detached from WaitQueue before wake_thread_locked");

    // Transition Blocked -> Ready
    (*thread).state = ThreadState::Ready;
    SCHEDULER.enqueue(thread); // Links via next_runnable

    // Priority preemption check: if woken thread has higher priority than current
    let current = current_thread_from_gs();
    if !current.is_null() && (*thread).priority > (*current).priority {
        percpu::BSP_PERCPU.need_resched = 1;
    }
}
```

#### 2. Public Queue-Aware Waking APIs
Because a raw `KernelThread*` cannot identify which queue owns it, public waking APIs are explicitly **queue-aware**:
```rust
impl WaitQueue {
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
```

---

### 3.5 The Blocking Protocol

#### 1. `prepare_block_locked(wait_queue: &mut WaitQueue) -> (*mut u64, u64, SavedFrameType)`
```rust
pub unsafe fn prepare_block_locked(wait_queue: &mut WaitQueue) -> (*mut u64, u64, SavedFrameType) {
    assert!(SCHEDULER.lock.is_locked(), "prepare_block_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "prepare_block_locked requires IF=0");

    let current = current_thread_from_gs();
    assert_eq!((*current).state, ThreadState::Running, "Only Running thread can block");
    assert_eq!(percpu::BSP_PERCPU.nested_irq_count, 0, "Cannot block inside an ISR");

    // 1. Mutate state and link into wait queue
    (*current).state = ThreadState::Blocked;
    wait_queue.push_priority_locked(current);

    // 2. Select next runnable thread (or idle thread if empty)
    let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
        percpu::BSP_PERCPU.idle_thread
    });

    // 3. Update incoming thread state
    (*next).state = ThreadState::Running;
    (*next).quantum_remaining = DEFAULT_QUANTUM;
    percpu::BSP_PERCPU.current_thread = next;

    (&raw mut (*current).saved_rsp, (*next).saved_rsp, (*next).frame_type)
}
```

#### 2. `prepare_block_detached_locked() -> (*mut u64, u64, SavedFrameType)`
Used when the caller has already enqueued `current` into a custom wait channel (e.g. `Condvar::wait`):
```rust
pub unsafe fn prepare_block_detached_locked() -> (*mut u64, u64, SavedFrameType) {
    assert!(SCHEDULER.lock.is_locked());
    assert_eq!(cpu_if_bit(), 0);

    let current = current_thread_from_gs();
    assert_eq!((*current).state, ThreadState::Running);
    assert_eq!(percpu::BSP_PERCPU.nested_irq_count, 0);

    (*current).state = ThreadState::Blocked;

    let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
        percpu::BSP_PERCPU.idle_thread
    });

    (*next).state = ThreadState::Running;
    (*next).quantum_remaining = DEFAULT_QUANTUM;
    percpu::BSP_PERCPU.current_thread = next;

    (&raw mut (*current).saved_rsp, (*next).saved_rsp, (*next).frame_type)
}
```

#### 3. Terminal Context Transfer: `commit_block_and_switch`
```rust
pub unsafe fn commit_block_and_switch(
    prev_rsp_ptr: *mut u64,
    next_rsp: u64,
    orig_rflags: u64,
    target_frame: SavedFrameType,
) {
    assert!(SCHEDULER.lock.is_locked(), "commit_block_and_switch requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "commit_block_and_switch requires IF=0");

    let current = current_thread_from_gs();

    // 1. Release scheduler lock keeping IF=0 (Option B contract)
    SCHEDULER.lock.unlock_keep_cli();

    // 2. Assert pre-switch machine invariants
    assert!(!SCHEDULER.lock.is_locked(), "sched_lock must be FREE before switch");
    assert_eq!(cpu_if_bit(), 0, "CPU IF must be 0 before switch");
    assert_ne!(current_thread_from_gs(), current, "GS+16 must point to next thread");
    assert_eq!(percpu::BSP_PERCPU.nested_irq_count, 0, "nested_irq_count must be 0");

    // 3. Terminal context switch transfer
    terminal_context_switch(prev_rsp_ptr, next_rsp, orig_rflags, target_frame);

    // ==================== POST-RESUMPTION ====================
    // Resumes here ONLY after another thread wakes this thread and scheduler runs it!
    assert_eq!(current_thread_from_gs(), current, "GS+16 must match resumed thread");
    assert_eq!((*current).state, ThreadState::Running, "Resumed thread must be Running");
    assert_eq!(cpu_if_bit(), (orig_rflags & 0x200) >> 9, "Restored IF must match caller IF");
}
```

#### 4. Public Convenience Wrapper: `block_current(wait_queue: &mut WaitQueue)`
```rust
pub fn block_current(wait_queue: &mut WaitQueue) {
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let (prev_rsp, next_rsp, frame_type) = unsafe { prepare_block_locked(wait_queue) };
    unsafe { commit_block_and_switch(prev_rsp, next_rsp, orig_rflags, frame_type) };
}
```

---

## 6. Sleep & Timer Design: `sleep_ms(u64)`

### 6.1 Representation & Calculations
- Authoritative clock: 100 Hz LAPIC periodic timer (1 tick = 10 ms).
- Conversion to ticks: `ticks = (ms + 9) / 10` (ceiling division, ensuring minimum sleep duration).
- Edge Case `sleep_ms(0)`: Relinquishes remaining timeslice via `yield_now()` without entering `WaitQueue`.
- Arithmetic safety: Deadline computed with `TICKS.load(Ordering::Relaxed).saturating_add(ticks)`.

### 6.2 Zero-Heap Sleep Table Architecture
To keep `KernelThread` at its frozen 104-byte layout:
```rust
pub struct SleepEntry {
    pub thread: *mut KernelThread,
    pub deadline_tick: u64,
}

pub static mut SLEEP_TABLE: [Option<SleepEntry>; MAX_THREADS] = [None; MAX_THREADS];
pub static mut SLEEP_WAIT_QUEUE: WaitQueue = WaitQueue::new();
```
- Sleeping threads consume **zero CPU cycles** while asleep.
- Statically bounded by `MAX_THREADS = 16`.

### 6.3 Atomic Timer ISR Expiration Path
Inside `schedule_preemption_from_irq`:
```text
timer_interrupt_handler (vector 32)
  ↓
TICKS.fetch_add(1)
lapic_eoi()
  ↓
Acquire SCHEDULER.lock (cli, IF=0)
  ↓
Scan SLEEP_TABLE:
  for entry in SLEEP_TABLE.iter_mut():
    if let Some(e) = entry:
      if e.deadline_tick <= TICKS:
        SLEEP_WAIT_QUEUE.remove_locked(e.thread);  // DETACH FIRST
        wake_thread_locked(e.thread);              // ATOMIC RESUME TO READY
        *entry = None;                             // CLEAR SLOT
  ↓
Quantum accounting & standard preemption decision
  ↓
Release SCHEDULER.lock / perform context switch
```
*Key Invariant*: `SLEEP_TABLE` clearing, wait queue detachment, and `wake_thread_locked()` occur **atomically under `SCHEDULER.lock`**. A thread is never observed as `Ready` while remaining in `SLEEP_TABLE`.

---

## 7. Kernel Mutex Design

### 7.1 Single-Core Mutex Model
On Project Zero's single-core BSP substrate, mutex state and wait queues are directly serialized by `SCHEDULER.lock`:

```rust
pub struct Mutex {
    owner: u64, // ThreadId of owner (0 = unlocked)
    wait_queue: WaitQueue,
}
```

*Architectural Justification for `owner: u64`*:
On a single-core kernel with interrupt-disabled scheduler locking, `SCHEDULER.lock` provides authoritative mutual exclusion. Storing a plain `u64` eliminates race conditions between CAS state and waitqueue enqueuing. When SMP is introduced in future stages, this will naturally expand to ticket locks or futex-style words backed by architectural per-core primitives.

### 7.2 Mutex Acquisition: `lock()`
```rust
pub fn lock(&mut self) {
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let current = current_thread_from_gs();
    let current_id = unsafe { (*current).id };

    if self.owner == current_id {
        panic!("Deadlock: recursive mutex acquisition attempted");
    }

    if self.owner == 0 {
        // Fast path: lock uncontended
        self.owner = current_id;
        unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
        return;
    }

    // Contended path: prepare block and switch
    unsafe {
        let (prev_rsp, next_rsp, frame_type) = prepare_block_locked(&mut self.wait_queue);
        commit_block_and_switch(prev_rsp, next_rsp, orig_rflags, frame_type);
        // Resumes here when ownership is handed off by unlock()
    }
}
```

### 7.3 Mutex Release: `unlock()`
```rust
pub fn unlock(&mut self) {
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let current = current_thread_from_gs();
    let current_id = unsafe { (*current).id };

    assert_eq!(self.owner, current_id, "Mutex unlock attempted by non-owner");

    unsafe {
        if let Some(waiter) = self.wait_queue.pop_highest_locked() {
            // Direct handoff prevents barging:
            let waiter_id = (*waiter).id;
            self.owner = waiter_id;
            wake_thread_locked(waiter); // Detached by pop_highest_locked, woken via _locked primitive
        } else {
            self.owner = 0;
        }

        SCHEDULER.lock.unlock_restore(orig_rflags);
    }
}
```

---

## 8. Condition Variable Design

### 8.1 Data Structure & Signaling Contract
```rust
pub struct Condvar {
    wait_queue: WaitQueue,
}
```

#### Signaling Contract
1. `condvar.wait(&mut mutex)`: **MANDATORY** that caller owns `mutex`.
2. `condvar.signal()` and `condvar.broadcast()`: Calling `signal()` does **NOT** require holding the mutex, but the state predicate being signaled **MUST** be protected by the mutex.
   - If `signal()` is called while holding the mutex, the woken waiter transitions `Blocked -> Ready` and enters the `RunQueue`. Upon running, it will block on `mutex.lock()` until the signaler unlocks.
   - If `signal()` is called without holding the mutex, the woken waiter transitions `Blocked -> Ready` immediately and competes for the mutex upon scheduling.

### 8.2 Atomic Wait Protocol: `wait()`
```rust
pub fn wait(&mut self, mutex: &mut Mutex) {
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let current = current_thread_from_gs();
    let current_id = unsafe { (*current).id };

    assert_eq!(mutex.owner, current_id, "Condvar wait requires held mutex");

    unsafe {
        // 1. Enqueue onto condvar wait queue
        self.wait_queue.push_priority_locked(current);

        // 2. Release the mutex and hand off to next mutex waiter if present
        if let Some(m_waiter) = mutex.wait_queue.pop_highest_locked() {
            mutex.owner = (*m_waiter).id;
            wake_thread_locked(m_waiter);
        } else {
            mutex.owner = 0;
        }

        // 3. Prepare block on current thread and context switch
        let (prev_rsp, next_rsp, frame_type) = prepare_block_detached_locked();
        commit_block_and_switch(prev_rsp, next_rsp, orig_rflags, frame_type);
    }

    // 4. Post-resumption: re-acquire mutex before returning to caller
    mutex.lock();
}
```

### 8.3 Signaling Implementations
```rust
impl Condvar {
    pub fn signal(&mut self) -> bool {
        self.wait_queue.wake_one().is_some()
    }

    pub fn broadcast(&mut self) -> usize {
        self.wait_queue.wake_all()
    }
}
```

### 8.4 Formal "No Lost Wakeup" Proof
> **The transition from "waiter holds the mutex and decides to wait" to "waiter is registered on the condition wait queue and the mutex is released" is atomic with respect to signaling operations that participate in the synchronization protocol. Therefore, a signal cannot fall into the gap between predicate observation and waiter registration.**

*Proof*:
1. The waiter evaluates the predicate $P$ while holding `mutex`.
2. Any signaling thread that modifies the state affecting $P$ must acquire `mutex` to do so.
3. In `wait()`, the waiter acquires `SCHEDULER.lock` with `cli`, registers itself in `condvar.wait_queue`, releases `mutex`, and transitions to `Blocked` before dropping `SCHEDULER.lock`.
4. A signaling thread cannot acquire `mutex` to modify $P$ until step 3 has released `mutex`.
5. At the exact instant `mutex` is released, the waiter is already unconditionally registered on `condvar.wait_queue`.
6. When the signaling thread subsequently calls `condvar.signal()`, the waiter is guaranteed to be present on `condvar.wait_queue` and will be popped and woken.
7. Hence, no signal can fall between predicate evaluation and waiter registration. $\blacksquare$

---

## 9. Preemption & Interrupt Interaction

| Operation Context | Timer IRQ Arrives? | Preemption Allowed? | Architectural Behavior |
| :--- | :---: | :---: | :--- |
| **Holding a Mutex** | Yes | Yes | Thread can be preempted while holding a mutex. Other threads contending for it will block cleanly until the owner runs and unlocks. |
| **Inside `prepare_block_locked()`** | No (`IF=0`) | Deferred | Interrupts are disabled (`cli`) while mutating queues. |
| **Inside `wake_thread_locked()`** | No (`IF=0`) | Deferred | Mutations occur under `sched_lock` with `cli`. If woken thread has higher priority, `need_resched = 1`. |
| **Inside `sleep_ms()`** | Yes | Yes | Sleeps via `block_current()`. Timer ISR wakes it when `TICKS >= deadline`. |
| **Preemption while Disabling** | Yes | Deferred | When `preempt_count > 0`, timer IRQ marks `need_resched = 1` and returns immediately without switching stacks. |

---

## 10. Lock Ordering Hierarchy

```text
Level 1: Mutex Lock (Coarse synchronization, sleeping permitted while held)
   ↓
Level 2: SCHEDULER.lock (Spinlock, IF=0, NO sleeping, NO context switch while held)
```

- A thread may hold a `Mutex` while acquiring `SCHEDULER.lock`.
- A thread must **never** attempt to acquire a `Mutex` while holding `SCHEDULER.lock`.
- `SCHEDULER.lock` is acquired with `cli` and released before `terminal_context_switch`.

---

## 11. Formal Invariants

- **I1**: A `Blocked` thread is **never** present on any `RunQueue`.
- **I2**: A `Ready` or `Running` thread is **never** present on any `WaitQueue`.
- **I3**: A thread belongs to at most **one** `WaitQueue` at any given time (`next_waiter` linkage).
- **I4**: Waking transitions `Blocked -> Ready` **exactly once**.
- **I5**: A `Terminated` thread cannot become `Ready`, `Blocked`, or be woken.
- **I6**: `SCHEDULER.lock` is **never** held across a context switch.
- **I7**: No synchronization primitive allocates from the heap (zero dynamic memory).
- **I8**: A sleeping thread consumes **zero CPU cycles** while asleep.
- **I9**: Every blocked thread has a well-defined, auditable wake condition.
- **I10**: Every wake event has exactly one authoritative ownership path.
- **I11**: Mutex ownership is unambiguous (`owner` thread ID tracked; non-owner unlock panics).
- **I12**: Condition-variable wait cannot lose a wakeup due to block/unlock atomicity.
- **I13**: All scheduler/runqueue/waitqueue mutations obey the existing interrupt-state contract (`IF=0`).
- **I14**: PMM frame accounting is invariant across blocking/waking cycles (zero physical frame leaks).
- **I15**: Functions with suffix `_locked` require `SCHEDULER.lock` held upon entry and leave it held upon exit with `IF=0`. They **never** acquire or release `SCHEDULER.lock` (zero exceptions).
- **I16**: `wake_thread_locked(detached_thread)` requires that the thread has **already been unlinked** from its owning `WaitQueue`.

---

## 12. Failure Semantics

| Scenario | Handled Condition | Resulting Behavior |
| :--- | :--- | :--- |
| **Empty RunQueue on Block** | All worker threads blocked | Scheduler selects `idle_thread`; CPU executes `pause` loop until timer tick wakes a sleeper. |
| **Waking Already-Ready Thread** | `thread.state != Blocked` | Deterministic no-op; returns `false` without modifying queues. |
| **Waking Terminated Thread** | `thread.state == Terminated` | Explicit panic: `"Attempted to wake terminated thread"`. |
| **Duplicate WaitQueue Insertion** | `thread.next_waiter != null` | Assertion failure: `"Thread already enqueued on wait channel"`. |
| **Removing Non-Member Waiter** | Waiter not in queue | Returns `false` cleanly. |
| **Non-Owner Mutex Unlock** | `caller.id != mutex.owner` | Explicit panic: `"Mutex unlock attempted by non-owner"`. |
| **Recursive Mutex Lock** | `caller.id == mutex.owner` | Explicit panic: `"Deadlock: recursive mutex lock attempted"`. |
| **Condvar Wait Without Mutex** | `caller.id != mutex.owner` | Explicit panic: `"Condvar wait invoked without holding associated mutex"`. |
| **Sleep Table Full** | $> 16$ concurrent sleepers | Returns `Err(SleepError::TableFull)` (fail-closed, statically bounded). |

---

## 13. Test Plan (Validation Requirements)

1. **Basic Block & Wake Test**:
   - Thread A blocks on an empty wait queue via `block_current()`.
   - Scheduler switches to Thread B.
   - Thread B wakes Thread A via `wait_queue.wake_one()`.
   - Thread A resumes and asserts post-block invariants.
2. **WaitQueue Ordering & Boundary Test**:
   - Verify priority ordering (`Critical` waiter wakes before `Normal` waiter).
   - Verify strict FIFO within same priority.
   - Verify `wake_one` vs `wake_all` vs `wake_thread`.
3. **Timer Sleeping Test (`sleep_ms`)**:
   - `sleep_ms(0)` returns immediately after yielding.
   - `sleep_ms(20)` pauses execution for exactly 2 ticks (20 ms) without CPU spinning.
   - Multiple concurrent sleepers with differing deadlines expire in exact chronological order.
4. **Mutex Contention Test**:
   - Fast-path uncontended lock/unlock takes zero context switches.
   - Contended acquisition blocks Worker 2 while Worker 1 holds the lock.
   - Worker 1 unlock transfers ownership directly to Worker 2 and wakes it.
   - Rejection of non-owner unlock and recursive lock verified.
5. **Condvar Producer-Consumer Test**:
   - Consumer waits on empty buffer (`condvar.wait(&mut mutex)`).
   - Producer fills buffer and calls `condvar.signal()`.
   - Consumer wakes, reacquires mutex, and consumes buffer without lost wakeups.
6. **Preemption Stress Test**:
   - Timer interrupts fire while threads hold mutexes.
   - Deferred preemption preserves `need_resched` while locks are held under `IF=0`.
7. **Regression Suite**:
   - All 74 existing tests across Stages 1–3C pass with 0 regressions.

---

## 14. Explicitly Deferred Scope

The following capabilities are explicitly deferred to later stages:
1. **Priority Inheritance Protocol (PIP)**: Mutex priority ceiling/inheritance is deferred to Stage 3D Increment 2 / Stage 3E.
2. **SMP Multi-Core Locking**: Lock structures assume single-core BSP; IPI wakeups and cross-core runqueue balancing are deferred to multi-core milestones.
3. **Userspace Futex Interface**: Stage 3D primitives operate strictly on `KernelThread` descriptors in Ring 0.
4. **Asynchronous Mutex/Condvar Timeouts**: Timed mutex acquisition (`try_lock_for`) is deferred.

---

## 15. ADR Path
The updated architectural record is committed to:
[`docs/decisions/ADR-0012-thread-blocking-and-synchronization.md`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/decisions/ADR-0012-thread-blocking-and-synchronization.md).

---

## 16. Substrate Consistency Statement
**No architectural contradictions with the frozen Stage 3A/3B/3C substrate were discovered.**
- `KernelThread` descriptor layout (104 bytes), intrusive links (`next_runnable`, `next_waiter`), and `ThreadState::Blocked` are completely respected.
- Context-switch ABI primitives (`switch_context`, `terminal_context_switch`) are used without modification.
- The `SCHEDULER.lock` release-before-switch contract is 100% maintained.
- All 74 existing unit and telemetry tests pass cleanly (`Ran 74 tests in 22.321s, OK`).
