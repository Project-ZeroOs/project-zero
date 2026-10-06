# Stage 5 Architecture — User Interaction Substrate & Spatial Presentation Subsystem

**Revision:** Rev2  
**Status:** DRAFT — ARCHITECTURE REVIEW REQUIRED  
**Implementation:** NOT AUTHORIZED  
**Authoritative Substrate:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), ADR-0001 through ADR-0030 (Frozen)

---

## 1. Purpose & Core Architectural Identity

> **Phase 5 is the capability-secured human interaction and presentation substrate that connects ZeroOS's intent and workload subsystems to physical human input and visual presentation.**

Stage 5 completes the ZeroOS architectural chain by providing a capability-bounded user interaction substrate. It establishes a trusted human interaction path, zero-copy presentation buffer management, anti-spoofing authorization UI rendering, and spatial workspace context presentation without modifying the frozen Stage 3 kernel nucleus or Stage 4 daemon substrate.

---

## 2. Current ZeroOS Substrate & Foundation Mapping

Phase 5 explicitly consumes existing frozen primitives across Stage 3 and Stage 4:

```text
Stage 3L Hardware Drivers ──► DeviceId::Display(0), DeviceId::Input(0)
Stage 3H Capability Engine ──► sys_cap_derive, sys_cap_revoke, C-Lists, Rights Attenuation
Stage 3G IPC & Shared Mem ──► ShmObject (SHM_MAP_READ, SHM_MAP_WRITE, W^X NX), IPC Channels
Stage 3B Scheduler Core   ──► Priority::Critical (Compositor), Priority::Normal (Services)
Stage 4D Workspace Engine ──► Context Graph, Spatial Viewport Metadata
Stage 4E Agent Runtime    ──► OP_AGENT_REQUEST_HUMAN_AUTH, Agent Security Containment
Stage 4F Intent Resolver  ──► OP_INTENT_SUBMIT, IntentDescriptor Execution Plans
```

**Kernel Primitives Requirement:** Exactly **ZERO** new kernel primitives are required. Stage 5 operates entirely in Ring 3 userspace using Stage 3G `ShmObject` memory, Stage 3H capability derivation, and Stage 3L device capabilities.

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
          ▼ (sys_shm_map of MMIO Framebuffer via Stage 3L DevCap)
Hardware Framebuffer Scanout
```

### 3.1 Display Authority & Mapping Contract
1. **Device Identification**: Display hardware is identified by Stage 3L `DeviceId::Display(0)`.
2. **Capability Issuance**: `init` holds the root `DevCap` for display hardware and delegates an attenuated `DevCap` (display write/scanout rights) to `compositord` upon startup (`OP_INIT_BIND_DISPLAY`).
3. **Memory Mapping**: `compositord` maps the MMIO framebuffer aperture using `sys_shm_map` with its display `DevCap`. Unprivileged processes hold no `DevCap` and cannot map display MMIO.
4. **Scheduler Integration**: `compositord` executes its composition loop in the kernel's existing **Priority::Critical** scheduling class (`Stage 3B`), guaranteeing execution latency budgets ($<16\text{ ms}$) without creating a new kernel scheduler class.

---

## 4. Capability Model: Elimination of `SurfaceCap` as Parallel Authority

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
  ▼ (Routed strictly to authui via brokerd)
authui (Trusted Authorization Overlay)
  │
  ├──► Validates Session HMAC Visual Badge (Hardware/Boot Secret)
  ├──► Requests Modal Input Lock from uids / compositord
  └──► Renders Transaction Details & User Approval Surface
```

