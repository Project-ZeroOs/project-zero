# STAGE 6F IMPLEMENTATION PLAN REV1

**Subsystem**: Stage 6 Final Subsystem Closure, Agent Spatial Grounding & Session Continuity (`groundd`)  
**Document State**: Implementation Plan Rev1 — Authorized Draft  
**Author**: DeepMind Advanced Agentic Coding Team  
**Date**: October 2026  

---

## 1. Executive Summary & Plan Mission

Stage 6F defines the final subsystem implementation of Stage 6 in ZeroOS (**Human Interaction, Spatial Presentation & Agent-Human Collaboration Subsystem**).

The purpose of this plan is to outline the step-by-step implementation strategy for:
1. **`groundd` (Spatial Grounding Query Broker)**: An unprivileged, read-only daemon that indexes spatial window coordinates from `shelld`/`compositord` and accessibility UI element metadata from `surfaced`.
2. **Multi-Node Session Continuity & Fail-Safe Epoch Fencing**: A zero-handle logical snapshot handoff protocol governed by monotonic `SessionMigrationEpoch` fencing (`I-6F-SESSION-EPOCH-FENCING`) and 0-or-1 single-authority safety (`I-6F-SINGLE-AUTHORITY-SAFETY`).

### Strict Architectural Boundaries
- **0 Bytes Kernel Nucleus Modification**: Files under `kernel/src/stage3/` remain 100% untouched.
- **0 New Capability Types**: All authorization relies strictly on pre-existing Stage 3H `WorkspaceAccessCap` (`0x0030`).
- **0 Direct Capability-Table Inspections**: `groundd` receives kernel-validated workspace identity context via Stage 3G/3H capability-bearing IPC (`sys_ipc_send`).
- **0 Ephemeral Handle Migrations**: Logical snapshots contain only logical state (0 kernel handles, 0 PIDs, 0 IPC descriptors, 0 SHM handles).
- **0 Un-Fenced Remote Commits**: Destination nodes commit Epoch $N+1$ strictly post-receipt of authenticated `FencingProofDescriptor` proof.

---

## 2. Implementation Phase Breakdown

### Phase 1: ABI Data Structures & Constants (`libzero`)

#### Objectives
1. Create `libzero/src/grounding.rs` and export in `libzero/src/lib.rs`.
2. Define `SpatialNodeQuery` (64 bytes) and `SpatialNodeDescriptor` (128 bytes) with explicit compile-time `size_of`, `align_of`, and `offset_of` assertions (`Gate 6F-1`).
3. Define `LogicalSessionSnapshotHeader` (64 bytes) and `FencingProofDescriptor` (64 bytes) with explicit layout assertions.
4. Register Stage 6F IPC opcodes (`0x0820`–`0x0823`, `0x0611`, `0x0612`) and error codes (`ERR_STALE_SPATIAL_INDEX` `0x0730`, `ERR_STALE_SESSION_EPOCH` `0x0731`).

#### Tasks & Artifacts
- **Create**: `libzero/src/grounding.rs`
- **Modify**: `libzero/src/lib.rs`, `libzero/src/error.rs`
- **Verification**: `cargo check -p libzero`

---

### Phase 2: `groundd` Spatial Indexing Daemon (`groundd/`)

#### Objectives
1. Create `groundd/Cargo.toml` and `groundd/src/main.rs`.
2. Implement preallocated spatial node index ring buffer (fixed capacity, zero dynamic heap allocations).
3. Implement `OP_GROUND_QUERY_SPATIAL` (`0x0820`) handler:
   - Extract kernel-validated `WorkspaceAccessCap` (`0x0030`) from descriptor context (`req.handles[0]`).
   - Validate expected layout and semantic generation counters against current authoritative indices (`ERR_STALE_SPATIAL_INDEX` `0x0730`).
   - Enforce 4-tier privacy classification and geometric redaction (Tier 2 `FLAG_SENSITIVE` $\to$ `BOUNDS_REDACTED` `[0,0,0,0]`, `NODE_REDACTED` `0xFF`; Tier 3 Stage 5 `authui` overlays $\to$ hidden).
4. Implement read-only enforcement: reject all write/mutation IPC opcodes with `InvalidRequest` (`0x02`).

#### Tasks & Artifacts
- **Create**: `groundd/Cargo.toml`, `groundd/src/main.rs`
- **Verification**: `cargo check -p groundd`

---

### Phase 3: Multi-Node Session Continuity & Epoch Fencing (`shelld` / `surfaced` / `fabricd`)

