# STAGE 4C ARCHITECTURE SPECIFICATION (REVISION 4)

## Workload Orchestration Subsystem Architecture Specification

**Status:** 🟡 **DRAFT — REQUIRES REVIEW (REVISION 4)** (2026-10-04)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024, Stage 4B Architecture Rev12 (`STAGE4B-ARCHITECTURE-REV12.md`), ADR-0025 Rev12, ADR-0026 Rev4  
**Kernel Baseline:** Stage 3A–3N Inviolate (0 Bytes Modified, Frozen Ring 0 Microkernel)  
**System Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Frozen, Stage 4B (`resourced`, Generic Resource Graph, Local Node Accounting, Leases) Frozen  
**Target Subsystem:** Stage 4C Workload Orchestration Subsystem (`workloadd`)

---

## 1. Scope and Frozen Dependencies

ZeroOS Stage 4C establishes the **Workload Orchestration Subsystem**, bridging high-level computational intent and the concrete, capability-bounded execution substrates of the operating system.

### 1.1 Inviolate Microkernel & System Baseline
Stage 4C operates strictly on top of three frozen, non-modifiable architectural layers:

1. **Stage 3 Microkernel Nucleus (Frozen)**:
   - Authoritative Ring 0 hardware isolation across up to 4 SMP cores.
   - Pre-allocated physical frame manager (`PMM`), virtual address space manipulation (`CR3`, `VMM`).
   - Priority-preemptive thread scheduler (`SCHEDULER`).
   - Process Table bounded to `MAX_PROCESSES = 16` (`kernel::task::process`).
   - Capability Derivation Tree (`CDT`) with handle tables bounded to `MAX_HANDLES_PER_PROCESS = 32` (`kernel::ipc::handle`).
   - Fast capability-checked IPC channels (`SYS_CHANNEL_*`) and shared memory (`SYS_SHM_*`).
   - **Kernel Constraint**: 0 bytes modified in kernel, 0 new system calls, 0 kernel allocation changes.

2. **Stage 4A System Service Substrate (Frozen)**:
   - Freestanding `#![no_std]` runtime (`libzero`).
   - PID 1 process supervisor and persistent `BootEpoch` authority (`init`).
   - Capability-mediated service discovery directory (`brokerd`).

3. **Stage 4B Resource & Lease Accounting Substrate (Frozen)**:
   - Local node heterogeneous Resource Graph managed by `resourced`.
   - Multi-dimensional vector conservation invariant: $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$.
   - Monotonic temporal capacity leases (`ResourceLease`, 144 bytes).
   - Quarantine Ledger ($C_{\text{unavail}}$) preserving hardware quiescence safety under invalidation.

```text
+-----------------------------------------------------------------------------+
| STAGE 4C: WORKLOAD ORCHESTRATION SUBSYSTEM (workloadd)                      |
|                                                                             |
|  * Workload Lifecycle State Machine & Task DAG Engine                       |
|  * Recovery Classes (Class 1 Pure, Class 2 Stateful, Class 3 Irreversible)  |
|  * 4-Phase Quarantine-Preserving Cancellation Pipeline                      |
|  * Configured System Process Capacity Policy (8 Concurrent Running Tasks)   |
|  * Explicit Supervisor Process Lifecycle Boundary & Capability Attenuation  |
+------------------------------------+----------------------------------------+
                                     | Consumes Leases via IPC (OP_LEASE_REQUEST)
+------------------------------------v----------------------------------------+
| STAGE 4B: LOCAL NODE ACCOUNTING & RESOURCE GRAPH (resourced) [FROZEN]       |
|                                                                             |
|  * Heterogeneous Resource Graph (Cycles Permitted)                          |
|  * Vector Conservation Invariant (C_avail + C_resv + C_alloc + C_unavail)   |
|  * Monotonic Temporal Capacity Leases (ResourceLease)                       |
|  * Quarantine Ledger & Safe Capacity Reclamation (C_unavail)                |
+------------------------------------+----------------------------------------+
                                     | Discovered via brokerd / Supervised by init
+------------------------------------v----------------------------------------+
| STAGE 4A: CORE USER-SPACE SERVICES (init, brokerd, libzero) [FROZEN]        |
+------------------------------------+----------------------------------------+
                                     | System Calls (SYS_CHANNEL_*, SYS_SHM_*, etc.)
+------------------------------------v----------------------------------------+
| STAGE 3: MICROKERNEL NUCLEUS (Processes, Threads, Caps, VM, SMP) [FROZEN]   |
+-----------------------------------------------------------------------------+
```

---

## 2. Workload Semantics

### 2.1 Definition: The Workload as the Unit of Intended Computation
A **Workload** is an authoritative Stage 4 administrative and operational object representing a goal-directed computational request. It encompasses:
1. A unique, non-reusable **`WorkloadId`** (`DistributedId`: 64-bit `node_id`, 64-bit `local_seq`).
2. An owner/creator identity (Client PID, Workspace ID, or Agent ID).
3. A bounded, directed acyclic graph (DAG) of **Tasks** (up to `MAX_TASKS_PER_WORKLOAD = 16`).
4. An aggregated multi-dimensional **Resource Envelope** derived from task demands.
5. A bounded **Capability Envelope** defining maximum permitted rights.
6. Active **Stage 4B Resource Leases** acquired from `resourced` to fulfill execution requirements.
7. Explicit associations with transient **Stage 3 Processes**.
8. Global execution policies: scheduling priority, end-to-end deadline, locality domain, energy policy, recovery class, and failure/retry semantics.

