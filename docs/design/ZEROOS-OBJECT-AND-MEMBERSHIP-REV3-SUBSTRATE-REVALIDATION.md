# ZeroOS Object & Membership — REV3 Substrate Revalidation

**Date**: October 8, 2026  
**Status**: 🟡 TARGETED REVISION REQUIRED  
**Target Document**: `docs/design/ZEROOS-OBJECT-AND-MEMBERSHIP-REV3-SUBSTRATE-REVALIDATION.md`  
**Authoritative Substrate**: `docs/design/ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV3.md`  
**Substrate Verification**: `docs/design/ZEROOS-FILESYSTEM-MUTATION-REV3-POST-IMPLEMENTATION-VERIFICATION.md`  
**Model Under Revalidation**: `docs/design/ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV7.md`

---

## 1. Executive Verdict

Following the completion and verification of the **REV3 Filesystem Mutation Substrate** (`ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV3.md`), the Object & Membership architecture (`ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV7.md`) has undergone a comprehensive forensic revalidation against the actual implemented kernel and library code.

### Verdict: 🟡 TARGETED REVISION REQUIRED

**Justification**:
1. **Substrate Alignment**: The completed REV3 substrate provides 100% of the required physical primitives (`SYS_DIR_CREATE`, `SYS_FILE_UNLINK`, `SYS_FILE_RENAME`, `SYS_DIR_READ`, `MUTATE = 0x8000`, `DiskJournalBlockV2`, `dir_inode.generation`, ascending lock ordering).
2. **Previous Blockers Closed**: All five historical substrate blockers (missing Ring 3 rename/unlink/mkdir syscalls, missing atomic WAL recovery, silent handle 0 root fallback, missing capability right, undefined directory enumeration under concurrent mutation) are **100% RESOLVED**.
3. **Model Revision Mandate**: REV7 was written under the old unamended Stage 3K baseline (which lacked rename/unlink/mkdir syscalls) and claimed that "raw `rename` is NOT PROVIDED in kernel syscalls." Now that REV3 Syscall 34 (`SYS_FILE_RENAME`) and Syscall 35 (`SYS_DIR_READ`) are implemented in kernel code, Object & Membership requires a targeted revision (**REV8**) to update its architectural specifications to consume the frozen REV3 primitives directly.

---

## 2. Source Baseline

The revalidation is grounded in actual source code and authoritative specifications:

- **Implemented Substrate Architecture**: `docs/design/ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV3.md`
- **Substrate Audit & Verification**:
  - `docs/design/ZEROOS-FILESYSTEM-MUTATION-REV3-IMPLEMENTATION-AUDIT.md`
  - `docs/design/ZEROOS-FILESYSTEM-MUTATION-REV3-POST-IMPLEMENTATION-VERIFICATION.md`
- **Actual Kernel & Runtime Implementation**:
  - `kernel/src/cap/types.rs` (`cap_rights::MUTATE = 0x8000`)
  - `kernel/src/syscall/numbers.rs` (Opcodes 32, 33, 34, 35)
  - `kernel/src/fs/types.rs` (`DiskJournalBlockV2`, `UserDirEntry`, `DiskInode.generation`)
  - `kernel/src/fs/journal.rs` (Format discrimination `ZERO_JR2` & V2 WAL recovery)
  - `kernel/src/fs/dir.rs` (`DirectoryManager::rename`, `create_dir`, `unlink`, `read_entries`)
  - `kernel/src/syscall/dispatch.rs` (`resolve_directory_inode`, `dispatch_file_rename`, `dispatch_dir_read`)
  - `libzero/src/syscall.rs` (`sys_dir_create`, `sys_file_unlink`, `sys_file_rename`, `sys_dir_read`)
- **Evaluated Model Specification**: `docs/design/ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV7.md`

---

## 3. Object Identity Audit

### A. Identification Today
- **Kernel Substrate**: Identified by `(device_id: u8/u32, inode_num: u32)`.
- **Directory Entries**: Identified by `UserDirEntry { inode_number: u32, file_type: u8, name_len: u8, name: [u8; 56] }`.
- **Syscall Handles**: `Handle(u32)` referencing a process `StorageObjectTable` slot mapping to `(device_id, inode_num)`.

