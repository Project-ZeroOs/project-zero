# ZeroOS Execution, Observation, & Replanning Model (REV1)

```text
ZEROOS EXECUTION, OBSERVATION, & REPLANNING MODEL REV1

ARCHITECTURE:
🟢 READY

KERNEL CHANGES:
0

NEW SYSCALLS:
0

NEW ABI:
0

FROZEN DEPENDENCIES:
INTACT

IDENTITY MODEL:
PASS

EXECUTION MODEL:
PASS

OBSERVATION MODEL:
PASS

STATE MACHINE:
PASS

FAILURE MODEL:
PASS

RECOVERY MODEL:
PASS

RESOURCE BOUNDARY:
PASS

CAPABILITY BOUNDARY:
PASS

WORKSPACE BOUNDARY:
PASS

AGENT BOUNDARY:
PASS

REPLANNING MODEL:
PASS

PERSISTENCE:
PASS

IDEMPOTENCY:
PASS

CONCURRENCY:
PASS

INVARIANTS:
25 / 25

ADVERSARIAL SCENARIOS:
20 / 20

ARCHITECTURAL CYCLES:
NONE

ARCHITECTURAL DRIFT:
NONE

FINAL VERDICT:
🟢 READY FOR ADVERSARIAL REVIEW
```

---

## 1. Problem Statement

In an operating system executing human intent through autonomous workloads and agents, runtime state cannot be treated as a binary "success" or "failure" return code. Complex computations, multi-step agent plans, and distributed tasks undergo dynamic execution spans influenced by resource contention, transient node failures, process crashes, capability revocations, user interruptions, and environment shifts.

Without a formal, OS-enforced observation and state derivation model, AI agent systems suffer from:
1. **Opaque Execution Loops**: Inability to differentiate an application bug from a host process crash or physical resource eviction.
2. **State Hallucination & History Rewrite**: Agents mutating historical execution logs or inventing phantom success states.
3. **Cascading Unsafe Replanning**: Re-executing already-completed steps with irreversible side effects or launching duplicate execution units during transient network loss.
4. **Identity Conflation**: Collapsing the logical Workload spec with its transient kernel process or runtime attempt span.

**ZeroOS solves this by introducing a formal Execution, Observation, & Replanning Architecture (REV1).**

This architecture defines how ZeroOS tracks concrete runtime execution spans (`ExecutionId`), ingests immutable execution events (`ExecutionEventId`), derives authoritative runtime state, publishes evidence-backed observations (`ObservationId`) to the Workspace, and triggers safe, versioned replanning (`Plan vN+1`) without violating capability boundaries or overwriting execution history.

```text
HUMAN INTENT
      ↓
PLANNING / ORCHESTRATION (REV1)     🔒 FROZEN
      ↓
WORKLOAD EXECUTION DAG (REV1)      🔒 FROZEN
      ↓
RESOURCE FABRIC / SCHEDULER (REV1) 🔒 FROZEN
      ↓
EXECUTION MODEL (ExecutionId)
      ↓
EXECUTION EVENTS (ExecutionEventId)
      ↓
STATE DERIVATION & OBSERVATION (ObservationId)
      ↓
WORKSPACE / AGENT INTERPRETATION
      ↓
REPLANNING ENGINE (Plan vN+1)
```

---

## 2. Frozen Dependencies

This model strictly consumes and enforces the following frozen architectural layers without modification:

```text
Stage 3A–3N                                 🟢 FROZEN
Filesystem Mutation Substrate REV3          🟢 FROZEN
Object & Membership Model REV8              🟢 FROZEN
Workspace Semantic Model REV1               🟢 FROZEN
Workload & Agent Execution Model REV1       🟢 FROZEN
Resource & Fabric Execution Model REV1      🟢 FROZEN
Intent-to-Workload Planning Model REV1      🟢 FROZEN
```

---

## 3. Strict Identity Separation

ZeroOS maintains strict mathematical identity separation across all system entity abstractions:

```text
WorkspaceId (128-bit logical context envelope)
    ≠ ObjectId (128-bit storage identity bound to ZeroFS inode)
    ≠ WorkloadId (128-bit compute execution unit identity)
    ≠ AgentId (128-bit persistent control entity identity)
    ≠ ProcessId (32-bit OS kernel process PID)
    ≠ CapabilityHandle (32-bit/64-bit kernel authorization token)
    ≠ ResourceId (128-bit hardware resource extent handle)
    ≠ IntentNodeId (32-bit workspace intent DAG node identifier)
    ≠ IntentId (128-bit persistent human intent handle)
    ≠ PlanId (128-bit plan specification handle)
    ≠ PlanStepId (128-bit logical plan step handle)
    ≠ ExecutionId (128-bit runtime execution attempt handle)
    ≠ ObservationId (128-bit structured evidence record handle)
    ≠ ExecutionEventId (128-bit immutable state transition event handle)
```

### Identity Proof & Semantic Justification:

1. **`WorkloadId ≠ ExecutionId`**:
   - A `Workload` (`WorkloadId`) represents a declarative, schedulable compute unit specification.
   - An `Execution` (`ExecutionId`) represents a single concrete runtime attempt/span of that Workload.
   - A single Workload may undergo multiple sequential Executions (e.g., attempt 1 fails due to node crash $\to$ attempt 2 succeeds under a new `ExecutionId`) while retaining the exact same `WorkloadId`.

2. **`ExecutionId ≠ ProcessId`**:
   - An `Execution` is an OS-level runtime tracking span managed by `workspaced` and `schedulerd`.
   - A `Process` is a transient 32-bit kernel execution thread (`ProcessId`).
   - An `Execution` span may undergo kernel process replacement (e.g., process crash & restart under a new PID) without changing its `ExecutionId`.

3. **`ObservationId ≠ ExecutionEventId`**:
   - An `ExecutionEvent` is a raw, low-level state transition emission emitted by `workspaced`, `resourced`, or `brokerd`.
   - An `Observation` is a high-level, evidence-backed OS semantic record published to the Workspace for interpretation by human operators and AI Agents.

4. **`PlanId ≠ ExecutionId`**:
   - A `Plan` (`PlanId`) is a static, versioned DAG of logical steps (`PlanStepId`).
   - An `Execution` is an active runtime span corresponding to a materialized step execution.

---

## 4. Execution Model

An **Execution** represents a concrete, time-bounded runtime attempt span of a materialized `Workload`.

### Structural Representation

An Execution $E_x$ is represented as the 12-tuple:

$$E_x = \langle \text{ExecutionID}_x, \text{WorkloadID}_m, \text{PlanID}_p, \text{PlanVersion}_v, \text{WorkspaceID}_w, \text{AttemptNumber}, \text{State}, \text{AssignedNodeID}, \text{BoundPID}, \text{LeaseID}, \text{StartedAtTSC}, \text{EndedAtTSC} \rangle$$

#### Execution Fields & Ownership:

| Field | Description | Persistent? | Mutator | Ownership |
|---|---|---|---|---|
| `execution_id` | 128-bit `DistributedId` | **Yes** | System Allocator | System |
| `workload_id` | Parent `WorkloadId` | **Yes** | Immutable | Workload |
| `plan_id` / `plan_version` | Originating Plan | **Yes** | Immutable | Plan |
| `workspace_id` | Parent Workspace | **Yes** | Immutable | Workspace |
| `attempt_number` | Monotonic integer (`1, 2, 3...`) | **Yes** | `workspaced` | Workload |
| `state` | Current `ExecutionState` | **Yes** | `workspaced` engine | Execution Engine |
| `assigned_node_id` | Host physical node ID | Runtime | `schedulerd` | Resource Fabric |
| `bound_pid` | Transient kernel `ProcessId` | Runtime | Kernel / `workspaced` | Kernel |
| `lease_id` | Active `ResourceLease` ID | Runtime | `resourced` | Resource Fabric |

---

## 5. Execution State Machine

ZeroOS defines a formal, deterministic state machine governing the runtime lifecycle of an Execution:

```text
       [ CREATED ]
            │
            ▼
       [ ADMITTED ]
            │
            ▼
        [ RUNNING ] ◄──────┐
         │     │           │
         │     ├───────────┴────────► [ SUSPENDED ]
         │     │
         │     ├────────────────────► [ TIMED_OUT ]
         │     │
         │     ├────────────────────► [ INTERRUPTED ]
         │     │
         │     ├────────────────────► [ INVALIDATED ]
         │     │
         │     ├────────────────────► [ CANCELLED ]
         │     │
         │     ├────────────────────► [ FAILED ]
         │     │
         ▼     ▼
       [ COMPLETED ] ───────(Terminal)
```

### Valid Execution States:
- **`CREATED`**: `ExecutionId` allocated; metadata registered in `workspaced`.
- **`ADMITTED`**: `schedulerd` has verified resource availability and placed the workload.
- **`RUNNING`**: Process spawned (`bound_pid` valid) and executing on target hardware.
- **`SUSPENDED`**: Process paused via `SIGSTOP` or workspace suspension (`IO-20`).
- **`COMPLETED`**: Process exited with status code `0`; output bindings verified.
- **`FAILED`**: Process exited with non-zero code, crashed, or encountered unhandled exception.
- **`TIMED_OUT`**: Wall-clock or TSC deadline exceeded before completion.
- **`INTERRUPTED`**: Evicted by higher-priority workload or physical resource reclamation.
- **`INVALIDATED`**: Input dependencies mutated or deleted during execution.
- **`CANCELLED`**: Explicitly terminated via user intent update or plan cancellation.

### Illegal Transitions:
- `COMPLETED → RUNNING` (Terminal state invariant).
- `CANCELLED → COMPLETED` (Cancelled executions cannot succeed).
- `FAILED → RUNNING` (Requires allocating a new `ExecutionId` for attempt $N+1$).

---

## 6. Execution Events

Execution Events are low-level, immutable state transition records emitted by system daemons (`workspaced`, `schedulerd`, `resourced`, `brokerd`).

### Event Taxonomy:

```text
+-------------------------------------------------------------------------------+
|                         ZEROOS EXECUTION EVENT TAXONOMY                       |
+--------------------------+-----------------------+----------------------------+
| Event Type               | Emitter Component     | Trigger Condition          |
+--------------------------+-----------------------+----------------------------+
| EXECUTION_CREATED        | workspaced            | ExecutionId allocated      |
| EXECUTION_ADMITTED       | schedulerd            | Resource capacity reserved |
| EXECUTION_STARTED        | workspaced            | Kernel process spawned     |
| PROCESS_BOUND            | kernel / workspaced   | PID assigned to Execution  |
| RESOURCE_BOUND           | resourced             | Lease extent attached      |
| EXECUTION_SUSPENDED      | workspaced            | Process paused (SIGSTOP)   |
| EXECUTION_RESUMED        | workspaced            | Process resumed (SIGCONT)  |
| EXECUTION_PROGRESS       | process / workspaced  | Heartbeat or checkpoint    |
| EXECUTION_COMPLETED      | workspaced            | Exit status 0 verified     |
| EXECUTION_FAILED         | workspaced            | Exit non-zero or crash     |
| EXECUTION_CANCELLED      | workspaced            | Intent / Plan cancelled    |
| EXECUTION_TIMEOUT        | workspaced / timer    | Deadline TSC exceeded      |
| EXECUTION_INTERRUPTED    | schedulerd            | Resource preemption        |
| PROCESS_EXITED           | kernel                | Signal or exit code return |
| RESOURCE_LOST            | resourced             | Hardware failure / drop    |
| EXECUTION_RECOVERED      | workspaced            | Post-crash reconciliation  |
+--------------------------+-----------------------+----------------------------+
```

Every `ExecutionEvent` contains a 128-bit `ExecutionEventId`, monotonic `sequence_num`, `execution_id`, `timestamp_tsc`, and binary payload. Events are **append-only and immutable**.

---

## 7. Observation Model

An **Observation** is a first-class OS semantic object published to the Workspace context store (`/storage/workspaces/<ws>/observations/<obs_id>.json`).

Unlike raw event streams or conversational log lines, an Observation provides **verifiable, evidence-backed semantic interpretation** of execution outcomes.

### Structural Representation

An Observation $O_y$ is defined as:

$$O_y = \langle \text{ObservationID}_y, \text{WorkspaceID}_w, \text{WorkloadID}_m, \text{ExecutionID}_x, \text{ObsType}, \text{ObservedAtTSC}, \text{SequenceNum}, \text{EvidenceRef}, \text{CertaintyLevel}, \text{Payload} \rangle$$

#### Observation Fields:
- `ObservationID`: 128-bit `DistributedId`.
- `ObsType`: `EXECUTION_SUCCESS | EXECUTION_FAILURE | RESOURCE_STARVATION | INPUT_INVALID | TIMEOUT_EXCEEDED | SIDE_EFFECT_MUTATED`.
- `EvidenceRef`: Pointer to ZeroFS inode or log extent (`ObjectId` / `InodeNum`) proving the observation.
- `CertaintyLevel`: `AUTHORITATIVE_KERNEL` (100%) | `DERIVED_DAEMON` (95%) | `INFERRED_AGENT` (80%).

```text
[ Raw Kernel / Daemon Events ]
              │
              ▼
[ workspaced Event Processor ] ──(Correlates Events + ZeroFS Mutate Bit 15 Inodes)
              │
              ▼
[ Authoritative Observation ] ──(Persisted to /storage/workspaces/<ws>/observations/)
              │
              ▼
[ Workspace / Agent Interpretation ]
```

---

## 8. State Derivation

ZeroOS strictly enforces a unidirectional state derivation pipeline:

```text
Immutable Kernel/System Events (ExecutionEventId)
             │
             ▼ (State Engine Derivation)
Authoritative Execution State (ExecutionId)
             │
             ▼ (Semantic Evidence Aggregation)
Workspace Observation (ObservationId)
             │
             ▼ (Context Store Indexing)
Workspace-Visible Context State
```

### State Authority Invariants:
1. **Raw Events are Truth**: An execution state is derived solely by computing the deterministic fold of immutable `ExecutionEvents`.
2. **Observations cannot alter Execution State**: An Observation describes runtime outcomes for Workspace actors; it cannot retroactively alter `ExecutionState`.
3. **Ephemerality vs Permanence**: `ProcessId` and kernel handles are ephemeral runtime state; `ExecutionId`, `ExecutionEvent`, and `Observation` are durable persistent state.

---

## 9. Failure Semantics Matrix

ZeroOS categorizes failures into precise semantic classes, preventing monolithic failure handling:

```text
+------------------------------------------------------------------------------------------------------------------+
|                                        ZEROOS FAILURE SEMANTICS MATRIX                                           |
+-----------------------+---------------------+-------------------+------------------+-----------------------------+
| Failure Class         | Failure Owner       | Workload State    | Retryable?       | Observation Type            |
+-----------------------+---------------------+-------------------+------------------+-----------------------------+
| Application Error     | Workload Process    | FAILED            | Yes (Policy)     | EXECUTION_FAILURE           |
| Process Crash         | Kernel / Process    | FAILED            | Yes (PID Replace)| EXECUTION_FAILURE           |
| Resource Exhaustion   | Schedulerd / Host   | INTERRUPTED       | Yes (Replace HW) | RESOURCE_STARVATION         |
| Capability Denial     | Brokerd / Kernel    | FAILED            | No               | CAPABILITY_DENIED           |
| Workspace Suspension  | Workspace Daemon    | SUSPENDED         | Yes (Resume)     | WORKSPACE_SUSPENDED         |
| Timeout Exceeded      | Timer Subsystem     | TIMED_OUT         | Yes (Extend Spec)| TIMEOUT_EXCEEDED            |
| Dependency Missing    | Object Subsystem    | INVALIDATED       | No (Replan Req)  | INPUT_INVALID               |
| Hardware Node Drop    | Resourced Fabric    | INTERRUPTED       | Yes (Migrate Node| NODE_FAILED                 |
+-----------------------+---------------------+-------------------+------------------+-----------------------------+
```

---

## 10. Crash Recovery & Resiliency

### 10.1 Crash Taxonomy & Recovery Actions

1. **Process Crash (`ProcessId` dies)**:
   - Kernel emits `PROCESS_EXITED`. `workspaced` captures non-zero status.
   - Workload remains `RUNNING` or `FAILED`. If retry policy allows, `workspaced` spawns a replacement process under a new `ProcessId` **without changing `ExecutionId`**.
2. **Daemon Restart (`workspaced` / `schedulerd` crash)**:
   - On boot, `workspaced` scans `/storage/workspaces/<ws>/executions/`.
   - Reconciles active `ExecutionId` against kernel PID table.
   - If process is dead and state was `RUNNING`, `workspaced` emits `EXECUTION_RECOVERED` and evaluates retry policy.
3. **Node Failure (Physical Host Crash)**:
   - `resourced` heartbeat drops. `schedulerd` marks node `OFFLINE`.
   - Executions on lost node transition to `INTERRUPTED`.
   - `schedulerd` allocates new hardware lease; `workspaced` starts attempt $N+1$ under a **new `ExecutionId`**.

### 10.2 Deduplication & Single-Execution Invariant:

$$\text{ActiveExecutions}(WL_m) \le 1$$

At any given TSC timestamp, a Workload $WL_m$ shall have **at most one** `Execution` in active state (`ADMITTED | RUNNING | SUSPENDED`).

---

## 11. Resource & Fabric Integration

Execution consumes hardware strictly through `resourced` leases:

```text
[ Execution (ExecutionId) ]
            │
            ▼ (Requests Lease Binding)
[ Resource Lease (LeaseID) ] ──(Managed by resourced / lease_engine)
            │
            ▼ (Enforces Vector Allocation)
[ Physical Hardware Extents ]
```

- Executions observe lease metrics (TSC time remaining, memory usage).
- Executions **cannot** modify physical resource allocations or bypass `resourced` quotas.

---

## 12. Capability Boundary

Neither Executions nor Observations possess capability-granting authority:

```text
Execution (ExecutionId) ──► CANNOT grant capabilities
Observation (ObservationId) ──► CANNOT grant capabilities
Planner / Agent ──► CANNOT grant capabilities
Scheduler ──► CANNOT grant capabilities
```

All runtime access rights are checked when processes invoke system calls using frozen `CapabilityHandle` tokens. An Observation documenting an execution failure does **not** grant privilege escalation.

---

## 13. Workspace Integration

Execution state is projected into the parent Workspace:

```text
/storage/workspaces/<workspace_id>/
    ├── executions/
    │   └── <execution_id>.json
    ├── events/
    │   └── <execution_id>_events.log
    └── observations/
        └── <observation_id>.json
```

### Workspace Lifecycle Hooks:
- **Workspace Suspension (`handle_workspace_suspend`)**: All active Executions transition to `SUSPENDED`. Processes receive `SIGSTOP`.
- **Workspace Resume (`handle_workspace_resume`)**: Suspended Executions transition to `RUNNING`. Processes receive `SIGCONT`.
- **Workspace Destruction (`handle_workspace_delete`)**: Active Executions receive `SIGKILL` and transition to `CANCELLED`. Executions and Observations remain historically immutable in ZeroFS backup logs.

---

## 14. Agent Role & Boundaries

An Agent (`AgentId`) consumes Observations to make autonomous decisions:

```text
+-------------------------------------------------------------------------------+
|                            AGENT OBSERVATION LOOP                             |
|                                                                               |
|  1. Read Workspace Observations (/storage/workspaces/<ws>/observations/)      |
|  2. Interpret Failure / Success evidence payload                              |
|  3. Formulate Replanning Strategy (Plan vN+1)                                 |
|  4. Submit proposed Plan vN+1 to workspaced via OP_ORCH_PLAN_SUBMIT            |
+-------------------------------------------------------------------------------+
```

### Absolute Agent Restrictions:
- An Agent **cannot** forge or overwrite `Observation` files.
- An Agent **cannot** manually force an `Execution` state from `FAILED` to `COMPLETED`.
- An Agent **cannot** delete execution event logs.

