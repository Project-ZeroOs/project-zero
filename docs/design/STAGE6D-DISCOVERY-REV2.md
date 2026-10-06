# Stage 6D — Architecture Discovery & Dependency Audit (Rev2)

**Topic:** Human Intent, Intent Resolution & Action/Workflow Boundary Subsystem  
**Status:** 🟢 DISCOVERY REV2 COMPLETE — APPROVED FOR ARCHITECTURE DRAFT  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen (`intentd`, `agentd`, `workloadd`, `fabricd`, `resourced`)  
- Stage 5 Presentation Subsystem — 🟢 Frozen (`compositord`, `surfaced`, `authui`)  
- Stage 6A User Session Substrate — 🟢 Frozen (`shelld`, `workspaced`)  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  
- Stage 6C Human Input & Interaction Routing — 🟢 Frozen & Committed (`uids`)  

---

## 1. Executive Architectural Conclusion

Stage 6D defines the **Human Intent, Intent Resolution & Action/Workflow Boundary Subsystem** for ZeroOS.

Stage 6C established a critical architectural boundary:
```text
Physical / Remote / Synthetic Input
                │
                ▼
              uids  (Input Observation & Focus Routing Authority)
         ┌──────┴────────┐
         ▼               ▼
 focused surface      intentd (Intent Interpretation Authority)
```

`uids` is strictly an **observation and routing authority**, NOT an intent authority. It observes raw HID events, applies hit-testing, assigns qualified monotonic TSC timestamps, tags unforgeable input provenance, and routes events to either the focused surface or to `intentd`.

Stage 6D establishes the complete architectural contract for how **`intentd`** ingests human expressions, command palette streams, hotkeys, gestures, accessibility actions, and agent proposals, converts them into validated structural execution plans, connects to Stage 4F without duplicating or modifying the frozen `Intent → Plan → Agent → Workload` pipeline, and dispatches tasks to `agentd`, `workloadd`, and `fabricd`.

### Core Architectural Decisions & Rev2 Closures:

1. **Precise Offline Autonomy (`I-INTENT-OFFLINE-AUTONOMY`)**: Intent resolution, agent lifecycle, local workload execution, and local resource management DO NOT require network connectivity. Network is treated as a **workload resource dependency**, rather than an OS/agent prerequisite. An intent whose validated execution plan carries an explicit network dependency (e.g., fetching a remote URL or external web query) is gracefully deferred, degraded, or rejected when network resources are unavailable.
2. **Subordination of Side-Effect Confirmation to Existing Authorization Architecture**: `intentd` is the **Intent Interpretation & Structural Plan Compiler Authority**, NOT an authorization authority. Class 3 side-effect classification invokes the pre-existing **Trusted Authorization Service / `authui`** authority boundary (`ModalLock`). `AuthorizationTransactionRef` remains protocol state, NOT a new capability or `intentd` authority.
3. **Zero Model Authority (`I-INTENT-MODEL-NON-AUTHORITY`)**: AI/LLM model outputs (whether local or remote) are treated strictly as **untrusted candidate proposals**. Model output CANNOT execute system operations directly, CANNOT issue kernel syscalls, and CANNOT bypass capability checks. All model proposals must be validated deterministically by `intentd` against the caller's Stage 3H capability set.
4. **Seamless Integration with Frozen Stage 4F Pipeline**: Stage 6D connects human/agent expressions into the frozen Stage 4F `ExecutionPlan` structure without introducing a second intent engine or duplicating workload abstractions.
5. **No Capability Creation or Amplification (`I-INTENT-NO-CAPABILITY-AMPLIFICATION`)**: `intentd` processes plans strictly using the pre-existing capability handles of the originating Session/Workspace (`caller_cap_table`). `intentd` CANNOT manufacture capabilities or leverage its daemon privilege to execute actions on behalf of unprivileged callers (Confused Deputy Prevention).
6. **No New Capability Type**: Audit proves no new capability type is required for Stage 6D. Existing Stage 3H and Stage 6A–6C capabilities (`SessionManagementCap`, `AgentManagementCap`, `AgentInputCap`, `AccessibilityPolicyCap`, `WorkspaceManagementCap`) are fully sufficient.
7. **Workspace Containment (`I-INTENT-WORKSPACE-CONTAINMENT`)**: Every intent and compiled workflow is strictly bound to its originating `session_id` and `workspace_id`. Cross-workspace execution without explicit `WorkspaceManagementCap` is rejected.
8. **Substrate Preservation**: Stage 6D requires **0 lines of code modified** in the Stage 3A–3N production Ring-0 microkernel nucleus.

---

## 2. Scope

The Stage 6D discovery covers:

