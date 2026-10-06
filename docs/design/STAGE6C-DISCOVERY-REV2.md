# Stage 6C — Architecture Discovery & Dependency Audit (Rev2)

**Topic:** Human Input, Interaction Routing & Intent Boundary Subsystem  
**Status:** 🟢 DISCOVERY REV2 COMPLETE — READY FOR ARCHITECTURE DRAFT  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen  
- Stage 5 Presentation Subsystem — 🟢 Frozen  
- Stage 6A User Session Substrate — 🟢 Frozen  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  

---

## 1. Executive Architectural Conclusion

Stage 6C does **not** require a new microkernel daemon or a duplicate capability system. Instead, Stage 6C defines a **Coordinated Multi-Service Boundary** centered around `uids` as the **User Interaction Ingestion & Input Focus Enforcer**, operating under strict authority delegation from `init` and policy coordination with `shelld`, `authui`, `surfaced`, `compositord`, `workspaced`, and `intentd`.

### Core Architectural Decisions & Rev2 Closures:

1. **`timestamp_monotonic_tsc` Derived from Stage 3C/4B Time Authority**: `InputEvent` timestamping uses `timestamp_monotonic_tsc: u64` representing receiver-local qualified monotonic TSC ticks from Stage 3C/4B. Wall clocks and remote timestamps are strictly prohibited for event ordering and security checks.
2. **Per-Device Monotonic Ordering & Deterministic Tie-Breaking**: `I-INPUT-PER-DEVICE-ORDER` guarantees sequential ordering per physical/logical device using `(sequence, device_generation)`. Cross-device ordering uses receiver-local qualified monotonic TSC with deterministic tie-breaking (e.g., numerical `device_id`). No global total ordering across independent hardware controllers is promised.
3. **Explicit Local Remote-Input Policy Authority**: `I-INPUT-REMOTE-AUTHORIZATION` dictates that remote input streams (Stage 6B) require an explicit local `RemoteInputPolicyCap` issued by `workspaced`/`shelld` for the target session/workspace. CSDT validation alone **shall never** grant local input authority.
4. **Observation vs. Interpretation Boundary**: `I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY` strictly separates `uids` (physical observation + focus routing) from `intentd` (intent interpretation/validation). `uids` passes raw intent payloads to `intentd`, which independently validates whether an event context constitutes a valid Intent or Plan.
5. **Policy vs. Enforcement Separation**: `I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT` formalizes `shelld` as the **Focus Policy Authority** (`SessionManagementCap`) and `uids` as the **Focus Enforcement Authority** (`InputFocusPolicyCap`). `shelld` dictates targets; `uids` enforces stream isolation and delivery.
6. **Machine-Testable 64-Byte `InputEvent` Wire Layout**: Explicit `event_type` enums (`KEY`, `BUTTON`, `POINTER_MOTION`, `TOUCH`, `SCROLL`, `DEVICE_STATE`) with mandatory zeroing of reserved fields and safe drop of unknown event types.
7. **Input State Reconciliation on Transition**: `I-INPUT-STATE-RECONCILIATION` establishes that workspace/session transitions discard queued target events and synthesize key-up/button-up release events to prevent stuck modifier/button states across workspace boundaries.
8. **Unforgeable Input Source Provenance**: `I-INPUT-SOURCE-PROVENANCE` defines an unforgeable `InputSource` enum (`Physical`, `Accessibility`, `Automation`, `Agent`, `Remote`, `TrustedAuth`). Source provenance is assigned exclusively by `uids` based on kernel capability validation (`DevCap`, `SyntheticInputCap`, `RemoteInputPolicyCap`, `ModalLock`). Applications cannot self-declare physical provenance.

---

## 2. Scope

The Stage 6C discovery covers:

- Ingestion of hardware input events via Stage 3L device ring buffers.
- Canonical 64-byte `InputEvent` wire representation with qualified monotonic TSC timestamping.
- Separation of `shelld` focus policy from `uids` focus enforcement.
- Quarantine and trusted routing of authentication input (`ModalLock`).
- Unforgeable provenance classification for physical, synthetic, accessibility, agent, and remote input.
- Explicit local authorization for remote input streams.
- Containment of input events within authorized session and workspace boundaries, including input state reconciliation on workspace transitions.
- Formal separation between `uids` event observation and `intentd` intent interpretation.
- Fail-closed crash recovery and threat modeling against keylogging, focus stealing, and event injection.