### 2.2 Workload Identity & Non-Reuse (`I-WORKLOAD-ID-NONREUSE`)
Every Workload is assigned a `WorkloadId`:
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DistributedId {
    pub node_id: u64,
    pub local_seq: u64,
}
```
- Issued exclusively by `workloadd`'s local `DistributedIdAllocator` backed by durable sequence persistence authority.
- **Monotonic Non-Reuse**: `local_seq` strictly increments. Once a `WorkloadId` terminates (`Completed`, `Failed`, `Cancelled`, `Reclaimed`), it is **permanently retired**. It is never reused under any circumstances.
- **Incarnation Generation**: The Workload maintains a monotonic 32-bit `generation` counter. Any restart or recovery increments `generation`. IPC messages presenting stale generations are rejected fail-closed with `ZeroError::StaleGeneration`.

---

## 3. Workload vs Process

ZeroOS strictly enforces the architectural distinction between kernel execution containers and Stage 4 computational units:

$$\mathbf{Process\ (Stage\ 3)} \quad\neq\quad \mathbf{Workload\ (Stage\ 4)}$$

### 3.1 Architectural Contrast Matrix

| Feature | Stage 3 Process (`kernel::task::process`) | Stage 4 Workload (`workloadd`) |
| :--- | :--- | :--- |
| **Execution Domain** | Single virtual address space (`CR3`), local CPU core, thread group. | Multi-task directed graph; coordinates multiple address spaces/processes. |
| **Lifetime & Lifecycle** | Tied directly to thread execution: `Creating -> Active -> Terminating -> Zombie -> Reclaiming -> Free`. | Goal-driven milestone state machine: `Creating -> Ready -> Running -> Completed/Failed`. Survives process crashes. |
| **Authority Primitive** | Process-local Capability Handle Table (`MAX_HANDLES_PER_PROCESS = 32`). | Global Workload Capability Envelope; attenuates rights per task handle. |
| **Resource Binding** | Bounded kernel handles; no concept of time-bounded physical capacity vectors. | Bounded Stage 4B Temporal Leases (`ResourceLease`); quota-checked and metered. |
| **Fault Boundary** | Hardware exception or fatal signal destroys the address space permanently. | Process crash is captured as a task event; evaluated by recovery class policy. |
| **Identity Continuity** | PID is freed upon kernel reclamation; never survives process termination. | `WorkloadId` persists across process crashes, task retries, and daemon restarts. |
| **Machine Boundary** | Strictly local to a single kernel instance (`I-NO-LIVE-PROCESS-MIGRATION`). | Logically separable; tasks can be scheduled locally or dispatched across fabric nodes. |

### 3.2 Task-to-Process Execution Isolation Rules

1. **One Task per Process at a Time (`I-TASK-PROCESS-ASSOCIATION`)**:
   - A Stage 3 Process executes at most one Task of one Workload at any given time.
   - *Justification*: Executing multiple tasks (or sequential tasks of different security tiers) within the same address space allows residual memory, cached secrets, heap fragmentation, and dangling pointers to contaminate subsequent executions.
   - *Kernel PMM Neutrality*: Upon process exit, Stage 3's `reclaim_process_resources_locked` guarantees that all mapped page frames, page tables, and thread stacks are completely returned to the Physical Memory Manager without leaks.
   - *Capability Table Reset*: A fresh process begins with a clean handle table (32 empty slots), preventing handle leakage across tasks.

2. **Process Non-Reuse Across Retries**:
   - When a task fails or crashes and is granted a retry, `workloadd` does **not** reuse the faulted process container.
   - The faulted process is terminated and reclaimed by Stage 3. `workloadd` requests a **fresh** Stage 3 process with a clean PML4 address space, new PID, and new handle table.

3. **Task Failure vs. Process Failure**:
   - **Task Failure**: Occurs when a task process exits with a non-zero status, violates a software precondition, or fails an application check.
   - **Process Failure**: Occurs when the kernel terminates a process due to an unhandled hardware exception (page fault, divide-by-zero, general protection fault) or an explicit `sys_channel_close` handle revocation.
   - Both events trigger task-level failure handling in `workloadd`, but process failures additionally require immediate capability tree invalidation and resource quarantine processing.

---

## 4. Task Model

A Task is an atomic node within a Workload's Directed Acyclic Graph (DAG).

### 4.1 Task Structure & Metadata
Every Task within a Workload has a unique 16-bit `TaskId` (index `0..MAX_TASKS_PER_WORKLOAD-1`):
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TaskDescriptor {
    pub task_id: u16,
    pub workload_id: DistributedId,
    pub state: TaskState,
    pub retry_count: u8,
    pub max_retries: u8,
    pub dependencies: u16,                  // Bitmask of dependent TaskIds (max 15)
    pub assigned_pid: Option<u64>,          // Stage 3 Process ID if Running
    pub lease_id: Option<DistributedId>,    // Active Stage 4B Lease ID
    pub demand: TaskResourceDemand,
}
```

### 4.2 Task Lifecycle State Machine
```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    Unallocated = 0,
    Blocked = 1,        // Dependencies unsatisfied
    Ready = 2,          // Dependencies complete; eligible for placement & lease request
    LeaseAcquired = 3,  // Lease granted by resourced; process launch pending
    Running = 4,        // Executing in dedicated Stage 3 process
    Completed = 5,      // Task execution succeeded; outputs committed
    Failed = 6,         // Task execution failed; retries exhausted or terminal error
    Cancelled = 7,      // Workload cancelled or ancestor task failed
    Stopped = 8,        // Execution stopped during cancellation before process exit
}
```

```text
                  +---------------+
                  |  Unallocated  |
                  +-------+-------+
                          | Define Task
                          v
                  +---------------+
      +---------->|    Blocked    |
      |           +-------+-------+
      |                   | All Dependencies Complete
      |                   v
      |           +---------------+
      |           |     Ready     |
      |           +-------+-------+
      |                   | Leases Granted by resourced
      |                   v
      |           +---------------+
      |           | LeaseAcquired |
      |           +-------+-------+
      |                   | Stage 3 Process Launched
      |                   v
      |           +---------------+
      | Retry     |    Running    +-----------------------+
      |           +---+---+---+---+                       |
      |               |   |   |                           |
      | +-------------+   |   +-----------------------+   |
      | |                 |                           |   |
      v v                 v                           v   v
+-----------+       +-----------+               +-----------+
|  Failed   |       | Completed |               | Cancelled |
+-----------+       +-----------+               +-----------+
```

---

## 5. Task DAG

Stage 4C orchestrates computation using a directed acyclic graph of tasks:

$$\mathbf{Resource\ Graph\ (Stage\ 4B)} \quad\neq\quad \mathbf{Task\ DAG\ (Stage\ 4C)}$$
- **Resource Graph (4B)**: Models physical hardware components and capacity topology. Directed cycles are explicitly permitted (e.g., `Node -> Device -> Storage -> Swap -> RAM -> Node`).
- **Task DAG (4C)**: Models computational dataflow and execution precedence. Must be **strictly acyclic** (`I-TASK-DAG-ACYCLIC`). Cycles represent unresolvable execution deadlocks and are rejected fail-closed at admission.

### 5.1 Representation & Admittance Sorting
- **Maximum Tasks**: `MAX_TASKS_PER_WORKLOAD = 16`.
- **Dependency Representation**: Compact bitmask `dependencies: u16` where bit $j$ is set if Task $i$ depends on Task $j$.
- **Admittance Cycle Detection**:
  Upon submission of a Task DAG, `workloadd` executes Kahn's Topological Sort Algorithm in $< 128\text{ bytes}$ stack memory:
  1. Calculate in-degrees for all defined tasks from dependency bitmasks.
  2. Enqueue all tasks with in-degree 0 into a static 16-entry ring buffer.
  3. Dequeue tasks, decrementing in-degrees of downstream dependents.
  4. If processed count equals defined task count, the graph is verified **strictly acyclic**.
  5. If any task retains in-degree $> 0$, the graph contains a directed cycle: `workloadd` rejects submission with `ZeroError::CyclicDependency`.

