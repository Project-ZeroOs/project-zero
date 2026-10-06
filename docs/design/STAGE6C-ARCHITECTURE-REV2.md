# Stage 6C — Architecture Specification (Rev2)

**Topic:** Human Input, Interaction Routing & Intent Boundary Subsystem  
**Status:** 🟢 ARCHITECTURE REV2 — FROZEN ARCHITECTURAL SPECIFICATION  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen  
- Stage 5 Presentation Subsystem — 🟢 Frozen  
- Stage 6A User Session Substrate — 🟢 Frozen  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  

---

## 1. Executive Summary & System Boundary

Stage 6C formalizes the architecture for human interaction, input event routing, and the intent boundary in ZeroOS. It establishes `uids` as the **User Interaction Ingestion & Input Focus Enforcement Daemon**, operating under authority delegated from `init` and coordinating with `shelld`, `authui`, `surfaced`, `compositord`, `workspaced`, and `intentd`.

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

## 2. Stage 3H Capability Specifications & Lineage Graph

Stage 6C relies strictly on Stage 3H object capabilities. No new capability frameworks or microkernel syscalls are created.

### 2.1 Capability Lineage & Scope Matrix

| Capability | ObjectType ID | Rights Bitmask | Root Authority | Delegator | Authoritative Holder | Scope & Delegation Restrictions |
|---|---|---|---|---|---|---|
| **`InputFocusPolicyCap`** | `0x0042` | `0x0007` (`SET_FOCUS` \| `ISOLATE_INPUT` \| `RECONCILE_STATE`) | `init` | `init` | `uids` | Global system input focus enforcement. Non-delegable. |
| **`RemoteInputPolicyCap`** | `0x0041` | `0x0003` (`INGEST_REMOTE` \| `BIND_REMOTE_STREAM`) | `init` | `workspaced` / `shelld` | `fabricd` / `uids` | Strictly **session/workspace scoped** (`SessionId`, `WorkspaceId`). Cannot grant cross-session authority. |
| **`SyntheticInputCap`** | `0x0043` | `0x0001` (`INJECT_SYNTHETIC`) | `init` | `init` | Local Test Automation | Local process scoped. Cannot enter `ModalLock` or access trusted input paths. |
| **`AccessibilityPolicyCap`**| `0x0044` | `0x0001` (`MONITOR_ACCESSIBILITY`) | `init` | `shelld` | Accessibility Daemon | Active workspace scoped; requires visual badge HUD indicator in `shelld`. |

---

## 3. Local Monotonic Timestamping Authority

1. **`timestamp_monotonic_tsc` Assignment**: The 64-bit `timestamp_monotonic_tsc` field is assigned **exclusively by the local `uids` ingestion boundary** using the Stage 3C/4B qualified monotonic TSC source upon event observation/ingestion.
2. **Rejection of Foreign Timestamps**: Sender-provided timestamps, network clock sync markers, and remote peer timestamps are **strictly ignored** for ordering, event validation, and security decisions (`I-INPUT-PER-DEVICE-ORDER`).
3. **No Wall-Clock Dependency**: Real-time wall clock (UTC) is never used for input ordering or verification.

---

## 4. Binary Wire Contract & 64-Byte ABI

All input events passing through `uids` conform to a deterministic, frozen 64-byte Little-Endian binary layout (`#[repr(C, packed)]`).