---

## 3. Non-Goals

1. **No Kernel Modifications**: Zero changes to Stage 3A–3N production Ring-0 nucleus.
2. **No Second Capability System**: All input permissions rely strictly on Stage 3H capability handles (`InputFocusPolicyCap`, `SystemSurfacePolicyCap`, `SessionManagementCap`, `RemoteInputPolicyCap`).
3. **No Intent Resolution in `uids`**: `uids` does not parse, evaluate, or execute natural-language intents or complex commands. It passes raw intent payloads to `intentd`.
4. **No Remote Input Authority from CSDT**: CSDT tokens and remote surface visibility do not grant remote nodes input authority over local surfaces without explicit local `RemoteInputPolicyCap`.
5. **No Wall Clock Dependencies**: Time ordering relies exclusively on qualified receiver-local monotonic TSC.
6. **No Persistent Event Storage**: Input events are strictly ephemeral and are flushed on session/workspace transitions.

---

## 4. Existing-Subsystem Dependency Map

```text
 Physical HID Hardware / IRQ (Stage 3L)
                  │
                  ▼
                uids (Input Focus Policy Enforcer - InputFocusPolicyCap)
         ┌────────┴────────────────────────┬────────────────────────┐
         │                                 │                        │
         ▼                                 ▼                        ▼
      shelld                            authui                   intentd
(Focus Policy Authority              (Trusted Auth Input        (Intent Interpretation
 SessionManagementCap)                   ModalLock)                 Authority)
         │                                 │                        │
         ▼                                 ▼                        ▼
    workspaced                         surfaced                 compositord
(Workspace Containment &           (Surface Viewport &      (Non-blocking Display
 RemoteInputPolicyCap)              Hit-Testing Bounds)          Scanout)
```

| Service | Stage Introduced | Stage 6C Role & Dependency Contract |
|---|---|---|
| **`uids`** | Stage 5 | Ingests physical/remote input, decodes 64-byte frames, enforces focus routing. |
| **`shelld`** | Stage 6A | Focus Policy Authority; holds `SessionManagementCap`, dictates active workspace/surface focus target to `uids`. |
| **`authui`** | Stage 5 | Holds trusted input overlay authority; requests `ModalLock` from `uids` for auth transactions. |
| **`surfaced`** | Stage 5 / 6B | Surface lifecycle authority; provides viewport bounds and hit-testing coordinates. |
| **`compositord`**| Stage 5 / 6B | Receives non-blocking frame buffers and cursor overlays from `surfaced`/`uids`. |
| **`workspaced`**| Stage 4D | Validates workspace membership and issues `RemoteInputPolicyCap` for authorized remote peers. |
| **`intentd`** | Stage 4F | Intent Interpretation Authority; receives human intent payloads from `uids` via `OP_INTENT_SUBMIT` (`0x0709`). |
| **`fabricd`** | Stage 4F / 6B | Delivers remote input events from network with CSDT provenance metadata. |

---

## 5. Authority Matrix

| Domain / Resource | Authoritative Owner | Capability Required | Enforcement Point |
|---|---|---|---|
| **Physical Input Devices** | `uids` | `DevCap` (Stage 3L) | `init` boot delegation |
| **Focus Policy Authority** | `shelld` | `SessionManagementCap` | `shelld` → `uids` (`OP_UIDS_SET_FOCUS`) |
| **Focus Enforcement Authority** | `uids` | `InputFocusPolicyCap` | `uids` event dispatch loop |
| **Trusted Input Path (`ModalLock`)** | `authui` | `AuthorizationTransactionRef` | `authui` → `uids` (`OP_UIDS_REQUEST_MODAL_LOCK`) |
| **Remote Input Authority** | `workspaced` / `shelld` | `RemoteInputPolicyCap` | `workspaced` + `uids` validation |
| **Workspace Containment** | `workspaced` | `WorkspaceManagementCap` | `workspaced` membership query |
| **Surface Hit-Testing / Bounds** | `surfaced` | `SurfacePolicyCap` | `surfaced` viewport bounds check |
| **Intent Interpretation** | `intentd` | `IntentSubmissionCap` | `intentd` validation pipeline |

