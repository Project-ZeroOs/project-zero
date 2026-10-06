# STAGE 4C ARCHITECTURE SPECIFICATION (REVISION 2)

## Workload Orchestration Subsystem Architecture Discovery & Dependency Audit

**Status:** 🟡 **DRAFT — REQUIRES REVIEW (REVISION 2)** (2026-09-23)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024, Stage 4B Architecture Rev12 (`STAGE4B-ARCHITECTURE-REV12.md`), ADR-0025 Rev12  
**Kernel Baseline:** Stage 3A–3N Inviolate (0 Bytes Modified, Frozen Ring 0 Microkernel)  
**System Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Frozen, Stage 4B (`resourced`, Generic Resource Graph, Local Node Accounting, Leases) Frozen  
**Target Subsystem:** Stage 4C Workload Orchestration Subsystem (`workloadd`)

---

## 1. Executive Summary

ZeroOS Stage 4C establishes the **Workload Orchestration Subsystem**, bridging high-level computational intent and the concrete, capability-bounded execution substrates of the operating system.

Prior to Stage 4C, ZeroOS possesses:
1. **Stage 3 Microkernel Nucleus (Frozen)**: Authoritative Ring 0 hardware isolation, pre-allocated physical frame management (`PMM`), virtual address space manipulation (`CR3`, `VMM`), priority-preemptive thread scheduling across SMP cores, capability tables (Stage 3H), fast capability-checked IPC and shared memory (Stage 3G), and isolated Process abstractions (Stage 3F).
2. **Stage 4A System Services (Frozen)**: Freestanding user-space runtime (`libzero`), system service supervisor and BootEpoch authority (`init`, PID 1), and capability-mediated service discovery directory (`brokerd`).
3. **Stage 4B Local Node Resource & Lease Accounting (Frozen)**: Generic heterogeneous Resource Graph, multi-dimensional vector conservation ($C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$), quota enforcement, and hardware-qualified monotonic temporal capacity leases (`resourced`).

Stage 4C introduces **`workloadd`**, the authoritative Ring 3 daemon that translates computational intent into verified, acyclic **Task Directed Acyclic Graphs (DAGs)**, evaluates multi-dimensional resource requirements against Stage 4B resource descriptors, acquires capability-bounded temporal capacity leases from `resourced`, orchestrates the execution lifecycle of Stage 3 processes to execute tasks, and enforces deterministic failure, retry, cancellation, and reclamation policies.

Crucially, Stage 4C formalizes the immutable architectural boundary:
$$\mathbf{Process} \neq \mathbf{Workload}$$
- A **Process** is a local kernel execution and memory-isolation primitive (an address space, page table root `CR3`, thread group, and handle table).
- A **Workload** is a Stage 4 unit of intended computation (a structured Task DAG, resource envelope, execution policy, capability envelope, lifecycle above processes, and multi-process failure/recovery domain).

### 1.1 Revision 2 Architectural Resolutions
Revision 2 addresses the four primary findings of the Phase 4C architecture review:
1. **Recovery-Class-Dependent Invariant (`I-WORKLOAD-RECOVERY-CLASS-DEPENDENT`)**:
   Replaces universal restart idempotence. Acknowledges that Class 3 (Irreversible External) computations cannot guarantee deterministic replay or absence of duplicate side effects. Formalizes explicit recovery semantics per workload class.
2. **Quarantine-Preserving Cancellation Pipeline (`I-WORKLOAD-CANCELLATION-QUARANTINE`)**:
   Reconciles cancellation with Stage 4B accounting. Enforces that lease invalidation or surrender during cancellation never immediately releases physical capacity to $C_{\text{avail}}$. Establishes the 4-phase cancellation pipeline: Request $\rightarrow$ Notification/Quiescence $\rightarrow$ Lease Surrender/Quarantine $\rightarrow$ Provider Confirmation.
3. **Architecturally Derived Static Bounds**:
   Replaces asserted bounds with rigorous derivations grounded directly in Stage 3 kernel limits (`MAX_PROCESSES = 16`, `MAX_HANDLES = 32`) and Stage 4B capacities (`MAX_LEASES_PER_NODE = 256`). Establishes `MAX_CONCURRENT_RUNNING_TASKS = 8` based on remaining unallocated kernel process slots.
4. **Concrete Process-Control & Capability Authority Matrix**:
   Eliminates ambient authority claims. Defines the explicit authority, requester, executor, and kernel handle types for workload creation, task scheduling, process spawning, process termination, and capability derivation.

---

## 2. Phase Boundary & Frozen Substrate Preservation

### 2.1 Preserved Stage 3 Microkernel Baseline (Ring 0)
Stage 3 represents the inviolate hardware-enforcing microkernel nucleus. Stage 4C adheres strictly to the following frozen boundaries:
- **0 Bytes Kernel Modification**: Stage 4C introduces zero kernel modifications, zero new syscalls, zero kernel heap allocations, and zero kernel-level workload abstractions.
- **Process Model (Stage 3F, ADR-0015)**: Bounded process table (`MAX_PROCESSES = 16`), intrusive zombie queue, address-space isolation (`CR3`), thread group lifecycle (`Creating -> Active -> Terminating -> Zombie -> Reclaiming -> Free`).
- **Capability Model (Stage 3H, ADR-0017)**: Handle tables per process (`MAX_HANDLES_PER_PROCESS = 32`), monotonic derivation (`SYS_CAP_DERIVE`), rights attenuation ($\text{ChildRights} \subseteq \text{ParentRights}$), and kernel-enforced permission gates.
- **IPC & Shared Memory (Stage 3G, ADR-0006/0016)**: Fast synchronous/asynchronous channel IPC (`SYS_CHANNEL_*`), cross-process handle transfer, and page-backed shared memory (`SYS_SHM_*`).
- **Preemptive SMP Scheduling (Stage 3B/3E/3N, ADR-0011/0023)**: Kernel thread priorities and LAPIC timer preemption across SMP cores (up to 4 cores). Stage 4C schedulers do **not** schedule CPU instruction streams; they schedule task readiness and process activation.

### 2.2 Preserved Stage 4A Substrate Baseline (Ring 3)
- **`libzero` (ADR-0024)**: Freestanding `#![no_std]` runtime providing syscall wrappers, IPC framing, error definitions, and core data structures.
- **`init` (ADR-0024, ADR-0025)**: PID 1 supervisor managing system daemon lifecycles (`Supervisor`), persistent BootEpoch maintenance (`boot_epoch::advance_boot_epoch`), and monotonic Time Authority Adapter publishing to `TimeObservationFrame`.
- **`brokerd` (ADR-0024)**: Service directory managing capability-verified IPC endpoints (`register_service`, `lookup_service`, `delegate_cap`).