```rust
#[repr(C, packed)]
pub struct InputEventHeader {
    pub device_id: u64,                // Stage 3L Physical/Logical Device ID (8 bytes, LE)
    pub timestamp_monotonic_tsc: u64,  // Assigned by local uids qualified monotonic TSC (8 bytes, LE)
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

### 4.1 Frozen Event Type Enums (`event_type: u16`)
- `EVENT_TYPE_KEY` (`0x0001`): Keyboard press / release.
- `EVENT_TYPE_BUTTON` (`0x0002`): Pointer button press / release.
- `EVENT_TYPE_POINTER_MOTION` (`0x0003`): Pointer motion ($X, Y$ coordinates or deltas).
- `EVENT_TYPE_TOUCH` (`0x0004`): Multi-touch point events (`touch_id`, `pressure`).
- `EVENT_TYPE_SCROLL` (`0x0005`): Scroll wheel deltas ($x = \text{horizontal}$, $y = \text{vertical}$).
- `EVENT_TYPE_DEVICE_STATE` (`0x0006`): Device attach / detach / generation reset.

---

## 5. Provenance-to-Authority Matrix & Local Agent Independence

`source_provenance: u16` is assigned **exclusively by `uids`** based on validated IPC capability credentials. Unprivileged client attempts to self-declare provenance flags are overwritten or dropped (`I-INPUT-SOURCE-PROVENANCE`).

| Provenance Enum | Value | Authoritative Source Path | Required Capability | Provenance Authority Rule |
|---|---|---|---|---|
| `INPUT_SOURCE_PHYSICAL` | `0x0000` | Stage 3L hardware IRQ device read | `DevCap` (Stage 3L) | Assigned ONLY to physical hardware IRQ streams ingested by `uids`. |
| `INPUT_SOURCE_ACCESSIBILITY` | `0x0001` | Local Accessibility Daemon IPC | `AccessibilityPolicyCap` | Assigned to local accessibility engines; triggers HUD badge in `shelld`. |
| `INPUT_SOURCE_AUTOMATION` | `0x0002` | Local Test Automation IPC | `SyntheticInputCap` | Assigned to local automated test drivers. Cannot access `ModalLock`. |
| `INPUT_SOURCE_AGENT` | `0x0003` | Local or Remote Agent Execution Context | `AgentInputCap` | Assigned to agent runtime interactions. Works offline without network. |
| `INPUT_SOURCE_REMOTE` | `0x0004` | `netd` / `fabricd` Network Ingestion | `RemoteInputPolicyCap` + CSDT | Assigned to remote network streams. Strictly workspace-scoped. |
| `INPUT_SOURCE_TRUSTED_AUTH` | `0x0005` | Authenticated `authui` Overlay Path | `AuthorizationTransactionRef` | **Exclusively generated by `authui`**. Cannot be delegated to agents, remote peers, or apps. |

### 5.1 Local Agent Independence Principle
- **Offline Agent Support**: An agent runtime operating locally under an authorized `AgentInputCap` submits input events directly to `uids` locally without requiring network connectivity, `netd`, or `fabricd`.
- **Non-Dependency on Remote Infrastructure**: ZeroOS does not treat agents as network-dependent services. Network transport is merely one optional provenance path (`INPUT_SOURCE_REMOTE`), distinct from local agent execution (`INPUT_SOURCE_AGENT`).

---

## 6. Focus Policy vs. Enforcement Architecture

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

1. `shelld` is the **sole source of focus policy**. Unprivileged applications cannot alter focus state (`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`).
2. `uids` enforces focus routing under `InputFocusPolicyCap`. Events are delivered exclusively to the active focused surface ID. Non-focused surfaces receive zero keypress or pointer event data (`I-INPUT-NO-KEYLEAK`).

---

## 7. Trusted Input Subsystem & Fail-Closed Quarantine (`ModalLock` & `authui`)

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
authui Overlay (Ingests Credentials / Confirmation Clicks)
```

### Trusted Path Rules & Fail-Closed Quarantine:
1. **Physical-Only Restriction**: `ModalLock` accepts **only** `INPUT_SOURCE_PHYSICAL` events. Synthetic (`AUTOMATION`), agent (`AGENT`), or remote (`REMOTE`) events received during `ModalLock` are unconditionally dropped (`I-INPUT-SYNTHETIC-BOUND`).
2. **Fail-Closed Crash Recovery (`I-INPUT-FAIL-CLOSED`)**:
   - If `authui` crashes or disconnects during `ModalLock`, `uids` **does not** simply release the lock to unquarantine normal applications.
   - `uids` immediately invalidates `ModalLock`, notifies `agentd` to mark all pending authorization transactions **DENIED**, and **maintains non-trusted input quarantine** (`FOCUS_STATE_QUARANTINED`).
   - Ordinary input remains blocked until `init` restarts `authui`, re-establishes the trusted path, and `shelld` explicitly restores normal workspace focus.

