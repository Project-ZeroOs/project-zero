# Stage 6C — Implementation Plan (Rev1)

**Topic:** Human Input, Interaction Routing & Intent Boundary Subsystem  
**Status:** 🟢 PLAN DRAFTED — AWAITING AUTHORIZATION — CODE IMPLEMENTATION NOT AUTHORIZED  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen  
- Stage 5 Presentation Subsystem — 🟢 Frozen  
- Stage 6A User Session Substrate — 🟢 Frozen  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  
- Stage 6C Architecture Specification Rev2 — 🟢 Frozen  

---

## 1. Executive Summary & Non-Negotiable Constraints

This implementation plan translates the frozen **Stage 6C Architecture Specification Rev2** into sequential, machine-verifiable implementation phases. It establishes exact service changes, IPC opcode additions, ABI structures, test harness integration, and rollback procedures without introducing new architectural primitives.

### Non-Negotiable Constraints:
1. **Zero Kernel Modifications**: Stage 3A–3N production Ring-0 microkernel nucleus must remain 100% byte-identical (0 bytes modified).
2. **Single Capability Framework**: All authorization relies strictly on Stage 3H capability handles (`InputFocusPolicyCap`, `RemoteInputPolicyCap`, `SyntheticInputCap`, `AccessibilityPolicyCap`, `AgentInputCap`).
3. **Local Monotonic Timestamp Authority**: `timestamp_monotonic_tsc` is assigned exclusively by `uids` using Stage 3C/4B qualified monotonic TSC upon observation/ingestion. Remote timestamps are ignored.
4. **Unforgeable Provenance**: Provenance is assigned exclusively by `uids` based on IPC socket capability credentials. `INPUT_SOURCE_TRUSTED_AUTH` is restricted to `authui`.
5. **Fail-Closed Quarantine**: `authui` crash invalidates `ModalLock`, denies pending auth transactions in `agentd`, and maintains input quarantine (`FOCUS_STATE_QUARANTINED`).
6. **Local Agent Independence**: Local agent input (`AgentInputCap`) operates offline without requiring network connectivity, `netd`, `fabricd`, or CSDT tokens.
7. **Sequential Phase Execution**: Phase $N+1$ shall not begin until Phase $N$ compiles cleanly and passes all intermediate checks.

---

## 2. Phased Implementation Roadmap

```text
Phase 6C.1: libzero Input ABI & Capability Declarations
     │
     ▼
Phase 6C.2: uids Ingestion, Timestamping & Provenance Assignment
     │
     ▼
Phase 6C.3: shelld Focus Policy & uids Enforcement Integration
     │
     ▼
Phase 6C.4: authui Trusted Path, ModalLock & Fail-Closed Quarantine
     │
     ▼
Phase 6C.5: workspaced / fabricd Remote Input Authorization Integration
     │
     ▼
Phase 6C.6: intentd Intent Ingestion Pipeline Integration
     │
     ▼
Phase 6C.7: Machine Verification Harness & Full System Regression
```

---

## 3. Phase Specifications

