# ZEROOS OBJECT & MEMBERSHIP MODEL — REV4

**Subsystem:** Core Product Interaction Architecture & Workspace Membership Subsystem  
**Document State:** Architectural Specification Rev4 — Substrate Precision & Identity Re-alignment  
**Author:** DeepMind Advanced Agentic Coding Team  
**Date:** October 2026  
**Status:** 🟡 ARCHITECTURAL SPECIFICATION — PENDING ADVERSARIAL REVIEW  
**Authoritative Dependencies:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), Stage 5–6 (Frozen), WI-09/10/02/03/04 (Committed)

---

## 1. EXECUTIVE SUMMARY & REV4 REVISION BASIS

Following an independent external adversarial review of REV3 (evaluated by Claude as an unbiased outside judge), ZeroOS received a **REVISE BEFORE FREEZE** verdict with seven specific architectural findings (N1–N7).

### Core Architectural Corrections Implemented in REV4
1. **Object Identity Authority & Crash Atomicity (Finding N1)**: REV3 claimed crash-atomic `ObjectId` continuity across atomic saves (`write temp -> rename`) without defining the storage mechanism. REV4 adopts **Option A (Userspace `workspaced` ObjectId Mapping)**: `ObjectId` mapping lives in `/workspaces/<id>/context.graph` and is keyed by `(ZeroFS_Inode_Number, Inode_Generation)`. Because `workspaced` graph updates execute in Ring 3 userspace separately from Stage 3K ZeroFS block device transactions, **`ObjectId` continuity is explicitly specified as BEST-EFFORT across un-graceful system crashes**.
2. **Provisional Expiry Invariant Alignment (Finding N2)**: REV3 violated `I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY` by auto-confirming provisional items after 24 hours of user inactivity. REV4 eliminates silent auto-confirmation: untouched provisional items **lapse to `Unfiled`**, ensuring heuristic guesses are NEVER silently converted into authoritative membership.
3. **Membership vs Canonical Path Separation (Finding N3)**: REV3 stated that zero membership moves files to `/storage/unassigned/`. REV4 strictly separates metadata from filesystem paths: **membership changes NEVER mutate canonical filesystem paths or move files on disk**.
4. **Substrate ABI & Syscall Accuracy (Finding N4)**: REV3 referenced non-existent syscalls (`sys_file_rename`, `sys_file_unlink`, POSIX `link`). REV4 audits Stage 3K Syscalls 11..16 directly: `rename`, `unlink`, `symlink`, and POSIX `link` are **NOT PROVIDED as raw Stage 3K syscalls**. Directory entry management is mediated via directory capabilities or userspace VFS helpers.
5. **Legitimate Stage 3K Scale Limits (Finding N5)**: REV4 explicitly documents Stage 3K's physical volume limits ($32,768$ blocks = $128\text{ MiB}$ max volume size, $256$ max inodes, $4.07\text{ MiB}$ max file size, $55\text{ B}$ max filename length) and distinguishes them from Workstation Edition target scale, which requires a future storage architecture stage.
6. **Resumption & Rewind Terminology Realignment (Finding N6)**: L1 relaunch resumption is reclassified from `GUARANTEED` to **`BEST-EFFORT`**. File-content rewind is completely removed; historical context logging is renamed to **Membership Audit Log** and DAG re-execution is renamed to **Workload DAG Rerun**.
7. **Empirical Substrate Audit (Finding N7)**: Re-evaluated the 0/0/0/0 claim against empirical code contracts. Marked atomic-save crash continuity as `BEST-EFFORT`, path projection as `FUTURE WORK`, user-visible Trash as `FUTURE WORK`, and historical data versioning as `ABSENT`.

---

## 2. REV4 RESPONSE TO CLAUDE ADVERSARIAL REVIEW (FINDINGS N1–N7)