---

## 6. DAG Mutation Authority

ZeroOS permits dynamic DAG extension under strict architectural rules:

### 6.1 Authority & Preconditions
1. **Mutation Authority**: Only the authenticated Workload Owner holding the Workload Control Capability may submit DAG extensions.
2. **Lifecycle State Constraint**: Mutations are permitted **only** while the Workload is in `Ready` or `Running`. If the Workload is in any terminal state (`Completed`, `Failed`, `Cancelled`, `Reclaimed`), mutations are rejected fail-closed with `ZeroError::InvalidLifecycleState`.
3. **Completed Workloads Immutable (`I-COMPLETED-WORKLOAD-IMMUTABLE`)**: A Workload that has reached `Completed` is permanently closed. Extending a completed computation requires submitting a new Workload.

### 6.2 Structural Extension Rules
1. **Existing Tasks Immutable (`I-TASK-DAG-IMMUTABLE-EXISTING`)**: Admitted tasks cannot have their code targets, resource requirements, or declared dependencies altered retroactively.
2. **Downstream-Only Extension (`I-TASK-DAG-DOWNSTREAM-EXTENSION`)**: New tasks may only declare dependencies on existing tasks. New tasks cannot become prerequisites for tasks already in `Ready`, `Running`, or `Completed`.
3. **Global Acyclicity Re-Verification**: Every mutation pass re-evaluates Kahn's Algorithm across the full combined graph before admitting new tasks.

---

## 7. Workload Lifecycle

A Workload transitions through a formal, deterministic state machine:

```text
                  +---------------+
                  |   Creating    |
                  +-------+-------+
                          | Submit DAG & Caps Verified
                          v
                  +---------------+
      +---------->|     Ready     |
      |           +-------+-------+
      |                   | Initial Tasks Dispatched
      |                   v
      |           +---------------+
      |           |    Running    +-----------------------+
      |           +---+---+---+---+                       |
      |               |   |   |                           |
      | +-------------+   |   +-------------+             |
      | |                 |                 |             |
      | v                 v                 v             |
+-----+---------+  +------+------+  +-------+-------+     |
|   Waiting     |  |  Suspended  |  |  Recovering   |     |
+---------------+  +-------------+  +-------+-------+     |
                                            |             |
                                            v             |
                                    +-------+-------+     |
                                    |  Cancelling   |<----+
                                    +-------+-------+
                                            |
        +-----------------------------------+-----------------------------------+
        |                                   |                                   |
        v                                   v                                   v
+---------------+                   +---------------+                   +---------------+
|   Completed   |                   |    Failed     |                   |   Cancelled   |
+-------+-------+                   +-------+-------+                   +-------+-------+
        |                                   |                                   |
        +-----------------------------------+-----------------------------------+
                                            |
                                            v
                                    +---------------+
                                    |   Reclaimed   |
                                    +---------------+
```

### 7.1 Formal Lifecycle State Transitions

| State | Preconditions | Authority | Side Effects | Next States |
| :--- | :--- | :--- | :--- | :--- |
| **Creating** | Client initiates submission with `WorkloadDescriptor`. | Client / `workloadd` | Assigns `WorkloadId`, initializes task table. | `Ready`, `Failed` |
| **Ready** | Task DAG verified acyclic; capability envelope validated. | `workloadd` | Tasks with in-degree 0 transition to `Ready`. | `Running`, `Cancelling` |
| **Running** | At least one task is `Running` or `LeaseAcquired`. | `workloadd` | Leases actively renewed; processes monitored. | `Waiting`, `Suspended`, `Recovering`, `Cancelling`, `Completed`, `Failed` |
| **Waiting** | All running tasks completed; remaining tasks blocked on dependencies. | `workloadd` | Idle leases surrendered to conserve power. | `Running`, `Cancelling`, `Failed` |
| **Suspended** | Client or supervisor pauses workload. | Client / Supervisor | Task threads paused; leases retained or hibernated. | `Running`, `Cancelling` |
| **Recovering**| Process crash or lease loss encountered; retry evaluated. | `workloadd` | Quarantines failed process; calculates backoff. | `Running`, `Failed` |
| **Cancelling**| Cancellation requested, deadline expired, or fatal error. | Client / `workloadd` | Enters 4-Phase Cancellation Pipeline. | `Cancelled`, `Failed` |
| **Completed** | All tasks in DAG reached `Completed` state. | `workloadd` | Terminal success; leases released; client notified. | `Reclaimed` |
| **Failed** | Terminal task failure, retry exhaustion, or security fault. | `workloadd` | Terminal failure; active processes killed; leases released. | `Reclaimed` |
| **Cancelled** | Cancellation pipeline teardown finished. | `workloadd` | Terminal cancellation; resources quarantined. | `Reclaimed` |
| **Reclaimed** | Handles closed; processes destroyed; client consumed result. | `workloadd` | Slot returned to pool; `WorkloadId` permanently retired. | None (Terminal) |

---

## 8. Task Lifecycle

Each individual task tracks state independently within the Workload DAG:

| Task State | Trigger / Condition | Action / Side Effect |
| :--- | :--- | :--- |
| `Unallocated` | Slot empty in workload descriptor. | Static allocation placeholder. |
| `Blocked` | In-degree $> 0$ (ancestor tasks incomplete). | Waits for upstream task completion. |
| `Ready` | In-degree $== 0$ (all ancestors `Completed`). | Placed in node placement queue; requests lease. |
| `LeaseAcquired` | `resourced` grants `ResourceLease`. | Prepares ELF binary and capability tokens for launch. |
| `Running` | Stage 3 process created and active. | Monitored by `workloadd` for exit or IPC signal. |
| `Completed` | Process exits with code 0; outputs verified. | Decrements in-degrees of downstream dependent tasks. |
| `Failed` | Non-zero exit, CPU fault, or retry limit hit. | Triggers recovery class handler or cancels DAG. |
| `Cancelled` | Parent workload cancelled or ancestor failed. | Task skipped; process never launched or stopped. |
| `Stopped` | Interrupted during active execution by cancellation. | Process forced to terminate; awaiting quarantine. |

---

## 9. Recovery Classes

ZeroOS replaces universal restart idempotence with three explicit, recovery-class-specific contracts:

### 9.1 Recovery Class Classification

