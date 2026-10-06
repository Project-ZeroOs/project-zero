# Stage 6B Architecture Specification (Rev2)
## Distributed Spatial Presentation Protocol & Remote Surface Proxy Subsystem

**Status**: 🟡 PROPOSED — PENDING REVIEW & FREEZE APPROVAL (IMPLEMENTATION NOT AUTHORIZED)  
**Author**: ZeroOS Core Architecture Team  
**Date**: 2026-10-06  
**Foundational Substrate**: Stage 3A–3N Kernel (Frozen), Stage 4A–4F Core System Services (Frozen), Stage 5 Presentation Subsystem (Frozen), Stage 6A User Session Substrate (Frozen)

---

## 1. Executive Summary & Core Architectural Invariants

Stage 6B defines the **Distributed Spatial Presentation Protocol**, establishing a multi-service extension contract across Stage 4F `fabricd`, Stage 3M `netd`, Stage 5 `surfaced`, Stage 5 `compositord`, and Stage 6A `shelld`.

```text
========================================================================================
STAGE 6B CORE ARCHITECTURAL INVARIANTS
========================================================================================
1. INVARIANT I-CSDT-NOT-SURFACE-AUTHORITY:
   Cross-System Delegation Tokens (CSDT) prove distributed transport context and provenance.
   They DO NOT constitute kernel capabilities, workspace authorization, or surface authority.
   CSDT != Kernel Capability != Workspace Authorization != Surface Authority.

2. INVARIANT I-REMOTE-STREAM-SINGLE-SURFACE:
   One authenticated remote frame stream corresponds to EXACTLY ONE RemoteSurfaceProxyDescriptor.

3. INVARIANT I-REMOTE-GENERATION-BOUND:
   stream_generation is monotonically increasing per remote surface stream incarnation.
   A frame whose stream_generation differs from the currently authorized proxy generation
   MUST be discarded without mutating presentation state.

4. INVARIANT I-REMOTE-CRC-NOT-AUTHORITY:
   payload_crc32 is for physical transport corruption detection ONLY, NOT security/authentication.
   Frame acceptance requires arrival over the already authenticated/authorized remote stream.

5. INVARIANT I-REMOTE-MALFORMED-NO-MUTATION:
   Malformed frame dimensions, damage bounds, or payload lengths MUST trigger InvalidArgument
   and MUST NOT mutate the local presentation buffer.

6. INVARIANT I-REMOTE-EPHEMERAL-HANDLES:
   No network socket handle or process-local handle is persisted in RemoteSurfaceProxyDescriptor
   or workspace persistent state. Ephemeral handles are held in runtime tables managed by surfaced/netd.
========================================================================================
```

---

## 2. Authority Chain & Service Responsibilities

Remote presentation surfaces follow a strict, non-bypassable authority chain:

```text
Remote Node (Producer)
       │
Remote surfaced
       │
Remote frame stream
       │
     netd (Stage 3M Network Transport)
       │
    fabricd (1. CSDT Token Validation & Provenance Logging)
       │
  workspaced (2. Independent Local Workspace Membership Verification)
       │
   surfaced (3. Allocates local_shm_handle & RemoteSurfaceProxyDescriptor)
       │
compositord (4. Non-Blocking 60 FPS Software Composition)
       │
    Display (VRAM Scanout)
```

### 2.1 Daemon Responsibility Matrix

| Daemon | Layer / Stage | Specific Stage 6B Responsibility Contract |
|---|---|---|
| **`netd`** | Stage 3M (Network) | Manages raw socket connection channels (`OP_SOCKET_*`) for frame packet streaming. |
| **`fabricd`** | Stage 4F (Fabric) | Validates `CsdtToken` signature and expiration. Stores `csdt_id` for provenance/audit. |
| **`workspaced`** | Stage 4D (Workspace) | Independently verifies `CSDT.workspace_id == Local Authorized WorkspaceId`. |
| **`surfaced`** | Stage 5 (Surfaces) | Sole owner of `local_shm_handle`. Allocates and manages `RemoteSurfaceProxyDescriptor`. |
| **`compositord`** | Stage 5 (Compositor) | Performs non-blocking software composition using local SHM double-buffers. |
| **`shelld`** | Stage 6A (Session) | Computes spatial window grid layout for remote viewport display. |

