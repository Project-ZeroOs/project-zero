# ZEROOS OBJECT & MEMBERSHIP MODEL REV6 — SUBSTRATE VERIFICATION AUDIT

**Subsystem:** Core Product Interaction Architecture & Workspace Membership Subsystem  
**Document State:** Authoritative Substrate Audit Report — Verification of Disputed Claims  
**Author:** DeepMind Advanced Agentic Coding Team  
**Date:** October 2026  
**Status:** 🟡 AUDIT COMPLETE — TARGETED REVISION REQUIRED (REV6 UNAPPROVED)  
**Authoritative Dependencies:** `STAGE3K-ARCHITECTURE-REV5.md`, `STAGE3H-ARCHITECTURE-REV6.md`, `STAGE3I-ARCHITECTURE-REV4.md`, `STAGE4B-ARCHITECTURE-REV12.md`, `STAGE4C-IMPLEMENTATION.md`, `STAGE4D-IMPLEMENTATION.md`, `ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV6.md`

---

## 1. AUDIT SCOPE & METHODOLOGY

This document presents a rigorous, empirical verification audit of **`ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV6.md`** against the external adversarial review findings (DeepSeek Review) and the authoritative frozen repository architecture contracts.

### Audit Objectives
1. Determine which external review blockers are **genuine architectural gaps**, which are **unproven over-claims in REV6**, and which are **false positives** arising from an incomplete review packet.
2. Verify every disputed substrate claim directly against repository source specifications (`STAGE3K-ARCHITECTURE-REV5.md`, `STAGE4D-IMPLEMENTATION.md`, etc.).
3. Classify all system claims strictly under five formal status categories: `VERIFIED`, `PARTIALLY VERIFIED`, `UNPROVEN`, `ABSENT`, or `ARCHITECTURAL GAP`.
4. Provide explicit document, section, and implementation evidence for every finding without inventing new kernel primitives or modifying frozen Stage 3A–3N substrates.

---

## 2. DEEPSEEK FINDING-BY-FINDING VERIFICATION

### Finding B1-1: Global Volume `ObjectId` Registry Ownership & Atomicity
- **DeepSeek Finding**: `/storage/system/object_id.registry` is asserted but not proven to exist, be persistable, crash-atomic, or single-writer in the frozen substrate.
- **Repository Evidence**:
  - **Path Existence & Storage**: Stage 3K (`STAGE3K-ARCHITECTURE-REV5.md` Section 6.2, 7.3) defines root directory inode (Inode 1) supporting directory entry creation (`DirectoryManager::create_entry`). Directory paths such as `/storage/system/` are standard directory inodes.
  - **Persistence Ownership**: Stage 4D (`STAGE4D-IMPLEMENTATION.md` Section 2.A, 5.B) defines `workspaced` as the Ring 3 service daemon creating and managing persistent control block files (`workspace.meta` and `context.graph`) via `FileManager::alloc_storage_object` and `FileManager::write`. Creating `/storage/system/object_id.registry` reuses the exact same `workspaced` persistent file mechanism.
  - **Single-Writer Enforcement**: Stage 4A (`STAGE4A-IMPLEMENTATION.md`) `init` supervisor spawns `workspaced` as a single daemon instance registered at `"workspace.service"` in Stage 4A `brokerd` (`OP_REGISTER_SERVICE` `0x4A01`). `brokerd` rejects duplicate registrations, guaranteeing single-writer ownership.
  - **Transaction Atomicity**: Stage 3K (`STAGE3K-ARCHITECTURE-REV5.md` Section 11.2 `I-STOR-JOURNAL-1`) guarantees single 32 KiB chunk transaction atomicity. Registry append writes $\le 32\text{ KiB}$ commit atomically via Stage 3K's 6-step intent journal protocol.
  - **Reconciliation & Recovery Rule**: If `workspaced` crashes or registry state diverges from ZeroFS, Stage 4D (`STAGE4D-IMPLEMENTATION.md` Section 6.B Metadata Primacy Invariant) establishes that ZeroFS physical file existence is sovereign. `workspaced` rebuilds missing registry entries on boot by scanning ZeroFS directory trees.
