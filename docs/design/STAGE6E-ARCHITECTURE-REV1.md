# STAGE 6E ARCHITECTURE REV1 — FROZEN CONTRACT

**Status:** Proposed Architecture  
**Stage:** 6E  
**Subsystem:** Human-Agent Telemetry, Interactive Feedback & Workflow Synthesis Subsystem (`observed`)  

---

## 1. Subsystem Architecture Overview

Stage 6E establishes `observed` (**Human-Agent Telemetry & Interactive Feedback Daemon**), completing the human-computer interaction loop for ZeroOS.

While Stage 6C (`uids`) observes raw input events, Stage 6D (`intentd`) parses intent, and Stage 4F (`workloadd`/`agentd`) executes Workload DAGs, **Stage 6E manages ongoing operational coordination between the user and active executions**:

```text
                                  ┌───────────────────────────┐
                                  │      shelld (Stage 6A)    │
                                  │   (Session & Layout UI)   │
                                  └─────────────┬─────────────┘
                                                │
                                    Telemetry / │ Interactive Feedback /
                                    Status Stream│ Action Recording
                                                ▼
┌───────────────────────────┐     ┌───────────────────────────┐     ┌───────────────────────────┐
│       uids (Stage 6C)     │────►│    observed (Stage 6E)    │◄────│     workloadd (Stage 4F)  │
│    (Input Event Tap)      │     │  (Coordination & Telemetry)│     │    (DAG & Task Execution) │
└───────────────────────────┘     └─────────────┬─────────────┘     └───────────────────────────┘
                                                │
                                       Sanitized Action Trace /
                                       Workflow Proposal
                                                ▼
                                  ┌───────────────────────────┐
                                  │     intentd (Stage 6D)    │
                                  │   (Proposal Validation &  │
                                  │   ExecutionPlan Synthesis)│
                                  └───────────────────────────┘
```

### Core Architectural Principle
`observed` is strictly a **coordinator and telemetry broker**, never an executor or authority grantor. It aggregates progress telemetry, routes asynchronous human feedback prompts, and records user demonstration interactions into sanitized workflow proposals.

---

## 2. Authority and Non-Authority Boundary

### 2.1 Explicit Authority Matrix

| Capability Category | `observed` Authority | Authoritative Daemon | Authority Lineage / Enforcer |
|---|---|---|---|
| Task / Process Execution | ❌ NO | `workloadd` / `agentd` | Stage 4F Process Management |
| Intent Parsing & Plan Compilation | ❌ NO | `intentd` | Stage 6D Intent Interpreter |
| Hardware / Remote Input Ingestion | ❌ NO | `uids` | Stage 6C Input Ingestion Authority |
| Window Focus & Layout Presentation | ❌ NO | `shelld` | Stage 6A Session Presentation |
| Security Privilege Elevation | ❌ NO | `authui` | Stage 5 Trusted Authorization |
| Kernel Capability Creation / Elevation | ❌ NO | Stage 3H Kernel | Stage 3H Capability Kernel |
| Resource Lease Allocation | ❌ NO | `resourced` | Stage 4B Resource Graph |
| **Telemetry Aggregation & Dispatch** | 🟢 **YES** | `observed` | Stage 6E Telemetry Broker |
| **Non-Security Interactive Feedback Routing**| 🟢 **YES** | `observed` | Stage 6E Feedback Protocol |
| **Action Trace Sanitization & Packaging** | 🟢 **YES** | `observed` | Stage 6E Recording Pipeline |

### 2.2 Capability Lineage Audit (Zero New Capabilities)

Stage 6E requires **zero new Stage 3H capability types**. It operates strictly using existing frozen kernel capability primitives:

```text
Existing Capability Type              Stage 6E Usage Contract
-----------------------              -----------------------
IpcEndpointCap (0x0001)              IPC endpoint connection to observed service
WorkspaceAccessCap (0x0030)          Workspace boundary containment for streams & traces
SyntheticInputCap (0x0043)           Enforced by uids when playing back synthesized plans
ResourceLeaseCap (0x0020)            Memory accounting for ring buffers in resourced
```

**Non-Authority Invariant:** `observed` possesses no ambient capabilities and cannot create, elevate, or delegate capabilities to any process, workload, or agent.

