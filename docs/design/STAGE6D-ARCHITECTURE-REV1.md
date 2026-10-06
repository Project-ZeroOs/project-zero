# Stage 6D — Architecture Specification: Human Intent, Intent Resolution & Action/Workflow Boundary Subsystem (Rev1)

**Status:** 🟡 ARCHITECTURE REV1 DRAFT — PENDING REVIEW & FREEZE AUTHORIZATION  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen (`intentd`, `agentd`, `workloadd`, `fabricd`, `resourced`)  
- Stage 5 Presentation Subsystem — 🟢 Frozen (`compositord`, `surfaced`, `authui`)  
- Stage 6A User Session Substrate — 🟢 Frozen (`shelld`, `workspaced`)  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  
- Stage 6C Human Input & Interaction Routing — 🟢 Frozen & Committed (`uids`)  

---

## 1. Executive Summary

Stage 6D defines the **Human Intent, Intent Resolution & Action/Workflow Boundary Subsystem** for ZeroOS.

Building upon the Stage 6C input routing boundary:
```text
Physical / Remote / Accessibility Signal
                   │
                   ▼
                 uids (Input Observation & Focus Routing Authority)
            ┌──────┴────────┐
            ▼               ▼
    focused surface      intentd (Intent Interpretation Authority)
```

Stage 6D specifies how **`intentd`** ingests human expressions (natural language, command palette, global hotkeys, gestures, accessibility semantic actions) and agent proposals, converts them into validated structural plans, and connects seamlessly to the frozen Stage 4F pipeline without duplicating or modifying the `Intent → Plan → Agent → Workload` execution model.

```text
Human / Agent Expression
          │
          ▼
        uids (Stage 6C Observation & Hit-Testing)
          │
          ▼
       intentd (Stage 6D / 4F Interpretation & Plan Compilation)
          │
          ▼
     ExecutionPlan (Stage 4F Structural Plan)
          │
          ▼
        Agent (Stage 4E Agent Runtime Subsystem)
          │
          ▼
     Workload DAG (Stage 4E / 4F Workload Orchestration)
          │
          ▼
        Task (Stage 4E Task Executable Unit)
          │
          ▼
       Process (Stage 3 Microkernel Process Execution)
```

### Key Architectural Invariants & Boundaries:

1. **`I-INTENT-OFFLINE-AUTONOMY`**: Intent resolution, agent lifecycle, local workload execution, and local resource management do not require network connectivity. Network is treated strictly as a **workload-level resource dependency**. An intent whose validated execution plan carries an explicit network dependency (e.g., remote URL fetch, web search, or remote compute placement via `fabricd`) is deferred, degraded, or rejected when that resource is unavailable, without stalling or crashing local OS operations or local agent loops.
2. **`I-INTENT-MODEL-NON-AUTHORITY`**: AI/LLM model outputs (whether local or remote) are treated strictly as **untrusted candidate proposals**. Model outputs CANNOT execute system operations directly, CANNOT issue kernel syscalls, and CANNOT bypass capability checks. All model proposals are deterministically validated by `intentd` against the caller's Stage 3H capability table.
3. **Subordination to Trusted Authorization Service (`authui`)**: `intentd` is the **Intent Interpretation & Structural Plan Compiler Authority**, NOT an authorization authority. When `intentd` detects a Class 3 side-effect step (destructive write, credential access, external network write, broad execution), it requests confirmation through the pre-existing **Trusted Authorization Service (`authui` / `ModalLock`)**. `AuthorizationTransactionRef` remains protocol state verifying physical human consent, NOT a new capability or `intentd` authority.
4. **`I-INTENT-NO-CAPABILITY-AMPLIFICATION`**: `intentd` operates with zero intrinsic authority over user resources. `intentd` validates candidate plans strictly using the caller's pre-existing Stage 3H capability handles (`caller_cap_table`). `intentd` daemon privileges are never leveraged to bypass caller authorization (Confused Deputy Prevention).
5. **Zero Substrate Modification**: Stage 6D requires **0 lines of code modified** in the Stage 3A–3N production Ring-0 microkernel nucleus.