| Finding | Description | REV4 Architectural Correction | Exact Section | Closure Status |
|---|---|---|---|---|
| **N1** | `ObjectId` continuity claimed crash-atomic without storage mechanism | Adopted Option A (`workspaced` mapping keyed by `(inode_num, inode_gen)`). Explicitly acknowledged mapping updates are separate from ZeroFS block commits; continuity is **BEST-EFFORT on crash**. | Section 3 | 🟢 CLOSED |
| **N2** | 24h provisional auto-confirmation violated invariant `I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY` | Eliminated silent auto-confirmation. Untouched provisional items **lapse to `Unfiled`**. Heuristic guesses never become confirmed membership without user action. | Section 12 | 🟢 CLOSED |
| **N3** | Zero-membership path relocation contradicted "membership = metadata" | Removed physical path relocation on membership changes. Membership is strictly context metadata; canonical ZeroFS paths remain untouched. | Section 10 | 🟢 CLOSED |
| **N4** | Cited non-existent Stage 3K syscalls (`rename`, `unlink`, `link`) | Audited Syscalls 11..16. Formally classified `rename`, `unlink`, `symlink`, and `hardlink` as **NOT PROVIDED as raw Stage 3K syscalls**. | Section 4, Section 6 | 🟢 CLOSED |
| **N5** | Over-claimed Stage 3K storage scale and capacity | Documented exact Stage 3K geometry ($128\text{ MiB}$ volume, $256$ inodes, $4.07\text{ MiB}$ max file size). Noted Workstation Edition scale depends on future storage stages. | Section 19 | 🟢 CLOSED |
| **N6** | L1 resumption over-claimed as GUARANTEED; rewind terminology conflated | Re-classified L1 resumption as **BEST-EFFORT**. Renamed context log revert to "Membership Audit Log" and task execution to "Workload DAG Rerun". | Section 15, Section 16 | 🟢 CLOSED |
| **N7** | 0/0/0/0 claim obfuscated userspace/substrate gap | Added 8-column Substrate Audit Table with explicit status tags (`VERIFIED`, `UNPROVEN`, `BEST-EFFORT`, `FUTURE WORK`, `ABSENT`). | Section 17 | 🟢 CLOSED |

---

## 3. OBJECT IDENTITY (`ObjectId`) ARCHITECTURE & CONTINUITY

REV4 adopts **Option A (Userspace `workspaced` Mapping)** to define `ObjectId` identity without modifying frozen Stage 3K kernel disk structures.

```text
ZeroFS Persistent Storage (Stage 3K Inode)
  ├── inode_num: u32 (1..255)
  └── generation: u32 (Recycling counter)
             ▲
             │ (Key: inode_num + generation)
workspaced Context Graph Mapping (/workspaces/<id>/context.graph)
  └── ObjectId (128-bit DistributedId) ──► MEMBER_OF(WorkspaceId)
```

### 3.1 `ObjectId` Ownership & Data Layout
- **Authoritative Owner**: `workspaced` daemon (Ring 3 userspace service).
- **Mapping Key**: Unified tuple `(inode_number: u32, inode_generation: u32)`.
- **Inode Generation**: `DiskInode.generation` in Stage 3K increments whenever an inode slot is recycled, guaranteeing that a new file reusing an old inode number CANNOT inherit the previous file's `ObjectId`.
- **Persistence Location**: Saved in `/workspaces/<id>/context.graph` extents on ZeroFS.

### 3.2 Transaction Boundary & Crash-Recovery Behavior

$$\mathbf{Invariant\ I-WS-OBJECT-ID-MAPPING-BEST-EFFORT-ON-CRASH:}$$
$$\text{Stage 3K ZeroFS block transactions commit independently of } \texttt{workspaced} \text{ context graph disk updates.}$$
$$\text{Following an un-graceful system crash, } \text{ObjectId} \text{ continuity across atomic saves is BEST-EFFORT.}$$