### B. Object & Membership Identification
- Identified by `ObjectId` = 128-bit `DistributedId` assigned authoritatively by Ring 3 `workspaced` and indexed in `/storage/system/object_id.registry`.

### C. Identity Survival Analysis
- **Same-Directory Rename**: **SURVIVES**. REV3 `SYS_FILE_RENAME` (opcode 34) updates directory slot text while preserving the exact `inode_num`. `workspaced` updates the path index without altering `ObjectId`.
- **Cross-Directory Rename**: **SURVIVES**. REV3 `SYS_FILE_RENAME` moves the directory entry across parent directories while preserving `inode_num`.
- **Rename-Over**:
  - *Source Object*: **SURVIVES**. Moves to target path with original `inode_num`. `ObjectId` remains intact.
  - *Target Object*: **TOMBSTONED**. Target inode is unlinked and reclaimed when open handles reach zero. Target `ObjectId` is tombstoned in registry.
- **Unlink**: **TOMBSTONED**. Slot is zeroed immediately. Target `links_count` decrements. When open handles close, storage is freed. `ObjectId` is tombstoned.
- **Close / Reopen**: **SURVIVES**. Inode persists on block device media. `workspaced` resolves `ObjectId` via `/storage/system/object_id.registry`.
- **Reboot**: **SURVIVES**. `DiskInode` is persistent. `workspaced` scans directories via `SYS_DIR_READ` and reconciles `/storage/system/object_id.registry` on boot.
- **Filesystem Recovery**: **SURVIVES**. `DiskJournalBlockV2` WAL guarantees atomic commit/rollback of rename and directory operations across crashes.

### D. Identity Comparison Matrix

| Identity Type | Scope | Lifetime | Persistent? | Reused? | Authority |
|---|---|---|---|---|---|
| **`ObjectId`** | System-Wide (Logical) | Creation to Unlink | YES (`workspaced` registry) | NO (128-bit UUID/UUIDv7) | `workspaced` (`/storage/system/object_id.registry`) |
| **`inode_num`** | Block Device Volume | Allocation to Free | YES (`DiskInode` on media) | YES (after `free_inode`) | ZeroFS Kernel (`InodeManager`) |
| **`Handle`** | Per-Process Table | `open` to `close` | NO (Volatile RAM) | YES (after handle close) | Process `StorageObjectTable` |
| **`UserDirEntry`** | Directory Slot | Slot creation to unlink | YES (`DiskDirEntry` on media) | YES (slot overwritten) | ZeroFS Directory (`DirectoryManager`) |
| **`ProcessId` (PID)** | System Task Table | Process spawn to exit | NO (Volatile RAM) | YES (after process exit) | Kernel Task Manager |
| **`WorkspaceId`** | Logical Workspace | Creation to Destroy | YES (Workspace metadata) | NO (128-bit DistributedId) | `workspaced` Context Graph |

---

## 4. Membership Model Audit

| Operation | Object Identity Effect | Membership Effect | Filesystem Primitive | Atomic? |
|---|---|---|---|---|
| **`CREATE_FILE`** | New `ObjectId` assigned in registry | Added as `Provisional` or `Confirmed` `MEMBER_OF(ws_id, obj_id)` | `SYS_FILE_OPEN` (`O_CREATE`), opcode 11 | YES (Kernel inode + dir insertion journaled) |
| **`CREATE_DIR`** | New `ObjectId` assigned in registry | Added as directory container object | `SYS_DIR_CREATE`, opcode 32 | YES (Kernel dir inode + dir insertion journaled) |
| **`UNLINK`** | `ObjectId` tombstoned in registry | `MEMBER_OF` edge removed from workspace graph | `SYS_FILE_UNLINK`, opcode 33 | YES (Kernel slot zeroed + links decremented journaled) |
| **Same-Dir `RENAME`**| `ObjectId` retained; path reference updated | Membership unchanged; path reference updated in registry | `SYS_FILE_RENAME`, opcode 34 | YES (`DiskJournalBlockV2` WAL committed) |
| **Cross-Dir `RENAME`**| `ObjectId` retained; path reference updated | Container/Workspace parent updated if moved across workspace paths | `SYS_FILE_RENAME`, opcode 34 | YES (`DiskJournalBlockV2` WAL committed) |
| **`RENAME-OVER`** | Source `ObjectId` survives; Target `ObjectId` tombstoned | Target `MEMBER_OF` edge removed; Source remains | `SYS_FILE_RENAME`, opcode 34 | YES (`DiskJournalBlockV2` dual-slot WAL committed) |

