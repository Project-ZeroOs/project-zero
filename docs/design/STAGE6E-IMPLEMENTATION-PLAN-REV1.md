# STAGE 6E — IMPLEMENTATION PLAN (REV1)

**Topic:** Human-Agent Telemetry, Interactive Feedback & Workflow Synthesis Subsystem (`observed`)  
**Status:** 🟢 PLAN DRAFTED — AWAITING AUTHORIZATION — CODE IMPLEMENTATION NOT AUTHORIZED  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen (0 bytes modified)  
- Stage 4A–4F Core System Services — 🟢 Frozen (`workloadd`, `agentd`, `resourced`, `workspaced`)  
- Stage 5 Presentation Subsystem — 🟢 Frozen (`compositord`, `surfaced`, `authui`)  
- Stage 6A User Session Substrate — 🟢 Frozen (`shelld`)  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  
- Stage 6C Human Input Routing — 🟢 Frozen & Committed (`uids`)  
- Stage 6D Human Intent Interpreter — 🟢 Frozen & Committed (`intentd`)  
- Stage 6E Architecture Specification Rev1 — 🟢 Frozen  

---

## 1. Executive Summary & Non-Negotiable Constraints

This implementation plan translates the frozen **Stage 6E Architecture Specification Rev1** into sequential, machine-verifiable implementation phases. It establishes exact service changes, ABI structures, pipeline handshakes, test harness integration, and verification procedures without introducing new architectural primitives, opcodes, or capability types.

### Non-Negotiable Implementation Constraints:

1. **Zero Kernel Modifications**: Stage 3A–3N production Ring-0 microkernel nucleus must remain 100% byte-identical (0 bytes modified).
2. **Zero New Capability Types**: All authorization relies strictly on existing Stage 3H capability handles (`IpcEndpointCap`, `WorkspaceAccessCap`, `SyntheticInputCap`, `ResourceLeaseCap`). No second capability system or new capability type shall be introduced.
3. **`observed` Coordinator Non-Authority**: `observed` is an unprivileged telemetry broker and interaction coordinator. It possesses **zero execution authority**, **zero capability creation authority**, **zero resource override authority**, and **zero intent translation authority**.
4. **Deterministic Fail-Closed Timeout Semantics (`I-6E-FAIL-CLOSED-FEEDBACK`)**: Unanswered, timed-out, or disconnected feedback prompts automatically resolve to `FEEDBACK_RESPONSE_CANCEL` (0x00). Agents do not block indefinitely.
5. **Mandatory Sensitive-Input Scrubbing (`I-6E-RECORDING-SANITATION`)**: All events tagged with `INPUT_FLAG_SENSITIVE` (0x80) from `uids` are scrubbed and discarded prior to action trace generation. Keystrokes in non-sensitive text fields are parameterized (`${PARAM_N}`).
6. **Recording Proposal Handoff (`I-6E-RECORDING-PROPOSAL-ONLY`)**: Action recordings cannot execute directly. Execution requires compilation by `intentd` into a Stage 4F `ExecutionPlan` via IPC opcode `OP_INTENT_COMPILE_PROPOSAL` (`0x0720`).
7. **Class-3 Security Interception**: Any prompt attempting to perform Class-3 security actions or privilege elevation via `observed` is intercepted and rejected (`ERR_SECURITY_CLASS_VIOLATION`). Class-3 security consent executes strictly via Stage 5 `authui`.
8. **Offline Operation (`I-6E-OFFLINE-SAFETY`)**: All telemetry aggregation, feedback prompt routing, and demonstration recording operate 100% locally without network dependencies.
9. **Sequential Phase Execution**: Phase $N+1$ shall not begin until Phase $N$ compiles cleanly and passes all intermediate checks.

---

## 2. Phased Implementation Roadmap

