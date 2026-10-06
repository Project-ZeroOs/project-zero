# STAGE 6E — DISCOVERY & DEPENDENCY AUDIT

**Status:** Complete  
**Stage:** 6E  
**Subsystem:** Human-Agent Telemetry, Interactive Feedback & Workflow Synthesis Subsystem (`observed`)  
**Verdict:** 🟢 COMPLETE — READY FOR ARCHITECTURE  

---

## 1. Executive Summary

This document performs the architectural discovery and dependency audit for **Stage 6E**, following the completion and verification of Stage 6A (User Session Substrate), Stage 6B (Distributed Spatial Presentation), Stage 6C (Human Input Routing), and Stage 6D (Human Intent & Workflow Boundary).

### Core Finding
Following Stage 6D, ZeroOS can parse raw human inputs into `HumanIntent` and dispatch structured `ExecutionPlan` graphs to Stage 4F `workloadd` / `agentd`. However, a critical operational gap remains in the human-computer interaction loop: **the system lacks a dedicated, unified substrate for real-time workflow telemetry observation, interactive mid-flight human feedback (human-in-the-loop steering), and user-demonstration workflow synthesis.**

Existing services cannot satisfy this requirement without violating their frozen single-responsibility boundaries:
- `shelld` (Stage 6A) owns spatial windowing and layout presentation; it is not a telemetry aggregator or feedback router.
- `uids` (Stage 6C) owns low-level input event routing; it has no semantic awareness of running agent DAGs or workflow state.
- `intentd` (Stage 6D) owns intent parsing and handoff; once an `ExecutionPlan` is dispatched to Stage 4F, `intentd` is stateless relative to runtime execution.
- `agentd` / `workloadd` (Stage 4E/4F) own process execution and DAG scheduling; they lack a unified human-facing interaction, ambient notification, and feedback routing interface.
- `authui` (Stage 5) presents synchronous, trusted security dialogs for Class-3 security elevation; it is not designed for asynchronous agent progress telemetry or non-security interactive feedback (Class-1/Class-2 parameters).

Stage 6E fills this gap by introducing `observed` (**Human-Agent Telemetry & Interactive Feedback Daemon**), establishing a clean, secure, and non-blocking human-agent interaction loop.

---

## 2. Current Frozen System Boundary

Stage 6E builds strictly on top of the immutable contracts established in prior stages:

```text
Stage 3A–3N Kernel Nucleus
  ├── Capability System (Stage 3H) — Sole capability authority
  ├── IPC System (Stage 3E) — Asynchronous capability-checked IPC
  ├── Filesystem / VFS (Stage 3I) — Resource containment
  └── Process / Device / Network / SMP (Stage 3C/3D/3J/3M)

Stage 4A–4F Core System Infrastructure
  ├── resourced (Stage 4B) — Resource Graph & Lease Accounting
  ├── workspaced (Stage 4D) — Workspace persistence & isolation
  ├── agentd / workloadd (Stage 4E/4F) — Workload DAG execution & Task scheduling
  └── Human Intent / ExecutionPlan (Stage 4F) — Structured workflow representation

Stage 5 Spatial Presentation & Security
  ├── compositord / surfaced (Stage 5) — Compositing & spatial presentation
  └── authui (Stage 5) — Trusted security authorization path

Stage 6A–6D Human Interaction Stack
  ├── shelld (Stage 6A) — Session, layout, focus policy
  ├── CSDT (Stage 6B) — Distributed spatial presentation
  ├── uids (Stage 6C) — Authoritative input event observation & routing
  └── intentd (Stage 6D) — Intent interpretation & ExecutionPlan handoff
```

**Boundary Invariant:** Stage 6E does not modify any frozen contract in Stages 3A–6D. Zero kernel code (Stage 3A–3N) is altered (0 bytes modified).

---

## 3. Human Computer Capability Map

The complete human-computer interaction lifecycle in ZeroOS is mapped below:

```text
  [ Human Input ]
         │
         ▼
  Stage 6C: uids (Input Observation & Provenance Routing)
         │
         ▼
  Stage 6D: intentd (Intent Parsing & ExecutionPlan Handoff)
         │
         ▼
  Stage 4F: workloadd / agentd (Workload DAG Execution)
         │
 ┌───────┴────────────────────────────────────────┐
 │ Stage 6E: observed (Telemetry & Feedback Loop)│
 └───────┬────────────────────────────────────────┘
         │
         ├──► Ambient Workflow Telemetry ───► shelld (Stage 6A rendering)
         ├──► Interactive Feedback Prompts ─► User Response Routing
         └──► Action Session Recording ─────► Reusable ExecutionPlan Synthesis
```