---

## 5. Rename Semantics

### 5.1 Substrate Realization
- **Same-Directory Rename**: Performed atomically in kernel via `SYS_FILE_RENAME` (opcode 34). Inode `inode_num` is unchanged. Parent directory `generation` increments once. `ObjectId` remains unchanged.
- **Cross-Directory Rename**: Performed atomically in kernel via `SYS_FILE_RENAME`. Inode `inode_num` is unchanged. Both `src_dir.generation` and `dst_dir.generation` increment. Lock ordering strictly follows `min(src, dst) -> max(src, dst)`. `ObjectId` remains unchanged while workspace container updates.
- **Rename-Over**: Performed atomically in kernel via `SYS_FILE_RENAME`. Target inode `links_count` decrements (freed if 0). Source inode replaces target in destination slot. Source `ObjectId` survives; Target `ObjectId` is tombstoned.
- **Open Destination**: Target inode enters `INODE_FLAG_PENDING_DELETE`. Unlinked from directory immediately; data blocks reclaimed when last open process handle closes.
- **Crash Atomicity**: `DiskJournalBlockV2` WAL logs both parent inodes and directory entries before commit. Recovery via `rollback_v2` (if uncommitted Intent) or `rollforward_v2` (if Committed) prevents any partial or corrupted state.

---

## 6. Unlink / Deletion Lifecycle

- **Directory Visibility**: `SYS_FILE_UNLINK` (opcode 33) zeroes the directory slot immediately. The entry becomes **invisible** to directory scanning and `SYS_DIR_READ`. Parent directory `generation` increments.
- **`INODE_FLAG_PENDING_DELETE` (`0x0001`)**:
  - If a file is unlinked while open handles exist, `links_count` drops to 0 and `INODE_FLAG_PENDING_DELETE` sets.
  - Inode remains accessible to existing handle holders until closed.
  - Data blocks are reclaimed when the final handle closes.
- **`ObjectId` Status**: As soon as `SYS_FILE_UNLINK` succeeds, `workspaced` tombstones the `ObjectId` and removes `MEMBER_OF` edges.
- **Reboot / Recovery**: Orphaned unlinked inodes with `links_count == 0` are reclaimed on volume mount.

---

## 7. Attribution

| Attribution Dimension | Substrate Primitive / Source | Recorded By Kernel? | Status |
|---|---|---|---|
| **Writer Process (PID)** | `DiskInode.creator_pid` (offset `0x18..0x20`) | YES (Recorded at inode allocation) | `VERIFIED` |
| **Capability Authority ($C_{\text{ws}}$)** | Process `CapabilityTable` & `resolve_directory_inode` (`MUTATE`) | YES (Validated on syscall entry) | `VERIFIED` |
| **Workload / Task DAG** | Stage 4C `workloadd` Task Descriptor | NO (Tracked in Ring 3 `workloadd`) | `PARTIALLY VERIFIED` |
| **Workspace Membership** | `workspaced` Context Graph (`/workspaces/<id>/context.graph`) | NO (Tracked in Ring 3 `workspaced`) | `DESIGN-LEVEL ONLY` |
| **Human Actor / User ID** | Ring 3 `uids` daemon | NO (Tracked in Ring 3 `uids`) | `DESIGN-LEVEL ONLY` |
| **Agent Identity** | Ring 3 `agentd` daemon | NO (Tracked in Ring 3 `agentd`) | `DESIGN-LEVEL ONLY` |

---

## 8. Event / History Model

