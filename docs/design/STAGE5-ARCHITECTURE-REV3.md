# Stage 5 Architecture — User Interaction Substrate & Spatial Presentation Subsystem

**Revision:** Rev3  
**Status:** DRAFT — ARCHITECTURE REVIEW REQUIRED  
**Implementation:** NOT AUTHORIZED  
**Authoritative Substrate:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), ADR-0001 through ADR-0030 (Frozen)

---

## 1. Purpose & Core Architectural Identity

> **Phase 5 is the capability-secured human interaction and presentation substrate that connects ZeroOS's intent and workload subsystems to physical human input and visual presentation.**

Stage 5 completes the ZeroOS architectural chain by providing a capability-bounded user interaction substrate. It establishes a trusted human interaction path, zero-copy presentation buffer management, anti-spoofing authorization UI rendering, and spatial workspace context presentation without modifying the frozen Stage 3 kernel nucleus or Stage 4 daemon substrate.

---

## 2. Frozen Substrate Audit & Primitive Mapping

Phase 5 explicitly consumes existing frozen primitives across Stage 3 and Stage 4:

```text
Stage 3L Hardware Drivers ──► DeviceId::Display(0), DeviceId::Input(0), sys_dev_map_mmio
Stage 3H Capability Engine ──► sys_cap_derive, sys_cap_revoke, C-Lists, Rights Attenuation
Stage 3G IPC & Shared Mem ──► ShmObject, sys_shm_map, W^X Unconditional NX, IPC Channels
Stage 3B Scheduler Core   ──► Priority::Critical (Compositor Thread), Priority::Normal
Stage 4D Workspace Engine ──► Context Graph, Spatial Viewport Metadata
Stage 4E Agent Runtime    ──► OP_AGENT_REQUEST_HUMAN_AUTH, Agent Security Containment
Stage 4F Intent Resolver  ──► OP_INTENT_SUBMIT, IntentDescriptor Execution Plans
```

### 2.1 Distinction Between MMIO Mapping and SHM Mapping
Stage 5 strictly distinguishes between device memory and shared process memory:
- **Display VRAM MMIO Mapping (`sys_dev_map_mmio`)**: Used exclusively by `compositord` with an authorized Stage 3L `DevCap` to map physical video memory registers.
- **Surface Framebuffer Mapping (`sys_shm_map`)**: Used by applications and `compositord` to map Stage 3G `ShmObject` shared memory buffers into their respective Ring 3 virtual address spaces.

**Kernel Primitives Requirement:** Exactly **ZERO** new kernel primitives are required. Stage 5 operates entirely in Ring 3 userspace using Stage 3G `ShmObject` memory, Stage 3H capability derivation, Stage 3L device capabilities, and Stage 3L `sys_dev_map_mmio`.

---

## 3. Physical Display & Device Authority Path

```text
Physical Display Hardware
          │
          ▼
Stage 3L DeviceId::Display(0)
          │
          ▼ (DevCap granted to compositord by init at boot)
compositord (Ring 3 User Space)
          │
          ▼ (sys_dev_map_mmio of MMIO VRAM via Stage 3L DevCap)
Physical VRAM Scanout Framebuffer (1024x768, ARGB8888, 3 MiB)
```

### 3.1 Display Authority & Mapping Contract
1. **Device Identification**: Display hardware is identified by Stage 3L `DeviceId::Display(0)`.
2. **Exclusive Display Ownership**: `init` holds the root `DevCap` for display hardware and delegates an attenuated `DevCap` (display write/scanout rights) **exclusively** to `compositord` upon startup (`OP_INIT_BIND_DISPLAY`). No other process is granted display `DevCap`.
3. **MMIO Mapping Primitive**: `compositord` maps physical video memory using `sys_dev_map_mmio(dev_cap, virt_addr, phys_mmio_addr, size)`. Unprivileged processes hold no `DevCap` and cannot invoke MMIO mapping on the display aperture.
4. **Display Format & Scanout**: Video mode fixed at $1024 \times 768$ pixels, 32 bits-per-pixel (ARGB8888), $4096\text{-byte}$ row stride, $3,145,728\text{-byte}$ total VRAM footprint.
5. **Scheduler Latency Semantics**: `compositord` thread executes in Stage 3B **Priority::Critical**. Whenever runnable, the scheduler selects `compositord` ahead of `Priority::Normal` background tasks. Soft target frame budget is $16.6\text{ ms}$ ($60\text{ Hz}$). Composition software blit takes $<2.5\text{ ms}$ for dirty rects. If a frame deadline is missed, the previous frame is retained without display corruption (buffer starvation safety).