```text
Phase 6E.1: libzero Telemetry, Feedback & Recording ABI Definitions
     │
     ▼
Phase 6E.2: observed Core Daemon, Telemetry Aggregation & Workspace Ring Buffer Broker
     │
     ▼
Phase 6E.3: Interactive Feedback Loop & Fail-Closed Timeout Engine
     │
     ▼
Phase 6E.4: Demonstration Action Recorder & Sensitive-Input Scrubbing Engine
     │
     ▼
Phase 6E.5: Workflow Proposal Compiler & intentd Handoff Integration
     │
     ▼
Phase 6E.6: Persistence (WAL / Storage) & Service Recovery Integration
     │
     ▼
Phase 6E.7: Integration Test Suite & Machine Acceptance Gates 6E-1…6E-14 Verification
```

---

## 3. Detailed Phase Specifications

### Phase 6E.1: `libzero` Telemetry, Feedback & Recording ABI Definitions
- **Target Files**: `libzero/src/observed.rs`, `libzero/src/lib.rs`, `libzero/src/ipc.rs`.
- **Changes**:
  1. Declare `TelemetryFrame` struct (64 bytes, `#[repr(C, align(64))]`):
     - `frame_id: u64`, `task_id: u64`, `workspace_id: u64`, `timestamp_tsc: u64`, `progress_pct: u8`, `telemetry_type: u8`, `status_code: u16`, `reserved: [u8; 4]`, `status_message: [u8; 32]`.
  2. Declare `FeedbackPrompt` struct (64 bytes, `#[repr(C, align(64))]`):
     - `prompt_id: u64`, `task_id: u64`, `workspace_id: u64`, `timeout_ms: u32`, `prompt_class: u8`, `default_action: u8`, `reserved: [u8; 2]`, `prompt_title: [u8; 32]`.
  3. Declare `FeedbackResponse` struct (64 bytes, `#[repr(C, align(64))]`):
     - `prompt_id: u64`, `task_id: u64`, `workspace_id: u64`, `response_status: u8`, `selected_option: u8`, `reserved: [u8; 6]`, `response_payload: [u8; 40]`.
  4. Declare `ActionRecordHeader` struct (64 bytes, `#[repr(C, align(64))]`).
  5. Define IPC opcodes:
     - `OP_OBSERVED_SUBSCRIBE_TELEMETRY` (`0x0801`)
     - `OP_OBSERVED_EMIT_TELEMETRY` (`0x0802`)
     - `OP_OBSERVED_EMIT_PROMPT` (`0x0803`)
     - `OP_OBSERVED_RESPOND_PROMPT` (`0x0804`)
     - `OP_OBSERVED_START_RECORDING` (`0x0805`)
     - `OP_OBSERVED_STOP_RECORDING` (`0x0806`)
     - `OP_INTENT_COMPILE_PROPOSAL` (`0x0720`)
  6. Add compile-time size and alignment assertions (`const_assert!(core::mem::size_of::<TelemetryFrame>() == 64)`).
- **Verification Gate**: `cargo check -p libzero` compiles cleanly with zero warnings.

---

### Phase 6E.2: `observed` Core Daemon, Telemetry Aggregation & Ring Buffer Broker
- **Target Files**: `observed/Cargo.toml`, `observed/src/main.rs`, `observed/src/telemetry.rs`, `workloadd/src/main.rs`.
- **Changes**:
  1. Create `observed` daemon crate in user workspace.
  2. Implement workspace telemetry ring buffer manager (64 KB per DAG, 2 MB max per workspace session) registered with `resourced`.
  3. Implement `OP_OBSERVED_SUBSCRIBE_TELEMETRY` handler: validate caller's `WorkspaceAccessCap` against target workspace ID; register subscriber channel.
  4. Implement `OP_OBSERVED_EMIT_TELEMETRY` handler in `workloadd`: broadcast task status changes and progress updates to `observed`.
  5. Implement ring buffer drop policy: if buffer overflows, drop oldest log frames (`telemetry_type == 3`), retain critical status updates (`telemetry_type == 1` or `5` or `6`).
- **Verification Gate**: `cargo check -p observed` compiles cleanly.

---

