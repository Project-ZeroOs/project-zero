# Stage 6B Implementation Plan (Rev1)
## Distributed Spatial Presentation Protocol & Remote Surface Proxy Subsystem

**Status**: 🟡 PROPOSED — PENDING REVIEW & AUTHORIZATION (CODE EXECUTION NOT AUTHORIZED)  
**Author**: ZeroOS Core Architecture Team  
**Date**: 2026-10-06  
**Authoritative Architectural Specification**: `STAGE6B-ARCHITECTURE-REV1.md` (Frozen Rev2)

---

## 1. Implementation Principles & Security Invariants

```text
========================================================================================
STAGE 6B IMPLEMENTATION INVARIANTS
========================================================================================
1. STAGE 3 NUCLEUS PRESERVATION:
   Stage 3A–3N production kernel nucleus MUST remain 100% byte-identical (0 bytes modified in Ring 0).
   Stage 4/5/6A daemons retain frozen contracts with explicitly authorized integration updates.

2. INVARIANT I-CSDT-NOT-SURFACE-AUTHORITY:
   CSDT token != Kernel Capability != Surface Authority != Workspace Authority.
   A valid CSDT proves transport context, but local surfaced registration requires independent
   local workspace membership validation (via workspaced) and local capability authority.

3. INVARIANT I-REMOTE-STREAM-SINGLE-SURFACE:
   One authenticated remote frame stream corresponds to EXACTLY ONE RemoteSurfaceProxyDescriptor.

4. INVARIANT I-REMOTE-GENERATION-BOUND:
   stream_generation is monotonically increasing per remote surface stream incarnation.
   A frame whose stream_generation differs from the currently authorized proxy generation
   MUST be discarded without mutating presentation state.

5. INVARIANT I-REMOTE-CRC-NOT-AUTHORITY:
   payload_crc32 is for physical transport corruption detection ONLY, NOT security/authentication.

6. INVARIANT I-REMOTE-MALFORMED-NO-MUTATION:
   Malformed frame dimensions, damage bounds, or payload lengths MUST trigger InvalidArgument
   and MUST NOT mutate the local presentation buffer.

7. INVARIANT I-REMOTE-EPHEMERAL-HANDLES:
   No network socket handle or process-local handle is persisted in RemoteSurfaceProxyDescriptor
   or workspace persistent state. Ephemeral handles are held in runtime tables managed by surfaced/netd.
========================================================================================
```

---

## 2. Phased Implementation Roadmap

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                      STAGE 6B IMPLEMENTATION ROADMAP                    │
├─────────────────────────────────────────────────────────────────────────┤
│ Phase 6B.1: libzero/src/presentation.rs Extensions & IPC Opcodes        │
│ Phase 6B.2: surfaced Remote Surface Proxy Integration                   │
│ Phase 6B.3: fabricd & workspaced CSDT & Membership Validation           │
│ Phase 6B.4: compositord Non-Blocking Remote Composition                 │
│ Phase 6B.5: shelld Remote Viewport Layout & Stale Badge Rendering       │
│ Phase 6B.6: Stage 6B QEMU Machine Verification & Regression Harness     │
└─────────────────────────────────────────────────────────────────────────┘
```

---

### Phase 6B.1: `libzero/src/presentation.rs` Extensions & IPC Opcodes

- **Target File**: `libzero/src/presentation.rs`.
- **IPC Opcodes Added**: `OP_SURFACE_REGISTER_REMOTE_PROXY` (`0x0713`), `OP_SURFACE_REGISTER_REMOTE_PROXY_RESP` (`0x0714`), `OP_SURFACE_UNREGISTER_REMOTE_PROXY` (`0x0715`), `OP_SURFACE_UNREGISTER_REMOTE_PROXY_RESP` (`0x0716`), `OP_SURFACE_QUERY_REMOTE_PROXY` (`0x0717`), `OP_SURFACE_QUERY_REMOTE_PROXY_RESP` (`0x0718`).
- **Core Binary Wire ABI**:
  - `RemoteFrameHeader` (72 bytes, `REMOTE_FRAME_HEADER_SIZE = 72`, `#[repr(C, packed)]`, Little-Endian byte order for all multi-byte fields).
  - `FLAG_FULL_FRAME = 0x0001`, `FLAG_DAMAGE_RECT = 0x0002`.
  - `RemoteSurfaceProxyDescriptor`: `local_surface_id`, `workspace_id`, `source_node_id`, `stream_generation`, `csdt_id`, `local_shm_handle`, `width`, `height`, `z_layer`, `stream_state`, `last_valid_frame_received_tsc`, `dropped_frame_count`.
