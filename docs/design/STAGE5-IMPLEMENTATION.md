# Phase 5 Implementation Plan — User Interaction Substrate & Spatial Presentation Subsystem

**Revision:** Rev2  
**Status:** DRAFT — IMPLEMENTATION PLAN FOR REVIEW  
**Authoritative Specifications:** Stage 5 Architecture Rev4 (Frozen), ADR-0031 (Adopted)  
**Preservation Boundary:** Stage 3A–3N Kernel Nucleus (Byte-Identical), Stage 4A–4F Architecture Preserved (Authorized `init` Service Registration Allowed)

---

## 1. Executive Summary & Strategy

Phase 5 implements the **User Interaction Substrate & Spatial Presentation Subsystem** for ZeroOS. It completes the bidirectional communication loop between physical human users and ZeroOS:
- **Input Direction**: Physical Input Drivers (`Stage 3L`) $\to$ `uids` $\to$ `intentd` (`Stage 4F`) $\to$ `agentd` (`Stage 4E`) $\to$ `workspaced` (`Stage 4D`) $\to$ `workloadd` (`Stage 4C`).
- **Presentation Direction**: Workload / Workspace Context $\to$ `surfaced` $\to$ `compositord` $\to$ Physical Display MMIO VRAM (`Stage 3L`).
- **Trusted Overlay**: `agentd` $\to$ `AuthorizationTransactionRef` $\to$ `authui` (ModalLock + Visual HMAC Badge) $\to$ `compositord` top-most composition layer ($255$).

Implementation will proceed sequentially across freestanding `no_std` user-space packages (`libzero`, `compositord`, `surfaced`, `authui`, `uids`), `init` supervisor integration (`init/src/main.rs`), Python test runners (`tests/test_stage5.py`), and authorized Stage 4 verification harness extensions (`kernel/src/stage4/tests.rs`).

---

## 2. Substrate Preservation & Supervisor Integration Boundary

```text
Stage 3A–3N Production Kernel Nucleus   ──► 100% Byte-Identical (0 bytes modified)
Stage 4A–4F Architecture               ──► Preserved & Fully Compatible
Stage 4A Supervisor Integration        ──► init/src/main.rs extended to register and
                                            supervise compositord, surfaced, authui, uids
Stage 5 Production Packages             ──► Created in libzero/src/presentation.rs,
                                            compositord/, surfaced/, authui/, uids/
Stage 5 Verification Harness            ──► Extended in kernel/src/stage4/tests.rs,
                                            kernel/src/stage4/mod.rs, kernel/src/lib.rs
```

---

## 3. Stage 3H Capability & Stage 4B Resource Authority Integration

### 3.1 Service Role Capabilities (`SurfacePolicyCap` and `InputFocusPolicyCap`)
`surfaced` and `uids` policy authorities are implemented as **standard Stage 3H capabilities** (`CapabilityNode`), NOT new capability types:

```text
Root Service Capability (held by init)
        │
        ├── sys_cap_derive ──► SurfacePolicyCap (ObjectType::ServiceRole, RIGHT_DELEGATE | RIGHT_CALL)
        │                       └── Delegated exclusively to surfaced at startup
        │
        └── sys_cap_derive ──► InputFocusPolicyCap (ObjectType::ServiceRole, RIGHT_DELEGATE | RIGHT_CALL)
                                └── Delegated exclusively to uids at startup
```

- **`SurfacePolicyCap`**: Grants `surfaced` the singleton service-role authority to register presentation surfaces with `workspaced` and validate workspace surface derivations ($C_{ws} \to C_{surf}$).
- **`InputFocusPolicyCap`**: Grants `uids` the singleton service-role authority to enforce input focus routing over `DeviceId::Input(0)` and process `ModalLock` requests.
- **Teardown & Revocation**: Revoking either service capability via `sys_cap_revoke` immediately halts surface registration or input focus routing.

### 3.2 Stage 4B Resource Authority Integration (`resourced`)
Rather than creating an independent Stage 5 memory allocator, `surfaced` integrates directly with **Stage 4B `resourced`**:
- Composition memory budget `MAX_SURFACE_RAM_MB` ($128\text{ MB}$) is acquired from `resourced` via Stage 4B `OP_LEASE_ACQUIRE` upon startup.
- If physical memory budget is exhausted, `resourced` rejects lease expansion, and `surfaced` rejects surface registration with `ZeroError::QuotaExceeded`.

---

## 4. Data Structures & Shared IPC Specifications (`0x0701`–`0x0712`)

### 4.1 Constants & Bounds (`libzero/src/presentation.rs`)