- **Verdict**: **PARTIALLY VERIFIED**. Storage creation, single-writer daemon lifecycle, and ZeroFS 32 KiB journal atomicity are `VERIFIED`. Crash atomicity across multi-chunk registry updates is `BEST-EFFORT`.

---

### Finding B2-1: Physical Identity Keying vs Stat ABI Boundaries
- **DeepSeek Finding**: REV6 claimed `(Path_Hash, generation, mtime)` as a verified physical keying mechanism, but `inode_number` is not exposed in Ring 3 stat ABI.
- **Repository Evidence**:
  - **Syscall 15 `SYS_FILE_STAT` Audit**: `STAGE3K-ARCHITECTURE-REV5.md` Section 15.5 defines `UserFileStat` (32 bytes):
    `size_bytes: u64`, `blocks_count: u64`, `file_type: u32`, `generation: u32`, `mtime_ticks: u64`.
    `inode_number` is **NOT EXPOSED** in `UserFileStat` to Ring 3 userspace!
  - **Userspace Keying Capability**: Ring 3 `workspaced` opens files via `SYS_FILE_OPEN` (Syscall 11) using relative path strings, and queries stat via `SYS_FILE_STAT` (Syscall 15).
  - **Generation Invariant**: Stage 3K (`STAGE3K-ARCHITECTURE-REV5.md` Section 7.2) confirms `DiskInode.generation` (u32) increments on inode slot recycling, ensuring recycled inodes are detected.
  - **Path Change Limitation**: Because `BLAKE2s(Canonical_Path_String)` changes when a file is renamed (`hash(a.txt) != hash(b.txt)`), path hash keying CANNOT physically derive identity across renames without observing the rename operation itself.
- **Verdict**: **UNPROVEN / BEST-EFFORT**. Ring 3 userspace path-based stat query is `VERIFIED`. Raw physical inode number exposure to Ring 3 is `ABSENT` (requires expanding `UserFileStat` from 32B to 40B). `Path_Hash + generation + mtime` is correctly classified as a **Best-Effort Userspace Observation Key**, NOT a physical stat invariant.

---

### Finding B3-1: Rename & Rename-Over Substrate Authority
- **DeepSeek Finding**: REV6 assumes POSIX source-inode-wins rename-over semantics without proving Stage 3K provides a raw rename syscall.
- **Repository Evidence**:
  - **Kernel Syscall Audit**: `STAGE3K-ARCHITECTURE-REV5.md` Section 15 enumerates Syscalls 11..16 (`OPEN`, `READ`, `WRITE`, `CLOSE`, `STAT`, `SYNC`). A raw `SYS_FILE_RENAME` syscall is **NOT PROVIDED** in Syscalls 11..16.
  - **Kernel Layer 4 Directory Primitives**: `STAGE3K-ARCHITECTURE-REV5.md` Section 2.A confirms kernel Layer 4 directory manager provides `DirectoryManager::create_entry` and `DirectoryManager::remove_entry`.
  - **Ring 3 Rename Execution**: Rename operations are executed by `libzero` VFS helper routines in Ring 3 userspace by creating the target directory entry and removing the old entry via directory handles.
  - **Rename-Over Behavior**: When `libzero` replaces an existing target entry, it unlinks the target entry (setting `PENDING_DELETE` if open handles exist per `STAGE3K-ARCHITECTURE-REV5.md` Section 13.2) and links the source inode to the target path. The source inode's content survives at the target path (**Physical Source Semantics**).
- **Verdict**: **PARTIALLY VERIFIED / BEST-EFFORT**. Kernel Layer 4 directory entry insertion/removal is `VERIFIED`. Raw `rename` syscall is `ABSENT` from Syscalls 11..16. Rename-over execution via `libzero` helper sequence is `BEST-EFFORT`.

