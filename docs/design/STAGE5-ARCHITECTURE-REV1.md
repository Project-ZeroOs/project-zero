# Stage 5 Architecture — User Interaction Substrate & Spatial Presentation Subsystem

**Revision:** Rev1  
**Status:** DRAFT — ARCHITECTURE REVIEW REQUIRED  
**Implementation:** NOT AUTHORIZED  
**Authoritative Substrate:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), ADR-0001 through ADR-0030 (Frozen)

---

## 1. Purpose & Core Philosophy

Stage 5 establishes the **User Interaction Substrate & Spatial Presentation Subsystem** for ZeroOS.

Following the completion of Stage 4 (System Services, Resource Accounting, Workload Orchestration, Workspace Context, Agent Supervision, and Intent Resolution/Compute Fabric), ZeroOS possesses a complete, capability-secured backend runtime. It can transform human intent into deterministic execution DAGs across local and remote compute nodes.

However, ZeroOS currently lacks a **trusted user-facing system boundary**. Without Stage 5:
- Human inputs cannot be securely ingested and routed to `intentd` without risking keylogging or input interception.
- Human authorization requests (`OP_AGENT_REQUEST_HUMAN_AUTH` from Stage 4E) have no trusted, un-spoofable visual surface for rendering.
- Active workspaces (`workspaced`, Stage 4D) and Intent execution plans (`intentd`, Stage 4F) cannot be presented spatially to the user.
- Applications and workspace tools cannot acquire capability-bounded display surfaces.

Stage 5 completes the ZeroOS vision defined in `docs/vision.md`:
> **"One coherent spatial environment where tools and workspaces project onto physical displays, governed by extreme responsiveness, strict capability boundaries, and immutable human authorization gates."**

---

## 2. Phase 5 Boundary & Non-Goals

### 2.1 What Stage 5 IS
Stage 5 is a freestanding, capability-bounded user-space presentation and interaction subsystem consisting of:
1. **`compositord`**: A zero-copy spatial compositor daemon that manages display hardware, framebuffers, input event demuxing, and zero-copy shared memory (`ShmObject`) surface composition.
2. **`surfaced`**: A display surface authority daemon that issues capability tokens (`SurfaceCap`) for application rendering windows and maps spatial context nodes from Stage 4D to visual presentation topologies.
3. **`authui`**: An isolated, trusted visual overlay daemon dedicated exclusively to rendering authenticated human authorization prompts (`OP_AGENT_REQUEST_HUMAN_AUTH`) with anti-spoofing badges.
4. **`uids`**: The User Interaction Daemon that translates physical human input (keyboard, pointer) into structured intent submissions for `intentd` (Stage 4F) and spatial navigation triggers.

### 2.2 What Stage 5 IS NOT (Explicit Non-Goals)
- **NOT an X11 or Wayland display server clone**: Stage 5 does not implement POSIX socket protocols, client-side window decorations, or legacy window management semantics.
- **NOT an ambient authority UI framework**: Surfaces cannot read input, inspect pixel buffers of peer surfaces, or capture display output without explicit Stage 3H capabilities.
- **NOT a 3D GPU graphics driver**: Stage 5 operates on 2D shared-memory framebuffers (`ShmObject`) and software-rasterized spatial geometry. Hardware 3D acceleration is deferred to future driver stages.
- **NOT a second capability authority or scheduler**: Stage 5 consumes Stage 3H capabilities and Stage 3B/4C scheduling/workload execution exclusively.

---

## 3. Justification: Why Phase 5 is the Correct Architectural Boundary

Evaluating candidate boundaries against the repository state and roadmap (`docs/vision.md`, `docs/roadmap.md` Stage 5):