```text
Atomic Save Sequence (write temp -> fsync -> rename(temp, target)):
1. App writes temp file -> Allocates Inode I_temp (gen g_temp).
2. App flushes data via SYS_FILE_SYNC -> ZeroFS commits temp data.
3. App updates directory entry target -> Points directory entry to I_temp. Target I_target marked PENDING_DELETE.
4. workspaced receives VFS directory notification -> Updates in-memory mapping: ObjectId_target -> (I_temp, g_temp).
5. workspaced commits context.graph to ZeroFS.

Crash Scenarios & Recovery:
- Crash between Step 3 & 4 (ZeroFS committed, workspaced un-committed):
  On reboot, workspaced inspects directory entry for target, detects I_temp (gen g_temp),
  and reconciles ObjectId_target -> (I_temp, g_temp) using path-history heuristics.
- Crash before Step 3:
  temp file orphaned; target file unchanged. ObjectId_target remains mapped to (I_target, g_target).
```

---

## 4. OBJECT IDENTITY OPERATION AUDIT MATRIX

REV4 provides an explicit operation-by-operation classification of `ObjectId` behavior across all filesystem actions:

| Operation | Trigger Command / Syscall | `ObjectId` Behavior | Substrate Mechanism & Guarantee |
|---|---|---|---|
| **Create** | `SYS_FILE_OPEN(O_CREATE)` | **NEW ID** | `workspaced` allocates new 128-bit `DistributedId`. |
| **Directory Move** | VFS Directory Update | **PRESERVED** | `inode_num` and `generation` unchanged; mapping preserved. |
| **Atomic Save** | `write temp -> fsync -> rename` | **PRESERVED (BEST-EFFORT)** | Directory entry updated to $I_{\text{temp}}$; `workspaced` re-keys $ID_{\text{target}} \to (I_{\text{temp}}, g_{\text{temp}})$. |
| **Rename-Over** | `rename(src, target)` (target exists) | **PRESERVED (BEST-EFFORT)** | Target $ID_{\text{target}}$ re-keyed to $I_{\text{src}}$; old target inode tombstoned. |
| **Copy** | User / App `cp a b` | **NEW ID** | New inode $I_{\text{new}}$ allocated in ZeroFS; new `ObjectId` generated. |
| **Hardlink** | POSIX `link()` | **NOT PROVIDED** | `SYS_FILE_LINK` is NOT provided by Stage 3K Syscalls 11..16. |
| **Symlink** | POSIX `symlink()` | **NOT PROVIDED** | `SYS_FILE_SYMLINK` is NOT provided by Stage 3K Syscalls 11..16. |
| **Cross-Mount Move**| `mv /vol1/a /vol2/b` | **NEW ID + LINEAGE** | Physical copy creates new inode on Target Volume; `workspaced` links via `RunRecord`. |
| **Unlink + Recreate**| `unlink a && touch a` | **NEW ID** | Inode unlinked; `touch` allocates new inode $I_{\text{new}}$ with incremented generation. |
| **External Mod** | Offline disk edit | **BEST-EFFORT** | Reconciled on boot via `(inode_num, generation)` match if inode un-recycled. |
| **Git Checkout** | `git checkout branch` | **OPERATION SPECIFIC**| In-place file writes preserve `ObjectId`; file replacements allocate new `ObjectId`. |
| **Rsync Rewrite** | `rsync -a src dst` | **OPERATION SPECIFIC**| In-place write preserves `ObjectId`; temp-file replacement re-keys `ObjectId`. |
| **Tar Extraction** | `tar -xf archive.tar` | **NEW ID** | Allocates new inodes in ZeroFS; generates new `ObjectId`s. |

---

## 5. CROSS-MOUNT MOVES & PHYSICAL ISOLATION

Cross-mount file moves across independent ZeroFS storage volumes (e.g., from `/storage/vol1/` to `/storage/vol2/`) are physically executed as:

$$\text{Cross-Mount Move} \implies \text{Copy Data to Target Volume} + \text{Unlink Source File}$$