---

## 4. Candidate Missing Capabilities

The discovery process evaluated 18 potential candidate categories for Stage 6E:

| Candidate Category | Status | Architectural Analysis |
|---|---|---|
| Application Lifecycle | 🟢 Satisfied | Managed by Stage 4F `workloadd` / `agentd` |
| Window/Spatial Presentation | 🟢 Satisfied | Managed by Stage 5 `surfaced` and Stage 6A `shelld` |
| Input Event Routing | 🟢 Satisfied | Managed by Stage 6C `uids` |
| Intent Parsing / Handoff | 🟢 Satisfied | Managed by Stage 6D `intentd` |
| Security Elevation | 🟢 Satisfied | Managed by Stage 5 `authui` |
| Network / Remote Compute | 🟢 Satisfied | Managed by Stage 3M / Stage 4B `resourced` |
| Resource Accounting | 🟢 Satisfied | Managed by Stage 4B `resourced` |
| Workspace Persistence | 🟢 Satisfied | Managed by Stage 4D `workspaced` |
| **Real-time Workflow Telemetry** | 🔴 **MISSING** | No service aggregates live DAG progress across workspaces for shell display |
| **Interactive Human Feedback Loop** | 🔴 **MISSING** | No mechanism for running agents to request mid-flight human parameters/choices |
| **Demonstration Action Recording** | 🔴 **MISSING** | No mechanism to capture user UI interactions and synthesize reusable Stage 4F Plans |
| Workflow Editing UX | 🟡 Out of Scope | Belongs to userland tools operating via Stage 4F `ExecutionPlan` ABI |
| Search / Discovery | 🟡 Out of Scope | Belongs to userland workspace indexing workloads |
| System Settings | 🟢 Satisfied | Managed via standard workspace configuration files in Stage 4D `workspaced` |

### Core Missing Capability Definition
The audit identifies three interconnected, missing human-computer interaction capabilities:
1. **Live Workflow Telemetry & Ambient Observation (`Telemetry Stream`)**: Aggregating real-time status, progress percentages, and log output from running Stage 4F Workload DAGs without polling individual processes.
2. **Interactive Human-in-the-Loop Feedback (`Feedback Loop`)**: Providing a safe, non-blocking asynchronous protocol for executing agents to prompt the human user for decision choices, mid-flight input parameters, or confirmation during task execution.
3. **Demonstration Action Recording & Plan Synthesis (`Action Recorder`)**: Capturing authorized user input sequences (`InputEvent` stream from `uids`) and system intent events (`intentd`), sanitizing them, and compiling them into reusable, deterministic Stage 4F `ExecutionPlan` definitions.

---

## 5. Existing-Service Ownership Audit

Every candidate capability was audited against existing frozen daemons to prevent domain creep:

```text
+----------------+-------------------------------------------------------------+-------------------+
| Daemon         | Current Frozen Responsibility                               | Can Own 6E?       |
+----------------+-------------------------------------------------------------+-------------------+
| shelld         | Window layout, workspace spatial presentation, focus policy | ❌ NO (Presentation)|
| intentd        | Ingests raw input, parses Intent, emits ExecutionPlan       | ❌ NO (Input Parser)|
| workloadd      | Schedules Tasks, manages Workload DAG state, handles leases | ❌ NO (Execution)  |
| agentd         | Runs agent worker processes, executes Task steps           | ❌ NO (Worker)     |
| uids           | Captures hardware/remote input, enforces focus routing      | ❌ NO (Raw Input)  |
| authui         | Trusted authorization path for Class-3 security dialogs     | ❌ NO (Security)   |
| workspaced     | Persists workspace directories and metadata bindings        | ❌ NO (Storage)    |
+----------------+-------------------------------------------------------------+-------------------+
```

**Conclusion:** Forcing telemetry aggregation, mid-flight feedback routing, or action recording into any existing daemon would violate their single-responsibility contracts and create coupling between execution (`workloadd`), presentation (`shelld`), input routing (`uids`), and security (`authui`). A dedicated service (`observed`) is architecturally required.

---

## 6. Authority / Capability Audit

Does Stage 6E require creating new Stage 3H capability types?

**Audit Result: NO NEW KERNEL CAPABILITIES REQUIRED.**