---

## 6. Input Event Model & Event-Type Semantics

Canonical 64-byte `InputEvent` Little-Endian wire contract:

```rust
#[repr(C, packed)]
pub struct InputEventHeader {
    pub device_id: u64,                // Stage 3L Physical/Logical Device ID (8 bytes)
    pub timestamp_monotonic_tsc: u64,  // Receiver-local qualified monotonic TSC ticks (8 bytes)
    pub sequence: u64,                 // Device monotonic event sequence counter (8 bytes)
    pub event_type: u16,               // Frozen Event Type enum (2 bytes)
    pub source_provenance: u16,        // Unforgeable InputSource enum (2 bytes)
    pub device_generation: u32,        // Monotonic device connection generation (4 bytes)
}                                      // Total Header = 32 bytes

#[repr(C, packed)]
pub struct InputEventPayload {
    pub code_or_button: u32,     // Keycode, mouse button, or gesture ID (4 bytes)
    pub x: i32,                  // Absolute/relative X coordinate or delta (4 bytes)
    pub y: i32,                  // Absolute/relative Y coordinate or delta (4 bytes)
    pub modifiers: u32,          // Active modifier bitmask (Shift, Ctrl, Alt, Super) (4 bytes)
    pub touch_id: u32,           // Touch point tracker ID (4 bytes)
    pub pressure: u32,           // Pressure / analog value (4 bytes)
    pub reserved: [u8; 8],       // Reserved fields — MUST BE ZERO (8 bytes)
}                                // Total Payload = 32 bytes

pub struct InputEvent {
    pub header: InputEventHeader,   // 32 bytes
    pub payload: InputEventPayload, // 32 bytes
}                                   // Total Size = 64 bytes
```

### Frozen Event Type Enums (`event_type: u16`):

```rust
pub const EVENT_TYPE_KEY: u16            = 0x0001; // Key press / release
pub const EVENT_TYPE_BUTTON: u16         = 0x0002; // Mouse/pointer button press / release
pub const EVENT_TYPE_POINTER_MOTION: u16 = 0x0003; // Pointer motion (X, Y deltas or absolute coordinates)
pub const EVENT_TYPE_TOUCH: u16          = 0x0004; // Multi-touch point events (touch_id, pressure)
pub const EVENT_TYPE_SCROLL: u16         = 0x0005; // Wheel scroll deltas (x=horizontal, y=vertical)
pub const EVENT_TYPE_DEVICE_STATE: u16   = 0x0006; // Device attach / detach / generation reset
```

- **Wire Validation Rules**: `payload.reserved` MUST be zero. Any `InputEvent` containing an unknown `event_type` or invalid `source_provenance` bitmask is dropped immediately without mutating system input state.

---

## 7. Unforgeable Input-Source Provenance

`source_provenance: u16` is assigned **exclusively by `uids`** upon event ingestion based on transport identity and kernel capability validation:

```rust
pub const INPUT_SOURCE_PHYSICAL: u16       = 0x0000; // Physical hardware IRQ via DevCap
pub const INPUT_SOURCE_ACCESSIBILITY: u16  = 0x0001; // Authorized accessibility synthetic input
pub const INPUT_SOURCE_AUTOMATION: u16     = 0x0002; // Test automation synthetic input (SyntheticInputCap)
pub const INPUT_SOURCE_AGENT: u16          = 0x0003; // Agent runtime requested event
pub const INPUT_SOURCE_REMOTE: u16         = 0x0004; // Remote network stream (RemoteInputPolicyCap + CSDT)
pub const INPUT_SOURCE_TRUSTED_AUTH: u16   = 0x0005; // ModalLock authui trusted path
```

- **Unforgeability Guarantee**: Applications cannot self-declare provenance flags. `uids` overrides any payload provenance flags with the validated IPC socket capability identity before delivering the event to target surfaces (`I-INPUT-SOURCE-PROVENANCE`).

---

## 8. Focus & Routing Model (Policy vs. Enforcement)

