# ZEROOS MVP IMPLEMENTATION PLAN REV2

## Status: 🟢 AUTHORIZED IMPLEMENTATION PLAN
**Document ID:** `ZEROOS-MVP-IMPLEMENTATION-PLAN-REV2`  
**Authoritative Specifications:** `ZEROOS-PRODUCTIZATION-PLAN-REV2.md` & `ZEROOS-MVP-IMPLEMENTATION-REVIEW-REV1.md`  
**Prerequisites:** Stages 3A–3N, 4A–4F, 5, 6A–6F (100% Completed, Verified & Validated)  
**Evidence Baseline:** `ZEROOS-PRODUCT-PROOF-RESULTS-EXP01-07` (7 / 7 Experiments Passed)  
**Execution Directive:** **DO NOT WRITE CODE YET. DO NOT BUILD STAGE 7. DO NOT ADD NEW OS PRIMITIVES OR KERNEL SYSCALLS.** Scope ZeroOS Workstation Edition v1.0 strictly to a single-node workstation product.

---

## 1. Executive Summary

The ZeroOS Product Proof validation phase proved that the Stage 3A–6F architecture absorbs computing complexity ($H, C, K, R, T$) across 7 scenario families without Category D architectural gaps. The subsequent productization specification (`ZEROOS-PRODUCTIZATION-PLAN-REV2.md`) scoped the initial release strictly to **ZeroOS Workstation Edition v1.0 (Single-Node Edition)**.

Following the adversarial review (`ZEROOS-MVP-IMPLEMENTATION-REVIEW-REV1.md`), this implementation plan defines the complete engineering delta required to deliver a booted single-node v1.0 product. 

In addition to the 3 Category B `libzero` application adapters, UI error formatting, and ISO packaging, Rev2 incorporates **`WI-09` (`init` daemon process spawning)** to unblock service startup, and **`WI-10` (VFS session snapshot disk persistence)** to ensure Stage 6F session recovery survives hard power resets on disk storage.

No kernel code modifications, new syscalls, capability types, sidecar daemons, or Stage 7 abstractions are introduced.

---

## 2. Current Repository Reality

An audit of the repository establishes that the core Stage 3A–6F microkernel nucleus and 5 core user-space daemons are **already fully implemented, freestanding, and verified**:

```text
               REPOSITORY SUBSTRATE INVENTORY AUDIT

  SUBSYSTEM PRIMITIVE                   STATUS             VERIFICATION / LOCATION
  ---------------------------------     ───────────────    ─────────────────────────
  Stage 3A–3N Ring0 Kernel Nucleus      EXISTS — READY     100% Byte-Identical (`kernel/src/stage3/`)
  Stage 3H Capability Token Engine      EXISTS — READY     test_stage3h.py PASS (`kernel/src/stage3/`)
  intentd Intent Resolution Daemon      EXISTS — READY     test_stage6d.py PASS (`intentd/src/main.rs`)
  workspaced Workload DAG & Session     EXISTS — READY     test_stage4a.py PASS (`workspaced/src/main.rs`)
  groundd Spatial Observation Broker    EXISTS — READY     test_stage6f.py PASS (`groundd/src/main.rs`)
  shelld / surfaced Compositor          EXISTS — READY     test_stage5.py PASS (`shelld/src/main.rs`)
  resourced Resource Lease Engine       EXISTS — READY     test_stage4f.py PASS (`resourced/src/main.rs`)
  fabricd Transport Infrastructure      EXISTS — READY     test_stage4f.py PASS (v1.0 Local Only)
  init Service Supervisor               EXISTS — INCOMPLETE init/src/main.rs (Exits after inline test)
  Session Snapshot Disk Ingress         EXISTS — INCOMPLETE Needs VFS file backing (/var/session/)
```

---

## 3. Existing vs. Missing Implementation

To maintain strict discipline, every v1.0 component is explicitly categorized:

| Component / Mechanism | Implementation Category | Owning Layer | Reused Contract | New Contract Required? |
|---|---|---|---|:---:|
| **Ring0 Microkernel Nucleus** | `EXISTS — READY` | Kernel (`kernel/src/`) | Stage 3A–3N Syscalls | **NO** |
| **Capability Engine** | `EXISTS — READY` | Kernel (`kernel/src/stage3/`) | Stage 3H Capability Tokens | **NO** |
| **Intent Resolution (`intentd`)** | `EXISTS — READY` | User-Space (`intentd/`) | Stage 6D IPC Opcodes | **NO** |
| **Workload DAG (`workspaced`)** | `EXISTS — READY` | User-Space (`workspaced/`) | Stage 4A DAG Lifecycle | **NO** |
| **Spatial Observation (`groundd`)**| `EXISTS — READY` | User-Space (`groundd/`) | Stage 6F Spatial Grounding | **NO** |
| **Spatial Compositor (`shelld`)** | `EXISTS — READY` | User-Space (`shelld/`) | Stage 5 Surface Compositing | **NO** |
| **Resource Lease Engine** | `EXISTS — READY` | User-Space (`resourced/`) | Stage 4F Resource Leases | **NO** |
| **Terminal Grounding Library** | `MISSING — CATEGORY B` | Application Library (`libzero`) | `groundd` Read-Only IPC | **NO** |
| **Markdown Surface Renderer** | `MISSING — CATEGORY B` | Application Library (`libzero`) | Stage 3H Capability Pipes | **NO** |
| **Toolchain Stdio Pipe Wrapper** | `MISSING — CATEGORY B` | Application Library (`libzero`) | Stage 3H Pipe Handles | **NO** |
| **Human UI Error Formatting** | `MISSING — CATEGORY C` | User-Space (`shelld/intentd`) | Standard IPC Responses | **NO** |
| **`init` Daemon Process Spawning** | `MISSING — CATEGORY C` | User-Space (`init/src/main.rs`)| Stage 4A Supervisor | **NO** |
| **VFS Session Snapshot Persistence**| `MISSING — CATEGORY C` | User-Space (`vfs/journal.rs`) | Stage 3K ZeroFS / Stage 6F | **NO** |
| **Bootable Single-Node ISO** | `MISSING — CATEGORY C` | Build Tools (`tools/`) | GRUB/QEMU Bootloader | **NO** |
| **Remote Fabric Compute Offload**| `NOT REQUIRED FOR V1.0`| Subsystem (`fabricd`) | Stage 4F/6F Fabric | Deferred to v1.1+ |

---

## 4. Detailed Specification of Work Items

### 4.1 WI-09 — `init` Daemon Process Spawning & Supervision (`init/src/main.rs`)

- **Discovered Repository State:** `init/src/main.rs` currently initializes supervisor data structures, runs an inline verification block, and calls `sys_exit(0)`.
- **Required Transition:** Update `init` from verification-only execution to a persistent user-space service supervisor that spawns and maintains background daemon processes.
- **Minimum v1.0 Service Set & Startup Ordering:**
  ```text
  Step 1: brokerd (Primary IPC Name Broker)
       │
       ▼
  Step 2: resourced (Local Node Resource Lease Manager)
       │
       ▼
  Step 3: workspaced (Workload DAG & Workspace State Container)
       │
       ▼
  Step 4: intentd (Intent Resolution & Plan Compiler)
       │
       ▼
  Step 5: groundd (Read-Only Spatial Observation Broker)
       │
       ▼
  Step 6: shelld / surfaced (Spatial Desktop Compositor & Canvas UI)
  ```
- **Supervision & Restart Behavior:** `init` executes a process supervision loop using existing `libzero::supervisor::Supervisor` handles. On daemon exit/crash, `init` retries startup up to `DEFAULT_MAX_RETRIES` (3 retries). If retries are exhausted, `init` notifies `shelld` to render user-space recovery UI.
- **Shutdown Behavior:** On clean shutdown signal, `init` issues orderly stop transitions in reverse startup order (`shelld -> groundd -> intentd -> workspaced -> resourced -> brokerd`) and calls `sys_exit(0)`.
- **Reused Contracts:** Existing Stage 4A `libzero::supervisor` and Stage 3F process spawning syscalls (`sys_spawn`).
- **Category:** **Category C (Missing User-Space Implementation)**. Reuses 100% existing kernel syscalls and Stage 4A contracts.

