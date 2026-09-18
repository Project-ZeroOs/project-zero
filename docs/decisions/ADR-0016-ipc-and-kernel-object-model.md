# ADR-0016: Zero-Heap IPC Subsystem and Unified Kernel Object Model

## Status
**Accepted / Frozen** (2026-09-17)

## Context
Stages 3A through 3F established preemptive multi-threading, priority scheduling, synchronization primitives, thread lifecycle reclamation, and isolated address spaces with formal process lifecycles. However, processes remained entirely isolated from one another with zero communication mechanisms. 

To support isolation boundaries and prepare for user space (Stages 3H–3I), the kernel requires a minimal, secure, zero-heap communication substrate satisfying the following constraints:
1. **Zero Kernel Dynamic Allocation**: Project Zero forbids dynamic kernel heaps (`kmalloc`, `Box`, `Vec`). All IPC queues, buffers, handles, and registries must reside in static `.bss` storage with deterministic bounds.
2. **Control Plane / Data Plane Split**: Control signaling requires bounded message passing; bulk data transfer requires zero-copy shared memory without double-buffering.
3. **Frozen Process Descriptor ABI (Stage 3F)**: `crate::task::process::Process` is frozen at exactly 128 bytes (8-byte aligned). Handle tables cannot be embedded inside `Process`.
4. **Endpoint Lifetime & Concurrent Closure Hazards**: Resolving the boundary condition where the last handle to an endpoint is closed while an IPC operation on that endpoint is in-flight or blocked (`I-CHAN-5`).
5. **Deterministic Peer Death & Resource Reclamation**: Process termination must never leak PMM frames, leave blocked threads suspended indefinitely, or cause use-after-free.

## Decision

We establish the **Stage 3G IPC Subsystem and Unified Kernel Object Model**:

### 1. Control Plane: Bounded Bidirectional `Channel`
- **Structure**: A symmetrical channel pair with two distinct endpoints (`Endpoint 0`, `Endpoint 1`) and two 4-message ring buffers ($2 \times 328$ bytes):
  - `Endpoint 0`: Sends to Ring $0 \to 1$, Receives from Ring $1 \to 0$.
  - `Endpoint 1`: Sends to Ring $1 \to 0$, Receives from Ring $0 \to 1$.
- **Message Size**: Fixed 80-byte `IpcMessage` containing header (`sender_pid`, `payload_len`, `flags`), a 64-byte inline payload buffer, and up to 4 attached opaque handle descriptors.
- **Backpressure & Waiters**: Flow control backpressure blocks senders on full rings (`waiters_tx`) and receivers on empty rings (`waiters_rx`) using intrusive 24-byte `WaitQueue` primitives.
- **Footprint**: Exactly 768 bytes per `Channel`. Stored in static `.bss` array `CHANNEL_TABLE: [Channel; 64]` (48 KiB).

### 2. Data Plane: Page-Granular `ShmObject`
- **Structure**: An array of 4 KiB `PhysFrame` descriptors allocated from PMM. Physical memory is physically non-contiguous but mapped contiguously into virtual address spaces.
- **Strict $W \oplus X$ Enforcement**: Mappings enforce `PAGE_USER | PAGE_PRESENT | PAGE_NX` (unconditional No-Execute) and optional `PAGE_WRITABLE`. Execution is strictly forbidden.
- **Authoritative Mapping Registry**: External static `.bss` array `SHM_MAPPING_TABLE: [ShmMapping; 64]` (2 KiB, 32 bytes per entry) tracks virtual base, PID, page count, and rights for each active mapping.

### 3. Unified Kernel Object Model & Monotonic IDs
- **`KERNEL_OBJECT_TABLE`**: 256 slots in `.bss` (10 KiB, 40 bytes per slot). Tracks object type (`Free`, `Channel`, `ShmObject`), 16-bit slot generation, pool index, and 32-byte `KernelObjectHeader`.
- **Reference Exactness**: `ref_count() == handle_refs + mapping_refs + in_flight_op_refs` at all times.
- **Monotonic Object ID & Terminal State (`I-OBJ-1`)**: Global atomic `NEXT_OBJECT_ID: AtomicU64` increments monotonically. Upon reaching `u64::MAX`, allocations deterministically fail with `ObjectIdExhausted` without wrapping to 0.

### 4. External Handle Table Registry & Process ABI Preservation
- **Preserved Process ABI (`I-PROC-1`)**: `Process` remains invariant at 128 bytes.
- **External Handle Tables**: `PROCESS_HANDLE_TABLES: [HandleTable; 16]` stored externally in `.bss` ($16 \times 520 = 8,320$ bytes). Each handle table holds 32 `HandleEntry` slots (16 bytes each).
- **Process Slot Resolution (`I-PROC-3`, `I-PROC-4`)**: `ProcessId` is never used as an array index. `resolve_current_process_slot(pid)` performs a lockless, bounded constant-time $O(1)$ scan of at most $\text{MAX\_PROCESSES} = 16$ entries.