### 2.3 Preserved Stage 4B Substrate Baseline (Ring 3)
- **`resourced` Daemon (ADR-0025 Rev12, STAGE4B-ARCHITECTURE-REV12)**: Authoritative manager of the generic heterogeneous Resource Graph, multi-dimensional capacity bookkeeping, and capacity leases.
- **Data Structures**:
  - `DistributedId` (128 bits: `node_id: u64`, `local_seq: u64`): Composite globally unique identifier.
  - `DimensionCapacityVector` (72 bytes: `dimensions: [u64; 8]`, `dimension_count: u8`): Multi-dimensional resource vector with checked arithmetic.
  - `ResourceDescriptor` (128 bytes): Heterogeneous hardware descriptor with `ResourceType`, `LocalityDomain`, `ResourceState`, `EnergyTier`.
  - `ResourceLease` (144 bytes): Contractual capacity token with `lease_id`, `resource_id`, `allocated_capacity`, `expiration_tick`, `generation`.
  - `CouplingConstraintMatrix`: Linear joint constraint evaluator ($\mathbf{A} \cdot \mathbf{C} \le \mathbf{b}$).
  - `TimeObservationFrame`: Atomic seqlock-protected monotonic hardware timestamp buffer.
- **Frozen 4B Accounting & Lease Invariants**:
  - `I-ACCOUNTING-CONSERVATION`: $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$.
  - `I-LEASE-AUTH-BOUNDED`: $\text{Lease Authority} \subseteq \text{Capability Authority}$.
  - `I-TIME-FAILURE-ACCOUNTING-SAFETY`: Invalidation of a lease (via `TimeAuthorityLost` or `ProviderLost`) invalidates lease authority immediately, but committed capacity moves to $C_{\text{unavail}}$ (quarantine) and **never** immediately increases $C_{\text{avail}}$ until the provider confirms physical resource release/quiescence.

```text
+-----------------------------------------------------------------------------+
| STAGE 4C: WORKLOAD ORCHESTRATION SUBSYSTEM (workloadd)                      |
|                                                                             |
|  * Workload Lifecycle State Machine                                         |
|  * Task DAG Acyclicity & Dependency Resolution                              |
|  * Resource Requirement Formulation (DimensionCapacityVector)               |
|  * Capability Attenuation & Process Binding                                 |
|  * Local Placement & Multi-Process Failure / Retry Policies                 |
+------------------------------------+----------------------------------------+
                                     | Consumes Leases & Quotas via IPC
+------------------------------------v----------------------------------------+
| STAGE 4B: LOCAL NODE ACCOUNTING & RESOURCE GRAPH (resourced) [FROZEN]       |
|                                                                             |
|  * Heterogeneous Resource Graph (Cycles Permitted)                          |
|  * N-Dimensional Capacity Bookkeeping (Conservation Invariant)              |
|  * Monotonic Temporal Capacity Leases (Seqlock Time Authority)              |
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

## 3. Dependency Direction Audit & Phase Boundary Establishment

### 3.1 Repository Dependency Direction Analysis
In the original preliminary roadmap sketch in `STAGE4-ARCHITECTURE-REV6.md` (Section 16), Stage 4C was tentatively labeled "Distributed Node Trust, Fabric Mesh & CSDT (`fabricd`)", with Workload Orchestration placed at 4D.

However, detailed architectural inspection of the repository, implementation logs, and Stage 4B verification reveals that **Workload Orchestration MUST strictly precede Distributed Fabric and Workspace Subsystems**:

```text
PROPOSED & AUDITED STAGE 4 SEQUENCE:
Stage 3 Microkernel Nucleus
  ↓
Stage 4A Core Service Runtime & Supervisor (init, brokerd, libzero)
  ↓
Stage 4B Local Node Resource Graph & Accounting Leases (resourced)
  ↓
Stage 4C Workload Orchestration & Task Execution Subsystem (workloadd)
  ↓
Stage 4D Workspace Container & Persistent Context (workspaced)
  ↓
Stage 4E Autonomous Agent Runtime & Two-Man Rule Supervision (agentd)
  ↓
Stage 4F Distributed Fabric Mesh & Policy-Driven Intent Resolution (fabricd / intentd)
```

### 3.2 Proof of Dependency Direction
1. **Local Node Execution Must Precede Distributed Remote Dispatch**:
   - A single physical machine must be capable of admitting a workload, constructing its task DAG, evaluating its resource requirements, acquiring local Stage 4B leases from `resourced`, and executing tasks within Stage 3 processes **locally** before any mechanism can intelligently offload or distribute tasks across a network.
   - Fabric (`fabricd`) is an optimization and transport layer between nodes. It is impossible to specify what `fabricd` transports, leases, or migrates if the fundamental unit of computation—the **Workload**—has not yet been defined and locally implemented.
2. **Workspaces (4D) and Agents (4E) are Consumers of Workloads**:
   - As established in ADR-0024 and STAGE4-ARCHITECTURE-REV6.md, Workspaces (Phase 4D) define persistent file context and security envelopes. Autonomous Agents (Phase 4E) run goal perception loops and *submit Workload DAGs* to execute tasks.
   - If Workload Orchestration (4C) does not exist, neither Workspaces nor Agents have an execution target for computational tasks. They would be forced to execute raw processes directly, destroying the architectural goal of goal-driven orchestration and re-coupling application logic directly to kernel primitives.
3. **Stage 4B Contract Completeness**:
   - Stage 4B (`resourced`) explicitly exposes IPC endpoints: `OP_RES_DISCOVER`, `OP_RES_QUERY`, `OP_LEASE_REQUEST`, `OP_LEASE_RENEW`, `OP_LEASE_RELEASE`, and `OP_QUOTA_QUERY`.
   - The primary intended client of these APIs on the local node is **`workloadd`**.
   - `workloadd` consumes these Stage 4B APIs to translate Task resource requirements into active leases.

**Conclusion:** The dependency sequence `Stage 4B -> Stage 4C (Workload Orchestration) -> Stage 4D (Workspace) -> Stage 4E (Agent) -> Stage 4F (Distributed Fabric)` is architecturally necessary and mathematically coherent.

---

## 4. The Workload Semantic Model

### 4.1 Definition: The Workload as the Unit of Intended Computation
A **Workload** is an authoritative Stage 4 administrative and operational object representing a goal-directed computational request. It encompasses:
1. A unique, non-reusable **`WorkloadId`** (`DistributedId`).
2. An owner/creator identity (Client PID, Workspace ID, or Agent ID).
3. A bounded, directed acyclic graph (DAG) of **Tasks**.
4. An aggregated multi-dimensional **Resource Envelope** derived from task demands.
5. A bounded **Capability Envelope** defining maximum permitted rights.
6. A set of active **Stage 4B Resource Leases** acquired to fulfill execution requirements.
7. Associations with transient **Stage 3 Processes**.
8. Global execution policies: scheduling priority, end-to-end deadline, locality domain, privacy level, energy policy, and failure/recovery semantics.

### 4.2 Workload Identity & Non-Reuse (`I-WORKLOAD-ID-NONREUSE`)
Every Workload is assigned a `WorkloadId`:
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DistributedId {
    pub node_id: u64,
    pub local_seq: u64,
}
```
- Issued exclusively by `workloadd`'s local `DistributedIdAllocator` backed by Stage 4B durable persistence authority.
- **Monotonic Non-Reuse**: `local_seq` strictly increments. Once a `WorkloadId` terminates (`Completed`, `Failed`, `Cancelled`, `Reclaimed`), it is **permanently retired**. It is never reused under any circumstances.
- **Incarnation Generation**: The Workload maintains a monotonic 32-bit `generation` counter. Any restart or recovery increments `generation`. IPC messages presenting stale generations are rejected fail-closed with `ZeroError::StaleGeneration`.