```text
                     +-----------------------------------+
                     |     Task / Process Failure        |
                     +-----------------+-----------------+
                                       |
                   +-------------------+-------------------+
                   |                   |                   |
                   v                   v                   v
           +---------------+   +---------------+   +---------------+
           |    Class 1    |   |    Class 2    |   |    Class 3    |
           | Ephemeral/Pure|   | Checkpointed  |   | Irreversible  |
           +-------+-------+   +-------+-------+   +-------+-------+
                   |                   |                   |
                   v                   v                   v
           +---------------+   +---------------+   +---------------+
           | Full Replay   |   | Resume from   |   | Transition to |
           | from Scratch  |   | Checkpoint Gc |   | FailedAt      |
           | Permitted     |   | Permitted     |   | Milestone(M)  |
           +---------------+   +---------------+   +---------------+
```

1. **Class 1 — Ephemeral Pure Compute (`I-RECOVERY-CLASS-1-IDEMPOTENT-REPLAY`)**:
   - **Contract**: Pure functions, stateless data processing, memory-only transformations. Zero persistent side-effects.
   - **Recovery Action**: Replay permitted from scratch. `workloadd` discards transient state, requests a fresh Stage 3 process, and re-executes the task from step 0.
   - **Guarantee**: Deterministic duplicate-free replay is mathematically safe because no external state was modified.

2. **Class 2 — Checkpointed Stateful (`I-RECOVERY-CLASS-2-CHECKPOINTED-RESUME`)**:
   - **Contract**: Stateful computations periodically committing verified checkpoints ($G_c$) to ZeroFS or persistent descriptors.
   - **Recovery Action**: Resume **only** from the latest verified checkpoint generation $G_c$. `workloadd` does not re-execute completed ancestor tasks; it restores process state to milestone $G_c$ within a fresh process container.
   - **Guarantee**: Bounded idempotence. Intermediate uncheckpointed work between $G_c$ and failure point is discarded.

3. **Class 3 — Irreversible External (`I-RECOVERY-CLASS-3-NO-TRANSPARENT-REPLAY`)**:
   - **Contract**: Tasks involving external physical side-effects (device register actuation, network socket writes, hardware power state changes, persistent storage mutations).
   - **Recovery Action**: **Transparent replay is strictly prohibited.** `workloadd` MUST NOT attempt automatic re-execution. Upon failure, the task transitions immediately to `FailedAtMilestone(M)`, where $M$ is the last durably logged milestone prior to side-effect execution.
   - **Guarantee**: Zero uncoordinated duplicate side-effects.

### 9.2 Compensation Handlers (Future Policy Boundary)
- In Stage 4C, compensation handlers are **not** implicitly executed by `workloadd`.
- If a Class 3 workload requires cleanup after `FailedAtMilestone(M)`, compensation tasks must be explicitly declared as downstream nodes in the Task DAG by the workload owner.
- Compensation tasks are themselves evaluated under the exact same process isolation and capability constraints, and may themselves be Class 3 operations. Automatic compensation loops are left as future Phase 4E/4F policy.

---

## 10. Cancellation Semantics

Cancellation in ZeroOS must **never** imply immediate physical capacity release. Stage 4C reconciles cancellation with Stage 4B accounting through a strict 4-Phase Quarantine-Preserving Pipeline.

### 10.1 The 4-Phase Cancellation Pipeline

```text
[Phase 1: Cancellation Requested]
  - workloadd transitions Workload state to 'Cancelling'.
  - Rejects any further task submissions, DAG mutations, or lease renewal requests.
            ↓
[Phase 2: Cooperative Stop & Process Termination]
  - workloadd transmits cancellation signal via IPC to active task processes.
  - Starts bounded cooperative grace period (ΔT_cancel_timeout = 50 ms).
  - Task process executes sys_exit(code) or control channel is closed via sys_channel_close.
  - Stage 3 kernel reclaims process address space (PML4) to PMM; process enters Zombie/Reclaimed.
            ↓
[Phase 3: Authority Teardown & Lease Release]
  - workloadd closes task capability handles via sys_channel_close (CDT invalidation).
  - workloadd surrenders lease authority via OP_LEASE_RELEASE IPC to resourced.
  - resourced invalidates lease token (LeaseState::Released).
  - resourced accounting engine transfers capacity from C_alloc to C_unavail (Quarantine Ledger).
  - C_avail remains strictly UNCHANGED at this stage.
            ↓
[Phase 4: Provider Confirmation & Physical Availability]
  - Underlying hardware driver / provider confirms device DMA/registers quiesced and reset.
  - resourced accounting engine transfers capacity from C_unavail to C_avail:
        C_unavail ← C_unavail - ΔC
        C_avail   ← C_avail + ΔC
  - workloadd transitions Workload to 'Cancelled', then 'Reclaimed'.
```

### 10.2 Explicit State Distinctions

To ensure complete clarity across architectural boundaries, ZeroOS explicitly distinguishes:
1. **Cancellation Requested**: Administrative intent registered (`WorkloadState::Cancelling`).
2. **Task Stopped**: Task execution halted (`TaskState::Stopped` / `Cancelled`).
3. **Process Terminated**: Stage 3 process memory destroyed (`ProcessState::Zombie` / `Free`).
4. **Lease Authority Invalidated**: Contractual token revoked (`LeaseState::Released` in `resourced`).
5. **Physical Resource Quiesced**: Hardware signals and DMA operations halted by driver provider.
6. **Physical Capacity Reclaimed**: Capacity returned to available pool ($C_{\text{unavail}} \rightarrow C_{\text{avail}}$ in `resourced`).

$$\mathbf{Invariant\ I-LEASE-INVALIDATION-NO-PHYSICAL-RELEASE:}\quad \text{Lease invalidation does NOT}$$
$$\text{itself make physical capacity available. Committed capacity MUST transition through } C_{\text{unavail}}\text{.}$$

---

## 11. Process Ownership & Supervision Boundary

### 11.1 Substrate Reality Audit: Process Lifecycle Primitives in Frozen Stage 3
Inspection of the Stage 3 microkernel (`kernel/src/syscall/dispatch.rs`, `kernel/src/task/process.rs`, `kernel/src/elf/load.rs`) and Stage 4A supervisor (`init/src/main.rs`, `libzero/src/supervisor.rs`) establishes the exact lifecycle authority model:

```text
[workloadd]  ──1. Task Execution Request──>  [Supervisor / Kernel Boundary]
                                                         │
                                             2. Create Process / AddressSpace
                                             3. Load ELF Binary
                                             4. Instantiate Capability Envelope
                                                         │
[workloadd]  <──5. Return ProcessId & Handle─────────────v
     │
     ├──6. Observe Exit (Completion Event / IPC)─────────┐
     └──7. Cooperative Stop (sys_channel_close)──────────v
                                             [Stage 3 Process] (PML4, Threads)
```