### 5. In-Flight Endpoint Lifetime & Linearization (`I-CHAN-5`)
Once an IPC operation validates a handle and acquires an in-flight pin (`in_flight_op_refs += 1`, `THREAD_IPC_STATE[t].occupied = true`):
1. Closing the last user handle to that endpoint cannot reclaim the underlying `Channel` or cause use-after-free.
2. If the operation accesses the ring buffer before closure commits, it completes normally.
3. If closure commits before access, the operation observes `endpoint.is_closed == true`, releases its pin, and returns `Err(IpcError::EndpointClosed)`.
4. If blocked in `waiters_rx` or `waiters_tx` when the last handle closes, the closing thread wakes the waiter under `SCHEDULER.lock`; the waiter observes closure, releases its pin, and returns `Err(IpcError::EndpointClosed)`.
5. Object reclamation is deferred until `ref_count() == 0` and all waitqueues are empty.

### 6. Monotonic Lock Hierarchy & Process Teardown
- **Lock Ordering (`I-LOCK-1`)**:
  $$\mathbf{KERNEL\_OBJECT\_TABLE\_LOCK} \prec \mathbf{SCHEDULER.lock} \prec \mathbf{CPU\ (IF=0)}$$
- **Three-Step Process Exit**:
  - **Step 1 (Termination Intent & Sibling Cancellation)**: Under `SCHEDULER.lock`, mark `ProcessState::Terminating`, cancel all sibling threads, and unlink them from IPC wait queues.
  - **Step 2 (IPC Handle Closure, SHM & In-Flight Cleanup)**: Under `KERNEL_OBJECT_TABLE_LOCK` (with `SCHEDULER.lock` released), sweep and clear in-flight operations of cancelled threads, unmap all SHM mappings from PML4, close all handles in `PROCESS_HANDLE_TABLES[slot]`, and wake peer waiters.
  - **Step 3 (Final Zombie Transition & Terminal Switch)**: Under `SCHEDULER.lock`, transition process to `ProcessState::Zombie`, signal completion event, switch address space, and terminal context switch.

## Invariant Catalog
- `I-OBJ-1`: Monotonic 64-bit `object_id` with terminal exhaustion state (`u64::MAX`).
- `I-OBJ-2`: Table occupancy preserved through `Active`, `PeerClosed`, `Quiescent`, and `Reclaiming`.
- `I-OBJ-3`: `ref_count() == handle_refs + mapping_refs + in_flight_op_refs`.
- `I-OBJ-4`: Every `in_flight_op_refs` increment has exactly one terminal decrement.
- `I-OBJ-5`: Bounded generation checks reject stale/dangling handles.
- `I-PROC-1`: `Process` descriptor frozen at 128 bytes. Handle tables external.
- `I-PROC-2`: Process slot reuse scrubs all 32 handle slots and advances generations.
- `I-PROC-3`: Process slot index is stable throughout active process lifetime.
- `I-PROC-4`: Bounded $O(1)$ scan for process slot resolution.
- `I-CHAN-1`: Endpoint open iff `handle_refs > 0`.
- `I-CHAN-2`: Persistent peer closure flag and `SIGNAL_PEER_CLOSED`.
- `I-CHAN-3`: Strict FIFO ordering on ring buffers.
- `I-CHAN-4`: Waiters in channel wait queues pin the object (`in_flight_op_refs > 0`).
- `I-CHAN-5`: In-flight endpoint lifetime & linearization under `KERNEL_OBJECT_TABLE_LOCK`.
- `I-IPC-1`: Single in-flight IPC operation per thread (`THREAD_IPC_STATE`).
- `I-LOCK-1`: `KERNEL_OBJECT_TABLE_LOCK` $\prec$ `SCHEDULER.lock` $\prec$ `IF=0`.
- `I-LOCK-2`: No locks held across `terminal_context_switch`.
- `I-LOCK-3`: No lock inversion.
- `I-WAIT-1`: Single wait queue membership per blocked thread.
- `I-TEARDOWN-1`: Sibling quiescence after Step 1 of teardown.
- `I-MEM-1`: Physical frames backing `ShmObject` pinned while `mapping_refs > 0`.
- `I-MEM-2`: Deterministic SHM unmap and TLB invalidation in Step 2 of teardown.
- `I-MSG-1`: Message payloads contain no raw kernel pointers.

## Verification Evidence
- 46 bare-metal in-kernel tests (3G-A through 3G-AT) executing in QEMU.
- Dedicated host-side harness `tests/test_stage3g.py` validating 9 static ELF symbols and all 46 live QEMU telemetry markers.
- Full 88-test Stage 1–3F regression suite + 1 dedicated Stage 3G harness (total 90 test assertions in pytest, representing 89 test suites/modules) passing with 100% success.
- Exact PMM physical frame neutrality verified across all tests.