### 4.3 Recovery-Class-Dependent Semantics (`I-WORKLOAD-RECOVERY-CLASS-DEPENDENT`)
Workloads in ZeroOS are partitioned into three distinct recovery classes with explicit, non-overlapping semantics:

| Classification | Lifecycle Semantics | State Durability | Failure / Recovery Action | Invariant Guarantee |
| :--- | :--- | :--- | :--- | :--- |
| **Class 1: Ephemeral Pure Compute** | Memory-resident only. Terminates on node reset or unrecoverable task crash. | Zero non-volatile state. Discarded on process or daemon crash. | **Safe Re-execution**: Discard partial state; restart task/process from scratch. | **Replay Safe**: Deterministic re-execution permitted; zero external side-effect risk. |
| **Class 2: Checkpointed Stateful** | State periodically committed to non-volatile checkpoints via ZeroFS / persistent descriptors. | Checkpoint metadata persisted with generation numbers. | **Resume from Checkpoint**: Roll back task state to latest verified checkpoint; resume task without re-running completed parent tasks. | **Bounded Idempotence**: Execution strictly resumes from milestone checkpoint generation $G_c$; intermediate uncheckpointed work is lost. |
| **Class 3: Irreversible External** | Involves external physical side-effects (device actuation, network transmissions, irreversible hardware states). | Explicit milestone logging before external execution. | **Compensate / Fail**: Transparent rollback is physically impossible. Transitions to `FailedAtMilestone(M)`. Executes registered compensation task if declared. | **No Universal Replay**: Replay prohibited. Duplicate execution cannot be prevented by replay; compensation tasks must be explicitly declared in the DAG. |

$$\mathbf{Invariant\ I-WORKLOAD-RECOVERY-CLASS-DEPENDENT:}\quad \text{Recovery behavior is strictly determined by}$$
$$\text{workload class. Class 1 permits replay from scratch; Class 2 resumes from verified checkpoint;}$$
$$\text{Class 3 strictly prohibits transparent replay and transitions to } \text{FailedAtMilestone}(M)\text{ fail-closed.}$$

---

## 5. Workload vs. Process: The Inviolate Separation

ZeroOS strictly enforces the distinction between kernel execution containers and Stage 4 computational units:

$$\mathbf{Process\ (Stage\ 3)} \quad\neq\quad \mathbf{Workload\ (Stage\ 4)}$$

### 5.1 Architectural Contrast Matrix

| Feature | Stage 3 Process (`kernel::task::process`) | Stage 4 Workload (`workloadd`) |
| :--- | :--- | :--- |
| **Execution Domain** | Single virtual address space (`CR3`), local CPU core, thread group. | Multi-task directed graph; coordinates multiple address spaces/processes. |
| **Lifetime & Lifecycle** | Tied directly to thread execution: `Creating -> Active -> Terminating -> Zombie -> Reclaiming -> Free`. | Goal-driven milestone state machine: `Creating -> Ready -> Running -> Completed/Failed`. Survives process crashes. |
| **Authority Primitive** | Process-local Capability Handle Table (`MAX_HANDLES = 32`). | Global Workload Capability Security Envelope; attenuates rights per task. |
| **Resource Binding** | Bounded kernel handles; no concept of time-bounded physical capacity vectors. | Bounded Stage 4B Temporal Leases (`ResourceLease`); quota-checked and metered. |
| **Fault Boundary** | Hardware exception or fatal signal destroys the address space permanently. | Process crash is captured as a task-level event; evaluated by retry/recovery policy. |
| **Identity Continuity** | PID is freed upon kernel reclamation; never survives process termination. | `WorkloadId` persists across process crashes, task retries, and daemon restarts. |
| **Machine Boundary** | **Strictly local to a single kernel instance (`I-NO-LIVE-PROCESS-MIGRATION`).** | Logically separable; tasks can be scheduled locally or dispatched across fabric nodes. |

### 5.2 Six Foundational Architectural Questions & Justifications

#### 1. Can one Workload contain multiple Processes?
**YES.** A Workload represents an entire computation. A pipeline DAG consisting of multiple tasks can execute tasks in separate processes—either concurrently (up to `MAX_CONCURRENT_RUNNING_TASKS = 8`) or sequentially.

#### 2. Can one Process execute tasks from multiple Workloads?
**NO (`I-TASK-PROCESS-ASSOCIATION`).** A Stage 3 Process is strictly dedicated to executing a single Task of a single Workload at any given time.
- *Justification*:
  1. **Address Space Isolation & Zero Residual State**: Executing tasks from different workloads (or even consecutive tasks of different security tiers) within the same address space allows residual memory, cached secrets, heap fragmentation, and dangling pointers to contaminate subsequent executions.
  2. **Deterministic Cleanup via Kernel PMM**: Upon process exit, Stage 3's `reclaim_process_resources_locked` guarantees that all mapped page frames, page tables, and thread stacks are completely returned to the Physical Memory Manager without leaks.
  3. **Capability Table Reset**: A fresh process begins with a clean handle table (32 empty slots), preventing handle leakage across tasks.

#### 3. What happens when a task crashes but its Process survives?
If a task encounters an unrecoverable computational error or runtime assertion but the process thread does not fault, the task runner library inside the process calls `workloadd`'s failure IPC reporting endpoint and terminates cleanly via `sys_exit(code)`. `workloadd` transitions the task to `TaskState::Failed` and cleans up the process.

#### 4. What happens when the Process crashes?
When a process suffers an unhandled CPU fault (e.g., page fault on unmapped address, general protection fault, invalid opcode), the Stage 3 kernel terminates the process, transitions it to `Zombie`, and wakes parent/monitoring threads.
`workloadd` receives the process termination notification:
1. The associated task transitions to `TaskState::Failed`.
2. Associated task capabilities and transient shared memory mappings are closed.
3. `workloadd` inspects the Workload's retry policy (`max_retries`, backoff).
4. If retries remain, `workloadd` launches a **fresh** Stage 3 process to re-execute the task.
5. If retries are exhausted, the failure propagates across the Task DAG, transitioning dependent tasks to `Cancelled` and the Workload to `WorkloadState::Failed`.

