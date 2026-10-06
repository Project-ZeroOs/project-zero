# Stage 6C — Architecture Specification (Rev1)

**Topic:** Human Input, Interaction Routing & Intent Boundary Subsystem  
**Status:** 🟡 ARCHITECTURE DRAFT — AWAITING REVIEW — IMPLEMENTATION NOT AUTHORIZED  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen  
- Stage 5 Presentation Subsystem — 🟢 Frozen  
- Stage 6A User Session Substrate — 🟢 Frozen  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  

---

## 1. Executive Summary & System Boundary

Stage 6C establishes the formal architecture for human interaction, input event routing, and the intent boundary in ZeroOS. It formalizes `uids` as the **User Interaction Ingestion & Input Focus Enforcement Daemon**, operating under authority delegated from `init` and coordinating with `shelld`, `authui`, `surfaced`, `compositord`, `workspaced`, and `intentd`.

```text
                               Physical HID / IRQ (Stage 3L)
                                             │
                                             ▼
                        ┌─────────────────────────────────────────┐
                        │                 uids                    │
                        │ (Input Focus Enforcement Authority)     │
                        └────────────────────┬────────────────────┘
                                             │
               ┌─────────────────────────────┼─────────────────────────────┐
               ▼                             ▼                             ▼
            shelld                        authui                        intentd
    (Focus Policy Authority)      (Trusted Modal Authority)     (Intent Interpretation)
               │                             │                             │
               ▼                             ▼                             ▼
          workspaced                      surfaced                    compositord
  (Workspace Containment &           (Surface Bounds &            (Non-blocking Display
    RemoteInputPolicyCap)            Hit-Testing Context)              Scanout)
```

---

## 2. Stage 3H Capability Specifications

Stage 6C relies strictly on Stage 3H object capabilities. No new kernel capability frameworks or syscalls are created.

### 2.1 `RemoteInputPolicyCap` (Stage 3H Object Type `0x0041`)
- **Object Type ID**: `CAP_TYPE_REMOTE_INPUT_POLICY` (`0x0041`).
- **Rights Bitmask**:
  - `RIGHT_INGEST_REMOTE` (`0x0001`): Right to deliver remote input streams to local workspace surfaces.
  - `RIGHT_BIND_REMOTE_STREAM` (`0x0002`): Right to bind a CSDT fabric stream to a local surface proxy for input routing.
- **Lineage & Authority Graph**:
  ```text
  init (Root Authority)
    └── workspaced (Workspace Management Authority)
          └── RemoteInputPolicyCap (Delegated per authorized remote session/workspace)
                └── fabricd / uids (Ingestion & Validation)
  ```
- **Holder**: `workspaced` is the authoritative issuer; `fabricd` presents this handle when forwarding remote input to `uids`.

### 2.2 `InputFocusPolicyCap` (Stage 3H Object Type `0x0042`)
- **Object Type ID**: `CAP_TYPE_INPUT_FOCUS_POLICY` (`0x0042`).
- **Rights Bitmask**:
  - `RIGHT_SET_FOCUS` (`0x0001`): Right to enforce focus target surfaces.
  - `RIGHT_ISOLATE_INPUT` (`0x0002`): Right to quarantine non-focused surface input queues.
  - `RIGHT_RECONCILE_STATE` (`0x0004`): Right to synthesize release events during workspace state transitions.
- **Holder**: `uids` holds `InputFocusPolicyCap` delegated by `init` at boot.

### 2.3 `SyntheticInputCap` (Stage 3H Object Type `0x0043`)
- **Object Type ID**: `CAP_TYPE_SYNTHETIC_INPUT` (`0x0043`).
- **Rights Bitmask**:
  - `RIGHT_INJECT_SYNTHETIC` (`0x0001`): Right to submit synthetic automation or test input events to `uids`.
- **Holder**: Issued exclusively to authorized testing/automation drivers by `init`. `SyntheticInputCap` **cannot** access trusted input paths (`ModalLock`).