Stage 6E operates entirely using existing Stage 3H capability primitives:
- `IpcEndpointCap` (Type `0x0001`): Used for IPC communications between `observed`, `shelld`, `workloadd`, and `uids`.
- `WorkspaceAccessCap` (Type `0x0030`): Enforces workspace scoping for telemetry channels and action recordings.
- `SyntheticInputCap` (Type `0x0043`): Enforces authority checks when playback of synthesized plans is requested.
- `ResourceLeaseCap` (Type `0x0020`): Tracks memory and buffer allocations for active telemetry channels in `resourced`.

**Authority Boundary:** `observed` acts as an unprivileged aggregation daemon within the system service tier. It possesses **zero** ambient kernel capabilities and cannot grant capabilities to processes or agents.

---

## 7. Stage 4F Duplication Audit

Does Stage 6E duplicate any Stage 4F concepts?

```text
Stage 4F Concrete Primitives      Stage 6E Observation Primitives
----------------------------      -------------------------------
HumanIntent                       TelemetryFrame (Status stream)
ExecutionPlan (DAG)               FeedbackPrompt (Interactive choice)
Workload / Task                   ActionSession (Recorded event log)
ResourceLease                     TelemetryLease (Resource consumption)
```

**Duplication Check:**
- `observed` does NOT duplicate `ExecutionPlan` parsing or DAG compilation (owned by `intentd`).
- `observed` does NOT schedule tasks or execute processes (owned by `workloadd` / `agentd`).
- `observed` does NOT manage resource leases (owned by `resourced`).
- `observed` merely *subscribes* to status events emitted by `workloadd` and routes feedback prompts between agents and `shelld`.

---

## 8. Offline / Network Dependency Audit

ZeroOS product thesis mandates that agents and core OS functions must operate seamlessly without network connectivity.

### Offline Operating Matrix

| System Condition | Telemetry Behavior | Feedback Loop Behavior | Action Recording Behavior |
|---|---|---|---|
| Network Available | Local & remote node telemetry aggregated | Prompts routed to local session | Local & remote actions recorded |
| Network Unavailable | Local node telemetry aggregated seamlessly | Prompts routed to local session | Local actions recorded seamlessly |
| Remote Node Offline | Remote telemetry marked STALE | Remote agent prompts marked FAILED | Remote recording paused cleanly |
| Local Resources Exhausted | Telemetry ring buffers shed old frames | Prompts fail-closed (DEFAULT_CANCEL) | Action recording terminated cleanly |

**Offline Invariant:** `observed` operates with **zero network dependencies**. Network is treated purely as a workload/transport dependency managed by Stage 3M / Stage 4B `resourced`.

---

## 9. AI / Model Authority Audit

ZeroOS product thesis mandates: **AI/Model outputs are untrusted proposals and must never acquire OS authority.**

### Model Boundary Pipeline

```text
Model Output (Untrusted Proposal)
       │
       ▼
[ Feedback Prompt / Synthesized Workflow Plan ]
       │
       ▼
Deterministic Schema Validation (libzero ABI)
       │
       ▼
Stage 6D intentd Validation (Capabilities & Boundary Check)
       │
       ▼
Stage 5 authui / Human User Explicit Approval (If Class-2/3)
       │
       ▼
Stage 4F ExecutionPlan Dispatch (Validated Authority)
```

**Rules:**
1. A model cannot issue a `FeedbackPrompt` that bypasses `authui` for Class-3 security actions.
2. Synthesized action recordings proposed by AI models must pass through `intentd` compilation and deterministic validation before execution.
3. Models possess 0 capabilities and cannot grant rights to `observed` or any workload.

---

## 10. Persistence Audit

### State Survival Matrix

| State Type | Process Restart | Service Restart | Session Restart | Machine Reboot | Network Loss |
|---|---|---|---|---|---|
| Active Telemetry Ring Buffer | ❌ Lost | ❌ Lost | ❌ Lost | ❌ Lost | 🟢 Retained |
| Pending Feedback Prompt | ❌ Lost | ❌ Fail-Closed | ❌ Fail-Closed | ❌ Lost | 🟢 Retained |
| Action Recording Buffer | 🟢 Persisted | 🟢 Persisted | 🟢 Persisted | 🟢 Persisted | 🟢 Retained |
| Synthesized `.plan` Artifact | 🟢 Persisted | 🟢 Persisted | 🟢 Persisted | 🟢 Persisted | 🟢 Retained |