---

## 4. Capability Model: Elimination of Parallel Surface Capabilities

Stage 5 introduces **NO new capability system** and **NO `SurfaceCap` primitive**.

Surface authorization is strictly derived from Stage 3H kernel capabilities:

```text
Workspace Capability (C_ws) [Stage 3H]
          │
          ▼ (sys_cap_derive via Workspace Authority)
Surface Framebuffer SHM Capability (C_surf) [Stage 3G ShmObject]
          │
          ├──► Granted to Application: SHM_MAP_READ | SHM_MAP_WRITE
          └──► Granted to Compositor:  SHM_MAP_READ
```

### 4.1 Surface Derivation Rules
- **No Parallel Handles**: Surfaces are identified by their Stage 3G `ShmObject` capability handle ($C_{surf}$).
- **Attenuated Access**: Application processes receive `SHM_MAP_WRITE` rights to draw into their assigned presentation buffer. `compositord` receives `SHM_MAP_READ` rights to sample framebuffers for presentation.
- **Revocation Lineage**: Calling `sys_cap_revoke` on the parent `WorkspaceCap` ($C_{ws}$) automatically nullifies all derived surface `ShmObject` capabilities, instantly revoking rendering and composition rights across all processes.

---

## 5. Trusted Human Authorization Pipeline & Transaction Binding

### 5.1 Authorization Request Spoofing Prevention
A numeric z-index alone does not guarantee UI security. Stage 5 establishes a **cryptographically bound, trusted presentation pipeline**:

```text
agentd (Stage 4E)
  │ (Dispatches OP_AGENT_REQUEST_HUMAN_AUTH)
  ▼
AuthorizationTransaction
  ├── transaction_id: DistributedId
  ├── agent_id: DistributedId
  ├── workspace_id: DistributedId
  ├── operation_id: DistributedId
  ├── requested_action_hash: [u8; 32]
  ├── nonce: u64
  └── expiry_tsc: u64
  │
  ▼ (Issued capability token C_auth_tx to authui via brokerd)
authui (Trusted Authorization Overlay)
  │
  ├──► Registers Presentation Object via surfaced using C_auth_tx
  │      └── surfaced validates C_auth_tx -> marks SURFACE_TYPE_AUTH_OVERLAY
  ├──► Requests Modal Input Lock from uids using C_auth_tx
  └──► Renders Transaction Details & Visual HMAC Badge (Session Secret K_session)
```

### 5.2 Authorization Security Invariants
1. **Transaction Binding**: Approval decisions (`UserApproved` / `UserDenied`) are cryptographically bound to `transaction_id` and `requested_action_hash`. An approval token cannot be reused for a different operation or workload.
2. **Capability-Gated Overlay Type (`SURFACE_TYPE_AUTH_OVERLAY`)**: `surfaced` permits setting `SURFACE_TYPE_AUTH_OVERLAY` **only** upon presenting a valid $C_{auth\_tx}$ capability token issued by `brokerd`/`agentd`. Unprivileged applications attempting to create an auth overlay surface are rejected with `ZeroError::PermissionDenied`.
3. **Compositor Composition Guarantee**: `compositord` renders `SURFACE_TYPE_AUTH_OVERLAY` strictly on top of all regular workspace surfaces. Peer surfaces cannot overlap, obscure, or intercept pixels within the auth overlay region.
4. **Visual HMAC Badge**: `authui` imprints a visual HMAC badge derived from a boot session secret ($K_{session}$). Peer surfaces cannot predict $K_{session}$ or obtain `SURFACE_TYPE_AUTH_OVERLAY`, rendering peer UI spoofing impossible.
5. **Crash Fail-Closed**: If `authui` crashes or loses connection during an active prompt, `agentd` immediately transitions the pending `AuthorizationTransaction` to **DENIED** (Fail-Closed).

