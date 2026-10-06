# ZEROOS MVP IMPLEMENTATION PLAN REV1

## Status: 🟢 AUTHORIZED IMPLEMENTATION PLAN
**Document ID:** `ZEROOS-MVP-IMPLEMENTATION-PLAN-REV1`  
**Target Specification:** `docs/design/ZEROOS-PRODUCTIZATION-PLAN-REV2.md`  
**Prerequisites:** Stages 3A–3N, 4A–4F, 5, 6A–6F (100% Completed, Verified & Validated)  
**Evidence Baseline:** `ZEROOS-PRODUCT-PROOF-RESULTS-EXP01-07` (7 / 7 Experiments Passed)  
**Execution Directive:** **DO NOT WRITE CODE YET. DO NOT BUILD STAGE 7. DO NOT ADD NEW OS PRIMITIVES OR KERNEL SYSCALLS.** This document establishes the concrete, minimal implementation sequence required to deliver **ZeroOS Workstation Edition v1.0 (Single-Node MVP)**.

---

## 1. Executive Summary

The ZeroOS Product Proof validation phase proved that the Stage 3A–6F architecture absorbs computing complexity ($H, C, K, R, T$) across 7 scenario families without Category D architectural gaps. The subsequent productization specification (`ZEROOS-PRODUCTIZATION-PLAN-REV2.md`) scoped the initial release strictly to **ZeroOS Workstation Edition v1.0 (Single-Node Edition)**.

This implementation plan defines the smallest concrete engineering delta required to turn the already-validated ZeroOS substrate into a shippable v1.0 single-node product. 

No kernel code modifications, new syscalls, capability types, sidecar daemons, or Stage 7 abstractions are introduced. Engineering work focuses exclusively on packaging 3 lightweight `libzero` Category B application adapters, formatting user-space UI error notifications, and assembling a bootable single-node ISO image.

---

## 2. Current Repository Reality

An audit of the repository reveals that the vast majority of required ZeroOS mechanisms are **already fully implemented, verified, and active**:

```text
               REPOSITORY SUBSTRATE INVENTORY AUDIT

  SUBSYSTEM PRIMITIVE                   STATUS             VERIFICATION
  ---------------------------------     ───────────────    ─────────────────────
  Stage 3A–3N Ring0 Kernel Nucleus      EXISTS — READY     100% Byte-Identical
  Stage 3H Capability Token Engine      EXISTS — READY     test_stage3h.py PASS
  intentd Intent Resolution Daemon      EXISTS — READY     test_stage6d.py PASS
  workspaced Workload DAG & Session     EXISTS — READY     test_stage4a.py PASS
  groundd Spatial Observation Broker    EXISTS — READY     test_stage6f.py PASS
  shelld Spatial Desktop Compositor     EXISTS — READY     test_stage5.py PASS
  resourced Resource Lease Engine       EXISTS — READY     test_stage4f.py PASS
  fabricd Transport Infrastructure      EXISTS — READY     test_stage4f.py PASS (v1.0 Local Only)
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
| **Session Snapshot Recovery** | `EXISTS — READY` | User-Space (`workspaced/shelld`) | Stage 6F Session Headers | **NO** |
| **Terminal Grounding Library** | `MISSING — CATEGORY B` | Application Library (`libzero`) | `groundd` Read-Only IPC | **NO** |
| **Markdown Surface Renderer** | `MISSING — CATEGORY B` | Application Library (`libzero`) | Stage 3H Capability Pipes | **NO** |
| **Toolchain Stdio Pipe Wrapper** | `MISSING — CATEGORY B` | Application Library (`libzero`) | Stage 3H Pipe Handles | **NO** |
| **Human UI Error Formatting** | `MISSING — CATEGORY C` | User-Space (`shelld/intentd`) | Standard IPC Responses | **NO** |
| **Bootable Single-Node ISO** | `MISSING — CATEGORY C` | Build Tools (`tools/`) | GRUB/QEMU Bootloader | **NO** |
| **Remote Fabric Compute Offload**| `NOT REQUIRED FOR V1.0`| Subsystem (`fabricd`) | Stage 4F/6F Fabric | Deferred to v1.1+ |

---

## 4. v1.0 Capability Matrix (Single-Node Edition)

The 10 mandatory single-node v1.0 capabilities and their implementation boundaries:

```text
1. Single-Node Workload Execution
   - Existing: workspaced DAG dispatch, capability token validation, resourced leases.
   - Missing: None.
   - Category: EXISTS — READY

