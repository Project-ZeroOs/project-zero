# ZEROOS OBJECT & MEMBERSHIP MODEL — REV7

**Subsystem:** Core Product Interaction Architecture & Workspace Membership Subsystem  
**Document State:** Architectural Specification Rev7 — Final Substrate-Reconciled Architecture  
**Author:** DeepMind Advanced Agentic Coding Team  
**Date:** October 2026  
**Status:** 🟡 ARCHITECTURAL SPECIFICATION — PENDING ADVERSARIAL REVIEW  
**Authoritative Dependencies:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), Stage 5–6 (Frozen), WI-09/10/02/03/04 (Committed)

---

## 1. EXECUTIVE SUMMARY & REV7 REVISION BASIS

Following an empirical substrate verification audit ([`ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV6-SUBSTRATE-AUDIT.md`](file:///c:/Users/vaish\.gemini\antigravity-ide\scratch\project-zero\docs\design\ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV6-SUBSTRATE-AUDIT.md)), ZeroOS received a **TARGETED REVISION MANDATE** to produce **REV7**.

REV7 is a **surgical reconciliation** that establishes precise, evidence-backed contracts without altering the core 3-way authority model, membership semantics, or frozen microkernel substrate:

1. **B1 — Global Registry Bounded Atomicity**: Single-writer `workspaced` registry persistence (`/storage/system/object_id.registry`) is verified for single 32 KiB chunk transaction atomicity (`I-STOR-JOURNAL-1`). Multi-chunk registry updates are explicitly specified as **`BEST-EFFORT`** on system crash; ZeroFS physical file state remains sovereign, and `workspaced` reconciles the registry on boot using canonical ZeroFS directory scanning.
2. **B2 — Observation Key & Continuity Boundaries**: Reclassifies Ring 3 path keying (`Path_Hash + generation + mtime`) as a **Best-Effort Userspace Observation Key** (`I-WS-OBJECT-ID-USERSPACE-OBSERVATION-KEY`). Documents explicit collision ($2^{-256}$ probability) and continuity-loss cases (un-observed renames outside `libzero`).
3. **B3 — Rename & Directory Mutation Mechanics**: Audited Syscalls 11..16: raw `rename` syscall is **NOT PROVIDED** in Stage 3K Syscalls 11..16. Directory mutations (`create_entry`, `remove_entry`) execute in kernel Layer 4, while rename-over is mediated by `libzero` Ring 3 VFS helpers (**Physical Source Semantics**). **Rename identity continuity is an observed userspace property, NOT a frozen physical filesystem guarantee.**
4. **B5 — Causal Attribution Boundaries**: Decouples **Writer Authority** (process capability $C_{\text{ws}}$), **Writer Process** (`DiskInode.creator_pid`), and **Workload Causality** (Stage 4C task DAG trigger). Eliminates any claim of per-write `TaskId` data block tagging. Deterministic attribution requires BOTH valid writer authority AND direct task DAG execution without untracked IPC hops.
5. **Closed Substrate Items**: Re-affirms closed items: Stage 3K `DiskSuperblock.mount_count` Block 1 persistence for monotonic cross-boot epoch ordering (🟢 **CLOSED**), crash-atomic EventLog write-replace compaction (🟢 **CLOSED**), membership/authority separation (🟢 **CLOSED**), and removal of historical file-content rewind (🟢 **CLOSED**).

---

## 2. REV7 RESPONSE TO SUBSTRATE AUDIT FINDINGS

| Audit Domain | REV6 Status | REV7 Reconciled Contract | Substrate Status |
|---|---|---|---|
| **B1: Global Registry** | Claimed universal atomicity | Single 32 KiB chunk journal atomicity verified. Multi-chunk updates are **BEST-EFFORT ON CRASH**. ZeroFS physical file existence is sovereign. | `VERIFIED / BEST-EFFORT` |
| **B2: Identity Key** | Claimed physical stat keying | Reclassified as **Best-Effort Userspace Observation Key**. `ObjectId` continuity across rename is an observed operation-history property. | `BEST-EFFORT` |
| **B3: Rename Mechanics** | Claimed raw rename syscall | Syscalls 11..16 audited: raw `rename` syscall is **NOT PROVIDED**. Rename mediated by `libzero` Ring 3 helpers (**Physical Source Semantics**). | `BEST-EFFORT` |
| **B4: Boot Epoch** | Disputed persistence | Proved mathematically: `DiskSuperblock.mount_count` (Block 1) is persisted and flushed prior to issuing timestamps or file operations. | `VERIFIED` (Stage 3K Block 1) |
| **B5: Write Causality** | Equated $C_{\text{ws}}$ with write causality | Decoupled `WRITER_AUTHORITY` ($C_{\text{ws}}$), `WRITER_PROCESS` (`creator_pid`), and `WORKLOAD_CAUSALITY` (task DAG). Removed per-write `TaskId` data block tagging claim. | `VERIFIED` (Stage 3H/4C) |
| **S3: EventLog Compaction**| Disputed crash safety | Proved crash-atomic compaction using `/workspaces/<id>/context.graph.tmp` write-replace staging and ZeroFS 6-step intent journal protocol. | `VERIFIED` (Stage 3K/4D) |

---

## 3. THREE-WAY SYSTEM AUTHORITY BOUNDARIES

ZeroOS enforces a strict 3-way system authority architecture:

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                          THREE-WAY AUTHORITY PROTOCOL                           │
├──────────────────────────────────┬──────────────────────────────┬───────────────┤
│          ZeroFS Storage          │ Global Volume Registry       │ Workspace     │
│       (Stage 3K Kernel)          │ (workspaced /system/)        │ Context Graph │
├──────────────────────────────────┼──────────────────────────────┼───────────────┤
│ Authoritative for:               │ Authoritative for:           │ Authoritative │
│ - Physical file existence        │ - Logical ObjectId           │ ONLY for:     │
│ - Physical block allocation      │   assignment & resolution    │ - Workspace   │
│ - Canonical POSIX paths          │ - Inode-to-ObjectId mapping  │   membership  │
│ - Raw inode generation & size    │ - Volume-wide identity index │   edges       │
└──────────────────────────────────┴──────────────────────────────┴───────────────┘
```

### 3.1 Sovereign Recovery Hierarchy
1. **ZeroFS Physical Sovereignty**: ZeroFS physical block storage is the sovereign source of truth for physical file existence. If ZeroFS shows a file was unlinked or deleted, no registry or context graph record can claim physical existence.
2. **Global Registry Logical Sovereignty**: `/storage/system/object_id.registry` (owned authoritatively by `workspaced`) is sovereign for assigning 128-bit `ObjectId`s. Workspace graphs store `MEMBER_OF` metadata edges referencing `ObjectId`s, preventing multi-workspace identity duplication.
3. **Crash Recovery Rule**: If an un-graceful system crash occurs while `workspaced` is committing a multi-chunk registry update, ZeroFS physical file state remains sovereign. On boot, `workspaced` scans ZeroFS directory trees and reconciles `/storage/system/object_id.registry` to match physical reality (**BEST-EFFORT ON CRASH**).

---

## 4. OBJECT ID IDENTITY & USERSPACE OBSERVATION KEY (RESOLVING B2)

$$\mathbf{Invariant\ I-WS-OBJECT-ID-USERSPACE-OBSERVATION-KEY:}$$
$$\text{Ring 3 userspace identity resolution } (\text{BLAKE2s}(\text{Path}), \text{generation}, \text{mtime\_ticks}) \text{ is a BEST-EFFORT OBSERVATION KEY.}$$
$$\text{ObjectId continuity across file rename is an observed operation-history property,}$$
$$\text{NOT a physically derivable identity from the frozen Stage 3K stat ABI (which omits inode numbers).}$$

```text
ZeroFS Physical File (/storage/workspaces/ws_a/data.csv)
  ├── DiskInode.generation: u32 (Recycling counter, Stage 3K Section 7.2)
  └── UserFileStat.mtime_ticks: u64 (Modification time, Syscall 15)
             ▲
             │ (Best-Effort Observation Key: Path_Hash + generation + mtime)
workspaced Global Volume Registry (/storage/system/object_id.registry)
  └── Maps Observation Key ──► ObjectId (128-bit DistributedId)
```

### 4.1 Collision & Continuity-Loss Boundary Analysis
- **Hash Collisions**: `BLAKE2s` output collision probability is $2^{-256}$ (practically zero).
- **Un-Observed Renames**: If a file is renamed outside `libzero` observation without IPC notification, `workspaced` detects the missing path on boot scanning, tombstones the old key, and registers the new path as a new `ObjectId` (or reconciles identity via path-history heuristics). `ObjectId` continuity across un-observed renames is **BEST-EFFORT**.

### 4.2 Rename-Over Semantics (Physical Source Semantics)
When `rename(src, target)` replaces an existing `target` file ($A \to B$ where $B$ exists):

```text
Before Rename:
  Source File A (/foo) ──► ObjectId_A
  Target File B (/bar) ──► ObjectId_B

Execution (Physical Source Semantics):
1. ZeroFS unlinks Target File B's inode; updates directory entry to point to Source File A's inode.
2. workspaced updates registry: Target Path /bar is now mapped to ObjectId_A.
3. ObjectId_B is tombstoned in object_id.registry and workspace context graphs.
4. RunRecord logs Event: "ObjectId_B superseded by ObjectId_A via rename-over".
```

Result: The surviving file content at `/bar` retains $ID_A$. Target $ID_B$ is cleanly tombstoned without semantic contradiction.

---

## 5. DIRECTORY MUTATION & LIBZERO RENAME MECHANICS (RESOLVING B3)

A direct audit of Syscalls 11..16 in [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish\.gemini\antigravity-ide\scratch\project-zero\docs\design\STAGE3K-ARCHITECTURE-REV5.md) confirms the kernel syscall boundary:

```text
Frozen Stage 3K Kernel Syscalls (Syscalls 11..16):
- SYS_FILE_OPEN (11)  : Open/create file via Directory Capability
- SYS_FILE_READ (12)  : Read bytes from open StorageObject handle
- SYS_FILE_WRITE (13) : Write bytes to open StorageObject handle
- SYS_FILE_CLOSE (14) : Close StorageObject handle
- SYS_FILE_STAT (15)  : Query file stat (size, blocks, type, generation, mtime)
- SYS_FILE_SYNC (16)  : Flush dirty buffers to block device storage
```

### 5.1 Substrate Operations & Libzero Helpers
- **Kernel Directory Primitives**: Kernel Layer 4 provides `DirectoryManager::create_entry` and `DirectoryManager::remove_entry`.
- **Raw Syscall Status**: Raw `rename`, `unlink`, `symlink`, and POSIX `link` syscalls are **NOT PROVIDED** in Stage 3K Syscalls 11..16.
- **Libzero Rename Execution**: Rename operations execute in Ring 3 via `libzero` VFS helper routines by creating the target directory entry and removing the old entry via directory capability handles.
- **Notification Mechanic**: `workspaced` receives rename notifications via `libzero` IPC calls or background directory scanning (**BEST-EFFORT**).

---

## 6. WRITER AUTHORITY, PROCESS LINEAGE, AND CAUSAL ATTRIBUTION (RESOLVING B5)

REV7 explicitly decouples process capability authority, physical writer process identity, and causal workload triggers:

```text
1. WRITER_AUTHORITY   ──► Process holds Workspace Capability Handle C_ws (Security Envelope)
2. WRITER_PROCESS     ──► PID executing physical SYS_FILE_WRITE (Recorded in DiskInode.creator_pid)
3. WORKLOAD_CAUSALITY ──► Specific Stage 4C WorkloadId/TaskId executing task DAG
4. WORKSPACE_ATTR     ──► Final workspace assignment (DETERMINISTIC, PROVISIONAL, AMBIGUOUS)
```

$$\mathbf{Invariant\ I-WS-WRITE-ATTRIBUTION-DETERMINISTIC:}$$
$$\text{Workspace Attribution is DETERMINISTIC iff:}$$
$$\text{1. WRITER\_AUTHORITY is valid } (C_{\text{ws}} \text{ handle held by writing process}), \mathbf{AND}$$
$$\text{2. WORKLOAD\_CAUSALITY is directly proven } (\text{Task DAG spawned by } \texttt{workloadd} \text{ without untracked IPC hops}), \mathbf{AND}$$
$$\text{3. Target path resides inside } \text{/storage/workspaces/<ws\_id>/} \text{ or explicit } \texttt{libzero} \text{ stream.}$$

```text
Attribution Classification Hierarchy:
- Process holds C_ws + direct workloadd TaskId + writes in /storage/workspaces/<id>/ ──► DETERMINISTIC
- Process steered by HOME/CWD + writes inside workspace home path                     ──► PROVISIONAL
- IPC handoff to single-instance app / detached worker lacking C_ws                   ──► AMBIGUOUS
- Global system log writes (/var/log/) / temporary lock files (/tmp/lock)              ──► SYSTEM-GENERATED
```

---

## 7. SINGLE-INSTANCE APPLICATIONS & IPC HANDOFF

When `Workspace B` triggers an action that sends an IPC message to an existing single-instance application running in `Workspace A`:

```text
Workspace B (User Action: Open document in Workspace B)
       │
       ▼ IPC Message (Carries Workspace B Context Descriptor)
Single-Instance Application Process (Running in Workspace A under C_wsA)
       │
       ▼ Executing Process writes output file out.dat
```

### 7.1 Single-Instance Attribution Rules
1. **`WRITER_AUTHORITY` Governs**: Physical write is executed by PID holding $C_{\text{wsA}}$. ZeroOS attributes the physical write to **Workspace A** (`DETERMINISTIC` for Workspace A).
2. **`WORKLOAD_CAUSALITY` Broken for Workspace B**: Because the write traversed an untracked IPC channel into an existing process in Workspace A, Workspace B lacks direct `WORKLOAD_CAUSALITY`.
3. **Workspace B Reference**: Workspace B receives a `Provisional` reference edge pointing to `out.dat`. ZeroOS NEVER transfers capability handles across IPC channels.

---

## 8. COLLAPSED STRUCTURAL MODEL & MEMBERSHIP SEMANTICS

REV7 maintains the collapsed structural model consisting of exactly **3 primitive concepts**:

```text
1. MEMBER_OF (Workspace Context Edge)
   - Metadata link connecting WorkspaceId to ObjectId
   - Attributes: Role (Owner / Reference), State (Confirmed / Provisional)

2. RunRecord (Workload & Task Execution Lineage)
   - Durable record of Stage 4C Workload DAG execution
   - Captures: WorkloadId, TaskId, Input ObjectIds, Output ObjectIds, Operator PID, Timestamp

3. EventLog (Append-Only Workspace Audit Log)
   - Sequential log of context graph mutations stored in /workspaces/<id>/context.graph
   - Captures: SequenceNo, QualifiedTimestamp, EventType (NodeAdd, NodeRemove, EdgeAdd)
```

$$\mathbf{Invariant\ I-WS-MEMBERSHIP-NOT-CAPABILITY:}$$
$$\text{A workspace membership edge } \text{MEMBER_OF}(W_A, O_X) \text{ is strictly contextual metadata.}$$
$$\text{Membership edges MUST NOT grant capability authority } (C_{\text{ws}}, C_{\text{file}}) \text{ or alter canonical filesystem paths.}$$

$$\mathbf{Invariant\ I-WS-PATH-TREE-FIRST-CLASS:}$$
$$\text{ZeroFS canonical POSIX paths } (/storage/workspaces/<id>/...) \text{ remain the single first-class source of truth.}$$
$$\text{Adding or removing a } \text{MEMBER_OF} \text{ edge MUST NOT move or rename physical files on disk.}$$

---

## 9. PROVISIONAL MEMBERSHIP & EVIDENCE HIERARCHY

$$\mathbf{Invariant\ I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY:}$$
$$\text{Provisional attribution MUST NOT silently convert heuristic inference into confirmed membership.}$$

```text
Provisional Item Created (Attributed via Advisory Steering)
         │
         ├── User Interaction Event:
         │     ├── Open / Read File ──────────────► OBSERVED INTERACTION (Remains Provisional)
         │     ├── Edit / Save File ──────────────► CONFIRMED MEMBERSHIP
         │     ├── Pin / Drag into Workspace ─────► CONFIRMED MEMBERSHIP
         │     └── Explicit Reassign / Detach ────► REASSIGNED / DETACHED
         │
         └── 24 Hours Monotonic Expiry (No user edit/save/pin) ──► Lapses to UNFILED
```

### 9.1 Interaction Evidence Classification
- **`Open / Read`**: Classified as `OBSERVED INTERACTION`. Does NOT convey explicit intent to bind membership; item remains `Provisional`.
- **`Edit / Save / Pin / Drag`**: Conveys explicit human intent; advances state to **`CONFIRMED MEMBERSHIP`**.
- **Untouched Expiry (24h)**: Lapses to `Unfiled`. Heuristic guesses are NEVER auto-confirmed without explicit user evidence.

---

## 10. TRASH, RETENTION, AND RECLAMATION

```text
Operation                 Target Entity                Storage Action                        Reversibility
─────────────────────────────────────────────────────────────────────────────────────────────────────────────
Remove Membership         ContextNode metadata edge     Deletes MEMBER_OF edge in workspaced   Reversible (Re-add edge)
Delete Object             ZeroFS Inode                  Unlinks directory entry via ZeroFS     IRREVERSIBLE (Data lost)
Open-Handle Unlink        Open StorageObject            Sets PENDING_DELETE on inode           Pending handle close
Physical Reclamation      Block Allocation Bitmap       Frees data blocks when handles = 0    Permanent block reuse
```

### 10.1 Substrate Limitation Notice
- **Irreversible Deletion**: Calling file delete unlinks the ZeroFS inode immediately. **File content deletion in ZeroFS is IRREVERSIBLE**.
- User-visible Trash (staging directory layer) is classified as **`FUTURE WORK / UNPROVEN`**.

---

## 11. PATH PROJECTION CONCEPTUAL CONTRACT

For an unmodified CLI binary running in `Workspace B` needing to read a referenced file residing physically in `Workspace A`:

```text
Workspace B Process Context
       │
       ▼ Open projected path
/workspaces/ws_b/.proj/<obj_id>/data.csv  ──(Read-Only VFS Projection)──► /storage/workspaces/ws_a/data.csv
```

### 11.1 Substrate Status
- **Read-Only Access**: Projected paths allow read-only access (`FILE_READ`). Writes return `-EACCES`.
- **Substrate Status**: Path projection is NOT implemented in Stage 3K kernel substrate. Classified as **`FUTURE WORK / UNPROVEN`**.

---

## 12. CHRONOLOGICAL TIME & BOOT EPOCH ORDERING PROOF (RESOLVING B4)

To establish a mathematically verifiable chronological ordering relation across cold reboots:

```rust
#[repr(C)]
pub struct QualifiedTimestamp {
    pub boot_epoch_sequence: u64, // 8 bytes: Monotonic boot epoch counter (DiskSuperblock.mount_count)
    pub monotonic_ticks: u64,     // 8 bytes: Monotonic ticks elapsed within current boot epoch
}
```

### 12.1 Boot Epoch Persistence & Ordering Proof
1. **Superblock Persistence**: Stage 3K `DiskSuperblock.mount_count` (stored in Block 1 at offset `0x54..0x5C`) is incremented and flushed to disk during Layer 3 `mount_volume()` **prior to issuing any timestamps or accepting filesystem operations**.
2. **Crash Atomicity**: Superblock update uses the 6-step journal protocol (`I-STOR-JOURNAL-1`). If power fails during mount, journal recovery rolls forward or re-executes mount count increment. `mount_count` NEVER regresses.
3. **Ordering Relation**:

$$\mathbf{Invariant\ I-WS-BOOT-TIME-ORDERING-VALID:}$$
$$\text{Chronological ordering between two timestamps } T_A \text{ and } T_B \text{ is authoritatively evaluated as:}$$
$$T_A < T_B \iff (T_A.\text{boot\_epoch\_sequence} < T_B.\text{boot\_epoch\_sequence}) \lor (T_A.\text{boot\_epoch\_sequence} == T_B.\text{boot\_epoch\_sequence} \land T_A.\text{monotonic\_ticks} < T_B.\text{monotonic\_ticks})$$

$$\text{Substrate Classification } \implies \mathbf{VERIFIED\ FOR\ ORDERING\ VIA\ STAGE\ 3K\ SUPERBLOCK\ MOUNT\_COUNT.}$$

---

## 13. RESUMPTION CONTRACT (BEST-EFFORT L1)

```text
Resumption Level              ZeroOS Status                 Contract & Mechanism
─────────────────────────────────────────────────────────────────────────────────────────────────────────
L0: Context Resumption        🟢 PERSISTED / RECONSTRUCTABLE Restores workspaced context graph, active focus,
                                                            spatial viewports, and action timeline.
L1: Relaunch Resumption       🟡 BEST-EFFORT                Relaunches app processes with allowlisted environment
                                                            subset (HOME, CWD, PATH). May fail if binary missing.
L2: App-Native Self-Restore   🟢 CONDITIONAL               Passes file tokens to apps supporting document restore.
L3: Process Checkpointing     ❌ EXCLUDED                   Arbitrary RAM/CPU register process checkpointing EXCLUDED.
```

---

## 14. EVENT LOG SEMANTICS & COMPACTION CRASH ATOMICITY (RESOLVING S3)

$$\mathbf{Invariant\ I-WS-EVENTLOG-NOT-FILE-VERSIONING:}$$
$$\text{The } \texttt{workspaced} \text{ Context EventLog is an append-only audit log of metadata graph mutations.}$$
$$\text{EventLog MUST NOT be equated with historical file-content versioning or filesystem data rewind.}$$

```rust
#[repr(C)]
pub struct ContextEventRecord {
    pub sequence_no: u64,             // Monotonic event sequence number within workspace
    pub timestamp: QualifiedTimestamp,// Qualified boot epoch timestamp
    pub event_type: u16,             // 1=NodeAdded, 2=NodeRemoved, 3=EdgeAdded, 4=ProvisionalLapsed
    pub target_object_id: DistributedId,// Target ObjectId
    pub workspace_id: DistributedId, // Workspace context
}
```

### 14.1 Compaction Protocol & Crash Atomicity
1. `workspaced` writes new compacted event log to `/workspaces/<id>/context.graph.tmp`.
2. Issues `SYS_FILE_SYNC` to flush data blocks to block device storage.
3. Replaces `/workspaces/<id>/context.graph` via ZeroFS directory entry update.
4. If power fails mid-compaction, `/workspaces/<id>/context.graph` remains 100% intact (**Crash-Atomic Compaction**).

---

## 15. REWIND BOUNDARIES & NON-REVERSIBLE SIDE EFFECTS

$$\mathbf{Invariant\ I-WS-FILE-CONTENT-REWIND-UNSUPPORTED-IN-STAGE3K:}$$
$$\text{Stage 3K ZeroFS lacks historical data extents. File-content rewind is REMOVED from the contract.}$$

$$\mathbf{Invariant\ I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE:}$$
$$\text{External side-effects (HTTP POSTs, emails sent, Git pushes) CANNOT be undone by context rewind.}$$

```text
Rewind Layer               Supported Mechanism                Substrate Guarantee
─────────────────────────────────────────────────────────────────────────────────────────────────────────
1. Membership / Context    workspaced context graph revert    100% Deterministic (Context log revert)
2. Spatial Layout          shelld / surfaced viewports        100% Deterministic (Viewport log revert)
3. Workload Rerun          workloadd task DAG re-execution    Supported for deterministic task DAGs
4. File Content Rewind     ZeroFS historical data extents     ❌ UNSUPPORTED (Stage 3K substrate lacks 
                                                                 historical data extent storage)
5. External Side-Effects   Remote network operations          ❌ NON-REVERSIBLE (Flagged in UI)
```

---

## 16. CURRENT FROZEN-SUBSTRATE IMPACT & LIMITATIONS AUDIT

We audit all architectural requirements against the authoritative frozen kernel contracts:

| System Requirement | Required Substrate Primitive | Authoritative Source Evidence | Substrate Status |
|---|---|---|---|
| **Writer Authority** | Kernel Process Capability Table | `kernel/src/cap/types.rs`, Stage 3H | `VERIFIED` |
| **Path Open** | `SYS_FILE_OPEN` (Syscall 11) | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `VERIFIED` |
| **Boot Epoch Persistence** | `DiskSuperblock.mount_count` | `STAGE3K-ARCHITECTURE-REV5.md` Section 7.1 | `VERIFIED` |
| **Global ObjectId Registry**| Userspace `workspaced` Registry | `/storage/system/object_id.registry` | `VERIFIED` (`workspaced`) |
| **Single-Chunk Registry TX** | Stage 3K 6-Step Intent Journal | `STAGE3K-ARCHITECTURE-REV5.md` Section 11.2 | `VERIFIED` |
| **Multi-Chunk Registry TX** | Multi-Chunk Write-Replace Staging | `/storage/system/object_id.registry` | **`BEST-EFFORT`** (ZeroFS physical state sovereign) |
| **Userspace Observation Key**| Path Hash + `generation` + `mtime` | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `BEST-EFFORT` |
| **Raw Inode Num Exposure** | Syscall Stat Expansion | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | **`UNPROVEN / FUTURE WORK`** |
| **Atomic Save Identity** | Userspace `workspaced` Re-keying | `/storage/system/object_id.registry` | **`BEST-EFFORT`** (Non-atomic with disk commits) |
| **Path Projection** | Read-Only Virtual VFS Directory | `STAGE3K-ARCHITECTURE-REV5.md` Section 14 | **`FUTURE WORK`** |
| **User-Visible Trash** | Staging Directory Layer | `STAGE3K-ARCHITECTURE-REV5.md` Section 19 | **`FUTURE WORK`** |
| **Historical Data Versions**| ZeroFS Data Extent Versioning | `STAGE3K-ARCHITECTURE-REV5.md` Section 6, 7 | **`ABSENT`** (Removed from MVP contract) |
| **Per-Instance `/tmp`** | VFS Namespace Virtualization | `STAGE3K-ARCHITECTURE-REV5.md` Section 4 | **`ABSENT`** (Marked Future Work) |
| **Browser Syscall Surface**| Microkernel Userspace Execution | Stage 3I, Stage 3J | **`UNPROVEN`** (MVP Prerequisite) |

```text
Current Frozen-Substrate Impact:
- Kernel Syscalls Added         : 0
- New Capability Types Added    : 0
- New Daemons Required          : 0
- Stage 3A–3N Code Modifications: 0
```

---

## 17. STAGE 3K STORAGE SCALE vs WORKSTATION EDITION SCALE

### 17.1 Stage 3K Bring-Up Storage Bounds
Direct audit of [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish\.gemini\antigravity-ide\scratch\project-zero\docs\design\STAGE3K-ARCHITECTURE-REV5.md) confirms exact physical volume limits:

```text
Stage 3K Physical Storage Bounds:
- Maximum Volume Size   : 134,217,728 bytes (128 MiB max volume capacity)
- Maximum Inodes        : 256 inodes per volume
- Maximum File Size     : 4,268,032 bytes (~4.07 MiB max single file size)
- Maximum Filename Len  : 55 bytes per component
- StorageObject Slots   : 32 concurrent open file descriptors in kernel BSS
```

$$\mathbf{Statement\ of\ Scale\ Alignment:}$$
$$\text{Stage 3K is a microkernel bring-up storage substrate for early system boot and verification.}$$
$$\text{Workstation Edition intended scale (10,000+ files, multi-gigabyte volumes) DEPENDS ON A FUTURE STORAGE STAGE.}$$

---

## 18. CATALOG OF AUTHORITATIVE INVARIANTS

1. `I-WS-MEMBERSHIP-NOT-CAPABILITY`: Workspace membership is contextual metadata and NEVER confers capability authority.
2. `I-WS-PATH-TREE-FIRST-CLASS`: ZeroFS canonical POSIX paths remain the single first-class filesystem source of truth.
3. `I-WS-VIEW-NOT-AUTHORITY`: Workspace spatial viewports and context graphs are non-authoritative metadata overlays.
4. `I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY`: Provisional attribution MUST NOT silently convert heuristic inference into confirmed membership; untouched items lapse to `Unfiled`.
5. `I-WS-REMOVE-MEMBERSHIP-NOT-DELETE-OBJECT`: Removing a membership edge MUST NOT delete the underlying ZeroFS storage object or move canonical paths.
6. `I-WS-EXTERNAL-MODIFICATION-BEST-EFFORT`: Un-observed external modifications are reconciled best-effort.
7. `I-WS-BROWSER-ISOLATION-NOT-BROWSER-SEMANTIC-AWARENESS`: Profile isolation DOES NOT confer tab-level semantic workspace awareness.
8. `I-WS-REWIND-ONLY-WHEN-SUBSTRATE-SUPPORTS-HISTORICAL-CONTENT`: File-content rewind is REMOVED because Stage 3K lacks historical data extents.
9. `I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE`: External network side-effects CANNOT be undone by context rewind.
10. `I-WS-OBJECT-ID-SINGLE-AUTHORITY`: `workspaced` owns the single authoritative Global Volume `ObjectId` Registry at `/storage/system/object_id.registry`.
11. `I-WS-OBJECT-ID-USERSPACE-OBSERVATION-KEY`: Userspace identity keying `(Path_Hash, generation, mtime)` is a best-effort observation key, NOT a kernel physical stat invariant.
12. `I-WS-WRITE-ATTRIBUTION-DETERMINISTIC`: Workspace Attribution is `DETERMINISTIC` iff `WRITER_AUTHORITY` ($C_{\text{ws}}$) AND direct `WORKLOAD_CAUSALITY` match.
13. `I-WS-BOOT-TIME-ORDERING-VALID`: Chronological timestamp ordering across cold reboots uses `(boot_epoch_sequence, monotonic_ticks)` anchored to `DiskSuperblock.mount_count`.
14. `I-WS-EVENTLOG-NOT-FILE-VERSIONING`: `ContextEventRecord` is an append-only metadata audit log, NOT file-content versioning.

---

## 19. ARCHITECTURAL SELF-AUDIT & CHECKLIST

- **Single `ObjectId` Registry Atomicity**: Single 32 KiB chunk journal writes are atomic; multi-chunk updates are BEST-EFFORT on crash with ZeroFS physical state sovereign (Section 3).
- **Observation Key Boundaries**: Keyed by `(Path_Hash, generation, mtime)`; un-observed renames outside `libzero` are re-keyed best-effort (Section 4).
- **Rename Mechanics**: Mediated via `libzero` Ring 3 helpers (**Physical Source Semantics**); raw `rename` syscall NOT PROVIDED in Syscalls 11..16 (Section 5).
- **Time Ordering Proof**: Monotonic epoch sequence anchored to Stage 3K `DiskSuperblock.mount_count` Block 1 persistence prior to issuing timestamps (Section 12).
- **Causal Attribution**: Decoupled into `WRITER_AUTHORITY`, `WRITER_PROCESS`, `WORKLOAD_CAUSALITY`, and `WORKSPACE_ATTRIBUTION`. Per-write `TaskId` data block tagging claim eliminated (Section 6).
- **Frozen-Substrate Impact**: 0 kernel code changes, 0 syscalls added, 0 capability types added, 0 daemons added (Section 16).

---

# ZEROOS OBJECT & MEMBERSHIP MODEL REV7 VERDICT

```text
REV7 STATUS:
ARCHITECTURE DRAFT COMPLETE — PENDING ADVERSARIAL REVIEW

IMPLEMENTATION:
NOT STARTED

FREEZE:
NOT APPROVED

STAGE 3A–3N:
FROZEN / UNMODIFIED

EXTERNAL REVIEW:
REQUIRED
```

```text
STATUS: 🟡 REVISION COMPLETE — PENDING EXTERNAL ADVERSARIAL REVIEW
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3A–3N: FROZEN (0 bytes modified)
STAGE 4A–4F: FROZEN (100% reused)
STAGE 5–6: FROZEN (100% reused)
NEW SYSCALLS: 0
NEW DAEMONS: 0
```
