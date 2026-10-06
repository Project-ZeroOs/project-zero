# Stage 6D — Implementation Plan (Rev1)

**Topic:** Human Intent, Intent Resolution & Action/Workflow Boundary Subsystem  
**Status:** 🟢 PLAN DRAFTED — AWAITING AUTHORIZATION — CODE IMPLEMENTATION NOT AUTHORIZED  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen (`intentd`, `agentd`, `workloadd`, `fabricd`, `resourced`)  
- Stage 5 Presentation Subsystem — 🟢 Frozen (`compositord`, `surfaced`, `authui`)  
- Stage 6A User Session Substrate — 🟢 Frozen (`shelld`, `workspaced`)  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  
- Stage 6C Human Input & Interaction Routing — 🟢 Frozen & Committed (`uids`)  
- Stage 6D Architecture Specification Rev1 — 🟢 Frozen  

---

## 1. Executive Summary & Non-Negotiable Constraints

This implementation plan translates the frozen **Stage 6D Architecture Specification Rev1** into sequential, machine-verifiable implementation phases. It establishes exact service changes, ABI structures, pipeline handshakes, test harness integration, and rollback procedures without introducing new architectural primitives, opcodes, or capability types.

### Non-Negotiable Constraints:

1. **Zero Kernel Modifications**: Stage 3A–3N production Ring-0 microkernel nucleus must remain 100% byte-identical (0 bytes modified).
2. **Single Capability Framework**: All authorization relies strictly on Stage 3H capability handles (`SessionManagementCap`, `AgentManagementCap`, `AgentInputCap`, `AccessibilityPolicyCap`, `WorkspaceManagementCap`). No second capability system or new capability type shall be introduced.
3. **Precise Offline Autonomy (`I-INTENT-OFFLINE-AUTONOMY`)**: Intent resolution, agent lifecycle, local workload execution, and local resource management DO NOT require network connectivity. Network is treated strictly as a **workload-level resource dependency**. An intent whose validated execution plan carries an explicit network dependency is deferred, degraded, or rejected when network resources are unavailable, without stalling or crashing local OS operations or local agent loops.
4. **Zero Model Authority (`I-INTENT-MODEL-NON-AUTHORITY`)**: AI/LLM model outputs (whether local or remote) are treated strictly as **untrusted candidate proposals**. Model outputs CANNOT execute system operations directly, CANNOT issue kernel syscalls, and CANNOT bypass capability checks. All model proposals are deterministically validated by `intentd` against the caller's Stage 3H capability table (`caller_cap_table`).
5. **Subordination to Trusted Authorization Service (`authui`)**: `intentd` is the **Intent Interpretation & Structural Plan Compiler Authority**, NOT an authorization authority. When `intentd` detects a Class 3 side-effect step, it requests confirmation through the pre-existing **Trusted Authorization Service (`authui` / `ModalLock`)**. `AuthorizationTransactionRef` remains protocol state, NOT a new capability or `intentd` authority.
6. **Confused-Deputy Prevention (`I-INTENT-CONFUSED-DEPUTY-PREVENTION`)**: `intentd` operates with zero intrinsic authority over user resources. `intentd` validates candidate plans strictly using the caller's pre-existing capability handles. `intentd` daemon privileges are never leveraged to bypass caller authorization.
7. **Seamless Stage 4F Pipeline Handoff**: `intentd` compiles validated expressions directly into the frozen Stage 4F `ExecutionPlan` structure and dispatches via `OP_WORKLOAD_CREATE` (`0x04E1`) to `workloadd`/`agentd` without creating duplicate intent engines or modifying workload abstractions.
8. **Sequential Phase Execution**: Phase $N+1$ shall not begin until Phase $N$ compiles cleanly and passes all intermediate checks.

---

## 2. Phased Implementation Roadmap

```text
Phase 6D.1: libzero Intent ABI & Data Struct Declarations
     │
     ▼
Phase 6D.2: intentd Ingestion, Deterministic Parsing & Capability Validation Engine
     │
     ▼
Phase 6D.3: shelld Command Palette, Hotkeys & Disambiguation UI Integration
     │
     ▼
Phase 6D.4: authui Trusted Authorization Service Integration & Modal Lock Confirmation
     │
     ▼
Phase 6D.5: Stage 4E/4F Execution Pipeline Handoff & Offline Network Resource Degradation
     │
     ▼
Phase 6D.6: workspaced Workflow Persistence & Recovery Integration
     │
     ▼
Phase 6D.7: Machine Verification Harness & Full System Regression
```

---

## 3. Phase Specifications