---

## 6. Input Ingestion & ModalLock Authority (`I-INPUT-NO-IMPLICIT-AUTHORITY`)

```text
Stage 3L Hardware Input Interrupt (Keyboard/Pointer)
          │
          ▼
uids (User Interaction Ingestion Daemon)
          │
          ▼ (Input Event Parsing & Focus Resolution)
  +-------+-------+
  │               │
  ▼               ▼
[Application Focus]  [User Intent Formulator]
  │               │
  ▼               ▼
App Input Stream   IntentDescriptor Payload
                  │
                  ▼ (Dispatches OP_INTENT_SUBMIT)
                intentd (Stage 4F Intent Resolver)
```

### 6.1 ModalLock & Focus Authority Invariants
1. **Focus Authority**: `uids` is the sole owner and enforcer of input focus state. No user-space process can self-declare input capture or modal lock merely by modifying local state.
2. **`ModalLock` Ingestion Rule**: `uids` transitions to `ModalLock` **only** when `authui` presents a valid $C_{auth\_tx}$ token (`OP_UIDS_REQUEST_MODAL_LOCK`). Applications presenting no $C_{auth\_tx}$ are rejected with `ZeroError::PermissionDenied`.
3. **Input Muting**: While `ModalLock` is active, 100% of input events are routed to `authui`. Peer applications receive `FocusLost` notifications and zero key/pointer events.
4. **Release & Teardown**: `ModalLock` is released exclusively when `authui` presents the completed transaction result or when `uids` detects `authui` IPC disconnection (crashing `authui` releases `ModalLock` immediately).
5. **Race Prevention**: Only one `ModalLock` can be active at any instant. Concurrent `ModalLock` requests are queued or rejected (`ZeroError::Busy`).
6. **Invariant `I-INPUT-NO-IMPLICIT-AUTHORITY`**: `uids` possesses ZERO capability authority, ZERO workload execution authority, and cannot approve authorization requests. All human intents must pass full `intentd` validation.

---

## 7. Zero-Copy Presentation Buffer Lifecycle & Producer/Consumer Ownership

Presentation surfaces utilize Stage 3G `ShmObject` buffers governed by a strict state transition matrix and explicit transition rights:

```rust
#[repr(C)]
pub struct PresentationBufferHeader {
    pub magic: u32,               // 0x5A505549 ("ZPUI")
    pub width: u32,               // Pixel width (1024)
    pub height: u32,              // Pixel height (768)
    pub stride_bytes: u32,        // Row stride in bytes (4096)
    pub format: u32,              // Pixel format (1 = ARGB8888)
    pub active_front_buffer: u8,  // 0 or 1 (Double-buffering)
    pub buffer_state: [u8; 2],    // STATE_FREE (0), STATE_WRITING (1), STATE_READY (2), STATE_COMPOSITING (3)
    pub damage_rect: [u32; 4],    // [x, y, width, height]
    pub sequence_num: u64,        // Monotonic frame counter
}
```

### 7.1 State Transition Matrix & Transition Rights

```text
STATE_FREE ──(Producer)──► STATE_WRITING ──(Producer)──► STATE_READY
    ▲                                                          │
    │                                                      (Consumer)
    └─────────────────(Consumer)◄─── STATE_COMPOSITING ◄───────┘
```