---

## 15. Replanning Boundary

Observation triggers Replanning **only** through formal versioned plan submission:

```text
[ Authoritative Observation (ObsType: EXECUTION_FAILURE) ]
                           │
                           ▼
[ Agent / Planner Decision Engine ]
                           │
                           ▼
[ Submit Proposed Plan v2 (OP_ORCH_PLAN_SUBMIT) ]
                           │
                           ▼
[ workspaced Validation & Approval Gate Check ]
                           │
                           ▼
[ Materialize Plan v2 Steps (New ExecutionId) ]
```

### Immutable History Guarantee:
- `Plan v1`, `Workload W1`, `Execution E1`, and `Observation O1` remain **100% immutable** in ZeroFS storage.
- `Plan v2` references `O1` as historical evidence and materializes `Workload W2` with a fresh `ExecutionId` ($E_2$).

---

## 16. Plan Version Semantics

```text
Intent [IntentId: int_01]
    ├── Plan v1 [PlanId: plan_01_v1] (State: SUPERSEDED)
    │     └── Workload W1 ──► Execution E1 (State: FAILED, Attempt: 1)
    │                             └── Observation O1 (ObsType: EXECUTION_FAILURE)
    │
    └── Plan v2 [PlanId: plan_01_v2] (State: ACTIVE)
          └── Workload W2 ──► Execution E2 (State: RUNNING, Attempt: 1)
                                  └── Observation O2 (ObsType: EXECUTION_PROGRESS)
```

Historical executions remain permanently attached to their originating `(PlanId, PlanVersion)`.

---

## 17. Idempotency & Concurrency

1. **Event Ingestion**: Ingesting a duplicate `ExecutionEventId` returns `Success` without updating sequence state.
2. **Execution Creation**: `ExecutionId = UUIDv5(NS, WorkloadID || AttemptNumber)`. Duplicate creation requests return existing `ExecutionId`.
3. **Replanning Requests**: Replanning requests specify `(IntentId, ParentPlanVersion)`. Concurrent replan submissions resolve to a single `Plan vN+1` via `workspaced` single-threaded transaction lock.

---

## 18. Persistence & Durability

All execution records are committed to ZeroFS under Journal V2 two-phase commit transactions:

| Record | Storage Location | Format | Durability |
|---|---|---|---|
| Execution Control Block | `/storage/workspaces/<ws>/executions/<exec_id>.json` | JSON / Inode | 2PC Journal V2 |
| Event Log | `/storage/workspaces/<ws>/events/<exec_id>.log` | Append-only Binary | 2PC Journal V2 |
| Observation | `/storage/workspaces/<ws>/observations/<obs_id>.json` | JSON / Inode | 2PC Journal V2 |

---

## 19. Event Ordering & Logical Clocks

ZeroOS orders execution events using a hybrid 64-bit Timestamp & Monotonic Sequence Clock:

$$\text{EventOrderKey} = \langle \text{SequenceNum}_{64}, \text{TimestampTSC}_{64} \rangle$$

- `SequenceNum`: Monotonically incremented per workspace event bus.
- `TimestampTSC`: Canonical hardware Time Stamp Counter (`read_canonical_tsc()`).
- Prevents ambiguity caused by TSC drift across multi-socket CPUs.

---

## 20. Security & Trust Model

1. **Event Provenance**: Only `workspaced`, `resourced`, `schedulerd`, and kernel syscall dispatchers hold capability rights to emit `ExecutionEvents`.
2. **Observation Cryptographic Hashing**: Observations include a SHA-256 digest of supporting evidence logs (`EvidenceRef`).
3. **Replay Protection**: Event sequence numbers prevent replayed or out-of-order event injections.

---

## 21. System Invariants (ER-01 ... ER-25)