- Boundary definition between raw input observation (`uids`) and semantic intent interpretation (`intentd`).
- Structure, representation, and lifecycle of `HumanIntent` and `WorkflowSpec`.
- Ingestion pipelines for natural language, command palette strings, hotkeys, multi-touch gestures, accessibility semantic actions, and local/remote agent proposals.
- `intentd` authority boundaries, responsibilities, and structural plan compilation into Stage 4F `ExecutionPlan` units.
- Binding of intent execution to `SessionId`, `WorkspaceId`, `AgentId`, `WorkloadId`, and `TaskId`.
- Distinction, transition, and persistence boundaries between one-shot intents and persistent workflows.
- Strict enforcement of Model Non-Authority for local/remote AI inference engines.
- Guaranteed offline operation semantics and workload-level network dependency classification.
- Handling of ambiguous, incomplete, or malformed intent expressions.
- Integration with the pre-existing Trusted Authorization Service (`authui` `ModalLock`) for Class 3 side-effect operations.
- Security analysis against confused-deputy attacks, privilege escalation, and cross-workspace leakage.
- Interaction with Stage 4F (`intentd`/`fabricd`), Stage 4E (`agentd`/`workloadd`), Stage 4B (`resourced`), Stage 4D (`workspaced`), Stage 5 (`authui`/`surfaced`/`compositord`), Stage 6A (`shelld`), and Stage 6C (`uids`).
- Identification of candidate invariants, machine-verifiable gates, and opcode/ABI reuse.

---

## 3. Non-Goals

1. **No Kernel Nucleus Modifications**: Zero changes to Stage 3A–3N Ring-0 microkernel code.
2. **No Second Capability System**: All permissions rely strictly on Stage 3H capability handles.
3. **No Intent Interpretation in `uids`**: `uids` remains strictly an input routing and focus enforcement daemon.
4. **No AI/Model Execution Authority**: LLMs, heuristics, and natural-language parsers CANNOT directly invoke system calls, mutate files, or grant permissions.
5. **No Implicit Network Dependency**: OS and agent execution must never fail or block merely because network interfaces are down or remote peers are unreachable.
6. **No Re-definition of Authorization Authority**: `intentd` does NOT become an authorization authority; authorization remains strictly governed by Stage 3H capabilities and the Stage 5 Trusted Authorization Service (`authui`).
7. **No CSDT Intent Authority**: CSDT tokens grant distributed provenance and accounting context, NOT local intent execution authority.
8. **No Remote Peer Authorization Bypass**: Remote nodes submitting intent requests must pass local session/workspace capability verification.

---

## 4. Existing-Subsystem Dependency Map

```text
       Physical HID / Remote Input / Accessibility / Agent Input
                                 │
                                 ▼
                               uids  (Input Focus Policy Enforcer - InputFocusPolicyCap)
                        ┌────────┴────────────────────────┬────────────────────────┐
                        │                                 │                        │
                        ▼                                 ▼                        ▼
                     shelld                            authui                   intentd
         (Focus Policy & Command Palette           (Trusted Auth           (Intent Interpretation
             SessionManagementCap)                   ModalLock)                 Authority)
                        │                                 │                        │
                        ▼                                 ▼                        ▼
                    workspaced                        surfaced                 agentd / workloadd
              (Workspace Containment &            (Surface Viewport &        (Workload DAG Execution &
             WorkspaceManagementCap)               Hit-Testing Bounds)        AgentManagementCap)
                                                                                   │
                                                                                   ▼
                                                                                fabricd
                                                                        (Compute Fabric & Placement)
```

| Service | Stage Introduced | Stage 6D Role & Dependency Contract |
|---|---|---|
| **`uids`** | Stage 5 / 6C | Input observation authority; tags 64-byte `InputEvent` headers and routes raw intent streams to `intentd`. |
| **`shelld`** | Stage 6A | User shell authority; provides command palette interface, hotkey mappings, and presents disambiguation UI to human. |
| **`authui`** | Stage 5 / 6A | Trusted Authorization Service overlay authority; executes `ModalLock` for human confirmation of Class 3 side-effect intents. |
| **`intentd`** | Stage 4F / 6D | Intent Interpretation Authority; ingests intent expressions, validates structural plans against capabilities, compiles `ExecutionPlan`. |
| **`agentd`** | Stage 4E | Agent runtime authority; receives intent-derived agent tasks for execution under `AgentManagementCap`. |
| **`workloadd`** | Stage 4E / 4F | Workload orchestration authority; schedules and tracks task execution across local/remote compute nodes. |
| **`fabricd`** | Stage 4F | Compute fabric authority; provides optional remote execution placement for compute-heavy steps without introducing network dependency. |
| **`workspaced`**| Stage 4D | Workspace state & persistence authority; persists approved `WorkflowSpec` instances under workspace boundary. |
| **`resourced`** | Stage 4B | Resource lease authority; enforces memory, CPU, and execution budget limits on intent resolution. |