---

## 2. System Architecture & Subsystem Boundaries

```text
 ┌────────────────────────────────────────────────────────────────────────────────────────┐
 │ Stage 6C: Ingestion & Routing Substrate                                               │
 │                                                                                        │
 │ Physical / Remote / Accessibility Input ──> uids (Timestamp, Provenance, Route)         │
 └───────────────────────────────────────────┬────────────────────────────────────────────┘
                                             │
                                             ▼
 ┌────────────────────────────────────────────────────────────────────────────────────────┐
 │ Stage 6D: Intent Interpretation & Plan Compilation Subsystem                           │
 │                                                                                        │
 │  shelld Command Palette / Hotkeys ──────┐                                              │
 │                                         ▼                                              │
 │  Raw Intent Ingestion ─────────────> intentd ──> Deterministic Parser / Local Model   │
 │                                         │                                              │
 │  Agent Proposals (agentd) ──────────────┘                                              │
 │                                         │                                              │
 │                                         ▼                                              │
 │                             Capability Validation Engine                               │
 │                             (Check caller_cap_table)                                   │
 │                                         │                                              │
 │                                         ▼                                              │
 │                            Side-Effect Classification                                  │
 │                        ┌────────────────┴────────────────┐                             │
 │                        ▼                                 ▼                             │
 │                 Class 1 / Class 2                       Class 3                        │
 │                (Read / Scoped Mutate)           (Consequential Mutation)                   │
 │                        │                                 │                             │
 │                        │                                 ▼                             │
 │                        │                    Trusted Authorization Service              │
 │                        │                       (authui ModalLock UI)                   │
 │                        │                                 │                             │
 │                        └────────────────┬────────────────┘                             │
 │                                         │                                              │
 │                                         ▼                                              │
 │                                Stage 4F ExecutionPlan                                  │
 └─────────────────────────────────────────┬──────────────────────────────────────────────┘
                                           │
                                           ▼
 ┌────────────────────────────────────────────────────────────────────────────────────────┐
 │ Stage 4E / 4F: Execution & Compute Fabric Subsystem                                    │
 │                                                                                        │
 │  ExecutionPlan ──> agentd ──> workloadd (DAG) ──> resourced (Lease) / fabricd (Compute) │
 └────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Direct Integration with Frozen Stage 4F Pipeline

Stage 6D **does not duplicate or replace** Stage 4F. It formalizes the front-end human and agent interaction substrate that compiles raw expressions into Stage 4F `ExecutionPlan` structures.

### Stage 4F Handshake Contract:

```rust
// Reused frozen Stage 4F opcode (0x0709)
pub const OP_INTENT_SUBMIT: u64      = 0x0709;
pub const OP_INTENT_SUBMIT_RESP: u64 = 0x070A;