**Storage Location:** All persistent state (recorded action streams and synthesized `.plan` files) is stored in the workspace directory managed by Stage 4D `workspaced` (`/workspace/workflows/`). Ephemeral IPC sockets, ring buffers, and handles are never persisted across reboots.

---

## 11. Failure & Recovery Audit

The failure matrix for `observed` is defined below:

| Failure Mode | Impact | Recovery Protocol |
|---|---|---|
| `observed` crash | Telemetry stream interrupted | Daemon restarted by init; re-queries `workloadd` active DAG state table |
| `workloadd` task crash | Telemetry frame emits `TASK_FAILED` | `observed` broadcasts task failure status to `shelld` |
| `shelld` session crash | Feedback UI unrendered | Prompts held in `observed` buffer until session reconnects or times out |
| Memory exhaustion | Ring buffer overflow | Ephemeral log frames dropped; essential task status frames retained |
| Malformed feedback input | User response parse error | Feedback prompt rejected; agent receives `FEEDBACK_ERR_INVALID` |

**Fail-Closed Rule:** If a feedback prompt times out or `observed` crashes while a prompt is pending, the prompt automatically resolves to `FEEDBACK_RESPONSE_CANCEL` (fail-closed), ensuring agents do not hang indefinitely or execute unauthorized actions.

---

## 12. Resource Accounting Audit

All resource consumption by `observed` is visible to and accounted by Stage 4B `resourced`:

- **CPU:** Process execution accounted under system service accounting group (`sys_service_group`).
- **Memory:** Telemetry ring buffers and prompt queues use fixed-size pre-allocated buffers (max 2 MB per active session).
- **Storage:** Action session recordings write directly to workspace storage bindings under workspace quota.
- **GPU/NPU:** 0 bytes / 0 cycles used by `observed`.

---

## 13. Security & Confused-Deputy Audit

### Security Threat Analysis & Mitigation

1. **Confused Deputy Prevention:**
   - Problem: An unprivileged process asks `observed` to route a feedback prompt impersonating a system agent.
   - Mitigation: `observed` verifies caller identity via Stage 3E IPC process credentials and requires an active `TaskExecutionCap` for the corresponding task.

2. **Capability Amplification:**
   - Problem: Synthesizing an action recording creates an `ExecutionPlan` with elevated rights.
   - Mitigation: Action recorder strips runtime capabilities from recorded events. Synthesized plans receive only the capabilities explicitly assigned during `intentd` handoff.

3. **Cross-Workspace Telemetry Leakage:**
   - Problem: Workspace A views telemetry streams or task logs from Workspace B.
   - Mitigation: Telemetry streams are strictly isolated by `WorkspaceAccessCap`. `observed` drops any subscription request lacking valid workspace capability.

4. **Synthetic Input Impersonation:**
   - Problem: Action recorder replay injects synthetic input into `uids` bypassing security locks.
   - Mitigation: Replay requires explicit `SyntheticInputCap` (Stage 6C invariant preserved).

---

## 14. Product Differentiation Analysis

Audit against the core ZeroOS Product Thesis:

> ZeroOS is a computer that manages the complexity of computing for the user, rather than making the user manage the computer.

### Concrete User Scenarios Enabled by Stage 6E

1. **Ambient Automation Observation:**
   - User command: *"Summarize my unread documents in the background."*
   - Desktop OS experience: User opens a terminal or application window, polls for progress, or loses track of background execution.
   - ZeroOS Stage 6E experience: Background agent executes in `workloadd`; `observed` streams subtle, ambient progress indicators directly to `shelld` status bar without cluttering workspace focus.

2. **Interactive Human-in-the-Loop Steering:**
   - Scenario: An agent compiling a complex report encounters two conflicting source files.
   - ZeroOS Stage 6E experience: Agent emits a non-blocking `FeedbackPrompt`. `shelld` displays an unobtrusive card asking *"Which source file should take priority?"* User clicks option; agent resumes execution without process termination or restart.

3. **Demonstration Action Recording & Workflow Reuse:**
   - User scenario: *"Every Monday, I format this data table, convert it to PDF, and place it in the weekly folder."*
   - ZeroOS Stage 6E experience: User clicks "Record Action", performs the steps once. `uids` and `observed` capture the interaction sequence, synthesize a deterministic Stage 4F `ExecutionPlan`, and register a custom `HumanIntent` with `intentd`. Future executions occur automatically with a single command.