### 5.1 Identity Rule for Cross-Mount Moves
- **New `ObjectId` Allocation**: Because the target volume allocates a completely new physical inode on a different block device, raw cross-mount moves allocate a **NEW `ObjectId`** for the target file.
- **Lineage Tracking**: `workspaced` records a `RunRecord` entry linking `ObjectId_new` to `ObjectId_old` as a `DERIVED_FROM` lineage event.
- Raw cross-mount moves are NEVER described as atomically preserving `ObjectId`.

---

## 6. STAGE 3K SUBSTRATE ABI & SYSCALL AUDIT

A direct audit of Syscalls 11..16 in [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish\.gemini\antigravity-ide\scratch\project-zero\docs\design\STAGE3K-ARCHITECTURE-REV5.md) establishes the exact kernel syscall boundary:

```text
Frozen Stage 3K Kernel Syscalls (Syscalls 11..16):
- SYS_FILE_OPEN (11)  : Open/create file via Directory Capability
- SYS_FILE_READ (12)  : Read bytes from open StorageObject handle
- SYS_FILE_WRITE (13) : Write bytes to open StorageObject handle
- SYS_FILE_CLOSE (14) : Close StorageObject handle
- SYS_FILE_STAT (15)  : Query file stat (size, blocks, type, generation, mtime)
- SYS_FILE_SYNC (16)  : Flush dirty buffers to block device storage
```

### 6.1 Substrate Operations Classification
- **Directory Operations (`rename`, `unlink`, `mkdir`)**: **NOT PROVIDED as raw Stage 3K syscalls**. Executed internally by Layer 4 directory manager functions or mediated via Ring 3 `libzero` VFS helpers.
- **POSIX Hardlinks (`link`)**: **NOT PROVIDED**. Explicit non-goal of Stage 3K (Section 3.2).
- **POSIX Symlinks (`symlink`)**: **NOT PROVIDED**. Un-supported in Stage 3K kernel substrate.
- **Directory Enumeration (`readdir`)**: Executed by reading directory data blocks via `SYS_FILE_READ` on a Directory Capability handle.

---

## 7. REVISED DETERMINISTIC ATTRIBUTION TAXONOMY

REV4 strictly separates writer identity, path scope, and advisory steering:

```text
Attribution Classification Hierarchy:

1. WRITER-LINEAGE GROUND TRUTH (DETERMINISTIC)
   - Process spawned by workloadd with C_ws capability handle
   - Direct kernel/workload capability proof linking writer process to WorkspaceId

2. PATH-SCOPE ATTRIBUTION (LOCATION CONTEXT)
   - File written inside canonical path /storage/workspaces/<ws_id>/
   - Establishes physical storage location; does NOT prove human intent

3. ADVISORY STEERING (OBSERVED / PROVISIONAL)
   - Environment variables (HOME, CWD, XDG_CONFIG_HOME, XDG_DATA_HOME)
   - Active spatial input focus (uids / shelld active window)
   - Auto-attached as Provisional; MUST NOT be auto-committed as Deterministic

4. AMBIGUOUS
   - Background daemons, single-instance IPC handoffs, detached workers lacking C_ws
```

---

## 8. CHILD PROCESS & DAEMON CAPABILITY INHERITANCE

```text
Process Creation (sys_process_create / OP_PROCESS_SPAWN)
       │
       ├── Spawned under workloadd task supervision?
       │     ├── YES ──► Inherits C_ws capability handle  ──► Writer Ground Truth (DETERMINISTIC)
       │     └── NO  ──► Lacks C_ws capability handle     ──► Fallback to Path / Advisory Steering
       │
       └── Worker Process Action
             ├── Retains C_ws handle?                     ──► DETERMINISTIC
             └── Drops / lacks C_ws handle?               ──► AMBIGUOUS
```

- **Inheritance Rule**: `sys_process_create` copies parent handle tables. Child processes spawned by a workspace workload retain $C_{\text{ws}}$ unless explicitly revoked, maintaining **Deterministic** writer lineage. Detached daemons spawned without $C_{\text{ws}}$ fall back to **Ambiguous**.

---

