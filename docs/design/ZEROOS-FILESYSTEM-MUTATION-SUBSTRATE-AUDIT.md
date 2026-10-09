# ZeroOS Filesystem Mutation Substrate Forensic Audit

**Audit Date**: October 7, 2026  
**Auditor**: Antigravity Core Architectural Verification Team  
**Subject**: Stage 3K Frozen Filesystem Substrate vs. Object & Membership Model (REV7) Requirements  
**Target Document**: `ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-AUDIT.md`  
**Status**: 🔴 ARCHITECTURAL BLOCKER CONFIRMED — CURRENT SUBSTRATE INSUFFICIENT

---

## 1. Audit Scope

This document presents a rigorous, empirical forensic audit of the ZeroOS codebase (`kernel/src/`, `libzero/src/`, `services/`) and frozen architectural specification documents (`STAGE3K-ARCHITECTURE-REV5.md`, `STAGE4D-IMPLEMENTATION.md`, `STAGE4B-ARCHITECTURE-REV12.md`, `STAGE3H-ARCHITECTURE-REV6.md`, `STAGE3I-ARCHITECTURE-REV4.md`) against the substrate assumptions made in `ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV7.md`.

### Central Forensic Question
> **Does the current ZeroOS repository actually provide a usable Ring 3 filesystem mutation substrate for the operations required by the Object & Membership model?**

### Audit Mandate & Constraints
- **NO Code Modifications**: No code, kernel drivers, syscalls, IPC interfaces, or daemons have been implemented or modified.
- **NO Architecture Modifications**: REV7 remains unchanged; REV8 has NOT been created.
- **NO Speculative Claims**: Operations are classified strictly as `VERIFIED`, `PARTIALLY VERIFIED`, `UNPROVEN`, or `ABSENT` based on repository source code and frozen specifications.

---

## 2. Actual Filesystem Stack

Forensic inspection of `kernel/src/syscall/dispatch.rs`, `kernel/src/fs/`, `libzero/src/`, and `services/` reveals the exact current physical stack:

```text
Ring 3 Application / Workspaced
      ↓
libzero (libzero/src/fs.rs)
      ↓ [Direct Syscall / Int 0x80]
Syscall Entry Point (kernel/src/syscall/dispatch.rs)
      ↓
Kernel VFS Layer (kernel/src/fs/vfs.rs)
      ↓
ZeroFS Core Engine (kernel/src/fs/zerofs.rs & directory.rs)
      ↓
Block Device Layer / ATA Driver (kernel/src/drivers/ata.rs)
```

### Layer Mapping Table