---

## 5. Authority Matrix

| Domain / Resource | Authoritative Owner | Capability Required | Enforcement Point |
|---|---|---|---|
| **Raw Input Ingestion & Routing** | `uids` | `InputFocusPolicyCap` | `uids` dispatch loop |
| **Hotkey & Shell UI Entry** | `shelld` | `SessionManagementCap` | `shelld` hotkey table |
| **Intent Semantics & Plan Validation** | `intentd` | `SessionManagementCap` / `AgentInputCap` | `intentd` plan engine |
| **Trusted Side-Effect Confirmation** | `authui` (Trusted Auth Service) | `AuthorizationTransactionRef` | `authui` `ModalLock` |
| **Agent Task Dispatch** | `agentd` | `AgentManagementCap` | `agentd` broker dispatch |
| **Workload DAG Orchestration** | `workloadd` | `WorkloadExecutionCap` | `workloadd` state machine |
| **Workflow Persistence** | `workspaced` | `WorkspaceManagementCap` | `workspaced` storage engine |
| **Resource Allocation Lease** | `resourced` | `ResourceLeaseCap` | `resourced` lease manager |

---

## 6. Comprehensive Architectural Discovery & Analysis

### 6.1 Exact Boundary Between Input Observation and Human Intent

- **Input Observation (`uids`)**: Operations at this layer deal exclusively with physical, remote, accessibility, synthetic, or agent-generated signals. `uids` decodes the 64-byte `InputEvent` frame, attaches qualified monotonic TSC timestamps, assigns unforgeable provenance (`INPUT_SOURCE_PHYSICAL`, `INPUT_SOURCE_ACCESSIBILITY`, `INPUT_SOURCE_AUTOMATION`, `INPUT_SOURCE_AGENT`, `INPUT_SOURCE_REMOTE`, `INPUT_SOURCE_TRUSTED_AUTH`), and applies spatial hit-testing. `uids` has **zero understanding** of what a key combo, gesture, or text string semantically means.
- **Human Intent (`intentd`)**: Operations at this layer deal with semantic goals expressed by humans or agents. `uids` forwards intent-bearing events to `intentd` via IPC (`OP_INTENT_SUBMIT` / `OP_UIDS_INGEST_INTENT`). `intentd` parses the raw payload, validates semantic syntax, maps expression to candidate actions, verifies caller capabilities, and compiles a structured Stage 4F `ExecutionPlan`.

```text
[Hardware Signal] ──> uids (Observe, Timestamp, Provenance, Route) ──> intentd (Interpret, Validate, Plan, Dispatch)
```

### 6.2 Intent Representation and Lifecycle

A `HumanIntent` object is a machine-readable structure defined in `libzero::intent`:

```rust
#[repr(C)]
pub struct HumanIntent {
    pub intent_id: DistributedId,        // 16-byte LE unique identifier
    pub session_id: DistributedId,       // Originating Session
    pub workspace_id: DistributedId,     // Originating Workspace
    pub agent_id: DistributedId,         // Originating Agent (0 if direct human)
    pub source_provenance: u16,          // Inherited from InputEvent provenance
    pub intent_type: u16,                // One-shot action, Command Palette, Hotkey, Gesture, Workflow
    pub side_effect_class: u8,           // Class 1 (Read-only), Class 2 (Scoped Mutate), Class 3 (Consequential)
    pub lifecycle_state: u8,             // Submitted, Parsed, Ambiguous, PendingApproval, Approved, Dispatched, Completed, Rejected, Failed
    pub raw_expression_len: u32,         // Byte length of raw expression payload
    pub payload_hash: [u8; 32],          // SHA-256 integrity hash of expression
}
```

#### Lifecycle State Transitions:
1. **`Submitted`**: Received by `intentd` from `uids`, `shelld`, or `agentd`.
2. **`Parsed`**: Parsed into candidate operations by deterministic parser or untrusted local model.
3. **`Ambiguous`**: Multiple candidate interpretations exist or confidence score below threshold; execution BLOCKED awaiting human disambiguation via `shelld`.
4. **`PendingApproval`**: Class 3 side-effect detected; execution BLOCKED awaiting human confirmation via Trusted Authorization Service (`authui` `ModalLock`).
5. **`Approved`**: Plan verified against caller capability table and human confirmation received (if required).
6. **`Dispatched`**: Compiled into Stage 4F `ExecutionPlan` and sent to `workloadd`/`agentd`.
7. **`Completed` / `Rejected` / `Failed`**: Terminal states recorded in telemetry log.