| Candidate Boundary | Evaluated Architectural Fit | Reason for Selection / Deferral |
|---|---|---|
| **User Interaction & Spatial Presentation Subsystem (Stage 5)** | **CRITICAL NEXT BOUNDARY** | Closes the loop between Human and System (`Human -> uids -> intentd -> agentd`). Solves input security, visual anti-spoofing, and spatial workspace presentation without breaking frozen substrate. |
| Software Packaging & Distribution | Defer | Dependencies (Stage 5 surface + Stage 4F fabric) exist, but software deployment requires a user surface to present install prompts. |
| Advanced GPU Hardware Driver Stack | Defer | Belongs in Stage 3L device driver layer; software framebuffers over Stage 3G SHM suffice for deterministic system presentation. |
| Autonomous Workflow Engine | Defer | Bypasses Stage 4E Agent supervision; Stage 4E/4F already handle intent DAGs and triggers. |

---

## 4. Current ZeroOS Capability Map (Post-Stage 4F)

Before Stage 5, the authoritative ZeroOS execution chain is:

```text
Human
  │ (Implicit / Non-architectural console)
  ▼
intentd (Stage 4F)  ───> Intent Resolution & Execution Plan DAG
  │
  ▼
agentd (Stage 4E)   ───> Ephemeral Agent & Security Sandbox
  │
  ▼
workspaced (Stage 4D) ───> Workspace Context Containment
  │
  ▼
workloadd (Stage 4C)  ───> Workload Task Execution DAG
  │
  ▼
resourced (Stage 4B)  ───> Physical Resource Lease Allocation
  │
  ▼
fabricd (Stage 4F)   ───> Compute Placement Engine (Local / Remote)
  │
  ▼
kernel (Stage 3A-3N) ───> Threads, Processes, Capabilities, Syscalls, ZeroFS
```

---

## 5. Missing Capabilities & System Gaps

Despite the complete Stage 4 runtime, five critical system gaps exist:

1. **Gap 1: Insecure Human Input Ingestion**: No capability-secured daemon ingests raw driver interrupts (`Stage 3L`) and converts them into structured `IntentDescriptor` submissions for `intentd`.
2. **Gap 2: Human Authorization Vulnerability**: `agentd` (Stage 4E) generates `OP_AGENT_REQUEST_HUMAN_AUTH` messages, but there is no trusted, hardware-backed visual overlay to render authorization requests without potential interception or spoofing by unprivileged processes.
3. **Gap 3: Spatial Context Visibility**: `workspaced` (Stage 4D) maintains a persistent context graph, but there is no surface engine to render spatial relationships, active context nodes, or agent states.
4. **Gap 4: Zero-Copy Presentation**: Applications running in Stage 3J processes have no mechanism to share framebuffers with display hardware without unsafe shared memory access or kernel bypass.
5. **Gap 5: Latency-Deterministic Composition**: Rendering and composition must execute within a real-time scheduling class (as mandated by `docs/vision.md` Section 3.3) isolated from compute workloads.

---

## 6. Subsystem Architecture

Stage 5 introduces four cooperative system services operating in Ring 3 userspace:

```text
                               +----------------------------------+
                               |           HUMAN USER             |
                               +----------------+-----------------+
                                                |
                       +------------------------+------------------------+
                       | Physical Displays      | Physical Input Devices |
                       +-----------+------------+------------+-----------+
                                   |                         |
                                   v                         v
                       +-----------+------------+------------+-----------+
                       |    COMPOSITORD (0x07)  |     UIDS (0x07)        |
                       |  (Spatial Compositor)  | (Input/Intent Ingest)  |
                       +-----------+------------+------------+-----------+
                                   |                         |
               +-------------------+                         |
               |                                             v
               v                                   +---------+----------+
+--------------+-------------+                     |   INTENTD (0x06)   |
|     SURFACED (0x07)        |                     | (Intent Resolver)  |
| (Surface Authority & Graph)|                     +---------+----------+
+--------------+-------------+                               |
               |                                             v
               v                                   +---------+----------+
+--------------+-------------+                     |   AGENTD (0x05)    |
|      AUTHUI (0x07)         | <─── Human Auth ────|  (Agent Runtime)   |
| (Trusted Anti-Spoof UI)    |    (0x0507 Req)     +--------------------+
+----------------------------+
```

