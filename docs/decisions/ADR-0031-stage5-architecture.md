# ADR-0031: Stage 5 User Interaction Substrate & Spatial Presentation Subsystem

- **Status:** DRAFT — ARCHITECTURE REVIEW REQUIRED
- **Implementation:** NOT AUTHORIZED
- **Deciders:** ZeroOS System Architecture Board
- **Date:** 2026-10-06
- **Technical Substrate:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), ADR-0001 through ADR-0030 (Frozen)

---

## 1. Context & Problem Statement

Following the completion of Stage 4F (Intent Resolution & Personal Compute Fabric), ZeroOS possesses a complete backend subsystem capable of resolving human intent into deterministic execution DAGs across a personal compute fabric.

However, ZeroOS lacks a **capability-secured user interaction substrate** to connect physical human input and display presentation safely. Specifically:
1. Physical human inputs (keyboard, mouse/pointer) have no secure ingestion path into `intentd` (Stage 4F) without risking input interception or keylogging.
2. Human authorization prompts (`OP_AGENT_REQUEST_HUMAN_AUTH` from Stage 4E) have no trusted, un-spoofable visual overlay for user rendering.
3. Workspaces (`workspaced`, Stage 4D) and Intent execution plans (`intentd`, Stage 4F) cannot be presented visually or spatially to the user.
4. User processes cannot render visual output without risking ambient access to physical framebuffers or peer display memory.

---

## 2. Considered Alternatives

### Alternative A: Monolithic Display Server (X11 / Wayland POSIX Clone)
- **Description**: Port an existing monolithic display protocol or build a Unix-domain socket window manager.
- **Rejected Reasons**: Bypasses Stage 3H capability security, introduces POSIX socket assumptions, relies on ambient authority for surface access, and violates microkernel fault isolation.

### Alternative B: Direct Process Framebuffer Mapping (Kernel Framebuffer)
- **Description**: Map physical display memory directly into user processes via kernel syscalls.
- **Rejected Reasons**: Destroys spatial composition, allows any process with framebuffer access to read/modify peer pixels (violating privacy), and prevents real-time compositor scheduling.

### Alternative C: Capability-Secured User Interaction & Presentation Subsystem (Chosen Stage 5)
- **Description**: A modular Ring 3 user-space presentation architecture consisting of:
  - `compositord`: Spatial compositor daemon managing display hardware (`DeviceId::Display(0)`) via `sys_dev_map_mmio` and blending Stage 3G `ShmObject` surfaces.
  - `surfaced`: Surface authority daemon mapping Stage 4D workspace context nodes to visual surfaces via Stage 3H capability derivation ($C_{ws} \to C_{surf}$).
  - `authui`: Isolated, trusted visual overlay daemon for rendering cryptographically bound `AuthorizationTransaction`s with visual HMAC badges.
  - `uids`: User interaction daemon ingesting input events and submitting formatted intent descriptors to `intentd` under invariant `I-INPUT-NO-IMPLICIT-AUTHORITY`.

---

## 3. Decision & Architectural Boundaries

We adopt **Alternative C (Stage 5 User Interaction Substrate Rev4)** as the authoritative Stage 5 boundary.

```text
Human Input ──► uids ──► intentd ──► agentd ──► workspaced ──► workloadd
                  ▲                                                   │
                  │                                                   ▼
                authui ◄── AuthorizationTransaction ◄────── resourced / fabricd
                  │
                  ▼
             compositord ◄── surfaced ◄── Workspace Surface
                  │
                  ▼
          Display Hardware
```

### Key Architectural Rules & Constraints (Closing Architectural Review Rev3)

