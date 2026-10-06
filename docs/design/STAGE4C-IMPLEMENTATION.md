# ZeroOS Phase 4C Implementation Plan & Verification Specification (Rev2)

**Subsystem**: Workload Orchestration & Task Execution (`workloadd`)  
**Parent Specifications**: Stage 4C Architecture Specification Rev4 (`docs/design/STAGE4C-ARCHITECTURE-REV4.md`) & ADR-0026 Rev4 (`docs/decisions/ADR-0026-workload-orchestration-and-task-execution.md`)  
**Status**: 📋 DRAFT IMPLEMENTATION PLAN REV2 (Pending User Approval — Zero Implementation Code Written)  
**Kernel & System Baseline**: Stage 3A–3N, Stage 4A, Stage 4B Inviolate (0 bytes modified in `kernel/`, `boot/`, `brokerd/`, `resourced/`)  
**Prerequisite Extension**: Stage 4A `init` Supervisor Process Spawner IPC Extension (`OP_PROCESS_SPAWN`, Ring 3 only, 0 bytes kernel modified)  
**Verification Boundary**: Machine-level Ring 3 user-space execution suite in QEMU driven by `tests/test_stage4c.py`.

---

## 1. Executive Summary & Core Architectural Invariants

Phase 4C implements the **Workload Orchestration Subsystem** (`workloadd`) for ZeroOS. `workloadd` translates high-level computational intent into verified, acyclic Task Directed Acyclic Graphs (DAGs), evaluates multi-dimensional resource requirements against Stage 4B resource descriptors, acquires capability-bounded temporal capacity leases from `resourced`, coordinates the execution lifecycle of Stage 3 processes via the Stage 4A supervisor spawner boundary, and enforces deterministic failure, recovery class, 4-phase cancellation, and quarantine reclamation policies.

### Core Architectural Invariants Carried Forward

1. **`I-WORKLOAD-PROCESS-SEPARATION`**: A Process is a Stage 3 execution container; a Workload is a Stage 4 unit of intended computation. A Workload must never be implemented as a simple process wrapper.
2. **`I-WORKLOAD-ID-NONREUSE`**: Every `WorkloadId` is issued from the unified Stage 4 `DistributedIdAllocator` backed by durable persistence authority and is permanently retired upon terminal completion or failure; identifiers are never reused.
3. **`I-TASK-DAG-ACYCLIC`**: The Task Graph of a Workload is strictly acyclic; directed cycles represent deadlocks and are rejected at admission via Kahn's Topological Sort Algorithm.
4. **`I-TASK-DAG-DEPENDENCY-CORRECTNESS`**: A task cannot transition to `Ready` or `Running` until all ancestor tasks in the DAG have successfully transitioned to `Completed`.
5. **`I-TASK-DAG-IMMUTABLE-EXISTING`**: Once admitted, an existing task's dependencies, resource demands, and code targets are permanently immutable.
6. **`I-TASK-DAG-DOWNSTREAM-EXTENSION`**: Dynamic DAG extensions may only append new downstream tasks that depend on existing tasks; new tasks cannot become prerequisites for already-admitted tasks.
7. **`I-COMPLETED-WORKLOAD-IMMUTABLE`**: Workloads in `Completed` state cannot be extended or mutated; new computation requires a new Workload submission.
8. **`I-WORKLOAD-CAPABILITY-SUBSET`**: All capabilities derived for tasks within a Workload must be strictly attenuated subsets of the parent Workload Capability Envelope (`SYS_CAP_DERIVE`).
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

## 2. Subsystem Architecture & File Layout

Phase 4C introduces `workloadd` as a freestanding `#![no_std]` Ring 3 system service, supported by `libzero` protocol extensions, Stage 4A supervisor spawner extensions, and a comprehensive 35-test QEMU integration suite:

```text
project-zero/
├── init/
│   └── src/
│       ├── supervisor.rs     <-- Stage 4A Supervisor extension for process spawning IPC
│       └── main.rs           <-- Register supervisor service endpoint ("supervisor.service")
├── libzero/
│   └── src/
│       ├── workload.rs       <-- Workload data structures, Task DAG, IPC OpCodes, framing
│       └── lib.rs            <-- Export workload module
├── workloadd/                <-- Workload Orchestration Daemon Package
│   ├── Cargo.toml
│   └── src/
│       └── main.rs           <-- Daemon entry point, state machine, placement, 4-phase cancellation
├── tests/
│   └── test_stage4c.py       <-- Authoritative 35-test machine-level integration suite
└── docs/
    └── design/
        ├── STAGE4C-ARCHITECTURE-REV4.md  <-- Frozen Architecture Specification
        └── STAGE4C-IMPLEMENTATION.md    <-- Authoritative Implementation Plan (This File)
```

---

## 3. Data Structure Specifications & Protocol OpCodes

All Stage 4C structures are `#![no_std]`, memory-aligned, and strictly bounded to prevent dynamic heap allocations.

### 3.1 Unified Identity Authority Mapping
- **`WorkloadId`**: Globally unique 128-bit `DistributedId` (`node_id: u64`, `local_seq: u64`) issued exclusively by `libzero::identity::DistributedIdAllocator` backed by Stage 4B durable persistence authority (`MemoryPersistenceAuthority` Slot 1). Monotonically non-reusable.
- **`TaskId`**: Bounded 16-bit numeric index (`0..15`) internal to a specific Workload. Not a separate `DistributedId`. Preserved across task retries.
- **`ProcessId` (PID)**: Ephemeral 64-bit kernel process identifier allocated exclusively by Stage 3 kernel (`PROCESS_TABLE`). Replaced with a new PID on task retry.
- **`LeaseId`**: Contractual 128-bit `DistributedId` issued exclusively by `resourced`.

### 3.2 Protocol OpCodes
```rust
// workloadd Service OpCodes (registered with brokerd as "workload.service")
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

// init Supervisor Process Spawner OpCodes (registered with brokerd as "supervisor.service")
pub const OP_PROCESS_SPAWN:           u32 = 0x0000_4A05;
pub const OP_PROCESS_SPAWN_RESP:      u32 = 0x0000_4A06;
pub const OP_PROCESS_TERMINATE:       u32 = 0x0000_4A07;
pub const OP_PROCESS_TERMINATE_RESP:  u32 = 0x0000_4A08;
pub const OP_PROCESS_EXIT_NOTIFY:     u32 = 0x0000_4A09;
```

---

## 4. Supervisor Process Spawner Boundary Specification

To fulfill Stage 4C process launching without adding Ring 0 system calls, Phase 4C.1 extends Stage 4A `init` with a Ring 3 Supervisor Process Spawner IPC endpoint (`"supervisor.service"`):

```text
+-----------------------------------------------------------------------------+
| Stage 4C Workload Daemon (workloadd)                                         |
|   1. Invokes SYS_CAP_DERIVE to derive attenuated Task Capability Handle      |
|   2. Formulates ProcessSpawnRequest (Binary Name, WorkloadId, TaskId, Cap)  |
|   3. Sends OP_PROCESS_SPAWN IPC message to init ("supervisor.service")      |
+------------------------------------+----------------------------------------+
                                     | Stage 3G IPC Channel
+------------------------------------v----------------------------------------+
| Stage 4A init Supervisor Daemon (PID 1)                                     |
|   1. Receives ProcessSpawnRequest; validates caller authority               |
|   2. Invokes kernel loader procedure (load_elf)                             |
|   3. Allocates Process Slot in PROCESS_TABLE                                |
|   4. Instantiates isolated AddressSpace (PML4 root frame)                   |
|   5. Transfers derived Capability Handle into PROCESS_HANDLE_TABLES[pslot]  |
|   6. Allocates initial thread; sets parent_pid = workloadd.pid               |
|   7. Enqueues thread into SCHEDULER                                         |
|   8. Returns ProcessSpawnResponse (assigned ProcessId & Control Handle)    |
|   9. Upon process exit: sends OP_PROCESS_EXIT_NOTIFY IPC to workloadd       |
+-----------------------------------------------------------------------------+
```