### Phase 6D.1: `libzero` Intent ABI & Data Struct Declarations
- **Target Files**: `libzero/src/intent.rs`, `libzero/src/lib.rs`, `libzero/src/presentation.rs`.
- **Changes**:
  1. Declare `HumanIntentHeader` (80 bytes, `#[repr(C, packed)]`, Little-Endian):
     - `intent_id: DistributedId` (16 bytes), `session_id: DistributedId` (16 bytes), `workspace_id: DistributedId` (16 bytes), `agent_id: DistributedId` (16 bytes), `timestamp_tsc: u64` (8 bytes), `source_provenance: u16` (2 bytes), `intent_type: u16` (2 bytes), `side_effect_class: u8` (1 byte), `lifecycle_state: u8` (1 byte), `is_network_dependent: u8` (1 byte), `_reserved: u8` (1 byte).
  2. Declare Intent Type constants (`INTENT_TYPE_ONE_SHOT` `0x0001` .. `INTENT_TYPE_WORKFLOW_TEMPLATE` `0x0007`).
  3. Declare Intent Lifecycle State constants (`INTENT_STATE_SUBMITTED` `0x01` .. `INTENT_STATE_FAILED` `0x09`).
  4. Ensure alignment and re-export of frozen Stage 4F `ExecutionPlan` and Stage 3G `IpcMessage` structures.
- **Verification Gate**: `cargo check -p libzero` compiles cleanly with zero warnings/errors.

---

### Phase 6D.2: `intentd` Ingestion, Deterministic Parsing & Capability Validation Engine
- **Target Files**: `intentd/src/main.rs`.
- **Changes**:
  1. Implement `OP_INTENT_SUBMIT` (`0x0709`) IPC message handler.
  2. Implement local deterministic parser engine for text expressions, command strings, and hotkey actions.
  3. Implement Model Non-Authority validation loop (`I-INTENT-MODEL-NON-AUTHORITY`): convert untrusted candidate proposals into structured `ExecutionPlan` steps.
  4. Implement Confused-Deputy capability check (`I-INTENT-CONFUSED-DEPUTY-PREVENTION`): iterate through `steps[i].required_capability` and assert that the caller's capability set (`caller_cap_table`) contains the matching capability handle. If missing, set status `ZeroError::PermissionDenied`.
  5. Implement Workspace Containment validation (`I-INTENT-WORKSPACE-CONTAINMENT`): assert that all target resource IDs and paths belong to `workspace_id`.
  6. Classify side-effects into Class 1 (Read), Class 2 (Scoped Mutate), and Class 3 (Consequential).
- **Verification Gate**: `cargo check -p intentd` compiles cleanly.

---

### Phase 6D.3: `shelld` Command Palette, Hotkeys & Disambiguation UI Integration
- **Target Files**: `shelld/src/main.rs`.
- **Changes**:
  1. Integrate command palette input collector ($Ctrl+Space$) to send `OP_INTENT_SUBMIT` to `intentd`.
  2. Maintain global hotkey table mapping key combinations to `INTENT_TYPE_HOTKEY` expressions.
  3. Implement Disambiguation UI handler: when `intentd` returns `INTENT_STATE_AMBIGUOUS`, `shelld` renders a candidate choice menu to the human user and submits the selected candidate choice back to `intentd`.
- **Verification Gate**: `cargo check -p shelld` compiles cleanly.

---

### Phase 6D.4: `authui` Trusted Authorization Service Integration & Modal Lock Confirmation
- **Target Files**: `authui/src/main.rs`, `intentd/src/main.rs`.
- **Changes**:
  1. Implement Class 3 side-effect confirmation handler in `intentd`: when `side_effect_class == 3`, set intent state to `INTENT_STATE_PENDING_AUTH` and invoke `authui` via IPC.
  2. Implement `authui` Trusted Authorization Service dialog: `authui` requests `ModalLock` from `uids`, renders isolated authorization modal displaying exact plan steps and target paths.
  3. Upon physical human confirmation, `authui` generates `AuthorizationTransactionRef` (16-byte `DistributedId` protocol state) and returns it to `intentd`.
  4. Implement fail-closed crash handling (`I-INTENT-FAIL-CLOSED`): if `authui` crashes or dialog is cancelled, `intentd` immediately invalidates pending transaction and sets intent state to `INTENT_STATE_REJECTED`.
- **Verification Gate**: `cargo check -p authui` compiles cleanly.

---

### Phase 6D.5: Stage 4E/4F Execution Pipeline Handoff & Offline Network Resource Degradation
- **Target Files**: `intentd/src/main.rs`, `agentd/src/main.rs`, `workloadd/src/main.rs`.
- **Changes**:
  1. Implement execution handoff: `intentd` dispatches approved `ExecutionPlan` to `workloadd` via `OP_WORKLOAD_CREATE` (`0x04E1`).
  2. Implement Workload Network Resource Dependency handling (`I-INTENT-OFFLINE-AUTONOMY`):
     - Check `plan.is_network_dependent`.
     - If network is online $\rightarrow$ dispatch compute steps to `fabricd` normally.
     - If network is offline $\rightarrow$ local steps complete normally; network-dependent steps return `ZeroError::TimeAuthorityUnavailable` or `ZeroError::StaleEndpoint` and mark step deferred/degraded without crashing local OS or agent loops.
- **Verification Gate**: `cargo check -p agentd` and `cargo check -p workloadd` compile cleanly.

---