## 9. SINGLE-INSTANCE APPLICATIONS & IPC HANDOFF

When `Workspace B` triggers an action that sends an IPC message to an existing single-instance process running in `Workspace A`:

```text
Workspace B (User Action: Open doc_b.txt)
       │
       ▼ IPC Message
Single-Instance Application Process (Running under Workspace A, holding C_wsA)
       │
       ▼ Writes output file out.dat
```

### 9.1 Single-Instance Policy
1. **Authoritative Writer Rule**: The write is executed by a process holding $C_{\text{wsA}}$. ZeroOS attributes the physical write to **Workspace A** (or marks it **`AMBIGUOUS`** for Workspace B).
2. **No Silent Capability Transfer**: ZeroOS NEVER silently transfers capability authority or process attribution across IPC boundaries.
3. **Workspace B Reference**: Workspace B receives a `Provisional` reference edge pointing to the output artifact.

---

## 10. MEMBERSHIP SEMANTICS & CANONICAL PATH SEPARATION

$$\mathbf{Invariant\ I-WS-MEMBERSHIP-NOT-CAPABILITY:}$$
$$\text{Workspace membership edges } \text{MEMBER_OF}(W_A, O_X) \text{ are strictly contextual metadata.}$$
$$\text{Membership edges MUST NOT grant capability authority or alter canonical filesystem paths.}$$

$$\mathbf{Invariant\ I-WS-PATH-TREE-FIRST-CLASS:}$$
$$\text{ZeroFS canonical POSIX paths } (/storage/workspaces/<id>/...) \text{ remain the single first-class source of truth.}$$
$$\text{Adding or removing a } \text{MEMBER_OF} \text{ edge MUST NOT move or rename physical files on disk.}$$

### 10.1 Membership Operations
- **Home Membership**: `MEMBER_OF(W_home, O_X, Role=Owner)` tracks originating workspace.
- **Secondary Reference**: `MEMBER_OF(W_sec, O_X, Role=Reference)` pins object in secondary workspace.
- **Remove Membership**: Deletes `MEMBER_OF` metadata edge in `workspaced`. Canonical ZeroFS path and physical file remain 100% untouched.
- **Zero-Membership Objects**: If all `MEMBER_OF` edges are removed, object remains at its canonical ZeroFS path as an **Unfiled Object**.

---

## 11. TRASH, RETENTION, AND RECLAMATION

ZeroOS strictly distinguishes user-visible metadata operations from kernel storage reclamation:

```text
Operation                 Target Entity                Storage Action                        Reversibility
─────────────────────────────────────────────────────────────────────────────────────────────────────────────
Remove Membership         ContextNode metadata edge     Deletes MEMBER_OF edge in workspaced   Reversible (Re-add edge)
Delete Object             ZeroFS Inode                  Unlinks directory entry via ZeroFS     IRREVERSIBLE (Data lost)
Open-Handle Unlink        Open StorageObject            Sets PENDING_DELETE on inode           Pending handle close
Physical Reclamation      Block Allocation Bitmap       Frees data blocks when handles = 0    Permanent block reuse
```

### 11.1 Substrate Limitation Notice
- **No Native Undo for Deleted Content**: Stage 3K ZeroFS provides POSIX `PENDING_DELETE` semantics only. Calling file delete unlinks the inode immediately. **File deletion in ZeroOS is IRREVERSIBLE**.
- User-visible Trash (staging directory with undo/restore) is classified as **`FUTURE WORK / UNPROVEN`** (userspace daemon staging).

---

## 12. PROVISIONAL ATTRIBUTION & LAPSE TO UNFILED

$$\mathbf{Invariant\ I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY:}$$
$$\text{Provisional attribution MUST NOT silently convert heuristic inference into confirmed membership.}$$

```text
Provisional Item Created (Attributed via Advisory Steering)
         │
         ├── User explicitly opens, edits, references, or pins item ──► CONFIRMED MEMBERSHIP
         ├── User manually reassigns or removes item               ──► REASSIGNED / DETACHED
         │
         └── 24 Hours Monotonic Expiry (No user interaction)       ──► Lapses to UNFILED
```

