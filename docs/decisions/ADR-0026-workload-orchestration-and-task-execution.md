# ADR-0026: Workload Orchestration & Task Execution Subsystem (`workloadd`)

## Status
🟡 **DRAFT — REQUIRES REVIEW (REVISION 4)** (2026-10-04)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024, Stage 4B Architecture Rev12 (`STAGE4B-ARCHITECTURE-REV12.md`), ADR-0025 Rev12, Stage 4C Architecture Specification Rev4 (`STAGE4C-ARCHITECTURE-REV4.md`)  
**Target Subsystem:** Stage 4C Workload Orchestration Subsystem (`workloadd`)  
**Kernel Baseline:** Stage 3A–3N Inviolate (0 Bytes Modified, Frozen Ring 0 Microkernel)  
**Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Frozen, Stage 4B (`resourced`) Frozen  

---

## Context

Stages 3A through 3N established an authoritative capability microkernel in 64-bit Long Mode across up to 4 SMP cores. Stage 4A implemented the freestanding user-space runtime (`libzero`), service supervisor (`init`), and capability-mediated service discovery directory (`brokerd`). Stage 4B finalized and verified local node heterogeneous resource modeling, multi-dimensional vector conservation, and monotonic temporal capacity leases (`resourced`).

Following architectural review of Rev3, **Revision 4** definitively resolves process creation authority, lease bound derivation, and process capacity policy framing:
1. **Process Creation & Supervision Boundary (Substrate Audit)**: Inspection of frozen Stage 3 (`kernel/src/syscall/dispatch.rs`, `kernel/src/task/process.rs`, `kernel/src/elf/load.rs`) and Stage 4A (`init/src/main.rs`, `libzero/src/supervisor.rs`) confirms that Stage 3 provides **no `SYS_PROCESS_CREATE` syscall** to Ring 3 daemons. Process creation, address space allocation (`CR3`), ELF loading (`load_elf`), initial thread allocation, and PID assignment are kernel loader/supervisor operations. `workloadd` depends on the Stage 4 System Supervisor Process Spawner boundary (`init` supervisor IPC or kernel process control boundary) to instantiate task processes, passing derived task capability handles ($\text{ChildRights} \subseteq \text{ParentRights}$) during process setup.
2. **Multi-Dimensional Lease Bound Derivation**: In Stage 4B (`resourced`), each `ResourceLease` binds to **exactly one `resource_id`** (`ResourceDescriptor`). Single multi-dimensional descriptors grant CPU, RAM, GPU, and DMA capacity under 1 lease ($1 \text{ task} \rightarrow 1 \text{ lease}$). If a task spans $M_{\text{desc}}$ distinct descriptors (e.g. Host CPU/RAM + PCIe Accelerator), it acquires $M_{\text{desc}}$ leases. `MAX_LEASES_PER_WORKLOAD` is explicitly classified as an **Implementation Configuration Bound** (set to 16 by default: $8 \text{ tasks} \times \le 2 \text{ descriptors/task} = 16$), fitting within Stage 4B's `MAX_LEASES_PER_NODE = 256`.
3. **Stage 4 Process Capacity Allocation Policy**: Clarifies that `MAX_PROCESSES = 16` is the hard Ring 0 kernel process table ceiling. `MAX_CONCURRENT_RUNNING_TASKS = 8` is a **Stage 4 Configuration Policy** ($16 \text{ MAX\_PROCESSES} - 5 \text{ daemons} - 3 \text{ reserved slots} = 8$), holding 3 process slots in reserve for system service capacity allocation policy to prevent daemon startup starvation (`ZeroError::ObjectTableFull`).

---

## Architectural Decisions

### ADR-0026-0: Inviolate Separation of Process and Workload
$$\mathbf{Process\ (Stage\ 3)} \quad\neq\quad \mathbf{Workload\ (Stage\ 4)}$$
1. **Stage 3 Process**: An isolated kernel execution and memory container. Owns a page table root (`CR3`), thread group, and handle table (up to 32 slots). Bounded by `MAX_PROCESSES = 16`. Process termination permanently destroys the container. Processes are strictly local and cannot migrate (`I-NO-LIVE-PROCESS-MIGRATION`).
2. **Stage 4 Workload**: A goal-directed computational request. Owns an acyclic Task DAG, aggregated resource demand envelope, capability envelope, temporal leases, and failure/recovery policies.
3. **Execution Rules**:
   - One Workload may contain multiple Processes (executing distinct tasks in the DAG).
   - One Process executes at most one Task of one Workload at any time (`I-TASK-PROCESS-ASSOCIATION`).
   - Process crash $\implies$ Task failure. The Workload survives, inspects recovery policy, and requests a **fresh** Stage 3 process without altering `WorkloadId` or `TaskId`.

---

