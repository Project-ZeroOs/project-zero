# Stage 6C — Architecture Discovery & Dependency Audit

**Topic:** Human Input, Interaction Routing & Intent Boundary Subsystem  
**Status:** 🟢 DISCOVERY COMPLETE — READY FOR ARCHITECTURE DRAFT  
**Authoritative Substrate:**  
- Stage 3A–3N Kernel Nucleus — 🟢 Frozen  
- Stage 4A–4F Core System Services — 🟢 Frozen  
- Stage 5 Presentation Subsystem — 🟢 Frozen  
- Stage 6A User Session Substrate — 🟢 Frozen  
- Stage 6B Distributed Spatial Presentation — 🟢 Frozen  

---

## 1. Executive Architectural Conclusion

Stage 6C does **not** require a new standalone microkernel daemon or a duplicate capability system. Instead, Stage 6C is defined as a **Coordinated Multi-Service Boundary** centered around `uids` as the **User Interaction Ingestion & Input Focus Enforcer**, operating under strict authority delegation from `init` and policy coordination with `shelld`, `authui`, `surfaced`, `compositord`, `workspaced`, and `intentd`.

### Core Architectural Decisions:

1. **`uids` is an Ingestion & Focus Enforcer, NOT an Intent Authority**: `uids` ingests physical/remote hardware events, applies focus transformation rules dictated by `shelld`, and routes decoded events to target application surfaces or `intentd`. `uids` possesses zero execution capabilities, zero privilege amplification, and zero intent resolution authority (`I-INPUT-NO-IMPLICIT-AUTHORITY`).
2. **Focus Authority Belongs to `shelld`**: `shelld` holds `SessionManagementCap` and acts as the single source of session/workspace focus policy. `shelld` issues focus target updates (`OP_UIDS_SET_FOCUS`) to `uids`. `uids` enforces input focus under its `InputFocusPolicyCap`.
3. **Trusted Path Belongs Exclusively to `authui`**: When an `AuthorizationTransaction` is active, `authui` requests a `ModalLock` from `uids`. During `ModalLock`, 100% of input events are quarantined and routed exclusively to `authui`. No background workload, shell component, or agent can keylog or intercept trusted authentication input (`I-INPUT-TRUSTED-PATH`).
4. **Remote Input Is Subordinate & Remote CSDT $\neq$ Input Authority**: Remote input received via `netd`/`fabricd` carries CSDT for provenance context only. It requires explicit local workspace membership validation by `workspaced` and surface binding by `surfaced` before `uids` will deliver events to a surface. Remote input can **never** trigger system shortcuts, acquire `ModalLock`, or deliver trusted input (`I-INPUT-REMOTE-NO-AUTHORITY`).
5. **No Kernel Syscall Additions**: Stage 3L device drivers and IRQ queues provide the physical input event stream. No new kernel syscalls or kernel-level input hooks are created.

---

## 2. Scope

The Stage 6C discovery covers:

- Ingestion of hardware input events (keyboard, pointer/mouse, touch, HID) via Stage 3L device interfaces.
- Canonical 64-byte `InputEvent` representation and Little-Endian wire contract.
- Input focus routing hierarchy across `uids`, `shelld`, `authui`, `surfaced`, `compositord`, `workspaced`, and `intentd`.
- Quarantine and routing of trusted authentication input (`ModalLock`).
- Synthetic input bounds and accessibility automation boundaries.
- Remote input event ingestion and CSDT non-authority separation.
- Containment of input events within authorized session and workspace boundaries.
- Threat modeling against focus stealing, keylogging, input replay, and synthetic event injection.
- Fail-closed crash recovery for input state.

---

## 3. Non-Goals