### 6.1 `compositord` (Spatial Compositor Daemon)
- **Role**: Manages physical display outputs, framebuffer devices (`Stage 3L`), and zero-copy surface blending via Stage 3G `ShmObject` channels.
- **Scheduling**: Executes in the Real-Time Priority Class (`Stage 3B`) to guarantee input-to-photon latency limits ($<16\text{ ms}$).
- **Security**: Holds exclusive `DevCap` for display output hardware. Does not expose raw display capabilities to user processes.

### 6.2 `surfaced` (Display Surface Authority)
- **Role**: Manages visual surface allocation, layer z-ordering, spatial transformation matrices, and mapping Stage 4D workspace context nodes to 2D/spatial viewports.
- **Authority**: Allocates `SurfaceCap` tokens derived from active `WorkspaceCap` handles. A process can only register a surface if it holds a valid workspace handle.

### 6.3 `authui` (Trusted Authorization Overlay Daemon)
- **Role**: Dedicated visual overlay process for rendering system authorization prompts (`OP_AGENT_REQUEST_HUMAN_AUTH`).
- **Anti-Spoofing Invariants**:
  - Rendered in a hardware-enforced top-most z-index layer inaccessible to regular processes.
  - Imprints a cryptographically verifiable hardware-backed visual badge (session nonce + HMAC token) on every prompt.
  - Blurs/isolates underlying workspace surfaces during prompt execution.

### 6.4 `uids` (User Interaction Daemon)
- **Role**: Receives input event streams from hardware input drivers (`Stage 3L`), performs focus targeting, and constructs `IntentDescriptor` messages for dispatch to `intentd` (`Stage 4F`).

---

## 7. Dependency Graph

```text
Stage 3A-3N Kernel Nucleus (Threads, IPC, Capabilities, Syscalls, ZeroFS, Drivers)
  │
  ├──► Stage 4A (init / brokerd - System Service Runtime & Cap Directory)
  │      │
  ├──► Stage 4B (resourced - Physical Resource Leases & Accounting)
  │      │
  ├──► Stage 4C (workloadd - Workload Orchestration & Task Execution DAGs)
  │      │
  ├──► Stage 4D (workspaced - Workspace Containment & Context Graph)
  │      │
  ├──► Stage 4E (agentd - Agent Supervision & Human Auth Trigger Gate)
  │      │
  └──► Stage 4F (intentd / fabricd - Intent Resolution & Compute Placement)
         │
         ▼
Stage 5 User Interaction Substrate & Spatial Presentation Subsystem
  ├── composite / render over Stage 3G SHM buffers & Stage 3L display devices
  ├── ingest input events from Stage 3L input drivers
  ├── map spatial workspace context from Stage 4D
  ├── display trusted human auth prompts from Stage 4E
  └── submit user intents directly to Stage 4F intentd
```

---

## 8. Service & Space Classification

| Component | Execution Domain | Address Space | Primary Isolation Primitive |
|---|---|---|---|
| `compositord` | Ring 3 User Space | Isolated Process | Dedicated PML4 + Real-Time Sched Priority + Exclusive Display `DevCap` |
| `surfaced` | Ring 3 User Space | Isolated Process | Dedicated PML4 + Workspace Capability Attenuation |
| `authui` | Ring 3 User Space | Isolated Process | Dedicated PML4 + Top-Most Compositor Layer Reservation |
| `uids` | Ring 3 User Space | Isolated Process | Dedicated PML4 + Exclusive Input `DevCap` |

---

## 9. Authority & Security Model

### 9.1 Capability Requirements (Stage 3H Integration)

Stage 5 introduces NO new kernel capability types. It relies on existing Stage 3H capability primitives:

```text
Root Display Cap (Stage 3L DevCap)
  │
  ▼ (Granted to compositord at boot by init)
Attenuated Display Cap (Write-Only Framebuffer)
  │
  ▼ (Delegated to surfaced)
Surface Capabilities (ShmObject Cap + Rights Mask)
  │
  ▼ (Delegated to Workspace Processes)
Attenuated Surface Render Cap (SHM_MAP_WRITE | SHM_MAP_READ)
```

### 9.2 Human Authorization Rendering Security
To prevent malicious applications from faking a human authorization dialog:
1. `agentd` dispatches `OP_AGENT_REQUEST_HUMAN_AUTH` to `brokerd`.
2. `brokerd` routes the request exclusively to `authui`'s registered IPC channel.
3. `authui` requests `compositord` to lock input focus and render the `authui` overlay on a reserved, top-most composition layer.
4. Unprivileged processes holding regular `SurfaceCap` tokens cannot elevate their composition layer index above `LAYER_WORKSPACE_MAX` ($100$). `authui` executes on `LAYER_SYSTEM_AUTH` ($255$).

---

## 10. Static Resource Bounds

All Stage 5 memory allocations are statically bounded to guarantee zero dynamic heap exhaustion during runtime:

```rust
pub const MAX_PHYSICAL_DISPLAYS: usize = 4;
pub const MAX_COMPOSITOR_SURFACES: usize = 64;
pub const MAX_INPUT_DEVICES: usize = 8;
pub const MAX_PENDING_AUTH_PROMPTS: usize = 4;
pub const MAX_SURFACE_RAM_MB: usize = 128; // Total composition memory budget
pub const DISPLAY_DEFAULT_WIDTH: u32 = 1024;
pub const DISPLAY_DEFAULT_HEIGHT: u32 = 768;
pub const DISPLAY_BYTES_PER_PIXEL: u32 = 4; // ARGB8888
```

---

## 11. IPC & Protocol Specification (`0x0701`–`0x0712`)

Stage 5 defines IPC opcodes within the range `0x0701`–`0x0712`:

| Opcode | Sender | Target | Description | Payload |
|---|---|---|---|---|
| `0x0701` | `init` | `compositord` | Bind display hardware device | `{ device_id: u64, width: u32, height: u32 }` |
| `0x0702` | `compositord` | `init` | Display bind response | `{ status: i32, display_id: u64 }` |
| `0x0703` | `workspaced` | `surfaced` | Create surface for workspace | `{ workspace_id: DistributedId, width: u32, height: u32 }` |
| `0x0704` | `surfaced` | `workspaced` | Surface create response | `{ status: i32, surface_id: u64, shm_handle: u32 }` |
| `0x0705` | Process | `surfaced` | Commit surface frame buffer | `{ surface_id: u64, damaged_rect: [u32; 4] }` |
| `0x0706` | `surfaced` | Process | Commit surface response | `{ status: i32 }` |
| `0x0707` | `agentd` | `authui` | Render human auth request | `{ auth_req_id: DistributedId, agent_id: DistributedId, prompt_len: u32 }` |
| `0x0708` | `authui` | `agentd` | Human auth response | `{ status: i32, auth_req_id: DistributedId, user_approved: u8 }` |
| `0x0709` | `uids` | `intentd` | Ingest human user intent | `{ workspace_id: DistributedId, intent_len: u32, payload: [u8; 512] }` |
| `0x070A` | `intentd` | `uids` | Intent ingest response | `{ status: i32, intent_id: DistributedId }` |

---

## 12. Persistence Model

Stage 5 state is partitioned into ephemeral presentation state and persistent workspace layout state:
- **Ephemeral State**: Surface framebuffers, dirty damage rects, current input focus, active auth overlay status (stored in BSS static tables).
- **Persistent State**: Workspace spatial layout and display viewport configurations are persisted to ZeroFS (`/sys/workspaces/<ws_id>/spatial.meta`) via Stage 4D persistence APIs (`OP_WORKSPACE_SAVE`).

---

## 13. Failure & Recovery Semantics