---

## 3. Telemetry Subscription & Event Model

### 3.1 IPC Telemetry Frame Contract (`TelemetryFrame`)

`workloadd` and executing agents emit telemetry frames over Stage 3E IPC ring buffers to `observed`.

```rust
#[repr(C, align(64))]
pub struct TelemetryFrame {
    pub frame_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub timestamp_tsc: u64,
    pub progress_pct: u8,
    pub telemetry_type: u8, // 1=Status, 2=Progress, 3=Log, 4=Metric, 5=Complete, 6=Failed
    pub status_code: u16,
    pub reserved: [u8; 4],
    pub status_message: [u8; 32],
}
```

### 3.2 Subscription Protocol
1. `shelld` requests a workspace telemetry subscription via IPC opcode `OP_OBSERVED_SUBSCRIBE_TELEMETRY` (0x0801), supplying a valid `WorkspaceAccessCap`.
2. `observed` validates `WorkspaceAccessCap` against the requested `workspace_id`.
3. If valid, `observed` streams filtered `TelemetryFrame` structures to `shelld`.
4. Ring buffers are pre-allocated (64 KB per DAG, max 2 MB per workspace session) and accounted to the workspace session in `resourced`.

---

## 4. Interactive Feedback Protocol & Timeout Semantics

Executing agents requiring human input (Class-1/Class-2 non-security choices, parameter selection) initiate feedback prompts through `observed`.

### 4.1 Feedback ABI Data Structures

```rust
#[repr(C, align(64))]
pub struct FeedbackPrompt {
    pub prompt_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub timeout_ms: u32,
    pub prompt_class: u8,    // 1=Informational Choice, 2=Parameter Selection
    pub default_action: u8,  // 0=Cancel/Fail-Closed
    pub reserved: [u8; 2],
    pub prompt_title: [u8; 32],
}

#[repr(C, align(64))]
pub struct FeedbackResponse {
    pub prompt_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub response_status: u8, // 0=Cancel, 1=Accepted, 2=SelectedOption
    pub selected_option: u8,
    pub reserved: [u8; 6],
    pub response_payload: [u8; 40],
}
```

### 4.2 Feedback Execution Flow & Fail-Closed Rules

```text
Agent Execution in workloadd
              │
              ▼
    [ Emits FeedbackPrompt ] ──► observed Service Validation (WorkspaceAccessCap check)
                                            │
                                            ▼
                              Routes to active shelld session
                                            │
               ┌────────────────────────────┴────────────────────────────┐
               ▼                                                         ▼
    User Selects Option (shelld)                                 Timeout / Disconnect / Crash
               │                                                         │
               ▼                                                         ▼
    [ Emits FeedbackResponse ]                                 [ Emits Fail-Closed ]
   (response_status = ACCEPTED)                                (response_status = CANCEL)
               │                                                         │
               └────────────────────────────┬────────────────────────────┘
                                            │
                                            ▼
                                Agent Execution Resumes
```

### 4.3 Class-3 Security Interception Rule
If an agent attempts to emit a `FeedbackPrompt` requesting security elevation, privilege escalation, or Class-3 consent:
- `observed` **intercepts and drops** the prompt immediately.
- `observed` returns `ERR_SECURITY_CLASS_VIOLATION` to the agent.
- Class-3 security prompts must execute strictly via the Stage 5 `authui` trusted path.

---

## 5. Demonstration Recording & Sensitive-Input Filtering

Stage 6E enables user demonstration recording to synthesize reusable workflows without introducing authority bypasses.

### 5.1 Recording Boundary & Event Tap

1. Recording is initiated by the user through `shelld` (`OP_OBSERVED_START_RECORDING`).
2. `observed` requests an audited event tap from `uids` (Stage 6C) bounded by `WorkspaceAccessCap`.
3. `observed` records high-level UI events (`InputEvent`) and high-level intent invocations (`intentd`).

### 5.2 What Gets Recorded vs. Discarded

```text
Recorded Events                           Discarded Events
---------------                           ----------------
- High-level element clicks               - Raw mouse hover / jitter
- Form field submit events                - Backspaces and editing pauses
- Window / Surface selection              - Keystrokes in sensitive fields
- Semantic Intent invocations             - Unfocused background activity
```