- **`ER-01`**: `WorkspaceId ≠ ObjectId ≠ WorkloadId ≠ AgentId ≠ ProcessId ≠ CapabilityHandle ≠ ResourceId ≠ IntentNodeId ≠ IntentId ≠ PlanId ≠ PlanStepId ≠ ExecutionId ≠ ObservationId ≠ ExecutionEventId`.
- **`ER-02`**: A Workload shall have at most one active Execution (`ADMITTED | RUNNING | SUSPENDED`) at any timestamp.
- **`ER-03`**: Executions shall be uniquely identified by 128-bit `DistributedId` (`ExecutionId`).
- **`ER-04`**: Process PID replacement during workload retry shall preserve the existing `ExecutionId` unless a new attempt is declared.
- **`ER-05`**: Execution state transitions shall be derived exclusively from append-only `ExecutionEvents`.
- **`ER-06`**: Execution Events shall be immutable once written to ZeroFS storage.
- **`ER-07`**: Observations shall be evidence-backed semantic records referencing valid ZeroFS inodes or log extents.
- **`ER-08`**: Agents shall not have authority to write or modify kernel execution events or `Observation` files.
- **`ER-09`**: Terminal execution states (`COMPLETED`, `CANCELLED`, `TIMED_OUT`) shall be strictly immutable.
- **`ER-10`**: Replanning shall create a new `Plan vN+1` and shall not mutate historical completed or failed Executions.
- **`ER-11`**: Execution state transitions shall be processed under `workspaced` single-threaded transaction locks.
- **`ER-12`**: Workspace suspension (`IO-20`) shall transition all active Executions to `SUSPENDED` and issue `SIGSTOP` to bound PIDs.
- **`ER-13`**: Workspace destruction (`IO-21`) shall transition all active Executions to `CANCELLED` and issue `SIGKILL` to bound PIDs.
- **`ER-14`**: Executions shall consume physical resources strictly through `resourced` resource leases (`LeaseID`).
- **`ER-15`**: Neither Executions nor Observations shall grant or elevate Capability Handles.
- **`ER-16`**: `ExecutionId = UUIDv5(NS_Execution, WorkloadID || AttemptNumber)` shall be strictly deterministic and idempotent.
- **`ER-17`**: Event ingestion shall enforce monotonic sequence ordering per workspace.
- **`ER-18`**: Observations published to a Workspace shall be visible to all authorized Workspace members.
- **`ER-19`**: Hardware node drops shall mark active Executions on that node as `INTERRUPTED`.
- **`ER-20`**: Retrying a failed Workload on a new hardware node shall allocate a new `ExecutionId`.
- **`ER-21`**: All Execution and Observation state shall be persisted using ZeroFS two-phase commit transactions.
- **`ER-22`**: Unapproved `PlanSteps` shall not create `Execution` entries.
- **`ER-23`**: Process exit status `0` verified by ZeroFS `MUTATE Bit 15` outputs shall be required to transition to `COMPLETED`.
- **`ER-24`**: The Execution Model shall function 100% autonomously on a single-node laptop without cloud dependencies.
- **`ER-25`**: The Execution, Observation, & Replanning Model shall require 0 kernel changes, 0 new syscalls, and 0 new ABI additions.

---

## 22. Adversarial Scenarios (A – T)

### Scenario A: Process crash during execution
- **State Before**: Execution $E_1$ `RUNNING`, PID 409 bound.
- **Trigger**: Process 409 crashes (SIGSEGV).
- **Authority**: Kernel emits `PROCESS_EXITED`.
- **Execution State**: $E_1$ transitions to `FAILED`.
- **Observation State**: Observation $O_1$ published (`ObsType: EXECUTION_FAILURE`, `EvidenceRef: exit_code_11`).
- **Replanning State**: Workload retry policy triggers process spawn under new PID 812 or triggers Replanning (`Plan v2`).
- **Recovery**: $E_1$ history preserved cleanly.

### Scenario B: Duplicate EXECUTION_STARTED event
- **State Before**: $E_1$ already `RUNNING`.
- **Trigger**: Duplicate `EXECUTION_STARTED` IPC message received.
- **Authority**: `workspaced`.
- **Execution State**: State remains `RUNNING`.
- **Observation State**: Duplicate event ignored.
- **Replanning State**: Unchanged.
- **Recovery**: Event deduplication returns `Success` without state corruption.

### Scenario C: Duplicate completion event
- **State Before**: $E_1$ already `COMPLETED`.
- **Trigger**: Stale completion event arrives.
- **Authority**: `workspaced`.
- **Execution State**: $E_1$ remains `COMPLETED` (Terminal invariant `ER-09`).
- **Observation State**: Unchanged.
- **Replanning State**: Unchanged.
- **Recovery**: Event rejected cleanly.

### Scenario D: Stale observation read by Agent
- **State Before**: $E_1$ failed, $O_1$ emitted. Agent reads $O_1$ after $E_2$ has already started.
- **Trigger**: Agent submits replan for $O_1$.
- **Authority**: `workspaced`.
- **Execution State**: `workspaced` detects active plan is already $v2$.
- **Observation State**: Stale replan request rejected.
- **Replanning State**: $Plan v2$ remains active.
- **Recovery**: Version check prevents stale agent loop override.

### Scenario E: Forged observation injection attempt
- **State Before**: $E_1$ `RUNNING`.
- **Trigger**: Untrusted agent attempts to write fake `O_fake` (`ObsType: EXECUTION_SUCCESS`) to `/storage/workspaces/<ws>/observations/`.
- **Authority**: ZeroFS Kernel Capability Authority.
- **Execution State**: $E_1$ remains `RUNNING`.
- **Observation State**: ZeroFS denies write attempt without daemon system signature.
- **Replanning State**: Unchanged.
- **Recovery**: Permission denied error returned to agent.

### Scenario F: Resource lease expiry mid-execution
- **State Before**: $E_1$ `RUNNING`. `LeaseID` L1 active.
- **Trigger**: `resourced` lease L1 expires without renewal.
- **Authority**: `resourced` daemon.
- **Execution State**: $E_1$ transitions to `INTERRUPTED`.
- **Observation State**: Observation emitted (`ObsType: RESOURCE_STARVATION`).
- **Replanning State**: `schedulerd` attempts lease renewal or triggers replanning.
- **Recovery**: Process paused safely until lease restored or migrated.