---

### 4.2 WI-10 — VFS-Backed Session Snapshot Persistence (`vfs/journal.rs`)

- **Discovered Repository State:** Stage 6F `LogicalSessionSnapshotHeader` (64B ABI) session snapshot recovery is validated in memory buffers, but lacks an automatic disk storage write-through path for hard resets.
- **Required Transition:** Connect `workspaced` and `shelld` session snapshot serializers to a durable single-node VFS disk image backing file (`/var/session/snapshot.bin`) using existing Stage 3K `ZeroFS` contracts.
- **Snapshot Storage Location:** `/var/session/snapshot.bin`.
- **Write / Commit Behavior:**
  - `shelld` and `workspaced` write updated snapshot headers on spatial layout updates or DAG state changes.
  - Flush to disk image completes within 50ms using standard VFS write syscalls.
- **Startup Restoration & Crash Behavior:**
  - Upon reboot, `init` launches `workspaced` and `shelld`.
  - `workspaced` checks `/var/session/snapshot.bin`.
  - Validates snapshot magic (`0x534E4150_36463031`) and SHA-256 header checksum.
  - Reconstructs active workspace layout, surface bounds, and DAG node progress.
- **Stale / Corrupt Snapshot Handling:** If header validation or checksum fails (e.g. truncated file from power loss mid-write), `workspaced` discards the corrupt journal block and restores the last valid checkpoint without crashing.
- **Reused Contracts:** Stage 3K `ZeroFS`, Stage 4D workspace persistence, Stage 6A session state, Stage 6F fencing.
- **Category:** **Category C (Missing User-Space Implementation)**. Reuses 100% existing VFS and Stage 6F snapshot contracts.

---

### 4.3 Category B Application IPC Adapters (`libzero/src/adapters/`)

1. **`zero-term-lib` (WI-02):** Connects terminal scrollback buffer to `groundd` spatial observation socket under `WorkspaceAccessCap` for EXP-02 information synthesis.
2. **`zero-doc-lib` (WI-03):** Enables native `shelld` rendering of markdown output streams received over `Stage 3H` capability IPC pipes for EXP-01 artifact assembly.
3. **`zero-exec-lib` (WI-04):** Wraps standard command-line tools in capability handles so `workspaced` can pipe stdio streams in memory without temporary files on disk for EXP-04 data pipelines.

---

### 4.4 `shelld` Human Error UI Formatter (WI-05)

- Converts raw IPC error codes (`ZeroError::TimeAuthorityUnavailable`) into clear user-facing notifications ("Service temporarily unavailable; task deferred"). Reuses existing `shelld` canvas text rendering.

---

### 4.5 Standalone Bootable ISO Packager (WI-01)

- `tools/build_iso.py` automates compilation of all crates and packages Stage 1 bootloader, Stage 2 nucleus, kernel ELF, and user-space binaries into `build/zeroos-v1.0-x86_64.iso`.

---

## 5. Recalculated MVP Critical Path

The updated dependency sequence required to deliver ZeroOS Workstation Edition v1.0:

```text
                      RECALCULATED MVP CRITICAL PATH

  Step 1: ISO Image Packager Script (tools/build_iso.py - WI-01)
       │
       ▼
  Step 2: init Daemon Process Spawning (init/src/main.rs - WI-09)
       │
       ▼
  Step 3: VFS Session Snapshot Disk Persistence (vfs/journal.rs - WI-10)
       │
       ▼
  Step 4: Category B Application Adapters (libzero/src/adapters/ - WI-02..04)
       │   ├── zero-term-lib (Terminal scrollback grounding adapter)
       │   ├── zero-doc-lib (Markdown capability pipe renderer)
       │   └── zero-exec-lib (Toolchain stdio handle wrapper)
       │
       ▼
  Step 5: Human Error UI Formatter (shelld/src/ui.rs - WI-05)
       │
       ▼
  Step 6: Machine Verification Gates (GATE-MVP-01 through GATE-MVP-12)
       │
       ▼
  Step 7: Single-Node 10-Minute Onboarding Scenario Verification (WI-06)
```