2. Workspace / Context Continuity
   - Existing: LogicalSessionSnapshotHeader (64B ABI), fencing proofs, spatial layout store.
   - Missing: Atomic snapshot disk flush timing polish.
   - Category: EXISTS — READY

3. Read-Only Spatial Grounding
   - Existing: groundd spatial grounding broker, WorkspaceAccessCap, privacy redaction.
   - Missing: zero-term-lib buffer adapter.
   - Category: MISSING — CATEGORY B

4. Intent-Driven Workload / DAG Execution
   - Existing: intentd intent submission, plan compilation, 1-click confirmation.
   - Missing: None.
   - Category: EXISTS — READY

5. Mid-Task Intent Pivot & Cancellation
   - Existing: OP_INTENT_CANCEL, workspaced DAG cancellation, Stage 3H token revocation.
   - Missing: None.
   - Category: EXISTS — READY

6. Surface-Independent Workload Survival
   - Existing: shelld socket disconnect decoupling, workspaced background execution.
   - Missing: None.
   - Category: EXISTS — READY

7. Minimal Application Integration
   - Existing: Capability IPC pipe handles, stdout/stdin streaming.
   - Missing: zero-term-lib, zero-doc-lib, zero-exec-lib.
   - Category: MISSING — CATEGORY B

8. Bootable / Installable Workstation Experience
   - Existing: Stage 1/2 bootloaders, kernel ELF builder, QEMU runner script.
   - Missing: Standalone single-node bootable ISO packager script (tools/build_iso.py).
   - Category: MISSING — CATEGORY C

9. Zero-Reconstruction Recovery
   - Existing: Stage 6F epoch validation (receive_fencing_proof), workspace auto-resume.
   - Missing: None.
   - Category: EXISTS — READY

10. Human-First 10-Minute Onboarding Flow
   - Existing: All supporting daemons (intentd, workspaced, groundd, shelld).
   - Missing: Integrated onboarding scenario test script.
   - Category: MISSING — CATEGORY C
```

---

## 5. MVP Critical Path

The minimal implementation sequence required to deliver ZeroOS Workstation Edition v1.0:

```text
                      ZEROOS v1.0 MVP CRITICAL PATH

  Step 1: ISO Image Builder Script (tools/build_iso.py)
       │
       ▼
  Step 2: Category B Application Adapters (libzero/src/adapters/)
       │   ├── zero-term-lib (Terminal scrollback grounding adapter)
       │   ├── zero-doc-lib (Markdown capability pipe renderer)
       │   └── zero-exec-lib (Toolchain stdio handle wrapper)
       │
       ▼
  Step 3: User-Space Human Error Formatting (shelld/src/ui.rs)
       │
       ▼
  Step 4: Machine Verification Gates (GATE-MVP-01 through GATE-MVP-12)
       │
       ▼
  Step 5: 10-Minute Human Onboarding Scenario End-to-End Verification