---

## 3. Binary Wire Contract & 64-Byte ABI

All input events passing between Stage 3L device ring buffers, `uids`, and target surfaces conform to a deterministic, frozen 64-byte Little-Endian binary layout (`#[repr(C, packed)]`).

```rust
#[repr(C, packed)]
pub struct InputEventHeader {
    pub device_id: u64,                // Stage 3L Physical/Logical Device ID (8 bytes, LE)
    pub timestamp_monotonic_tsc: u64,  // Receiver-local qualified monotonic TSC ticks (8 bytes, LE)
    pub sequence: u64,                 // Per-device monotonic event sequence counter (8 bytes, LE)
    pub event_type: u16,               // Frozen Event Type enum (2 bytes, LE)
    pub source_provenance: u16,        // Unforgeable InputSource enum (2 bytes, LE)
    pub device_generation: u32,        // Monotonic device connection generation (4 bytes, LE)
}                                      // Total Header = 32 bytes

#[repr(C, packed)]
pub struct InputEventPayload {
    pub code_or_button: u32,     // Keycode, mouse button, or gesture ID (4 bytes, LE)
    pub x: i32,                  // Absolute/relative X coordinate or delta (4 bytes, LE)
    pub y: i32,                  // Absolute/relative Y coordinate or delta (4 bytes, LE)
    pub modifiers: u32,          // Active modifier bitmask (Shift, Ctrl, Alt, Super) (4 bytes, LE)
    pub touch_id: u32,           // Touch point tracker ID (4 bytes, LE)
    pub pressure: u32,           // Pressure / analog value (4 bytes, LE)
    pub reserved: [u8; 8],       // Reserved fields — MUST BE ZERO (8 bytes)
}                                // Total Payload = 32 bytes

#[repr(C, packed)]
pub struct InputEvent {
    pub header: InputEventHeader,   // 32 bytes
    pub payload: InputEventPayload, // 32 bytes
}                                   // Total Size = 64 bytes
```

### 3.1 Frozen Event Type Enums (`event_type: u16`)
- `EVENT_TYPE_KEY` (`0x0001`): Keyboard press / release.
- `EVENT_TYPE_BUTTON` (`0x0002`): Pointer button press / release.
- `EVENT_TYPE_POINTER_MOTION` (`0x0003`): Pointer motion ($X, Y$ coordinates or deltas).
- `EVENT_TYPE_TOUCH` (`0x0004`): Multi-touch point events (`touch_id`, `pressure`).
- `EVENT_TYPE_SCROLL` (`0x0005`): Scroll wheel deltas ($x = \text{horizontal}$, $y = \text{vertical}$).
- `EVENT_TYPE_DEVICE_STATE` (`0x0006`): Device attach / detach / generation reset.

### 3.2 Wire Validation Rules
1. `payload.reserved` MUST be zero (`[0u8; 8]`). Non-zero reserved fields cause immediate event discard.
2. `timestamp_monotonic_tsc` MUST originate from receiver-local Stage 3C/4B qualified monotonic TSC. No wall-clock or remote network timestamps are accepted for ordering or security decisions.
3. Unknown or out-of-range `event_type` values are dropped safely without mutating input state.

---

## 4. Unforgeable Provenance & Capability Authentication

`source_provenance: u16` is assigned **exclusively by `uids`** upon receiving an event, overriding any caller-provided provenance field:

```rust
pub const INPUT_SOURCE_PHYSICAL: u16       = 0x0000; // Hardware IRQ via DevCap
pub const INPUT_SOURCE_ACCESSIBILITY: u16  = 0x0001; // Authorized accessibility daemon
pub const INPUT_SOURCE_AUTOMATION: u16     = 0x0002; // Test automation via SyntheticInputCap
pub const INPUT_SOURCE_AGENT: u16          = 0x0003; // Agent runtime requested event
pub const INPUT_SOURCE_REMOTE: u16         = 0x0004; // Remote stream via RemoteInputPolicyCap + CSDT
pub const INPUT_SOURCE_TRUSTED_AUTH: u16   = 0x0005; // ModalLock authui trusted path
```