| Event Type | Trigger | Persistent? | Crash Atomic? | Substrate Mechanism |
|---|---|---|---|---|
| **File Creation** | `SYS_FILE_OPEN` (`O_CREATE`) | YES | YES | Kernel Journal V2 + `workspaced` EventLog |
| **Directory Creation** | `SYS_DIR_CREATE` (32) | YES | YES | Kernel Journal V2 + `workspaced` EventLog |
| **File Unlink** | `SYS_FILE_UNLINK` (33) | YES | YES | Kernel Journal V2 + `workspaced` EventLog |
| **File Rename** | `SYS_FILE_RENAME` (34) | YES | YES | Kernel Journal V2 + `workspaced` EventLog |
| **Cross-Dir Move** | `SYS_FILE_RENAME` (34) | YES | YES | Kernel Journal V2 + `workspaced` EventLog |
| **Rename-Over** | `SYS_FILE_RENAME` (34) | YES | YES | Kernel Journal V2 + `workspaced` EventLog |
| **Membership Edge Change**| `workspaced` IPC | YES | YES | `workspaced` `/workspaces/<id>/context.graph.tmp` write-replace |

---

## 9. Path vs Object Identity

1. **Rename (`/a/file` -> `/b/file`)**:
   - `DiskInode.inode_num`: **UNCHANGED**.
   - `ObjectId`: **UNCHANGED**.
   - Canonical POSIX Path: Changes from `/a/file` to `/b/file`.
2. **Rename-Over (`/b/file` -> `/c/file` where `/c/file` exists)**:
   - Source Object: Survives at path `/c/file` with original `ObjectId`.
   - Target Object: Unlinked/reclaimed; Target `ObjectId` tombstoned.

Path is a projected directory reference. `ObjectId` represents logical entity identity.

---

## 10. Workspace Containment

- **Directory Containment**: Enforced by REV3 kernel `resolve_directory_inode`. Handled 0 does NOT fall back to root. Validates process slot, handle generation, directory `InodeType`, and `MUTATE` capability right.
- **Capability Containment**: Enforced by Stage 3H capability table. Process can only access filesystem paths reachable through held capability handles.
- **Workspace Containment**: Managed by Ring 3 `workspaced` by granting processes capability handles restricted to their designated workspace directory (`/storage/workspaces/<ws_id>/`).

---

## 11. Legacy / Browser / Application Objects

- **Native ZeroOS Object**: Managed via `workspaced` `ObjectId` and `libzero` IPC.
- **Filesystem-Backed Object**: Standard ZeroFS file/directory backing an `ObjectId`.
- **Legacy Application Objects**: Created by legacy binaries using standard POSIX syscalls (`SYS_FILE_OPEN`, `SYS_FILE_READ`, `SYS_FILE_WRITE`, `SYS_FILE_CLOSE`). `workspaced` discovers these objects via `SYS_DIR_READ` directory scanning and assigns `ObjectId`s best-effort. Legacy applications do not require `ObjectId` awareness.

---

## 12. Crash Consistency

```text
1. Normal Commit:
   - ZeroFS WAL: Committed (DiskJournalBlockV2 state = Committed).
   - Inodes & Dir Entries: Updated on media.
   - Registry: Updated by workspaced.
   - EventLog: Appended in workspaced.
   - Consistency: 100% Consistent.

2. Crash Before Commit (Intent State):
   - ZeroFS WAL: Rollback_v2 restores pre-mutation inodes & dir entries.
   - Inodes & Dir Entries: Intact at pre-mutation state.
   - Registry / EventLog: Unchanged.
   - Consistency: 100% Consistent (Pre-mutation state).

3. Crash After WAL Commit but Before Registry Flush:
   - ZeroFS WAL: Rollforward_v2 completes physical directory mutation.
   - Inodes & Dir Entries: Post-mutation state on media.
   - Registry: Stale on media (workspaced interrupted).
   - Boot Recovery: workspaced boots, scans ZeroFS via SYS_DIR_READ, detects generation bump, reconciles object_id.registry to match ZeroFS physical state.
   - Consistency: Eventually Consistent (ZeroFS is sovereign).
```

---

## 13. Claim / Evidence Matrix