```

---

## 6. Component Implementation Plan

### 6.1 `libzero` Application IPC Adapters (`libzero/src/adapters/`)
- **Objective:** Package lightweight Category B application integration adapters using existing `libzero` capability IPC handles.
- **Dependencies:** `libzero` IPC pipe interfaces.
- **New Syscalls / Architecture:** **ZERO.**

### 6.2 `shelld` Human Error Formatting (`shelld/src/ui.rs`)
- **Objective:** Convert raw kernel/IPC status codes (`ZeroError::TimeAuthorityUnavailable`) into clear user-facing notifications ("Service temporarily unavailable; task deferred").
- **Dependencies:** `shelld` compositor text rendering.
- **New Syscalls / Architecture:** **ZERO.**

### 6.3 Standalone ISO Image Packager (`tools/build_iso.py`)
- **Objective:** Assemble Stage 1 bootloader, Stage 2 nucleus, kernel ELF, and user-space daemon binaries into a single bootable ISO image (`build/zeroos-v1.0-x86_64.iso`).
- **Dependencies:** `tools/run_qemu.py`, GRUB/xorriso packager tools.
- **New Syscalls / Architecture:** **ZERO.**

---

## 7. Application Integration Plan (Category B)

Integration uses the **Smallest Adapter Surface Principle**:

```text
┌──────────────────────────────────────────────────────────────────────────┐
│                   CATEGORY B LIGHTWEIGHT ADAPTER SURFACE                 │
├──────────────────────────────────────────────────────────────────────────┤
│ 1. Terminal Adapter (zero-term-lib): Exposes terminal buffer to groundd  │
│ 2. Document Adapter (zero-doc-lib):  Renders markdown IPC streams        │
│ 3. Toolchain Adapter (zero-exec-lib): Wraps stdio in capability pipes    │
└──────────────────────────────────────────────────────────────────────────┘
```

1. **`zero-term-lib` (Terminal Integration):**
   - *Why Needed:* Enables `groundd` to read active terminal error tracebacks for EXP-02 information synthesis.
   - *Existing Contract Used:* `groundd` read-only spatial grounding IPC interface.
   - *Implementation:* Lightweight C/Rust header linking terminal window buffer to `groundd` observation socket under `WorkspaceAccessCap`.

2. **`zero-doc-lib` (Document Integration):**
   - *Why Needed:* Enables `shelld` to render rich markdown output generated by `workspaced` DAG nodes for EXP-01 artifact assembly.
   - *Existing Contract Used:* `Stage 3H` capability IPC pipes.
   - *Implementation:* Stream reader passing markdown text to `shelld` canvas surface.

3. **`zero-exec-lib` (Toolchain Integration):**
   - *Why Needed:* Enables standard build tools (`make`, `cargo`, `gcc`) to execute as DAG nodes for EXP-04 data pipelines.
   - *Existing Contract Used:* `Stage 3H` pipe handles (`stdin`/`stdout`).
   - *Implementation:* Stdio handle wrapper passing output streams through capability handles without temporary disk files.

---

## 8. Boot / Packaging / Recovery Plan

1. **Boot Sequence:**
   `Stage 1 (Bootsector) -> Stage 2 (Nucleus Setup) -> Kernel ELF -> Init Task -> Core Daemons (intentd, workspaced, resourced, groundd, shelld)`
2. **Single-Node ISO Packaging:** `tools/build_iso.py` automates compilation of all crates and generates `zeroos-v1.0-x86_64.iso`.
3. **Session Recovery Sequence:** `shelld` launches $\to$ reads `LogicalSessionSnapshotHeader` $\to$ validates epoch fencing proof via `receive_fencing_proof` $\to$ restores spatial surface layout $\to$ `workspaced` resumes active DAG execution.

---

## 9. Verification & Acceptance Gates

The v1.0 MVP implementation will be verified against 12 machine-verifiable acceptance gates:

| Gate ID | Gate Name | Setup / Action | Machine-Verifiable Evidence | Failure Class |
|---|---|---|---|:---:|
| **GATE-MVP-01** | Clean Compilation | Build kernel and all daemons. | Exit code 0, 0 compiler errors/warnings. | Cat A |
| **GATE-MVP-02** | Bootable ISO Image | Boot `zeroos-v1.0-x86_64.iso` in QEMU. | QEMU serial output: `"ZeroOS Nucleus Initialized"`. | Cat A |
| **GATE-MVP-03** | Workspace Restoration | Trigger `QEMU system_reset` during session. | `LogicalSessionSnapshotHeader` verified; layout restored. | Cat C |
| **GATE-MVP-04** | Intent-to-DAG Execution | Submit intent via `intentd`. | `OP_INTENT_SUBMIT_RESP` success; DAG node complete. | Cat C |
| **GATE-MVP-05** | Mid-Task Intent Pivot | Submit `OP_INTENT_CANCEL` mid-job. | DAG canceled within 120ms; resource leases freed. | Cat C |
| **GATE-MVP-06** | Surface Disconnect | Terminate `shelld` surface socket 30s. | `workspaced` DAG completes in background ($R = 0$). | Cat C |
| **GATE-MVP-07** | Spatial Grounding | Query `groundd` spatial context. | Visible window text returned; auth fields redacted. | Cat C |
| **GATE-MVP-08** | Power-Loss Recovery | Hard reset during active DAG. | Fencing proof validated $N \to N+1$; DAG resumed. | Cat C |
| **GATE-MVP-09** | Application Integration | Execute `zero-exec-lib` stdio pipe. | Intermediate data streamed via `Stage 3H` IPC; 0 `/tmp` files. | Cat B |
| **GATE-MVP-10** | 10-Min Onboarding Flow| Run integrated onboarding script. | All 4 single-node scenarios complete ($H \le 2, K = 0$). | Cat C |
| **GATE-MVP-11** | Offline Autonomy | Disconnect virtual network interface. | Local intent submission & spatial grounding succeed. | Cat C |
| **GATE-MVP-12** | Kernel Preservation | SHA-256 hash `kernel/src/stage3/`. | 100% byte-identical to Stage 3N baseline. | Cat D |

---

## 10. Product Proof Mapping

Every v1.0 work item maps directly to Product Proof scenario families and evaluation metrics:

```text
               PRODUCT PROOF EXPERIMENT MAPPING

  WORK ITEM                   PROVED BY EXPERIMENT    PRIMARY METRIC
  ------------------------    ────────────────────    ───────────────────────
  Intent-to-DAG Execution     EXP-01 (Creation)       H: 8 -> 2 (75% reduction)
  Spatial Grounding           EXP-02 (Information)    H: 7 -> 1, C: 3 -> 0
  Session Snapshot Recovery   EXP-03 (Continuity)     R: 12 -> 0, C: 4 -> 0
  Capability IPC Piping       EXP-04 (Coordination)   K: 5 -> 0 (100% reduction)
  Surface Decoupling          EXP-05 (Long-Running)   R: 8 -> 0, T = 0
  Dynamic Intent Pivot        EXP-06 (Intent Change)  R: 4 -> 0, T = 0
  Fabric Remote Offload       EXP-07 (Distributed)    DEFERRED TO v1.1+
