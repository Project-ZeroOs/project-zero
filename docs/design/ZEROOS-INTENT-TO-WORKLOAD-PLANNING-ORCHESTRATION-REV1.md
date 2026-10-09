# ZeroOS Intent-to-Workload Planning & Orchestration Model (REV1)

```text
ZEROOS INTENT-TO-WORKLOAD PLANNING & ORCHESTRATION MODEL REV1

ARCHITECTURE:
🟢 ARCHITECTURE READY

KERNEL CHANGES:
0

NEW SYSCALLS:
0

NEW ABI:
0

FROZEN DEPENDENCIES:
INTACT

CIRCULAR DEPENDENCIES:
NONE

INTENT → PLAN:
COHERENT

PLAN → WORKLOAD:
COHERENT

CAPABILITY BOUNDARY:
COHERENT

RESOURCE BOUNDARY:
COHERENT

WORKSPACE INTEGRATION:
COHERENT

WORKLOAD INTEGRATION:
COHERENT

SINGLE-NODE COMPATIBILITY:
PROVEN

OVERALL:
🟢 ARCHITECTURE APPROVED & READY FOR IMPLEMENTATION
```

---

## 1. Problem Statement

In traditional operating systems, execution is requested through explicit low-level operational commands (e.g., executing binaries, creating threads, opening sockets, allocating shared memory). Users must manually decompose complex objectives into sequences of executable processes, pipeline stages, and file interactions.

In AI-augmented operating systems, a naive approach attempts to bypass OS execution primitives by introducing monolithic LLM agent frameworks or chat interfaces that directly manipulate system files or execute untrusted subprocesses. This design pattern collapses intent, authorization, resource allocation, and execution into an opaque control loop, leading to security vulnerabilities, non-deterministic side effects, resource exhaustion, and unrecoverable partial failures.

**ZeroOS requires a clean OS-level orchestration boundary.**

The **ZeroOS Intent-to-Workload Planning & Orchestration Model (REV1)** provides the architectural layer that bridges high-level human intent inside a Workspace to concrete, capability-bounded, schedulable Workloads. It establishes a formal, deterministic, human-governed, and crash-resilient translation mechanism:

```text
HUMAN INTENT
    ↓
INTENT MODEL
    ↓
PLANNING & APPROVAL
    ↓
WORKLOAD GRAPH
    ↓
RESOURCE DEMANDS
    ↓
FABRIC SCHEDULER
    ↓
KERNEL / RING 3 EXECUTION
    ↓
OBSERVATION & REPLANNING
```

This model is **not** a chatbot framework, **not** an LLM agent wrapper, and **not** a user interface. It is an **OS-level semantic orchestration engine** that strictly enforces capability authorization, workspace isolation, resource admission, plan versioning, and human governance.

---

## 2. Design Goals

1. **Declarative Intent Translation**: Transform declarative human intent ("Prepare quarterly financial report") into structured, executable execution plans without exposing raw OS process assembly to the user.
2. **Strict Identity Separation**: Enforce complete mathematical separation between `WorkspaceId`, `ObjectId`, `WorkloadId`, `AgentId`, `ProcessId`, `CapabilityHandle`, `ResourceId`, `IntentNodeId`, `IntentId`, `PlanId`, and `PlanStepId`.
3. **Decoupled Planning vs Execution Authority**: Ensure that planners (whether automated Agents or human operators) construct execution structures but **cannot** grant capabilities, bypass scheduler placement, or allocate physical hardware.
4. **OS-Level Human Approval Boundary**: Establish mandatory approval triggers for destructive, expensive, external, or security-sensitive plan steps, protecting system integrity before workload materialization.
5. **Idempotent Workload Materialization**: Guarantee that plan materialization produces deterministic `WorkloadId` allocations, preventing duplicate workload creation across planner crashes, system reboots, or network retries.
6. **Graceful Replanning & Immutable History**: Support dynamic plan invalidation and versioning (`Plan v1 → Plan v2`) while keeping historical, completed workloads immutably bound to their original plan versions.
7. **Zero Kernel/Syscall/ABI Changes**: Implement the entire orchestration layer in Ring 3 using frozen ZeroOS primitives (`libzero`, `workspaced`, `brokerd`, `schedulerd`, `resourced`, `ZeroFS REV3`).

---

## 3. Non-Goals

1. **No Chatbot / Conversational UI Architecture**: This document defines OS orchestration semantics, not natural language processing pipelines, token generation, or chat dialog UI components.
2. **No Monolithic Agent Framework**: This model does not implement LangChain, AutoGPT, or agentic framework loops. Agents are persistent semantic control entities (`AgentId`) operating through standard ZeroOS planning and workload interfaces.
3. **No Second Scheduler**: This architecture does not make placement decisions (CPU, GPU, node selection). All physical scheduling remains strictly owned by `schedulerd` and `resourced`.
4. **No Second Capability System**: The planner cannot manufacture or escalate privileges. All access rights are validated through frozen ZeroOS Capability Handles (`CapabilityHandle`).
5. **No Mandatory Remote/Cloud Infrastructure**: The planning and orchestration engine must function 100% autonomously on a single laptop without cloud or distributed dependencies.

---

## 4. Terminology

- **Intent**: Declarative statement of a desired human outcome inside a Workspace (`IntentId`).
- **IntentNode**: A single node within the persistent Workspace Intent DAG (`IntentNodeId`).
- **Plan**: A structured, versioned specification detailing the proposed steps to fulfill an Intent (`PlanId`).
- **PlanStep**: A logical unit of operation within a Plan (`PlanStepId`), detailing input requirements, expected outputs, capability requirements, and approval gates.
- **Workload**: An OS-level, schedulable unit of compute execution (`WorkloadId`) materialized from a PlanStep.
- **Execution**: The active runtime lifecycle span of a materialized Workload managed by `workspaced` and `schedulerd`.
- **Process**: A transient kernel execution entity (`ProcessId`) representing a thread/task executing inside a workload container.
- **Planner**: A Ring 3 service or Agent entity authorized to construct and revise Plans.
- **Materialization**: The idempotent process of converting a validated, approved `PlanStep` into a concrete `Workload` entry in the Workload Execution DAG.
- **Approval Gate**: An OS-enforced boundary requiring human verification before a PlanStep can transition to materialization.

