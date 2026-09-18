# ADR-0014: Thread Lifecycle, Zombie State, and Resource Reclamation

## Status
**Accepted / Frozen** (2026-09-15)

## Context
In Stages 3A through 3D, threads could be spawned, scheduled cooperatively and preemptively, blocked on wait channels (`WaitQueue`, `Mutex`, `Condvar`, `sleep_ms`, `Event`), and terminated via `exit_current_thread()`. However, thread resource destruction relied on test scaffolding (`destroy_thread()`). The kernel lacked an authoritative, automated lifecycle management subsystem:

1. A terminating thread cannot deallocate its own stack while executing on it.
2. Observers require thread termination metadata (exit code, accounting) after execution halts.
3. Language-level `Drop` cannot be relied upon across non-returning context switches (`exit_current_thread() -> !`).
4. Reclaiming resources without concurrency control leads to double-reclamation races between joiners and background scavengers.
5. Reusing descriptor slots without monotonic identity tracking exposes the kernel to ABA use-after-free hazards.

## Decision

We establish the **Stage 3E Thread Lifecycle & Resource Reclamation Subsystem**:

### 1. Extended Thread Lifecycle States
The thread state machine is extended to:
```text
Initializing -> Ready <-> Running <-> Blocked -> Zombie -> Reclaiming -> Free
```
- **`Zombie`**: Execution has permanently ceased. RSP is completely off the stack. The 16 KiB stack and descriptor remain allocated to preserve the exit status (`exit_code: i32`) and completion signal.
- **`Reclaiming`**: An atomic, exclusive transition from `Zombie` claimed under `SCHEDULER.lock`. Exactly one context (either `Joiner` or `Reaper`) holds the right to unmap the stack and free the descriptor.
- **`Free`**: The 4 physical frames have been returned to PMM, the guard page and stack virtual pages unmapped from VMM, the arena bitmap slot cleared, and the descriptor zeroed with `occupied = false`.

### 2. Hybrid Reclamation Model (Model C)
- **Joinable Threads (`JoinHandle`)**: Managed through an opaque, linear `JoinHandle`. Calling `handle.join()` blocks on the private embedded `ManualReset` `completion_event`, transitions the thread `Zombie -> Reclaiming`, reclaims the stack, zeroes the descriptor, and returns the exit code.
- **Detached & Abandoned Threads (`Reaper`)**: Threads explicitly detached via `handle.detach()`, or whose parent terminates before joining, are marked `is_detached = true`. Upon entering `Zombie`, they are enqueued into the intrusive `ZOMBIE_QUEUE`. The kernel `Reaper` (executed during idle or low-priority execution) transitions them to `Reclaiming` and deallocates their stacks and descriptors.

### 3. Nested Lock Elimination: `Event::signal_locked()`
To prevent self-deadlock in `exit_current_thread()` (which already holds non-reentrant `SCHEDULER.lock`):
- `Event::signal_locked(&mut self)` executes under caller-held `SCHEDULER.lock` with `IF=0`. It performs the complete wake logic and waiter detachment checks without acquiring or releasing the lock.
- `Event::signal(&mut self)` remains the public outer locking wrapper.

### 4. Single `ZOMBIE_QUEUE` Membership Invariant & Explicit Flag
A descriptor may appear in `ZOMBIE_QUEUE` at most once.
- `KernelThread` maintains an explicit membership boolean `zombie_queued: bool` (byte offset 76).
- `enqueue_zombie_locked()` asserts `zombie_queued == false` and sets `zombie_queued = true`.
- `dequeue_zombie_locked()` asserts `zombie_queued == true` and resets `zombie_queued = false`.
- All paths capable of enqueueing a Zombie (`detach_thread_locked`, `exit_current_thread_with_code` child abandonment, `exit_current_thread_with_code` self-enqueue) strictly check `!target.zombie_queued` rather than inspecting pointer successors (`next_zombie == NULL`).
- When `handle.detach()` is called, the target thread is immediately unlinked from its parent's intrusive child list under `SCHEDULER.lock`, setting `parent_id = 0`.
- When the parent later terminates, its `first_child` traversal cannot visit the detached child, eliminating duplicate enqueue paths.
- `JoinHandle::Drop` is a best-effort convenience wrapper; kernel lifecycle correctness does NOT depend on Rust `Drop` execution. Kernel-level parent abandonment remains authoritative.

### 5. Self-Join & Reference Safety
- `JoinHandle::join()` immediately compares `target_id` against the calling thread's ID, returning `Err(ThreadError::SelfJoin)` prior to lock acquisition.
- Descriptors track unique monotonic 64-bit `ThreadId` values that never wrap, preventing ABA confusion upon slot reuse.

## Consequences

### Positive
- **Deterministic Lifetime Management**: No thread resources are leaked regardless of whether threads are joined or detached.
- **Zero Dynamic Allocation**: Intrusive child lists, intrusive `ZOMBIE_QUEUE`, and embedded 32-byte `Event` require no heap.
- **Capability Compatibility**: `JoinHandle` and `ThreadId` establish an unforgeable linear handle pattern directly portable to future Ring 3 process capabilities.
- **PMM Neutrality**: Physical frame accounting is strictly preserved across all test suites.

### Invariants Maintained
- `SAVED-FRAME INVARIANT`: `(*current).frame_type = SavedFrameType::Cooperative` stamped across `exit_current_thread()`.
- `SCHEDULER.lock` remains non-reentrant.
- Stack memory is unmapped strictly after execution transfers off the dying stack.