### 4.1 Process Spawn Request & Response Structures
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ProcessSpawnRequest {
    pub workload_id: DistributedId,               // 16 bytes: Parent Workload ID
    pub task_id: u16,                              // 2 bytes: Target Task ID (0..15)
    pub binary_name: [u8; 32],                     // 32 bytes: Executable target identifier
    pub task_cap_handle: u32,                      // 4 bytes: Attenuated capability handle
    pub _padding: [u8; 10],                        // 10 bytes: Padding to 64 bytes
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ProcessSpawnResponse {
    pub status: i32,                               // 4 bytes: ZeroError as i32 (0 = Success)
    pub process_id: u64,                           // 8 bytes: Assigned Stage 3 ProcessId (PID)
    pub control_channel_handle: u32,               // 4 bytes: Process control channel handle
    pub _padding: [u8; 16],                        // 16 bytes: Padding to 32 bytes
}
```

- **Maximum Outstanding Spawn Requests**: Bounded to $\le 8$ simultaneous active task processes node-wide matching `MAX_CONCURRENT_RUNNING_TASKS`. Requests exceeding 8 are rejected fail-closed with `ZeroError::ObjectTableFull`.

---

## 5. Detailed Implementation Phases

Implementation will proceed in 9 strict sequential phases. Zero kernel bytes will be modified.

```text
Phase 4C.1: Supervisor Spawner Extension (init IPC OP_PROCESS_SPAWN) & libzero Protocol
   ↓
Phase 4C.2: workloadd Core State Machine & Unified Persistence Identity Integration
   ↓
Phase 4C.3: Task DAG Admittance Engine & Topological Sort
   ↓
Phase 4C.4: Full Stage 4B Resource Lease Lifecycle Integration (Acquisition to Quarantine)
   ↓
Phase 4C.5: Complete Capability Lifecycle Engine (Derivation, Isolation, Teardown)
   ↓
Phase 4C.6: Process Supervision Boundary Integration & Asynchronous Exit Handler
   ↓
Phase 4C.7: 4-Phase Cancellation & Quarantine Teardown Pipeline
   ↓
Phase 4C.8: Three-Tier Recovery Class Engine (Class 1 / 2 / 3 Router)
   ↓
Phase 4C.9: QEMU Machine-Level Verification Suite (tests/test_stage4c.py - 35 Tests)
```

### Phase 4C.1: Supervisor Spawner Extension & `libzero` Protocol
- Extend `init` (`init/src/supervisor.rs`, `init/src/main.rs`) to register service endpoint `"supervisor.service"` handling `OP_PROCESS_SPAWN` and `OP_PROCESS_TERMINATE`.
- Implement `libzero/src/workload.rs`: define `WorkloadDescriptor`, `TaskDescriptor`, `TaskResourceDemand`, `WorkloadState`, `TaskState`, `RecoveryClass`, OpCodes, and request/response serialization.

### Phase 4C.2: `workloadd` Core State Machine & Identity Integration
- Implement `workloadd/src/main.rs`.
- Initialize `DistributedIdAllocator` using `libzero::identity::DistributedIdAllocator` backed by Stage 4B `MemoryPersistenceAuthority` Slot 1.
- Allocate static BSS workload control block array (`MAX_CONCURRENT_WORKLOADS = 8`, 4096 bytes BSS). Register service `"workload.service"` with `brokerd`.

### Phase 4C.3: Task DAG Admittance Engine
- Implement Kahn's Topological Sort Algorithm on stack memory ($< 128\text{ bytes}$).
- Enforce admittance invariants: non-empty graph, task count $\le 16$, strictly acyclic (`I-TASK-DAG-ACYCLIC`).
- Implement dynamic DAG extension validation (`Ready`/`Running` state required; completed workloads immutable `I-COMPLETED-WORKLOAD-IMMUTABLE`; downstream-only extensions `I-TASK-DAG-DOWNSTREAM-EXTENSION`).

### Phase 4C.4: Full Stage 4B Resource Lease Lifecycle Integration
- Implement end-to-end lease lifecycle engine:
  $$\text{Task Admission} \rightarrow \text{Demand Validation} \rightarrow \text{OP\_LEASE\_REQUEST} \rightarrow \text{Lease Active} \rightarrow \text{Task Execution}$$
  $$\rightarrow \text{Completion / Failure / Cancellation} \rightarrow \text{OP\_LEASE\_RELEASE} \rightarrow C_{\text{unavail}} \text{ Quarantine} \rightarrow C_{\text{avail}}$$
- Handle `LeaseState::Expired`, `TimeAuthorityLost`, and `ProviderLost` fail-closed, verifying capacity transitions to $C_{\text{unavail}}$ (Quarantine Ledger) and $C_{\text{avail}}$ remains unchanged until provider confirmation.

### Phase 4C.5: Complete Capability Lifecycle Engine
- Implement capability lifecycle:
  $$\text{Workload Root Cap} \xrightarrow{\text{SYS\_CAP\_DERIVE}} \text{Task Cap Envelope} \xrightarrow{\text{OP\_PROCESS\_SPAWN}} \text{Process Handle Table}$$
  $$\xrightarrow{\text{Task Failure / Cancel}} \text{Capability Teardown via sys\_channel\_close} \rightarrow \text{CDT Cascade Revocation}$$
- Validate capability rights attenuation ($\text{ChildRights} \subseteq \text{ParentRights}$) and verify that failed/retried tasks receive fresh capability envelopes with zero authority leakage.

### Phase 4C.6: Process Supervision & Exit Handler
- Connect `workloadd` to `"supervisor.service"`. Submit `OP_PROCESS_SPAWN` requests for ready tasks.
- Bind returned Stage 3 `ProcessId` (PID) to `TaskId`.
- Process asynchronous `OP_PROCESS_EXIT_NOTIFY` messages from `init`. Update task state (`Completed` if code 0, `Failed` if non-zero or fault).

### Phase 4C.7: 4-Phase Cancellation Pipeline
- Implement 4-Phase Cancellation Pipeline:
  1. Administrative intent registered (`WorkloadState::Cancelling`); new tasks/extensions rejected.
  2. Cooperative stop signal sent to running processes (50 ms timeout); supervisor kill invoked if timeout expires.
  3. Task capabilities closed via `sys_channel_close`; lease surrendered to `resourced` via `OP_LEASE_RELEASE`, moving capacity to $C_{\text{unavail}}$.
  4. Provider confirmation observed; capacity moved from $C_{\text{unavail}}$ to $C_{\text{avail}}$; workload reclaimed.

### Phase 4C.8: Three-Tier Recovery Class Engine
- **Class 1 (Ephemeral Pure Compute)**: Discard transient state; request fresh process from supervisor; re-execute task from step 0.
- **Class 2 (Checkpointed Stateful)**: Read latest ZeroFS checkpoint $G_c$; request fresh process; resume execution from $G_c$.
- **Class 3 (Irreversible External)**: Prohibit transparent replay; transition task state to `FailedAtMilestone(M)`; execute registered downstream compensation tasks if declared in DAG.

### Phase 4C.9: Machine-Level Verification Suite
- Implement `tests/test_stage4c.py` executing 35 automated Ring 3 integration tests in QEMU context.

---

## 6. Machine-Level Integration & Verification Suite (35 Tests)

The authoritative verification suite (`tests/test_stage4c.py`) executes Ring 3 binaries within QEMU, verifying 35 formal integration assertions:

```text
===============================================================================
STAGE 4C VERIFICATION SUITE (tests/test_stage4c.py) - 35 TEST SCENARIOS
===============================================================================
 -- WORKLOAD LIFECYCLE & IDENTITY --
 1. test_workload_create_monotonic_id          - Unique WorkloadId issuance from unified 4B allocator.
 2. test_workload_generation_increment         - Generation counter increment on mutation/restart.
 3. test_workload_full_state_transitions       - Validates Creating -> Ready -> Running -> Completed.
 4. test_workload_identity_immutability        - WorkloadId & TaskId preserved across retries; new PID allocated.

 -- DAG ADMITTANCE & MUTATION --
 5. test_task_dag_admit_valid_pipeline         - Valid linear pipeline DAG admittance.
 6. test_task_dag_reject_direct_cycle          - Immediate rejection of A -> B -> A cycle.
 7. test_task_dag_reject_complex_cycle         - Rejection of multi-node indirect cycles.
 8. test_task_dag_dependency_ordering          - Tasks execute strictly in dependency order.
 9. test_dag_mutation_authority_running        - Downstream extension permitted while Workload is Running.
10. test_dag_mutation_rejected_completed       - Extension rejected fail-closed when Workload is Completed.

 -- SUPERVISOR SPAWNER & PROCESS LIFECYCLE --
11. test_supervisor_spawner_process_launch     - Process creation via init OP_PROCESS_SPAWN IPC.
12. test_task_process_association_isolation    - 1 task per process invariant preserved.
13. test_multi_process_workload_coordination   - Workload coordinates multiple task processes.
14. test_task_clean_completion                 - Exit code 0 propagation and task completion.
15. test_task_app_failure_handling             - Non-zero exit triggers task failure; process reclaimed.
16. test_process_crash_workload_survival       - CPU fault destroys process; workload survives & retries.

 -- CAPABILITY LIFECYCLE & SECURITY ENVELOPE --
17. test_capability_attenuation_enforced       - Task capability derived via SYS_CAP_DERIVE.
18. test_task_capability_isolation_boundary    - Task cannot access capabilities outside workload envelope.
19. test_failed_task_capability_teardown       - Failed task capabilities invalidated fail-closed.
20. test_retry_fresh_capability_state          - Retried task receives fresh capability envelope.
21. test_workload_completion_no_cap_leak       - Terminal completion leaves zero capability leaks in CDT.

 -- FULL RESOURCE LEASE LIFECYCLE & QUARANTINE --
22. test_resource_demand_formulation           - DimensionCapacityVector demand construction.
23. test_lease_acquisition_via_resourced       - OP_LEASE_REQUEST IPC to resourced.
24. test_lease_rejection_insufficient_capacity - Queue/fail when C_avail capacity is exhausted.
25. test_lease_release_on_task_completion      - OP_LEASE_RELEASE invoked on task completion.
26. test_lease_quarantine_on_task_failure      - Capacity moves to C_unavail when task fails.
27. test_time_authority_loss_quarantine        - TimeAuthorityLost halts task, preserving C_unavail.
28. test_provider_loss_quarantine              - ProviderLost moves capacity to C_unavail.

 -- RECOVERY CLASSES & CANCELLATION RACES --
29. test_class1_pure_compute_replay            - Ephemeral Class 1 full replay from scratch.
30. test_class2_checkpointed_stateful_resume   - Class 2 stateful resume strictly from checkpoint Gc.
31. test_class3_irreversible_fail_at_milestone - Class 3 transitions to FailedAtMilestone(M) without replay.
32. test_workload_cancellation_quarantine      - 4-phase cancellation preserves C_unavail quarantine.
33. test_cancellation_race_process_launch      - Cancellation racing with process spawn handles cleanly.

 -- END-TO-END MEMORY & SUBSTRATE VERIFICATION --
34. test_pmm_frame_leak_neutrality_deep        - 0 physical frame leaks after full failure/retry/cancel cycles.
35. test_frozen_substrate_preservation         - 0 bytes modified in kernel, init, brokerd, resourced.
===============================================================================
```

---

## 7. Strict Non-Goals & Phase Boundary

The following are strictly OUT OF SCOPE for Phase 4C:
- **Zero Kernel Modifications**: 0 bytes modified in Stage 3 Ring 0 microkernel (`kernel/`).
- **No Workspace UI or Persistence Containers**: Workspaced belongs exclusively to Phase 4D.
- **No Autonomous Agent Perception/Reasoning Loops**: Agent runtime belongs to Phase 4E.
- **No Remote Network Mesh Dispatch**: Distributed fabric belongs to Phase 4F.

---

STATUS: DRAFT IMPLEMENTATION PLAN REV2 — PENDING USER APPROVAL
IMPLEMENTATION: NOT AUTHORIZED (PENDING USER APPROVAL OF THIS PLAN)
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