```rust
pub const OP_COMPOSITOR_BIND_DISPLAY: u32 = 0x0701;
pub const OP_COMPOSITOR_BIND_DISPLAY_RESP: u32 = 0x0702;
pub const OP_SURFACE_REGISTER: u32 = 0x0703;
pub const OP_SURFACE_REGISTER_RESP: u32 = 0x0704;
pub const OP_SURFACE_COMMIT: u32 = 0x0705;
pub const OP_SURFACE_COMMIT_RESP: u32 = 0x0706;
pub const OP_AUTHUI_DISPATCH_TRANSACTION: u32 = 0x0707;
pub const OP_AUTHUI_DISPATCH_TRANSACTION_RESP: u32 = 0x0708;
pub const OP_UIDS_INGEST_INTENT: u32 = 0x0709;
pub const OP_UIDS_INGEST_INTENT_RESP: u32 = 0x070A;
pub const OP_UIDS_REQUEST_MODAL_LOCK: u32 = 0x070B;
pub const OP_UIDS_REQUEST_MODAL_LOCK_RESP: u32 = 0x070C;

// Derived from Stage 3L device table bounds (MAX_DEVICES = 16)
pub const MAX_PHYSICAL_DISPLAYS: usize = 4;
pub const MAX_COMPOSITOR_SURFACES: usize = 64;
pub const MAX_INPUT_DEVICES: usize = 8;
pub const MAX_PENDING_AUTH_TRANSACTIONS: usize = 4;
pub const MAX_SURFACE_RAM_MB: usize = 128; // Capped via Stage 4B resourced lease

pub const DISPLAY_DEFAULT_WIDTH: u32 = 1024;
pub const DISPLAY_DEFAULT_HEIGHT: u32 = 768;
pub const DISPLAY_BYTES_PER_PIXEL: u32 = 4; // ARGB8888
pub const LAYER_WORKSPACE_DEFAULT: u8 = 10;
pub const LAYER_SYSTEM_AUTH: u8 = 255;
```

### 4.2 Presentation Buffer Header (`PresentationBufferHeader`)

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

---

## 5. Sequential Implementation Phases

### Phase 5.1: `libzero` Extension (`libzero/src/presentation.rs`)
- Implement IPC opcodes (`0x0701`–`0x0712`), static structures (`PresentationBufferHeader`, `PresentationSurfaceDescriptor`, `InputEventDescriptor`, `AuthorizationTransactionRef`), and serialization helpers.
- Export `presentation` in `libzero/src/lib.rs`.
- Verification: `cargo check` in `libzero`.

### Phase 5.2: Compositor Daemon (`compositord/`)
- Create `compositord` package under `compositord/Cargo.toml` and `compositord/src/main.rs`.
- Implements Stage 3L `sys_dev_map_mmio` display VRAM mapping (`[0xE0000000, 0xE0300000)`), dirty-rect blitting, Priority::Critical execution, and zero-copy SHM sampling.
- Verification: `cargo check -p compositord`.

### Phase 5.3: Display Surface Authority (`surfaced/`)
- Create `surfaced` package under `surfaced/Cargo.toml` and `surfaced/src/main.rs`.
- Implements surface registration ($C_{ws} \to C_{surf}$ derivation validation), Stage 4B `resourced` memory lease acquisition, workspace spatial context mapping, and ephemeral surface reconstruction from `workspaced`.
- Verification: `cargo check -p surfaced`.

### Phase 5.4: Trusted Authorization Overlay (`authui/`)
- Create `authui` package under `authui/Cargo.toml` and `authui/src/main.rs`.
- Implements `AuthorizationTransactionRef` rendering, visual HMAC badge calculation ($K_{session}$), and `ModalLock` interaction requests.
- Verification: `cargo check -p authui`.

### Phase 5.5: User Interaction Daemon (`uids/`)
- Create `uids` package under `uids/Cargo.toml` and `uids/src/main.rs`.
- Implements input event ingestion from `DeviceId::Input(0)`, focus routing (`Unfocused -> Focused -> Captured -> ModalLock`), and `I-INPUT-NO-IMPLICIT-AUTHORITY` intent formatting for `intentd`.
- Verification: `cargo check -p uids`.

### Phase 5.6: `init` Supervisor Integration & Kernel Verification Harness
- Update `init/src/main.rs` to register and supervise Stage 5 daemons (`compositord`, `surfaced`, `authui`, `uids`).
- Add `run_stage5_verification` and static helper daemons (`compositord_helper`, `surfaced_helper`, `authui_helper`, `uids_helper`) in `kernel/src/stage4/tests.rs`.
- Re-export `run_stage5_verification` in `kernel/src/stage4/mod.rs`.
- Call `stage4::tests::run_stage5_verification(...)` in `kernel/src/lib.rs` right before QEMU exit.
- Verification: `cargo check --target x86_64-unknown-none` in `kernel/`.

### Phase 5.7: Python Test Suite (`tests/test_stage5.py`)
- Create `tests/test_stage5.py` modeling all 26 Stage 5 QEMU tests (`5-A` to `5-Z`).
- Execute QEMU test runner and assert exit status `33` (`0x21`).
- Execute full repository regression (`python -m unittest discover tests`).

---

## 6. 26 Test Acceptance Matrix (`5-A` to `5-Z`)