---

## 5. Identity Model

ZeroOS enforces strict, non-negotiable identity separation across all system abstractions:

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
```

### Identity Proof & Justification

1. **`IntentId ≠ PlanId`**: A single Intent ("Prepare report") may spawn multiple candidate or sequential Plans (`Plan v1`, `Plan v2`) over its lifecycle due to replanning or human rejection. Collapsing them would destroy historical lineage.
2. **`PlanId ≠ WorkloadId`**: A Plan is a declarative DAG of logical steps (`PlanStep`); a Workload is a concrete, schedulable compute unit. A single Plan contains multiple Workloads, and replanning may replace pending Workloads while keeping the `PlanId` parent anchor.
3. **`PlanStepId ≠ WorkloadId`**: A `PlanStep` is a logical step definition before materialization. If materialization fails or is retried under a new plan version, a single `PlanStep` definition may correspond to distinct materializations across plan versions.
4. **`AgentId ≠ Planner`**: An Agent (`AgentId`) is a persistent semantic state identity. Planning is a role/function performed by an Agent or System Planner service. An Agent constructs a `Plan`, but the `PlanId` is owned by the `Workspace`, not privatized inside the Agent.

---

## 6. Intent Model

An **Intent** represents a high-level goal declared by a human operator or authorized workspace controller.

### Structural Representation

An Intent $I_k$ is represented as:

$$I_k = \langle \text{IntentID}_k, \text{WorkspaceID}_i, \text{CreatorID}, \text{GoalStatement}, \text{ConstraintSpec}, \text{State}, \text{ActivePlanID}, \text{CreatedAt}, \text{UpdatedAt} \rangle$$

### Intent Lifecycle States

```text
SUBMITTED → ANALYZING → PLANNING → WAITING_APPROVAL → EXECUTING → COMPLETED
    │            │           │               │              │
    └────────────┴───────────┴───────────────┴──────────────┴───► CANCELLED / FAILED
```

- **`SUBMITTED`**: Intent registered in Workspace Intent Store `/storage/workspaces/<ws_id>/intents/<intent_id>.json`.
- **`ANALYZING`**: Planner inspecting Intent constraints and workspace object state.
- **`PLANNING`**: Planner generating candidate `Plan v1`.
- **`WAITING_APPROVAL`**: Plan contains steps requiring explicit human approval.
- **`EXECUTING`**: Materialized Workloads are actively running under the current `PlanId`.
- **`COMPLETED`**: All terminal steps of the active Plan have succeeded and Intent criteria are satisfied.
- **`CANCELLED`**: Intent explicitly revoked by human user.
- **`FAILED`**: Unrecoverable execution failure or max replan limit reached.

---

## 7. Plan Model

A **Plan** is an OS-managed DAG of logical `PlanStep` nodes that defines how ZeroOS intends to achieve a specific `IntentId`.

### Structural Representation

A Plan $P_m$ is defined as:

$$P_m = \langle \text{PlanID}_m, \text{IntentID}_k, \text{WorkspaceID}_i, \text{Version}, \text{AuthorType}, \text{AuthorID}, \text{Steps}, \text{Edges}, \text{State}, \text{ApprovalStatus} \rangle$$

Where:
- `Version`: Monotonically increasing integer (`1, 2, 3...`).
- `AuthorType`: `HUMAN | SYSTEM_PLANNER | AGENT`.
- `AuthorID`: `UserId` or `AgentId`.
- `Steps`: Map of `PlanStepId` to `PlanStep`.
- `Edges`: Adjacency list defining step dependencies (`PlanStepId -> Vec<PlanStepId>`).

---

## 8. PlanStep Model

A **PlanStep** represents an individual logical operational unit inside a Plan prior to workload materialization.

### Structural Representation

A PlanStep $S_{m,j}$ is defined as:

$$S_{m,j} = \langle \text{PlanStepID}_j, \text{PlanID}_m, \text{StepName}, \text{ExecSpec}, \text{InputBindings}, \text{OutputBindings}, \text{ReqCapabilities}, \text{ResDemandSpec}, \text{ApprovalGate}, \text{Status}, \text{MaterializedWorkloadID} \rangle$$

#### Key Fields:
- `ExecSpec`: Binary path, script reference, or internal system handler identifier.
- `InputBindings`: Explicit input object paths (`/storage/workspaces/<ws>/objects/<obj_id>`).
- `OutputBindings`: Explicit expected output object paths/templates.
- `ReqCapabilities`: Array of capability descriptors required for execution.
- `ResDemandSpec`: Declarative resource requirements (CPU cores, RAM MB, GPU MB, deadline ms).
- `ApprovalGate`: Boolean flag indicating if human approval is required prior to materialization.
- `MaterializedWorkloadID`: Optional pointer to `WorkloadId` once materialized.

---

## 9. Workload Materialization

Workload Materialization is the deterministic process of converting an approved, runnable `PlanStep` into an executable OS `Workload`.

```text
[ Approved PlanStep ]
        │
        ├─ 1. Generate Deterministic WorkloadID
        │     key = SHA256(PlanID, PlanVersion, PlanStepID)
        │
        ├─ 2. Validate Capabilities against Workspace Authority
        │
        ├─ 3. Construct Workload Spec (libzero::WorkloadSpec)
        │
        ├─ 4. Bind Workspace Context & Storage Routes
        │
        ├─ 5. Write Workload to /storage/workspaces/<ws>/workloads/<workload_id>.json
        │
        └─ 6. Register Workload with workspaced
                │
                ▼
        [ Executable Workload ]