| State Transition | Authorized Principal | Trigger Event | Crash / Teardown Safety Action |
|---|---|---|---|
| `FREE -> WRITING` | Application (Producer) | Application begins rendering frame. | If producer crashes in `WRITING`, `surfaced` resets buffer state to `FREE` on IPC teardown. |
| `WRITING -> READY` | Application (Producer) | Application completes rendering; dispatches `OP_SURFACE_COMMIT`. | If producer crashes in `READY`, `compositord` blits frame once and transitions buffer to `FREE`. |
| `READY -> COMPOSITING` | `compositord` (Consumer) | Compositor begins blit during frame composition. | If compositor crashes in `COMPOSITING`, restart supervisor resets buffer state to `FREE`. |
| `COMPOSITING -> FREE` | `compositord` (Consumer) | Compositor completes blit to VRAM. | Normal frame cycle completion. |

---

## 8. Application Presentation & Spatial Viewport Architecture

```text
Application Process (Stage 3J)
          │
          ▼ (Registers SHM surface with C_ws)
surfaced (Display Surface Authority)
          │
          ▼ (Maps surface to Workspace Context Graph node)
workspaced (Stage 4D Context Graph & Spatial Viewport)
          │
          ▼ (Spatial Transformation Matrix: x, y, z, scale, opacity)
compositord (Spatial Compositor)
          │
          ▼ (Blits active surfaces to physical display MMIO)
Physical Display MMIO VRAM
```

---

## 9. Fail-Closed Failure & Recovery Matrix

| Component Failure | Direct System Impact | Architectural Recovery Action |
|---|---|---|
| `compositord` Crash | Display output halts. | `init` restarts `compositord`, re-assigns display `DevCap`, re-maps VRAM MMIO via `sys_dev_map_mmio`. `surfaced` re-registers surface SHM capabilities. No kernel panic. |
| `authui` Crash | Authorization UI disappears. | `agentd` immediately transitions all pending `AuthorizationTransaction`s to **DENIED** (Fail-Closed). `uids` releases `ModalLock`. `init` restarts `authui`. |
| `uids` Crash | Input event ingestion pauses. | Input queues flushed. `init` restarts `uids`, re-binds input `DevCap`. Active focus states reset safely. |
| `surfaced` Crash | Surface spatial mapping resets. | Applications retain SHM buffers; `workspaced` re-synchronizes workspace context graph to `surfaced`. |
| Display Removal | Output target disconnected. | `compositord` detects device removal via Stage 3L notification; surface rendering redirects to remaining displays or pauses. |

---

## 10. Static Bounds & Resource Budgets

```rust
pub const MAX_PHYSICAL_DISPLAYS: usize = 4;
pub const MAX_COMPOSITOR_SURFACES: usize = 64;
pub const MAX_INPUT_DEVICES: usize = 8;
pub const MAX_PENDING_AUTH_TRANSACTIONS: usize = 4;
pub const MAX_SURFACE_RAM_MB: usize = 128; // Composition memory limit
pub const DISPLAY_DEFAULT_WIDTH: u32 = 1024;
pub const DISPLAY_DEFAULT_HEIGHT: u32 = 768;
pub const DISPLAY_BYTES_PER_PIXEL: u32 = 4; // ARGB8888
```

---

## 11. IPC Specification (`0x0701`–`0x0712`)