### 6.3 Input Entry Pipelines into Intent Subsystem

1. **Natural Language / Command Palette**: User opens command palette in `shelld` ($Ctrl+Space$). Text stream is collected by `shelld` and submitted to `intentd` via `OP_INTENT_SUBMIT`.
2. **Keyboard Shortcuts / System Hotkeys**: `shelld` maintains global hotkey map. When `uids` observes a hotkey event, it matches `shelld`'s policy table and routes a structured `INTENT_TYPE_HOTKEY` packet to `intentd`.
3. **Multi-Touch / Pointer Gestures**: `uids` gesture recognizer detects multi-finger swipe or circle patterns and submits gesture intent descriptors to `intentd`.
4. **Accessibility Input**: Accessibility services holding `AccessibilityPolicyCap` submit semantic action requests directly through `uids`, tagged as `INPUT_SOURCE_ACCESSIBILITY`, which are forwarded to `intentd`.
5. **Traditional Applications**: Applications send structured IPC intent requests (`OP_INTENT_SUBMIT`) to `intentd` within their active workspace scope.
6. **Local / Remote Agents**: Agents operating under `AgentInputCap` submit intent proposals to `intentd` via `agentd` broker.

### 6.4 `intentd` Authority and Exact Responsibilities

`intentd` is the **Intent Interpretation & Structural Plan Compiler Authority**.

#### Core Responsibilities:
- Ingest raw intent expressions from `uids`, `shelld`, applications, and `agentd`.
- Disambiguate expression syntax and semantics using deterministic grammar engines or local model proposal services.
- Enforce **Model Non-Authority**: treat all model outputs as untrusted proposal data.
- Validate proposed execution steps against the Stage 3H capability table of the originating `session_id`/`workspace_id`.
- Hand off Class 3 side-effect confirmation requests to the pre-existing **Trusted Authorization Service (`authui`)**.
- Compile validated intents into Stage 4F `ExecutionPlan` structures and dispatch to `workloadd`/`agentd`.
- Manage persistent workflow templates in coordination with `workspaced`.

### 6.5 Binding to Session, Workspace, Agent, Workload, and Task

Every `HumanIntent` and compiled `ExecutionPlan` MUST carry explicit binding identifiers:

```text
HumanIntent ──> ExecutionPlan ──> WorkloadDAG ──> Task
   │                 │                │            │
   ├── session_id    ├── session_id   ├── session  ├── session
   ├── workspace_id  ├── workspace_id ├── workspace├── workspace
   └── agent_id      └── agent_id     └── workload └── task_id
```

- **Session Binding**: Guarantees that intent authority is scoped to the authenticated user session.
- **Workspace Containment**: Guarantees that resources targeted by the intent belong exclusively to the active workspace.
- **Agent Binding**: Identifies whether a human or an agent initiated the intent pipeline.
- **Workload & Task Binding**: Maps high-level intent goals into concrete executable units tracked by Stage 4E `workloadd`.

### 6.6 One-Shot Intent vs. Persistent Workflow

| Feature | One-Shot Intent | Persistent Workflow |
|---|---|---|
| **Semantics** | Ephemeral, immediate single-action command | Multi-step DAG, scheduled trigger, or event-driven automation |
| **Examples** | "Mute sound", "Close window", "Find file X" | "Daily backup workspace to disk", "CI test pipeline on commit" |
| **Lifetime** | Immediate execution; transient in-memory state | Persistent definition stored in `workspaced`; reusable execution template |
| **State Storage** | Ephemeral `intentd` memory queue | `workspaced` persistence engine (`WorkflowSpec`) |
| **Trigger Mechanism** | Direct user/agent invocation | Schedule, event hook, or manual instantiation |
| **Recovery** | Client retries idempotent `IntentId` on failure | Automatically recovered by `workloadd`/`workspaced` on daemon crash |

### 6.7 Workflow Creation, Persistence, Editing, Approval, and Execution Boundaries

1. **Creation**: User or agent submits a multi-step workflow proposal (`WorkflowSpec`) to `intentd`.
2. **Parsing & Validation**: `intentd` validates structural DAG validity, dependency graph, and resource requirements.
3. **Human Approval Boundary**: If the workflow contains any step with Class 3 side effects or broad capability requirements, `intentd` MUST trigger confirmation via the Trusted Authorization Service (`authui`). Execution is BLOCKED until explicit human confirmation.
4. **Persistence**: Upon approval, `intentd` submits the `WorkflowSpec` to `workspaced` via `OP_WORKSPACE_STORE_WORKFLOW`. The spec is saved in workspace persistent storage.
5. **Editing**: Modifying a workflow creates a new versioned `WorkflowSpec`. If permissions increase, re-approval via `authui` is mandatory.
6. **Execution**: Triggered workflows are instantiated as Stage 4E Workloads by `workloadd`.