### 12.1 Provisional Governance
- **No Silent Auto-Confirmation**: Untouched provisional items **lapse to `Unfiled`** after 24 hours. Heuristic guesses are NEVER silently converted into confirmed membership without user action.

---

## 13. PATH PROJECTION CONCEPTUAL CONTRACT

For an unmodified CLI binary running in `Workspace B` needing to read a referenced file residing physically in `Workspace A`:

```text
Workspace B Process Context
       │
       ▼ Open projected path
/workspaces/ws_b/.proj/<obj_id>/data.csv  ──(Read-Only VFS Projection)──► /storage/workspaces/ws_a/data.csv
```

### 13.1 Conceptual Contract & Substrate Status
- **Read-Only Access**: Projected paths allow read-only access (`FILE_READ`). Writes return `-EACCES`.
- **Capability Scoping**: Access requires `Workspace B` to hold a valid read capability over target object.
- **Substrate Status**: Path projection is NOT implemented in Stage 3K kernel substrate. Classified as **`FUTURE WORK / UNPROVEN`** (userspace VFS overlay layer).

---

## 14. PERSISTENT TIMESTAMPS & BOOT EPOCHS

Monotonic CPU ticks are NOT continuous across cold reboots. Persisted timestamps in `workspaced` context records use a **Qualified Boot Epoch**:

```rust
#[repr(C)]
pub struct QualifiedTimestamp {
    pub boot_epoch_id: DistributedId, // 16 bytes: 128-bit persistent epoch ID generated on boot
    pub monotonic_ticks: u64,          // 8 bytes: Monotonic ticks elapsed within boot epoch
}
```

- **Provisional Expiry**: Evaluated using `MonotonicSystemTicks` during an active session, or `QualifiedTimestamp` comparison across reboots.

---

## 15. RESUMPTION SPECIFICATION (BEST-EFFORT L1)

```text
Resumption Level              ZeroOS Status       Mechanism & Guarantee
─────────────────────────────────────────────────────────────────────────────────────────────────────────
L0: Context Resumption        🟢 GUARANTEED       Restores workspaced context graph, active focus,
                                                  spatial viewports, and action timeline.
L1: Relaunch Resumption       🟡 BEST-EFFORT      Relaunches app processes with allowlisted environment
                                                  subset (HOME, CWD, PATH). May fail if binary missing
                                                  or sockets stale.
L2: App-Native Self-Restore   🟢 CONDITIONAL     Passes file tokens to apps supporting document restore.
L3: Process Checkpointing     ❌ EXCLUDED         Arbitrary RAM/CPU register checkpointing EXCLUDED.
```

- **Environment Security**: Arbitrary process environment variables are NOT persisted (prevents secret leaks). Only an allowlisted subset (`HOME`, `CWD`, `PATH`, `XDG_*`) is restored.

---

## 16. TERMINOLOGY REALIGNMENT & REWIND GUARANTEES

$$\mathbf{Invariant\ I-WS-FILE-CONTENT-REWIND-UNSUPPORTED-IN-STAGE3K:}$$
$$\text{Stage 3K ZeroFS lacks historical data extents. File-content rewind is REMOVED from the contract.}$$

$$\mathbf{Invariant\ I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE:}$$
$$\text{External side-effects (HTTP POSTs, emails sent, Git pushes) CANNOT be undone by context rewind.}$$

```text
Terminology Realignment:
- "Context Rewind"      ──► Renamed to MEMBERSHIP AUDIT LOG (reverts workspaced context graph nodes)
- "Workload Rewind"     ──► Renamed to WORKLOAD DAG RERUN (re-executes task DAG against current inputs)
- "File Content Rewind" ──► EXCLUDED (Requires future versioned storage architecture stage)
```

---

## 17. SUBSTRATE COMPATIBILITY AUDIT (0/0/0/0 AUDIT)