#### 5. Can a Workload restart using new Processes?
**YES.** The Workload is decoupled from specific kernel PIDs. During a task retry, or when resuming a checkpointed Workload (Class 2), `workloadd` instantiates completely new Stage 3 processes with new address spaces and new thread groups.

#### 6. Which identity remains authoritative across restart?
The **`WorkloadId`** (and internal **`TaskId`**) remains authoritative and invariant across all process terminations and task retries. Kernel PIDs are purely ephemeral execution containers.

---

## 6. The Workload Task DAG Engine

Stage 4C orchestrates computation using a directed acyclic graph of tasks:

$$\mathbf{Resource\ Graph\ (Stage\ 4B)} \quad\neq\quad \mathbf{Task\ DAG\ (Stage\ 4C)}$$
- **Resource Graph**: Models physical hardware components and capacity topology. Directed cycles are explicitly permitted (e.g., `Node -> Device -> Storage -> Swap -> RAM -> Node`).
- **Task DAG**: Models computational dataflow and execution precedence. Must be **strictly acyclic** (`I-TASK-DAG-ACYCLIC`). Cycles represent unresolvable execution deadlocks and are rejected fail-closed at admission.

### 6.1 Task Structure & State Machine
Every Task within a Workload has a unique 16-bit `TaskId` (index `0..MAX_TASKS_PER_WORKLOAD-1`).

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    Unallocated = 0,
    Blocked = 1,        // Waiting for incoming DAG dependencies to complete
    Ready = 2,          // All dependencies satisfied; eligible for lease acquisition & placement
    LeaseAcquired = 3,  // Stage 4B leases successfully granted; ready for process launch
    Running = 4,        // Stage 3 process is actively executing the task
    Completed = 5,      // Task execution succeeded; outputs committed
    Failed = 6,         // Task execution failed; retries exhausted or terminal error
    Cancelled = 7,      // Ancestor task failed or workload was cancelled; skipped
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
      +---------------+   |   +-----------------------+   |
      |                   |                           |   |
      v                   v                           v   v
+-----------+       +-----------+               +-----------+
|  Failed   |       | Completed |               | Cancelled |
+-----------+       +-----------+               +-----------+
```

### 6.2 DAG Representation & Topological Cycle Detection
To maintain deterministic execution without dynamic heap allocation in user space:
- **Maximum Tasks per Workload**: `MAX_TASKS_PER_WORKLOAD = 16`.
- **Dependency Representation**: A 16-bit bitmask `dependencies: u16` where bit $j$ is set if Task $i$ depends on Task $j$.
- **Admittance Cycle Detection**:
  Upon submission of a Task DAG, `workloadd` executes Kahn's Algorithm:
  1. Calculate in-degrees for all defined tasks from the dependency bitmask.
  2. Enqueue all tasks with in-degree 0 into a static 16-entry ring buffer.
  3. Dequeue tasks, decrementing in-degrees of their downstream dependents.
  4. If the number of processed tasks equals the total defined task count, the graph is verified **strictly acyclic**.
  5. If any task remains with in-degree $> 0$, the graph contains a directed cycle: `workloadd` rejects the submission immediately with `ZeroError::CyclicDependency`.

### 6.3 Dynamic DAG Extension Authority & Invariants
ZeroOS permits dynamic DAG extension (e.g., dynamically generating downstream processing tasks based on intermediate results) under strict architectural constraints:
1. **Mutation Authority**: Only the authenticated Workload Owner holding the Workload Control Capability may submit extensions.
2. **Lifecycle State Constraint**: Mutations are permitted **only** while the Workload is in `Ready` or `Running`. If the Workload is in any terminal state (`Completed`, `Failed`, `Cancelled`, `Reclaimed`), mutations are rejected fail-closed with `ZeroError::InvalidLifecycleState`.
3. **Immutability of Admitted Tasks (`I-TASK-DAG-IMMUTABLE-EXISTING`)**: Existing tasks cannot have their dependencies, resource demands, or code targets modified.
4. **Downstream-Only Extension (`I-TASK-DAG-DOWNSTREAM-EXTENSION`)**: New tasks may only declare dependencies on existing tasks. New tasks cannot become prerequisites for tasks already in `Ready`, `Running`, or `Completed`.
5. **Cycle Verification**: Every extension pass re-evaluates global acyclicity across the combined graph before admitting new tasks.

---

## 7. Stage 4B Resource Requirement & Lease Integration

Stage 4C sits immediately above the frozen Stage 4B substrate and consumes physical capacity strictly through contractual leases.

### 7.1 The Requirement Formulation Contract
Each Task declares its resource requirements via a structured demand descriptor:
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TaskResourceDemand {
    pub resource_type: ResourceType,              // Cpu, Memory, GpuCore, etc.
    pub locality_domain: LocalityDomain,          // HostLocal, Numa0, Numa1, PcieBus
    pub required_capacity: DimensionCapacityVector, // [u64; 8]
    pub min_duration_ticks: u64,                  // Initial lease TTL
    pub coupling_constraints: Option<CouplingConstraintMatrix>,
}
```

```text
+-----------------------------------------------------------------------------+
| Task Resource Demand (Stage 4C)                                             |
|   ResourceType: Memory                                                      |
|   Locality: HostLocal                                                       |
|   Required: [64 MiB, 0, ...]                                                |
|   TTL: 500,000 ticks                                                        |
+------------------------------------+----------------------------------------+
                                     |
                                     | 1. Query Resource Graph (OP_RES_DISCOVER / OP_RES_QUERY)
                                     | 2. Acquire Lease (OP_LEASE_REQUEST)
                                     v
+-----------------------------------------------------------------------------+
| Stage 4B Accounting & Lease Authority (resourced)                           |
|   1. Check Capability Coverage (req.handles[0] or auth token)               |
|   2. Check Quota Ceiling (check_and_charge_quota)                           |
|   3. Vector Conservation Admission (avail -= req, alloc += req)             |
|   4. Issue ResourceLease Token                                              |
+------------------------------------+----------------------------------------+
                                     |
                                     v
+-----------------------------------------------------------------------------+
| Active ResourceLease Token (Stage 4C)                                       |
|   lease_id: DistributedId { node_id: 1, local_seq: 104 }                    |
|   generation: 1                                                             |
|   expiration_tick: 1,500,000                                                |
|   allocated_capacity: [64 MiB, 0, ...]                                      |
+-----------------------------------------------------------------------------+
```

### 7.2 Strict Boundary: 4C vs. 4B
ZeroOS enforces an absolute separation of responsibilities between Stage 4C and Stage 4B:

| Mechanism | Stage 4C (`workloadd`) | Stage 4B (`resourced`) |
| :--- | :--- | :--- |
| **Physical Hardware Descriptors** | Consumes descriptors; never creates or modifies them. | **Authoritative**: Registers and updates hardware facts and PMIC telemetry. |
| **Capacity Bookkeeping** | Formulates requirement vectors; never tracks physical counters. | **Authoritative**: Maintains vector conservation ($C_{\text{avail}}, C_{\text{alloc}}, C_{\text{unavail}}$). |
| **Lease Authority** | Requests, holds, renews, and voluntarily releases leases. | **Authoritative**: Issues `ResourceLease`, assigns expiration tick, tracks generation. |
| **Quarantine & Physical Release** | Observes `TimeAuthorityLost` / `ProviderLost`; quiesces tasks. | **Authoritative**: Manages Quarantine Ledger; gates return of capacity to $C_{\text{avail}}$. |
| **Process Execution** | Spawns, monitors, and terminates Stage 3 processes. | **Zero Process Knowledge**: Completely decoupled from processes and threads. |

$$\mathbf{Invariant\ I-WORKLOAD-NO-ACCOUNTING-BYPASS:}\quad \text{workloadd must NEVER allocate, bind, or utilize}$$
$$\text{physical hardware resources without presenting a valid, active Stage 4B ResourceLease issued by resourced.}$$

---

## 8. Capability & Security Integration

All authority in ZeroOS is rooted in the frozen Stage 3H Capability Model. Stage 4C does not invent a secondary permission system; it strictly wraps and attenuates Stage 3 capabilities.

$$\mathbf{Lease\ Authority} \quad\subseteq\quad \mathbf{Workload\ Authority} \quad\subseteq\quad \mathbf{Capability\ Authority}$$

### 8.1 Concrete Authority & Process-Control Matrix
To eliminate ambient authority assumptions, every privileged operation is explicitly mapped to its authorizing capability token, executing service, and kernel boundary:

| Operation | Requester | Authorizing Capability | Executing Service | Authoritative State Owner | Kernel Boundary / Handle Type | Failure Behavior |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Create Workload** | Client / Agent | Client Root Capability (`CLOSE \| INSPECT`) | `workloadd` | `workloadd` | None (Daemon IPC) | Reject if table full (`ZeroError::ObjectTableFull`) or invalid cap. |
| **Submit / Extend DAG** | Workload Owner | Workload Control Capability (`DUPLICATE \| TRANSFER`) | `workloadd` | `workloadd` | None (Daemon IPC) | Reject if cyclic (`ZeroError::CyclicDependency`) or immutable state. |
| **Request Lease** | `workloadd` | Attenuated Task Capability (`required_rights`) | `resourced` | `resourced` | Channel Handle passing | Reject if coverage incomplete or quota exceeded (`ZeroError::PermissionDenied`). |
| **Derive Task Capability**| `workloadd` | Parent Workload Capability | Kernel (Stage 3H) | Kernel CDT | `SYS_CAP_DERIVE` syscall | Reject if requested rights exceed parent (`ZeroError::PermissionDenied`). |
| **Create Task Process** | `workloadd` | Workload Process Execution Capability | Kernel (Stage 3F/3J) | Kernel Process Table | `load_elf` / `create_process` (`parent_pid = workloadd.pid`) | Reject if `MAX_PROCESSES` exhausted (`ZeroError::ObjectTableFull`). |
| **Observe Process Exit** | `workloadd` | Parent Process Role (`parent_pid`) | Kernel Scheduler | Kernel Zombie Queue | Process Completion Event / IPC Notification | If process faults, kernel transitions to `Zombie`, notifies `workloadd`. |
| **Terminate Process** | `workloadd` | Process Handle with `CLOSE \| REVOKE` | Kernel Process Manager | Kernel Process Table | `sys_channel_close` / handle revocation | Forcibly cancels process threads, reclaims address space to PMM. |
| **Revoke Task Capabilities**| `workloadd` | Parent Capability Handle | Kernel (Stage 3H) | Kernel CDT | `sys_channel_close` / CDT cascade | Recursive invalidation of all descendant capability nodes. |
| **Release Lease** | `workloadd` | Active `LeaseId` + Generation | `resourced` | `resourced` | `OP_LEASE_RELEASE` via IPC | Reject if generation mismatch; capacity moves to $C_{\text{unavail}}$. |

---

## 9. Workload Scheduling & Local Node Placement

### 9.1 Separation of Schedulers
ZeroOS maintains a strict distinction between kernel thread scheduling and workload orchestration:
- **Stage 3 Microkernel Scheduler (Frozen)**: Authoritative for preemptive dispatch of executable threads onto physical CPU cores via LAPIC timer slices.
- **Stage 4C Workload Scheduler**: Authoritative for:
  1. Evaluating task dependency resolution (moving tasks from `Blocked` to `Ready`).
  2. Workload priority queuing (`High`, `Normal`, `Low`, `Background`).
  3. Bounded deadline tracking ($T_{\text{deadline}} - T_{\text{current}}$).
  4. Multi-dimensional resource matching against available Stage 4B capacity.
  5. Placing tasks into execution containers (launching Stage 3 processes within bounded slots).

### 9.2 Local Node Placement Engine
Local node placement determines which local resources in the Stage 4B Resource Graph are leased for a ready task:
1. **Filter Phase (Hard Constraints)**:
   - Resource compatibility: Does the resource match `ResourceType` (CPU, RAM, GPU, Accelerator)?
   - Locality compatibility: Does the resource match the requested `LocalityDomain` (e.g., `Numa0`)?
   - Capacity feasibility: Does the resource's $C_{\text{avail}}$ vector satisfy $C_{\text{avail}} \ge \mathbf{C}_{\text{task}}$?
   - Coupling constraints: Does $(\mathbf{C}_{\text{alloc}} + \mathbf{C}_{\text{task}})$ satisfy $\mathbf{A} \cdot \mathbf{C} \le \mathbf{b}$?
2. **Rank Phase (Soft Optimization)**:
   - Rank candidates by energy efficiency (`EnergyTier::Measured` preferred over `Estimated`).
   - Thermal penalty minimization (select resources with lower `current_temp_mxc`).
   - Power budget headroom (select resources with lower `current_power_mw`).

---

## 10. Workload Lifecycle State Machine

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

### 10.1 Formal Transition Specifications