| Marker | Test Scenario | Mandatory Assertion | Failure Criteria |
|---|---|---|---|
| `5-A` | `compositord` MMIO Binding | `sys_dev_map_mmio` succeeds for `DeviceId::Display(0)` | `DevCap` rejection or MMIO fault |
| `5-B` | Software Framebuffer Composition | Dirty rect ARGB8888 blitted to VRAM | Pixel corruption or out-of-bounds blit |
| `5-C` | Surface Registration & Cap Derivation | $C_{ws} \to C_{surf}$ derived via `sys_cap_derive` | Forged handle allowed or invalid derivation |
| `5-D` | Zero-Copy Presentation Buffer Lifecycle | Buffer state transitions `FREE -> WRITING -> READY -> COMPOSITING -> FREE` | State race or permanent deadlock |
| `5-E` | Spatial Viewport Transformation | Viewport $(x, y, z, \text{scale})$ mapped to display bounds | Clipping error or negative scale crash |
| `5-F` | Trusted Overlay Policy Binding | `SURFACE_TYPE_AUTH_OVERLAY` permitted only with valid `AuthorizationTransactionRef` | Peer surface obscuring overlay |
| `5-G` | Visual HMAC Badge Verification | `authui` imprints HMAC computed with $K_{session}$ | Missing or static visual badge |
| `5-H` | Missing `AuthorizationTransactionRef` Rejection | Request lacking valid `AuthorizationTransactionRef` rejected from layer $255$ | Unprivileged surface on layer $255$ |
| `5-I` | `uids` Input Driver Event Ingestion | Key/pointer events ingested from `DeviceId::Input(0)` | Dropped events or driver fault |
| `5-J` | Input Focus Routing & Isolation | Focused surface receives events; unfocused receives 0 | Keylogging across unfocused surfaces |
| `5-K` | `I-INPUT-NO-IMPLICIT-AUTHORITY` | `uids` cannot issue capabilities or create workloads | Capability creation by `uids` |
| `5-L` | Human Intent Ingestion Pipeline | `uids` formats input payload $\to$ `intentd` `OP_INTENT_SUBMIT` | Bypassing `intentd` validation |
| `5-M` | Priority::Critical Compositor Scheduling | `compositord` thread scheduled ahead of Normal priority | Scheduler inversion |
| `5-N` | Soft Frame Target & Deadline Safety | Compositor detects deadline miss; retains previous frame without VRAM corruption | VRAM corruption or sampling `WRITING`/`FREE` |
| `5-O` | Surface Destruction & Cap Revocation | `sys_cap_revoke` on $C_{ws}$ unmaps surface SHM | Leaked SHM mapping |
| `5-P` | `authui` Crash Fail-Closed Security | `authui` crash transitions active auth tx to `DENIED` | Auth tx left in pending/approved state |
| `5-Q` | Multi-Head Display Topology Setup | Display descriptors allocated up to Stage 3L `MAX_PHYSICAL_DISPLAYS` bound | Buffer overflow on display table |
| `5-R` | `compositord` Crash & Re-Bind Recovery | `init` restarts `compositord`; re-binds display `DevCap` | Permanent black screen / panic |
| `5-S` | `surfaced` Crash & Viewport Reconstruction | Ephemeral handles discarded; `surfaced` reconstructs surfaces from authoritative `workspaced` graph | Handles persisted in ZeroFS |
| `5-T` | Protocol Robustness & Unknown Opcode | Unknown opcodes return `ZeroError::InvalidRequest` | Daemon panic on bad IPC tag |
| `5-U` | Invalid Surface Handle Rejection | Operations on closed/invalid surface return `NotFound` | Null pointer dereference |
| `5-V` | Stage 4B Resource Lease Integration | `surfaced` acquires composition RAM lease from `resourced` (`MAX_SURFACE_RAM_MB`) | Direct un-accounted RAM allocation |
| `5-W` | Workspace Deletion Surface Teardown | Workspace deletion revokes all associated surface buffers | Leaked surface memory |
| `5-X` | IPC Message Serialization Integrity | Payload fields verified bit-identical | Serialization mismatch |
| `5-Y` | PMM Memory Neutrality Gate | Baseline free frames == Final free frames | Leaked physical frames |
| `5-Z` | Production Substrate Preservation Gate | 0 bytes modified in Stage 3A–3N production kernel nucleus | Modification of Stage 3 kernel nucleus |

---

## 7. Verification & Completion Criteria

1. **Compilation**: `cargo check --target x86_64-unknown-none` passes cleanly for `libzero`, `compositord`, `surfaced`, `authui`, `uids`, and `kernel`.
2. **QEMU Machine Verification**: `python -m unittest tests/test_stage5.py` executes with exit code `33` (`0x21`).
3. **Full System Regression**: `python -m unittest discover tests` passes 100% across all tests.
4. **Substrate Neutrality**: 0 bytes modified in Stage 3A–3N production kernel nucleus; Stage 4A–4F architecture preserved; baseline PMM frames == final PMM frames.