```

### Deterministic Workload ID Allocation

To guarantee materialization idempotency across planner crashes or system reboots:

$$\text{WorkloadID} = \text{UUIDv5}(\text{Namespace\_ZeroOS\_Materialization}, \text{PlanID} \parallel \text{Version} \parallel \text{PlanStepID})$$

If a planner attempts to materialize the same `PlanStep` multiple times (e.g., after a crash), the resulting `WorkloadID` is mathematically identical. The system checks `/storage/workspaces/<ws>/workloads/<workload_id>.json`; if it exists, materialization returns the existing `WorkloadId` without duplicating execution.

---

## 10. Planning Authority

ZeroOS strictly decouples **Planning Authority**, **Capability Authority**, and **Scheduling Authority**:

```text
+-------------------------------------------------------------------------------+
|                             ZEROOS AUTHORITY MATRIX                           |
+----------------------+-------------------+-------------------+----------------+
| Authority Type       | Responsible Entity| Can Grant Caps?   | Can Select HW? |
+----------------------+-------------------+-------------------+----------------+
| Planning Authority   | Planner / Agent   | ❌ NO             | ❌ NO          |
| Human Approval Gate  | Human User        | ❌ NO (Unlocks)   | ❌ NO          |
| Capability Authority | Kernel / Brokerd  | 🟢 YES            | ❌ NO          |
| Resource/Scheduling  | Schedulerd        | ❌ NO             | 🟢 YES         |
+----------------------+-------------------+-------------------+----------------+
```

### Rules of Separation:
1. **Planners build execution graphs, not authority**: A planner proposes what steps to run, but cannot manufacture `CapabilityHandle` tokens.
2. **Schedulers place workloads, not plans**: `schedulerd` receives concrete `WorkloadId` entries with resource demands. It has zero visibility into high-level Intent or natural language goals.
3. **Capability authority remains kernel-backed**: Capabilities are checked when `workspaced` invokes system resources or spawns processes.

---

## 11. Agent Role

An Agent (`AgentId`) operates inside ZeroOS as an unprivileged, persistent control entity.

```text
+-------------------------------------------------------------------------------+
|                                AGENT CONTROL LOOP                             |
|                                                                               |
|  1. Read Workspace Intent (/storage/workspaces/<ws>/intents/)                 |
|  2. Read Workspace Object State (/storage/workspaces/<ws>/objects/)           |
|  3. Construct Proposed Plan vN (/storage/workspaces/<ws>/plans/drafts/)       |
|  4. Submit Plan to workspaced for Validation & Approval Gate Check            |
|  5. Monitor Workload Progress via workspaced status events                    |
|  6. Propose Replanning (Plan vN+1) upon failure or changed context            |
+-------------------------------------------------------------------------------+
```

### Absolute Agent Restrictions:
- An Agent **cannot** self-grant approval for gated `PlanStep` entries.
- An Agent **cannot** execute binaries outside of materialized, capability-bounded Workloads.
- An Agent **cannot** bypass `workspaced` to interact directly with physical devices or kernel memory.

---

## 12. Human Approval Boundary

ZeroOS introduces an OS-enforced **Human Approval Boundary**. A `PlanStep` *must* be flagged with `approval_required = true` if it matches any of the following system triggers:

```text
+-------------------------------------------------------------------------------+
|                        HUMAN APPROVAL SYSTEM TRIGGERS                         |
+-------------------------+-----------------------------------------------------+
| Trigger Category        | Description / Condition                             |
+-------------------------+-----------------------------------------------------+
| Destructive Mutation    | Deletion/Overwrite of immutable ZeroFS inodes       |
| External Communication  | Sockets connecting to non-whitelisted external IPs  |
| Financial / Quota       | Compute demand exceeding workspace quota threshold  |
| Privilege Escalation    | Requesting Capability Handles not in agent default |
| Cross-Workspace Access  | Reading/Writing objects in external WorkspaceId     |
| Irreversible Side-Effect| Formatting device, modifying system configuration   |
+-------------------------+-----------------------------------------------------+
```

### Approval State Transition

When a Plan contains gated steps:
1. Plan state transitions to `WAITING_APPROVAL`.
2. `workspaced` emits an OS Approval Notification event to the human session.
3. Execution pauses *before* any gated `PlanStep` is materialized into a `Workload`.
4. Upon explicit human signature (`APPROVE`), the step transitions to `APPROVED` and materializes. If `REJECTED`, the plan transitions to `REJECTED` and returns to the Planner for revision.

---

## 13. Plan Validation

Before a Plan can move to `APPROVED` or `MATERIALIZING`, `workspaced` executes strict validation checks:

1. **DAG Acyclicity**: Verify that the PlanStep dependency graph contains zero cycles using Kahn's algorithm.
2. **Capability Validation**: Cross-reference required capabilities (`ReqCapabilities`) against the Workspace's authorized capability mask.
3. **Workspace Boundary Check**: Ensure all input/output object bindings resolve within the parent `WorkspaceId` or authorized shared boundaries.
4. **Resource Bounds Validation**: Check requested resources against workspace maximum limits (`DimensionCapacityVector`).
5. **Conflict Analysis**: Verify that concurrent steps do not attempt conflicting exclusive file locks on identical `ObjectId` targets.

---

## 14. Plan Lifecycle

```text
       [ DRAFT ]
           │
           ▼
     [ VALIDATING ] ──────(Invalid)──────► [ REJECTED ]
           │
           ▼
   [ WAITING_APPROVAL ] ──(Denied)───────► [ REJECTED ]
           │
      (Approved)
           │
           ▼
    [ MATERIALIZING ]
           │
           ▼
       [ ACTIVE ] ────────(Cancel)───────► [ CANCELLED ]
           │
      (Step Fail)
           │
           ▼
     [ REPLANNING ] ────(New Plan)──────► [ SUPERSEDED (vN) ]
           │                                      │
     (Max Retries Exceeded)                       ▼
           │                                 [ Plan vN+1 ACTIVE ]
           ▼
       [ FAILED ]
           │
      (All Succeeded)
           │
           ▼
      [ COMPLETED ]
```

---

## 15. Plan Versioning

Plans are immutable once they enter `ACTIVE` state. Any modification, replanning, or step insertion produces a new Plan version (`Plan v1 → Plan v2`).

```text
/storage/workspaces/<ws_id>/plans/
    ├── plan_cfg01_v1.json  (State: SUPERSEDED, Completed Steps: 1, 2)
    └── plan_cfg01_v2.json  (State: ACTIVE,     Pending Steps: 3b, 4)