### Phase 6D.6: `workspaced` Workflow Persistence & Recovery Integration
- **Target Files**: `workspaced/src/main.rs`, `intentd/src/main.rs`.
- **Changes**:
  1. Implement `WorkflowSpec` storage in `workspaced` persistent storage authority.
  2. Implement crash recovery: on `intentd` or `workloadd` restart, reload persistent workflow templates from `workspaced` transaction log and resume standing workflows cleanly.
- **Verification Gate**: `cargo check -p workspaced` compiles cleanly.

---

### Phase 6D.7: Machine Verification Harness & Full System Regression
- **Target Files**: `kernel/src/stage4/tests.rs`, `kernel/src/stage4/mod.rs`, `kernel/src/lib.rs`, `tests/test_stage6d.py`.
- **Changes**:
  1. Implement `run_stage6d_verification` test harness in `kernel/src/stage4/tests.rs` covering gates `6D-1` through `6D-14`.
  2. Export `run_stage6d_verification` in `kernel/src/stage4/mod.rs` and invoke from `kernel/src/lib.rs`.
  3. Create Python integration test script `tests/test_stage6d.py` to execute QEMU machine verification and assert clean ISA debug exit code 33 (`0x21`).
  4. Verify PMM frame neutrality (Baseline == Final) and 0 bytes modified in Stage 3A–3N production kernel nucleus.
  5. Run full system regression suite (`python -m unittest discover tests`).
- **Verification Gate**: `test_stage6d.py` passes with exit code 33 (`0x21`), and full test suite passes 100%.

---

## 4. Machine-Verifiable Invariants & Acceptance Gates Map

| Gate ID | Architectural Invariant / Requirement | Verification Phase | Target Criteria |
|---|---|---|---|
| **`6D-1`** | `I-INTENT-OFFLINE-AUTONOMY` | Phase 6D.5 / 6D.7 | Local intent resolution & agent execution pass with network interfaces disabled; network-dependent steps return deferred/degraded state |
| **`6D-2`** | `I-INTENT-MODEL-NON-AUTHORITY` | Phase 6D.2 / 6D.7 | Attempted capability escalation in model candidate proposal returns `PermissionDenied` |
| **`6D-3`** | `I-INTENT-CONFUSED-DEPUTY-PREVENTION` | Phase 6D.2 / 6D.7 | Unprivileged caller intent targeting privileged path returns `PermissionDenied` |
| **`6D-4`** | `I-INTENT-WORKSPACE-CONTAINMENT` | Phase 6D.2 / 6D.7 | Intent targeting foreign workspace without `WorkspaceManagementCap` rejected |
| **`6D-5`** | `I-INTENT-TRUSTED-SIDE-EFFECT-CONFIRMATION` | Phase 6D.4 / 6D.7 | Class 3 intent triggers Trusted Authorization Service (`authui` `ModalLock`) requiring `AuthorizationTransactionRef` |
| **`6D-6`** | `I-INTENT-FAIL-CLOSED` | Phase 6D.4 / 6D.7 | Pending Class 3 intent cancelled immediately on `authui` crash |
| **`6D-7`** | Ambiguous Intent Blocking | Phase 6D.3 / 6D.7 | Ambiguous expression blocks execution and renders `shelld` candidate menu |
| **`6D-8`** | Workflow Persistence & Recovery | Phase 6D.6 / 6D.7 | `WorkflowSpec` restored from `workspaced` after `intentd` restart |
| **`6D-9`** | Source Provenance Integrity | Phase 6D.2 / 6D.7 | `InputSource` provenance preserved accurately from `uids` to `ExecutionPlan` |
| **`6D-10`**| Agent Proposal Capability Bound | Phase 6D.2 / 6D.7 | Agent intent proposal verified against `AgentInputCap` and workspace caps |
| **`6D-11`**| CSDT Non-Authority Verification | Phase 6D.2 / 6D.7 | Remote peer intent without local capability rejected despite valid CSDT |
| **`6D-12`**| Resource Lease Bounds | Phase 6D.2 / 6D.7 | Intent parsing memory allocation enforced within `resourced` quota |
| **`6D-13`**| PMM Frame Neutrality | Phase 6D.7 | 100% physical frame count equality before and after Stage 6D suite |
| **`6D-14`**| Microkernel Substrate Preservation | Phase 6D.7 | 0 bytes modified in Stage 3A–3N production kernel nucleus |

---

## 5. Summary of Deliverables

- `libzero/src/intent.rs`: `HumanIntentHeader`, type & state constants.
- `intentd/src/main.rs`: Intent ingestion, deterministic parser, Model Non-Authority validator, Confused-Deputy checker, Class 3 side-effect classifier, Stage 4F plan handoff engine.
- `shelld/src/main.rs`: Command palette ingestion, hotkey mapper, disambiguation menu UI.
- `authui/src/main.rs`: Trusted Authorization Service dialog & transaction ref emitter.
- `agentd`, `workloadd`: Workload execution handoff & offline network resource dependency handling.
- `workspaced`: `WorkflowSpec` persistence engine.
- `kernel/src/stage4/tests.rs` & `tests/test_stage6d.py`: Machine verification gates `6D-1` through `6D-14`.