```text
                          ┌───────────────────────────┐
                          │ Physical / Remote Device  │
                          └─────────────┬─────────────┘
                                        │
                                        ▼
                          ┌───────────────────────────┐
                          │     uids Ingestion        │
                          └─────────────┬─────────────┘
                                        │
                         Is ModalLock Active (authui)?
                                      ╱   ╲
                                 YES ╱     ╲ NO
                                    ╱       ╲
                                   ▼         ▼
                        ┌─────────────┐   Is System Shortcut?
                        │   authui    │         ╱   ╲
                        │ Trusted Path│    YES ╱     ╲ NO
                        └─────────────┘       ╱       ╲
                                             ▼         ▼
                                  ┌─────────────┐   Get Focused Surface
                                  │   shelld    │   from active workspace
                                  │ System HUD  │         │
                                  └─────────────┘         ▼
                                                ┌──────────────────┐
                                                │ Target Surface   │
                                                │ (App / intentd)  │
                                                └──────────────────┘
```

### Policy vs. Enforcement Boundary:
- `shelld` (**Policy Authority**): Evaluates user interaction state, active session, and active workspace grid layout. Issues `OP_UIDS_SET_FOCUS` to `uids`.
- `uids` (**Enforcement Authority**): Receives focus policy updates, updates its active routing table, enforces `ModalLock` isolation, and performs physical event delivery (`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`).

---

## 9. Trusted-Input Model & `authui` Integration

1. **`ModalLock` Activation**: `uids` acquires `ModalLock` **only** upon receiving `OP_UIDS_REQUEST_MODAL_LOCK` accompanied by a valid `AuthorizationTransactionRef` from `authui`.
2. **Input Stream Isolation**: While `ModalLock` is active, input event queues for all other application surfaces and background workloads are isolated (`FOCUS_STATE_CAPTURED`).
3. **No Synthetic Trusted Input**: `ModalLock` accepts **only** `INPUT_SOURCE_PHYSICAL` events. `INPUT_SOURCE_SYNTHETIC`, `INPUT_SOURCE_AGENT`, or `INPUT_SOURCE_REMOTE` events received during `ModalLock` are dropped immediately (`I-INPUT-TRUSTED-PATH`).
4. **Fail-Closed Release**: `ModalLock` is released exclusively when `authui` presents transaction completion or when `uids` detects `authui` IPC disconnection (crashing `authui` immediately flushes input queues and releases `ModalLock`).

---

## 10. Remote Input & Explicit Local Authorization Boundary

Stage 6B introduced remote surface proxies (`OP_SURFACE_REGISTER_REMOTE_PROXY`). Stage 6C establishes the explicit remote input authorization contract:

```text
Remote Node → netd → fabricd (CSDT Provenance) → workspaced (RemoteInputPolicyCap) → uids → Remote Surface Proxy
```

1. **CSDT $\neq$ Input Authority**: CSDT proves origin identity but **does not** grant local input authority (`I-INPUT-REMOTE-NO-AUTHORITY`).
2. **Explicit `RemoteInputPolicyCap` Required**: `workspaced` and `shelld` must issue an explicit `RemoteInputPolicyCap` authorizing input ingestion for the specific remote session/workspace (`I-INPUT-REMOTE-AUTHORIZATION`).
3. **Strict Remote Isolation**:
   - Remote input is tagged with `INPUT_SOURCE_REMOTE`.
   - Remote input can only interact with remote surface proxies associated with its authorized fabric stream.
   - Remote input **cannot** execute system shortcuts, grant local focus, or enter trusted `authui` overlays.

---

## 11. Workspace Containment & Input State Reconciliation

1. **Workspace Boundary**: Events delivered to surface $S$ carry explicit `workspace_id` validation from `workspaced`.
2. **Input State Reconciliation (`I-INPUT-STATE-RECONCILIATION`)**:
   - When a workspace switch occurs, `uids` discards queued events bound to the deactivated target.
   - `uids` synthesizes key-up / button-up release events for all currently held modifier keys (Shift, Ctrl, Alt, Super) and mouse buttons, delivering them to the deactivated surface.
   - `uids` clears its transient held-key table and requires fresh physical keypress/click observations before registering held inputs in the newly activated workspace.

---

## 12. Observation vs. Interpretation (`uids` → `intentd`)