| State | Preconditions | Authority | Side Effects | Next States |
| :--- | :--- | :--- | :--- | :--- |
| **Creating** | Client initiates submission with `WorkloadDescriptor`. | Client / `workloadd` | Assigns `WorkloadId`, initializes empty task table. | `Ready`, `Failed` |
| **Ready** | Task DAG verified acyclic; capability envelope validated. | `workloadd` | Tasks with in-degree 0 transition to `Ready`. | `Running`, `Cancelled` |
| **Running** | At least one task is `Running` or `LeaseAcquired`. | `workloadd` | Leases actively renewed; processes monitored. | `Waiting`, `Suspended`, `Recovering`, `Cancelling`, `Completed`, `Failed` |
| **Waiting** | All running tasks completed; remaining tasks blocked on external events. | `workloadd` | Leases for idle tasks released to save energy. | `Running`, `Cancelling`, `Failed` |
| **Suspended** | User or supervisor pauses execution. | Client / Supervisor | Running processes suspended via kernel thread freeze; leases retained or hibernated. | `Running`, `Cancelling` |
| **Recovering** | Process crash or lease loss encountered; retry initiated. | `workloadd` | Quarantines failed process; calculates backoff; requests replacement lease. | `Running`, `Failed` |
| **Cancelling** | User request, deadline expiration, or fatal dependency failure. | Client / `workloadd` | Enters 4-phase cancellation pipeline. | `Cancelled`, `Failed` |
| **Completed** | All tasks in DAG reached `Completed` state. | `workloadd` | Terminal success; all leases released; client notified. | `Reclaimed` |
| **Failed** | Unrecoverable error, task retries exhausted, or security violation. | `workloadd` | Terminal failure; active processes killed; leases released; failure logged. | `Reclaimed` |
| **Cancelled** | Cancellation teardown completed. | `workloadd` | Terminal cancellation; all resources released. | `Reclaimed` |
| **Reclaimed** | All transient handles, processes, and leases destroyed; client consumed result. | `workloadd` | Workload slot returned to pool; `WorkloadId` permanently retired. | None (Terminal) |

---

## 11. Cancellation Pipeline & 4B Quarantine Reconciliation

### 11.1 The 4-Phase Cancellation Pipeline
To guarantee absolute resource safety and prevent physical overcommit, cancellation in Stage 4C strictly separates authority revocation from physical resource reclamation:

```text
[Phase 1: Cancellation Requested]
  - workloadd transitions Workload state to 'Cancelling'.
  - Rejects any further task submissions or lease renewal requests.
            ↓
[Phase 2: Cooperative Stop & Process Termination]
  - workloadd transmits cancellation signal via IPC to active task processes.
  - Starts bounded cooperative grace period (ΔT_cancel_timeout = 50 ms).
  - If process does not exit, workloadd invokes process termination authority via kernel handle.
  - Kernel reclaims process address space (PML4) to PMM.
            ↓
[Phase 3: Lease Surrender & Quarantine]
  - workloadd invokes OP_LEASE_RELEASE on resourced.
  - resourced marks lease state as 'Released'.
  - resourced transfers allocated capacity to C_unavail (Quarantine Ledger):
        C_alloc ← C_alloc - ΔC
        C_unavail ← C_unavail + ΔC
        C_avail remains strictly UNCHANGED.
            ↓
[Phase 4: Provider Confirmation & Physical Availability]
  - Underlying hardware driver / provider confirms device DMA/registers quiesced and reset.
  - resourced transfers quarantined capacity to C_avail:
        C_unavail ← C_unavail - ΔC
        C_avail ← C_avail + ΔC
  - workloadd transitions Workload to 'Cancelled', then 'Reclaimed'.
```

$$\mathbf{Invariant\ I-WORKLOAD-CANCELLATION-QUARANTINE:}\quad \text{Cancelling a workload revokes lease authority}$$
$$\text{immediately, but committed physical capacity MUST enter } C_{\text{unavail}} \text{ and cannot return to } C_{\text{avail}}$$
$$\text{until the hardware provider confirms physical release and quiescence.}$$

---

## 12. Failure, Recovery & Fault Matrix

ZeroOS explicitly defines system behavior for all physical and logical failure events:

| Failure Event | Workload State | Task State | Process Action | Capability Action | Lease Action | Recovery / Terminal Result |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Task Computational Error** | `Running` | `Running -> Failed` | Process exits cleanly (`sys_exit`). | Task capability handles closed. | Lease held if retrying; released if fatal. | Evaluate retry counter: if $N < N_{\text{max}}$, backoff and retry. Else fail workload. |
| **Process Crash / CPU Fault** | `Running -> Recovering` | `Running -> Failed` | Kernel terminates process (`Zombie`). | Capability handles reclaimed by kernel. | Voluntary release or retained for retry. | Spawn fresh replacement process with new PID. Invariant `I-TASK-PROCESS-ASSOCIATION` preserved. |
| **Lease Expiration (TTL Exceeded)** | `Running -> Recovering` | `Running -> Blocked` | Process thread paused or killed. | Lease capability token invalidated. | `resourced` marks lease `Expired`; capacity reclaimed. | `workloadd` requests fresh lease from `resourced`. If granted, resume/restart task. |
| **Time Authority Lost** | `Running -> Suspended` | `Running -> Blocked` | Running process paused immediately. | Temporal capabilities suspended. | `resourced` moves capacity to $C_{\text{unavail}}$ (quarantine). | Fail closed. Wait for time adapter recovery or abort workload. **Zero overcommit.** |
| **Provider Lost (HW Disconnect)** | `Running -> Recovering` | `Running -> Failed` | Process terminates if hardware bound. | Hardware capability handles revoked. | `resourced` moves physical capacity to $C_{\text{unavail}}$. | Re-evaluate placement against surviving resources; reschedule task if pure/idempotent. |
| **Client Channel Closed (PeerClosed)** | `Cancelling` | `All -> Cancelled` | Terminate all active processes. | Close all workload capabilities. | Execute 4-phase cancellation pipeline. | Clean terminal cancellation; zero resource leaks (`I-LEASE-CLEANUP`). |
| **Resource Exhaustion ($C_{\text{avail}} = 0$)** | `Ready -> Waiting` | `Ready` (Queued) | No process launched. | None. | Pending lease request queued or rejected. | Task waits in queue until leases released, or aborts if deadline expires. |
| **Dependency Failure** | `Cancelling -> Failed` | Downstream `-> Cancelled` | Downstream processes never launched. | Downstream capabilities never derived. | No downstream leases acquired. | Workload terminates in `Failed`. Partial outputs quarantined. |
| **System Power Loss / Reboot** | Terminal Crash | All Reset | All processes reset by hardware boot. | All memory handles wiped. | Reset by `init` BootEpoch increment. | On reboot, Class 2 resumes from verified checkpoint; Class 1/3 discarded. |

$$\mathbf{Invariant\ I-LEASE-INVALIDATION-NO-PHYSICAL-RELEASE:}\quad \text{When a lease is invalidated due to}$$
$$\text{TimeAuthorityLost or ProviderLost, workloadd must treat the lease as revoked, but resourced MUST NOT}$$
$$\text{return capacity to } C_{\text{avail}} \text{ until physical quiescence or reset is verified.}$$

---

## 13. Persistence, Identity & Restart Semantics

