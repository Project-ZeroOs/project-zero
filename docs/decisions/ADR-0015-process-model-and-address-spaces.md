# ADR-0015: Process Model, Address Spaces, and Formal Process Lifecycle

## Status
**Accepted / Frozen** (2026-09-15)

## Context
Stages 3A through 3E established a preemptive, multi-threaded kernel execution engine with guarded stacks, deterministic priority scheduling, intrusive synchronization primitives (`WaitQueue`, `Mutex`, `Condvar`, `Event`, `sleep_ms`), and thread lifecycle reclamation. However, all execution entities shared the single bootstrap higher-half page table (`MASTER_KERNEL_PML4`), with no address space isolation, no process-level resource boundaries, and no formal protocol for terminating multi-threaded task groups.

Specifically:
1. **Lack of Address Space Isolation**: All threads ran under the same PML4, exposing all memory to all threads.
2. **Missing Process Abstraction**: The kernel could not group threads into coherent resource containers.
3. **No Process Termination Protocol**: Simply marking threads as Zombies leaves them stranded inside intrusive wait queues (`WaitQueue`, `Event.waiters`, `Mutex.waiters`, `Condvar.waiters`) and the timer `SleepTable`, causing corrupted pointers and silent deadlocks.
4. **CR3 Hazard**: Tearing down an address space while CR3 on any CPU points to its PML4 causes catastrophic triple faults.
5. **State Contradictions**: Asymmetric terminal transitions (`Active -> Zombie` vs `Active -> Terminating -> Zombie`) lead to un-quiesced execution.

## Decision

We establish the **Stage 3F Process Model, Address Spaces, and Formal Lifecycle Subsystem**:

### 1. Distinct AddressSpace Abstraction
Each non-kernel `Process` owns an isolated `AddressSpace`:
- **Kernel Aperture**: Higher-half entries (PML4[256..511], covering HHDM at `0xFFFF800000000000` and Kernel VMA at `0xFFFFFFFF80000000`) are cloned from `MASTER_KERNEL_PML4`. All entries enforce `USER=0` (supervisor mode).
- **User Aperture**: Lower-half entries (PML4[0..255], `0x0000000000000000` to `0x00007FFFFFFFFFFF`) are initialized to 0 (unmapped). Stage 4 will map user ELF binaries here with `USER=1`.
- **Destruction**: Reclaims only user-space page table structures; the shared kernel aperture is never freed. Root PML4 frame is returned to PMM only when the address space is verified inactive in CR3.

### 2. Single Canonical Terminal Process Lifecycle
The process state machine strictly adheres to:
```text
Creating -> Active -> Terminating -> Zombie -> Reclaiming -> Free
```
- **`Active`**: The process is running normally with one or more threads.
- **`Terminating`**: Initiated by `process_exit(code)` or the exit of the final thread. Guarantees:
  1. No new threads or child processes may be created.
  2. No process-owned thread may continue normal execution.
  3. All non-current threads are quiesced/cancelled from blocking structures.
  4. All blocking structure memberships are cleanly removed under `SCHEDULER.lock`.
  5. The `AddressSpace` remains valid until process reclamation begins.
- **`Zombie`**: All threads have reached `ThreadState::Zombie`. Exit code is latched, and completion event is signaled.
- **`Reclaiming`**: Exactly one context (joiner or reaper) has claimed the right to deallocate the process resources.
- **`Free`**: All threads and the `AddressSpace` have been deallocated, and the process slot is cleared.

### 3. Formal Process-Termination Cancellation Protocol
A thread in `Zombie`, `Reclaiming`, or `Free` MUST NOT reside in any:
`RunQueue`, `WaitQueue`, `SleepTable`, `Event.waiters`, `Mutex.waiters`, `Condvar.waiters`, or any other blocking/execution structure.