### 6.8 AI / Model Inference as Untrusted Proposal Data

ZeroOS enforces the **Zero Model Authority Principle (`I-INTENT-MODEL-NON-AUTHORITY`)**:

```text
[Human Expression] ──> Model / Parser Engine (Untrusted Proposal) ──> Candidate Plan
                                                                            │
                                                                            ▼
                                                                  intentd Safety Engine
                                                                 (Capability Validation)
                                                                            │
                                                                 ┌──────────┴──────────┐
                                                                 ▼                     ▼
                                                          [Valid & Authorized]   [Invalid / Escalated]
                                                                 │                     │
                                                                 ▼                     ▼
                                                          ExecutionPlan             REJECTED
```

- Local or remote LLMs, neural networks, or heuristic parsers generate candidate `PlanProposal` structs.
- Model outputs **CANNOT** invoke system calls, **CANNOT** access raw file handles, and **CANNOT** bypass capability checks.
- `intentd` validates candidate plans deterministically:
  1. Does the originating session possess Stage 3H capabilities for every action in the plan?
  2. Does any action attempt cross-workspace access without explicit `WorkspaceManagementCap`?
  3. Does any action exceed resource quotas?
- If validation fails, `intentd` rejects the proposal immediately, regardless of model confidence scores.

### 6.9 Offline Operation & Workload Network Dependency Semantics

**`I-INTENT-OFFLINE-AUTONOMY`**: Intent resolution, agent lifecycle, local workload execution, and local resource management DO NOT require network connectivity.

- Network is treated as a **workload resource dependency**, rather than an OS or agent prerequisite.
- **Offline Resolution Path**:
  - `intentd` uses local parsers, hotkey tables, structured grammar engines, and local small models.
  - All core local intents (window management, local app launching, workspace switching, local file operations, local agent orchestration) operate 100% offline.
- **Workload Network Dependencies**:
  - If a validated execution plan carries an explicit network dependency (e.g., fetching a remote URL, external web query, or remote compute placement via `fabricd`):
    - When network is online $\rightarrow$ execution completes normally.
    - When network is offline $\rightarrow$ the specific network-dependent workload step is **deferred, degraded, or rejected** with `ZeroError::TimeAuthorityUnavailable` or `ZeroError::StaleEndpoint`.
  - Network unavailability **MUST NEVER** stall, crash, or block local OS operations, local agent loops, or local workload execution.

### 6.10 Ambiguous or Underspecified Intent Handling

When an intent expression yields ambiguous interpretations (multiple candidate plans with similar confidence or incomplete parameters):

```text
[Ambiguous Expression] ──> intentd ──> State: AMBIGUOUS
                                           │
                                           ▼
                                    shelld / authui
                               (Disambiguation Palette UI)
                                           │
                                           ▼
                                [Human Selection / Input] ──> Approved Plan
```

1. `intentd` MUST NOT guess, extrapolate randomly, or execute a default high-risk branch.
2. `intentd` sets intent state to `Ambiguous`.
3. `intentd` emits a disambiguation payload to `shelld`, which renders a candidate selection menu to the user.
4. Execution remains strictly **BLOCKED** until the user selects a candidate or cancels the intent.

### 6.11 Subordination of Class 3 Side-Effect Confirmation to Existing Authorization Architecture

Side effects are classified into three strict tiers:

- **Class 1 (Read-Only / Ephemeral)**: File search, status query, UI focus change. No prompt required.
- **Class 2 (Workspace-Scoped Mutation)**: Creating local file in workspace, launching app within workspace bounds. Inline notification toast in `shelld`.
- **Class 3 (Consequential Side Effects)**: Destructive file deletion, external network egress, credential access, broad system configuration change, workspace deletion, code execution with elevated capabilities.

#### Authority Boundary for Class 3 Confirmation:
`intentd` is **NOT** an authorization authority. It merely performs intent parsing and plan compilation. When `intentd` detects a Class 3 operation during plan compilation, it triggers confirmation through the existing **Trusted Authorization Service (`authui` / `ModalLock`)**:

```text
Human Intent ──> intentd ──> validated Plan ──> Agent / Workload ──> side-effect classification
                                                                            │
                                                                            ▼
                                                                Trusted Authorization Service
                                                                     (authui / ModalLock)
```