1. **Substrate Immutability**: Production Stage 3A–3N kernel nucleus and Stage 4A–4F system daemons remain 100% byte-identical and preserved. Zero new kernel primitives are added.
2. **MMIO vs. SHM Mapping Distinction & Stage 3L Alignment (`ADR-0021`)**: `compositord` maps physical video memory `[0xE0000000, 0xE0300000)` via `sys_dev_map_mmio` using an attenuated display `DevCap` delegated exclusively by `init`. Surface buffers are mapped via `sys_shm_map` using Stage 3G `ShmObject` handles. Unprivileged processes hold no display `DevCap`.
3. **No Parallel Capability System**: `SurfaceCap` and `C_auth_tx` are explicitly rejected. Invariant `I-AUTH-TX-NOT-CAPABILITY` is enforced: `AuthorizationTransactionRef` is authenticated protocol state owned by `agentd` and conveys zero Stage 3H kernel capability. Surface presentation buffers are represented exclusively by Stage 3G `ShmObject` capabilities ($C_{surf}$) derived from parent Stage 3H `WorkspaceCap` handles ($C_{ws}$).
4. **Service Role Envelopes via `init` Delegation**: Ring 3 policy roles are established during bootstrap: `init` delegates `SurfacePolicyCap` to `surfaced` and `InputFocusPolicyCap` to `uids`. Unprivileged daemons cannot self-declare policy authority.
5. **Capability-Gated Trusted Overlay (`SURFACE_TYPE_AUTH_OVERLAY`)**: Setting `SURFACE_TYPE_AUTH_OVERLAY` requires `surfaced` to query `agentd` to verify an active `AuthorizationTransaction` for `authui`. Unprivileged applications attempting to set an auth overlay are rejected with `ZeroError::PermissionDenied`.
6. **Trusted Authorization Transaction Binding**: Authorization decisions are cryptographically bound to an `AuthorizationTransaction` (`transaction_id`, `agent_id`, `workspace_id`, `operation_id`, `requested_action_hash`). Approvals cannot be reused. Prompts carry a visual HMAC badge ($K_{session}$) to prevent peer UI spoofing.
7. **ModalLock & Input Focus Authority**: `uids` is the sole owner and enforcer of input focus under its `InputFocusPolicyCap`. `uids` enters `ModalLock` **only** upon validating an active `AuthorizationTransaction` with `agentd` (`OP_UIDS_REQUEST_MODAL_LOCK`). Peer surfaces receive `FocusLost`.
8. **Fail-Closed Auth UI**: On `authui` crash, all pending `AuthorizationTransaction`s in `agentd` immediately transition to **DENIED** (Fail-Closed). `uids` releases `ModalLock` automatically.
9. **Invariant `I-INPUT-NO-IMPLICIT-AUTHORITY`**: `uids` is an input ingestion service with ZERO capability authority, ZERO workload execution authority, and cannot approve authorization requests. All human intents must pass full `intentd` validation.
10. **Producer/Consumer Buffer Lifecycle & Teardown Safety**: Double-buffered Stage 3G `ShmObject` with atomic state transitions: `FREE -> WRITING` (Producer), `WRITING -> READY` (Producer), `READY -> COMPOSITING` (Consumer), `COMPOSITING -> FREE` (Consumer). Producer crash, process termination, workspace deletion ($C_{ws}$ revocation), or `compositord` crash safely resets buffer state to `FREE`, preventing permanent deadlocks.
11. **Compositor Scheduler Latency Semantics**: `compositord` thread executes in Stage 3B **Priority::Critical**. Soft target frame budget is $16.6\text{ ms}$ ($60\text{ Hz}$). On frame deadline miss, previous frame is retained without display corruption.

---

## 4. Consequences & Security Implications

### Positive Consequences
- Completes the end-to-end ZeroOS execution loop (`Human -> uids -> intentd -> agentd -> workspaced -> workloadd -> resourced -> fabricd -> kernel`).
- Prevents UI spoofing and keylogging via capability-bounded input focus and top-most authorization overlays.
- Ensures latency-deterministic display composition ($<16.6\text{ ms}$) via Priority::Critical execution.
- Requires zero new kernel primitives and preserves full Stage 3A–3N and Stage 4A–4F substrate byte-identical neutrality.

### Negative Consequences / Trade-offs
- Additional system service IPC overhead (`0x0701`–`0x0712`).
- 2D software composition memory budget capped at `MAX_SURFACE_RAM_MB` ($128\text{ MB}$).

---

## 5. Status Record

```text
STATUS: DRAFT — ARCHITECTURE REVIEW REQUIRED
IMPLEMENTATION: NOT AUTHORIZED

STAGE 3A–3N: FROZEN
STAGE 4A–4F: FROZEN
```