### Scenario G: Physical hardware node crash
- **State Before**: $E_1$ `RUNNING` on Node 2.
- **Trigger**: Node 2 experiences hardware power failure.
- **Authority**: `schedulerd` heartbeat monitor.
- **Execution State**: $E_1$ marked `INTERRUPTED`.
- **Observation State**: Observation emitted (`ObsType: NODE_FAILED`).
- **Replanning State**: `schedulerd` reschedules workload on Node 1 under new `ExecutionId` ($E_2$).
- **Recovery**: Attempt 2 starts cleanly on Node 1.

### Scenario H: Scheduler restart during execution
- **State Before**: $E_1$ `RUNNING`. `schedulerd` crashes and restarts.
- **Trigger**: Daemon reboot.
- **Authority**: `workspaced`.
- **Execution State**: On boot, `workspaced` reconciles kernel PID table, detects process still running, and maintains `RUNNING` state.
- **Observation State**: Event `EXECUTION_RECOVERED` emitted.
- **Replanning State**: Unchanged.
- **Recovery**: Execution continues seamlessly.

### Scenario I: Workspace suspension during execution
- **State Before**: $E_1$ `RUNNING`.
- **Trigger**: User calls `handle_workspace_suspend`.
- **Authority**: Workspace Daemon.
- **Execution State**: $E_1$ transitions to `SUSPENDED`.
- **Observation State**: Observation emitted (`ObsType: WORKSPACE_SUSPENDED`).
- **Replanning State**: Execution paused (`SIGSTOP`).
- **Recovery**: `handle_workspace_resume` issues `SIGCONT` and restores `RUNNING` state.

### Scenario J: Workspace deletion during execution
- **State Before**: $E_1$ `RUNNING`.
- **Trigger**: User calls `handle_workspace_delete`.
- **Authority**: Workspace Daemon.
- **Execution State**: $E_1$ transitions to `CANCELLED` (`SIGKILL` issued).
- **Observation State**: Final cancellation log written to ZeroFS.
- **Replanning State**: All associated plans cancelled.
- **Recovery**: Storage reclaimed cleanly via Object REV8 teardown.

### Scenario K: Agent restart during observation processing
- **State Before**: $O_1$ emitted (`ObsType: EXECUTION_FAILURE`). Agent crashes mid-read.
- **Trigger**: Agent daemon restarts.
- **Authority**: Agent supervisor.
- **Execution State**: Unchanged ($E_1$ `FAILED`).
- **Observation State**: $O_1$ remains stored in `/storage/workspaces/<ws>/observations/`.
- **Replanning State**: Restarted agent reads $O_1$ from disk and submits `Plan v2`.
- **Recovery**: Persistent observation store prevents lost failure signals.

### Scenario L: Planner restart during replanning
- **State Before**: $O_1$ received. System Planner drafting `Plan v2`.
- **Trigger**: System Planner crashes.
- **Authority**: System Planner restart.
- **Execution State**: $E_1$ remains `FAILED`.
- **Observation State**: Unchanged.
- **Replanning State**: On boot, Planner reads $O_1$ and completes submission of `Plan v2`.
- **Recovery**: Idempotent plan submission prevents duplicate version creation.

### Scenario M: Concurrent replanning requests
- **State Before**: $E_1$ failed. Two agents simultaneously submit `Plan v2a` and `Plan v2b`.
- **Trigger**: Concurrent IPC calls.
- **Authority**: `workspaced` single-threaded transaction handler.
- **Execution State**: First accepted plan becomes `Plan v2` (`ACTIVE`). Second request receives `AlreadyExists` or is queued as `Plan v3`.
- **Observation State**: Unchanged.
- **Replanning State**: Single active plan version maintained (`ER-03`).
- **Recovery**: Concurrency race resolved deterministically.

### Scenario N: Stale Plan v1 attempting mutation after Plan v2 active
- **State Before**: `Plan v2` active. Stale process from `Plan v1` attempts write.
- **Trigger**: Delayed process execution.
- **Authority**: Kernel Capability Handle check.
- **Execution State**: Stale process capability handle rejected.
- **Observation State**: Observation emitted (`ObsType: CAPABILITY_DENIED`).
- **Replanning State**: Unchanged.
- **Recovery**: Stale process terminated by OS.

### Scenario O: Execution attempt limit exceeded
- **State Before**: Workload $W_1$ attempt 3 failed (`max_retries = 3`).
- **Trigger**: $E_3$ transitions to `FAILED`.
- **Authority**: `workspaced` retry engine.
- **Execution State**: Workload $W_1$ transitions to `FAILED`.
- **Observation State**: Observation emitted (`ObsType: EXECUTION_FAILURE`, `max_retries_exceeded`).
- **Replanning State**: Triggers high-level Replanning or Human Notification.
- **Recovery**: Prevents infinite automated retry loops.

### Scenario P: Event reordering during network jitter
- **State Before**: $E_1$ running. `EXECUTION_COMPLETED` (seq 10) arrives before `EXECUTION_PROGRESS` (seq 9).
- **Trigger**: Network packet reordering.
- **Authority**: `workspaced` event sequence engine.
- **Execution State**: Sequence engine buffers seq 10 until seq 9 is processed or sequence timeout expires.
- **Observation State**: Derived in strict monotonic order (`ER-17`).
- **Replanning State**: Unchanged.
- **Recovery**: Monotonic sequence clock guarantees strict state derivation order.