We audit all architectural requirements against the authoritative frozen kernel contracts:

| System Requirement | Required Substrate Primitive | Authoritative Source Evidence | Substrate Status |
|---|---|---|---|
| **Writer Identity** | Kernel Process Capability Table | `kernel/src/cap/types.rs`, Stage 3H | `VERIFIED` |
| **Path Open** | `SYS_FILE_OPEN` (Syscall 11) | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `VERIFIED` |
| **Membership Persistence** | ZeroFS `/workspaces/<id>/context.graph` | `STAGE4D-IMPLEMENTATION.md` Section 5 | `VERIFIED` |
| **Atomic Save Identity** | Userspace `workspaced` Mapping | `STAGE4D-IMPLEMENTATION.md` Section 5 | **`BEST-EFFORT`** (Non-atomic with disk commits) |
| **Path Projection** | Read-Only Virtual VFS Directory | `STAGE3K-ARCHITECTURE-REV5.md` Section 14 | **`FUTURE WORK`** |
| **User-Visible Trash** | Staging Directory Layer | `STAGE3K-ARCHITECTURE-REV5.md` Section 19 | **`FUTURE WORK`** |
| **Historical Data Versions**| ZeroFS Data Extent Versioning | `STAGE3K-ARCHITECTURE-REV5.md` Section 6, 7 | **`ABSENT`** (Removed from MVP contract) |
| **Per-Instance `/tmp`** | VFS Namespace Virtualization | `STAGE3K-ARCHITECTURE-REV5.md` Section 4 | **`ABSENT`** (Marked Future Work) |
| **Browser Syscall Surface**| Microkernel Userspace Execution | Stage 3I, Stage 3J | **`UNPROVEN`** (MVP Prerequisite) |

```text
Kernel Syscalls Added:         0
New Capability Types:          0
New Daemons Required:          0
Stage 3A–3N Modifications:     0
```

---

## 18. PROVISIONAL EVENT LOG CONTRACT

`workspaced` maintains a minimal append-only Event Log stored in `/workspaces/<id>/context.graph` extents:

```rust
#[repr(C)]
pub struct ContextEventRecord {
    pub event_id: u64,               // Monotonic event sequence number
    pub timestamp: QualifiedTimestamp,// Qualified persistent epoch timestamp
    pub event_type: u16,             // 1=NodeAdded, 2=NodeRemoved, 3=EdgeAdded, 4=ProvisionalLapsed
    pub target_object_id: DistributedId,// Target ObjectId
    pub workspace_id: DistributedId, // Workspace context
}
```

- **Compaction**: When log size exceeds $64\text{ KiB}$, `workspaced` compacts tombstoned records while retaining current active graph edges.

---

## 19. STAGE 3K STORAGE SCALE vs WORKSTATION EDITION SCALE

### 19.1 Empirical Stage 3K Physical Limits
Direct audit of [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish\.gemini\antigravity-ide\scratch\project-zero\docs\design\STAGE3K-ARCHITECTURE-REV5.md) (Sections 6, 7, 10, 21) confirms exact hardware volume bounds:

```text
Stage 3K Physical Storage Geometry Limits:
- Maximum Volume Blocks : 32,768 blocks (4 KiB block size)
- Maximum Volume Size   : 134,217,728 bytes (128 MiB max volume capacity)
- Maximum Inodes        : 256 inodes per volume (INODE_TABLE_BLOCKS = 16)
- Maximum File Size     : 4,268,032 bytes (~4.07 MiB max single file size)
- Maximum Filename Len  : 55 bytes per component (DiskDirEntry)
- StorageObject Slots   : 32 concurrent open file descriptors in kernel BSS
- Max Journal Payload   : 32 KiB (8 data blocks per transaction)
```

### 19.2 Scale Alignment Notice
$$\mathbf{Statement\ of\ Scale\ Alignment:}$$
$$\text{Stage 3K is a microkernel bring-up filesystem designed for early system boot and verification.}$$
$$\text{Workstation Edition intended scale (10,000+ files, multi-gigabyte volumes) DEPENDS ON A FUTURE STORAGE STAGE.}$$