1. **No Kernel Modifications**: Zero changes to Stage 3A–3N production Ring-0 nucleus.
2. **No Second Capability System**: All input permissions rely strictly on Stage 3H capability handles (`InputFocusPolicyCap`, `SystemSurfacePolicyCap`, `SessionManagementCap`).
3. **No Intent Resolution in `uids`**: `uids` does not parse, evaluate, or execute natural-language intents or complex commands. It passes raw intent payloads to `intentd`.
4. **No Remote Input Authority**: CSDT tokens and remote surface visibility do not grant remote nodes input authority over local system components or non-owned local surfaces.
5. **No Persistent Event Storage**: Input events are strictly ephemeral and are flushed on session/workspace transitions.

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
(Session/Workspace               (Trusted Auth Input            (Intent Resolution
 Focus Policy)                       ModalLock)                     Authority)
         │                                 │                        │
         ▼                                 ▼                        ▼
    workspaced                         surfaced                 compositord
(Workspace Containment             (Surface Viewport &      (Non-blocking Display
   Membership)                      Hit-Testing Bounds)          Scanout)
```

| Service | Stage Introduced | Stage 6C Role & Dependency Contract |
|---|---|---|
| **`uids`** | Stage 5 | Ingests physical/remote input, decodes 64-byte frames, enforces focus routing. |
| **`shelld`** | Stage 6A | Holds `SessionManagementCap`, dictates active workspace/surface focus policy to `uids`. |
| **`authui`** | Stage 5 | Holds trusted input overlay authority; requests `ModalLock` from `uids` for auth transactions. |
| **`surfaced`** | Stage 5 / 6B | Surface lifecycle authority; provides viewport bounds and hit-testing coordinates. |
| **`compositord`**| Stage 5 / 6B | Receives non-blocking frame buffers and cursor overlays from `surfaced`/`uids`. |
| **`workspaced`**| Stage 4D | Validates workspace membership and lifecycle state prior to event delivery. |
| **`intentd`** | Stage 4F | Receives human intent payloads from `uids` via `OP_INTENT_SUBMIT` (`0x0709`). |
| **`fabricd`** | Stage 4F / 6B | Delivers remote input events from network with CSDT provenance metadata. |

---

## 5. Authority Matrix

| Domain / Resource | Authoritative Owner | Capability Required | Enforcement Point |
|---|---|---|---|
| **Physical Input Devices** | `uids` | `DevCap` (Stage 3L) | `init` boot delegation |
| **Input Focus Target** | `shelld` | `SessionManagementCap` | `shelld` → `uids` (`OP_UIDS_SET_FOCUS`) |
| **Focus Routing Enforcement** | `uids` | `InputFocusPolicyCap` | `uids` event dispatch loop |
| **Trusted Input Path (`ModalLock`)** | `authui` | `AuthorizationTransactionRef` | `authui` → `uids` (`OP_UIDS_REQUEST_MODAL_LOCK`) |
| **Workspace Containment** | `workspaced` | `WorkspaceManagementCap` | `workspaced` membership query |
| **Surface Hit-Testing / Bounds** | `surfaced` | `SurfacePolicyCap` | `surfaced` viewport bounds check |
| **Intent Ingestion Payload** | `intentd` | `IntentSubmissionCap` | `uids` → `intentd` (`OP_INTENT_SUBMIT`) |
| **Remote Input Handling** | `fabricd` / `uids` | Valid CSDT + Workspace Membership | `workspaced` + `uids` validation |

---

## 6. Input Event Model Proposal

Canonical 64-byte `InputEvent` wire contract:

```rust
#[repr(C, packed)]
pub struct InputEventHeader {
    pub device_id: u64,          // Stage 3L Physical/Logical Device ID (8 bytes)
    pub timestamp_ns: u64,       // Receiver-local monotonic nanosecond timestamp (8 bytes)
    pub sequence: u64,           // Device monotonic event sequence counter (8 bytes)
    pub event_type: u16,         // Event type (2 bytes: Key, PointerMotion, PointerButton, Touch, Gesture)
    pub flags: u16,              // Event flags (2 bytes: Physical, Synthetic, Remote, Trusted)
    pub device_generation: u32,  // Monotonic device connection generation (4 bytes)
}                                // Total Header = 32 bytes