---

### Finding B4-1: Boot Epoch Persistence & Monotonic Timestamp Ordering
- **DeepSeek Finding**: `mount_count` is asserted as `BootEpochSequence` without proving it is persisted before timestamps are issued or survives crash.
- **Repository Evidence**:
  - **Superblock Persistence**: `STAGE3K-ARCHITECTURE-REV5.md` Section 7.1 defines `DiskSuperblock` (Block 1) containing `mount_count: u64` at offset `0x54..0x5C`.
  - **Mount Incrementation & Flush**: `STAGE3K-ARCHITECTURE-REV5.md` Section 11.1 & 12 state that during volume mount (`mount_volume()`), `mount_count` is incremented and written to Block 1 via Step 5 of the commit sequence, followed by an ATA `CACHE FLUSH` **prior to accepting any user file operations or issuing timestamps**.
  - **Crash Atomicity**: Superblock writes use Stage 3K 6-step intent journal protocol (`I-STOR-JOURNAL-1`). If power fails during mount, journal recovery rolls forward or re-executes superblock mount count update. `mount_count` is persistent and monotonically increasing across reboots.
  - **Ordering Formula**: `QualifiedTimestamp { boot_epoch_sequence: u64, monotonic_ticks: u64 }` evaluated as:
    $T_A < T_B \iff (T_A.\text{epoch} < T_B.\text{epoch}) \lor (T_A.\text{epoch} == T_B.\text{epoch} \land T_A.\text{ticks} < T_B.\text{ticks})$.
- **Verdict**: **VERIFIED**. `DiskSuperblock.mount_count` is persisted to Block 1 on disk prior to timestamp issuance, providing a mathematically valid cross-boot chronological ordering relation.

---

### Finding B5-1: Capability Possession vs Causal Write Attribution
- **DeepSeek Finding**: Stage 4C binds TaskId to process creation, not to individual `SYS_FILE_WRITE` syscall payloads. Capability possession does not prove write causality.
- **Repository Evidence**:
  - **Process Task Association**: Stage 4C (`STAGE4C-IMPLEMENTATION.md` Section 5) binds `workload_id`, `task_id`, and `workspace_id` to `ProcessControlBlock` during `OP_PROCESS_SPAWN`.
  - **Syscall Execution**: Stage 3K (`STAGE3K-ARCHITECTURE-REV5.md` Section 15.3) `SYS_FILE_WRITE` validates handle rights (`FILE_WRITE`).
  - **On-Disk Inode Metadata**: `STAGE3K-ARCHITECTURE-REV5.md` Section 7.2 confirms `DiskInode` records `creator_pid: u64` at offset `0x18..0x20`. Data blocks themselves do NOT store `task_id` or `workload_id` tags.
  - **Causal Lineage Boundary**: `workloadd` maintains `PID -> (WorkloadId, TaskId, WorkspaceId)` mapping in RAM. If a process forks a child or sends IPC to a shared daemon without passing a capability token, the shared daemon's write is executed under the daemon's PID, breaking direct `TaskId` write causality.
- **Verdict**: **PARTIALLY VERIFIED**. Process ownership (`PID -> WorkspaceId`) and `creator_pid` inode tagging are `VERIFIED`. Per-write byte-payload `TaskId` tagging on disk data blocks is `ABSENT`. REV6 correctly decoupled `WRITER_AUTHORITY` from `WORKLOAD_CAUSALITY`.

---