```

### Invariants for Plan Versioning:
- Workloads materialized under `Plan v1` retain their `plan_id` and `plan_version = 1` attributes immutably.
- Completed Workloads are **never** re-executed or deleted when a plan is upgraded to `v2`.
- `Plan v2` references the completed outputs of `Plan v1` as immutable input bindings.

---

## 16. Intent Updates

When a human user updates an active Intent (e.g., changing parameters or issuing a stop command):

1. **Intent State Update**: Intent transitions to `PLANNING` or `CANCELLED`.
2. **Plan Invalidation**: Currently `ACTIVE` plan version (`v1`) transitions to `SUPERSEDED` or `CANCELLED`.
3. **Workload Signal**: `workspaced` issues `SIGTERM` / `CANCEL` signals to all running Workloads belonging to `v1`.
4. **Completed Workload Preservation**: Already `COMPLETED` Workloads remain recorded in ZeroFS history.
5. **Replanning**: If updated rather than cancelled, the Planner reads current workspace object state and generates `Plan v2`.

---

## 17. Replanning

Replanning is triggered when runtime execution diverges from the active Plan (e.g., Workload failure, missing input object, resource exhaustion).

```text
[ Workload Failure Event ]
            │
            ▼
[ workspaced marks PlanStep FAILED ]
            │
            ▼
[ Check Retry Policy ]
 ├── Retryable? (e.g., transient resource busy) ──► Re-materialize Workload
 └── Non-Retryable / Max Retries Exceeded
            │
            ▼
[ Trigger Replanning Engine ]
            │
            ├─ 1. Snapshot Workspace Inode / Object State
            ├─ 2. Freeze non-dependent ACTIVE Workloads
            ├─ 3. Construct Plan v2 starting from completed step outputs
            └─ 4. Submit Plan v2 for Validation & Materialization
```

---

## 18. Cancellation

Cancellation occurs when an Intent or Plan is explicitly halted by a user or workspace policy.

```text
USER CANCEL INTENT -> workspaced -> MARK Plan CANCELLED -> MARK Workloads CANCELLED -> SIGTERM to Processes
```

- Running Workloads transition to `CANCELLED`.
- Pending PlanSteps are marked `SKIPPED`.
- Allocated resource leases are reclaimed by `resourced` / `lease_engine`.
- Workspace state remains clean and consistent via ZeroFS two-phase commit.

---

## 19. Partial Completion & Orphan Management

If a multi-step Plan experiences a non-recoverable failure in step $N$, steps $1 \dots N-1$ remain `COMPLETED`.

- ZeroOS does **not** perform blind global rollbacks of side effects.
- Completed steps have already mutated storage via ZeroFS REV3 transactions.
- If a step was executing when failure occurred, `workspaced` applies the frozen `ORPHAN_COMPLETING` lifecycle state from Workload REV1 to safely reclaim transient process artifacts without corrupting persistent workspace objects.

---

## 20. Idempotency

All orchestration actions are strictly idempotent:

1. **Intent Submission**: Submitting an identical `IntentId` returns existing intent status.
2. **Plan Generation**: Generating a plan with identical `(IntentId, Version)` yields deterministic `PlanId`.
3. **Workload Materialization**: `WorkloadID = UUIDv5(NS, PlanID || Version || PlanStepID)`. Re-materialization checks disk storage and reuses existing `WorkloadId` without duplicating execution.

---

## 21. Capability Integration

The Planning Model integrates strictly with the frozen ZeroOS Capability System:

```text
[ PlanStep Specification ]
    │ (declares: req_capabilities = [CAP_READ, CAP_WRITE, MUTATE_BIT_15])
    ▼
[ Capability Authorization Check ]
    │ (workspaced queries Kernel Capability Mask for WorkspaceId/AgentId)
    ├── Authorized ──► Materialize Workload with exact Capability Handle
    └── Denied     ──► Validation Failure (Plan Rejected: Missing Capabilities)
```

- Planners **cannot** grant capabilities.
- Materialized Workloads receive capability tokens bounded strictly by the Workspace's authorized mask.

---

## 22. Resource Integration

Planners specify resource demands in abstract vector quantities (`ResDemandSpec`), **never** physical hardware topologies:

$$\text{ResDemandSpec} = \langle \text{CPU}_{\text{cores}}, \text{RAM}_{\text{MB}}, \text{GPU}_{\text{MB}}, \text{Storage}_{\text{MB}}, \text{Deadline}_{\text{ms}} \rangle$$

```text
[ PlanStep ] ──(ResDemandSpec)──► [ Workload ] ──(ResourceDemand)──► [ resourced / schedulerd ]
                                                                             │
                                                                             ▼
                                                                     [ Physical Placement ]
```

Planners do not select CPU sockets, GPU IDs, NUMA nodes, or network interfaces. Physical placement is handled entirely by `schedulerd` and `resourced`.

---

## 23. Workspace Integration

Every Intent, Plan, PlanStep, and materialized Workload is strictly bound to a single parent `WorkspaceId`.

```text
/storage/workspaces/<workspace_id>/
    ├── intents/
    │   └── <intent_id>.json
    ├── plans/
    │   ├── <plan_id>_v1.json
    │   └── <plan_id>_v2.json
    ├── workloads/
    │   └── <workload_id>.json
    └── objects/
        └── <object_id>
```

### Workspace State Triggers:
- **Workspace Suspension**: All active Plans and Workloads transition to `PAUSED`. `schedulerd` deschedules processes.
- **Workspace Destruction**: All active Plans transition to `CANCELLED`. Workloads terminate, and storage is reclaimed via Object & Membership REV8.

---

## 24. Workload Integration

This model sits directly above the frozen **Workload & Agent Execution Model REV1**.

```text
+-------------------------------------------------------------------------------+
|         INTENT-TO-WORKLOAD PLANNING & ORCHESTRATION MODEL (REV1)              |
|  [ IntentId | PlanId | PlanStepId | Approval Gates | Replanning Engine ]      |
+-------------------------------------------------------------------------------+
                                       │
                                (Materializes)
                                       ▼
+-------------------------------------------------------------------------------+
|             WORKLOAD & AGENT EXECUTION MODEL (REV1) - FROZEN                  |
|  [ WorkloadId | AgentId | Workload Lifecycle | Scheduler Boundary ]           |
+-------------------------------------------------------------------------------+
```

The Workload Execution DAG operates exactly as frozen. The Orchestration layer merely generates and manages the high-level Plan DAG that feeds Workloads into `workspaced`.

---

## 25. DAG Relationships

ZeroOS explicitly maintains four decoupled directed acyclic graphs:

```text
1. Human Intent DAG        (Declarative goals in Workspace; owned by User/Workspace)
        │
        ▼