### Provenance Assignment Rules:
- **Physical Provenance**: Assigned ONLY to events ingested directly from Stage 3L hardware device ring buffers bound to `uids` via `DevCap`.
- **Capability Overrides**: `uids` inspects the sender's IPC socket capability credentials. If an unprivileged caller submits an event claiming `INPUT_SOURCE_PHYSICAL`, `uids` overwrites `source_provenance` with `INPUT_SOURCE_AUTOMATION` or drops the frame (`I-INPUT-SOURCE-PROVENANCE`).

---

## 5. Focus Policy vs. Enforcement Architecture

ZeroOS strictly separates **Focus Policy** from **Focus Enforcement**:

```text
shelld (Focus Policy Authority)
  │ Holds SessionManagementCap
  │ Dictates active session/workspace focus target
  ▼
OP_UIDS_SET_FOCUS (0x0719)
  │
  ▼
uids (Focus Enforcement Authority)
  │ Holds InputFocusPolicyCap
  │ Updates local routing table & enforces stream isolation
  ▼
Target Surface / Application
```

### Focus Contract Rules:
1. `shelld` is the **sole source of focus policy**. Unprivileged processes cannot self-declare focus or request focus switches (`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`).
2. `uids` enforces focus routing under `InputFocusPolicyCap`. Events are delivered exclusively to the focused surface ID registered by `shelld`. Non-focused surfaces receive zero keypress or pointer event data (`I-INPUT-NO-KEYLEAK`).

---

## 6. Trusted Input Subsystem (`ModalLock` & `authui`)

To guarantee credential security during authentication transactions:

```text
authui (Auth Transaction Active)
  │ Sends OP_UIDS_REQUEST_MODAL_LOCK (0x071B) with AuthorizationTransactionRef
  ▼
uids (Input Focus Enforcement)
  │ 1. Verifies AuthorizationTransactionRef with agentd
  │ 2. Acquires ModalLock
  │ 3. Quarantines non-auth input queues (FOCUS_STATE_CAPTURED)
  │ 4. Routes 100% of physical input exclusively to authui
  ▼
authui Overlay (Ingests Passwords / PINs / Confirmation Clicks)
```

### Trusted Path Rules:
1. **Modal Lock Dominance**: While `ModalLock` is active, 100% of physical input events bypass ordinary focus targets and are delivered exclusively to `authui` (`I-INPUT-TRUSTED-PATH`).
2. **Physical-Only Restriction**: `ModalLock` accepts **only** `INPUT_SOURCE_PHYSICAL` events. Synthetic (`INPUT_SOURCE_AUTOMATION`), agent (`INPUT_SOURCE_AGENT`), or remote (`INPUT_SOURCE_REMOTE`) events received during `ModalLock` are unconditionally dropped (`I-INPUT-SYNTHETIC-BOUND`).
3. **Fail-Closed Teardown**: `ModalLock` is released exclusively when `authui` presents transaction completion or when `uids` detects `authui` IPC disconnection. Crashing `authui` causes `uids` to immediately flush input queues and release `ModalLock` (`I-INPUT-FAIL-CLOSED`).

---

## 7. Remote Input Subsystem & Stage 6B Integration

Stage 6B introduced remote surface proxies (`OP_SURFACE_REGISTER_REMOTE_PROXY`). Stage 6C defines the explicit remote input authorization pipeline:

```text
Remote Node ──► netd ──► fabricd (CSDT Provenance) ──► workspaced (RemoteInputPolicyCap) ──► uids ──► Remote Surface Proxy
```