1. **Process Creation Path**:
   - Stage 3 system call dispatch contains **NO `SYS_PROCESS_CREATE` syscall** exposed to ordinary Ring 3 applications.
   - Process creation and ELF loading (`load_elf`, `AddressSpace` allocation, PML4 setup, initial thread binding) are supervisor and kernel loader operations.
   - **Explicit Boundary Statement**: `workloadd` depends on the **Stage 4 System Supervisor Process Spawner Boundary** (`init` supervisor IPC or kernel process control boundary) to instantiate task processes. `workloadd` does not invent a Ring 0 creation syscall.
2. **Process Ownership & ID Allocation**:
   - The Stage 3 Kernel Process Table (`PROCESS_TABLE`) owns the underlying process descriptor and allocates the numeric `ProcessId` (PID).
   - `workloadd` holds logical task ownership (`parent_pid = workloadd.pid`) and binds the returned `ProcessId` to its internal `TaskId`.
3. **Initial Capability Envelope Setup**:
   - `workloadd` derives an attenuated child capability handle from the Workload's root capability envelope via `SYS_CAP_DERIVE` ($\text{ChildRights} \subseteq \text{ParentRights}$).
   - `workloadd` passes this capability token to the supervisor/kernel loader during task process creation, populating the process's initial handle table (`PROCESS_HANDLE_TABLES[pslot]`).
4. **ELF Loading & Thread Initialization**:
   - The kernel ELF loader (`kernel::elf::load_elf`) parses executable segments, maps virtual memory pages into the isolated `AddressSpace`, sets up the user stack frame, and allocates the initial thread.
5. **Process Exit Observation & Replacement**:
   - `workloadd` observes process completion via Stage 3 event handles (`completion_event`), supervisor exit messages, and IPC channel closure notifications.
   - When a task fails or crashes, `workloadd` does not attempt to recycle the faulted address space. `workloadd` requests a **fresh** process container from the supervisor for task retries.

### 11.2 Process & Control Authority Matrix

| Operation | Requester | Required Authority | Executor | Authoritative State Owner | Kernel Boundary / Handle Type | Failure Behavior |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Create Workload** | Client / Agent | Client Root Capability (`CLOSE \| INSPECT`) | `workloadd` | `workloadd` | None (Daemon IPC) | Reject if table full (`ZeroError::ObjectTableFull`). |
| **Create Task** | Workload Owner | Workload Control Capability (`DUPLICATE \| TRANSFER`) | `workloadd` | `workloadd` | None (Daemon IPC) | Reject if cyclic or immutable state (`ZeroError::InvalidLifecycleState`). |
| **Request Lease** | `workloadd` | Attenuated Task Capability (`required_rights`) | `resourced` | `resourced` | Channel Handle passing | Reject if coverage incomplete or quota exceeded (`ZeroError::PermissionDenied`). |
| **Create Process** | `workloadd` | Supervision Request to `init` / Kernel Loader | Supervisor / Kernel | Kernel Process Table | Supervisor IPC / `load_elf` (`parent_pid = workloadd.pid`) | Reject if `MAX_PROCESSES = 16` exhausted (`ZeroError::ObjectTableFull`). |
| **Observe Process Exit** | `workloadd` | Parent Process Role (`parent_pid`) | Kernel / Supervisor | Kernel Zombie Queue | Process Completion Event / IPC Notification | Kernel transitions process to `Zombie`, notifies `workloadd`. |
| **Terminate Process** | `workloadd` | Channel Close or Supervisor Kill Request | Supervisor / Kernel | Kernel Process Table | `sys_channel_close` / `init` Supervisor IPC | Forcibly cancels process threads, reclaims address space to PMM. |
| **Revoke Task Capabilities**| `workloadd` | Parent CDT Capability Handle | Kernel Boundary | Capability CDT | `sys_channel_close` / CDT cascade | Recursive invalidation of all descendant capability nodes. |
| **Release / Invalidate Lease** | `workloadd` | Active `LeaseId` + Generation | `resourced` | Lease Table (`resourced`) | `OP_LEASE_RELEASE` via IPC | Reject if generation mismatch; capacity moves to $C_{\text{unavail}}$. |

---

## 12. Capability Authority

All authority in Stage 4C is rooted in the frozen Stage 3H Capability Model. Stage 4C does not invent secondary permission systems; it strictly wraps and attenuates Stage 3 capabilities.

$$\mathbf{Lease\ Authority} \quad\subseteq\quad \mathbf{Workload\ Authority} \quad\subseteq\quad \mathbf{Capability\ Authority}$$

### 12.1 Capability Attenuation Chain (`SYS_CAP_DERIVE`)
1. **Workload Envelope**: Client presents a Stage 3 capability handle upon workload submission. The rights embedded in this handle establish the maximum security envelope for the workload.
2. **Task Capability Derivation**: For each task process, `workloadd` invokes `SYS_CAP_DERIVE` to produce an attenuated child capability token:
   $$\text{ChildRights} \subseteq \text{ParentRights}$$
3. **Hardware & Resource Verification**: When requesting leases from `resourced`, `workloadd` passes the task capability. `resourced` verifies that the capability contains the required rights (`DEV_READ`, `DEV_WRITE`, `DEV_MAP_MMIO`, etc.) before granting the lease (`I-LEASE-AUTH-BOUNDED`).

---

## 13. Resource/Lease Integration

Stage 4C consumes physical capacity strictly through contractual Stage 4B leases.

### 13.1 Requirement Formulation & 4B Lease Mapping
Each Task declares its resource requirements via a `TaskResourceDemand` struct:
- `resource_type`: `Cpu`, `Memory`, `GpuCore`, `Dma`, `Accelerator`.
- `locality_domain`: `HostLocal`, `Numa0`, `Numa1`, `PcieBus`.
- `required_capacity`: `DimensionCapacityVector` (`[u64; 8]`).
- `min_duration_ticks`: Initial lease TTL.

`workloadd` submits an `OP_LEASE_REQUEST` IPC message to `resourced`. `resourced` performs vector conservation checking:
$$\mathbf{C}_{\text{avail}} \ge \mathbf{C}_{\text{task}} \quad \text{and} \quad \mathbf{A} \cdot (\mathbf{C}_{\text{alloc}} + \mathbf{C}_{\text{task}}) \le \mathbf{b}$$
If admitted, `resourced` issues a `ResourceLease` token (144 bytes) bound to the target `resource_id` and decrements $\mathbf{C}_{\text{avail}}$.

$$\mathbf{Invariant\ I-WORKLOAD-NO-ACCOUNTING-BYPASS:}\quad \text{workloadd must NEVER allocate, bind, or utilize}$$
$$\text{physical hardware resources without presenting a valid, active Stage 4B ResourceLease issued by resourced.}$$

---

## 14. Authority Teardown vs Physical Reclamation