#### Objectives
1. Update `shelld` to own `SurfaceLayoutGeneration` and `surfaced` to own `SurfaceSemanticGeneration` (`I-6F-GENERATION-OWNERSHIP`).
2. Implement IPC mutation boundary fencing check in `shelld`/`surfaced`: verify local state is `STATE_AUTHORITATIVE` and request epoch matches active `committed_epoch` (`I-6F-MUTATION-FENCE`).
3. Implement `LogicalSessionSnapshot` serialization in `shelld` (bounded by 16 WS / 256 surfaces / 64 KB payload).
4. Implement fail-safe handoff state machine:
   - Node B issues single-use `dest_nonce` (`NONCE_ISSUED`).
   - Node A advances epoch to $N+1$, durably persists `STATE_FENCED_PENDING_COMMIT(N)`, emits `FencingProofDescriptor` payload (`FencingProofDescriptor` + Stage 4F node identity signature).
   - Node B validates fencing proof, writes `committed_epoch = N+1` (`STATE_AUTHORITATIVE`), and triggers Stage 4A `brokerd` $\to$ `sys_cap_derive` to issue local `WorkspaceAccessCap` (`0x0030`).
5. Implement crash recovery protocol: reboot in `STATE_FENCED_PENDING_COMMIT` initializes as `STATE_RECOVERY_UNCERTAIN` and remains fenced until commit or abort is proven (`I-6F-SINGLE-AUTHORITY-SAFETY`).

#### Tasks & Artifacts
- **Modify**: `shelld/src/main.rs`, `surfaced/src/main.rs`, `intentd/src/main.rs`
- **Verification**: `cargo check --workspace`

---

### Phase 4: Verification Harness & Acceptance Gates (`kernel` / `tests`)

#### Objectives
1. Implement `groundd_helper` and `run_stage6f_verification` in `kernel/src/stage4/tests.rs`.
2. Add `run_stage6f_verification` call in `kernel/src/stage4/mod.rs` and `kernel/src/lib.rs`.
3. Implement Python test suite `tests/test_stage6f.py` verifying gates `6F-1` through `6F-16`.

#### Tasks & Artifacts
- **Modify**: `kernel/src/stage4/tests.rs`, `kernel/src/stage4/mod.rs`, `kernel/src/lib.rs`
- **Create**: `tests/test_stage6f.py`
- **Verification**: `python -m unittest tests/test_stage6f.py`

---

## 3. Comprehensive Machine Acceptance Gates (`6F-1` through `6F-16`)

```text
6F-1   Spatial Query ABI Alignment                    (SpatialNodeQuery 64B & SpatialNodeDescriptor 128B size/align/offset assertions)
6F-2   Spatial Element Grounding                      (groundd returns valid bounding boxes for Tier 1 elements)
6F-3   Sensitive Element Geometric Masking            (FLAG_SENSITIVE returns BOUNDS_REDACTED [0,0,0,0] & NODE_REDACTED 0xFF)
6F-4   Trusted Overlay Exclusion                      (Stage 5 authui overlays return 0 elements to groundd queries)
6F-5   Workspace Containment Isolation                (Queries lacking matching WorkspaceAccessCap 0x0030 return PermissionDenied)
6F-6   Absence of Mutation Authority Audit            (groundd possesses 0 write/mutation IPC opcodes or syscall paths)
6F-7   Bounded Logical Session Snapshot Serialization (LogicalSessionSnapshot bounded by 16 WS / 256 Surfaces / 64 KB)
6F-8   Ephemeral Handle Non-Migration Audit          (0 raw kernel handles, PIDs, or SHM pointers in export payloads)
6F-9   Cryptographic Fencing Proof & Anti-Replay      (Destination commits Epoch N+1 strictly post-FencingProofDescriptor validation)
6F-10  Post-Fencing Remote Capability Derivation      (Local WorkspaceAccessCap 0x0030 derived strictly post-commitment via brokerd)
6F-11  Offline Autonomy Verification                  (Local spatial queries execute 100% offline without network)
6F-12  groundd Crash Non-Impact Recovery             (groundd crash removes observation availability without modifying workspace/capability state)
6F-13  Zero New Capability Authority Audit            (0 new Stage 3H capability object types or rights bits)
6F-14  Physical Memory Manager (PMM) Neutrality       (Baseline free frame count == Final free frame count)
6F-15  Stage 3A–3N Kernel Preservation                (kernel/src/stage3/ retains 0 bytes modified)
6F-16  Spatial & Semantic Freshness Verification      (Queries with stale layout/semantic generation return ERR_STALE_SPATIAL_INDEX 0x0730)
```

---

## 4. Execution Workflow & Next Steps

Upon review and approval of this plan by the user:
1. Implement Phase 1 (`libzero/src/grounding.rs`).
2. Implement Phase 2 (`groundd/src/main.rs`).
3. Implement Phase 3 (`shelld`, `surfaced`, `intentd` updates).
4. Implement Phase 4 (`kernel/src/stage4/tests.rs` & `tests/test_stage6f.py`).
5. Execute full system regression suite and verify QEMU/ISA exit, PMM neutrality, and 0 kernel bytes modified.
6. Present Stage 6F final completion report without starting Stage 7 automatically.

---