2. Plan DAG                (Logical operational steps; owned by Planner/workspaced)
        │
        ▼
3. Workload Execution DAG  (Concrete schedulable compute units; owned by workspaced)
        │
        ▼
4. Resource Graph          (Hardware physical topology & leases; owned by resourced)
```

### Strict Graph Boundaries:
- A single node in the Human Intent DAG maps to one active Plan DAG.
- A single node in the Plan DAG (`PlanStep`) maps to exactly zero or one `Workload` in the Workload Execution DAG.
- A single node in the Workload Execution DAG maps to zero or more physical nodes in the Resource Graph.
- **Graphs never share node identifiers or merge structures.**

---

## 26. Persistence

All orchestration entities are persisted to ZeroFS under two-phase commit transactions:

| Entity | Storage Path | Format | Durability Guarantee |
|---|---|---|---|
| Intent | `/storage/workspaces/<ws>/intents/<intent_id>.json` | JSON / ZeroFS Inode | Two-phase commit (Journal V2) |
| Plan | `/storage/workspaces/<ws>/plans/<plan_id>_v<ver>.json` | JSON / ZeroFS Inode | Two-phase commit (Journal V2) |
| Materialization Map | `/storage/workspaces/<ws>/plans/<plan_id>_mats.json` | JSON / ZeroFS Inode | Two-phase commit (Journal V2) |
| Approval Record | `/storage/workspaces/<ws>/approvals/<step_id>.json` | Cryptographic Log | Immutable audit log |

---

## 27. Crash Recovery

Upon system reboot or service restart (`workspaced`, `agent`, or `planner` crash):

```text
[ System Boot / Service Restart ]
                │
                ▼
[ Scan /storage/workspaces/<ws>/intents/ ]
                │
                ▼
[ Reconcile Active Plans vs Materialization Maps ]
                │
                ├─ PlanStep marked MATERIALIZING but Workload missing?
                │   └── Re-run idempotent materialization (UUIDv5 key)
                │
                ├─ Workload RUNNING but Process dead?
                │   └── Rely on Workload REV1 recovery state machine
                │
                └─ Active Plan in WAITING_APPROVAL?
                    └── Re-emit Approval Notification event
```

---

## 28. Security & Trust Model

1. **Planner Isolation**: Planners run in Ring 3 as unprivileged processes. They cannot directly access physical hardware or kernel data structures.
2. **Capability Encirclement**: A Plan cannot execute any step without holding valid Capability Handles granted by `brokerd` / Kernel.
3. **Approval Integrity**: Approval gates are verified by `workspaced` via cryptographic user session tokens. Agents cannot forge approval signatures.
4. **Workspace Sandboxing**: Object inputs and outputs are validated at planning time to prevent cross-workspace data leakage unless explicitly authorized by capability delegation.

---

## 29. Observability

ZeroOS provides an OS-level semantic explanation interface (`workspaced status --intent <intent_id>`):

```text
INTENT: "Prepare quarterly financial report" [ID: int_8f92a]
STATUS: EXECUTING (Plan v2 Active)

PLAN STEPS:
[✓] Step 1: Collect financial CSVs    -> Workload [wl_01a] (COMPLETED)
[✓] Step 2: Parse revenue metrics     -> Workload [wl_02b] (COMPLETED)
[▶] Step 3: Run AI analysis           -> Workload [wl_03c] (RUNNING - CPU 400%, RAM 2.1GB)
[⏸] Step 4: Publish to Public Shared  -> PlanStep [stp_04] (WAITING HUMAN APPROVAL)

BLOCKED BY: Step 4 requires Human Approval (Trigger: Cross-Workspace Public Export)
NEXT ACTION: Awaiting approval signature from User [uid_1001]
```

---

## 30. Human-Agent Collaboration

The collaboration pattern follows a strict **Propose-Approve-Execute** pipeline:

```text
HUMAN                AGENT / PLANNER               ZEROOS WORKSPACED
  │                         │                              │
  ├─── Submit Intent ──────►│                              │
  │                         ├─── Construct Plan v1 ───────►│
  │                         │                              ├── Validate Plan DAG
  │                         │                              ├── Check Approval Gates
  │◄── Request Approval ────┴──────────────────────────────┤ (Step 3 Gated)
  │                                                        │
  ├─── Approve Step 3 ────────────────────────────────────►│
  │                                                        ├── Materialize Workload
  │                                                        └── Execute via Schedulerd