// ExecutionPlan (Stage 4F frozen ABI)
#[repr(C)]
pub struct ExecutionPlan {
    pub plan_id: DistributedId,
    pub session_id: DistributedId,
    pub workspace_id: DistributedId,
    pub initiator_agent_id: DistributedId,
    pub step_count: u32,
    pub side_effect_class: u8,
    pub is_network_dependent: u8,
    pub _reserved: [u8; 2],
    pub steps: [PlanStep; MAX_PLAN_STEPS],
}
```

1. **Submission**: `shelld`, application, or `agentd` sends `OP_INTENT_SUBMIT` with expression payload to `intentd`.
2. **Compilation**: `intentd` compiles the expression into an `ExecutionPlan`.
3. **Capability Check**: `intentd` inspects `steps[i].required_capability`. If caller lacks the capability in `caller_cap_table`, return `ZeroError::PermissionDenied`.
4. **Side-Effect Check**: If `side_effect_class == 3`, trigger `authui` `ModalLock` confirmation.
5. **Execution Handoff**: `intentd` submits the compiled `ExecutionPlan` to `workloadd`/`agentd` via `OP_WORKLOAD_CREATE` (`0x04E1`).

---

## 4. Subordination of Side-Effect Confirmation to Existing Authorization Architecture

`intentd` is **NOT** an authorization authority. When a Class 3 side-effect operation is compiled into a plan step, `intentd` delegates confirmation to the pre-existing **Trusted Authorization Service (`authui`)**:

```text
       intentd                                 authui                             uids
 (Plan Compiler)                     (Trusted Auth Service)                (Focus Enforcer)
        │                                        │                                │
        │── 1. Request Confirmation (Plan) ─────>│                                │
        │                                        │── 2. Request ModalLock ───────>│
        │                                        │                                │ (Quarantine Input)
        │                                        │<── 3. ModalLock Granted ───────│
        │                                        │
        │                                [Display Isolated]
        │                                [Authorization UI]
        │                                        │
        │                                [Human Physical]
        │                                [ Confirmation ]
        │                                        │
        │<── 4. AuthorizationTransactionRef ─────│
        │                                        │── 5. Release ModalLock ───────>│
        │                                                                         │ (Restore Focus)
```

- `AuthorizationTransactionRef` is a 16-byte `DistributedId` proving physical human consent for the specific `plan_id` payload hash.
- It is **protocol state**, NOT a new capability.
- If `authui` crashes or the user dismisses the dialog, `ModalLock` is invalidated and `intentd` cancels the plan (`I-INTENT-FAIL-CLOSED`).

---

## 5. Invariants

| Invariant | Definition |
|---|---|
| **`I-INTENT-OFFLINE-AUTONOMY`** | Intent resolution, agent lifecycle, local workload execution, and local resource management do not require network connectivity. An intent whose validated execution plan has an explicit network dependency may be deferred, degraded, or rejected when that resource is unavailable. |
| **`I-INTENT-MODEL-NON-AUTHORITY`** | Model/parser outputs are untrusted proposals and can never grant or expand system authority. |
| **`I-INTENT-NO-CAPABILITY-AMPLIFICATION`** | `intentd` must validate plan steps strictly against caller capability handles; `intentd` daemon privileges shall never be leveraged to bypass caller checks. |
| **`I-INTENT-WORKSPACE-CONTAINMENT`** | All intents, plans, workloads, and task artifacts are strictly bound to their originating `workspace_id`. |
| **`I-INTENT-CONFUSED-DEPUTY-PREVENTION`** | Unprivileged processes cannot use natural language or intent IPC to execute actions exceeding their capability set. |
| **`I-INTENT-TRUSTED-SIDE-EFFECT-CONFIRMATION`** | Class 3 side-effect actions mandate physical human confirmation via the Trusted Authorization Service (`authui` `ModalLock`). |
| **`I-INTENT-FAIL-CLOSED`** | Crashes in `authui`, `shelld`, or `intentd` during intent resolution must invalidate pending transaction locks and cancel unconfirmed intents. |
| **`I-INTENT-SOURCE-PROVENANCE-PRESERVATION`** | Provenance assigned by `uids` must be preserved end-to-end through `HumanIntent` and `ExecutionPlan`. |

---

## 6. ABI & Data Struct Specifications

### 6.1 `HumanIntent` Representation (`libzero/src/intent.rs`)

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanIntentHeader {
    pub intent_id: DistributedId,        // 16 bytes: LE DistributedId
    pub session_id: DistributedId,       // 16 bytes: LE DistributedId
    pub workspace_id: DistributedId,     // 16 bytes: LE DistributedId
    pub agent_id: DistributedId,         // 16 bytes: LE DistributedId (0 if human)
    pub timestamp_tsc: u64,              // 8 bytes: Monotonic TSC ticks
    pub source_provenance: u16,          // 2 bytes: uids provenance enum
    pub intent_type: u16,                // 2 bytes: Hotkey, Palette, Gesture, Agent, Workflow
    pub side_effect_class: u8,           // 1 byte: 1=Read, 2=Scoped Mutate, 3=Consequential
    pub lifecycle_state: u8,             // 1 byte: Submitted, Parsed, Ambiguous, PendingAuth, Approved, Dispatched
    pub is_network_dependent: u8,        // 1 byte: 0=Local, 1=Network Dependent
    pub _reserved: u8,                   // 1 byte: Padding
} // Exactly 80 bytes

pub const INTENT_TYPE_ONE_SHOT: u16       = 0x0001;
pub const INTENT_TYPE_COMMAND_PALETTE: u16 = 0x0002;
pub const INTENT_TYPE_HOTKEY: u16          = 0x0003;
pub const INTENT_TYPE_GESTURE: u16         = 0x0004;
pub const INTENT_TYPE_ACCESSIBILITY: u16   = 0x0005;
pub const INTENT_TYPE_AGENT_PROPOSAL: u16  = 0x0006;
pub const INTENT_TYPE_WORKFLOW_TEMPLATE: u16 = 0x0007;

pub const INTENT_STATE_SUBMITTED: u8      = 0x01;
pub const INTENT_STATE_PARSED: u8         = 0x02;
pub const INTENT_STATE_AMBIGUOUS: u8      = 0x03;
pub const INTENT_STATE_PENDING_AUTH: u8   = 0x04;
pub const INTENT_STATE_APPROVED: u8       = 0x05;
pub const INTENT_STATE_DISPATCHED: u8     = 0x06;
pub const INTENT_STATE_COMPLETED: u8      = 0x07;
pub const INTENT_STATE_REJECTED: u8       = 0x08;
pub const INTENT_STATE_FAILED: u8         = 0x09;
```