| Opcode | Sender | Target | Description | Payload |
|---|---|---|---|---|
| `0x0701` | `init` | `compositord` | Bind display hardware device | `{ device_id: u64, width: u32, height: u32 }` |
| `0x0702` | `compositord` | `init` | Display bind response | `{ status: i32, display_id: u64 }` |
| `0x0703` | Process | `surfaced` | Register presentation surface | `{ workspace_id: DistributedId, shm_handle: u32, width: u32, height: u32 }` |
| `0x0704` | `surfaced` | Process | Surface register response | `{ status: i32, surface_id: u64 }` |
| `0x0705` | Process | `compositord` | Commit surface frame buffer | `{ surface_id: u64, damage_rect: [u32; 4] }` |
| `0x0706` | `compositord` | Process | Frame commit response | `{ status: i32 }` |
| `0x0707` | `agentd` | `authui` | Dispatch human auth transaction | `{ transaction_id: DistributedId, req_action_hash: [u8; 32], nonce: u64 }` |
| `0x0708` | `authui` | `agentd` | Auth transaction response | `{ status: i32, transaction_id: DistributedId, user_approved: u8 }` |
| `0x0709` | `uids` | `intentd` | Ingest formatted human intent | `{ workspace_id: DistributedId, intent_len: u32, payload: [u8; 512] }` |
| `0x070A` | `intentd` | `uids` | Intent ingest response | `{ status: i32, intent_id: DistributedId }` |
| `0x070B` | `authui` | `uids` | Request ModalLock input lock | `{ transaction_token: DistributedId }` |
| `0x070C` | `uids` | `authui` | ModalLock response | `{ status: i32 }` |

---

## 12. Machine Verification Strategy (QEMU Ring3)

Stage 5 will be verified through a dedicated Python test runner (`tests/test_stage5.py`) asserting:
1. **Compilation**: `libzero`, `compositord`, `surfaced`, `authui`, and `uids` compile cleanly for `x86_64-unknown-none`.
2. **QEMU Machine Verification (26 Tests: 5-A to 5-Z)**:
   - `5-A`: `compositord` Display Device Binding & VRAM MMIO Mapping (`sys_dev_map_mmio`).
   - `5-B`: Software Framebuffer Composition & Blit Verification.
   - `5-C`: Surface Registration & Stage 3H `WorkspaceCap` Derivation.
   - `5-D`: Zero-Copy Shared Memory Presentation Buffer Lifecycle & Ownership Protocol.
   - `5-E`: Spatial Viewport Transformation & Clipping.
   - `5-F`: Layer Z-Ordering Enforcement (`SURFACE_TYPE_AUTH_OVERLAY` precedence).
   - `5-G`: Visual HMAC Badge Verification on `authui` Prompts.
   - `5-H`: Unprivileged `SURFACE_TYPE_AUTH_OVERLAY` Escalation Rejection.
   - `5-I`: `uids` Hardware Input Event Ingestion.
   - `5-J`: Input Focus Routing & `ModalLock` Capability Validation.
   - `5-K`: `I-INPUT-NO-IMPLICIT-AUTHORITY` Enforcement (`uids` cannot issue capabilities).
   - `5-L`: Intent Submission (`uids` -> `intentd` `OP_INTENT_SUBMIT`).
   - `5-M`: Priority::Critical Scheduling Assignment for `compositord`.
   - `5-N`: Input-to-Photon Frame Latency Target Compliance ($<16.6\text{ ms}$).
   - `5-O`: Surface Destruction & Capability Revocation Teardown.
   - `5-P`: `authui` Crash Fail-Closed Transaction Denial.
   - `5-Q`: Multi-Display Head Topology Setup.
   - `5-R`: `compositord` Crash & Re-Bind Recovery.
   - `5-S`: `surfaced` Crash & Viewport Reconstruction.
   - `5-T`: Protocol Robustness & Unknown Opcode Rejection.
   - `5-U`: Invalid Surface Handle Rejection.
   - `5-V`: Surface RAM Quota Enforcement (`MAX_SURFACE_RAM_MB`).
   - `5-W`: Workspace Deletion Surface Teardown.
   - `5-X`: IPC Protocol Format Verification.
   - `5-Y`: PMM Frame Neutrality (Baseline == Final frame count).
   - `5-Z`: Substrate Preservation (0 bytes modified in Stage 3A-3N and Stage 4A-4F).

---

## 13. Status Record

```text
STATUS: DRAFT — ARCHITECTURE REVIEW REQUIRED
IMPLEMENTATION: NOT AUTHORIZED

STAGE 3A–3N: FROZEN
STAGE 4A–4F: FROZEN
```