#[repr(C, packed)]
pub struct InputEventPayload {
    pub code_or_button: u32,     // Keycode, mouse button, or gesture ID (4 bytes)
    pub x: i32,                  // Absolute/relative X coordinate or delta (4 bytes)
    pub y: i32,                  // Absolute/relative Y coordinate or delta (4 bytes)
    pub modifiers: u32,          // Active modifier bitmask (Shift, Ctrl, Alt, Super) (4 bytes)
    pub touch_id: u32,           // Touch point tracker ID (4 bytes)
    pub pressure: u32,           // Pressure / analog value (4 bytes)
    pub reserved: [u8; 8],       // Reserved padding for wire symmetry (8 bytes)
}                                // Total Payload = 32 bytes

pub struct InputEvent {
    pub header: InputEventHeader,   // 32 bytes
    pub payload: InputEventPayload, // 32 bytes
}                                   // Total Size = 64 bytes
```

### Physical Observation vs. Authority-Bearing Interpretation:

- **Physical Observation**: Raw 64-byte `InputEvent` ingested from Stage 3L device ring buffer by `uids`. Contains `device_id` and raw hardware codes. Has zero authority.
- **Authority-Bearing Interpretation**: Context-decorated event created in-memory by `uids` after applying active focus lookup. Attaches `session_id`, `workspace_id`, and target `surface_id`. Routed strictly to the authorized target.

---

## 7. Focus & Routing Model

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

### Routing Rules:
1. **Modal Lock Dominance**: If `authui` holds `ModalLock`, all non-system events are delivered exclusively to `authui`.
2. **System Shortcuts**: Global shortcuts (e.g., Super+Tab, Ctrl+Alt+Del) are intercepted by `uids` and routed directly to `shelld`.
3. **Application Focus**: Ordinary keypresses, mouse events, and touch points are routed to the active `surface_id` determined by `shelld` and validated by `workspaced`.

---

## 8. Trusted-Input Model

To prevent credential theft and keylogging by untrusted processes:

1. **`ModalLock` Ingestion Rule**: `uids` transitions to `ModalLock` **only** upon receiving `OP_UIDS_REQUEST_MODAL_LOCK` accompanied by a valid `AuthorizationTransactionRef` from `authui`.
2. **Input Isolation**: While `ModalLock` is active, input event queues for all other applications and background workloads are frozen and isolated (`FOCUS_STATE_CAPTURED`).
3. **Release Guarantee**: `ModalLock` is released exclusively when `authui` submits transaction completion or when `uids` detects `authui` IPC disconnection (crashing `authui` immediately flushes input queues and releases lock).
4. **No Synthetic Trusted Input**: Trusted input must carry `INPUT_FLAG_PHYSICAL`. Synthetic or remote input received during `ModalLock` is unconditionally dropped (`I-INPUT-TRUSTED-PATH`).

---

## 9. Synthetic-Input Model

ZeroOS distinguishes physical human input from automated or synthetic input:

1. **Synthetic Input Flags**: Synthetic events generated by testing harnesses or accessibility tools carry `INPUT_FLAG_SYNTHETIC`.
2. **Capability Prerequisite**: Generating synthetic events requires `SyntheticInputCap` issued by `init`.
3. **Restrictions on Synthetic Input**:
   - Synthetic input **cannot** pass through the trusted input path (`ModalLock`).
   - Synthetic input **cannot** trigger high-privilege system shortcuts.
   - Synthetic input **cannot** impersonate physical user presence for safety confirmation prompts (`I-INPUT-SYNTHETIC-BOUND`).

---

## 10. Remote-Input Model / Stage 6B Integration

Stage 6B introduced remote surface proxies (`OP_SURFACE_REGISTER_REMOTE_PROXY`). Stage 6C defines the interaction boundary for remote surfaces:

1. **Remote Input Path**:
   ```text
   Remote Peer → netd → fabricd (CSDT validation) → workspaced (Session check) → uids → Remote Surface Proxy
   ```
2. **CSDT Is Provenance Only**: A valid CSDT proves network origin and identity, but does **not** grant local input authority (`I-INPUT-REMOTE-NO-AUTHORITY`).
3. **Remote Event Flagging**: All input originating from a remote node carries `INPUT_FLAG_REMOTE`.
4. **Strict Isolation**:
   - Remote input can only interact with remote surface proxies owned by the corresponding fabric stream.
   - Remote input **cannot** move focus to local surfaces.
   - Remote input **cannot** execute system shortcuts.
   - Remote input **cannot** enter `authui` modal overlays.

---

## 11. Workspace & Session Containment

1. **Hierarchy**: `Input Event → Active Session → Active Workspace → Target Surface`.
2. **Validation**: Before `uids` delivers an event to surface $S$, it verifies that $S \in \text{SessionRecord.authorized\_workspaces}$.
3. **State Transition Flushing**:
   - When a workspace is suspended, closed, or switched, `shelld` notifies `uids` via `OP_UIDS_SET_FOCUS`.
   - `uids` instantly flushes all pending input event queues for the deactivated workspace (`I-INPUT-WORKSPACE-CONTAINMENT`).
   - Prevents stale keypresses from leaking into newly activated workspaces.

---

## 12. Lifecycle & Recovery Model

Input device and routing state lifecycle:

```text
Discovered ──► Attached ──► Ready ──► Active ──► Quiescing ──► Detached ──► Released
```

### Crash & Failure Recovery Matrix:

| Component Failure | Security Effect | Recovery Action |
|---|---|---|
| **`uids` Crash** | Input ingestion stops; no key leakage. | `init` restarts `uids`, re-binds `DevCap`. `shelld` resends active focus state (`OP_UIDS_SET_FOCUS`). |
| **`authui` Crash** | Active modal input lock invalidated. | `agentd` marks pending transactions `DENIED`. `uids` releases `ModalLock` and flushes input queues (Fail-Closed). |
| **`shelld` Crash** | Focus updates pause. | `uids` enters `FOCUS_STATE_QUARANTINED` (drops application input) until `shelld` completes crash recovery and re-establishes focus policy. |
| **Device Disconnect** | Hardware stream interrupted. | `uids` increments `device_generation`, flushes device queues, transitions device state to `Detached`. |

---

## 13. Security Threat Model

| Threat Vector | Exploit Mechanism | Stage 6C Countermeasure & Invariant |
|---|---|---|
| **Keylogging** | Background app listens to global keystrokes. | Strict focus routing: `uids` only sends events to the single focused surface. No global keyhooks (`I-INPUT-NO-KEYLEAK`). |
| **Focus Stealing** | Malicious app requests focus during password entry. | Focus policy owned solely by `shelld`. `uids` rejects unprivileged focus requests (`I-INPUT-NO-FOCUS-STEAL`). |
| **Credential Interception** | App captures credentials in modal dialogs. | `authui` `ModalLock` isolates input stream completely during authentication (`I-INPUT-TRUSTED-PATH`). |
| **Synthetic Impersonation** | Script generates fake user confirmation click. | `ModalLock` rejects `INPUT_FLAG_SYNTHETIC` events unconditionally (`I-INPUT-SYNTHETIC-BOUND`). |
| **Remote Privilege Escalation** | Remote node sends fake system shortcut via CSDT. | Remote input carries `INPUT_FLAG_REMOTE`; system shortcuts and local focus changes prohibited (`I-INPUT-REMOTE-NO-AUTHORITY`). |
| **Stale Event Injection** | Delayed keypresses executed after workspace switch. | Workspace switch triggers mandatory input queue flush in `uids` (`I-INPUT-WORKSPACE-CONTAINMENT`). |

---

## 14. Resource & Accounting Implications

- **RAM Quotas**: `uids` event ring buffers use fixed-size static arrays (e.g., max 256 pending 64-byte events per queue = 16 KB total). Zero dynamic allocation in kernel space.
- **CPU Accounting**: Input event decoding and hit-testing executed within `uids` user-space thread budget under Stage 4B CPU resource limits.
- **Coalescing**: High-frequency pointer motion events are coalesced within a 16ms compositor frame window to prevent IPC channel saturation.

---

## 15. ABI / IPC Impact

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

## 16. Persistence Boundary

| State Element | Lifetime Category | Description |
|---|---|---|
| **Raw Input Events** | Ephemeral | Processed immediately and discarded. Never persisted. |
| **Decoded `InputEvent` Buffers**| Ephemeral | Bounded in-memory ring buffers; flushed on workspace switch. |
| **Active Focus Target** | Ephemeral | Maintained in `uids` RAM; re-synced from `shelld` on crash. |
| **`ModalLock` State** | Ephemeral | Valid only during active `AuthorizationTransaction`. |
| **Device Connection Table** | Reconstructable | Re-queried from Stage 3L IRQ registry on `uids` startup. |
| **Input Policy Preferences** | Persistent | Keyboard layout, pointer speed stored in `workspaced` persistent context. |

---

## 17. Machine-Verifiable Invariants

The following 12 machine-verifiable invariants are proposed for Stage 6C architecture verification:

1. **`I-INPUT-SESSION-CONTAINMENT`**: Input events must be tagged with and constrained to the active `SessionId`.
2. **`I-INPUT-WORKSPACE-CONTAINMENT`**: Input events must not cross workspace boundaries; workspace transitions flush pending queues.
3. **`I-INPUT-FOCUS-AUTHORITY`**: `shelld` is the sole source of focus policy; unprivileged focus requests are rejected.
4. **`I-INPUT-NO-CAPABILITY-AMPLIFICATION`**: Ingesting or routing an input event yields zero kernel capability amplification.
5. **`I-INPUT-TRUSTED-PATH`**: While `authui` holds `ModalLock`, 100% of non-auth input queues are isolated and quarantined.
6. **`I-INPUT-NO-KEYLEAK`**: Non-focused surfaces receive zero keypress or pointer event data.
7. **`I-INPUT-NO-FOCUS-STEAL`**: Background tasks cannot alter active focus state or break `ModalLock`.
8. **`I-INPUT-GENERATION-BOUND`**: Out-of-order or stale device generation events are discarded without state mutation.
9. **`I-INPUT-REMOTE-NO-AUTHORITY`**: Remote input (CSDT) cannot execute system shortcuts, grant local focus, or enter trusted overlays.
10. **`I-INPUT-SYNTHETIC-BOUND`**: Synthetic input cannot enter `ModalLock` or simulate human authentication approval.
11. **`I-INPUT-ORDERING`**: Receiver-local monotonic timestamps and device sequence numbers guarantee deterministic event ordering.
12. **`I-INPUT-FAIL-CLOSED`**: Daemon crash (`uids`, `authui`, `shelld`) causes input state to reset to a secure, fail-closed neutral state.

---

## 18. Open Architectural Questions

1. **High-DPI / Touch Multi-Gesture Transformation**: Should touch gesture decoding (e.g., pinch-to-zoom) occur inside `uids` or be passed as raw multi-touch points to `surfaced` for visual transformation?
   - *Initial Discovery Recommendation*: `uids` decodes basic gestures (pan, tap) into canonical `InputEventPayload`, while complex surface multi-touch mapping is performed using `surfaced` viewport transforms.
2. **Accessibility Hook Authority**: How should screen readers or assistive synthetic input engines request high-privilege event monitoring without violating `I-INPUT-NO-KEYLEAK`?
   - *Initial Discovery Recommendation*: Require an explicit `AccessibilityPolicyCap` issued by `init` and validated by `shelld` with visual badge indicator.

---

## 19. Explicit Implementation Boundary

```text
Stage 6C Discovery Rev1           🟢 COMPLETE
Stage 6C Architecture Specification 🔴 NOT YET DRAFTED
Stage 6C Implementation Plan        🔴 NOT YET DRAFTED
Stage 6C Code Implementation        🔴 NOT AUTHORIZED
```

**Implementation must not begin until Stage 6C Architecture Specification is formally reviewed and frozen.**

---

## Discovery Verdict

```text
🟢 DISCOVERY COMPLETE — READY FOR ARCHITECTURE DRAFT
```