### Phase 6E.3: Interactive Feedback Loop & Fail-Closed Timeout Engine
- **Target Files**: `observed/src/feedback.rs`, `shelld/src/main.rs`.
- **Changes**:
  1. Implement `OP_OBSERVED_EMIT_PROMPT` handler in `observed`: validate task ownership and caller `WorkspaceAccessCap`.
  2. Class-3 Security Interception: check if prompt requests security escalation or Class-3 consent. If true, reject immediately with `ERR_SECURITY_CLASS_VIOLATION`.
  3. Route valid Class-1/Class-2 prompts to active `shelld` session.
  4. Implement asynchronous timer reactor: track `timeout_ms` for active prompts.
  5. Implement Fail-Closed Timeout Handler: if prompt timer expires before receiving `OP_OBSERVED_RESPOND_PROMPT`, emit synthetic `FeedbackResponse` with `response_status = FEEDBACK_RESPONSE_CANCEL` (0x00) and notify agent.
- **Verification Gate**: `cargo check -p observed -p shelld` compiles cleanly.

---

### Phase 6E.4: Demonstration Action Recorder & Sensitive-Input Scrubbing Engine
- **Target Files**: `observed/src/recorder.rs`, `uids/src/main.rs`.
- **Changes**:
  1. Implement `OP_OBSERVED_START_RECORDING` and `OP_OBSERVED_STOP_RECORDING` IPC handlers in `observed`.
  2. Request audited event tap from `uids` using `WorkspaceAccessCap`.
  3. Implement Sensitive-Input Scrubbing Filter:
     - Check `InputEvent.flags` for `INPUT_FLAG_SENSITIVE` (`0x80`).
     - Immediately discard any event marked sensitive.
     - Scrub string payloads from password fields, security boxes, and confidential surfaces.
  4. Implement Text Parameterizer: convert literal text keystrokes into structured placeholders (`${PARAM_1}`, `${PARAM_2}`).
  5. Serialize sanitized event stream into `ActionTrace` structure.
- **Verification Gate**: `cargo check -p observed -p uids` compiles cleanly.

---

### Phase 6E.5: Workflow Proposal Compiler & `intentd` Handoff Integration
- **Target Files**: `observed/src/compiler.rs`, `intentd/src/main.rs`.
- **Changes**:
  1. Implement `WorkflowProposal` generator in `observed`: package `ActionTrace` into unvalidated JSON/binary workflow proposal.
  2. Implement IPC dispatch `OP_INTENT_COMPILE_PROPOSAL` (`0x0720`) from `observed` to `intentd`.
  3. Implement proposal compilation handler in `intentd`:
     - Validate proposed steps against allowed `HumanIntent` schemas.
     - Assert workspace capability eligibility.
     - Synthesize validated Stage 4F `ExecutionPlan` structure.
     - Register compiled plan in `workspaced` registry.
  4. Assert that `observed` never dispatches plans directly to `workloadd` (preserves execution authority in `intentd`).
- **Verification Gate**: `cargo check -p observed -p intentd` compiles cleanly.

---

### Phase 6E.6: Persistence (WAL / Storage) & Service Recovery Integration
- **Target Files**: `observed/src/persistence.rs`, `workspaced/src/main.rs`.
- **Changes**:
  1. Implement WAL writer in `observed` flushing active action recordings to `/workspace/recordings/` directory via `workspaced`.
  2. Implement daemon recovery sequence on launch:
     - Query `workloadd` for active DAG status tables via `OP_WORKLOAD_QUERY_ACTIVE_DAGS`.
     - Re-subscribe to active telemetry streams.
     - Flush all un-responded feedback prompts from pre-crash session to `FEEDBACK_RESPONSE_CANCEL`.
     - Mark interrupted WAL recordings as `RECORDING_STATUS_INTERRUPTED`.
- **Verification Gate**: `cargo check -p observed -p workspaced` compiles cleanly.

---