---

## 15. Proposed 6E Boundary

The proposed Stage 6E boundary sits strictly between human presentation (`shelld`), task execution (`workloadd`), intent parsing (`intentd`), and input observation (`uids`):

```text
                               ┌─────────────────────────┐
                               │     Human Session       │
                               │      shelld (6A)        │
                               └────────────┬────────────┘
                                            │ Telemetry / Feedback UI
                                            ▼
┌────────────────────────┐     ┌─────────────────────────┐     ┌─────────────────────────┐
│  Input Observation     │────►│     Stage 6E:           │◄────│    Task Execution       │
│      uids (6C)         │     │     observed            │     │    workloadd (4F)       │
└────────────────────────┘     └────────────┬────────────┘     └─────────────────────────┘
                                            │ Recorded Session / Synthesized Plan
                                            ▼
                               ┌─────────────────────────┐
                               │   Intent Interpreter    │
                               │     intentd (6D)        │
                               └─────────────────────────┘
```

---

## 16. Proposed Service Ownership

The Stage 6E architecture introduces **one new system service**:

### `observed` (Human-Agent Telemetry & Interactive Feedback Daemon)
- **Primary Function:** Aggregates real-time workflow status streams, manages asynchronous human feedback prompts, and records user demonstration sessions for workflow synthesis.
- **Service Tier:** Unprivileged System Daemon (Stage 4).
- **Dependencies:** Stage 3E IPC, Stage 4B `resourced`, Stage 4D `workspaced`, Stage 4F `workloadd`, Stage 6A `shelld`, Stage 6C `uids`, Stage 6D `intentd`.

---

## 17. Proposed Data / IPC Boundaries

Stage 6E defines three primary IPC structures (64-byte aligned ABI):

### 1. `TelemetryFrame` (64 bytes)
Emitted by `workloadd` / `agentd` to `observed` for ambient status reporting.
```rust
#[repr(C, align(64))]
pub struct TelemetryFrame {
    pub frame_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub timestamp_tsc: u64,
    pub progress_pct: u8,
    pub status_code: u8,
    pub reserved: [u8; 6],
    pub status_message: [u8; 32],
}
```

### 2. `FeedbackPrompt` (64 bytes)
Emitted by executing agents to `observed` to solicit human feedback.
```rust
#[repr(C, align(64))]
pub struct FeedbackPrompt {
    pub prompt_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub timeout_ms: u32,
    pub prompt_type: u8, // 1 = Choice, 2 = Confirmation, 3 = Input String
    pub default_action: u8, // 0 = Fail-closed / Cancel
    pub reserved: [u8; 2],
    pub prompt_title: [u8; 32],
}
```

### 3. `ActionRecordHeader` (64 bytes)
Used when recording user input interaction sessions for workflow synthesis.
```rust
#[repr(C, align(64))]
pub struct ActionRecordHeader {
    pub session_id: u64,
    pub workspace_id: u64,
    pub start_timestamp_tsc: u64,
    pub event_count: u32,
    pub status: u8, // 1 = Recording, 2 = Stopped, 3 = Synthesized
    pub reserved: [u8; 27],
}
```

---

## 18. Candidate Invariants

The following machine-verifiable invariants are proposed for Stage 6E:

1. **`I-6E-NO-AUTHORITY-AMPLIFICATION`**: `observed` cannot create, elevate, or grant Stage 3H capabilities to any process, agent, or user.
2. **`I-6E-WORKSPACE-CONTAINMENT`**: Telemetry streams, feedback prompts, and action recordings are strictly isolated by workspace ID and verified via `WorkspaceAccessCap`.
3. **`I-6E-OFFLINE-SAFETY`**: All `observed` IPC functions and telemetry aggregation operate deterministically without network sockets or external service dependencies.
4. **`I-6E-MODEL-NO-AUTHORITY`**: AI models cannot bypass schema validation or issue Class-3 security prompts directly through `observed`.
5. **`I-6E-NO-STAGE4F-DUPLICATION`**: `observed` does not maintain task execution state, perform DAG scheduling, or allocate resource leases.
6. **`I-6E-RESOURCE-ACCOUNTING`**: Memory buffers for telemetry streams and action recording logs are accounted to the host workspace session in `resourced`.
7. **`I-6E-FAIL-CLOSED-FEEDBACK`**: Any feedback prompt that times out, disconnects, or encounters a service crash must resolve to `FEEDBACK_RESPONSE_CANCEL`.