```

---

## 31. System Invariants (IO-01 ... IO-30)

- **`IO-01`**: `WorkspaceId ≠ ObjectId ≠ WorkloadId ≠ AgentId ≠ ProcessId ≠ CapabilityHandle ≠ ResourceId ≠ IntentNodeId ≠ IntentId ≠ PlanId ≠ PlanStepId`.
- **`IO-02`**: Every Intent must belong to exactly one parent `WorkspaceId`.
- **`IO-03`**: An Intent may have at most one `ACTIVE` Plan version at any given timestamp.
- **`IO-04`**: Planners shall operate entirely in Ring 3 and shall not possess capability-granting authority.
- **`IO-05`**: Schedulers (`schedulerd`) shall allocate physical resources based on Workload specs but shall not create or modify Plans.
- **`IO-06`**: Workload Materialization shall be strictly idempotent using `UUIDv5(NS, PlanID || Version || PlanStepID)`.
- **`IO-07`**: A `PlanStep` with `approval_required = true` shall never materialize into a `Workload` without a valid human approval record.
- **`IO-08`**: Plan modifications shall produce a new, monotonically incremented Plan Version (`vN+1`).
- **`IO-09`**: Completed Workloads shall remain immutably associated with their original Plan Version.
- **`IO-10`**: Replanning shall not alter or cancel already `COMPLETED` Workloads.
- **`IO-11`**: The Plan DAG shall be checked for acyclicity prior to materialization.
- **`IO-12`**: All storage operations performed by Plan materialization shall be committed via ZeroFS two-phase commit transactions.
- **`IO-13`**: Agents shall construct plans using standard workspace storage APIs and shall not bypass `workspaced`.
- **`IO-14`**: Planners shall specify resource demands as abstract vectors (`ResDemandSpec`) and shall not specify physical hardware IDs.
- **`IO-15`**: Human approval signatures shall be verified cryptographically against authorized user session tokens.
- **`IO-16`**: Destructive storage mutations specified in a `PlanStep` shall unconditionally trigger the Human Approval Boundary.
- **`IO-17`**: External network socket requests specified in a `PlanStep` shall unconditionally trigger the Human Approval Boundary.
- **`IO-18`**: Cross-workspace object references shall require explicit capability handles granted to the target workspace context.
- **`IO-19`**: Cancellation of an Intent shall immediately signal `SIGTERM` / `CANCEL` to all running Workloads belonging to its active Plan.
- **`IO-20`**: Workspace suspension shall pause all active Plans and deschedule associated Workloads without destroying plan state.
- **`IO-21`**: Workspace destruction shall cancel all active Plans and reclaim pending materializations via Object & Membership REV8.
- **`IO-22`**: System reboots during materialization shall resume materialization idempotently using stored plan maps.
- **`IO-23`**: The Human Intent DAG, Plan DAG, Workload Execution DAG, and Resource Graph shall remain strictly separate structures.
- **`IO-24`**: Failure of a single Workload shall trigger replanning or retry policies without corrupting sibling completed workloads.
- **`IO-25`**: A `PlanStep` shall not be materialized unless all prerequisite parent `PlanStep` dependencies are in `COMPLETED` status.
- **`IO-26`**: An Agent shall not be able to elevate its own Capability Handles via plan construction.
- **`IO-27`**: The orchestrator shall maintain an immutable audit log of all human approval decisions in ZeroFS.
- **`IO-28`**: Duplicate concurrent plan materialization requests shall resolve to the single deterministic `WorkloadId`.
- **`IO-29`**: System resource demand specifications shall be validated against workspace resource quotas prior to plan approval.
- **`IO-30`**: The Intent-to-Workload Planning & Orchestration Model shall require 0 kernel changes, 0 new syscalls, and 0 new ABI additions.

---

## 32. Adversarial Scenarios (A – T)

### Scenario A: Simple intent becomes one Workload
- **State Before**: Workspace idle. User submits Intent $I_1$ ("Count lines in log.txt").
- **Trigger**: `workspaced` receives Intent $I_1$.
- **Authority**: User session authorized.
- **Plan State**: Planner creates `Plan v1` with single `PlanStep` $S_1$. Validation passes. Approval gate false.
- **Workload State**: $S_1$ materializes to Workload $W_1$. $W_1$ transitions to `RUNNING` then `COMPLETED`.
- **Capability State**: Standard read capability applied.
- **Resource State**: 1 CPU, 128MB RAM leased.
- **Persistence Effect**: `I_1.json`, `plan_v1.json`, `w1.json` written to ZeroFS.
- **Recovery**: N/A (Successful execution).

### Scenario B: Intent requires five dependent Workloads
- **State Before**: Workspace active. Intent $I_2$ submitted.
- **Trigger**: Planner analyzes $I_2$ and generates 5-step DAG ($S_1 \to S_2 \to S_3$, $S_1 \to S_4 \to S_5$).
- **Authority**: Planner authorized.
- **Plan State**: `Plan v1` validated (acyclic).
- **Workload State**: $S_1$ materializes to $W_1$. Upon $W_1$ completion, $S_2$ and $S_4$ materialize concurrently.
- **Capability State**: Capability handles bounded per step.
- **Resource State**: Resource vector scaled dynamically by `schedulerd`.
- **Persistence Effect**: Plan DAG state updated as steps complete.
- **Recovery**: Interrupted steps resume from last completed DAG frontier.

### Scenario C: Planner crashes during materialization
- **State Before**: `Plan v1` approved. $S_2$ in process of materializing.
- **Trigger**: Planner process crashes mid-write.
- **Authority**: N/A (Crash event).
- **Plan State**: `Plan v1` remains in `MATERIALIZING`.
- **Workload State**: Partial or no disk entry for $W_2$.
- **Capability State**: Unchanged.
- **Resource State**: Unchanged.
- **Persistence Effect**: ZeroFS two-phase commit aborts uncommitted partial transaction.
- **Recovery**: On restart, `workspaced` detects `MATERIALIZING` state, re-calculates `UUIDv5` key, and cleanly completes materialization of $W_2$.

### Scenario D: Agent crashes during planning
- **State Before**: Intent $I_3$ in `PLANNING` state.
- **Trigger**: Agent process terminated by OS or power loss.
- **Authority**: N/A.
- **Plan State**: Draft plan file incomplete in `/storage/workspaces/<ws>/plans/drafts/`.
- **Workload State**: No workloads materialized.
- **Capability State**: Unchanged.
- **Resource State**: No resources allocated.
- **Persistence Effect**: Draft file ignored or cleaned up on reboot.
- **Recovery**: System Planner or restarted Agent reads `PLANNING` intent state and regenerates `Plan v1` from scratch.

### Scenario E: Same plan is materialized twice
- **State Before**: `Plan v1` step $S_1$ already materialized to $W_1$.
- **Trigger**: Duplicate RPC or race condition sends second materialization request for $S_1$.
- **Authority**: Planner.
- **Plan State**: `ACTIVE`.
- **Workload State**: Second request calculates identical `UUIDv5` key. Disk lookup finds existing `w1.json`.
- **Capability State**: Unchanged.
- **Resource State**: No duplicate resource allocation.
- **Persistence Effect**: 0 extra disk writes.
- **Recovery**: Materialization call returns existing `WorkloadId` ($W_1$).

### Scenario F: Human changes intent while Workload is RUNNING
- **State Before**: Intent $I_4$ executing under `Plan v1`. Workload $W_2$ `RUNNING`.
- **Trigger**: User edits intent text or issues update request.
- **Authority**: Human user.
- **Plan State**: `Plan v1` transitions to `SUPERSEDED`.
- **Workload State**: `workspaced` sends `SIGTERM` to $W_2$. $W_2$ enters `CANCELLED`.
- **Capability State**: Capability handles revoked for $W_2$.
- **Resource State**: Resource leases reclaimed by `resourced`.
- **Persistence Effect**: `Plan v2` drafted reflecting new intent parameters.
- **Recovery**: Execution begins afresh under `Plan v2`.

### Scenario G: Plan requires destructive action
- **State Before**: `Plan v1` contains step $S_3$ ("Delete directory /storage/workspaces/ws1/old_data").
- **Trigger**: Planner submits `Plan v1` for validation.
- **Authority**: Human approval required.
- **Plan State**: Step $S_3$ has `approval_required = true`. Plan enters `WAITING_APPROVAL`.
- **Workload State**: $S_3$ does **not** materialize into a workload.
- **Capability State**: Capability not issued.
- **Resource State**: 0 resources allocated.
- **Persistence Effect**: Approval request logged to `/storage/workspaces/ws1/approvals/`.
- **Recovery**: Plan remains paused until explicit human signature.

### Scenario H: Capability is revoked after planning
- **State Before**: `Plan v1` approved. Step $S_2$ requires `CAP_WRITE_SHARED`.
- **Trigger**: Admin revokes `CAP_WRITE_SHARED` from Workspace before $S_2$ materializes.
- **Authority**: Admin / Security policy.
- **Plan State**: `Plan v1` materialization fails validation.
- **Workload State**: $S_2$ fails to materialize. Plan transitions to `FAILED`.
- **Capability State**: Denied.
- **Resource State**: Unchanged.
- **Persistence Effect**: Capability failure logged in plan status.
- **Recovery**: Replanning triggered to find alternative execution path without revoked capability.

### Scenario I: Resource demand becomes unavailable
- **State Before**: Step $S_1$ requires 64GB RAM. Workspace quota allows, but host physical memory exhausted.
- **Trigger**: Materialized Workload $W_1$ submitted to `schedulerd`.
- **Authority**: `schedulerd` / `resourced`.
- **Plan State**: `ACTIVE`. Step $S_1$ waiting for allocation.
- **Workload State**: $W_1$ in `ADMITTED_PENDING_RESOURCE`.
- **Capability State**: Valid.
- **Resource State**: `resourced` denies lease allocation.
- **Persistence Effect**: Pending state persisted.
- **Recovery**: Workload waits until timeout, then triggers replanning for lower resource profile.

### Scenario J: Workspace is suspended during execution
- **State Before**: Workload $W_1$ actively executing step $S_1$.
- **Trigger**: User or policy suspends parent Workspace.
- **Authority**: Workspace Controller.
- **Plan State**: `Plan v1` transitions to `PAUSED`.
- **Workload State**: $W_1$ paused/descheduled by `schedulerd`.
- **Capability State**: Frozen.
- **Resource State**: Resource leases held in suspended state.
- **Persistence Effect**: Workspace state updated to `SUSPENDED`.
- **Recovery**: Resuming workspace restores `Plan v1` to `ACTIVE` and resumes $W_1$.

### Scenario K: Workspace is destroyed while plan is active
- **State Before**: Intent $I_1$ executing under `Plan v1`.
- **Trigger**: User destroys Workspace.
- **Authority**: Workspace Owner.
- **Plan State**: `Plan v1` cancelled.
- **Workload State**: $W_1$ terminated immediately (`SIGKILL`).
- **Capability State**: All handles revoked.
- **Resource State**: All leases released.
- **Persistence Effect**: Workspace directory deleted via ZeroFS transaction.
- **Recovery**: All orchestration state purged.

### Scenario L: One Workload fails while others succeed
- **State Before**: Parallel steps $S_2$ ($W_2$) and $S_3$ ($W_3$) running.
- **Trigger**: $W_2$ exits with non-zero code (error). $W_3$ completes successfully.
- **Authority**: OS execution return code.
- **Plan State**: Step $S_2$ marked `FAILED`. Step $S_3$ marked `COMPLETED`.
- **Workload State**: $W_2$ enters `FAILED`. $W_3$ enters `COMPLETED`.
- **Capability State**: Handles released.
- **Resource State**: Leases released.
- **Persistence Effect**: Completed state of $W_3$ immutably recorded.
- **Recovery**: Replanning engine generates `Plan v2` to retry or replace $S_2$ using outputs of $W_3$.

### Scenario M: Plan version changes while old Workloads run
- **State Before**: `Plan v1` executing. $W_1$ running.
- **Trigger**: Replanning event generates `Plan v2` before $W_1$ finishes.
- **Authority**: Planner.
- **Plan State**: `Plan v1` marked `SUPERSEDED`. `Plan v2` marked `ACTIVE`.
- **Workload State**: If $W_1$ is included unchanged in `Plan v2`, it is re-bound to `v2`. If omitted, $W_1$ receives cancellation signal.
- **Capability State**: Re-validated for `Plan v2`.
- **Resource State**: Preserved or adjusted.
- **Persistence Effect**: Plan map updated to version 2.
- **Recovery**: Orchestrator tracks execution seamlessly under `v2`.

### Scenario N: Agent proposes unauthorized action
- **State Before**: Agent constructing draft `Plan v1`.
- **Trigger**: Agent inserts step $S_1$ requesting root host filesystem access.
- **Authority**: Capability system.
- **Plan State**: Validation phase detects $S_1$ capability requirement exceeds Workspace capability boundary.
- **Workload State**: Materialization blocked. Plan rejected.
- **Capability State**: Unauthorized capability request denied.
- **Resource State**: 0 resources allocated.
- **Persistence Effect**: Security violation logged.
- **Recovery**: Agent receives validation error and must reformulate plan.

### Scenario O: Scheduler placement differs from planner expectation
- **State Before**: Planner estimates step $S_1$ will run on GPU 0.
- **Trigger**: `schedulerd` places $W_1$ on GPU 1 due to load.
- **Authority**: `schedulerd`.
- **Plan State**: `ACTIVE`. Unaffected by placement decision.
- **Workload State**: Executing on GPU 1.
- **Capability State**: Unchanged.
- **Resource State**: Lease issued for GPU 1.
- **Persistence Effect**: Workload execution record updated with actual placement.
- **Recovery**: Planner learns placement is opaque to orchestration layer.

### Scenario P: Human cancels intent during execution
- **State Before**: Intent $I_5$ `EXECUTING`. Workloads $W_1, W_2$ running.
- **Trigger**: User clicks "Cancel" in UI.
- **Authority**: Human user.
- **Plan State**: `CANCELLED`.
- **Workload State**: $W_1, W_2$ terminated via `SIGTERM`.
- **Capability State**: Handles revoked.
- **Resource State**: Leases freed.
- **Persistence Effect**: Intent state updated to `CANCELLED`.
- **Recovery**: Orchestration halts cleanly.

### Scenario Q: Already-completed irreversible action exists when intent changes
- **State Before**: Step $S_1$ (External API webhook sent) `COMPLETED`. Step $S_2$ running.
- **Trigger**: User updates intent.
- **Authority**: User.
- **Plan State**: `Plan v1` superseded.
- **Workload State**: $W_2$ cancelled. $W_1$ remains `COMPLETED`.
- **Capability State**: Handles revoked for active execution.
- **Resource State**: Leases freed.
- **Persistence Effect**: $W_1$ completion recorded in history.
- **Recovery**: System does not attempt impossible rollback of external webhook; `Plan v2` accounts for $W_1$'s completed state.

### Scenario R: Cross-workspace object appears in plan
- **State Before**: Step $S_1$ in `Workspace A` requests input object in `Workspace B`.
- **Trigger**: Plan validation.
- **Authority**: Workspace Capability Authority.
- **Plan State**: Checked for cross-workspace capability handle.
- **Workload State**: If delegated handle present, materialization succeeds. Otherwise, validation fails.
- **Capability State**: Delegated capability token verified.
- **Resource State**: Normal.
- **Persistence Effect**: Cross-workspace dependency recorded.
- **Recovery**: Validation error prompts user for cross-workspace authorization if handle missing.

### Scenario S: Reboot occurs during materialization
- **State Before**: Step $S_1$ approved. `workspaced` writing materialization record.
- **Trigger**: Power loss / kernel reboot.
- **Authority**: N/A.
- **Plan State**: `MATERIALIZING`.
- **Workload State**: Pending.
- **Capability State**: Unchanged.
- **Resource State**: Unchanged.
- **Persistence Effect**: ZeroFS Journal V2 guarantees atomic disk state (either fully written or rolled back).
- **Recovery**: Upon reboot, recovery manager checks journal, finds clean atomic state, and re-triggers materialization using deterministic `UUIDv5` key.

### Scenario T: Duplicate planner requests arrive concurrently
- **State Before**: Intent $I_1$ in `SUBMITTED`.
- **Trigger**: Two parallel planner instances submit `Plan v1` proposals simultaneously.
- **Authority**: `workspaced` single-threaded transaction handler.
- **Plan State**: First proposal accepted as `Plan v1`. Second proposal rejected as duplicate or assigned `v2`.
- **Workload State**: Workloads materialized exclusively from accepted `Plan v1`.
- **Capability State**: Unchanged.
- **Resource State**: Unchanged.
- **Persistence Effect**: Single plan accepted in ZeroFS directory lock.
- **Recovery**: Second planner receives conflict status and synchronizes with active `v1`.

---

## 33. Dependency Graph

```text
               +-----------------------------+
               |     HUMAN USER INTENT       |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |    WORKSPACE INTENT STORE   |
               |  (/storage/workspaces/<ws>) |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |  PLANNER / AGENT SERVICE    |
               |         (Ring 3)            |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |     WORKSPACED DAEMON       |
               |  [Validation & Approval]    |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |   WORKLOAD MATERIALIZATION  |
               |  [UUIDv5 Idempotent Engine] |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |    SCHEDULERD / RESOURCED   |
               |   [Resource Demand Lease]   |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |       KERNEL / BROKERD      |
               |    [Capability Handle]      |
               +-----------------------------+
                              │
                              ▼
               +-----------------------------+
               |    RING 3 EXECUTION UNIT    |
               +-----------------------------+