### ADR-0026-1: Task Directed Acyclic Graph (DAG) vs. Stage 4B Resource Graph
$$\mathbf{Resource\ Graph\ (Stage\ 4B)} \quad\neq\quad \mathbf{Task\ DAG\ (Stage\ 4C)}$$
1. **Resource Graph (4B)**: Models physical hardware topology and capacity relationships. Heterogeneous, typed, and directed. Directed structural cycles are explicitly permitted (e.g., `Node -> Device -> Storage -> Swap -> RAM -> Node`).
2. **Task DAG (4C)**: Models computational dependencies and execution precedence. Must be **strictly acyclic** (`I-TASK-DAG-ACYCLIC`). Cycles represent unresolvable execution deadlocks and are rejected at admission via topological sorting.
3. **Bounds**: Statically bounded to `MAX_TASKS_PER_WORKLOAD = 16`. Dependencies are represented via compact 16-bit bitmasks (`u16`).
4. **Dynamic Mutation Authority**:
   - Only the authenticated Workload owner may submit DAG extensions while in `Ready` or `Running`.
   - Admitted tasks are permanently immutable (`I-TASK-DAG-IMMUTABLE-EXISTING`).
   - New tasks may only declare dependencies on existing tasks (`I-TASK-DAG-DOWNSTREAM-EXTENSION`).
   - Workloads in `Completed` state cannot be mutated (`I-COMPLETED-WORKLOAD-IMMUTABLE`).

---

### ADR-0026-2: Stage 4B Resource Lease Integration & Accounting Non-Bypass
1. **Lease Consumption**: `workloadd` translates Task resource requirements into `DimensionCapacityVector` requests and submits them to `resourced` via Stage 3G IPC (`OP_LEASE_REQUEST`).
2. **Authority Decoupling**:
   $$\mathbf{Lease\ Authority} \quad\subseteq\quad \mathbf{Workload\ Authority} \quad\subseteq\quad \mathbf{Capability\ Authority}$$
   `resourced` verifies that the caller's capability grants coverage over the target resource's `required_rights`.
3. **Strict Non-Bypass (`I-WORKLOAD-NO-ACCOUNTING-BYPASS`)**: `workloadd` never allocates, binds, or utilizes physical hardware resources without presenting a valid, active `ResourceLease` issued by `resourced`.
4. **Quarantine Invariant Preservation (`I-LEASE-INVALIDATION-NO-PHYSICAL-RELEASE`, `I-WORKLOAD-CANCELLATION-QUARANTINE`)**: When a lease authority is invalidated or surrendered during cancellation, `resourced` transfers allocated capacity to $C_{\text{unavail}}$ (Quarantine Ledger). $C_{\text{avail}}$ remains unchanged until the hardware provider confirms physical release or quiescence.

---

### ADR-0026-3: Capability Attenuation & Security Envelope
1. **Workload Security Envelope**: A client presents a Stage 3 capability handle upon workload submission. The rights embedded in this token establish the upper bound of authority for the entire Workload (`I-WORKLOAD-CAPABILITY-SUBSET`).
2. **Task Capability Derivation**: For each task process, `workloadd` invokes the kernel capability derivation syscall `SYS_CAP_DERIVE` to produce an attenuated child capability token ($\text{ChildRights} \subseteq \text{ParentRights}$).
3. **Revocation Propagation**: When a workload terminates or is cancelled, all derived task capabilities are closed via `sys_channel_close`, invalidating process authority fail-closed across the kernel Capability Derivation Tree (`CDT`).

---

### ADR-0026-4: Workload Lifecycle, Recovery Classes & Cancellation Pipeline
1. **Canonical Lifecycle State Machine**:
   $$\text{Creating} \longrightarrow \text{Ready} \longrightarrow \text{Running} \longrightarrow \begin{cases} \text{Waiting} \\ \text{Suspended} \\ \text{Recovering} \\ \text{Cancelling} \end{cases} \longrightarrow \begin{cases} \text{Completed} \\ \text{Failed} \\ \text{Cancelled} \end{cases} \longrightarrow \text{Reclaimed}$$
2. **Three-Tier Recovery Classes**:
   - **Class 1 (Ephemeral Pure Compute, `I-RECOVERY-CLASS-1-IDEMPOTENT-REPLAY`)**: Stateless compute. Safe re-execution on failure from scratch; zero persistent state.
   - **Class 2 (Checkpointed Stateful, `I-RECOVERY-CLASS-2-CHECKPOINTED-RESUME`)**: Resumes strictly from latest verified ZeroFS checkpoint $G_c$ upon process or daemon crash.
   - **Class 3 (Irreversible External, `I-RECOVERY-CLASS-3-NO-TRANSPARENT-REPLAY`)**: External actuation or network mutations. Transparent replay is prohibited; transitions immediately to `FailedAtMilestone(M)`.
3. **4-Phase Cancellation Pipeline**:
   $$\text{Cancellation Requested} \longrightarrow \text{Process Terminated} \longrightarrow \text{Lease Surrendered (Quarantined in } C_{\text{unavail}}\text{)} \longrightarrow \text{Provider Confirms Physical Release (} C_{\text{avail}}\text{)}$$