### Phase 6C.1: `libzero` Input ABI & Capability Declarations
- **Target Files**: `libzero/src/presentation.rs`, `libzero/src/input.rs` (or `libzero/src/lib.rs`), `libzero/src/error.rs`, `libzero/src/ipc.rs`.
- **Changes**:
  1. Define 64-byte `InputEvent` binary layout (`#[repr(C, packed)]`, Little-Endian):
     - `InputEventHeader` (32 bytes): `device_id: u64`, `timestamp_monotonic_tsc: u64`, `sequence: u64`, `event_type: u16`, `source_provenance: u16`, `device_generation: u32`.
     - `InputEventPayload` (32 bytes): `code_or_button: u32`, `x: i32`, `y: i32`, `modifiers: u32`, `touch_id: u32`, `pressure: u32`, `reserved: [u8; 8]`.
  2. Define event type constants (`EVENT_TYPE_KEY` `0x0001` .. `EVENT_TYPE_DEVICE_STATE` `0x0006`).
  3. Define provenance source constants (`INPUT_SOURCE_PHYSICAL` `0x0000` .. `INPUT_SOURCE_TRUSTED_AUTH` `0x0005`).
  4. Define Stage 3H capability object type constants:
     - `CAP_TYPE_REMOTE_INPUT_POLICY` (`0x0041`)
     - `CAP_TYPE_INPUT_FOCUS_POLICY` (`0x0042`)
     - `CAP_TYPE_SYNTHETIC_INPUT` (`0x0043`)
     - `CAP_TYPE_ACCESSIBILITY_POLICY` (`0x0044`)
     - `CAP_TYPE_AGENT_INPUT` (`0x0045`)
  5. Register Stage 6C IPC opcodes:
     - `OP_UIDS_SET_FOCUS` (`0x0719`) / `OP_UIDS_SET_FOCUS_RESP` (`0x071A`)
     - `OP_UIDS_REQUEST_MODAL_LOCK` (`0x071B`) / `OP_UIDS_REQUEST_MODAL_LOCK_RESP` (`0x071C`)
     - `OP_UIDS_ROUTE_REMOTE_INPUT` (`0x071D`) / `OP_UIDS_ROUTE_REMOTE_INPUT_RESP` (`0x071E`)
- **Verification Gate**: `cargo check -p libzero` compiles cleanly with zero warnings/errors.

---

### Phase 6C.2: `uids` Ingestion, Timestamping & Provenance Assignment
- **Target Files**: `uids/src/main.rs`.
- **Changes**:
  1. Implement device ingestion loop reading Stage 3L IRQ ring buffers (`DevCap`).
  2. Implement local qualified monotonic TSC timestamping: override any payload timestamp with local Stage 3C/4B qualified TSC observation (`timestamp_monotonic_tsc`).
  3. Implement caller capability validation & unforgeable provenance assignment:
     - Ingested via `DevCap` $\rightarrow$ `INPUT_SOURCE_PHYSICAL` (`0x0000`).
     - Ingested via `SyntheticInputCap` $\rightarrow$ `INPUT_SOURCE_AUTOMATION` (`0x0002`).
     - Ingested via `AgentInputCap` $\rightarrow$ `INPUT_SOURCE_AGENT` (`0x0003`).
     - Overwrite any payload provenance claim submitted by untrusted clients (`I-INPUT-SOURCE-PROVENANCE`).
  4. Implement reserved byte zero validation: reject any `InputEvent` with non-zero `payload.reserved` bytes.
  5. Implement motion event coalescing: combine pointer motion events occurring within a single 16ms window into a single coordinate update.
- **Verification Gate**: `cargo check -p uids` compiles cleanly.

---

### Phase 6C.3: `shelld` Focus Policy & `uids` Focus Enforcement Integration
- **Target Files**: `shelld/src/main.rs`, `uids/src/main.rs`.
- **Changes**:
  1. Update `shelld` to act as Focus Policy Authority: on workspace layout change, active surface switch, or window focus shift, `shelld` sends `OP_UIDS_SET_FOCUS` (`0x0719`) to `uids`.
  2. Update `uids` to act as Focus Enforcement Authority:
     - Maintain active `(session_id, workspace_id, surface_id)` routing entry.
     - Deliver events exclusively to the active focused surface (`I-INPUT-NO-KEYLEAK`).
     - Reject unprivileged client attempts to self-declare focus (`I-INPUT-NO-FOCUS-STEAL`).
  3. Implement input state reconciliation (`I-INPUT-STATE-RECONCILIATION`):
     - On receiving `OP_UIDS_SET_FOCUS` for a workspace switch, `uids` flushes pending target event queues.
     - For held modifier keys (Shift, Ctrl, Alt, Super) or pointer buttons in transient state, `uids` synthesizes release events (`EVENT_TYPE_KEY` / `EVENT_TYPE_BUTTON` release) and sends them to the deactivated surface.
     - `uids` resets transient held-key table and requires fresh physical keypress/click observations in the new workspace.