```text
[Crash Event]                [Recovery Action]
compositord crashes   ───>   init restarts compositord; hardware display re-initialized;
                             surfaced re-attaches SHM buffers; zero kernel panic.

surfaced crashes      ───>   workspaced re-registers active workspace surfaces;
                             dirty buffers re-committed.

authui crashes        ───>   agentd marks pending human auth requests as TIMED_OUT / DENIED;
                             authui restarted cleanly by init.

uids crashes          ───>   Input queues flushed; uids restarted by init;
                             device drivers remain intact.
```

---

## 14. Machine Verification Strategy (QEMU Ring3)

Stage 5 will be verified through a dedicated Python test suite (`tests/test_stage5.py`) asserting:
1. **Compilation**: `libzero`, `compositord`, `surfaced`, `authui`, and `uids` build cleanly for `x86_64-unknown-none`.
2. **QEMU Machine Verification (26 Tests: 5-A to 5-Z)**:
   - `5-A`: `compositord` Display Binding & Video Mode Verification.
   - `5-B`: Software Framebuffer Render & Blit Test.
   - `5-C`: `surfaced` Surface Allocation & `SurfaceCap` Attenuation.
   - `5-D`: Zero-Copy Shared Memory Framebuffer Swap (`ShmObject`).
   - `5-E`: Spatial Layout Transformation & Viewport Clipping.
   - `5-F`: Layer Z-Ordering Enforcement (`LAYER_SYSTEM_AUTH` precedence).
   - `5-G`: `authui` Prompt Injection Protection (Unprivileged z-index escalation rejected).
   - `5-H`: Anti-Spoofing Visual Badge Validation.
   - `5-I`: `uids` Input Driver Event Ingestion & Dispatch.
   - `5-J`: Input Focus Routing & Isolation.
   - `5-K`: Direct Intent Ingestion (`uids` -> `intentd` `OP_INTENT_SUBMIT`).
   - `5-L`: Spatial Workspace Context Rendering (`workspaced` -> `surfaced`).
   - `5-M`: Real-Time Scheduling Class Assignment for `compositord`.
   - `5-N`: Input-to-Photon Frame Latency Budget Compliance ($<16\text{ ms}$).
   - `5-O`: Surface Destruction & Zero-Copy SHM Release.
   - `5-P`: `authui` Timeout & Denial Enforcement.
   - `5-Q`: Multi-Display Head Topology Setup.
   - `5-R`: `compositord` Crash & Re-Attach Recovery.
   - `5-S`: `surfaced` Crash & Surface Reconstruction.
   - `5-T`: Protocol Robustness & Unknown Opcode Rejection.
   - `5-U`: Invalid Surface Handle Rejection.
   - `5-V`: Framebuffer Memory Quota Enforcement (`MAX_SURFACE_RAM_MB`).
   - `5-W`: Workspace Deletion Surface Teardown.
   - `5-X`: IPC Message Format Verification.
   - `5-Y`: PMM Leak Neutrality (Baseline == Final frame count).
   - `5-Z`: Substrate Preservation (0 bytes modified in Stage 3A-3N and Stage 4A-4F).

---

## 15. Architectural Invariants

1. **Substrate Immutability**: Stage 3A–3N production kernel nucleus and Stage 4A–4F system daemons MUST remain 100% byte-identical.
2. **Input Isolation**: A process cannot receive input events unless it holds active input focus granted by `uids`.
3. **Top-Most Authorization Overlay**: `authui` owns an immutable top-most presentation layer ($255$) that cannot be obscured or spoofed by user surfaces.
4. **Zero Ambient Authority**: Surfaces require explicit `SurfaceCap` tokens derived from active `WorkspaceCap` handles.

---

## 16. Status Record

```text
STATUS: DRAFT — ARCHITECTURE REVIEW REQUIRED
IMPLEMENTATION: NOT AUTHORIZED

STAGE 3A–3N: FROZEN
STAGE 4A–4F: FROZEN
```