1. `intentd` compiles candidate plan and flags Class 3 side-effect steps.
2. `intentd` invokes the Trusted Authorization Service (`authui`).
3. `authui` requests `ModalLock` from `uids` and presents an isolated trusted authorization dialog to the human.
4. Upon physical human approval, `authui` emits `AuthorizationTransactionRef`.
5. `AuthorizationTransactionRef` remains **protocol state** verifying user confirmation, NOT a new capability or `intentd` authority.
6. If `authui` crashes or is dismissed, `ModalLock` is invalidated and execution is cancelled (`I-INTENT-FAIL-CLOSED`).

### 6.12 Capability Enforcement: No Creation or Amplification of Authority

- **`I-INTENT-NO-CAPABILITY-AMPLIFICATION`**: `intentd` operates with zero intrinsic authority over user resources.
- When evaluating an intent, `intentd` checks the `caller_cap_table` attached to the `session_id`/`workspace_id`.
- `intentd` cannot invent, synthesize, or amplify capabilities.
- An intent attempting an action for which the caller lacks a Stage 3H capability handle is rejected with `ZeroError::PermissionDenied`.

### 6.13 Workspace Containment

- **`I-INTENT-WORKSPACE-CONTAINMENT`**: Every intent is tagged with `workspace_id`.
- All sub-tasks, generated workloads, file handles, and surface operations inherit this `workspace_id`.
- `intentd` verifies that all target paths, IPC channels, and memory buffers belong to `workspace_id`.
- Cross-workspace intent execution without `WorkspaceManagementCap` is rejected with `ZeroError::PermissionDenied`.

### 6.14 Interaction with Existing Stage 4F Intent/Plan Architecture

Stage 4F defined `intentd` and `fabricd` IPC contracts:
- `OP_INTENT_SUBMIT (0x0709)` & `OP_INTENT_SUBMIT_RESP (0x070A)`
- `ExecutionPlan` struct and `CSDTToken` generation.

Stage 6D completes the front-end human and agent interaction substrate feeding directly into Stage 4F without duplicating or modifying the frozen pipeline:
```text
Human / Agent Expression
          │
          ▼
        uids (Observation / Routing)
          │
          ▼
       intentd (Interpretation & Plan Compilation)
          │
          ▼
     ExecutionPlan (Stage 4F Struct)
          │
          ▼
        Agent (Stage 4E Agent Runtime)
          │
          ▼
     Workload DAG (Stage 4E / 4F Workload)
          │
          ▼
        Task (Stage 4E Executable Unit)
          │
          ▼
       Process (Stage 3 Ring-3 Execution)
```

Stage 6D reuses Stage 4F's frozen `ExecutionPlan` structure and `OP_INTENT_SUBMIT` opcode without modification.

### 6.15 Interaction with agentd, workloadd, fabricd, and resourced

- **`agentd` (Stage 4E)**: Receives intent-derived agent tasks. Agents operate under `AgentManagementCap` and `AgentInputCap`.
- **`workloadd` (Stage 4E)**: Manages execution of compiled `WorkloadDAG` instances derived from `intentd` plans.
- **`fabricd` (Stage 4F)**: Executes compute tasks across local CPU/GPU or optional remote peers.
- **`resourced` (Stage 4B)**: Allocates RAM, CPU, and execution leases for intent resolution and workload execution.

### 6.16 Existing IPC / Opcodes / ABIs Reuse Audit

Stage 6D reuses existing frozen opcodes and structures:

- **Opcodes**:
  - `OP_INTENT_SUBMIT (0x0709)` & `OP_INTENT_SUBMIT_RESP (0x070A)` (Stage 4F)
  - `OP_UIDS_INGEST_INTENT (0x0711)` & `OP_UIDS_INGEST_INTENT_RESP (0x0712)` (Stage 5/6C)
  - `OP_SESSION_CREATE (0x0601)` & `OP_SESSION_SWITCH_WORKSPACE (0x0603)` (Stage 6A)
  - `OP_WORKLOAD_CREATE (0x04E1)` & `OP_WORKLOAD_CANCEL (0x04E3)` (Stage 4E)
- **ABIs & Structs**:
  - `InputEvent` (64-byte LE wire contract, Stage 6C)
  - `DistributedId` (16-byte LE struct, Stage 3I/4B)
  - `IpcMessage` (80-byte wire contract, Stage 3G)
  - `ExecutionPlan` (Stage 4F)

No new syscalls or duplicate opcodes are required.

### 6.17 Capability Audit: Is a New Capability Type Necessary?

**Audit Conclusion**: **NO NEW CAPABILITY TYPE IS NECESSARY.**