---

## 8. Remote Input Subsystem & Stage 6B Integration

Stage 6B introduced remote surface proxies (`OP_SURFACE_REGISTER_REMOTE_PROXY`). Stage 6C defines the explicit remote input authorization contract:

```text
Remote Node ──► netd ──► fabricd (CSDT Provenance) ──► workspaced (RemoteInputPolicyCap) ──► uids ──► Remote Surface Proxy
```

1. **CSDT $\neq$ Local Input Authority**: CSDT proves origin identity but **does not** grant local input authority (`I-INPUT-REMOTE-NO-AUTHORITY`).
2. **Explicit Workspace-Scoped `RemoteInputPolicyCap` Required**: `workspaced`/`shelld` must issue a session/workspace-scoped `RemoteInputPolicyCap` authorizing input ingestion for the remote surface proxy (`I-INPUT-REMOTE-AUTHORIZATION`).
3. **Remote Isolation**:
   - Remote input is assigned `INPUT_SOURCE_REMOTE`.
   - Remote input can interact only with remote surface proxies bound to its authorized fabric stream.
   - Remote input **cannot** execute system shortcuts, alter local focus, or enter trusted `authui` overlays.

---

## 9. Workspace Containment & Input State Reconciliation

1. **Workspace Boundary**: Events delivered to surface $S$ carry explicit `workspace_id` validation from `workspaced` (`I-INPUT-WORKSPACE-CONTAINMENT`).
2. **Input State Reconciliation Protocol (`I-INPUT-STATE-RECONCILIATION`)**:
   When `shelld` triggers a workspace or session switch:
   - **Step 1 (Queue Flush)**: `uids` instantly discards queued events bound to the deactivated workspace.
   - **Step 2 (Release Synthesis)**: For any key or button currently marked as "pressed" in `uids` transient state, `uids` synthesizes a key-up / button-up release event (`EVENT_TYPE_KEY` / `EVENT_TYPE_BUTTON`) and delivers it to the deactivated surface to prevent stuck modifiers or buttons.
   - **Step 3 (Observation Reset)**: `uids` clears its transient held-key table. The newly activated workspace requires fresh physical keypress/click observations before registering held inputs.

---

## 10. Observation vs. Interpretation (`uids` → `intentd`)

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
3. **`I-INPUT-PER-DEVICE-ORDER`**: Event sequence ordering is strictly enforced per device `(sequence, device_generation)`; timestamping is assigned exclusively by local `uids` qualified monotonic TSC.
4. **`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`**: `shelld` is Focus Policy Authority (`SessionManagementCap`); `uids` is Focus Enforcement Authority (`InputFocusPolicyCap`).
5. **`I-INPUT-NO-CAPABILITY-AMPLIFICATION`**: Ingesting or routing an input event yields zero kernel capability amplification.
6. **`I-INPUT-TRUSTED-PATH`**: While `authui` holds `ModalLock`, 100% of non-auth input queues are isolated and quarantined.
7. **`I-INPUT-NO-KEYLEAK`**: Non-focused surfaces receive zero keypress or pointer event data.
8. **`I-INPUT-NO-FOCUS-STEAL`**: Background tasks cannot alter active focus state or break `ModalLock`.
9. **`I-INPUT-GENERATION-BOUND`**: Out-of-order or stale device generation events are discarded without state mutation.
10. **`I-INPUT-REMOTE-AUTHORIZATION`**: Remote input streams (CSDT) require an explicit workspace-scoped `RemoteInputPolicyCap`. CSDT validation alone SHALL NEVER grant input authority.
11. **`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`**: `uids` performs event observation; `intentd` independently validates intent interpretation. `uids` yields zero implicit intent authority.
12. **`I-INPUT-SOURCE-PROVENANCE`**: Unforgeable `InputSource` provenance is assigned exclusively by `uids` based on IPC capability credentials. `INPUT_SOURCE_TRUSTED_AUTH` is restricted to `authui`.
13. **`I-INPUT-STATE-RECONCILIATION`**: Workspace/session switch triggers input queue flushing and modifier/button release state reconciliation.
14. **`I-INPUT-FAIL-CLOSED`**: `authui` failure invalidates `ModalLock`, denies pending authorization transactions, and maintains input quarantine until a new trusted path is established.