| Layer | Component | Source File / Location | Actual Exposed API | Caller Authority | Substrate Status |
|---|---|---|---|---|---|
| **Ring 3 API** | `libzero::fs` | [`libzero/src/fs.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/libzero/src/fs.rs) | `open`, `read`, `write`, `close`, `stat`, `sync` | Ring 3 Task | 🟡 Partial Primitive Surface |
| **IPC Layer** | User-Space IPC | N/A | No filesystem IPC server/daemon exists | N/A | 🔴 ABSENT |
| **Syscall Interface** | Kernel Dispatcher | [`kernel/src/syscall/dispatch.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/syscall/dispatch.rs#L45-L120) | Opcodes 11..16 (`SYS_FILE_*`) | Ring 3 Task with Capability | 🟢 VERIFIED (6 syscalls only) |
| **Kernel VFS** | VFS Layer 3 | [`kernel/src/fs/vfs.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/fs/vfs.rs) | `vfs_open`, `vfs_read`, `vfs_write`, `vfs_close`, `vfs_stat`, `vfs_sync` | Syscall Handler | 🟢 VERIFIED |
| **Kernel Directory Manager** | ZeroFS Layer 4 | [`kernel/src/fs/directory.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/fs/directory.rs) | `lookup`, `create_entry`, `remove_entry`, `rename_entry` | Ring 0 Kernel Internals ONLY | 🔴 Kernel-Only (No Syscall Wrapper) |
| **ZeroFS Disk Engine** | ZeroFS Layer 2 | [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/STAGE3K-ARCHITECTURE-REV5.md#L300-L450) | Block allocation, Journal, Inode tables | Ring 0 Driver | 🟢 VERIFIED |

---

## 3. Syscall Surface

Syscall opcodes are defined in [`kernel/src/syscall/numbers.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/syscall/numbers.rs) and dispatched in [`kernel/src/syscall/dispatch.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/syscall/dispatch.rs#L45-L120).

### Complete Frozen Syscall Table (Filesystem-related)

| Syscall Opcode | Name | Signature / Arguments | Functionality / Scope |
|---|---|---|---|
| `11` (`0x0B`) | `SYS_FILE_OPEN` | `path: *const u8, path_len: usize, flags: u32, cap: CapHandle -> Result<FileHandle>` | Opens file; creates file if `O_CREATE` set and parent exists |
| `12` (`0x0C`) | `SYS_FILE_READ` | `handle: FileHandle, buf: *mut u8, len: usize -> Result<usize>` | Reads file content or raw 64-byte `DiskDirEntry` structs |
| `13` (`0x0D`) | `SYS_FILE_WRITE` | `handle: FileHandle, buf: *const u8, len: usize -> Result<usize>` | Writes byte payload to regular file |
| `14` (`0x0E`) | `SYS_FILE_CLOSE` | `handle: FileHandle -> Result<()>` | Flushes handle state and releases file handle |
| `15` (`0x0F`) | `SYS_FILE_STAT` | `handle: FileHandle, stat_out: *mut UserFileStat -> Result<()>` | Returns 32-byte stat struct (`size`, `blocks`, `type`, `gen`, `mtime`) |
| `16` (`0x10`) | `SYS_FILE_SYNC` | `handle: FileHandle -> Result<()>` | Commits dirty data/inode blocks to block storage/journal |

### Findings
1. **Opcodes 17+**: Syscalls for `rename`, `unlink`, `mkdir`, `rmdir`, `symlink`, or `link` **DO NOT EXIST** in the kernel syscall dispatch table.
2. **Capability Verification**: `SYS_FILE_OPEN` requires a valid `C_file` or parent directory capability handle.

---

## 4. Ring 3 / libzero Surface

Forensic inspection of [`libzero/src/fs.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/libzero/src/fs.rs) confirms:

- **Exposed Functions**: `File::open()`, `File::create()`, `File::read()`, `File::write()`, `File::sync()`, `File::stat()`, `File::close()`.
- **Absent Wrappers**: Zero wrappers exist for `rename()`, `unlink()`, `remove()`, `mkdir()`, or `readdir()`.
- **Raw Int 0x80 Calls**: `libzero` issues raw system call instructions directly targeting syscall numbers 11 through 16.

---

## 5. Directory Mutation Audit

- **File Creation inside Existing Directory**: `VERIFIED`. `SYS_FILE_OPEN(flags = O_CREATE)` invokes kernel `DirectoryManager::create_entry()`, which allocates a raw inode and inserts a 64-byte `DiskDirEntry` into the target directory if the parent directory exists.
- **Directory Creation (`mkdir`)**: 🔴 **ABSENT FROM RING 3 SUBSTRATE**.
  - `DirectoryManager` in `kernel/src/fs/directory.rs` contains no `mkdir` primitive.
  - Directories can only be pre-formatted into the superblock during initial disk image generation or instantiated directly by Ring 0 kernel initialization.

---

## 6. Rename / Rename-Over Audit

REV7 relies on atomic renames (`rename(temp, target)`) for eventlog compaction and object metadata updates.

- **Kernel Internal**: [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/STAGE3K-ARCHITECTURE-REV5.md#L410-L460) defines `DirectoryManager::rename_entry(parent_inode, old_name, new_name)`.
- **Ring 3 Substrate Path**: 🔴 **ABSENT**.
  - No syscall number exists for rename.
  - `libzero` has no `rename()` API.
  - No IPC opcode exists to delegate rename to a server.
- **Rename-Over Behavior**: Unproven and unimplemented even in kernel internals; `rename_entry` fails with `AlreadyExists` if the target entry exists, requiring an explicit unlink pass.

---

## 7. Delete / Unlink Audit

REV7 object lifecycle operations assume object deletion unlinks directory entries (`unlink /storage/system/...`).

- **Kernel Internal**: `DirectoryManager::remove_entry(parent_inode, name)` exists in kernel Layer 4.
- **Ring 3 Substrate Path**: 🔴 **ABSENT**.
  - No `SYS_FILE_UNLINK` or `SYS_FILE_REMOVE` syscall exists.
  - `libzero` exposes no deletion function.
  - Ring 3 applications (including `workspaced`) **CANNOT** delete any file or directory.

---

## 8. Directory Enumeration Audit

REV7 assumes `workspaced` can scan `/storage/system/` and `/workspaces/<id>/` during boot reconciliation.

- **Syscall Mechanism**: `SYS_FILE_OPEN` can open a directory inode. `SYS_FILE_READ` on a directory handle reads sequential 64-byte `DiskDirEntry` records directly from disk blocks.
- **Data Format**:
  ```rust
  #[repr(C)]
  pub struct DiskDirEntry {
      pub inode_number: u32,
      pub entry_type: u8,
      pub name_len: u8,
      pub reserved: u16,
      pub name: [u8; 56],
  }
  ```
- **Substrate Status**: 🟡 **PARTIALLY VERIFIED**.
  - Direct byte-level reading via `SYS_FILE_READ` is physically possible.
  - `libzero` lacks an iterator (`readdir`) wrapper, requiring caller applications to manually parse the binary 64-byte `DiskDirEntry` array.

---

## 9. Atomic Write-Replace Audit

REV7 specifies:
```text
Write temp file -> SYS_FILE_SYNC -> atomic rename(temp, target) -> SYS_FILE_SYNC
```

- **Forensic Assessment**: 🔴 **ABSENT FROM RING 3 SUBSTRATE**.
  - Because `rename` is missing from the Ring 3 ABI, Ring 3 applications cannot perform atomic write-replace operations.
  - If a file exists, it can only be overwritten in-place using `SYS_FILE_WRITE`, exposing readers to torn writes during crashes.

---

## 10. EventLog Compaction Audit

REV7 specifies compacting `context.graph` by creating `context.graph.tmp` and replacing `context.graph`.

- **Forensic Assessment**: 🔴 **NOT PROVIDED BY CURRENT SUBSTRATE**.
  - Stage 3K transaction journal records (`TxRecord`) log individual block allocations and inode updates.
  - The journal does NOT support multi-entry directory atomic replacements across distinct filenames from Ring 3.

---

## 11. Capability / Authority Audit

- **Authority Requirement**: Mutating directory contents requires parent directory write authority (`C_dir_write`).
- **Capability Granularity**:
  - `SYS_FILE_OPEN` checks capability handles passed in registers.
  - Ring 3 tasks possess individual process capabilities (`C_task`).
  - No capability delegation mechanism exists for dynamic kernel-managed directory creation.

---

## 12. Workspaced Implementation Audit

Forensic examination of `services/workspaced/`:

1. **Object Registry Creation**: `UNPROVEN`. `workspaced` can issue `SYS_FILE_OPEN(O_CREATE)` to create `/storage/system/object_id.registry` ONLY IF `/storage/system/` pre-exists on the disk image.
2. **Directory Creation**: `ABSENT`. Cannot create `/workspaces/<id>/`.
3. **Directory Scanning**: `PARTIALLY VERIFIED`. Can read raw directory bytes if target directory is pre-opened.
4. **Rename / Unlink / Atomic Replace**: `ABSENT`. Cannot perform any metadata updates requiring file deletion or renaming.

---

## 13. Boot Epoch Audit

Forensic inspection of boot temporal sequencing:

- **Kernel Mount Sequence**: `STAGE3K-ARCHITECTURE-REV5.md` specifies that during `ZeroFS::mount_volume()`, the kernel reads Block 1 (`DiskSuperblock`), increments `mount_count` (offset `0x54..0x5C`), and synchronously flushes the updated superblock to disk.
- **Authority**: `DiskSuperblock.mount_count` is persisted and enforced by Ring 0 kernel initialization prior to mounting Ring 3 processes.
- **Ring 3 Visibility**: `BootEpochSequence: u64` is published in the read-only kernel shared page (`KUSER_SHARED_DATA`).
- **Verdict**: 🟢 **VERIFIED**. Boot Epoch ordering is physically backed by kernel superblock persistence.

---

## 14. Write Attribution Audit

Forensic inspection of write tracking:

- **Inode Level**: `DiskInode` stores `creator_pid: u32` at offset `0x18..0x20` set during `O_CREATE`.
- **Data Block Level**: `SYS_FILE_WRITE` modifies file payload blocks. Individual data blocks do NOT record writer PIDs or TaskIDs.
- **Process Level**: `workloadd` maintains `PID -> (WorkspaceId, TaskId)` mapping in memory.
- **Verdict**: 🟡 **PARTIALLY VERIFIED**. File creation attribution (`creator_pid`) is stored in the disk inode. Subsequent mutations overwrite block data without updating writer PID metadata.

---

## 15. Complete Operation Matrix

| Operation | Ring 3 API | libzero API | IPC | Syscall | Kernel Implementation | Capability Required | Actually Callable? | Crash Guarantee |
|---|---|---|---|---|---|---|---|---|
| **Open** | `File::open` | `fs::open` | Direct Syscall | `SYS_FILE_OPEN` (11) | `vfs_open` / `lookup` | `C_file` / `C_dir` | 🟢 VERIFIED | Inode read-only |
| **Create File** | `File::create` | `fs::create` | Direct Syscall | `SYS_FILE_OPEN` (11, `O_CREATE`) | `DirectoryManager::create_entry` | Parent `C_dir_write` | 🟢 VERIFIED (in existing dir) | Journaled Inode/Dir |
| **Read File** | `File::read` | `fs::read` | Direct Syscall | `SYS_FILE_READ` (12) | `vfs_read` | `C_file_read` | 🟢 VERIFIED | Data read |
| **Write File** | `File::write` | `fs::write` | Direct Syscall | `SYS_FILE_WRITE` (13) | `vfs_write` | `C_file_write` | 🟢 VERIFIED (in-place only) | Block dirty state |
| **Sync** | `File::sync` | `fs::sync` | Direct Syscall | `SYS_FILE_SYNC` (16) | `vfs_sync` / `TxJournal::commit` | `C_file_write` | 🟢 VERIFIED | Block/Inode flushed |
| **Readdir** | `ABSENT` | `ABSENT` | Direct Syscall | `SYS_FILE_READ` (12) | Direct byte read of `DiskDirEntry` | Directory Handle | 🟡 PARTIALLY VERIFIED (raw bytes) | Read-only |
| **Mkdir** | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `N/A` | 🔴 ABSENT | None |
| **Unlink** | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `DirectoryManager::remove_entry` (Ring 0) | `N/A` | 🔴 ABSENT | None |
| **Rename** | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `DirectoryManager::rename_entry` (Ring 0) | `N/A` | 🔴 ABSENT | None |
| **Rename-over** | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `N/A` | 🔴 ABSENT | None |
| **Delete** | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `DirectoryManager::remove_entry` (Ring 0) | `N/A` | 🔴 ABSENT | None |
| **Atomic Replace** | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `ABSENT` | `N/A` | 🔴 ABSENT | None |

---

## 16. Adversarial Review Reconciliation

### Claim 1: "Stage 3K Syscall ABI exposes only open/read/write/close/stat/sync."
- **Repository Evidence**: [`kernel/src/syscall/dispatch.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/syscall/dispatch.rs#L45-L120) contains handlers ONLY for opcodes 11..16.
- **Verdict**: 🟢 **VERIFIED CORRECT**.

### Claim 2: "Layer 4 DirectoryManager functions (rename_entry, remove_entry) are kernel internals with no Ring 3 path."
- **Repository Evidence**: [`kernel/src/fs/directory.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/fs/directory.rs) contains `remove_entry` and `rename_entry`. No syscall dispatcher entry or IPC handler calls them.
- **Verdict**: 🟢 **VERIFIED CORRECT**.

### Claim 3: "Ring 3 directory creation (mkdir), deletion (unlink), and renaming (rename) are missing."
- **Repository Evidence**: No syscall, libzero wrapper, or IPC opcode exists for these operations in `libzero/src/` or `kernel/src/syscall/`.
- **Verdict**: 🟢 **VERIFIED CORRECT**.

---

## 17. Genuine Missing Primitives

The forensic audit proves that the following filesystem primitives required by the Object & Membership Model (REV7) are **demonstrably absent** from the frozen Ring 3 substrate:

1. **`SYS_FILE_MKDIR` (Directory Creation)**: Ring 3 applications cannot create workspace or system directories.
2. **`SYS_FILE_UNLINK` / `SYS_FILE_REMOVE` (Directory Entry Deletion)**: Ring 3 applications cannot delete files or remove tombstoned objects.
3. **`SYS_FILE_RENAME` (Atomic File Renaming)**: Ring 3 applications cannot execute atomic write-replace (`temp -> target`) or move object files across path namespaces.

---

## 18. Final Decision

🔴 **CURRENT SUBSTRATE INSUFFICIENT**

The adversarial review finding is **TRUE**. The frozen Stage 3K filesystem substrate does NOT provide the Ring 3 filesystem mutation primitives (`mkdir`, `unlink`, `rename`, `atomic write-replace`) required by `ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV7.md`.

```text
REV7:
UNCHANGED

REV8:
NOT CREATED

IMPLEMENTATION:
NOT STARTED

STAGE 3A–3N:
UNMODIFIED

OBJECT & MEMBERSHIP FREEZE:
NOT APPROVED
```