- `InputFocusPolicyCap (0x0042)`: Held by `uids` for input routing.
- `SessionManagementCap (0x0031)`: Held by `shelld` for shell/session operations.
- `AgentManagementCap (0x0033)` & `AgentInputCap (0x0045)`: Held by `agentd`/agents for agent orchestration and proposal submission.
- `AccessibilityPolicyCap (0x0044)`: Held by accessibility services.
- `WorkspaceManagementCap (0x0030)`: Held by `workspaced` for workspace isolation.

Stage 6D intent resolution operates entirely within this pre-existing Stage 3H/6A–6C capability matrix.

### 6.18 Persistence Requirements and Crash / Recovery Semantics

- **One-Shot Ephemeral Intents**: Stored in transient memory queues within `intentd`. If `intentd` crashes, transient state is cleared; client applications retry using idempotent `intent_id`.
- **Persistent Workflows**: Saved in `workspaced` persistent storage (`WorkflowSpec`). If `intentd` or `workloadd` crashes, active workflows are reloaded from `workspaced` state logs upon daemon restart, resuming execution without resource leaks or corrupted state.

### 6.19 Security Boundaries and Confused-Deputy Risks

- **Confused-Deputy Vulnerability**: An unprivileged process or agent submits a natural-language prompt (e.g., "Delete system file X") hoping `intentd` will execute the action using `intentd`'s elevated system daemon privileges.
- **Mitigation (`I-INTENT-CONFUSED-DEPUTY-PREVENTION`)**: `intentd` NEVER evaluates permissions based on its own process capabilities. Every candidate plan step is validated against the caller's capability handle set (`caller_cap_table`). If the caller lacks `FileWriteCap` for path X, `intentd` rejects the plan immediately.

### 6.20 Remote Input / Remote Presentation Interaction with Intent

- Remote input events arrive from Stage 6B via `fabricd` and `uids`.
- `uids` verifies local `RemoteInputPolicyCap`.
- `uids` attaches `INPUT_SOURCE_REMOTE` provenance and forwards raw intent stream to `intentd`.
- `intentd` processes remote intents subject to local workspace capability bounds. CSDT tokens provide distributed provenance context but **NEVER** grant local authorization bypass (`I-INTENT-REMOTE-NON-AUTHORITY`).

### 6.21 Failure and Fail-Closed Behavior

- **Parser / Model Failure**: If local parser or model times out, emits invalid format, or fails: `intentd` rejects intent with `ZeroError::InvalidRequest` or `ZeroError::ServiceFailed`.
- **`authui` Crash during Modal Lock**: If `authui` crashes while displaying a Class 3 confirmation modal, `uids` invalidates `ModalLock`, and pending intent execution is immediately cancelled (`I-INTENT-FAIL-CLOSED`).
- **`intentd` Daemon Crash**: Active workloads managed by `workloadd` continue safely or pause; `shelld` re-binds to `intentd` upon auto-restart.

### 6.22 Resource Accounting and Network Dependency Semantics

- Intent parsing, disambiguation, and plan compilation consume memory and CPU bounded by Stage 4B `resourced` leases.
- Network connectivity is strictly a workload resource dependency. If network is down, local execution paths proceed normally, while network-dependent workload steps degrade gracefully without error cascades or blocking timeouts.

### 6.23 Machine-Verifiable Invariants and Acceptance Gates

- **`I-INTENT-OFFLINE-AUTONOMY`**: Intent resolution, agent lifecycle, local workload execution, and local resource management do not require network connectivity. An intent whose validated execution plan has an explicit network dependency may be deferred, degraded, or rejected when that resource is unavailable.
- **`I-INTENT-MODEL-NON-AUTHORITY`**: Model/parser outputs are untrusted proposals and can never grant or expand system authority.
- **`I-INTENT-NO-CAPABILITY-AMPLIFICATION`**: `intentd` executing plans strictly within caller capability bounds.
- **`I-INTENT-WORKSPACE-CONTAINMENT`**: Rejection of cross-workspace intent execution without `WorkspaceManagementCap`.
- **`I-INTENT-CONFUSED-DEPUTY-PREVENTION`**: Unprivileged caller intent rejected when targeting privileged paths.
- **`I-INTENT-TRUSTED-SIDE-EFFECT-CONFIRMATION`**: Class 3 side effects requiring confirmation through the Trusted Authorization Service (`authui` `ModalLock`).
- **`I-INTENT-FAIL-CLOSED`**: Pending side-effect intent cancelled on `authui` crash.
- **`I-INTENT-STATE-RECONCILIATION`**: Disambiguation menu cancellation clearing intent state cleanly.