ZeroOS explicitly decouples logical authority teardown from physical hardware reclamation. A rigid single destruction order is prohibited because hardware device reset times vary asynchronously.

### 14.1 Two-Phase Teardown Model

```text
+-----------------------------------------------------------------------------+
| PHASE A: IMMEDIATE AUTHORITY TEARDOWN (Synchronous / Instantaneous)         |
|   1. workloadd revokes Task Capability Handles via kernel CDT.               |
|   2. Task Process execution halted; threads cancelled; CR3 root cleared.    |
|   3. Workload state set to 'Cancelling' / 'Failed'.                         |
|   4. Lease status in resourced set to 'Released' / 'Expired'.               |
|   5. Capacity transferred from C_alloc to C_unavail (Quarantine Ledger).    |
+------------------------------------+----------------------------------------+
                                     | Asynchronous Hardware Quiescence Wait
+------------------------------------v----------------------------------------+
| PHASE B: ASYNCHRONOUS PHYSICAL RECLAMATION (Event-Driven / Provider-Gated)  |
|   1. Kernel PMM reclaims physical page frames (AddressSpace teardown).      |
|   2. Device Driver / Hardware Provider confirms DMA registers reset.        |
|   3. resourced accounting engine processes Quarantine Ledger record.        |
|   4. Capacity transferred from C_unavail to C_avail.                        |
|   5. Process slot in PROCESS_TABLE returned to ProcessState::Free.          |
+-----------------------------------------------------------------------------+
```

---

## 15. Static Bounds and Derivation

All bounds in Stage 4C are derived from Stage 3 kernel constraints, Stage 4B lease semantics, and Stage 4 capacity management policy.

### 15.1 Architectural Limits vs. Design Policies

$$\text{MAX\_PROCESSES\ (16)} \quad\longrightarrow\quad \text{Stage 4 System Capacity Policy} \quad\longrightarrow\quad \text{MAX\_CONCURRENT\_RUNNING\_TASKS\ (8)}$$

1. **Kernel Capacity Baseline**:
   - Stage 3 kernel process table limit: `MAX_PROCESSES = 16`.
   - Active baseline daemons: 5 processes (PID 0 master kernel, PID 1 `init`, PID 2 `brokerd`, PID 3 `resourced`, PID 4 `workloadd`).
   - Unallocated kernel process headroom: $16 - 5 = 11$ available slots.

2. **Stage 4 Process Capacity Policy**:
   - Stage 4 establishes an administrative process capacity allocation policy, reserving 3 process slots for system services.
   - **Policy Framing**:
     - *System Service Reservation*: 3 process slots are held in reserve to ensure system service stability.
     - *Task Concurrency Ceiling*: `MAX_CONCURRENT_RUNNING_TASKS = 8` ($16 - 5 - 3 = 8$).
     - *Starvation Prevention*: Guarantees system services can be instantiated without encountering `ZeroError::ObjectTableFull`.

3. **Stage 4B Lease Bound Mapping (`MAX_LEASES_PER_WORKLOAD`)**:
   - In Stage 4B (`resourced`), each `ResourceLease` (144 bytes) binds to **exactly one `resource_id`** (`ResourceDescriptor`).
   - Stage 4B resource descriptors contain a multi-dimensional capacity vector (`DimensionCapacityVector`), allowing CPU, RAM, GPU, and DMA capacity to be granted within a single lease when managed by one descriptor.
   - **Task Lease Requirement**:
     - A task whose resource demands are satisfied by 1 resource descriptor acquires **exactly 1 lease** per task ($1 \text{ task} \rightarrow 1 \text{ lease}$).
     - If a task requires $M_{\text{desc}}$ distinct hardware resource descriptors (e.g. Host CPU/RAM descriptor + PCIe Accelerator descriptor), it acquires $M_{\text{desc}}$ leases per task.
   - **Workload Lease Bound**:
     - `MAX_LEASES_PER_WORKLOAD` is an **Implementation Configuration / Capacity Allocation Bound** set to 16 by default ($8 \text{ running tasks} \times \le 2 \text{ descriptors/task} = 16 \text{ leases}$).
     - Fits comfortably within Stage 4B's node-wide ceiling `MAX_LEASES_PER_NODE = 256`.

4. **Task Retry Bound (`DEFAULT_MAX_TASK_RETRIES = 3`)**:
   - Classified strictly as a **Bounded Orchestration Policy**, not a kernel-derived architectural limit.
   - Prevents infinite crash loops and zombie queue thrashing.
   - **Exhaustion State Pipeline**:
     $$\text{Task Execution Error / Process Exit != 0} \longrightarrow \text{Increment } \text{retry\_count}$$
     $$\downarrow$$
     $$\text{retry\_count} == \text{max\_retries} \implies \text{No further automatic retry}$$
     $$\downarrow$$
     $$\text{TaskState} \rightarrow \text{Failed}$$
     $$\downarrow$$
     $$\text{Workload Recovery Class Policy Evaluates Next State (Class 1/2/3)}$$

### 15.2 Comprehensive Derivation Matrix

| Parameter | Derived Value | Driving Resource / Invariant | Limit Type | Exhaustion Behavior |
| :--- | :--- | :--- | :--- | :--- |
| **`MAX_CONCURRENT_RUNNING_TASKS`** | **8** | **Kernel Baseline minus System Reservation**: $16 \text{ (MAX\_PROCESSES)} - 5 \text{ (daemons)} - 3 \text{ (reserved service slots)} = 8$. | **Stage 4 Configuration Policy** | Tasks wait in `Ready` queue until a running task exits and yields a process slot. |
| **`MAX_CONCURRENT_WORKLOADS`** | **8** | **Static BSS Footprint**: `workloadd` maintains fixed control blocks ($\approx 512\text{B}$ per Workload). 8 workloads fit in 4096 bytes (1 frame). | **Implementation Configuration** | `OP_WORKLOAD_CREATE` rejected with `ZeroError::ObjectTableFull`. |
| **`MAX_TASKS_PER_WORKLOAD`** | **16** | **Dependency Bitmask Width**: 16-bit bitmask (`u16`) representation for zero-allocation stack topological sorting. | **Hard Architectural Limit** | DAG submission rejected with `ZeroError::UnsupportedResourceShape`. |
| **`MAX_DEPENDENCIES_PER_TASK`** | **15** | **Bitmask Range**: In a 16-task graph, a task can depend on any subset of other tasks ($16 - 1 = 15$). | **Hard Architectural Limit** | Excess dependencies rejected during validation. |
| **`MAX_LEASES_PER_WORKLOAD`** | **16 (default)** | **4B Lease Mapping**: $1 \text{ lease per resource descriptor}$. $8 \text{ tasks} \times \le 2 \text{ descriptors/task} = 16 \text{ leases}$. | **Implementation Configuration** | Lease request queued or rejected if quota exceeded. |
| **`DEFAULT_MAX_TASK_RETRIES`** | **3** | **Bounded Orchestration Policy**: Bounded retries prevent thrashing the Stage 3 kernel zombie queue (`ZOMBIE_PROCESS_QUEUE`). | **Bounded Orchestration Policy** | Task transitions to `Failed`; propagates failure to Workload DAG. |
| **`MAX_HANDLES_PER_TASK_PROCESS`** | **8 / 32** | **Stage 3 `MAX_HANDLES_PER_PROCESS = 32`**: Task process requires $\le 8$ handles (1 control, 2 I/O, 1..5 resource/shm), leaving 24 handles headroom. | **Hard Architectural Limit** | Handle allocation fails with `ZeroError::ObjectTableFull`. |