- **Verification Gate**: `cargo check -p shelld` and `cargo check -p uids` compile cleanly.

---

### Phase 6C.4: `authui` Trusted Path, `ModalLock` & Fail-Closed Quarantine
- **Target Files**: `authui/src/main.rs`, `uids/src/main.rs`, `agentd/src/main.rs`.
- **Changes**:
  1. Update `authui` to request `ModalLock` via `OP_UIDS_REQUEST_MODAL_LOCK` (`0x071B`) during active `AuthorizationTransaction`s.
  2. Update `uids` to enforce `ModalLock`:
     - Validate `AuthorizationTransactionRef` with `agentd`.
     - Transition to `ModalLock` state: isolate non-auth input queues (`FOCUS_STATE_CAPTURED`).
     - Route 100% of physical input (`INPUT_SOURCE_PHYSICAL`) exclusively to `authui` overlay (`I-INPUT-TRUSTED-PATH`).
     - Drop non-physical events (`AUTOMATION`, `AGENT`, `REMOTE`) unconditionally during `ModalLock` (`I-INPUT-SYNTHETIC-BOUND`).
  3. Implement fail-closed crash quarantine (`I-INPUT-FAIL-CLOSED`):
     - If `authui` IPC disconnects or crashes during `ModalLock`, `uids` invalidates `ModalLock`, notifies `agentd` to mark pending transactions `DENIED`, and transitions non-trusted input to `FOCUS_STATE_QUARANTINED`.
     - Ordinary input remains blocked until `init` restarts `authui` and `shelld` explicitly restores normal workspace focus.
- **Verification Gate**: `cargo check -p authui`, `cargo check -p uids`, and `cargo check -p agentd` compile cleanly.

---

### Phase 6C.5: `workspaced` / `fabricd` Remote Input Authorization Integration
- **Target Files**: `workspaced/src/main.rs`, `fabricd/src/main.rs`, `uids/src/main.rs`.
- **Changes**:
  1. Update `workspaced` to issue workspace/session-scoped `RemoteInputPolicyCap` (`0x0041`) for authorized remote sessions.
  2. Update `fabricd` to forward remote input events via `OP_UIDS_ROUTE_REMOTE_INPUT` (`0x071D`) with CSDT metadata and `RemoteInputPolicyCap`.
  3. Update `uids` to validate remote input (`I-INPUT-REMOTE-AUTHORIZATION`, `I-INPUT-REMOTE-NO-AUTHORITY`):
     - Verify local `RemoteInputPolicyCap` matches target `SessionId` and `WorkspaceId`. CSDT validation alone yields `ZeroError::PermissionDenied`.
     - Assign `INPUT_SOURCE_REMOTE` (`0x0004`).
     - Restrict remote input strictly to authorized remote surface proxies. Prohibit remote input from executing system shortcuts, altering local focus, or entering `authui` overlays.
- **Verification Gate**: `cargo check -p workspaced`, `cargo check -p fabricd`, and `cargo check -p uids` compile cleanly.

---

### Phase 6C.6: `intentd` Intent Ingestion Pipeline Integration
- **Target Files**: `intentd/src/main.rs`, `uids/src/main.rs`.
- **Changes**:
  1. Update `uids` to act as raw event observer & shortcut extractor: format raw intent payloads and forward them to `intentd` via `OP_INTENT_SUBMIT` (`0x0709`).
  2. Update `intentd` to act as Intent Interpretation Authority (`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`):
     - Independently validate intent payload, workspace containment, and safety boundary rules.
     - Reject malformed or unauthorized intent submissions with `ZeroError::InvalidRequest` or `PermissionDenied`.
     - `uids` yields zero implicit intent resolution or execution capability.