---

## 14. Machine Verification & Acceptance Plan (Gates `6C-1` to `6C-14`)

| Gate | Machine Verification Test Case | Required Output / Invariant |
|---|---|---|
| **`6C-1`** | 64-Byte `InputEvent` ABI Validation | `sizeof(InputEvent) == 64`, LE encoding verified (`I-INPUT-PER-DEVICE-ORDER`). |
| **`6C-2`** | Qualified Monotonic TSC Timestamping | Timestamp assigned exclusively by local `uids` TSC clock; remote timestamps ignored. |
| **`6C-3`** | Focus Policy vs Enforcement Separation | `shelld` updates focus via `OP_UIDS_SET_FOCUS`; unprivileged rejected (`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`). |
| **`6C-4`** | `ModalLock` Trusted Path Isolation | `authui` acquires `ModalLock`; non-auth queues receive 0 keypresses (`I-INPUT-TRUSTED-PATH`). |
| **`6C-5`** | Synthetic Input `ModalLock` Rejection | `INPUT_SOURCE_AUTOMATION` dropped during `ModalLock` (`I-INPUT-SYNTHETIC-BOUND`). |
| **`6C-6`** | Remote Input Policy Authorization | Remote input without workspace-scoped `RemoteInputPolicyCap` rejected (`I-INPUT-REMOTE-AUTHORIZATION`). |
| **`6C-7`** | CSDT Non-Authority Verification | Valid CSDT without local `RemoteInputPolicyCap` yields `PermissionDenied`. |
| **`6C-8`** | Unforgeable Provenance Assignment | Client physical provenance flag overwritten by `uids`; `INPUT_SOURCE_TRUSTED_AUTH` restricted to `authui`. |
| **`6C-9`** | Workspace State Reconciliation | Workspace switch flushes queue & synthesizes modifier key-up events (`I-INPUT-STATE-RECONCILIATION`). |
| **`6C-10`**| Keyleak Prevention | Non-focused surface receives 0 input bytes (`I-INPUT-NO-KEYLEAK`). |
| **`6C-11`**| Observation vs Interpretation Boundary | `uids` passes raw payload to `intentd`; `uids` cannot execute intent (`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`). |
| **`6C-12`**| Stale Device Generation Discard | Disconnected device generation events dropped (`I-INPUT-GENERATION-BOUND`). |
| **`6C-13`**| `authui` Fail-Closed Quarantine | `authui` crash invalidates `ModalLock`, denies pending transactions, and maintains input quarantine until re-established (`I-INPUT-FAIL-CLOSED`). |
| **`6C-14`**| Substrate & PMM Neutrality | 0 bytes modified in Stage 3A–3N kernel nucleus; 100% PMM frame neutral. |

---

## 15. Implementation Boundary & State Transitions

```text
Stage 6C Discovery Rev2               🟢 APPROVED
Stage 6C Architecture Specification    🟢 FROZEN (Rev2)
Stage 6C Implementation Plan          🔴 NOT YET DRAFTED
Stage 6C Code Implementation          🔴 NOT AUTHORIZED
```

**Implementation must not begin until Stage 6C Implementation Plan is formally drafted, reviewed, and approved.**