### 5.2 Authorization Security Invariants
1. **Transaction Binding**: Approval decisions (`UserApproved` / `UserDenied`) are cryptographically bound to `transaction_id` and `requested_action_hash`. An approval token cannot be reused for a different operation or workload.
2. **Visual HMAC Badge**: Every authorization dialog rendered by `authui` includes a visual HMAC token derived from a boot-time session secret ($K_{session}$). Peer surfaces cannot generate or predict this visual badge, rendering UI spoofing visually detectable.
3. **Modal Input Lock**: While an authorization prompt is active, `compositord` and `uids` lock input focus strictly to `authui`. All peer surfaces receive `FocusLost` and their input channels are muted.
4. **Crash Fail-Closed**: If `authui` crashes or loses connection during an active prompt, `agentd` immediately transitions the pending `AuthorizationTransaction` to **DENIED** (Fail-Closed).

---

## 6. Input Ingestion & Routing Model (`I-INPUT-NO-IMPLICIT-AUTHORITY`)

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

### 6.1 Invariant `I-INPUT-NO-IMPLICIT-AUTHORITY`
`uids` is an input routing and intent formatting service. It possesses:
- **ZERO Capability Authority**: Cannot issue, modify, or delegate Stage 3H capabilities.
- **ZERO Workload Execution Authority**: Cannot create workloads or spawn processes directly.
- **ZERO Authorization Approval Authority**: Cannot approve human authorization requests on behalf of the user.
- **Strict Intent Validation**: All human intents formatted by `uids` must pass full deterministic validation in `intentd` (Stage 4F) and agent containment in `agentd` (Stage 4E).

---

## 7. Zero-Copy Presentation Buffer Contract

Presentation surfaces utilize Stage 3G `ShmObject` buffers governed by a strict zero-copy synchronization contract:

```rust
#[repr(C)]
pub struct PresentationBufferHeader {
    pub magic: u32,               // 0x5A505549 ("ZPUI")
    pub width: u32,               // Pixel width (e.g., 1024)
    pub height: u32,              // Pixel height (e.g., 768)
    pub stride_bytes: u32,        // Row stride in bytes (e.g., 4096)
    pub format: u32,              // Pixel format (1 = ARGB8888)
    pub active_front_buffer: u8,  // 0 or 1 (Double-buffering)
    pub buffer_state: [u8; 2],    // STATE_FREE (0), STATE_WRITING (1), STATE_READY (2), STATE_COMPOSITING (3)
    pub damage_rect: [u32; 4],    // [x, y, width, height]
    pub sequence_num: u64,        // Monotonic frame counter
}
```

### 7.1 Synchronization Protocol
1. **Producer (Application)**:
   - Selects back buffer in `STATE_FREE`.
   - Sets state to `STATE_WRITING`, renders frame, updates `damage_rect`.
   - Atomically sets state to `STATE_READY` and notifies `compositord` via IPC (`OP_SURFACE_COMMIT`).
2. **Consumer (`compositord`)**:
   - Inspects `STATE_READY` back buffer.
   - Atomically transitions buffer to `STATE_COMPOSITING` during composition blit.
   - Transitions buffer to `STATE_FREE` upon completion, permitting application reuse.
3. **Scanout Safety**: `compositord` never reads from buffers in `STATE_WRITING` or `STATE_FREE`, guaranteeing zero tearing and zero race conditions.

---

## 8. Application Presentation & Spatial Viewport Architecture

```text
Application Process (Stage 3J)
          │
          ▼ (Requests presentation surface)
surfaced (Display Surface Authority)
          │
          ▼ (Maps surface to Workspace Context Graph node)
workspaced (Stage 4D Context Graph & Spatial Viewport)
          │
          ▼ (Spatial Transformation Matrix: x, y, z, scale, opacity)
compositord (Spatial Compositor)
          │
          ▼ (Blits active surfaces to physical display)
Physical Display Output
```

### 8.1 Viewport Separation
- **`workspaced` (Stage 4D)**: Holds abstract context graph nodes and spatial relationships (topological node adjacency, workspace bounds).
- **`surfaced` (Stage 5)**: Maintains application presentation surfaces and attaches them to `workspaced` context nodes.
- **`compositord` (Stage 5)**: Performs frame composition by sampling presentation surfaces according to their spatial viewport matrices.