- **Verification Gate**: `cargo check --target x86_64-unknown-none` in `libzero` succeeds with code 0.

---

### Phase 6B.2: `surfaced` Remote Surface Proxy Integration

- **Target File**: `surfaced/src/main.rs`.
- **Responsibilities**:
  - Handlers for `OP_SURFACE_REGISTER_REMOTE_PROXY`, `OP_SURFACE_UNREGISTER_REMOTE_PROXY`, `OP_SURFACE_QUERY_REMOTE_PROXY`.
  - Allocates local SHM buffer and returns `local_surface_id`.
  - Enforces `I-REMOTE-STREAM-SINGLE-SURFACE` and `I-REMOTE-GENERATION-BOUND`.
  - Restricts remote surface $z$-layer to $z \le 99$.
  - Enforces checked arithmetic for damage rectangles (`damage.x + damage.width <= width`, `damage.y + damage.height <= height`) and payload length bounds.
  - Implements receiver-local monotonic clock stale detection ($T_{\text{stale}} = 500\text{ ms}$).
- **Verification Gate**: `cargo check --target x86_64-unknown-none` in `surfaced` succeeds with code 0.

---

### Phase 6B.3: `fabricd` & `workspaced` CSDT & Membership Validation

- **Target Files**: `fabricd/src/main.rs`, `workspaced/src/main.rs`.
- **Responsibilities**:
  - `fabricd`: Validates `CsdtToken` signature and expiration. Stores `csdt_id` strictly for audit/provenance metadata (`I-REMOTE-CRC-NOT-AUTHORITY`).
  - `workspaced`: Independently verifies `CSDT.workspace_id` matches an authorized local `WorkspaceId` belonging to the active session.
- **Verification Gate**: `cargo check --target x86_64-unknown-none` succeeds with code 0.

---

### Phase 6B.4: `compositord` Non-Blocking Remote Composition

- **Target File**: `compositord/src/main.rs`.
- **Responsibilities**:
  - Non-blocking 60 FPS software composition using the local presentation SHM double-buffer.
  - When `stream_state == STREAM_STATE_STALE` ($> 500\text{ ms}$ local TSC since last frame), renders last valid front buffer without stalling scanout loop.
- **Verification Gate**: `cargo check --target x86_64-unknown-none` in `compositord` succeeds with code 0.

---

### Phase 6B.5: `shelld` Remote Viewport Layout & Stale Badge Rendering

- **Target File**: `shelld/src/main.rs`.
- **Responsibilities**:
  - Places remote surface proxies into spatial workspace grid layout.
  - Renders visual network disconnect badge overlay when remote stream transitions to `STREAM_STATE_STALE`.
- **Verification Gate**: `cargo check --target x86_64-unknown-none` in `shelld` succeeds with code 0.

---

### Phase 6B.6: Stage 6B QEMU Machine Verification & Regression Harness

- **Target Files**: `kernel/src/stage4/tests.rs` (adding `run_stage6b_verification`), `tests/test_stage6b.py`.
- **Responsibilities**:
  - Executes 26 machine verification scenarios (`6B-1` through `6B-26`).
  - Asserts ISA debug exit status 33 (`0x21`).
  - Verifies 0 frame leaks (PMM baseline == PMM final).
  - Asserts 0 bytes modified in Stage 3A–3N production kernel nucleus.
  - Runs full system regression (`python -m unittest discover tests`).
- **Verification Gate**: 100% test pass across all 26 Stage 6B scenarios and all existing regression suites.

---

## 3. Machine-Verifiable Acceptance Matrix (`6B-1` – `6B-26`)