```

---

## 11. v1.0 Non-Goals

The following features are explicitly **EXCLUDED** from v1.0 scope:

```text
❌ DO NOT implement Multi-Node Fabric Remote Compute (EXP-07 Offload) in v1.0.
❌ DO NOT write Stage 7 Architecture or Kernel Code.
❌ DO NOT create new kernel syscalls or modify kernel/src/stage3/.
❌ DO NOT introduce new Stage 3H capability types.
❌ DO NOT create custom sidecar adapter daemons.
❌ DO NOT build AI benchmark infrastructure or complex demo apps.
❌ DO NOT build administrative system monitoring dashboards.
❌ DO NOT introduce distributed consensus frameworks.
```

---

## 12. Risks and Failure Classification

If an implementation work item fails during execution, the failure MUST be categorized strictly under the 5-category taxonomy:
- **Category A (Test Setup Issue):** Test harness script timeout or QEMU configuration error.
- **Category B (Missing App Integration):** Legacy application IPC wrapper defect or stdio pipe adapter mismatch.
- **Category C (Missing User-Space Implementation):** User-space daemon logic edge case or ISO build script failure.
- **Category D (Genuine Architectural Gap):** Core OS primitive defect requiring kernel changes. *(If Category D is encountered, STOP immediately and conduct architectural review).*
- **Category E (Feature Not Valuable):** Feature operates technically but provides no user value.

---

## 13. MVP Definition of Done

> **ZeroOS Workstation Edition v1.0 MVP is COMPLETE when:**  
> All 12 Machine Acceptance Gates (`GATE-MVP-01` through `GATE-MVP-12`) pass cleanly in QEMU/bare-metal automated test harness, `zeroos-v1.0-x86_64.iso` boots cleanly, all Category B `libzero` application libraries stream data via `Stage 3H` capability pipes without intermediate disk files, Stage 3A–3N microkernel bytes remain 100% byte-identical, and PMM frame leak accounting proves 0 leaked frames.

---

## 14. Post-MVP / v1.1 Boundary

| Feature / Subsystem | Status in v1.0 | Target Milestone |
|---|:---:|:---:|
| Single-Node Intent & DAG Execution | 🟢 Included | v1.0 MVP |
| Spatial Grounding & Session Recovery | 🟢 Included | v1.0 MVP |
| Category B `libzero` App Libraries | 🟢 Included | v1.0 MVP |
| Standalone Bootable ISO Image | 🟢 Included | v1.0 MVP |
| Multi-Node Fabric Compute Offload | 🔴 Excluded | Deferred to v1.1 |
| Multi-Node Fencing Synchronization | 🔴 Excluded | Deferred to v1.1 |
| Cross-Node Resource Lease Trading | 🔴 Excluded | Deferred to v1.1 |

---

## 15. Implementation Order & Work Item Scope Table

| Work Item ID | Description | Category | Owner | Depends On | Product Proof | Verification Gate | Included in v1.0? |
|---|---|---|---|---|---|---|:---:|
| **WI-01** | Standalone ISO Packager (`build_iso.py`) | Cat C | Build Tools | `run_qemu.py` | EXP-01 | GATE-MVP-01, 02 | **YES** |
| **WI-02** | `zero-term-lib` Terminal Grounding Adapter | Cat B | `libzero` | `groundd` | EXP-02 | GATE-MVP-07, 09 | **YES** |
| **WI-03** | `zero-doc-lib` Markdown Pipe Renderer | Cat B | `libzero` | `shelld` | EXP-01 | GATE-MVP-04, 09 | **YES** |
| **WI-04** | `zero-exec-lib` Toolchain Pipe Adapter | Cat B | `libzero` | `workspaced` | EXP-04 | GATE-MVP-04, 09 | **YES** |
| **WI-05** | `shelld` Human Error UI Formatter | Cat C | `shelld` | `intentd` | EXP-06 | GATE-MVP-05, 10 | **YES** |
| **WI-06** | Single-Node 10-Min Onboarding Test Script | Cat C | Harness | WI-01 to 05 | EXP-01..06 | GATE-MVP-10, 11 | **YES** |
| **WI-07** | Ring0 Nucleus Byte Preservation Audit | Cat D | Kernel | All WI | Stage 3A–3N | GATE-MVP-12 | **YES** |
| **WI-08** | Multi-Node Fabric Compute Offloader | Cat C | `fabricd` | `workspaced` | EXP-07 | N/A | **NO (v1.1)** |

---

## 16. Final Plan Sign-Off

```text
================================================================================
               ZEROOS MVP IMPLEMENTATION PLAN REV1 SIGN-OFF

  v1.0 Product Scope:            🟢 ZEROOS WORKSTATION EDITION v1.0 (SINGLE-NODE)
  Implementation Delta:          🟢 3 LIBZERO ADAPTERS + ISO PACKAGER + UI FORMATTER
  Kernel Nucleus Preservation:   🟢 100% BYTE-IDENTICAL (0 NEW SYSCALLS / STAGE 7)
  Acceptance Gates:              🟢 12 MACHINE-VERIFIABLE GATES (GATE-MVP-01..12)
================================================================================
```

## 🟢 IMPLEMENTATION PLAN FROZEN & AUTHORIZED

The implementation plan for ZeroOS Workstation Edition v1.0 is finalized and frozen. No implementation code has been written. Execution is ready to proceed to Phase 1 (ISO packager and Category B `libzero` application libraries) when authorized.