### Finding EL-1: Context EventLog Compaction Crash Atomicity
- **DeepSeek Finding**: `ContextEventRecord` compaction lacks proven crash-atomic file replacement.
- **Repository Evidence**:
  - **Write-Replace Sequence**: `STAGE4D-IMPLEMENTATION.md` Section 5.C specifies that `workspaced` writes compacted context records to a temporary file (`context.graph.tmp`), executes `SYS_FILE_SYNC` (Syscall 16), and updates the directory entry to replace `context.graph`.
  - **Journal Atomicity**: Stage 3K (`STAGE3K-ARCHITECTURE-REV5.md` Section 11.1) directory entry updates commit via 6-step journal protocol (`I-STOR-JOURNAL-1`).
  - **Crash Safety**: If power fails before directory update, `context.graph.tmp` is orphaned and original `context.graph` remains active. If power fails after commit, new `context.graph` is active.
- **Verdict**: **VERIFIED**. EventLog compaction using temporary file write-replace staging is crash-atomic via Stage 3K journal protocol.

---

### Finding 0/0/0/0: Substrate Impact Audit
- **DeepSeek Finding**: "Zero new kernel modifications" does not prove zero substrate dependencies or zero missing primitives.
- **Repository Evidence**:
  - **Kernel Code Preservation**: `kernel/src/stage3/` retains 0 modified bytes. Syscalls 1..16 remain frozen.
  - **Userspace Bring-Up Requirements**: Implementing `workspaced` registry scanning, `libzero` VFS wrappers, and path projection requires Ring 3 userspace software development.
- **Verdict**: **VERIFIED**. 0 kernel syscalls, 0 capability types, 0 daemons, and 0 kernel code changes added. Userspace bring-up dependencies are explicitly audited.

---

## 3. VERIFIED REV6 CLAIMS

The following claims in REV6 are directly proven by authoritative repository evidence:

1. **3-Way Authority Separation**: ZeroFS physical file sovereignty (`STAGE3K-ARCHITECTURE-REV5.md`), `workspaced` daemon single-instance lifecycle (`STAGE4A-IMPLEMENTATION.md`, `STAGE4D-IMPLEMENTATION.md`), and `context.graph` metadata containment (`STAGE4D-IMPLEMENTATION.md`).
2. **Boot Epoch Ordering**: `DiskSuperblock.mount_count` persistent epoch counter (`STAGE3K-ARCHITECTURE-REV5.md` Section 7.1).
3. **Membership Non-Authority**: Invariant `I-WS-MEMBERSHIP-NOT-CAPABILITY` supported by Stage 3H capability handle isolation (`STAGE3H-ARCHITECTURE-REV6.md`).
4. **Provisional Expiry to Unfiled**: Untouched provisional items lapse to `Unfiled` without auto-confirmation (`I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY`).
5. **Stage 3K Storage Scale Bounds**: $128\text{ MiB}$ volume limit, $256$ inodes, $4.07\text{ MiB}$ max file size (`STAGE3K-ARCHITECTURE-REV5.md` Section 6, 7).
6. **File-Content Rewind Removal**: ZeroFS historical data extents confirmed `ABSENT` (`STAGE3K-ARCHITECTURE-REV5.md` Section 6, 7).

---

## 4. UNPROVEN REV6 CLAIMS (REQUIRES DOWNGRADE OR RE-CLASSIFICATION)

1. **Physical Stat Keying (`I-WS-OBJECT-ID-PHYSICAL-KEY-VERIFIED`)**: `UserFileStat` in Stage 3K does NOT expose `inode_number` to Ring 3. Must be downgraded from `VERIFIED` to **`BEST-EFFORT OBSERVATION KEY`**.
2. **Crash-Atomic `ObjectId` Continuity**: `workspaced` registry updates execute in Ring 3 userspace separately from Stage 3K block transactions. Must be downgraded to **`BEST-EFFORT ON CRASH`**.
3. **Raw Syscall Rename**: `rename`, `unlink`, `symlink`, and POSIX `link` are NOT provided as raw Stage 3K syscalls in 11..16. Must be classified as **`NOT PROVIDED AS RAW SYSCALLS / LIBZERO MEDIATED`**.

---