```text
| Test ID | Behavioral Machine Assertion Contract                              | Explicit Verification Output Marker |
|---------|-------------------------------------------------------------------|--------------------------------------|
| 6B-1    | RemoteFrameHeader 72-Byte Exact ABI Size Validation                | [Test 6B-1: Header 72-Byte Size]: PASS|
| 6B-2    | Little-Endian Wire Field Serialization & Deserialization          | [Test 6B-2: LE Wire Encoding]: PASS  |
| 6B-3    | CSDT Provenance Logging (csdt_id metadata non-authority check)   | [Test 6B-3: CSDT Provenance]: PASS   |
| 6B-4    | Local Workspace Membership Independent Verification (workspaced)  | [Test 6B-4: Local WS Auth]: PASS    |
| 6B-5    | surfaced Local SHM Handle Exclusive Allocation (local_shm_handle)| [Test 6B-5: Exclusive SHM Handle]: PASS|
| 6B-6    | Remote Stream Single-Surface Incarnation Binding                  | [Test 6B-6: Single Surface Stream]: PASS|
| 6B-7    | Stream Generation Validation & Stale Incarnation Discard          | [Test 6B-7: Generation Bound]: PASS  |
| 6B-8    | Remote Surface Z-Layer Restriction (z <= 99 enforced)             | [Test 6B-8: Z-Layer Restriction]: PASS|
| 6B-9    | Remote System Layer Escalation Rejection (z>=100 -> PermissionDenied)| [Test 6B-9: System Layer Reject]: PASS|
| 6B-10   | Exclusive Auth Overlay Lock (z=255 rejected for remote surfaces)   | [Test 6B-10: Auth Overlay Lock]: PASS|
| 6B-11   | RemoteFrameHeader CRC32 Checksum Validation                       | [Test 6B-11: CRC32 Checksum]: PASS   |
| 6B-12   | Out-of-Order Frame Sequence Discard (frame_sequence <= last_seq)  | [Test 6B-12: Sequence Discard]: PASS |
| 6B-13   | Damage Rect Bounds & Payload Length Verification                   | [Test 6B-13: Damage Rect Bounds]: PASS|
| 6B-14   | Malformed Frame Discard Without Presentation Buffer Mutation      | [Test 6B-14: Malformed Frame Discard]: PASS|
| 6B-15   | Receiver-Local Monotonic Clock Stale Frame Detection (T_stale=500ms)| [Test 6B-15: Local Stale Clock]: PASS|
| 6B-16   | Non-Blocking Compositor Scanout Loop Preservation (60 FPS)        | [Test 6B-16: Non-Blocking Scanout]: PASS|
| 6B-17   | Stale Frame Network Disconnect Badge Rendering in shelld          | [Test 6B-17: Disconnect Badge]: PASS |
| 6B-18   | Remote Surface Proxy Registration Dispatch                        | [Test 6B-18: Proxy Register]: PASS   |
| 6B-19   | Remote Surface Proxy Unregistration Dispatch                      | [Test 6B-19: Proxy Unregister]: PASS |
| 6B-20   | Stage 4F Provider-Loss Remote Proxy Teardown                      | [Test 6B-20: Provider Teardown]: PASS|
| 6B-21   | Workspace State Preservation during Remote Proxy Disconnect        | [Test 6B-21: Workspace Preserved]: PASS|
| 6B-22   | Local Presentation SHM Zero-Copy Scanout Verification              | [Test 6B-22: Local Zero-Copy]: PASS  |
| 6B-23   | Protocol Robustness & Invalid Opcode Handling (0x0713..0x0718)     | [Test 6B-23: Protocol Robustness]: PASS|
| 6B-24   | Stage 4B Composition RAM Lease Compliance (MAX_SURFACE_RAM_MB)    | [Test 6B-24: RAM Lease Bounds]: PASS |
| 6B-25   | PMM Frame Neutrality (Baseline == Final Free Frames)               | [Test 6B-25: PMM Neutrality]: PASS   |
| 6B-26   | Stage 3A–3N Kernel Nucleus Byte-Identical Preservation             | [Test 6B-26: Kernel Preserved]: PASS |
```

---

## 4. Status & Authorization Request

```text
Stage 3A–3N Kernel Nucleus     🟢 FROZEN & VERIFIED (Byte-Identical)
Stage 4A–4F Core Services      🟢 FROZEN & VERIFIED
Stage 5 User Interaction       🟢 FROZEN & VERIFIED
Stage 6A User Session Substrate  🟢 FROZEN & VERIFIED
Stage 6B Architecture Spec     🟢 FROZEN (REV2)
Stage 6B Implementation Plan   🟢 PROPOSED (STAGE6B-IMPLEMENTATION.md)

Code Execution                 🛑 NOT AUTHORIZED (Awaiting Implementation Approval)
```

**Submitted for review and implementation authorization.**  
Upon authorization, code execution will begin starting with **Phase 6B.1 (`libzero/src/presentation.rs` extensions)**.