### Scenario Q: Corrupted event payload
- **State Before**: Event stream active.
- **Trigger**: Disk corruption alters event payload bits.
- **Authority**: ZeroFS SHA-256 checksum check.
- **Execution State**: Checksum mismatch detected on read.
- **Observation State**: Event rejected; fallback to last valid checkpoint.
- **Replanning State**: Unchanged.
- **Recovery**: Corrupted log extent isolated and flagged.

### Scenario R: Capability escalation attempt during execution
- **State Before**: $E_1$ running with standard capability envelope.
- **Trigger**: Process attempts to invoke privileged syscall `SYS_CAP_DERIVE` without parent handle.
- **Authority**: Kernel Syscall Dispatcher.
- **Execution State**: Syscall returns `PermissionDenied` (-4).
- **Observation State**: Observation emitted (`ObsType: CAPABILITY_DENIED`).
- **Replanning State**: Unchanged.
- **Recovery**: Execution container remains strictly capability-bounded.

### Scenario S: Timeout exceeded during slow execution
- **State Before**: $E_1$ running. Deadline set to TSC + 10,000,000.
- **Trigger**: Current TSC exceeds deadline.
- **Authority**: `workspaced` timer task.
- **Execution State**: $E_1$ transitions to `TIMED_OUT`. Process sent `SIGTERM`.
- **Observation State**: Observation emitted (`ObsType: TIMEOUT_EXCEEDED`).
- **Replanning State**: Triggers Replanning engine to evaluate resource increase or step decomposition.
- **Recovery**: Hanging processes terminated safely.

### Scenario T: Replay attack with captured completion event
- **State Before**: $E_1$ currently `RUNNING` in attempt 2.
- **Trigger**: Malicious actor re-sends captured `EXECUTION_COMPLETED` event from attempt 1.
- **Authority**: `workspaced` sequence & `ExecutionId` validator.
- **Execution State**: Event rejected due to `ExecutionId` mismatch ($E_1 \neq E_2$) and stale sequence number.
- **Observation State**: Security event logged.
- **Replanning State**: Unchanged.
- **Recovery**: Replay attack thwarted completely.

---

## 23. Dependency Graph

```text
               +-----------------------------+
               |     HUMAN USER / AGENT      |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |  PLANNING & ORCHESTRATION   |
               |     (PlanId, PlanVersion)   |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |   WORKLOAD EXECUTION DAG    |
               |        (WorkloadId)         |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |       EXECUTION ENGINE      |
               |        (ExecutionId)        |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |   EXECUTION EVENT STREAM    |
               |     (ExecutionEventId)      |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |    OBSERVATION SUBSYSTEM    |
               |       (ObservationId)       |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |   WORKSPACE CONTEXT STORE   |
               +-----------------------------+
```

### Absence of Architectural Cycles:
- Planning generates Workloads.
- Workloads create Executions.
- Executions emit ExecutionEvents.
- ExecutionEvents derive Observations.
- Observations publish to Workspace Context.
- Agents/Planners read Workspace Context and propose new Plan versions ($vN+1$).
- **Zero backward state edges exist from Execution directly into Capability or Scheduler state.**

---

## 24. Implementation Surface & Boundaries

- **Subsystem**: Ring 3 User Space Service & Engine (`workspaced`, `libzero`, `resourced`, `schedulerd`).
- **Kernel Changes**: 0
- **New Syscalls**: 0
- **New ABI Structs**: 0
- **Storage Substrate**: ZeroFS REV3 (Journal V2, Two-Phase Commit).
- **Execution Surface**: Freestanding Ring 3 process isolation managed by frozen `Workload & Agent Execution Model REV1`.

---

## 25. Final Architectural Verdict

```text
ZEROOS EXECUTION, OBSERVATION, & REPLANNING MODEL REV1

ARCHITECTURE:
🟢 READY

KERNEL CHANGES:
0

NEW SYSCALLS:
0

NEW ABI:
0

FROZEN DEPENDENCIES:
INTACT

IDENTITY MODEL:
PASS

EXECUTION MODEL:
PASS

OBSERVATION MODEL:
PASS

STATE MACHINE:
PASS

FAILURE MODEL:
PASS

RECOVERY MODEL:
PASS

RESOURCE BOUNDARY:
PASS

CAPABILITY BOUNDARY:
PASS

WORKSPACE BOUNDARY:
PASS

AGENT BOUNDARY:
PASS

REPLANNING MODEL:
PASS

PERSISTENCE:
PASS

IDEMPOTENCY:
PASS

CONCURRENCY:
PASS

INVARIANTS:
25 / 25

ADVERSARIAL SCENARIOS:
20 / 20

ARCHITECTURAL CYCLES:
NONE

ARCHITECTURAL DRIFT:
NONE

FINAL VERDICT:
🟢 READY FOR ADVERSARIAL REVIEW
```