### Phase 6E.7: Integration Test Suite & Machine Acceptance Gates Verification
- **Target Files**: `tests/stage6e_gates.rs`, `kernel/src/stage4/tests.rs`.
- **Changes**:
  1. Write automated test suite exercising all 14 machine acceptance gates (**Gate 6E-1** to **Gate 6E-14**).
  2. Verify 64-byte ABI alignment and layout assertions (Gate 6E-1).
  3. Verify telemetry frame delivery latency < 1ms (Gate 6E-2).
  4. Verify feedback prompt fail-closed timeout resolution (Gate 6E-3).
  5. Verify Class-3 prompt interception (Gate 6E-4).
  6. Verify sensitive input scrubbing (Gate 6E-5).
  7. Verify proposal handoff to `intentd` (Gate 6E-6).
  8. Verify workspace containment isolation (Gate 6E-7).
  9. Verify 100% offline operation without network sockets (Gate 6E-8).
  10. Verify memory ring buffer accounting in `resourced` (Gate 6E-9).
  11. Verify daemon crash recovery (Gate 6E-10).
  12. Verify synthetic input playback authority check in `uids` (Gate 6E-11).
  13. Verify zero capability creation by `observed` (Gate 6E-12).
  14. Verify zero bytes modified in Stage 3A–3N kernel nucleus (Gate 6E-13).
  15. Run full system regression suite (Gate 6E-14).
- **Verification Gate**: QEMU test runner passes with exit code 33, 100% regression pass, 0 bytes kernel modified.

---

## 4. Machine Acceptance Gate Mapping

| Gate ID | Target Verification Test | Command | Success Criteria |
|---|---|---|---|
| **Gate 6E-1** | `test_6e_abi_alignment` | `cargo test --test stage6e_gates test_6e_abi_alignment` | All structs 64 bytes, 64-byte aligned |
| **Gate 6E-2** | `test_6e_telemetry_delivery` | `cargo test --test stage6e_gates test_6e_telemetry_delivery` | Status frame delivered < 1ms |
| **Gate 6E-3** | `test_6e_feedback_timeout` | `cargo test --test stage6e_gates test_6e_feedback_timeout` | Resolves to `FEEDBACK_RESPONSE_CANCEL` |
| **Gate 6E-4** | `test_6e_class3_interception` | `cargo test --test stage6e_gates test_6e_class3_interception` | Returns `ERR_SECURITY_CLASS_VIOLATION` |
| **Gate 6E-5** | `test_6e_sensitive_scrubbing`| `cargo test --test stage6e_gates test_6e_sensitive_scrubbing` | Sensitive event dropped; secret scrubbed |
| **Gate 6E-6** | `test_6e_proposal_handoff` | `cargo test --test stage6e_gates test_6e_proposal_handoff` | Proposal sent to `intentd` via `0x0720` |
| **Gate 6E-7** | `test_6e_workspace_containment`| `cargo test --test stage6e_gates test_6e_workspace_containment`| Returns `ERR_CAPABILITY_DENIED` |
| **Gate 6E-8** | `test_6e_offline_operation` | `cargo test --test stage6e_gates test_6e_offline_operation` | Pass with network disabled |
| **Gate 6E-9** | `test_6e_memory_accounting` | `cargo test --test stage6e_gates test_6e_memory_accounting` | Visible in `resourced` workspace lease |
| **Gate 6E-10**| `test_6e_crash_recovery` | `cargo test --test stage6e_gates test_6e_crash_recovery` | Daemon restarts; prompts resolve CANCEL |
| **Gate 6E-11**| `test_6e_synthetic_authority` | `cargo test --test stage6e_gates test_6e_synthetic_authority` | Playback checks `SyntheticInputCap` |
| **Gate 6E-12**| `test_6e_zero_capability_grant`| `cargo test --test stage6e_gates test_6e_zero_capability_grant`| Kernel rejects capability grant |
| **Gate 6E-13**| `test_6e_nucleus_preservation` | `git diff --stat origin/main -- kernel/src/stage3/` | 0 bytes modified |
| **Gate 6E-14**| `test_6e_full_regression` | `cargo test --workspace` | 100% pass across all test suites |

---

## 5. Rollback Procedures & Verification Checklist

### Rollback Strategy
If any phase fails compilation or violates an acceptance gate:
1. Revert target file changes using `git checkout`.
2. Clean cargo artifacts via `cargo clean`.
3. Assert that Stage 3A–6D regression tests pass cleanly before retrying.

### Final Authorization Status

```text
Stage 6E Architecture Rev1       🟢 APPROVED / FROZEN
Stage 6E Implementation Plan Rev1🟢 READY FOR AUTHORIZATION
Stage 6E Code Implementation     🛑 NOT AUTHORIZED (Awaiting User Sign-off)
```