### Remote Input Rules:
1. **CSDT $\neq$ Local Input Authority**: CSDT proves network origin identity but **does not** grant local input authority (`I-INPUT-REMOTE-NO-AUTHORITY`).
2. **Explicit `RemoteInputPolicyCap` Required**: `workspaced`/`shelld` must issue an explicit `RemoteInputPolicyCap` authorizing input ingestion for the specific remote session/workspace (`I-INPUT-REMOTE-AUTHORIZATION`).
3. **Remote Isolation**:
   - Remote input is assigned `INPUT_SOURCE_REMOTE`.
   - Remote input can interact only with remote surface proxies bound to its authorized fabric stream.
   - Remote input **cannot** execute system shortcuts, alter local focus, or enter trusted `authui` overlays.

---

## 8. Workspace Containment & Input State Reconciliation

1. **Workspace Boundary**: Events delivered to surface $S$ carry explicit `workspace_id` validation from `workspaced` (`I-INPUT-WORKSPACE-CONTAINMENT`).
2. **Input State Reconciliation Protocol (`I-INPUT-STATE-RECONCILIATION`)**:
   When `shelld` triggers a workspace or session switch:
   - **Step 1 (Queue Flush)**: `uids` instantly discards all queued input events targeted at the deactivated workspace.
   - **Step 2 (Release Synthesis)**: For any key or button currently marked as "pressed" in `uids` transient state, `uids` synthesizes a key-up / button-up release event (`EVENT_TYPE_KEY` / `EVENT_TYPE_BUTTON` with release payload) and delivers it to the deactivated surface to prevent stuck modifiers or buttons.
   - **Step 3 (Observation Reset)**: `uids` clears its transient held-key table. The newly activated workspace requires fresh physical keypress/click observations before registering held inputs.

---

## 9. Observation vs. Interpretation (`uids` → `intentd`)

ZeroOS strictly separates **Physical Observation** from **Intent Interpretation**:

```text
Physical Input ──► uids (Observation & Shortcut Extraction)
                     │ Formats raw payload
                     ▼
                   OP_INTENT_SUBMIT (0x0709)
                     │
                     ▼
                   intentd (Intent Interpretation Authority)
                     │ Validates safety, structural correctness & DAG
                     ▼
                   Validated Intent / Execution Plan
```

- **Observation Role (`uids`)**: Captures raw input events, matches system shortcuts (e.g. Super+Tab), and formats raw natural language payloads. `uids` possesses zero execution capability and zero intent resolution authority (`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`).
- **Interpretation Role (`intentd`)**: Acts as the sole **Intent Interpretation Authority**. `intentd` independently validates intent context against Stage 4F safety rules before creating execution plans.

---

## 10. State Machines & Failure Recovery Matrix

### 10.1 Input Device Lifecycle State Machine

```text
Discovered ──► Attached ──► Ready ──► Active ──► Quiescing ──► Detached ──► Released
```

- `Discovered`: IRQ/HID device detected by Stage 3L.
- `Attached`: `init` delegates `DevCap` to `uids`.
- `Ready`: `uids` initializes event ring buffer and queries `device_generation`.
- `Active`: `uids` routes events according to active focus policy.
- `Quiescing`: Session transition in progress; input queues draining.
- `Detached`: Device disconnected; `uids` increments `device_generation` and flushes queues.
- `Released`: Device handle freed by Stage 3L.

### 10.2 Service Crash & Failure Recovery Matrix

| Component Failure | Security Effect | Recovery Action |
|---|---|---|
| **`uids` Crash** | Ingestion pauses; zero key leakage. | `init` restarts `uids`, re-binds `DevCap`. `shelld` resends active focus state (`OP_UIDS_SET_FOCUS`). |
| **`authui` Crash** | Active modal lock invalidated. | `agentd` marks pending transactions `DENIED`. `uids` releases `ModalLock` and flushes input queues (Fail-Closed). |
| **`shelld` Crash** | Focus updates pause. | `uids` enters `FOCUS_STATE_QUARANTINED` (drops application input) until `shelld` completes crash recovery and re-establishes focus policy. |
| **`compositord` Crash** | Visual rendering pauses. | `uids` buffers up to 256 events; drops oldest overflow events cleanly without kernel panic. |
| **Device Disconnect** | Hardware stream interrupted. | `uids` increments `device_generation`, flushes device queues, transitions device state to `Detached`. |