---

## 6. Refined Machine Acceptance Gates

All 12 gates are updated for single-node scope and disk-backed persistence:

| Gate ID | Gate Name | Setup / Action | Machine-Verifiable Evidence | Failure Class |
|---|---|---|---|:---:|
| **GATE-MVP-01** | Clean Compilation | Build kernel and all daemons. | Exit code 0, 0 compiler errors/warnings. | Cat A |
| **GATE-MVP-02** | Bootable ISO Image | Boot `zeroos-v1.0-x86_64.iso` in QEMU. | QEMU serial output: `"ZeroOS Nucleus Initialized"`. | Cat A |
| **GATE-MVP-03** | End-to-End Recovery | Write snapshot to `/var/session/snapshot.bin` $\to$ reset $\to$ reboot. | `workspaced` reads snapshot file; spatial layout & DAG state restored. | Cat C |
| **GATE-MVP-04** | Intent-to-DAG Execution | Submit intent via `intentd`. | `OP_INTENT_SUBMIT_RESP` success; DAG node complete. | Cat C |
| **GATE-MVP-05** | Mid-Task Intent Pivot | Submit `OP_INTENT_CANCEL` mid-job. | DAG canceled within 120ms; resource leases freed. | Cat C |
| **GATE-MVP-06** | Surface Disconnect | Terminate `shelld` surface socket 30s. | `workspaced` DAG completes in background ($R = 0$). | Cat C |
| **GATE-MVP-07** | Spatial Grounding | Query `groundd` spatial context. | Visible window text returned; auth fields redacted. | Cat C |
| **GATE-MVP-08** | Single-Node Fencing | Hard reset during active session. | Local `BootEpochId` incremented; pre-reset nonces invalidated. | Cat C |
| **GATE-MVP-09** | Application Integration | Execute `zero-exec-lib` stdio pipe. | Intermediate data streamed via `Stage 3H` IPC; 0 `/tmp` files. | Cat B |
| **GATE-MVP-10** | 10-Min Onboarding Flow| Run single-node onboarding script. | All 4 single-node scenarios complete ($H \le 2, K = 0$). | Cat C |
| **GATE-MVP-11** | Offline Autonomy | Disconnect virtual network interface. | Local intent submission & spatial grounding succeed. | Cat C |
| **GATE-MVP-12** | Kernel Preservation | SHA-256 hash `kernel/src/stage3/`. | 100% byte-identical to Stage 3N baseline. | Cat D |

*Note on GATE-MVP-08:* Multi-node fencing proofs over `fabricd` (`receive_fencing_proof`) remain fully validated in the codebase, but are relocated to the v1.1+ regression test suite to match single-node v1.0 scope.

---

## 7. Product Proof Scenario & Dimension Mapping

```text
               PRODUCT PROOF EXPERIMENT MAPPING

  WORK ITEM                   PROVED BY EXPERIMENT    PRIMARY METRIC
  ------------------------    ────────────────────    ───────────────────────
  Intent-to-DAG Execution     EXP-01 (Creation)       H: 8 -> 2 (75% reduction)
  Spatial Grounding           EXP-02 (Information)    H: 7 -> 1, C: 3 -> 0
  WI-10 Disk Recovery         EXP-03 (Continuity)     R: 12 -> 0, C: 4 -> 0
  Capability IPC Piping       EXP-04 (Coordination)   K: 5 -> 0 (100% reduction)
  Surface Decoupling          EXP-05 (Long-Running)   R: 8 -> 0, T = 0
  Dynamic Intent Pivot        EXP-06 (Intent Change)  R: 4 -> 0, T = 0
  Fabric Remote Offload       EXP-07 (Distributed)    DEFERRED TO v1.1+
```

---

## 8. v1.0 Non-Goals

The following items remain strictly **EXCLUDED** from v1.0:

```text
❌ DO NOT implement Multi-Node Fabric Remote Compute (EXP-07 Offload) in v1.0 (Deferred to v1.1+).
❌ DO NOT write Stage 7 Architecture or Kernel Code.
❌ DO NOT create new kernel syscalls or modify kernel/src/stage3/.
❌ DO NOT introduce new Stage 3H capability types.
❌ DO NOT create custom sidecar adapter daemons.
❌ DO NOT build AI benchmark infrastructure or complex demo apps.
❌ DO NOT build administrative system monitoring dashboards.
❌ DO NOT introduce distributed consensus frameworks.
```

---

## 9. Definition of MVP Completion

> **ZeroOS Workstation Edition v1.0 MVP is COMPLETE when:**  
> All 12 Machine Acceptance Gates (`GATE-MVP-01` through `GATE-MVP-12`) pass cleanly in automated QEMU test harness, `zeroos-v1.0-x86_64.iso` boots into `shelld`, `init` spawns and supervises all 6 core daemons, `workspaced` flushes/restores snapshot state to `/var/session/snapshot.bin`, Category B `libzero` libraries stream stdio via `Stage 3H` capability pipes without intermediate disk files, Stage 3A–3N microkernel bytes remain 100% byte-identical, and PMM frame accounting proves 0 leaked frames.

---

## 10. Implementation Work Item Scope Table

| Work Item ID | Description | Category | Owner | Depends On | Product Proof | Verification Gate | Included in v1.0? |
|---|---|---|---|---|---|---|:---:|
| **WI-01** | Standalone ISO Packager (`build_iso.py`) | Cat C | Build Tools | `run_qemu.py` | EXP-01 | GATE-MVP-01, 02 | **YES** |
| **WI-02** | `zero-term-lib` Terminal Grounding Adapter | Cat B | `libzero` | `groundd` | EXP-02 | GATE-MVP-07, 09 | **YES** |
| **WI-03** | `zero-doc-lib` Markdown Pipe Renderer | Cat B | `libzero` | `shelld` | EXP-01 | GATE-MVP-04, 09 | **YES** |
| **WI-04** | `zero-exec-lib` Toolchain Pipe Adapter | Cat B | `libzero` | `workspaced` | EXP-04 | GATE-MVP-04, 09 | **YES** |
| **WI-05** | `shelld` Human Error UI Formatter | Cat C | `shelld` | `intentd` | EXP-06 | GATE-MVP-05, 10 | **YES** |
| **WI-06** | Single-Node 10-Min Onboarding Test Script | Cat C | Harness | WI-01 to 05, 09, 10 | EXP-01..06 | GATE-MVP-10, 11 | **YES** |
| **WI-07** | Ring0 Nucleus Byte Preservation Audit | Cat D | Kernel | All WI | Stage 3A–3N | GATE-MVP-12 | **YES** |
| **WI-08** | Multi-Node Fabric Compute Offloader | Cat C | `fabricd` | `workspaced` | EXP-07 | N/A | **NO (v1.1)** |
| **WI-09 (NEW)**| `init` Daemon Process Spawner (`main.rs`)| Cat C | `init` | Stage 4A | EXP-03, 05 | GATE-MVP-02, 03 | **YES** |
| **WI-10 (NEW)**| VFS Session Snapshot Journal (`journal.rs`)| Cat C | `vfs` | Stage 3K, 6F | EXP-03 | GATE-MVP-03, 08 | **YES** |

---

## 11. MVP Rev2 Sign-Off

```text
Architecture:
🟢 Validated for Stage 3A–6F contracts and Product Proof scenarios

v1.0 Product Scope:
🟢 Single-node workstation edition

Implementation Plan:
🟢 Revised to include WI-09 (init spawning) and WI-10 (VFS session persistence)

Category D Architectural Gap:
🟢 None identified (0 new syscalls, 0 new capability types, 0 Stage 7)

Implementation:
🔴 NOT YET AUTHORIZED

Stage 7:
🔴 NOT STARTED
```

---

## 🟢 IMPLEMENTATION PLAN REV2 FROZEN

The revised implementation plan for ZeroOS Workstation Edition v1.0 is complete and frozen. No implementation code has been written. Execution is ready to proceed to Phase 1 (ISO packager, WI-09 init spawner, WI-10 VFS session persistence, and Category B `libzero` application libraries) when authorized.