1. **`uids` Role**: Captures physical keyboard/pointer events, matches physical shortcut keys, and formats raw intent payloads. `uids` acts strictly as an **Observation & Shortcut Extraction Agent** (`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`).
2. **`intentd` Role**: Acts as the **Intent Interpretation Authority**. `intentd` independently receives the raw payload via `OP_INTENT_SUBMIT` (`0x0709`), validates intent context against Stage 4F safety rules, and generates an actionable Plan or DAG. `uids` cannot self-declare or execute an intent.

---

## 13. Lifecycle & Failure Recovery Matrix

```text
Discovered ──► Attached ──► Ready ──► Active ──► Quiescing ──► Detached ──► Released
```

| Component Failure | Security Effect | Recovery Action |
|---|---|---|
| **`uids` Crash** | Ingestion pauses; zero key leakage. | `init` restarts `uids`, re-binds `DevCap`. `shelld` resends active focus state (`OP_UIDS_SET_FOCUS`). |
| **`authui` Crash** | Active modal input lock invalidated. | `agentd` marks pending transactions `DENIED`. `uids` releases `ModalLock` and flushes input queues (Fail-Closed). |
| **`shelld` Crash** | Focus updates pause. | `uids` enters `FOCUS_STATE_QUARANTINED` (drops application input) until `shelld` completes crash recovery and re-establishes focus policy. |
| **Device Disconnect** | Hardware stream interrupted. | `uids` increments `device_generation`, flushes device queues, transitions device state to `Detached`. |

---

## 14. Security Threat Model

| Threat Vector | Exploit Mechanism | Stage 6C Countermeasure & Invariant |
|---|---|---|
| **Keylogging** | Background app listens to global keystrokes. | Strict focus routing: `uids` sends events only to single focused surface (`I-INPUT-NO-KEYLEAK`). |
| **Focus Stealing** | Malicious app requests focus during password entry. | Focus policy owned solely by `shelld` (`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`). Unprivileged focus requests rejected (`I-INPUT-NO-FOCUS-STEAL`). |
| **Credential Interception** | App captures credentials in modal dialogs. | `authui` `ModalLock` isolates input stream completely (`I-INPUT-TRUSTED-PATH`). |
| **Synthetic Impersonation** | Script generates fake user confirmation click. | `ModalLock` rejects non-physical events unconditionally (`I-INPUT-SYNTHETIC-BOUND`). |
| **Remote Privilege Escalation** | Remote node sends fake system shortcut via CSDT. | CSDT alone grants zero authority; remote input requires explicit `RemoteInputPolicyCap` (`I-INPUT-REMOTE-AUTHORIZATION`). |
| **Stale Event / Stuck Key Leak** | Delayed keypresses executed across workspace. | Workspace switch triggers state reconciliation and modifier key release synthesis (`I-INPUT-STATE-RECONCILIATION`). |

---

## 15. Resource & Accounting Implications

- **RAM Quotas**: Ring buffers in `uids` use static arrays (max 256 pending 64-byte events per queue = 16 KB total). Zero dynamic allocation in kernel space.
- **CPU Accounting**: Decoding and hit-testing run within `uids` user-space thread budget under Stage 4B CPU limits.
- **Coalescing**: Pointer motion events are coalesced within a 16ms frame window to prevent IPC saturation.

---

## 16. ABI / IPC Impact

No kernel syscall additions. Standardized IPC opcodes in `libzero`:

| Opcode | Sender | Receiver | Purpose | Payload Summary |
|---|---|---|---|---|
| `0x0719` | `shelld` | `uids` | Set Active Focus Target | `{ session_id: u64, workspace_id: u64, surface_id: u64 }` |
| `0x071A` | `uids` | `shelld` | Focus State Change Ack | `{ status: i32, surface_id: u64 }` |
| `0x071B` | `authui` | `uids` | Request Modal Lock | `{ transaction_token: DistributedId }` |
| `0x071C` | `uids` | `authui` | Modal Lock Status | `{ status: i32 }` |
| `0x071D` | `fabricd` | `uids` | Deliver Remote Input Frame | `{ csdt_id: DistributedId, frame: InputEvent }` |
| `0x071E` | `uids` | `intentd` | Ingest Human Intent | `{ workspace_id: DistributedId, intent_len: u32, payload: [u8; 512] }` |

---

## 17. Persistence Boundary