---

## 11. Resource Accounting & Performance Bounds

- **RAM Ring Buffer Quotas**: Event queues in `uids` use fixed-size static ring buffers (max 256 pending 64-byte `InputEvent` structs per surface queue = 16 KB total). Zero dynamic kernel heap allocation.
- **CPU Budget**: Event decoding and viewport hit-testing run within `uids` user-space thread budget under Stage 4B CPU resource limits.
- **16ms Pointer Coalescing**: Pointer motion events (`EVENT_TYPE_POINTER_MOTION`) occurring within a single 16ms compositor frame window are coalesced into a single updated $X, Y$ coordinate frame to prevent IPC channel saturation.

---

## 12. IPC Specification & Opcode Registry

Standardized Stage 6C IPC opcodes added to `libzero`:

| Opcode | Enum Identifier | Sender | Receiver | Payload Schema | Response Schema |
|---|---|---|---|---|---|
| `0x0719` | `OP_UIDS_SET_FOCUS` | `shelld` | `uids` | `{ session_id: u64, workspace_id: u64, surface_id: u64 }` | `{ status: i32 }` |
| `0x071A` | `OP_UIDS_SET_FOCUS_RESP` | `uids` | `shelld` | Response payload | — |
| `0x071B` | `OP_UIDS_REQUEST_MODAL_LOCK` | `authui` | `uids` | `{ transaction_token: DistributedId }` | `{ status: i32 }` |
| `0x071C` | `OP_UIDS_REQUEST_MODAL_LOCK_RESP` | `uids` | `authui` | Response payload | — |
| `0x071D` | `OP_UIDS_ROUTE_REMOTE_INPUT` | `fabricd` | `uids` | `{ csdt_id: DistributedId, frame: InputEvent }` | `{ status: i32 }` |
| `0x071E` | `OP_UIDS_ROUTE_REMOTE_INPUT_RESP` | `uids` | `fabricd` | Response payload | — |

---

## 13. Machine-Verifiable Invariants

The Stage 6C architecture enforces 14 machine-verifiable invariants:

1. **`I-INPUT-SESSION-CONTAINMENT`**: Input events must be tagged with and constrained to active `SessionId`.
2. **`I-INPUT-WORKSPACE-CONTAINMENT`**: Input events must not cross workspace boundaries.
3. **`I-INPUT-PER-DEVICE-ORDER`**: Event sequence ordering is strictly enforced per device `(sequence, device_generation)`; cross-device ordering uses receiver-local monotonic TSC with deterministic tie-breaking.
4. **`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`**: `shelld` is Focus Policy Authority (`SessionManagementCap`); `uids` is Focus Enforcement Authority (`InputFocusPolicyCap`).
5. **`I-INPUT-NO-CAPABILITY-AMPLIFICATION`**: Ingesting or routing an input event yields zero kernel capability amplification.
6. **`I-INPUT-TRUSTED-PATH`**: While `authui` holds `ModalLock`, 100% of non-auth input queues are isolated and quarantined.
7. **`I-INPUT-NO-KEYLEAK`**: Non-focused surfaces receive zero keypress or pointer event data.
8. **`I-INPUT-NO-FOCUS-STEAL`**: Background tasks cannot alter active focus state or break `ModalLock`.
9. **`I-INPUT-GENERATION-BOUND`**: Out-of-order or stale device generation events are discarded without state mutation.
10. **`I-INPUT-REMOTE-AUTHORIZATION`**: Remote input streams (CSDT) require an explicit local `RemoteInputPolicyCap` issued by `workspaced`/`shelld`. CSDT validation alone SHALL NEVER grant input authority.
11. **`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`**: `uids` performs event observation; `intentd` independently validates intent interpretation. `uids` yields zero implicit intent authority.
12. **`I-INPUT-SOURCE-PROVENANCE`**: Unforgeable `InputSource` provenance is assigned exclusively by `uids` based on IPC capability validation. Applications cannot self-declare provenance.
13. **`I-INPUT-STATE-RECONCILIATION`**: Workspace/session switch triggers input queue flushing and modifier/button release state reconciliation.
14. **`I-INPUT-FAIL-CLOSED`**: Daemon crash (`uids`, `authui`, `shelld`) causes input state to reset to a secure, fail-closed neutral state.