---

### ADR-0026-5: Static Bounds & Capacity Allocation Policy
- `MAX_CONCURRENT_RUNNING_TASKS = 8`: **Stage 4 Configuration Policy**. Derived from kernel limit `MAX_PROCESSES = 16` minus 5 active daemons (PID 0, `init`, `brokerd`, `resourced`, `workloadd`) minus 3 reserved system service process slots. Enforced node-wide across all active workloads.
- `MAX_CONCURRENT_WORKLOADS = 8`: **Implementation Configuration**. Derived from static BSS footprint in `workloadd` (1 frame / 4096 bytes).
- `MAX_TASKS_PER_WORKLOAD = 16`: **Hard Architectural Limit**. Derived from 16-bit bitmask dependency representation (`u16`) and stack topological sort memory limits.
- `MAX_DEPENDENCIES_PER_TASK = 15`: **Hard Architectural Limit**. Max in-degree in 16-task DAG.
- `MAX_LEASES_PER_WORKLOAD = 16 (default)`: **Implementation Configuration**. Derived from $1 \text{ lease per resource descriptor} \times 8 \text{ tasks} \times \le 2 \text{ descriptors/task} = 16 \text{ leases}$.
- `DEFAULT_MAX_TASK_RETRIES = 3`: **Bounded Orchestration Policy**. Bounded to prevent thrashing the Stage 3 kernel zombie queue (`ZOMBIE_PROCESS_QUEUE`). Failure triggers Workload Recovery Class evaluation.

---

### ADR-0026-6: Process Lifecycle & Authority Matrix

| Operation | Requester | Required Authority | Executor | Authoritative State Owner |
| :--- | :--- | :--- | :--- | :--- |
| **Create Workload** | Client / Agent | Client Root Capability (`CLOSE \| INSPECT`) | `workloadd` | `workloadd` |
| **Create Task** | Workload Owner | Workload Control Capability (`DUPLICATE \| TRANSFER`) | `workloadd` | `workloadd` |
| **Request Lease** | `workloadd` | Attenuated Task Capability (`required_rights`) | `resourced` | `resourced` |
| **Create Process** | `workloadd` | Supervision Request to `init` / Kernel Loader | Supervisor / Kernel | Process Table |
| **Observe Process Exit** | `workloadd` | Parent Process Role (`parent_pid`) | Kernel / Supervisor | Zombie Queue |
| **Terminate Process** | `workloadd` | Channel Close / Supervisor IPC Request | Supervisor / Kernel | Process Table |
| **Revoke Task Capabilities**| `workloadd` | Parent CDT Capability Handle | Kernel Boundary | Capability CDT |
| **Release / Invalidate Lease** | `workloadd` | Active `LeaseId` + Generation | `resourced` | Lease Table (`resourced`) |

---

## Consequences

### Positive
- Resolves both remaining blocker findings from Rev3 review.
- Proves exact process creation path from Stage 3 and Stage 4A supervisor contracts without inventing a Ring 0 syscall.
- Formally maps 4B multi-dimensional resource descriptors to task lease requirements.
- Frames system process reservation strictly as Stage 4 capacity allocation policy.
- Strict microkernel compliance (0 bytes kernel modified).

### Negative / Trade-offs
- Task concurrency on a single node is bounded to 8 simultaneous Stage 3 processes across all workloads.
- Additional tasks in `Ready` queue must wait for running processes to complete and yield process slots.

---

## Invariant Catalog

1. `I-WORKLOAD-PROCESS-SEPARATION`
2. `I-WORKLOAD-ID-NONREUSE`
3. `I-TASK-DAG-ACYCLIC`
4. `I-TASK-DAG-DEPENDENCY-CORRECTNESS`
5. `I-TASK-DAG-IMMUTABLE-EXISTING`
6. `I-TASK-DAG-DOWNSTREAM-EXTENSION`
7. `I-COMPLETED-WORKLOAD-IMMUTABLE`
8. `I-WORKLOAD-CAPABILITY-SUBSET`
9. `I-WORKLOAD-LEASE-SUBSET`
10. `I-WORKLOAD-NO-ACCOUNTING-BYPASS`
11. `I-WORKLOAD-RESOURCE-REQUIREMENT-CONSISTENCY`
12. `I-WORKLOAD-LIFECYCLE-CONSISTENCY`
13. `I-TASK-LIFECYCLE-CONSISTENCY`
14. `I-TASK-PROCESS-ASSOCIATION`
15. `I-WORKLOAD-FAILURE-NO-RESURRECTION`
16. `I-RECOVERY-CLASS-1-IDEMPOTENT-REPLAY`
17. `I-RECOVERY-CLASS-2-CHECKPOINTED-RESUME`
18. `I-RECOVERY-CLASS-3-NO-TRANSPARENT-REPLAY`
19. `I-WORKLOAD-CANCELLATION-QUARANTINE`
20. `I-LEASE-INVALIDATION-NO-PHYSICAL-RELEASE`
