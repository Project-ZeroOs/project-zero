# ZEROOS OBJECT & MEMBERSHIP MODEL — REV8

**Subsystem:** Core Product Interaction Architecture & Workspace Membership Subsystem  
**Document State:** Architectural Specification Rev8 — Frozen REV3 Substrate-Aligned Architecture  
**Author:** DeepMind Advanced Agentic Coding Team  
**Date:** October 2026  
**Status:** 🟢 FROZEN FOR IMPLEMENTATION  
**Authoritative Dependencies:**  
- `ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV3.md` (Frozen)  
- `ZEROOS-FILESYSTEM-MUTATION-REV3-POST-IMPLEMENTATION-VERIFICATION.md` (Verified)  
- `ZEROOS-OBJECT-AND-MEMBERSHIP-REV3-SUBSTRATE-REVALIDATION.md` (Revalidated)  
- Stage 3A–3N (Frozen Baseline + Approved Additive Amendments 3H/3I/3K)  
- Stage 4A–4F (Frozen Baseline)

---

## 1. EXECUTIVE SUMMARY & REV8 REVISION BASIS

Following the formal completion, verification, and forensic revalidation of the **REV3 Filesystem Mutation Substrate** ([`ZEROOS-OBJECT-AND-MEMBERSHIP-REV3-SUBSTRATE-REVALIDATION.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-OBJECT-AND-MEMBERSHIP-REV3-SUBSTRATE-REVALIDATION.md)), the **ZeroOS Object & Membership Model** has been updated to **REV8**.

REV8 is a **targeted substrate-alignment revision**. It does NOT redesign the Object & Membership product model, 3-way system authority boundaries, or workspace semantics. Instead, it replaces obsolete filesystem substrate assumptions from REV7 (such as claiming "raw `rename` syscall is NOT PROVIDED") with direct bindings to the implemented and frozen **REV3 kernel primitives**:

1. **Direct Syscall Binds**: Formally binds Object & Membership operations to REV3 Syscalls: `SYS_DIR_CREATE` (32), `SYS_FILE_UNLINK` (33), `SYS_FILE_RENAME` (34), and `SYS_DIR_READ` (35).
2. **Capability Authorization**: Explicitly enforces Bit 15 capability right `MUTATE = 0x8000` for all directory creation, file unlink, and rename operations via `resolve_directory_inode`.
3. **Atomic WAL Journal Alignment**: Binds same-directory rename, cross-directory move, and rename-over to the exact physical crash-recovery guarantees of `DiskJournalBlockV2` (`ZERO_JR2`).
4. **Directory Enumeration & Concurrency Control**: Consumes persistent `dir_inode.generation` counters and Policy A (`SyscallError::ResourceConflict` / `-EBUSY`) generation mismatch error ABI for deterministic Ring 3 enumeration restarts in `libzero`.
5. **Decoupled System Atomicity**: Explicitly decouples physical ZeroFS transaction atomicity (`DiskJournalBlockV2`) from Ring 3 `workspaced` logical registry atomicity (`/storage/system/object_id.registry`). Establishes ZeroFS physical sovereignty and specifies boot reconciliation rules via `SYS_DIR_READ`.

---

## 2. FROZEN REV3 SUBSTRATE DEPENDENCIES

REV8 treats the following primitives as external, frozen kernel dependencies:

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                       FROZEN REV3 SUBSTRATE DEPENDENCIES                        │
├───────────────────┬─────────────────────────────┬───────────────────────────────┤
│ Domain            │ Primitive Symbol            │ Concrete Value / Interface    │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Mutation Syscalls │ SYS_DIR_CREATE              │ Syscall Opcode 32             │
│                   │ SYS_FILE_UNLINK             │ Syscall Opcode 33             │
│                   │ SYS_FILE_RENAME             │ Syscall Opcode 34             │
│                   │ SYS_DIR_READ                │ Syscall Opcode 35             │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Capability Right  │ cap_rights::MUTATE          │ Bit 15 (0x8000)               │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Journal WAL       │ DiskJournalBlockV2          │ 4096-byte layout ("ZERO_JR2") │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Directory Gen     │ dir_inode.generation        │ u32 counter at 0x30..0x34     │
│ Concurrency Ctrl  │ Policy A Mismatch           │ ResourceConflict (-EBUSY/-17) │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Containment       │ resolve_directory_inode     │ Strict validation; NO SILENT  │
│                   │                             │ ROOT FALLBACK (Handle 0 fails)│
└───────────────────┴─────────────────────────────┴───────────────────────────────┘
```

---

## 3. REV7 → REV8 SUBSTRATE CHANGE MAP

| REV7 Section | REV7 Obsolete Assumption | REV8 Substrate-Aligned Replacement | Rationale / Architectural Evidence |
|---|---|---|---|
| **Section 1 (B3)** | Claimed raw `rename` syscall is **NOT PROVIDED** in Stage 3K | Bound directly to **`SYS_FILE_RENAME` (Opcode 34)** | REV3 added kernel `DirectoryManager::rename` and Syscall 34. |
| **Section 2 (B3)** | Claimed rename is mediated solely by Ring 3 copy/delete helpers | Executed natively in kernel via **`SYS_FILE_RENAME`** with dual-slot WAL logging | REV3 kernel provides atomic cross-directory rename and rename-over. |
| **Section 4 (B2)** | Keyed identity solely via `Path_Hash + generation + mtime` | Keyed via **`ObjectId` in `workspaced` registry**, using `SYS_DIR_READ` `inode_number` | REV3 Syscall 35 returns `UserDirEntry` containing physical `inode_number`. |
| **Section 5 (B3)** | Directory mutation required Layer 4 internal kernel calls | Bound directly to **`SYS_DIR_CREATE` (32)** and **`SYS_FILE_UNLINK` (33)** | REV3 exposed Ring 3 syscalls for directory creation and unlinking. |
| **Section 5 (B3)** | Capability validation used `FILE_WRITE` for directory edits | Requires explicit Bit 15 **`cap_rights::MUTATE` (`0x8000`)** | REV3 Stage 3H amendment defined `MUTATE` generic management right. |
| **Section 6 (B5)** | Directory handle resolution permitted handle 0 root fallback | Bound to **`resolve_directory_inode`**; **NO SILENT ROOT FALLBACK** | REV3 strictly validates handle 0; invalid handles fail with `BadHandle`. |
| **Section 14 (S3)**| EventLog enumeration lacked concurrency semantics | Consumes **`SYS_DIR_READ` Policy A** generation mismatch (`-EBUSY`) | REV3 directory generation counter bumping and Policy A check. |

---

## 4. OBJECT IDENTITY & RELATIONSHIP MATRIX

ZeroOS strictly distinguishes logical object identity, physical filesystem storage identity, directory text references, and process capability handles.

```text
ZeroFS Physical Inode (device_id, inode_num) ◄─── Stores physical data & links_count
            ▲
            │ (Indexed by workspaced Global Volume Registry)
            │
      128-bit ObjectId ◄────────────────────── Logical entity identity (DistributedId)
            ▲
            │ (Referenced by Workspace Context Graph)
            │
MEMBER_OF(ws_id, obj_id) ──────────────────── Contextual metadata membership edge
```

### 4.1 System Identity Matrix

| Identity Type | Scope | Lifetime | Persistent? | Changes on Rename? | Authority |
|---|---|---|---|---|---|
| **`ObjectId`** | System-Wide (Logical) | Object Creation to Unlink | YES (`workspaced` registry) | **NO** | `workspaced` (`/storage/system/object_id.registry`) |
| **`inode_num`** | Block Device Volume | Allocation to Free | YES (`DiskInode` on media) | **NO** | ZeroFS Kernel (`InodeManager`) |
| **`Handle`** | Per-Process Table | `open` to `close` | NO (Volatile RAM) | **NO** | Process `StorageObjectTable` |
| **`UserDirEntry`** | Directory Slot | Slot creation to unlink | YES (`DiskDirEntry` on media) | **YES** (Text name updated) | ZeroFS Directory (`DirectoryManager`) |
| **`ProcessId` (PID)** | System Task Table | Process spawn to exit | NO (Volatile RAM) | **NO** | Kernel Task Manager |
| **`WorkspaceId`** | Logical Workspace | Creation to Destroy | YES (Workspace metadata) | **NO** | `workspaced` Context Graph |

$$\mathbf{Invariant\ I-WS-IDENTITY-RENAME-STABILITY:}$$
$$\text{Executing a rename operation } (\text{same-directory or cross-directory}) \text{ MUST NOT alter the file's } \text{DiskInode.inode\_num}$$
$$\text{or its logical } \text{ObjectId. } \text{Rename updates directory text and workspace container references only.}$$

---

## 5. OBJECT CREATION MECHANICS

Creation is explicitly decoupled into **Physical Filesystem Creation** (kernel space) and **Object & Membership Registration** (userspace `workspaced`).

```text
Human / Agent Intent (e.g. "Create Document in Workspace B")
            │
            ▼
Process holds Workspace Capability Handle C_ws (Bit 15 MUTATE right)
            │
            ▼
Physical Creation: SYS_FILE_OPEN(dir_h, "data.csv", O_CREATE | O_RDWR)  [Opcode 11]
   OR SYS_DIR_CREATE(dir_h, "subdir")                                    [Opcode 32]
            │
            ├── Kernel allocates DiskInode (assigns creator_pid)
            ├── Inserts entry in parent directory
            ├── Increments parent dir_inode.generation
            └── Journal V2 WAL commits transaction
            │
            ▼
Object Registration: libzero notifies workspaced via IPC
            │
            ├── workspaced allocates 128-bit ObjectId (DistributedId)
            ├── Registers mapping (ObjectId ──► device_id, inode_num, path) in object_id.registry
            └── Adds MEMBER_OF(ws_id, obj_id) edge in /workspaces/<ws_id>/context.graph
```

- **Transaction Boundary**: Physical creation and logical registration are separate operations. Physical creation is journaled atomically by ZeroFS (`DiskJournalBlockV2`). Logical registration is performed by `workspaced`.

---

## 6. DIRECTORY MEMBERSHIP vs WORKSPACE MEMBERSHIP

ZeroOS maintains a strict architectural separation between **Physical Directory Membership** and **Logical Workspace Membership**:

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         MEMBERSHIP LAYER SEPARATION                             │
├──────────────────────────────────┬──────────────────────────────────────────────┤
│ Physical Directory Membership    │ Logical Workspace Membership                 │
├──────────────────────────────────┼──────────────────────────────────────────────┤
│ - Defined by POSIX directory tree│ - Defined by workspaced Context Graph        │
│   (/storage/workspaces/ws_a/...) │   (/workspaces/<ws_id>/context.graph)        │
│ - Expressed as DiskDirEntry      │ - Expressed as MEMBER_OF(ws_id, obj_id) edge │
│ - Dictates physical path lookup  │ - Dictates logical search & spatial context  │
│ - Mutated via SYS_FILE_RENAME    │ - Mutated via workspaced IPC API             │
└──────────────────────────────────┴──────────────────────────────────────────────┘
```

$$\mathbf{Invariant\ I-WS-MEMBERSHIP-NOT-PATH:}$$
$$\text{Adding or removing a logical } \text{MEMBER_OF} \text{ edge in } \texttt{workspaced} \text{ DOES NOT alter physical filesystem paths.}$$
$$\text{Physical directory moves execute via } \texttt{SYS\_FILE\_RENAME} \text{ and update both layers simultaneously.}$$

---

## 7. SAME-DIRECTORY RENAME SEMANTICS

When an application renames a file within the same directory (`/dirA/old.txt` $\to$ `/dirA/new.txt`):

```text
Application Call: libzero::rename(dir_handle, "old.txt", dir_handle, "new.txt")
            │
            ▼
Kernel Syscall: SYS_FILE_RENAME(src_dir_h, "old.txt", dst_dir_h, "new.txt", flags=0) [Opcode 34]
            │
            ├── 1. resolve_directory_inode validates dir_handle for cap_rights::MUTATE (0x8000)
            ├── 2. Acquires parent dir_inode lock
            ├── 3. DirectoryManager::rename updates directory entry text in-place
            ├── 4. Increments parent dir_inode.generation once
            ├── 5. DiskJournalBlockV2 WAL logs Intent ──► Committed
            └── 6. Returns SyscallError::Success (0)
            │
            ▼
Registry / Context Result:
  - ObjectId                 : UNCHANGED
  - DiskInode.inode_num      : UNCHANGED
  - Parent Container / WS    : UNCHANGED
  - Canonical POSIX Path     : UPDATED (/dirA/new.txt)
```

---

## 8. CROSS-DIRECTORY RENAME SEMANTICS

When a file is moved between different directories (`/dirA/file.txt` $\to$ `/dirB/file.txt`):

```text
Application Call: libzero::rename(src_dir_h, "file.txt", dst_dir_h, "file.txt")
            │
            ▼
Kernel Syscall: SYS_FILE_RENAME(src_dir_h, "file.txt", dst_dir_h, "file.txt", flags=0) [Opcode 34]
            │
            ├── 1. resolve_directory_inode validates both handles for cap_rights::MUTATE
            ├── 2. Enforces Ascending Lock Ordering: min(src_inode, dst_inode) ──► max(src_inode, dst_inode)
            ├── 3. Removes entry slot from src_dir; inserts entry slot in dst_dir
            ├── 4. Increments BOTH src_dir.generation and dst_dir.generation
            ├── 5. DiskJournalBlockV2 WAL logs dual-slot Intent ──► Committed
            └── 6. Returns SyscallError::Success (0)
            │
            ▼
Registry / Context Result:
  - ObjectId                 : UNCHANGED
  - DiskInode.inode_num      : UNCHANGED
  - Source Membership        : REMOVED (if moving across workspace root boundaries)
  - Destination Membership   : ESTABLISHED
  - Canonical POSIX Path     : UPDATED (/dirB/file.txt)
```

---

## 9. RENAME-OVER SEMANTICS

When `rename(src, dst)` replaces an existing target file (`/dirA/source.txt` $\to$ `/dirB/target.txt` where `/dirB/target.txt` exists):

```text
Before Mutation:
  Source File A (/dirA/source.txt) ──► inode 42 ──► ObjectId_A
  Target File B (/dirB/target.txt) ──► inode 99 ──► ObjectId_B

Kernel Execution (SYS_FILE_RENAME Opcode 34):
  1. Validates cap_rights::MUTATE on both directory handles.
  2. Acquires locks in ascending order: min(src_inode, dst_inode) ──► max(src_inode, dst_inode).
  3. Decrements Target inode 99 links_count.
     ├── IF active open handles exist: Sets INODE_FLAG_PENDING_DELETE (0x0001) on inode 99.
     └── IF no open handles exist: Frees inode 99 and data blocks immediately via free_inode.
  4. Overwrites destination slot in dirB with inode 42.
  5. Zeroes source slot in dirA.
  6. Increments BOTH dirA.generation and dirB.generation.
  7. DiskJournalBlockV2 WAL commits dual-slot, dual-inode images atomically.

Object & Membership Registry Result:
  - Source ObjectId_A        : SURVIVES at path /dirB/target.txt (linked to inode 42).
  - Target ObjectId_B        : TOMBSTONED in /storage/system/object_id.registry.
  - Target Storage          : Reclaimed immediately or when active handles close (PENDING_DELETE).
```

---

## 10. UNLINK & DELETION LIFECYCLE

Unlinking a file maps directly to REV3 Syscall 33 (`SYS_FILE_UNLINK`):

```text
Application Call: libzero::unlink(parent_dir_h, "file.txt")
            │
            ▼
Kernel Syscall: SYS_FILE_UNLINK(parent_dir_h, "file.txt") [Opcode 33]
            │
            ├── 1. resolve_directory_inode validates parent_dir_h for cap_rights::MUTATE (0x8000)
            ├── 2. DirectoryManager::unlink zeroes directory slot (inode_num = 0)
            ├── 3. Increments parent_dir.generation
            ├── 4. Decrements target DiskInode.links_count
            │      ├── IF links_count == 0 AND open_handles > 0:
            │      │     └── Sets INODE_FLAG_PENDING_DELETE (0x0001); inode stays alive for open handles
            │      └── IF links_count == 0 AND open_handles == 0:
            │            └── Reclaims inode and data blocks immediately (free_inode + free_block)
            └── 5. DiskJournalBlockV2 WAL logs transaction ──► Committed
```

### 10.1 Object & Membership State Transition

```text
LIVE ──(SYS_FILE_UNLINK)──► UNLINKED ──(workspaced)──► TOMBSTONED ──(Handles Close)──► RECLAIMED
  │                           │                            │                            │
  ├── Dir Slot Valid          ├── Dir Slot Zeroed          ├── ObjectId Tombstoned      ├── Inode Freed
  ├── Inode links >= 1        ├── Invisible to readdir     ├── MEMBER_OF Edge Removed   ├── Blocks Freed
  └── Registry Active         └── links_count == 0         └── Un-resolvable by Path    └── Storage Reused
```

- **Directory Visibility**: The slot is zeroed immediately, making the file **invisible** to `SYS_DIR_READ` and path lookups.
- **`INODE_FLAG_PENDING_DELETE`**: Existing process handles can continue reading/writing until closed.
- **Storage Reclamation**: Block storage is reclaimed when the final process handle closes or upon volume mount recovery.

---

## 11. GLOBAL OBJECT REGISTRY SEMANTICS

REV8 defines `/storage/system/object_id.registry` as an **Authoritative Logical Index** owned authoritatively by Ring 3 `workspaced`.

```text
Global Volume Registry (/storage/system/object_id.registry)
  ├── Maps 128-bit ObjectId ──────────► Physical (device_id, inode_num)
  ├── Maps 128-bit ObjectId ──────────► Canonical POSIX Path (/storage/workspaces/<id>/...)
  └── Primary Role                   : Authoritative Logical Mapping & Resolution Index
```

### 11.1 Substrate Coupling & Boot Reconciliation
1. **Single-Chunk Registry Writes**: `workspaced` writes to `/storage/system/object_id.registry` in 32 KiB chunks. Writes fitting within a single 32 KiB chunk are journaled atomically by ZeroFS (`I-STOR-JOURNAL-1`).
2. **Multi-Chunk Registry Writes**: Updates spanning multiple chunks are **`BEST-EFFORT ON CRASH`**.
3. **ZeroFS Sovereignty**: ZeroFS physical block storage is sovereign for physical file existence. If a crash occurs during a multi-chunk registry update, ZeroFS physical file state prevails.
4. **Boot Reconciliation**: Upon system boot, `workspaced` enumerates ZeroFS directory trees using `SYS_DIR_READ` (Opcode 35), inspects `dir_inode.generation` counters, and reconciles `/storage/system/object_id.registry` to match ZeroFS physical reality.

---

## 12. CRASH CONSISTENCY ANALYSIS

ZeroFS physical WAL atomicity (`DiskJournalBlockV2`) is strictly decoupled from Ring 3 `workspaced` logical registry atomicity.

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                         CRASH CONSISTENCY CASE ANALYSIS                         │
├───────────────────┬──────────────────────────────────┬──────────────────────────┤
│ Scenario          │ Physical ZeroFS State            │ Logical Registry State   │
├───────────────────┼──────────────────────────────────┼──────────────────────────┤
│ Case A:           │ Committed (DiskJournalBlockV2    │ Updated                  │
│ Normal Commit     │ state = Committed)               │ (Synchronized)           │
├───────────────────┼──────────────────────────────────┼──────────────────────────┤
│ Case B:           │ Rolled Back to pre-mutation state│ Unchanged                │
│ Crash before WAL  │ (Journal state = Intent)         │ (Synchronized)           │
├───────────────────┼──────────────────────────────────┼──────────────────────────┤
│ Case C:           │ Rolled Forward to post-mutation  │ Stale on media           │
│ Crash after WAL,  │ state (Journal state = Committed)│ (Reconciled on boot via  │
│ before Registry   │                                  │ SYS_DIR_READ scan)       │
├───────────────────┼──────────────────────────────────┼──────────────────────────┤
│ Case D: Reboot    │ Sovereign physical reality       │ Reconciled to match      │
│ during Reconcile  │ preserved                        │ ZeroFS physical reality  │
└───────────────────┴──────────────────────────────────┴──────────────────────────┘
```

$$\mathbf{Principle\ of\ Physical\ Sovereignty:}$$
$$\text{ZeroFS physical block storage is the single sovereign source of truth for physical file existence.}$$
$$\text{Logical registry metadata in } \texttt{workspaced} \text{ is eventually consistent and reconciles to physical ZeroFS state on boot.}$$

---

## 13. EVENT & HISTORY MODEL

The `workspaced` Context EventLog is an append-only Ring 3 audit log of workspace metadata graph mutations. It is strictly separate from the ZeroFS physical WAL (`DiskJournalBlockV2`).

```rust
#[repr(C)]
pub struct ContextEventRecord {
    pub sequence_no: u64,              // Monotonic sequence number within workspace
    pub timestamp: QualifiedTimestamp, // Qualified boot epoch timestamp (mount_count + ticks)
    pub event_type: u16,              // 1=NodeAdd, 2=NodeRemove, 3=EdgeAdd, 4=ProvisionalLapsed
    pub target_object_id: DistributedId,// Target ObjectId
    pub workspace_id: DistributedId,  // Workspace context ID
}
```

| Event Type | Physical Trigger | Semantic Event | Atomic with ZeroFS? | Event Log Storage |
|---|---|---|---|---|
| **File Creation** | `SYS_FILE_OPEN` (`O_CREATE`) | `NodeAdd` | NO (Eventually Consistent) | `/workspaces/<id>/context.graph` |
| **Directory Creation** | `SYS_DIR_CREATE` (32) | `NodeAdd` | NO (Eventually Consistent) | `/workspaces/<id>/context.graph` |
| **File Unlink** | `SYS_FILE_UNLINK` (33) | `NodeRemove` | NO (Eventually Consistent) | `/workspaces/<id>/context.graph` |
| **File Rename** | `SYS_FILE_RENAME` (34) | `PathUpdate` | NO (Eventually Consistent) | `/workspaces/<id>/context.graph` |
| **Cross-Dir Move** | `SYS_FILE_RENAME` (34) | `EdgeAdd/Remove` | NO (Eventually Consistent) | `/workspaces/<id>/context.graph` |
| **Rename-Over** | `SYS_FILE_RENAME` (34) | `Superceded` | NO (Eventually Consistent) | `/workspaces/<id>/context.graph` |

---

## 14. CAUSAL ATTRIBUTION MODEL

REV8 maintains the decoupled attribution hierarchy established in REV7:

```text
1. WRITER_AUTHORITY   ──► Process holds Workspace Capability Handle C_ws (Bit 15 MUTATE right)
2. WRITER_PROCESS     ──► PID executing physical SYS_FILE_WRITE (Recorded in DiskInode.creator_pid)
3. WORKLOAD_CAUSALITY ──► Specific Stage 4C WorkloadId/TaskId executing task DAG
4. WORKSPACE_ATTR     ──► Final workspace assignment (DETERMINISTIC, PROVISIONAL, AMBIGUOUS)
```

- **Kernel Capability Role**: Holding Bit 15 `cap_rights::MUTATE` (`0x8000`) authorizes the kernel to execute directory mutations (`SYS_DIR_CREATE`, `SYS_FILE_UNLINK`, `SYS_FILE_RENAME`). Capability authorization proves process authority (`WRITER_AUTHORITY`), but does NOT automatically convey high-level human or agent intent (`WORKLOAD_CAUSALITY`). High-level attribution is recorded by Ring 3 daemons (`workloadd`, `uids`, `agentd`).

---

## 15. WORKSPACE BOUNDARIES & CONTAINMENT

Workspace containment is enforced across three distinct system layers:

```text
1. Physical Directory Layer : ZeroFS directory subtrees (/storage/workspaces/<ws_id>/)
2. Kernel Capability Layer  : Handle validation in resolve_directory_inode (cap_rights::MUTATE)
3. Workspace Context Layer  : Logical graph edges in /workspaces/<ws_id>/context.graph owned by workspaced
```

- **Access Enforcement**: Processes spawned within `Workspace A` are granted capability handles scoped strictly to `/storage/workspaces/ws_a/`. Cross-workspace file access requires explicit capability delegation via `brokerd` (`sys_cap_derive`).

---

## 16. APPLICATION & LEGACY OBJECT CLASSIFICATION

ZeroOS categorizes runtime objects into four distinct architectural tiers:

```text
1. Native ZeroOS Objects   : First-class DistributedId objects managed via workspaced IPC.
2. Filesystem-Backed Objects: Standard ZeroFS files/directories backing a logical ObjectId.
3. Projected Objects        : Read-only VFS directory projections into workspace views.
4. Legacy Application State : Files created by unmodified POSIX binaries via standard syscalls.
```

- **Legacy Compatibility**: Unmodified POSIX applications interact with ZeroFS using standard path syscalls (`SYS_FILE_OPEN`, `SYS_FILE_READ`, `SYS_FILE_WRITE`, `SYS_FILE_CLOSE`). They do NOT require `ObjectId` awareness. `workspaced` discovers newly created files via `SYS_DIR_READ` directory scanning and registers `ObjectId`s best-effort.

---

## 17. FAILURE & RECOVERY MATRIX

| Operation | Filesystem Result | Object / Membership Result | Crash & Reboot Recovery |
|---|---|---|---|
| **`create`** | Inode allocated, entry added to parent dir | `ObjectId` registered, `MEMBER_OF` edge added | ZeroFS WAL rolls forward/back; `workspaced` reconciles registry on boot via `SYS_DIR_READ`. |
| **`mkdir`** | Dir inode allocated, entry added to parent dir | `ObjectId` registered, container node added | ZeroFS WAL rolls forward/back; `workspaced` reconciles registry on boot. |
| **`unlink`** | Slot zeroed, links_count decremented | `ObjectId` tombstoned, `MEMBER_OF` edge removed | Slot zeroed immediately; storage freed when open handles reach 0 or on boot recovery. |
| **Same-Dir `rename`**| Entry text updated, parent generation bumped once | `ObjectId` unchanged, path reference updated | ZeroFS V2 WAL rolls forward/back atomically; `workspaced` updates path index. |
| **Cross-Dir `rename`**| Entry moved, both parent generations bumped | `ObjectId` unchanged, container edge updated | Dual-slot V2 WAL rolls forward/back atomically; `workspaced` updates container edge. |
| **`rename-over`** | Target unlinked, source entry installed, both gen bumped | Target `ObjectId` tombstoned, Source `ObjectId` survives | Dual-slot V2 WAL rolls forward/back; target storage reclaimed when handles close. |

---

## 18. CLAIM / EVIDENCE MATRIX

| ID | REV8 Architectural Claim | REV3 Substrate Primitive | Repository Source Evidence | Status |
|---|---|---|---|---|
| **C1** | Directory Creation Syscall | `SYS_DIR_CREATE` (Opcode 32) | `kernel/src/syscall/dispatch.rs:1410` | **VERIFIED** |
| **C2** | File Unlink Syscall | `SYS_FILE_UNLINK` (Opcode 33) | `kernel/src/syscall/dispatch.rs:1490` | **VERIFIED** |
| **C3** | Atomic Rename Syscall | `SYS_FILE_RENAME` (Opcode 34) | `kernel/src/syscall/dispatch.rs:1530` | **VERIFIED** |
| **C4** | Directory Read Syscall | `SYS_DIR_READ` (Opcode 35) | `kernel/src/syscall/dispatch.rs:1599` | **VERIFIED** |
| **C5** | Bit 15 Mutation Right | `cap_rights::MUTATE` | `kernel/src/cap/types.rs:58` | **VERIFIED** |
| **C6** | Dual-Slot WAL Recovery | `DiskJournalBlockV2` | `kernel/src/fs/types.rs:346` | **VERIFIED** |
| **C7** | Dir Modification Counter | `dir_inode.generation` | `kernel/src/fs/types.rs:184` | **VERIFIED** |
| **C8** | Concurrency Error ABI | Policy A Mismatch (`-EBUSY`) | `kernel/src/fs/dir.rs:370` | **VERIFIED** |
| **C9** | Strict Handle Containment | `resolve_directory_inode` | `kernel/src/syscall/dispatch.rs:1339` | **VERIFIED** |
| **C10**| Global `ObjectId` Registry | Ring 3 `workspaced` | `/storage/system/object_id.registry` | **SUPPORTED** |
| **C11**| Workspace Context Graph | Ring 3 `workspaced` | `/workspaces/<id>/context.graph` | **SUPPORTED** |
| **C12**| Boot Mount Count Epoch | `DiskSuperblock.mount_count` | `kernel/src/fs/types.rs:88` | **VERIFIED** |
| **C13**| Historical Data Versioning | N/A | Excluded from MVP Contract | **EXCLUDED** |

---

## 19. ARCHITECTURAL AUTHORITY HIERARCHY

```text
┌─────────────────────────────────────────────────────────┐
│ Human / Agent Intent (uids / agentd)                    │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│ Workspace Context & Attribution (workspaced / workloadd)│
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│ Object Identity & Relationships (object_id.registry)    │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│ REV3 Filesystem Mutation Substrate (Syscalls 32..35)    │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│ ZeroFS Physical Block Storage (Inodes & Journal V2 WAL) │
└─────────────────────────────────────────────────────────┘
```

---

## 20. IMPLEMENTATION STATUS

```text
REV8 ARCHITECTURE:
FROZEN / READY FOR IMPLEMENTATION

REV3 FILESYSTEM DEPENDENCY:
FROZEN & VERIFIED

SUBSTRATE CODE MODIFICATIONS REQUIRED:
0 (All kernel primitives implemented & verified)

WORKSPACED DAEMON IMPLEMENTATION:
NOT STARTED (Next Phase)
```

---

## 21. FINAL VERDICT

```text
FINAL VERDICT: 🟢 READY FOR IMPLEMENTATION

EXPLANATION:
All obsolete filesystem substrate assumptions from REV7 have been removed. Every Object & Membership
filesystem interaction is now precisely bound to the implemented and verified REV3 kernel primitives
(SYS_DIR_CREATE, SYS_FILE_UNLINK, SYS_FILE_RENAME, SYS_DIR_READ, cap_rights::MUTATE = 0x8000,
DiskJournalBlockV2, dir_inode.generation, and resolve_directory_inode). The architecture is frozen
and ready for Ring 3 workspaced daemon implementation.
```