### 13.1 What Survives Failures?
- **Process Crash**: `WorkloadId`, `TaskId`, Task DAG state, accumulated outputs, and active leases survive. Ephemeral PIDs and address spaces perish.
- **`workloadd` Daemon Crash**:
  - `workloadd`'s state is preserved via the Stage 4B durable persistence authority (`MemoryPersistenceAuthority` or non-volatile journal).
  - Upon supervisor restart (`init`), `workloadd` recovers its sequence ceiling $S_{\text{ceil}}$, reconstructs active workloads from persistent descriptors, and inspects surviving processes.
- **Node Reboot**:
  - All Stage 3 memory and processes are lost.
  - BootEpoch increments monotonically (`boot_epoch::advance_boot_epoch`).
  - Class 1 workloads perish completely.
  - Class 2 workloads read their latest durable checkpoint from ZeroFS and re-submit a recovery Workload DAG.

### 13.2 Durable ID Allocation Crash Ordering
In compliance with `I-ID-DURABLE-ALLOCATOR-STATE`:
$$\text{Persist Sequence Ceiling } S_{\text{ceil}} \quad\longrightarrow\quad \text{Issue WorkloadId / TaskId} \quad\longrightarrow\quad \text{Publish Externally}$$
Upon crash recovery, sequence allocation resumes strictly at $S_{\text{resume}} > S_{\text{ceil}}$, preventing any possibility of duplicate `WorkloadId` issuance across reboots.

---

## 14. Architecturally Derived Static Bounds

In strict adherence to ZeroOS microkernel design principles, all bounds are derived from underlying Stage 3 and Stage 4B architectural limits:

| Parameter | Bound | Derivation & Architectural Grounding | Failure Behavior |
| :--- | :--- | :--- | :--- |
| **`MAX_CONCURRENT_RUNNING_TASKS`** | **8** | **Derived from Stage 3 `MAX_PROCESSES = 16`**: 16 total process slots minus 1 (Master Kernel PID 0), minus 4 system daemons (`init`, `brokerd`, `resourced`, `workloadd`), minus 3 reserved system headroom (future `workspaced`, `agentd`, `fabricd`) leaves exactly **8 available slots** for concurrent task processes. | Tasks wait in `Ready` queue until a running task completes and frees a process slot. |
| **`MAX_CONCURRENT_WORKLOADS`** | **8** | **Derived from Static BSS Footprint**: `workloadd` maintains fixed control blocks. At $\approx 512\text{ bytes}$ per Workload Control Block, 8 workloads occupy exactly 4096 bytes (1 memory frame) without dynamic heap allocation. | `OP_WORKLOAD_CREATE` rejected with `ZeroError::ObjectTableFull`. |
| **`MAX_TASKS_PER_WORKLOAD`** | **16** | **Derived from Dependency Bitmask Width**: 16-bit bitmask (`u16`) enables zero-allocation dependency matrices and $O(V+E)$ topological sort cycle detection in $< 128\text{ bytes}$ stack memory. | DAG submission rejected with `ZeroError::UnsupportedResourceShape`. |
| **`MAX_DEPENDENCIES_PER_TASK`** | **15** | **Derived from Bitmask Range**: A task can depend on any subset of other tasks in the 16-task graph ($16 - 1 = 15$). | Excess dependencies rejected at validation. |
| **`MAX_LEASES_PER_WORKLOAD`** | **8** | **Derived from Running Task Limit**: Since a workload can run at most 8 concurrent tasks, it requires at most 8 active capacity leases simultaneously. Fits comfortably within Stage 4B `MAX_LEASES_PER_NODE = 256`. | Lease request queued or rejected if quota exceeded. |
| **`DEFAULT_MAX_TASK_RETRIES`** | **3** | **Bounded Crash Loop Prevention**: Maximum 3 retries prevents thrashing the Stage 3 kernel zombie process queue and starving system scheduling. Configurable up to ceiling of 5. | Task transitions to `Failed`; propagates to workload. |
| **`MAX_HANDLES_PER_TASK_PROCESS`** | **8 / 32** | **Derived from Stage 3 `MAX_HANDLES = 32`**: Task process requires $\le 8$ handles (1 control channel, 1 input, 1 output, 1..5 resource tokens), leaving 24 handles of safety headroom. | Handle allocation fails with `ZeroError::ObjectTableFull`. |

---

## 15. Security & Authority Audit

### 15.1 Adversarial Vulnerability Audit
1. **Capability Amplification**:
   - *Risk*: A task attempts to acquire capabilities or access resources beyond its owner's envelope.
   - *Mitigation*: Strictly enforced monotonic attenuation via `SYS_CAP_DERIVE`. The kernel enforces $\text{ChildRights} \subseteq \text{ParentRights}$. `resourced` rejects lease requests lacking valid capability coverage (`I-LEASE-AUTH-BOUNDED`).
2. **Workload-to-Process Authority Escalation**:
   - *Risk*: A rogue task process attempts to manipulate other tasks or workloads.
   - *Mitigation*: Tasks run in isolated Stage 3 address spaces with private handle tables. A process is given only the specific IPC handles required for its task inputs and outputs.
3. **Accounting & Lease Bypass**:
   - *Risk*: `workloadd` or a process attempts to execute computation without acquiring a Stage 4B lease.
   - *Mitigation*: Kernel physical resources (DMA, GPU apertures, high-performance shm) are gated by capability tokens issued exclusively by `resourced` upon lease grant.
4. **Stale Identifier & Replay Attacks**:
   - *Risk*: An attacker sends IPC messages using a recycled `WorkloadId` or stale `LeaseId`.
   - *Mitigation*: Monotonic `local_seq` allocation guarantees zero identifier reuse. Every state transition increments `generation`. Stale messages are rejected immediately.
5. **Duplicate Task Execution**:
   - *Risk*: A race condition causes a task to be launched simultaneously in two separate processes.
   - *Mitigation*: State transitions in `workloadd` are serialized. A task can only transition from `Ready` to `Running` once.

---

## 16. Authoritative Invariants Catalog

The Workload Orchestration Subsystem is governed by 17 non-negotiable architectural invariants:

1. **`I-WORKLOAD-PROCESS-SEPARATION`**: A Process is an isolated kernel execution primitive; a Workload is a Stage 4 unit of intended computation. A Workload must never be implemented as a simple process wrapper.
2. **`I-WORKLOAD-ID-NONREUSE`**: Every `WorkloadId` is issued from a durable monotonic sequence and is permanently retired upon terminal completion or failure; identifiers are never reused.
3. **`I-TASK-DAG-ACYCLIC`**: The Task Graph of a Workload is strictly acyclic; directed cycles represent deadlocks and are rejected at admission.
4. **`I-TASK-DAG-DEPENDENCY-CORRECTNESS`**: A task cannot transition to `Ready` or `Running` until all ancestor tasks in the DAG have successfully transitioned to `Completed`.
5. **`I-TASK-DAG-IMMUTABLE-EXISTING`**: Once admitted, an existing task's dependencies, resource demands, and code targets are permanently immutable.
6. **`I-TASK-DAG-DOWNSTREAM-EXTENSION`**: Dynamic DAG extensions may only append new downstream tasks that depend on existing tasks; new tasks cannot become prerequisites for already-admitted tasks.
7. **`I-WORKLOAD-CAPABILITY-SUBSET`**: All capabilities derived for tasks within a Workload must be strictly attenuated subsets of the parent Workload Capability Security Envelope.
8. **`I-WORKLOAD-LEASE-SUBSET`**: Leases requested for a task must not exceed the authority granted by the task's authorizing capability token.
9. **`I-WORKLOAD-NO-ACCOUNTING-BYPASS`**: `workloadd` must never allocate, bind, or utilize physical hardware resources without an active Stage 4B `ResourceLease` issued by `resourced`.
10. **`I-WORKLOAD-RESOURCE-REQUIREMENT-CONSISTENCY`**: The resource capacity requested for a task must match the task's declared `DimensionCapacityVector` and satisfy `resourced` coupling constraints.
11. **`I-WORKLOAD-LIFECYCLE-CONSISTENCY`**: Workload state transitions must strictly adhere to the formal state machine; terminal states (`Completed`, `Failed`, `Cancelled`, `Reclaimed`) are irreversible.
12. **`I-TASK-LIFECYCLE-CONSISTENCY`**: Task state transitions must strictly follow the canonical task lifecycle; a failed task cannot transition directly to completed without re-execution.
13. **`I-TASK-PROCESS-ASSOCIATION`**: A Stage 3 Process may execute at most one Task of one Workload at any time; processes are never shared across workloads.
14. **`I-WORKLOAD-FAILURE-NO-RESURRECTION`**: A Workload in `Failed` or `Cancelled` cannot be resurrected; retries occur via fresh process instantiation under the existing Workload or via a newly submitted Workload.
15. **`I-WORKLOAD-RECOVERY-CLASS-DEPENDENT`**: Recovery is strictly conditioned on recovery class. Class 1 permits replay; Class 2 resumes from verified checkpoint; Class 3 prohibits transparent replay and transitions to `FailedAtMilestone` fail-closed.
16. **`I-WORKLOAD-CANCELLATION-QUARANTINE`**: Cancelling a workload revokes lease authority immediately, but committed capacity enters $C_{\text{unavail}}$ and cannot return to $C_{\text{avail}}$ until the provider confirms physical release and quiescence.
17. **`I-LEASE-INVALIDATION-NO-PHYSICAL-RELEASE`**: Invalidation of a lease due to `TimeAuthorityLost` or `ProviderLost` moves capacity to $C_{\text{unavail}}$ until physical quiescence or reset is verified.

---

## 17. Verification Architecture (Future Machine-Level Suite)

When implementation is authorized, verification will occur via a comprehensive 28-test integration suite executed in real Ring 3 within QEMU:

```text
SUITE 4C-VERIFY (28 Machine-Level Integration Tests):
 1. test_workload_create_monotonic_id          - Verifies WorkloadId uniqueness and non-reuse.
 2. test_workload_generation_increment         - Verifies generation increment on mutation/restart.
 3. test_task_dag_admit_valid_pipeline         - Verifies linear pipeline DAG admission.
 4. test_task_dag_reject_direct_cycle          - Verifies immediate rejection of A -> B -> A cycle.
 5. test_task_dag_reject_complex_cycle         - Verifies rejection of multi-node indirect cycles.
 6. test_task_dag_dependency_ordering          - Verifies tasks execute strictly in dependency order.
 7. test_resource_demand_formulation           - Verifies DimensionCapacityVector construction.
 8. test_lease_acquisition_via_resourced       - Verifies OP_LEASE_REQUEST IPC to resourced.
 9. test_lease_rejection_insufficient_capacity - Verifies graceful queue/fail when C_avail exhausted.
10. test_capability_attenuation_enforced       - Verifies task capabilities are strictly attenuated.
11. test_lease_authority_bounded_by_cap        - Verifies lease request rejected if cap rights insufficient.
12. test_task_to_process_launch                - Verifies execution of task in dedicated Stage 3 process.
13. test_multi_process_workload_execution      - Verifies multiple processes managed by single workload.
14. test_task_process_isolation                - Verifies no cross-workload process reuse.
15. test_task_clean_completion                 - Verifies normal exit code propagation and task completion.
16. test_task_crash_retry_success              - Verifies process crash triggers retry with new process.
17. test_task_retry_exhaustion_failure         - Verifies workload failure after max retries exceeded.
18. test_workload_cancellation_quarantine     - Verifies cancellation halts processes, capacity quarantined in C_unavail.
19. test_lease_expiration_handling             - Verifies expired lease halts task and triggers renewal.
20. test_time_authority_loss_quarantine        - Verifies TimeAuthorityLost halts task, C_unavail preserved.
21. test_provider_loss_handling                - Verifies ProviderLost triggers task failure/rescheduling.
22. test_peer_closed_client_cleanup            - Verifies client disconnect releases all leases & processes.
23. test_class2_checkpoint_and_resume          - Verifies Class 2 stateful resume from checkpoint.
24. test_class3_irreversible_fail_at_milestone - Verifies Class 3 transitions to FailedAtMilestone without replay.
25. test_pmm_leak_neutrality                   - Verifies zero physical frame leaks after 100 workloads.
26. test_stage3_boundary_preservation          - Verifies 0 kernel modifications and intact syscalls.
27. test_stage4a_service_preservation          - Verifies init and brokerd remain intact and responsive.
28. test_stage4b_accounting_preservation       - Verifies resourced conservation invariant holds.
```

---

## 18. Strict Non-Goals

The following are strictly out of scope for Phase 4C:
- **No Implementation Code**: No daemons, libraries, or test runners implemented in this phase.
- **No Kernel Changes**: 0 bytes modified in Stage 3 microkernel.
- **No Stage 4A/4B Modifications**: No modifications to `init`, `brokerd`, or `resourced`.
- **No Workspace UI or Context Storage**: Workspaces belong exclusively to Phase 4D.
- **No Autonomous Agent Reasoning**: LLM loops and perception belongs to Phase 4E.
- **No Distributed Fabric Networking**: Remote P2P node mesh belongs to Phase 4F.
- **No Dynamic Kernel Allocations**: All structures remain statically bounded.

---

## 19. Architectural Document Metadata

- **Author**: Antigravity AI (Pair Programming with User)
- **Document Version**: Revision 2 (Adversarial Corrections)
- **Date**: 2026-09-23
- **Classification**: ZeroOS Architecture Discovery Specification