---

## 7. Operational Pipeline Workflows

### 7.1 Natural Language / Command Palette Ingestion

1. Human opens command palette ($Ctrl+Space$) in `shelld`.
2. `shelld` collects raw text input stream.
3. `shelld` submits `OP_INTENT_SUBMIT` to `intentd` carrying text payload and caller session/workspace handles.
4. `intentd` invokes local deterministic parser or local small model runner.
5. `intentd` produces candidate `ExecutionPlan`.
6. `intentd` validates candidate plan steps against caller capabilities.
7. If valid, `intentd` dispatches plan to `workloadd`.

### 7.2 Disambiguation Workflow

```text
[Expression] ──> intentd ──> Candidate Matches > 1 ──> State: AMBIGUOUS
                                                            │
                                                            ▼
                                                     shelld Palette
                                            (Renders Candidate Options)
                                                            │
                                                            ▼
                                                 [Human Selects Option]
                                                            │
                                                            ▼
                                                     intentd Approved
```

- When expression yields ambiguity or confidence score $< 0.70$:
  - `intentd` sets state to `INTENT_STATE_AMBIGUOUS`.
  - `intentd` returns candidate choice list to `shelld`.
  - `shelld` renders choice menu to human.
  - Execution is BLOCKED until human selects candidate or cancels.

### 7.3 Class 3 Side-Effect Confirmation Workflow

1. `intentd` parses plan step requiring Class 3 side-effects (e.g. `OP_FS_REMOVE_RECURSIVE`).
2. `intentd` sets state to `INTENT_STATE_PENDING_AUTH`.
3. `intentd` invokes `authui` Trusted Authorization Service.
4. `authui` activates `ModalLock` via `uids` and renders modal showing target paths and actions.
5. Human confirms physically.
6. `authui` returns `AuthorizationTransactionRef`.
7. `intentd` sets state to `INTENT_STATE_APPROVED` and dispatches plan.

---

## 8. Offline Operation & Network Resource Dependency Model