---

## 19. Candidate Machine Acceptance Gates

Stage 6E requires 14 machine-verifiable acceptance gates:

| Gate ID | Assertion Description | Boundary Tested | Expected Result | Failure Condition |
|---|---|---|---|---|
| **Gate 6E-1** | ABI Layout & Alignment | `TelemetryFrame` / `FeedbackPrompt` ABI | Exactly 64 bytes, 64-byte aligned | Layout padding mismatch |
| **Gate 6E-2** | Telemetry Stream Aggregation | `workloadd` ──► `observed` ──► `shelld` | Real-time status frame delivered < 1ms | Dropped or delayed frame |
| **Gate 6E-3** | Feedback Prompt Fail-Closed | Prompt timeout / disconnect | Resolves to `FEEDBACK_RESPONSE_CANCEL` | Agent blocks or executes |
| **Gate 6E-4** | Workspace Containment | Cross-workspace telemetry read attempt | Rejected with `ERR_CAPABILITY_DENIED` | Data leakage across workspaces |
| **Gate 6E-5** | Offline Operation | Network interface disabled | Telemetry and feedback 100% functional | Network dependency error |
| **Gate 6E-6** | Capability Non-Amplification | Attempt by `observed` to grant capability | Rejected by Stage 3H kernel | Capability granted |
| **Gate 6E-7** | Action Recording Capture | `uids` event stream capture | Events captured cleanly in order | Missing or reordered events |
| **Gate 6E-8** | Plan Synthesis Handoff | Synthesized plan ──► `intentd` | Validated `ExecutionPlan` generated | Malformed plan accepted |
| **Gate 6E-9** | Memory Buffer Accounting | Allocation of 2MB ring buffer | Visible in `resourced` workspace lease | Unaccounted memory allocation |
| **Gate 6E-10**| Service Crash Recovery | Kill `observed` during DAG execution | `observed` restarts, recovers DAG state | Lost DAG tracking |
| **Gate 6E-11**| Synthetic Input Authority | Playback of recorded session | Requires `SyntheticInputCap` | Playback without capability |
| **Gate 6E-12**| Model Non-Authority Check | AI model issuing Class-3 prompt | Intercepted and routed to `authui` | Direct prompt execution |
| **Gate 6E-13**| Nucleus Modification Gate | Kernel code inspection | 0 bytes modified in Stage 3A–3N | Kernel modified |
| **Gate 6E-14**| Full System Regression | Run all Stage 3A–6D tests | 100% pass across all test suites | Regression failure |

---

## 20. Unresolved Questions

1. **Action Sanitization Policy:** When recording human interaction sessions for workflow synthesis, how should sensitive inputs (e.g., credentials typed into fields) be automatically detected and redacted before compiling the Stage 4F `ExecutionPlan`? *(To be resolved in Stage 6E Architecture Rev1).*
2. **Telemetry Ring Buffer Sizing:** What is the optimal per-workspace ring buffer size for high-frequency agent logs to balance memory footprint in `resourced` against log retention depth? *(Default proposed: 64 KB per active DAG, maximum 2 MB per workspace).*

---

## 21. Architectural Risks

1. **Log Flooding:** High-frequency task execution in `workloadd` could flood `observed` with telemetry IPC frames, starving CPU resources.
   - *Mitigation:* Implement rate-limiting and lossy log-level filtering at `workloadd` source, while keeping status code updates lossy-free.
2. **Unattended Agent Stalls:** Agents requiring feedback might stall indefinitely if the human user ignores a non-critical prompt.
   - *Mitigation:* Mandatory timeout values for all `FeedbackPrompt` instances; automatic fail-closed resolution on expiry.

---

## 22. Recommendation & Final Discovery Verdict

### Summary Recommendation
The discovery audit confirms that a genuine, necessary capability gap exists in ZeroOS following Stage 6D: **Real-Time Telemetry Aggregation, Interactive Human Feedback Loop, and Action Session Recording.**

Existing services (`shelld`, `intentd`, `workloadd`, `uids`, `authui`) cannot fulfill these needs without violating their frozen architecture boundaries. Creating `observed` (Human-Agent Telemetry & Interactive Feedback Daemon) cleanly addresses these requirements while strictly preserving all Stage 3A–6D contracts.

---

### FINAL DISCOVERY VERDICT

```text
🟢 COMPLETE — READY FOR ARCHITECTURE
```