---

## 9. Bidirectional Input & Focus Control

### 9.1 Focus State Machine
Input focus is maintained by `uids` across four mutually exclusive states:

```text
Unfocused ──► Focused ──► Captured ──► ModalLock (authui Active)
```

- **Unfocused**: Surface receives no input events.
- **Focused**: Surface receives pointer and keyboard events directed to its bounding region.
- **Captured**: Surface captures pointer events outside its bounding region (e.g., drag operations).
- **ModalLock**: Activated exclusively when `authui` displays an active `AuthorizationTransaction`. All other surfaces transition to `Unfocused`; 100% of input is routed to `authui`.

---

## 10. Fail-Closed Failure & Recovery Matrix

| Component Failure | Direct System Impact | Architectural Recovery Action |
|---|---|---|
| `compositord` Crash | Display output halts momentarily. | `init` restarts `compositord`, re-assigns display `DevCap`, re-maps VRAM MMIO. `surfaced` re-registers surface SHM capabilities. No kernel panic. |
| `authui` Crash | Authorization UI disappears. | `agentd` immediately transitions all pending `AuthorizationTransaction`s to **DENIED** (Fail-Closed). `init` restarts `authui`. |
| `uids` Crash | Input event ingestion pauses. | Input queues flushed. `init` restarts `uids`, re-binds input `DevCap`. Active focus states reset safely. |
| `surfaced` Crash | Surface spatial mapping resets. | Applications retain SHM buffers; `workspaced` re-synchronizes workspace context graph to `surfaced`. |
| Display Removal | Output target disconnected. | `compositord` detects device removal via Stage 3L notification; surface rendering redirects to remaining displays or pauses. |

---

## 11. Static Bounds & Resource Budgets

All Stage 5 memory structures are statically allocated to guarantee zero dynamic heap exhaustion during operation:

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

## 12. IPC Specification (`0x0701`–`0x0712`)

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

---

## 13. Machine Verification Strategy (QEMU Ring3)

Stage 5 will be verified through a dedicated Python test runner (`tests/test_stage5.py`) asserting:
1. **Compilation**: `libzero`, `compositord`, `surfaced`, `authui`, and `uids` compile cleanly for `x86_64-unknown-none`.
2. **QEMU Machine Verification (26 Tests: 5-A to 5-Z)**:
   - `5-A`: `compositord` Display Device Binding & VRAM Mapping.
   - `5-B`: Software Framebuffer Composition & Blit Verification.
   - `5-C`: Surface Registration & Stage 3H `WorkspaceCap` Derivation.
   - `5-D`: Zero-Copy Shared Memory Presentation Buffer Swap (`ShmObject`).
   - `5-E`: Spatial Viewport Transformation & Clipping.
   - `5-F`: Layer Z-Ordering Enforcement (`ModalLock` precedence).
   - `5-G`: Visual HMAC Badge Verification on `authui` Prompts.
   - `5-H`: Unprivileged Layer Escalation Rejection (`authui` anti-spoofing).
   - `5-I`: `uids` Hardware Input Event Ingestion.
   - `5-J`: Input Focus Routing & Isolation.
   - `5-K`: `I-INPUT-NO-IMPLICIT-AUTHORITY` Enforcement (`uids` cannot issue capabilities).
   - `5-L`: Intent Submission (`uids` -> `intentd` `OP_INTENT_SUBMIT`).
   - `5-M`: Real-Time Scheduling Class Assignment for `compositord`.
   - `5-N`: Input-to-Photon Frame Latency Budget Compliance ($<16\text{ ms}$).
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

## 14. Status Record

```text
STATUS: DRAFT — ARCHITECTURE REVIEW REQUIRED
IMPLEMENTATION: NOT AUTHORIZED

STAGE 3A–3N: FROZEN
STAGE 4A–4F: FROZEN
```