```text
                                  [Input Intent]
                                        │
                                        ▼
                                 intentd Local
                             (Parser / Hotkey Table)
                                        │
                                        ▼
                                 ExecutionPlan
                                        │
                       ┌────────────────┴────────────────┐
                       ▼                                 ▼
            [Local Workload Step]            [Network-Dependent Step]
                       │                                 │
                       ▼                                 ├─ Network Online  ──> Executes via fabricd
             Executes 100% Offline                       │
             via agentd / workloadd                      └─ Network Offline ──> Deferred / Degraded /
                                                                                Rejected gracefully
```

- Local intent resolution, hotkey processing, agent lifecycle, workspace management, and local workload execution DO NOT depend on network.
- If a plan contains a step with an explicit network dependency (e.g., `fabricd` remote node placement or remote HTTP query):
  - When network is online $\rightarrow$ step completes normally.
  - When network is offline $\rightarrow$ step fails gracefully with `ZeroError::TimeAuthorityUnavailable` or `ZeroError::StaleEndpoint`.
- Local execution NEVER hangs or crashes due to network outage.

---

## 9. Threat Model & Security Boundaries

1. **Confused Deputy Attack**: Unprivileged process submits prompt "Delete /sys/config". `intentd` checks caller's Stage 3H capability table. Caller lacks `FileWriteCap` for `/sys/config` $\rightarrow$ `intentd` returns `ZeroError::PermissionDenied`.
2. **Model Escalation Attack**: Untrusted local/remote model injects `SystemSurfacePolicyCap` into candidate plan. `intentd` safety engine detects capability mismatch $\rightarrow$ proposal REJECTED.
3. **CSDT Authorization Bypass**: Remote peer submits intent claiming authority via CSDT token. `intentd` checks local `RemoteInputPolicyCap` and workspace authorization $\rightarrow$ rejected if local cap missing.

---

## 10. Machine-Verifiable Invariants & Acceptance Gates

| Gate ID | Verification Description | Target Assertion / Criteria |
|---|---|---|
| **`6D-1`** | Offline Intent Autonomy | Local intent resolution & agent execution pass with all network interfaces disabled; network-dependent plan steps return deferred/degraded state |
| **`6D-2`** | Zero Model Authority Rejection | Attempted capability escalation in model output returns `PermissionDenied` |
| **`6D-3`** | Confused Deputy Prevention | Unprivileged caller intent targeting privileged path returns `PermissionDenied` |
| **`6D-4`** | Workspace Containment | Intent targeting foreign workspace without `WorkspaceManagementCap` rejected |
| **`6D-5`** | Class 3 Side-Effect Modal Confirmation | Class 3 intent triggers Trusted Authorization Service (`authui` `ModalLock`) and requires `AuthorizationTransactionRef` |
| **`6D-6`** | Fail-Closed Modal Crash Cancellation | Pending Class 3 intent cancelled immediately on `authui` crash |
| **`6D-7`** | Ambiguous Intent Blocking | Ambiguous expression blocks execution and renders `shelld` candidate menu |
| **`6D-8`** | Workflow Persistence & Recovery | `WorkflowSpec` restored from `workspaced` after `intentd` restart |
| **`6D-9`** | Source Provenance Integrity | `InputSource` provenance preserved accurately from `uids` to `ExecutionPlan` |
| **`6D-10`**| Agent Proposal Capability Bound | Agent intent proposal verified against `AgentInputCap` and workspace caps |
| **`6D-11`**| CSDT Non-Authority Verification | Remote peer intent without local capability rejected despite valid CSDT |
| **`6D-12`**| Resource Lease Bounds | Intent parsing memory allocation enforced within `resourced` quota |
| **`6D-13`**| PMM Frame Neutrality | 100% physical frame count equality before and after Stage 6D suite |
| **`6D-14`**| Microkernel Substrate Preservation | 0 bytes modified in Stage 3A–3N production kernel nucleus |

---

## 11. Substrate Preservation Guarantee

Stage 6D requires **0 lines of code modified** in the Stage 3A–3N production Ring-0 microkernel nucleus. All logic is implemented entirely within user-space daemons (`intentd`, `shelld`, `authui`, `agentd`, `workloadd`).