## 5. GENUINE ARCHITECTURAL GAPS

1. **Ring 3 Inode Number Exposure**: Stage 3K `SYS_FILE_STAT` does not return `inode_number`. Zero-path physical inode binding requires a future ABI extension (`UserFileStat` expansion from 32B to 40B).
2. **Per-Instance `/tmp` Isolation**: Frozen Stage 3K substrate provides a single global `/tmp` directory. Per-workspace `/tmp` namespace virtualization requires future VFS layer work.
3. **User-Visible Trash / Undo**: Stage 3K file deletion unlinks inodes immediately (`PENDING_DELETE`). User-visible Trash with undo capability is an un-implemented userspace staging concept.

---

## 6. FALSE POSITIVES / REVIEW-PACKET LIMITATIONS

1. **"Global Volume Registry (`/storage/system/object_id.registry`) is Un-persistable"**: **FALSE POSITIVE**. Stage 4D (`STAGE4D-IMPLEMENTATION.md` Section 2.A) and Stage 3K (`STAGE3K-ARCHITECTURE-REV5.md` Section 11.2) fully prove persistent file creation and 32 KiB journal-atomic writes for `workspaced`.
2. **"`mount_count` is Not Persisted Before Timestamps"**: **FALSE POSITIVE**. Stage 3K (`STAGE3K-ARCHITECTURE-REV5.md` Section 11.1 Step 5 & Section 12) proves `mount_count` is written to Block 1 and flushed via ATA `CACHE FLUSH` during `mount_volume()` prior to accepting user operations.
3. **"EventLog Compaction Corrupts State on Crash"**: **FALSE POSITIVE**. Stage 4D (`STAGE4D-IMPLEMENTATION.md` Section 5.C) and Stage 3K 6-step journal protocol prove atomic file write-replace staging.

---

## 7. CRASH CONSISTENCY MATRIX

| Operation | ZeroFS Transaction Atomicity | `object_id.registry` Atomicity | `context.graph` Atomicity | Combined System Recovery Behavior |
|---|---|---|---|---|
| **Create File** | Atomic (Step 3 Commit) | Best-Effort (Userspace IPC) | Best-Effort (Userspace IPC) | ZeroFS file created. On reboot, `workspaced` scans directory and adds missing registry entry. |
| **Directory Rename** | Atomic (Step 3 Commit) | Best-Effort (Userspace IPC) | Best-Effort (Userspace IPC) | ZeroFS directory updated. On reboot, `workspaced` detects path change and re-keys registry entry. |
| **Rename-Over ($A \to B$)** | Atomic (Step 3 Commit) | Best-Effort (Userspace IPC) | Best-Effort (Userspace IPC) | ZeroFS updates directory to $I_A$; unlinks $I_B$. On reboot, `workspaced` maps path $B \to ID_A$ and tombstones $ID_B$. |
| **Registry Update** | N/A (32 KiB Journal TX) | Atomic (Step 3 Commit) | Separate Operation | Journal recovers registry block to committed state. |
| **Context Graph Write**| N/A (32 KiB Journal TX) | Separate Operation | Atomic (Step 3 Commit) | Journal recovers context graph block to committed state. |
| **EventLog Compaction**| Atomic (Write-Replace TX) | N/A | Atomic (Directory Swap TX) | `context.graph.tmp` orphaned on crash; original `context.graph` remains 100% active. |
| **Boot Epoch Increm.**| Atomic (Superblock TX) | N/A | N/A | `DiskSuperblock.mount_count` flushed during mount prior to timestamp issuance; never regresses. |

---

## 8. ATTRIBUTION MATRIX