---

## 3. Deterministic Wire-Level Remote Frame Protocol Contract (`RemoteFrameHeader`)

Remote frame streaming over Stage 3M `netd` socket channels uses an explicit **72-byte binary wire ABI** (`RemoteFrameHeader`).

```text
========================================================================================
WIRE ABI ENCODING CONTRACT (INVARIANT I-REMOTE-HEADER-SIZE):
- Header Size: EXACTLY 72 BYTES (REMOTE_FRAME_HEADER_SIZE = 72).
- Byte Order: Explicit Little-Endian for all multi-byte fields.
- Struct Alignment: #[repr(C, packed)] without implicit compiler padding.
========================================================================================
```

```rust
pub const REMOTE_FRAME_MAGIC: u32 = 0x5A505246; // "ZPRF"
pub const REMOTE_FRAME_HEADER_SIZE: usize = 72;

pub const FLAG_FULL_FRAME: u16  = 0x0001;
pub const FLAG_DAMAGE_RECT: u16 = 0x0002;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteFrameHeader {
    /// Magic identifier ("ZPRF" = 0x5A505246) (4 bytes).
    pub magic: u32,
    /// Protocol version (1 byte).
    pub version: u8,
    /// Pixel format: 1=ARGB8888 (1 byte).
    pub format: u8,
    /// Stream Flags: Bit 0=FULL_FRAME, Bit 1=DAMAGE_RECT, Bits 2..15 reserved (2 bytes).
    pub flags: u16,
    /// Target Local Session Context (16 bytes).
    pub target_session_id: DistributedId,
    /// Monotonic Stream Generation Incarnation (8 bytes).
    pub stream_generation: u64,
    /// Monotonic Frame Sequence Number (8 bytes).
    pub frame_sequence: u64,
    /// Viewport Width in Pixels (4 bytes).
    pub width: u32,
    /// Viewport Height in Pixels (4 bytes).
    pub height: u32,
    /// Damage Rect: [x, y, damage_width, damage_height] (16 bytes).
    pub damage_rect: [u32; 4],
    /// Length of Frame Payload following header in bytes (4 bytes).
    pub payload_len: u32,
    /// CRC32 Checksum of Payload Data (4 bytes).
    pub payload_crc32: u32,
}
```

### 3.1 Packet Validation & Damage Parsing Rules
1. **Magic & Flags Invalidation**: Header `magic != 0x5A505246` or reserved bits `(flags & !0x0003) != 0` trigger `ZeroError::InvalidArgument` and socket disconnect.
2. **Generation Bound (`I-REMOTE-GENERATION-BOUND`)**: Packets with `stream_generation != proxy.stream_generation` are **discarded** without mutating local presentation state.
3. **Out-of-Order Packets**: Packets with `frame_sequence <= last_processed_sequence` are **discarded**.
4. **CRC32 Checksum Failure (`I-REMOTE-CRC-NOT-AUTHORITY`)**: Packets failing `payload_crc32` validation are **dropped**. CRC32 is for transport corruption detection ONLY.
5. **Payload & Damage Bounds (`I-REMOTE-PAYLOAD-BOUNDS`, `I-REMOTE-DAMAGE-BOUNDS`)**:
   - `FLAG_FULL_FRAME`: Expected `payload_len == width * height * 4`.
   - `FLAG_DAMAGE_RECT`: Checked arithmetic enforces `damage.x + damage.width <= width` and `damage.y + damage.height <= height`. Expected `payload_len == damage_width * damage_height * 4`.
   - Pixel Payload Format: Row-major 4 bytes/pixel ARGB8888.
   - Malformed payloads trigger `ZeroError::InvalidArgument` and **MUST NOT mutate the local presentation buffer**.

---

## 4. `RemoteSurfaceProxyDescriptor` & Ephemeral Lifetime Specification

`RemoteSurfaceProxyDescriptor` is stored inside `surfaced` for local proxy state management:

```rust
pub const STREAM_STATE_CONNECTING: u8   = 0;
pub const STREAM_STATE_ACTIVE: u8       = 1;
pub const STREAM_STATE_STALE: u8        = 2;
pub const STREAM_STATE_DISCONNECTED: u8 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RemoteSurfaceProxyDescriptor {
    /// Local Monotonic Surface Identity (8 bytes).
    pub local_surface_id: u64,
    /// Authoritative Local Workspace Membership (16 bytes).
    pub workspace_id: DistributedId,
    /// Originating Fabric Node Identity (8 bytes).
    pub source_node_id: u64,
    /// Monotonic Stream Generation Incarnation (8 bytes).
    pub stream_generation: u64,
    /// Provenance Audit CSDT ID (Metadata Only) (16 bytes).
    pub csdt_id: DistributedId,
    /// Local Composition SHM Handle (Owned exclusively by surfaced) (4 bytes).
    pub local_shm_handle: u32,
    /// Viewport Width in Pixels (4 bytes).
    pub width: u32,
    /// Viewport Height in Pixels (4 bytes).
    pub height: u32,
    /// Z-Layer (Strictly constrained z <= 99) (1 byte).
    pub z_layer: u8,
    /// Stream State: 0=Connecting, 1=Active, 2=Stale, 3=Disconnected (1 byte).
    pub stream_state: u8,
    /// Explicit Padding (2 bytes).
    pub _pad0: [u8; 2],
    /// Receiver-Local Arrival Monotonic Timestamp (8 bytes).
    pub last_valid_frame_received_tsc: u64,
    /// Monotonic Dropped Frame Counter (8 bytes).
    pub dropped_frame_count: u64,
}
```

---

## 5. Receiver-Local Monotonic Timing & Stale State Machine

Stale frame detection ($T_{\text{stale}} = 500\text{ ms}$) is evaluated **strictly using the local qualified monotonic clock** (`read_canonical_tsc()`):

```text
               RECEIVER-LOCAL MONOTONIC TIMING STATE MACHINE
               
Packet Arrival:
  Local Reception TSC = T_rx
  Update: last_valid_frame_received_tsc = T_rx

Compositor Swap Tick:
  Current Local TSC = T_current
  Elapsed Local ms = (T_current - last_valid_frame_received_tsc) / MonotonicFrequency
  
  ┌────────────────────────────────────────────────────────────────────────┐
  │ Elapsed Local ms <= 500 ms  ──► Render Front Buffer (STREAM_ACTIVE)    │
  │ Elapsed Local ms > 500 ms   ──► Render Last Valid Frame (STREAM_STALE) │
  └────────────────────────────────────────────────────────────────────────┘
```

- **Non-Blocking Compositor Invariant**: `compositord` **never stalls** waiting for remote network packets.
- **Stale Visual Indicator**: When `Elapsed Local ms > 500 ms`, `surfaced` marks `stream_state = STREAM_STATE_STALE`. `shelld` renders a visual network disconnect badge over the viewport. Remote timestamps in incoming packets are logged as telemetry but **never used** for timing decisions.

---

## 6. Z-Layer Security & Provider Failure Invariants

### 6.1 Z-Layer Security Boundary
- Remote presentation surfaces stream from external fabric peers and are **strictly restricted to workspace application layers ($z \in [0 \dots 99]$)**.
- Remote surface registration requests specifying $z \ge 100$ or $z=255$ are **rejected** by `surfaced` with `ZeroError::PermissionDenied`.

### 6.2 Provider Failure & Subordination Contract
When a remote fabric node fails or disconnects:
1. `fabricd` detects provider loss via heartbeats and triggers Stage 4F provider-loss quarantine.
2. `fabricd` notifies `surfaced` via IPC `OP_SURFACE_UNREGISTER_REMOTE_PROXY`.
3. `surfaced` removes `RemoteSurfaceProxyDescriptor` and frees the local SHM buffer.
4. `shelld` updates its visual viewport tree.
5. **Workspace Invariant**: The local `WorkspaceId` remains `Active` in `workspaced`. Remote presentation failure **must not** mutate workspace lifecycle state.

---

## 7. IPC Protocol Specification Extensions (Range: `0x0713` – `0x0718`)

Stage 5 `presentation` IPC range (`0x0701..0x0712`) is extended for remote surface proxies:

```text
0x0713: OP_SURFACE_REGISTER_REMOTE_PROXY       0x0714: OP_SURFACE_REGISTER_REMOTE_PROXY_RESP
0x0715: OP_SURFACE_UNREGISTER_REMOTE_PROXY     0x0716: OP_SURFACE_UNREGISTER_REMOTE_PROXY_RESP
0x0717: OP_SURFACE_QUERY_REMOTE_PROXY          0x0718: OP_SURFACE_QUERY_REMOTE_PROXY_RESP
```

---

## 8. Machine-Verifiable Acceptance Gates (`6B-1` – `6B-26`)

```text
[Gate 6B-1]   RemoteFrameHeader 72-Byte Exact ABI Size Validation (REMOTE_FRAME_HEADER_SIZE)
[Gate 6B-2]   Little-Endian Wire Field Serialization & Deserialization
[Gate 6B-3]   CSDT Provenance Logging (csdt_id metadata non-authority check)
[Gate 6B-4]   Local Workspace Membership Independent Verification (workspaced validation)
[Gate 6B-5]   surfaced Local SHM Handle Exclusive Allocation (local_shm_handle)
[Gate 6B-6]   Remote Stream Single-Surface Incarnation Binding (I-REMOTE-STREAM-SINGLE-SURFACE)
[Gate 6B-7]   Stream Generation Validation & Stale Incarnation Discard (I-REMOTE-GENERATION-BOUND)
[Gate 6B-8]   Remote Surface Z-Layer Restriction (z <= 99 enforced)
[Gate 6B-9]   Remote System Layer Escalation Rejection (z>=100 -> PermissionDenied)
[Gate 6B-10]  Exclusive Auth Overlay Lock (z=255 rejected for remote surfaces)
[Gate 6B-11]  RemoteFrameHeader CRC32 Checksum Validation (Corruption detection only)
[Gate 6B-12]  Out-of-Order Frame Sequence Discard (frame_sequence <= last_seq)
[Gate 6B-13]  Damage Rect Bounds & Payload Length Verification (I-REMOTE-PAYLOAD-BOUNDS)
[Gate 6B-14]  Malformed Frame Discard Without Presentation Buffer Mutation
[Gate 6B-15]  Receiver-Local Monotonic Clock Stale Frame Detection (T_stale = 500 ms)
[Gate 6B-16]  Non-Blocking Compositor Scanout Loop Preservation (60 FPS maintained)
[Gate 6B-17]  Stale Frame Network Disconnect Badge Rendering in shelld
[Gate 6B-18]  Remote Surface Proxy Registration Dispatch (OP_SURFACE_REGISTER_REMOTE_PROXY)
[Gate 6B-19]  Remote Surface Proxy Unregistration Dispatch (OP_SURFACE_UNREGISTER_REMOTE_PROXY)
[Gate 6B-20]  Stage 4F Provider-Loss Remote Proxy Teardown
[Gate 6B-21]  Workspace State Preservation during Remote Proxy Disconnect
[Gate 6B-22]  Local Presentation SHM Zero-Copy Scanout Verification
[Gate 6B-23]  Protocol Robustness & Invalid Opcode Handling (0x0713..0x0718)
[Gate 6B-24]  Stage 4B Composition RAM Lease Compliance (MAX_SURFACE_RAM_MB = 128)
[Gate 6B-25]  PMM Frame Neutrality (0 frame leaks across remote stream lifecycle)
[Gate 6B-26]  Stage 3A–3N Kernel Nucleus Byte-Identical Preservation (0 bytes modified)
```

---

## 9. Document Status & Authorization Request

```text
Stage 3A–3N Kernel Substrate     🟢 FROZEN & VERIFIED (Byte-Identical)
Stage 4A–4F Core Services         🟢 FROZEN & VERIFIED
Stage 5 User Interaction          🟢 FROZEN & VERIFIED
Stage 6A User Session Substrate  🟢 FROZEN & VERIFIED
Stage 6B Architecture Spec       🟡 PROPOSED FOR REVIEW (Rev2 - STAGE6B-ARCHITECTURE-REV1.md)

Implementation                    🛑 NOT AUTHORIZED
```

**Submitted for review and freeze approval.** Implementation remains blocked until explicit authorization.