---

## 7. Proposed Architecture Boundary

```text
Physical / Remote / Synthetic / Accessibility Signal
                          │
                          ▼
                        uids  (Input Focus Enforcer - InputFocusPolicyCap)
                   ┌──────┴────────────────────────┐
                   ▼                               ▼
            focused surface                     intentd (Intent Interpretation Authority)
                                                   │
                                          ┌────────┴────────┐
                                          ▼                 ▼
                                    shelld / authui     agentd / workloadd
                              (Disambiguation & Modal)  (Workload DAG Execution)
                                                            │
                                                            ▼
                                                         fabricd (Optional Remote Placement)
```

The proposed architecture boundary establishes **`intentd`** as the sole Intent Interpretation Authority, operating as a multi-service coordinator between `uids` (input routing), `shelld` (user shell UI), `authui` (trusted confirmation modal), `workspaced` (workflow persistence), `agentd`/`workloadd` (execution engine), and `resourced`/`fabricd` (resource leases & optional compute placement).

---

## 8. Unresolved Questions

1. **Exact Local Grammar vs. Small Model Runtime Payload**: What is the maximum binary footprint allocated for `intentd`'s offline deterministic parser engine within Stage 4B RAM lease bounds?
2. **Disambiguation Timeout Policy**: How long should an intent remain in the `Ambiguous` or `PendingApproval` state before `intentd` automatically times out and cancels the intent?

---

## 9. Dependencies on Frozen Stages

- **Stage 3A–3N**: Production Ring-0 microkernel, IPC channels, and Stage 3H capability handles.
- **Stage 4A–4F**: `intentd` IPC opcodes, `ExecutionPlan` structure, `agentd` agent runtime, `workloadd` workload orchestration, `workspaced` persistence, `resourced` leases, and `fabricd` compute placement.
- **Stage 5**: `surfaced`, `compositord`, and `authui` trusted modal lock overlay.
- **Stage 6A**: `shelld` session management and command palette interface.
- **Stage 6B**: Distributed spatial presentation and remote input provenance.
- **Stage 6C**: `uids` 64-byte `InputEvent` observation, timestamping, hit-testing, and focus routing.

---

## 10. Candidate Invariants

1. **`I-INTENT-OFFLINE-AUTONOMY`**: Intent resolution, agent lifecycle, local workload execution, and local resource management do not require network connectivity. An intent whose validated execution plan has an explicit network dependency may be deferred, degraded, or rejected when that resource is unavailable.
2. **`I-INTENT-MODEL-NON-AUTHORITY`**: Model/parser outputs are untrusted proposals and can never grant or expand system authority.
3. **`I-INTENT-NO-CAPABILITY-AMPLIFICATION`**: `intentd` must validate plan steps strictly against caller capability handles; `intentd` daemon privileges shall never be leveraged to bypass caller checks.
4. **`I-INTENT-WORKSPACE-CONTAINMENT`**: All intents, plans, workloads, and task artifacts are strictly bound to their originating `workspace_id`.
5. **`I-INTENT-CONFUSED-DEPUTY-PREVENTION`**: Unprivileged processes cannot use natural language or intent IPC to execute actions exceeding their capability set.
6. **`I-INTENT-TRUSTED-SIDE-EFFECT-CONFIRMATION`**: Class 3 side-effect actions mandate physical human confirmation via the Trusted Authorization Service (`authui` `ModalLock`).
7. **`I-INTENT-FAIL-CLOSED`**: Crashes in `authui`, `shelld`, or `intentd` during intent resolution must invalidate pending transaction locks and cancel unconfirmed intents.
8. **`I-INTENT-SOURCE-PROVENANCE-PRESERVATION`**: Provenance assigned by `uids` must be preserved end-to-end through `HumanIntent` and `ExecutionPlan`.

---

## 11. Candidate Machine-Verification Gates

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

## 12. Recommendation

**Recommendation:** 🟢 **COMPLETE — APPROVED**

Stage 6D Discovery Rev2 incorporates both requested closures:
1. `I-INTENT-OFFLINE-AUTONOMY` precisely defines offline operation while treating network as a workload-level resource dependency.
2. Class 3 side-effect confirmation is strictly subordinated to the pre-existing **Trusted Authorization Service (`authui`)**, preserving `intentd` as an intent interpretation/compiler authority rather than an authorization authority.

The discovery connects seamlessly to frozen Stage 4F (`Intent → ExecutionPlan → Agent → WorkloadDAG → Task → Process`) without creating duplicate intent engines or modifying kernel substrates.

The subsystem is approved to proceed to **`STAGE6D-ARCHITECTURE-REV1.md`**.