- **Verification Gate**: `cargo check -p intentd` and `cargo check -p uids` compile cleanly.

---

### Phase 6C.7: Machine Verification Harness & Full System Regression
- **Target Files**: `kernel/src/stage4/tests.rs`, `kernel/src/stage4/mod.rs`, `kernel/src/lib.rs`, `tests/test_stage6c.py`.
- **Changes**:
  1. Implement `run_stage6c_verification` in `kernel/src/stage4/tests.rs` covering all 14 Stage 6C acceptance gates (`6C-1` through `6C-14`).
  2. Re-export `run_stage6c_verification` in `kernel/src/stage4/mod.rs` and invoke in `kernel/src/lib.rs`.
  3. Create `tests/test_stage6c.py` Python QEMU test runner asserting 100% pass on all 14 gates and ISA exit code 33 (`0x21`).
  4. Run full repository regression test suite (`python -m unittest discover tests`).
- **Verification Gate**: 100% pass on all Stage 6C tests + 100% pass on full 182+ system regression suite + 0 bytes modified in Stage 3A–3N nucleus.

---

## 4. Acceptance Test Gates (`6C-1` to `6C-14`)

```text
[Gate 6C-1]  64-Byte InputEvent ABI & LE Encoding Verification
[Gate 6C-2]  Qualified Monotonic TSC Timestamping (uids local TSC authority)
[Gate 6C-3]  Focus Policy (shelld) vs Focus Enforcement (uids) Integration
[Gate 6C-4]  ModalLock Trusted Path & 100% Input Isolation (authui)
[Gate 6C-5]  Synthetic Input ModalLock Rejection (INPUT_SOURCE_AUTOMATION)
[Gate 6C-6]  Remote Input Policy Cap Authorization (RemoteInputPolicyCap)
[Gate 6C-7]  CSDT Non-Authority Verification (CSDT alone yields PermissionDenied)
[Gate 6C-8]  Unforgeable Provenance Assignment & TrustedAuth Isolation
[Gate 6C-9]  Workspace Transition Queue Flush & Modifier Release Event Synthesis
[Gate 6C-10] Non-Focused Surface Keyleak Prevention (0 bytes delivered)
[Gate 6C-11] Observation vs Interpretation Boundary (uids -> intentd)
[Gate 6C-12] Stale Device Generation Discard (device_generation validation)
[Gate 6C-13] authui Crash Fail-Closed Quarantine (Quarantine maintained)
[Gate 6C-14] Substrate Preservation (0 bytes Stage 3A–3N) & PMM Neutrality
```

---

## 5. Rollback & Recovery Procedures

If any verification gate fails during phase execution:

1. **Phase Reversion**: Revert working tree changes for the failing phase using `git checkout` or `git restore`.
2. **State Inspection**: Inspect QEMU debug logs (`file:///C:/Users/vaish/.gemini/antigravity-ide/brain/...`) to determine exact error status or assertion code.
3. **Substrate Safeguard**: Verify `git diff kernel/src/` to confirm that Stage 3A–3N production kernel files (`kernel/src/arch/`, `kernel/src/mem/`, `kernel/src/task/`, `kernel/src/sys/`, `kernel/src/ipc/`) have 0 modifications.
4. **Clean Re-compilation**: Execute `cargo check --target x86_64-unknown-none` across all modified microkernel daemons (`uids`, `shelld`, `authui`, `workspaced`, `fabricd`, `intentd`).

---

## 6. Current Implementation Boundary

```text
Stage 6C Discovery Rev2               🟢 APPROVED
Stage 6C Architecture Specification    🟢 FROZEN (Rev2)
Stage 6C Implementation Plan          🟢 DRAFTED (Rev1)
Stage 6C Implementation Code          🛑 NOT AUTHORIZED
```

**Implementation shall not begin until Stage 6C Implementation Plan Rev1 is formally reviewed and authorized.**