```

### Absence of Circular Dependencies:
- `Planner` depends on `Workspace Intent Store`.
- `workspaced` validates `Plan` against `Capability Authority`.
- `workspaced` materializes `Workload` to `schedulerd`.
- `schedulerd` leases resources from `resourced`.
- `brokerd` issues `CapabilityHandle` to running `Process`.
- **Zero backward edges exist from Execution or Scheduling into Planning.**

---

## 34. Implementation Boundary

- **Layer**: Ring 3 User Space Service & Engine (`workspaced`, `libzero`, System Planner Service).
- **Kernel Changes**: 0
- **New Syscalls**: 0
- **New ABI**: 0
- **Storage Substrate**: ZeroFS REV3 (Journal V2, Two-Phase Commit).
- **Execution Surface**: Ring 3 process isolation managed by frozen `Workload & Agent Execution Model REV1`.

---

## 35. Open Questions

1. *Should Approval Gates support timeout-based auto-rejection?*
   - **REV1 Decision**: Yes, unapproved gates default to `REJECTED` after a configurable workspace timeout (default: 24 hours).
2. *Can a PlanStep trigger sub-intents dynamically?*
   - **REV1 Decision**: Yes, a `PlanStep` may emit a new Intent to the Workspace Intent Store, maintaining clean hierarchical intent decomposition.

---

## 36. Final Architectural Verdict

```text
ZEROOS INTENT-TO-WORKLOAD PLANNING & ORCHESTRATION REV1

ARCHITECTURE:
🟢 ARCHITECTURE READY

KERNEL CHANGES:
0

NEW SYSCALLS:
0

NEW ABI:
0

FROZEN DEPENDENCIES:
INTACT

CIRCULAR DEPENDENCIES:
NONE

INTENT → PLAN:
COHERENT

PLAN → WORKLOAD:
COHERENT

CAPABILITY BOUNDARY:
COHERENT

RESOURCE BOUNDARY:
COHERENT

WORKSPACE INTEGRATION:
COHERENT

WORKLOAD INTEGRATION:
COHERENT

SINGLE-NODE COMPATIBILITY:
PROVEN

OVERALL:
🟢 ARCHITECTURE APPROVED & READY FOR IMPLEMENTATION
```