| State Element | Lifetime Category | Description |
|---|---|---|
| **Raw Input Events** | Ephemeral | Processed immediately and discarded. Never persisted. |
| **Decoded `InputEvent` Buffers**| Ephemeral | Bounded in-memory ring buffers; flushed on workspace switch. |
| **Active Focus Target** | Ephemeral | Maintained in `uids` RAM; re-synced from `shelld` on crash. |
| **`ModalLock` State** | Ephemeral | Valid only during active `AuthorizationTransaction`. |
| **Device Connection Table** | Reconstructable | Re-queried from Stage 3L IRQ registry on `uids` startup. |
| **Input Policy Preferences** | Persistent | Keyboard layout, pointer speed stored in `workspaced` persistent context. |

---

## 18. Machine-Verifiable Invariants

1. **`I-INPUT-SESSION-CONTAINMENT`**: Input events must be tagged with and constrained to the active `SessionId`.
2. **`I-INPUT-WORKSPACE-CONTAINMENT`**: Input events must not cross workspace boundaries; workspace transitions flush pending queues.
3. **`I-INPUT-PER-DEVICE-ORDER`**: Event sequence ordering is strictly enforced per physical/logical device; cross-device ordering uses receiver-local qualified monotonic TSC with deterministic tie-breaking.
4. **`I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT`**: `shelld` is the Focus Policy Authority (`SessionManagementCap`); `uids` is the Focus Enforcement Authority (`InputFocusPolicyCap`).
5. **`I-INPUT-NO-CAPABILITY-AMPLIFICATION`**: Ingesting or routing an input event yields zero kernel capability amplification.
6. **`I-INPUT-TRUSTED-PATH`**: While `authui` holds `ModalLock`, 100% of non-auth input queues are isolated and quarantined.
7. **`I-INPUT-NO-KEYLEAK`**: Non-focused surfaces receive zero keypress or pointer event data.
8. **`I-INPUT-NO-FOCUS-STEAL`**: Background tasks cannot alter active focus state or break `ModalLock`.
9. **`I-INPUT-GENERATION-BOUND`**: Out-of-order or stale device generation events are discarded without state mutation.
10. **`I-INPUT-REMOTE-AUTHORIZATION`**: Remote input streams (CSDT) require an explicit local `RemoteInputPolicyCap` issued by `workspaced`/`shelld`. CSDT validation alone SHALL NEVER grant input authority.
11. **`I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY`**: `uids` performs event observation; `intentd` independently validates intent interpretation. `uids` yields zero implicit intent authority.
12. **`I-INPUT-SOURCE-PROVENANCE`**: Unforgeable `InputSource` provenance is assigned exclusively by `uids` based on kernel capability validation. Applications cannot self-declare provenance.
13. **`I-INPUT-STATE-RECONCILIATION`**: Workspace/session switch triggers input queue flushing and modifier/button release state reconciliation.
14. **`I-INPUT-FAIL-CLOSED`**: Daemon crash (`uids`, `authui`, `shelld`) causes input state to reset to a secure, fail-closed neutral state.

---

## 19. Open Architectural Questions

1. **High-DPI / Touch Multi-Gesture Transformation**: Should touch gesture decoding (e.g., pinch-to-zoom) occur inside `uids` or be passed as raw multi-touch points to `surfaced` for visual transformation?
   - *Discovery Conclusion*: `uids` decodes basic gestures (pan, tap) into canonical `InputEventPayload`, while complex surface multi-touch mapping is performed using `surfaced` viewport transforms.
2. **Accessibility Hook Authority**: How should screen readers or assistive synthetic input engines request high-privilege event monitoring without violating `I-INPUT-NO-KEYLEAK`?
   - *Discovery Conclusion*: Require an explicit `AccessibilityPolicyCap` issued by `init` and validated by `shelld` with visual badge indicator.

---

## 20. Explicit Implementation Boundary

```text
Stage 6C Discovery Rev2           🟢 COMPLETE
Stage 6C Architecture Specification 🔴 NOT YET DRAFTED
Stage 6C Implementation Plan        🔴 NOT YET DRAFTED
Stage 6C Code Implementation        🔴 NOT AUTHORIZED
```

**Implementation must not begin until Stage 6C Architecture Specification is formally reviewed and frozen.**

---

## Discovery Verdict

```text
🟢 DISCOVERY REV2 COMPLETE — READY FOR ARCHITECTURE DRAFT
```