---

## 16. Failure/Recovery

ZeroOS explicitly defines behavior for all physical and logical failure events:

| Failure Event | Workload State | Task State | Process Action | Capability Action | Lease Action | Recovery / Terminal Result |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Task App Error** | `Running` | `Running -> Failed` | Process exits cleanly (`sys_exit`). | Task capability handles closed. | Lease held if retrying; released if fatal. | Evaluate recovery class: Class 1 retries if $N < N_{\text{max}}$; Class 2 resumes checkpoint; Class 3 fails at milestone. |
| **Process CPU Fault** | `Running -> Recovering` | `Running -> Failed` | Kernel terminates process (`Zombie`). | Capability handles reclaimed by kernel. | Voluntary release or retained for retry. | Request fresh process from supervisor with new PID. Invariant `I-TASK-PROCESS-ASSOCIATION` preserved. |
| **Lease Expiration** | `Running -> Recovering` | `Running -> Blocked` | Process thread paused or killed. | Lease capability token invalidated. | `resourced` marks lease `Expired`; capacity to $C_{\text{unavail}}$. | `workloadd` requests fresh lease. If granted, resume/restart task. |
| **Time Authority Lost**| `Running -> Suspended` | `Running -> Blocked` | Process thread paused immediately. | Temporal capabilities suspended. | `resourced` moves capacity to $C_{\text{unavail}}$ (quarantine). | Fail closed. Wait for time adapter recovery or abort workload. Zero overcommit. |
| **Provider Lost (HW)**| `Running -> Recovering` | `Running -> Failed` | Process terminates if hardware bound. | Hardware capability handles revoked. | `resourced` moves capacity to $C_{\text{unavail}}$. | Re-evaluate placement against surviving resources; reschedule if pure/idempotent. |
| **Client Disconnect** | `Cancelling` | `All -> Cancelled` | Terminate all active task processes. | Close all workload capabilities. | Execute 4-Phase Cancellation Pipeline. | Clean terminal cancellation; zero resource leaks (`I-LEASE-CLEANUP`). |
| **Resource Exhaustion**| `Ready -> Waiting` | `Ready` (Queued) | No process launched. | None. | Pending lease request queued or rejected. | Task waits in queue until leases released, or aborts if deadline expires. |
| **Dependency Failure** | `Cancelling -> Failed` | Downstream `-> Cancelled` | Downstream processes never launched. | Downstream capabilities never derived. | No downstream leases acquired. | Workload terminates in `Failed`. Partial outputs quarantined. |

---

## 17. Persistence

### 17.1 Survival Matrix across Failures
- **Process Crash**: `WorkloadId`, `TaskId`, Task DAG state, accumulated outputs, and active leases survive. Ephemeral PIDs and address spaces perish.
- **`workloadd` Daemon Crash**:
  - `workloadd` state is preserved via durable persistence authority (`MemoryPersistenceAuthority` or non-volatile journal).
  - Upon supervisor restart (`init`), `workloadd` recovers sequence ceiling $S_{\text{ceil}}$, reconstructs active workloads, and inspects surviving processes.
- **Node Reboot**:
  - All Stage 3 memory and processes are lost.
  - `BootEpoch` increments monotonically (`boot_epoch::advance_boot_epoch`).
  - Class 1 workloads perish completely.
  - Class 2 workloads read latest durable checkpoint from ZeroFS and re-submit a recovery Workload DAG.

### 17.2 Durable ID Allocation Crash Ordering
In compliance with `I-ID-DURABLE-ALLOCATOR-STATE`:
$$\text{Persist Sequence Ceiling } S_{\text{ceil}} \quad\longrightarrow\quad \text{Issue WorkloadId / TaskId} \quad\longrightarrow\quad \text{Publish Externally}$$
Upon crash recovery, sequence allocation resumes strictly at $S_{\text{resume}} > S_{\text{ceil}}$, preventing any possibility of duplicate `WorkloadId` issuance across reboots.

---

## 18. IPC/Service Interfaces

`workloadd` registers service endpoint `"workload.service"` with `brokerd` via Stage 3G IPC:

```rust
// Protocol OpCodes for workloadd
pub const OP_WORKLOAD_CREATE:         u32 = 0x0000_4C01;
pub const OP_WORKLOAD_CREATE_RESP:    u32 = 0x0000_4C02;
pub const OP_WORKLOAD_SUBMIT_DAG:     u32 = 0x0000_4C03;
pub const OP_WORKLOAD_SUBMIT_DAG_RESP:u32 = 0x0000_4C04;
pub const OP_WORKLOAD_QUERY:          u32 = 0x0000_4C05;
pub const OP_WORKLOAD_QUERY_RESP:     u32 = 0x0000_4C06;
pub const OP_WORKLOAD_CANCEL:         u32 = 0x0000_4C07;
pub const OP_WORKLOAD_CANCEL_RESP:    u32 = 0x0000_4C08;
pub const OP_WORKLOAD_EXTEND_DAG:     u32 = 0x0000_4C09;
pub const OP_WORKLOAD_EXTEND_DAG_RESP:u32 = 0x0000_4C0A;
```

---

## 19. Security/Threat Analysis

### 19.1 Threat Model & Safeguards
1. **Capability Amplification**:
   - *Threat*: Task attempts to acquire rights beyond parent workload envelope.
   - *Mitigation*: Monotonic attenuation via `SYS_CAP_DERIVE`. Kernel enforces $\text{ChildRights} \subseteq \text{ParentRights}$.
2. **Accounting & Lease Bypass**:
   - *Threat*: Task attempts execution without active Stage 4B lease.
   - *Mitigation*: Physical device MMIO and DMA access are gated by capability tokens issued exclusively by `resourced` upon lease grant (`I-WORKLOAD-NO-ACCOUNTING-BYPASS`).