---

## 20. CATALOG OF AUTHORITATIVE INVARIANTS

1. `I-WS-MEMBERSHIP-NOT-CAPABILITY`: Workspace membership is contextual metadata and NEVER confers capability authority.
2. `I-WS-PATH-TREE-FIRST-CLASS`: ZeroFS canonical POSIX paths remain the single first-class filesystem source of truth.
3. `I-WS-VIEW-NOT-AUTHORITY`: Workspace spatial viewports and context graphs are non-authoritative metadata overlays.
4. `I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY`: Provisional attribution MUST NOT silently convert heuristic inference into confirmed membership; untouched items lapse to `Unfiled`.
5. `I-WS-REMOVE-MEMBERSHIP-NOT-DELETE-OBJECT`: Removing a membership edge MUST NOT delete the underlying ZeroFS storage object or move canonical paths.
6. `I-WS-EXTERNAL-MODIFICATION-BEST-EFFORT`: Un-observed external modifications are reconciled best-effort.
7. `I-WS-BROWSER-ISOLATION-NOT-BROWSER-SEMANTIC-AWARENESS`: Profile isolation DOES NOT confer tab-level semantic workspace awareness.
8. `I-WS-REWIND-ONLY-WHEN-SUBSTRATE-SUPPORTS-HISTORICAL-CONTENT`: File-content rewind is REMOVED because Stage 3K lacks historical data extents.
9. `I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE`: External network side-effects CANNOT be undone by context rewind.
10. `I-WS-OBJECT-ID-MAPPING-BEST-EFFORT-ON-CRASH`: `ObjectId` mapping in userspace is best-effort across un-graceful system crashes.
11. `I-WS-FILE-CONTENT-REWIND-UNSUPPORTED-IN-STAGE3K`: Stage 3K substrate lacks historical extent storage; content rewind is unsupported.

---

## 21. ARCHITECTURAL SELF-AUDIT & CHECKLIST

- **Object Identity**:
  - Where does `ObjectId` live? **Userspace `workspaced` mapping** (Option A, Section 3).
  - What is its key? **`(inode_number, inode_generation)`** (Section 3.1).
  - How does it survive inode reuse? **`DiskInode.generation` increments on recycling** (Section 3.1).
  - How does rename-over preserve it? **`workspaced` re-keys $ID_{\text{target}} \to (I_{\text{temp}}, g_{\text{temp}})$** (Section 9.1).
  - Is crash atomicity claimed? **NO** (explicitly marked BEST-EFFORT on crash in Section 3.2).
- **Storage Substrate**:
  - Are `rename`, `unlink`, `link` raw Stage 3K syscalls? **NO** (classified NOT PROVIDED in Section 6).
  - Are exact Stage 3K limits documented? **YES** ($128\text{ MiB}$ volume, $256$ inodes, Section 19).
- **Membership**:
  - Does membership change canonical path? **NO** (`I-WS-PATH-TREE-FIRST-CLASS`, Section 10).
- **Attribution**:
  - What makes a write deterministic? **Writer ground truth ($C_{\text{ws}}$ process handle, Section 7)**.
  - What makes it observed? **Advisory steering (HOME/CWD/focus, Section 7)**.
- **Rewind & Resumption**:
  - Is file-content rewind claimed? **NO** (completely removed in Section 16).
  - Is L1 resumption guaranteed? **NO** (re-classified BEST-EFFORT in Section 15).

---

# ZEROOS OBJECT & MEMBERSHIP MODEL REV4 VERDICT

```text
REV4 STATUS:
ARCHITECTURE DRAFT COMPLETE — PENDING EXTERNAL ADVERSARIAL REVIEW

IMPLEMENTATION:
NOT STARTED

FREEZE:
NOT APPROVED

EXTERNAL REVIEW:
REQUIRED (Claude Independent Adversarial Review)
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