When a process terminates, all process-owned threads are cancelled under `SCHEDULER.lock` with `IF=0`:
- **`Ready` Threads**: Removed from their priority `RunQueue` (`critical_queue`, `high_queue`, or `normal_queue`) via `remove_locked()`. Transitioned to `Zombie`.
- **Threads blocked on `Event`**: Removed from `event.waiters` via `WaitQueue::remove_locked()`, decrementing waiter count and clearing `next_waiter = NULL`.
- **Threads blocked on `Condvar`**: Removed from `condvar.waiters` via `WaitQueue::remove_locked()`, decrementing waiter count and clearing `next_waiter = NULL`.
- **Threads blocked on `Mutex` (Waiters)**: Removed from `mutex.waiters` via `WaitQueue::remove_locked()`, decrementing waiter count and clearing `next_waiter = NULL`.
- **Threads owning a `Mutex` (Holders)**: If the terminating thread is the `owner` of a mutex, ownership is released. If waiters exist, the highest-priority waiter that is *not* currently being cancelled is granted ownership; otherwise, `mutex.owner = 0`.
- **Threads in `SleepTable`**: Removed from `SLEEP_TABLE` by clearing the entry to `None`, and removed from `SLEEP_WAIT_QUEUE` via `remove_locked()`.
- **Stack Quiescence**: Stacks are reclaimed only after threads have completely exited execution and are off-CPU.

### 4. Process Zombie Queue Membership & Monotonic Identity
- A process may appear in `ZOMBIE_PROCESS_QUEUE` at most once.
- `Process` maintains an explicit `zombie_queued: bool` flag.
- Enqueue requires `state == ProcessState::Zombie` and `zombie_queued == false`. It sets `zombie_queued = true`.
- Dequeue requires `zombie_queued == true` and resets `zombie_queued = false`.
- Monotonic 64-bit `ProcessId` prevents ABA reuse hazards.

### 5. Master Kernel Process (PID 0)
- PID 0 is a permanent, immortal process owning `MASTER_KERNEL_PML4`.
- Permanently in `ProcessState::Active`.
- Never destroyed, never placed in `ZOMBIE_PROCESS_QUEUE`.
- Owns the bootstrap thread and the idle thread.

### 6. Machine-Level CR3 Invariant & Decoupled Switching
At every scheduler dispatch boundary:
```text
CR3 == current_thread.process.address_space.pml4_root
```
- **Same-Process Thread Switch**: CR3 write is skipped (0 cycles, 0 TLB invalidations).
- **Cross-Process Thread Switch**: CR3 is updated to `next_thread.process.address_space.pml4_root`.
- Thread context switching (callee-saved GPRs / preemptive iretq frame) and address-space switching remain architecturally decoupled. The frozen Stage 3C frame ABI is unchanged.

### 7. Address Space Quiescence & Active CR3 Hazard Elimination
An `AddressSpace` may enter `Reclaiming` only when:
1. No process-owned thread can execute again.
2. No process-owned thread remains in any execution or blocking queue.
3. No CPU has CR3 pointing at the `AddressSpace`.
Before releasing an address space, the local CR3 is inspected. If it matches the dying PML4 root, CR3 is immediately switched to `MASTER_KERNEL_PML4` under `SCHEDULER.lock` prior to freeing any physical frames.

### 8. Distinct Parentage Hierarchies
- **`parent_process`**: Defines the process hierarchy (process grouping and process exit reporting).
- **`creator_thread` / `JoinHandle`**: Represents thread-level lifecycle management.
Thread parentage and Process parentage are distinct, non-conflated relationships.

## Consequences

### Positive
- Strict hardware memory isolation between processes.
- Memory leak prevention: exact PMM frame accounting is guaranteed across process creation and destruction.
- Deterministic, panic-free multi-thread cancellation without stranded pointers in intrusive wait queues.
- Fully compatible with future SMP (TLB shootdown hooks preserved).
- Complete preservation of Stage 1–3E contracts and 86 passing tests.

### Invariants Maintained
- `CR3 == current_thread.process.address_space.pml4_root` at all scheduling points.
- Every thread in `Zombie`, `Reclaiming`, or `Free` is disconnected from all blocking queues.
- `process.zombie_queued == true` iff currently in `ZOMBIE_PROCESS_QUEUE`.