3. **Identifier Replay & Stale State Injection**:
   - *Threat*: Attacker sends IPC messages referencing retired `WorkloadId` or stale `LeaseId`.
   - *Mitigation*: `local_seq` monotonically increments without reuse (`I-WORKLOAD-ID-NONREUSE`). Every state mutation increments 32-bit `generation`. Stale messages rejected fail-closed.
4. **Duplicate Task Execution Race**:
   - *Threat*: Race condition causes simultaneous execution of one task in multiple processes.
   - *Mitigation*: `workloadd` state transitions are serialized under internal lock. Task transition `Ready -> Running` occurs atomically with process slot reservation.

---

## 20. Invariants

The Workload Orchestration Subsystem is governed by 20 non-negotiable architectural invariants:

1. **`I-WORKLOAD-PROCESS-SEPARATION`**: A Process is an isolated kernel execution primitive; a Workload is a Stage 4 unit of intended computation. A Workload must never be implemented as a simple process wrapper.
2. **`I-WORKLOAD-ID-NONREUSE`**: Every `WorkloadId` is issued from a durable monotonic sequence and is permanently retired upon terminal completion or failure; identifiers are never reused.
3. **`I-TASK-DAG-ACYCLIC`**: The Task Graph of a Workload is strictly acyclic; directed cycles represent deadlocks and are rejected at admission.
4. **`I-TASK-DAG-DEPENDENCY-CORRECTNESS`**: A task cannot transition to `Ready` or `Running` until all ancestor tasks in the DAG have successfully transitioned to `Completed`.
5. **`I-TASK-DAG-IMMUTABLE-EXISTING`**: Once admitted, an existing task's dependencies, resource demands, and code targets are permanently immutable.
6. **`I-TASK-DAG-DOWNSTREAM-EXTENSION`**: Dynamic DAG extensions may only append new downstream tasks that depend on existing tasks; new tasks cannot become prerequisites for already-admitted tasks.
7. **`I-COMPLETED-WORKLOAD-IMMUTABLE`**: Workloads in `Completed` state cannot be extended or mutated; new computation requires a new Workload submission.
8. **`I-WORKLOAD-CAPABILITY-SUBSET`**: All capabilities derived for tasks within a Workload must be strictly attenuated subsets of the parent Workload Capability Envelope.
9. **`I-WORKLOAD-LEASE-SUBSET`**: Leases requested for a task must not exceed the authority granted by the task's authorizing capability token.
10. **`I-WORKLOAD-NO-ACCOUNTING-BYPASS`**: `workloadd` must never allocate, bind, or utilize physical hardware resources without an active Stage 4B `ResourceLease` issued by `resourced`.
11. **`I-WORKLOAD-RESOURCE-REQUIREMENT-CONSISTENCY`**: The resource capacity requested for a task must match the task's declared `DimensionCapacityVector` and satisfy `resourced` coupling constraints.
12. **`I-WORKLOAD-LIFECYCLE-CONSISTENCY`**: Workload state transitions must strictly adhere to the formal state machine; terminal states (`Completed`, `Failed`, `Cancelled`, `Reclaimed`) are irreversible.
13. **`I-TASK-LIFECYCLE-CONSISTENCY`**: Task state transitions must strictly follow the canonical task lifecycle; a failed task cannot transition directly to completed without re-execution.
14. **`I-TASK-PROCESS-ASSOCIATION`**: A Stage 3 Process may execute at most one Task of one Workload at any time; processes are never shared across workloads or tasks.
15. **`I-WORKLOAD-FAILURE-NO-RESURRECTION`**: A Workload in `Failed` or `Cancelled` cannot be resurrected; retries occur via fresh process instantiation under the existing Workload or via a newly submitted Workload.
16. **`I-RECOVERY-CLASS-1-IDEMPOTENT-REPLAY`**: Class 1 recovery permits full task replay from scratch; duplicate execution produces zero external side-effects.
17. **`I-RECOVERY-CLASS-2-CHECKPOINTED-RESUME`**: Class 2 recovery resumes execution exclusively from the latest verified checkpoint generation $G_c$.
18. **`I-RECOVERY-CLASS-3-NO-TRANSPARENT-REPLAY`**: Class 3 recovery strictly prohibits transparent replay; failed tasks transition immediately to `FailedAtMilestone(M)`.
19. **`I-WORKLOAD-CANCELLATION-QUARANTINE`**: Cancelling a workload revokes lease authority immediately, but committed physical capacity MUST enter $C_{\text{unavail}}$ and cannot return to $C_{\text{avail}}$ until the provider confirms physical release and quiescence.
20. **`I-LEASE-INVALIDATION-NO-PHYSICAL-RELEASE`**: Invalidation of a lease due to `TimeAuthorityLost` or `ProviderLost` moves capacity to $C_{\text{unavail}}$ until physical quiescence or reset is verified.

---

## 21. Dependency Graph

```text
Stage 3 Microkernel Nucleus (Frozen: Processes, Threads, Caps, VM, SMP)
  ↓
Stage 4A System Services (Frozen: libzero, init PID 1, brokerd)
  ↓
Stage 4B Resource Accounting (Frozen: resourced, Resource Graph, Leases, Quarantine)
  ↓
Stage 4C Workload Orchestration (workloadd: Task DAG, Recovery Classes, 4-Phase Cancellation)
  ↓
Stage 4D Workspace Container Subsystem (Future: workspaced)
  ↓
Stage 4E Autonomous Agent Runtime Subsystem (Future: agentd)
  ↓
Stage 4F Distributed Fabric Subsystem (Future: fabricd / intentd)
```

---

## 22. Non-Goals

The following are strictly out of scope for Phase 4C:
- **No Implementation Code**: No daemons, libraries, or test runners implemented in this phase.
- **No Kernel Changes**: 0 bytes modified in Stage 3 microkernel.
- **No Stage 4A/4B Modifications**: No modifications to `init`, `brokerd`, or `resourced`.
- **No Workspace UI or Context Storage**: Workspaces belong exclusively to Phase 4D.
- **No Autonomous Agent Reasoning**: LLM loops and perception belongs to Phase 4E.
- **No Distributed Fabric Networking**: Remote P2P node mesh belongs to Phase 4F.
- **No Dynamic Kernel Allocations**: All structures remain statically bounded.

---

## 23. Open Issues

None. All architectural findings (process creation/supervision boundary, 4B lease mapping, process capacity policy framing, recovery classes, and cancellation quarantine pipeline) have been definitively resolved.

---

## 24. Future Implementation Boundary

When implementation of Stage 4C is authorized in subsequent phases, `workloadd` will be built as a freestanding Ring 3 binary in `workloadd/src/main.rs` consuming `libzero` and communicating with `init`, `brokerd`, and `resourced` strictly via Stage 3G IPC.

---

STATUS: DRAFT — REQUIRES REVIEW
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