| ID | Object & Membership Claim | REV3 Primitive | Actual Source | Status | Gap |
|---|---|---|---|---|---|
| **C1** | Raw Directory Creation | `SYS_DIR_CREATE` (Opcode 32) | `kernel/src/syscall/dispatch.rs:1410` | **VERIFIED** | None |
| **C2** | Raw File Unlink | `SYS_FILE_UNLINK` (Opcode 33) | `kernel/src/syscall/dispatch.rs:1490` | **VERIFIED** | None |
| **C3** | Raw Atomic Rename | `SYS_FILE_RENAME` (Opcode 34) | `kernel/src/syscall/dispatch.rs:1530` | **VERIFIED** | None |
| **C4** | Directory Enumeration | `SYS_DIR_READ` (Opcode 35) | `kernel/src/syscall/dispatch.rs:1599` | **VERIFIED** | None |
| **C5** | Bit 15 Capability Authorization | `cap_rights::MUTATE` | `kernel/src/cap/types.rs:58` | **VERIFIED** | None |
| **C6** | Dual-Slot WAL Recovery | `DiskJournalBlockV2` | `kernel/src/fs/types.rs:346` | **VERIFIED** | None |
| **C7** | Directory Modification Counter | `dir_inode.generation` | `kernel/src/fs/types.rs:184` | **VERIFIED** | None |
| **C8** | Concurrency Control | Policy A Mismatch (`-EBUSY`) | `kernel/src/fs/dir.rs:370` | **VERIFIED** | None |
| **C9** | Strict Handle Containment | `resolve_directory_inode` | `kernel/src/syscall/dispatch.rs:1339` | **VERIFIED** | None |
| **C10**| Global `ObjectId` Registry | Ring 3 `workspaced` | `/storage/system/object_id.registry` | **SUPPORTED** | `workspaced` implementation required |
| **C11**| Workspace Context Graph | Ring 3 `workspaced` | `/workspaces/<id>/context.graph` | **SUPPORTED** | `workspaced` implementation required |
| **C12**| Historical File Versioning | N/A | Removed from REV3/REV7 contract | **EXCLUDED** | Intentionally excluded from MVP |

---

## 14. Previous Blocker Resolution

```text
OLD BLOCKER 1: Missing Ring 3 Directory Mutation Syscalls
STATUS: RESOLVED (REV3 implemented SYS_DIR_CREATE, SYS_FILE_UNLINK, SYS_FILE_RENAME, SYS_DIR_READ)

OLD BLOCKER 2: Missing Kernel Atomic Rename & Journal WAL
STATUS: RESOLVED (REV3 implemented DirectoryManager::rename with DiskJournalBlockV2 dual-slot WAL)

OLD BLOCKER 3: Loose Root-Directory Handle Fallback
STATUS: RESOLVED (REV3 resolve_directory_inode strictly validates handles; NO SILENT ROOT FALLBACK)

OLD BLOCKER 4: Missing Generic Mutation Capability Right
STATUS: RESOLVED (REV3 implemented MUTATE = 0x8000)

OLD BLOCKER 5: Undefined Directory Read Concurrency Under Mutation
STATUS: RESOLVED (REV3 implemented Policy A dir_inode.generation mismatch returning SyscallError::ResourceConflict / -EBUSY)

NEW STATUS:
ALL PREVIOUS SUBSTRATE BLOCKERS ARE 100% RESOLVED BY REV3.
```

---

## 15. Remaining Architectural Gaps

While all kernel substrate blockers are closed by REV3, Object & Membership requires a targeted revision (**REV8**) to address the following model-level items:

1. **Outdated REV7 Substrate Assumptions**: REV7 states in Section 5 ("B3 Resolution") that "raw `rename` syscall is NOT PROVIDED in Stage 3K." This claim is obsolete now that REV3 Syscall 34 (`SYS_FILE_RENAME`) is implemented. REV8 must update Section 5 to directly bind `SYS_FILE_RENAME`.
2. **Observation Key Optimization**: REV7 defined an observation key based on `(BLAKE2s(Path), generation, mtime)`. REV8 can optimize directory enumeration by directly using `UserDirEntry.inode_number` returned by REV3 Syscall 35 (`SYS_DIR_READ`).
3. **`workspaced` Daemon Implementation Plan**: A concrete implementation specification for Ring 3 `workspaced` (owning `/storage/system/object_id.registry` and `/workspaces/<id>/context.graph`) must be authored following REV8 freeze.

---

## 16. Final Verdict

```text
FINAL VERDICT: 🟡 TARGETED REVISION REQUIRED

EXPLANATION:
The completed REV3 filesystem mutation substrate provides 100% of the required kernel primitives.
All previous substrate blockers are fully resolved. Object & Membership requires a targeted revision
(REV8) solely to update its architectural specifications to consume the frozen REV3 primitives directly.
```