---

## 14. Machine Verification & Acceptance Plan (Gates `6C-1` to `6C-14`)

| Gate | Machine Verification Test Case | Required Output / Invariant |
|---|---|---|
| **`6C-1`** | 64-Byte `InputEvent` ABI Validation | `sizeof(InputEvent) == 64`, LE encoding verified (`I-INPUT-PER-DEVICE-ORDER`). |
| **`6C-2`** | Qualified Monotonic TSC Timestamping | Timestamp anchored to Stage 3C/4B TSC clock; wall clock rejected. |
| **`6C-3`** | Focus Policy vs Enforcement Separation | `shelld` updates focus via `OP_UIDS_SET_FOCUS`; unprivileged focus requests rejected (`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`). |
| **`6C-4`** | `ModalLock` Trusted Path Isolation | `authui` acquires `ModalLock`; non-auth queues receive 0 keypresses (`I-INPUT-TRUSTED-PATH`). |
| **`6C-5`** | Synthetic Input `ModalLock` Rejection | `INPUT_SOURCE_AUTOMATION` dropped during `ModalLock` (`I-INPUT-SYNTHETIC-BOUND`). |
| **`6C-6`** | Remote Input Policy Authorization | Remote input without `RemoteInputPolicyCap` rejected (`I-INPUT-REMOTE-AUTHORIZATION`). |
| **`6C-7`** | CSDT Non-Authority Verification | Valid CSDT without local `RemoteInputPolicyCap` yields `PermissionDenied`. |
| **`6C-8`** | Unforgeable Provenance Assignment | Client-submitted physical source flag overwritten by `uids` (`I-INPUT-SOURCE-PROVENANCE`). |
| **`6C-9`** | Workspace State Reconciliation | Workspace switch flushes queue and synthesizes modifier key-up events (`I-INPUT-STATE-RECONCILIATION`). |
| **`6C-10`**| Keyleak Prevention | Non-focused surface receives 0 input bytes (`I-INPUT-NO-KEYLEAK`). |
| **`6C-11`**| Observation vs Interpretation Boundary | `uids` passes raw payload to `intentd`; `uids` cannot execute intent (`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`). |
| **`6C-12`**| Stale Device Generation Discard | Disconnected device generation events dropped (`I-INPUT-GENERATION-BOUND`). |
| **`6C-13`**| `authui` Crash Fail-Closed Recovery | `authui` crash releases `ModalLock` and denies pending transactions (`I-INPUT-FAIL-CLOSED`). |
| **`6C-14`**| Substrate & PMM Neutrality | 0 bytes modified in Stage 3A–3N kernel nucleus; 100% PMM frame neutral. |

---

## 15. Implementation Boundary & State Transitions

```text
Stage 6C Discovery Rev2               🟢 APPROVED
Stage 6C Architecture Specification    🟢 DRAFTED (Rev1)
Stage 6C Implementation Plan          🔴 NOT YET DRAFTED
Stage 6C Code Implementation          🔴 NOT AUTHORIZED
```

**Implementation must not begin until Stage 6C Architecture Specification is formally reviewed and frozen.**