| Process / Writer Scenario | Writer Authority ($C_{\text{ws}}$) | Writer Process (PID) | Workload Causality (Task DAG) | Workspace Attribution Classification |
|---|---|---|---|---|
| **Direct Workload Task** | Valid $C_{\text{ws}}$ Handle | Task PID | Direct `workloadd` TaskId | **`DETERMINISTIC`** |
| **Child Process (`execve`)** | Inherited $C_{\text{ws}}$ Handle | Child PID | Direct `workloadd` Lineage | **`DETERMINISTIC`** |
| **Delegated Process** | Explicitly Delegated $C_{\text{ws}}$ | Delegate PID | Delegated Task Lineage | **`DETERMINISTIC`** |
| **IPC Helper Process** | Lacks $C_{\text{ws}}$ Handle | Helper PID | Untracked IPC Hop | **`AMBIGUOUS`** |
| **Single-Instance App** | Holds $C_{\text{wsA}}$ (Workspace A) | App PID | Untracked IPC from Workspace B | **`DETERMINISTIC` (for Workspace A) / `PROVISIONAL` (for Workspace B)** |
| **Detached Worker** | Lacks $C_{\text{ws}}$ Handle | Worker PID | Task Terminated | **`PROVISIONAL / AMBIGUOUS`** |
| **Post-Task Worker** | Dropped $C_{\text{ws}}$ Handle | Worker PID | Task Completed | **`PROVISIONAL / AMBIGUOUS`** |
| **External Daemon** | System Process | Daemon PID | No Workload Lineage | **`SYSTEM-GENERATED / UNATTRIBUTED`** |

---

## 9. FROZEN-SUBSTRATE IMPACT AUDIT

```text
Kernel Substrate Impact:
- Stage 3A–3N Code Modifications: 0 bytes (100% Frozen)
- Syscalls Added                : 0 (Syscalls 1..16 Frozen)
- Capability Types Added       : 0 (Reuses 0x0030 & 0x0020-0x0080)
- Daemons Added                : 0 (Reuses workspaced, workloadd, brokerd)

Existing Primitives Reused:
1. Stage 3K ZeroFS Superblock mount_count (Block 1, offset 0x54) ──► Boot Epoch Ordering
2. Stage 3K 6-Step Intent Journal & 32 KiB TX Atomicity           ──► Registry & EventLog Write-Replace Atomicity
3. Stage 3H Capability Handles & C-List Attenuation               ──► Writer Authority Envelope
4. Stage 4A brokerd Service Name Registration                     ──► Single-Writer workspaced Daemon Lifecycle
5. Stage 4D workspaced Control Block & Context Graph Storage      ──► Global Registry & Workspace Graph Storage

Unproven Substrate Dependencies:
1. Userspace path-based observation keying across un-notified renames (Best-Effort).
2. Libzero VFS wrapper directory notification ordering to workspaced (Best-Effort).

Actual Architectural Gaps:
1. Un-exposed inode_number in Ring 3 UserFileStat (Syscall 15).
2. Un-supported per-instance /tmp namespace virtualization.
3. Un-supported Stage 3K ZeroFS historical data extent versioning.
```

---

## 10. FINAL AUDIT VERDICT

🟡 **REV6 PARTIALLY VERIFIED — TARGETED REVISION REQUIRED**

### Summary Verdict Rationale
REV6 successfully established the core 3-way authority model, membership/authority separation, rewind boundaries, and provisional expiry lapse. However, REV6 contains **two over-claims** that must be precisely downgraded in REV7:
1. **Downgrade Physical Identity Keying**: Re-classify `(Path_Hash, generation, mtime)` from a "verified physical key" to a **Best-Effort Userspace Observation Key**, acknowledging that `inode_number` is not exposed in `UserFileStat`.
2. **Downgrade Write Causality**: Re-classify capability possession from "deterministic causality" to **Writer Authority**, requiring BOTH capability possession AND direct workload task lineage for `DETERMINISTIC` attribution status.

```text
REVISION:
NOT CREATED

IMPLEMENTATION:
NOT STARTED

FREEZE:
NOT APPROVED

STAGE 3A–3N:
UNMODIFIED
```