### 5.3 Sensitive-Input Filtering & Secret Scrubbing Protocol

To prevent accidental recording of credentials, tokens, or personal secrets:
1. **Sensitive Field Tagging:** `uids` marks all input fields associated with security attributes (password inputs, API token fields, key boxes) with `INPUT_FLAG_SENSITIVE` (0x80).
2. **Automatic Scrubbing:** Any `InputEvent` carrying `INPUT_FLAG_SENSITIVE` is immediately **scrubbed and discarded** by `observed`.
3. **Field Parameterization:** Text inputs in non-sensitive fields are stripped of literal string values and replaced with generic variable placeholders (`${PARAM_1}`, `${PARAM_2}`).
4. **Confidential Surface Isolation:** Surfaces tagged as confidential by `shelld` are excluded from the action trace stream.

---

## 6. Workflow-Proposal → `intentd` → Stage 4F Path

**Crucial Invariant:** Action recordings do NOT yield executable scripts and cannot execute directly. A recorded trace is purely an unvalidated workflow proposal.

### 6.1 Action Trace Compilation Pipeline

```text
User Demonstration Session
            │
            ▼
Stage 6C uids Input Tap ──► Sensitive-Input Filter ──► ActionTrace Buffer (observed)
                                                             │
                                                             ▼
                                                [ Unvalidated WorkflowProposal ]
                                                             │
                                                             ▼
                                               IPC: OP_INTENT_COMPILE_PROPOSAL
                                                             │
                                                             ▼
                                                    Stage 6D intentd
                                              (Deterministic Schema Check &
                                               Capability Requirement Audit)
                                                             │
                                                             ▼
                                                  Stage 4F ExecutionPlan
                                              (Stored in workspaced Registry)
                                                             │
                                                             ▼
                                               Stage 4F agentd / workloadd
                                              (Authoritative DAG Execution)
```

### 6.2 IPC Contract for Plan Compilation
- `observed` sends the sanitized `WorkflowProposal` artifact to `intentd` via IPC opcode `OP_INTENT_COMPILE_PROPOSAL` (0x0720).
- `intentd` verifies that all proposed actions conform to valid `HumanIntent` schemas and that the workspace holds required capabilities.
- Only upon successful compilation by `intentd` does a valid Stage 4F `ExecutionPlan` exist.

---

## 7. Persistence & Crash Recovery Architecture

### 7.1 State Persistence Matrix

```text
State Type                     Storage Class      Lifecycle / Recovery Policy
----------                     -------------      ---------------------------
Telemetry Ring Buffer          Ephemeral Memory   Lost on restart/reboot; re-queried from workloadd
Pending Feedback Prompt        Ephemeral Memory   Resolves to CANCEL on crash (fail-closed)
Active Action Recording        Workspace Disk WAL Flushed to /workspace/recordings/; recovered on restart
Synthesized ExecutionPlan      Workspace Storage  Persisted in /workspace/workflows/ via workspaced
```

### 7.2 Daemon Restart Recovery Protocol
If `observed` crashes or is restarted by the init manager:
1. `observed` initializes its pre-allocated memory buffers.
2. `observed` sends `OP_WORKLOAD_QUERY_ACTIVE_DAGS` to `workloadd` to re-establish telemetry stream subscriptions.
3. All pending feedback prompts prior to the crash are marked expired (`FEEDBACK_RESPONSE_CANCEL`), ensuring agents do not hang indefinitely.
4. Incomplete action recordings are marked `RECORDING_STATUS_INTERRUPTED` in workspace storage.

---

## 8. Offline Behavior Model

`observed` operates with **zero network dependencies**.

```text
Network Available       ──► Local & remote node telemetry aggregated seamlessly
Network Unavailable     ──► Local telemetry aggregated; remote nodes marked OFFLINE
Remote Node Disconnect  ──► Remote telemetry marked STALE; remote feedback prompts fail-closed
Local Resource Exhaustion──► Telemetry ring buffers drop oldest log frames; status frames retained
```

All IPC, buffer management, action trace filtering, and proposal packaging function fully when network interfaces are offline.

---

## 9. Machine-Verifiable Invariants

1. **`I-6E-NO-AUTHORITY-AMPLIFICATION`**: `observed` cannot create, elevate, or grant Stage 3H capabilities to any process, workload, or agent.
2. **`I-6E-WORKSPACE-CONTAINMENT`**: Telemetry streams, feedback prompts, and action recordings are strictly isolated by workspace ID and enforced via `WorkspaceAccessCap`.
3. **`I-6E-OFFLINE-SAFETY`**: All `observed` functions execute deterministically without network socket dependencies.
4. **`I-6E-MODEL-NO-AUTHORITY`**: AI models cannot issue Class-3 security prompts or bypass schema compilation via `observed`.
5. **`I-6E-NO-STAGE4F-DUPLICATION`**: `observed` maintains zero process execution state, DAG scheduling, or resource lease allocation logic.
6. **`I-6E-RESOURCE-ACCOUNTING`**: Memory buffers for telemetry streams are accounted to host workspace sessions in `resourced`.
7. **`I-6E-FAIL-CLOSED-FEEDBACK`**: Unanswered, timed-out, or disconnected feedback prompts automatically resolve to `FEEDBACK_RESPONSE_CANCEL`.
8. **`I-6E-RECORDING-SANITATION`**: All `InputEvent` records tagged with `INPUT_FLAG_SENSITIVE` are scrubbed prior to action trace generation.
9. **`I-6E-RECORDING-PROPOSAL-ONLY`**: Action recordings cannot execute directly; execution requires compilation by `intentd` into a Stage 4F `ExecutionPlan`.

---

## 10. Machine Acceptance Gates

| Gate ID | Assertion Description | Boundary Tested | Expected Result | Failure Condition |
|---|---|---|---|---|
| **Gate 6E-1** | ABI Layout & Alignment | `TelemetryFrame` / `FeedbackPrompt` ABI | Exactly 64 bytes, 64-byte aligned | Alignment or padding error |
| **Gate 6E-2** | Telemetry Frame Delivery | `workloadd` ──► `observed` ──► `shelld` | Frame delivered in < 1ms | Dropped or delayed frame |
| **Gate 6E-3** | Feedback Prompt Timeout | Prompt timeout expiration | Resolves to `FEEDBACK_RESPONSE_CANCEL` | Agent blocks indefinitely |
| **Gate 6E-4** | Class-3 Interception | Security prompt sent to `observed` | Intercepted; returns `ERR_SECURITY_CLASS_VIOLATION` | Security prompt routed |
| **Gate 6E-5** | Sensitive Input Scrubbing | Password field input recording | Event dropped; string scrubbed | Plaintext secret logged |
| **Gate 6E-6** | Recording Proposal Handoff| Action trace compilation | Sent to `intentd` via `OP_INTENT_COMPILE_PROPOSAL` | Executed directly |
| **Gate 6E-7** | Workspace Isolation | Cross-workspace telemetry request | Rejected with `ERR_CAPABILITY_DENIED` | Cross-workspace leak |
| **Gate 6E-8** | Offline Operation | Network disabled | Telemetry and feedback 100% functional | Network error |
| **Gate 6E-9** | Memory Lease Accounting | 2MB ring buffer allocation | Accounted to workspace lease in `resourced` | Unaccounted allocation |
| **Gate 6E-10**| Daemon Crash Recovery | Kill `observed` process | Restarts; pending prompts resolve CANCEL | Hanging agent or crash |
| **Gate 6E-11**| Synthetic Input Authority | Playback of synthesized plan | `uids` checks `SyntheticInputCap` | Playback without cap |
| **Gate 6E-12**| Zero Capability Grant | Capability grant attempt by `observed` | Rejected by Stage 3H kernel | Capability granted |
| **Gate 6E-13**| Nucleus Modification Gate| Kernel code inspection | 0 bytes modified in Stage 3A–3N | Kernel modified |
| **Gate 6E-14**| Full System Regression | Run all Stage 3A–6D tests | 100% pass across all test suites | Regression failure |

---

## 11. Final Architectural Status

```text
Stage 6E Architecture Rev1       🟢 FROZEN / CLOSED
Stage 3A–3N Kernel Nucleus        🟢 0 BYTES MODIFIED
Stage 6E Implementation Plan      🛑 NOT AUTHORIZED (Pending Approval)
```
