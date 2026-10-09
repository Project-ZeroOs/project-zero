# ZeroOS Filesystem Mutation and Directory Operations Architecture (REV1)

**Status**: DRAFT — PENDING ADVERSARIAL REVIEW  
**Date**: October 7, 2026  
**Author**: Antigravity Core Systems & Filesystem Architecture Team  
**Target Document**: `ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV1.md`  
**Prerequisite Context**: [`ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-AUDIT.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-AUDIT.md)

---

## 1. Current Substrate Baseline

This section defines the exact, verified baseline of the frozen ZeroOS Stage 3K filesystem substrate as established in [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/STAGE3K-ARCHITECTURE-REV5.md) and [`ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-AUDIT.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-AUDIT.md).

### 1.1 Existing Frozen Syscall ABI
The kernel syscall dispatcher ([`kernel/src/syscall/dispatch.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/syscall/dispatch.rs#L45-L120)) exposes exactly six filesystem syscalls:

```text
Opcode 11 (0x0B): SYS_FILE_OPEN(path_ptr, path_len, flags, cap_handle) -> Result<FileHandle>
Opcode 12 (0x0C): SYS_FILE_READ(handle, buf_ptr, buf_len)               -> Result<usize>
Opcode 13 (0x0D): SYS_FILE_WRITE(handle, buf_ptr, buf_len)              -> Result<usize>
Opcode 14 (0x0E): SYS_FILE_CLOSE(handle)                                -> Result<()>
Opcode 15 (0x0F): SYS_FILE_STAT(handle, stat_out_ptr)                  -> Result<()>
Opcode 16 (0x10): SYS_FILE_SYNC(handle)                                 -> Result<()>
```

### 1.2 Capability & Authority Model
- **No Path-Based Authority**: Raw path strings carry zero authority. All operations require a valid capability handle (`CapHandle`) registered in the calling process's capability table (`CapabilityTable`).
- **File & Directory Capabilities**: `SYS_FILE_OPEN` validates `cap_handle` against target inode/directory permissions. Passing an unauthorized capability handle yields `Err(KernelError::AccessDenied)`.
- **StorageObject Representation**: Regular files and directories are backed by on-disk `DiskInode` records (128 bytes).

### 1.3 Kernel Directory Internals
Layer 4 of the ZeroFS engine ([`kernel/src/fs/directory.rs`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/fs/directory.rs)) defines internal Ring 0 routines:
- `DirectoryManager::lookup(parent_inode, name)`
- `DirectoryManager::create_entry(parent_inode, name, inode_type)`
- `DirectoryManager::remove_entry(parent_inode, name)`
- `DirectoryManager::rename_entry(parent_inode, old_name, new_name)`

*Baseline Fact*: `create_entry` is invoked by `SYS_FILE_OPEN` when `O_CREATE` is set in flags and the target parent directory exists. `remove_entry` and `rename_entry` are pure Ring 0 functions with **zero** Ring 3 syscall or IPC access paths.

### 1.4 ZeroFS Journal & Disk Model
- **Superblock & Mount Epoch**: Block 1 stores `DiskSuperblock`. `ZeroFS::mount_volume()` increments `mount_count` (offset `0x54..0x5C`) and synchronously flushes the block during kernel boot, establishing a crash-durable `BootEpochId`.
- **Transaction Engine**: Stage 3K implements Write-Ahead Logging (WAL) over a dedicated 2 MiB journal ring (`TxJournal`).
- **Transaction Steps**:
  ```text
  Intent Record -> CoW Data Blocks -> Commit Record -> Metadata Inode/Dir Blocks -> Superblock Update -> Clear Intent
  ```
- **Transaction Limits**: Frozen Stage 3K enforces `MAX_ALLOCATED_BLOCKS_PER_TX = 8`.

### 1.5 Pending-Delete & Open Handles
- `DiskInode` maintains an in-memory `open_ref_count: u32` and on-disk `link_count: u16`.
- Unlinking an open file marks the inode as `pending_delete = true`. Data blocks remain allocated until `open_ref_count` reaches zero, at which point the inode and blocks are freed during `SYS_FILE_CLOSE`.

---

## 2. Required Operations Surface

To support a real filesystem, workspace containment, and the Object & Membership model, ZeroOS requires a defined, minimal set of filesystem operations.

### 2.1 Complete Operation Matrix Analysis

| Operation Primitive | Semantic Requirement | Required Authority | Affected Target | Journal Impact | Kernel vs Userspace Boundary |
|---|---|---|---|---|---|
| `OPEN` | Obtain handle to existing file | `C_file_read` / `C_file_write` | Inode | None (Read) | Existing Syscall 11 |
| `CREATE_FILE` | Instantiate file in existing directory | `C_dir_write` (parent) | Parent Dir + New Inode | Alloc Inode + Dir Entry (2 blks) | Existing Syscall 11 (`O_CREATE`) |
| `READ` | Read byte stream | File Handle (`Read`) | File Data Blocks | None | Existing Syscall 12 |
| `WRITE` | Modify byte stream | File Handle (`Write`) | File Data Blocks | Dirty Data Blocks | Existing Syscall 13 |
| `STAT` | Fetch metadata struct | File Handle (`Read`) | Inode | None | Existing Syscall 15 |
| `SYNC` | Flush dirty blocks to disk | File Handle (`Write`) | File + Inode + Journal | Commit Transaction | Existing Syscall 16 |
| `CREATE_DIR` | Allocate directory inode & entry | `C_dir_write` (parent) | Parent Dir + New Dir Inode | Alloc Dir Inode + Parent Entry | **Proposed Syscall 17** |
| `UNLINK` | Remove directory entry & decrement link | `C_dir_write` (parent) | Parent Dir + Target Inode | Update Dir Entry + Inode Link | **Proposed Syscall 18** |
| `RENAME` | Move entry between directories | `C_dir_src_write` + `C_dir_dst_write` | Src Dir + Dst Dir | Update 2 Dir Entries (Max 4 blks) | **Proposed Syscall 19** |
| `RENAME_OVER` | Atomic write-replace over existing file | `C_dir_src_write` + `C_dir_dst_write` | Src Dir + Dst Dir + Dst Inode | Update Dir Entries + Unlink Target | **Proposed Syscall 19** |
| `READ_DIR` | Enumerate directory entries | `C_dir_read` | Directory Inode | None | **Proposed Syscall 20** |

### 2.2 Derivative Primitives (Userspace Policy)
- `ATOMIC_REPLACE`: Implemented in `libzero` as `write(temp) -> sync() -> rename(temp, target)`.
- `MOVE` / `COPY`: Implemented in `libzero` as `rename` (if same mount) or `read -> create -> write -> sync -> unlink` (if cross-volume).
- `REMOVE_DIRECTORY`: Implemented in `libzero` as `unlink(dir)` (kernel enforces directory non-empty check).

---

## 3. Authority Location & Architectural Evaluation

We evaluate three potential architectural designs for where directory mutation authority should reside.

### 3.1 Option A: Minimal Stage 3K Syscall Extension
Expose four clean, capability-enforced syscalls (`SYS_DIR_CREATE`, `SYS_FILE_UNLINK`, `SYS_FILE_RENAME`, `SYS_DIR_READ`) directly targeting kernel Layer 4 `DirectoryManager`.

- **Pros**: Direct integration with ZeroFS journal; atomic transaction boundary enforced by kernel; zero IPC overhead; crash consistency guaranteed by kernel WAL engine.
- **Cons**: Requires adding four syscall opcodes to Stage 3K ABI.

### 3.2 Option B: Pure Ring 3 Filesystem Authority Service
Move directory metadata and mutation logic to a user-space daemon (`fileserv`), keeping the kernel as a raw block storage device or raw file provider.

- **Pros**: Keeps kernel syscall surface minimal.
- **Cons**: 🔴 **ARCHITECTURALLY FATAL**. Because ZeroFS directory structure (`DiskDirEntry`), block allocation bitmaps, and transaction journaling reside in kernel Layer 2/4, a Ring 3 daemon cannot modify directory entries without either raw block access (bypassing kernel security) or kernel primitives. If the daemon crashes mid-operation, directory state becomes corrupt without kernel WAL protection.

### 3.3 Option C: Hybrid Kernel Transaction Primitive
The kernel exposes a single low-level transaction syscall (`SYS_FS_TRANSACT`) accepting a batch of directory operation opcodes in a ring buffer.

- **Pros**: Single syscall addition.
- **Cons**: Extremely high ABI verification complexity; difficult machine proof validation; redundant buffer parsing overhead.

### 3.4 Selected Architecture: Option A (Minimal Stage 3K Syscall Extension)
Option A is selected because filesystem tree integrity, inode bitmap allocation, and journal crash consistency are fundamental kernel responsibilities in ZeroOS. Option A minimizes kernel complexity while delivering deterministic security and crash-atomicity guarantees.

---

## 4. Chosen Architecture Overview

The ZeroOS Filesystem Substrate extends the Stage 3K syscall dispatcher with four structured, capability-gated syscalls:

```text
Ring 3 Application / libzero / workspaced
      │
      ├── SYS_FILE_OPEN  (Opcode 11) ──► Existing File/Open
      ├── SYS_FILE_READ  (Opcode 12) ──► Existing File Read
      ├── SYS_FILE_WRITE (Opcode 13) ──► Existing File Write
      ├── SYS_FILE_CLOSE (Opcode 14) ──► Existing File Close
      ├── SYS_FILE_STAT  (Opcode 15) ──► Existing File Stat
      ├── SYS_FILE_SYNC  (Opcode 16) ──► Existing File Sync
      │
      ├── SYS_DIR_CREATE (Opcode 17) ──► Kernel DirectoryManager::create_entry (Directory)
      ├── SYS_FILE_UNLINK(Opcode 18) ──► Kernel DirectoryManager::remove_entry
      ├── SYS_FILE_RENAME(Opcode 19) ──► Kernel DirectoryManager::rename_entry
      └── SYS_DIR_READ   (Opcode 20) ──► Kernel DirectoryManager::read_entries
```

---

## 5. Directory Capabilities & Authority Model

### 5.1 Handle-Based Directory Authority
Raw string paths carry no ambient authority. Directory mutation operations require a valid parent directory capability handle:

```rust
pub struct DirCap {
    pub dir_inode: u32,
    pub rights: CapRights, // CAP_RIGHT_READ | CAP_RIGHT_WRITE | CAP_RIGHT_MUTATE
}
```

### 5.2 Rights & Attenuation
- `CAP_RIGHT_READ` (`0x01`): Grants permission to execute `SYS_DIR_READ` and `SYS_FILE_STAT`.
- `CAP_RIGHT_WRITE` (`0x02`): Grants permission to invoke `SYS_FILE_OPEN(O_CREATE)`.
- `CAP_RIGHT_MUTATE` (`0x04`): Grants permission to execute `SYS_DIR_CREATE`, `SYS_FILE_UNLINK`, and `SYS_FILE_RENAME`.

Passing a handle lacking `CAP_RIGHT_MUTATE` to `SYS_DIR_CREATE`, `SYS_FILE_UNLINK`, or `SYS_FILE_RENAME` returns `Err(KernelError::AccessDenied)`.

---

## 6. File Creation (`SYS_FILE_OPEN` with `O_CREATE`)

File creation within an existing directory remains handled by `SYS_FILE_OPEN` using opcode 11:

```text
SYS_FILE_OPEN(dir_cap_handle, name_ptr, name_len, flags = O_CREATE | O_RDWR) -> Result<FileHandle>
```

1. Kernel validates `dir_cap_handle` contains `CAP_RIGHT_WRITE`.
2. `DirectoryManager::lookup` checks if `name` exists in parent directory.
3. If entry exists and `O_EXCL` is set, returns `Err(KernelError::AlreadyExists)`.
4. If entry does not exist, allocates inode from ZeroFS inode bitmap, writes initial inode with `creator_pid = current_pid`, inserts `DiskDirEntry` into parent directory, logs transaction to `TxJournal`, and returns open `FileHandle`.

---

## 7. Directory Creation (`SYS_DIR_CREATE`)

### 7.1 Syscall ABI Signature (Opcode 17)
```rust
pub fn sys_dir_create(
    parent_dir_handle: Handle,
    name_ptr: *const u8,
    name_len: usize,
) -> Result<Handle, KernelError>
```

### 7.2 Semantics & Execution Sequence
1. Kernel validates `parent_dir_handle` has `CAP_RIGHT_MUTATE`.
2. Validates `name_len <= 56` and name contains no `/` or null bytes.
3. Locks parent directory inode.
4. Checks duplicate entry via `DirectoryManager::lookup`. If present, returns `Err(KernelError::AlreadyExists)`.
5. Allocates a new directory inode from ZeroFS inode bitmap. Initializes directory header with `file_type = FT_DIRECTORY`, `link_count = 1`, `size_bytes = 0`.
6. Inserts 64-byte `DiskDirEntry` into parent directory block.
7. Commits operation as a single journal transaction (`TxRecord`).
8. Instantiates and returns a new directory capability handle (`Handle`) with full `CAP_RIGHT_READ | CAP_RIGHT_WRITE | CAP_RIGHT_MUTATE`.

---

## 8. Directory Enumeration (`SYS_DIR_READ`)

### 8.1 Syscall ABI Signature (Opcode 20)
```rust
pub fn sys_dir_read(
    dir_handle: Handle,
    entry_offset: u32,
    buf_ptr: *mut UserDirEntry,
    max_entries: usize,
) -> Result<usize, KernelError>
```

### 8.2 Structured Data Record
`SYS_DIR_READ` does not return unstructured raw bytes; it copies fixed-width `UserDirEntry` structs to user-space:

```rust
#[repr(C)]
pub struct UserDirEntry {
    pub inode_number: u32,
    pub file_type: u8,       // 1 = Regular File, 2 = Directory
    pub name_len: u8,        // 1..56
    pub reserved: u16,
    pub name: [u8; 56],
}
```

### 8.3 Semantics
- Returns the number of entries successfully copied into `buf_ptr`.
- Returns `Ok(0)` when `entry_offset` reaches or exceeds total entries (End of Directory).
- Thread-safe and stateless: `entry_offset` is maintained by the Ring 3 caller (e.g., `libzero::readdir`).

---

## 9. Directory Unlink & Removal (`SYS_FILE_UNLINK`)

### 9.1 Syscall ABI Signature (Opcode 18)
```rust
pub fn sys_file_unlink(
    parent_dir_handle: Handle,
    name_ptr: *const u8,
    name_len: usize,
) -> Result<(), KernelError>
```

### 9.2 Semantics & Execution Sequence
1. Validates `parent_dir_handle` has `CAP_RIGHT_MUTATE`.
2. Looks up target entry in parent directory. If absent, returns `Err(KernelError::NotFound)`.
3. Checks target inode type:
   - If target is a directory, verifies directory is empty (contains zero active entries). If non-empty, returns `Err(KernelError::DirectoryNotEmpty)`.
4. Removes 64-byte `DiskDirEntry` from parent directory block.
5. Decrements target inode `link_count`.
6. **Pending-Delete Handling**:
   - If `link_count == 0` AND `open_ref_count == 0`: Frees data blocks to volume block bitmap, frees inode to inode bitmap.
   - If `link_count == 0` AND `open_ref_count > 0`: Marks inode `pending_delete = true`. Deferred cleanup executes when `open_ref_count` drops to zero during `SYS_FILE_CLOSE`.
7. Logs atomic entry removal and link count update to `TxJournal`.

---

## 10. Rename Architecture (`SYS_FILE_RENAME`)

### 10.1 Syscall ABI Signature (Opcode 19)
```rust
pub fn sys_file_rename(
    src_dir_handle: Handle,
    src_name_ptr: *const u8,
    src_name_len: usize,
    dst_dir_handle: Handle,
    dst_name_ptr: *const u8,
    dst_name_len: usize,
    flags: u32, // RENAME_EXCL (0x01), RENAME_SWAP (0x02)
) -> Result<(), KernelError>
```

### 10.2 Parameter Rules & Cross-Directory Boundaries
- `src_dir_handle` and `dst_dir_handle` must both possess `CAP_RIGHT_MUTATE`.
- Rename operations are restricted to directory trees within the same mounted volume. Cross-volume renames return `Err(KernelError::CrossDeviceLink)`.

---

## 11. Rename-Over & Replacement Policy

ZeroOS explicitly adopts the **DESTINATION-REPLACE-ATOMIC** policy for `SYS_FILE_RENAME` when `dst_name` pre-exists in `dst_dir`.

### 11.1 Deterministic Replacement Semantics (`rename(A, B)`)

```text
Before Rename:
  src_dir ──► DiskDirEntry("A", Inode #100)
  dst_dir ──► DiskDirEntry("B", Inode #200)

Execution (Single Journal Transaction):
  1. dst_dir Entry "B" pointer updated to Inode #100
  2. src_dir Entry "A" zeroed/removed
  3. Inode #200 link_count decremented (Pending delete if open)

After Rename:
  dst_dir ──► DiskDirEntry("B", Inode #100)  [Surviving Inode: #100]
  Inode #200 ──► Unlinked / Tombstoned
```

### 11.2 ObjectId Invariants during Rename-Over
- **Surviving Physical Identity**: Inode `#100` (Source) retains its physical identity, generation, and data blocks.
- **Tombstoned Identity**: Inode `#200` (Destination) is unlinked.
- **Atomic Replacement**: Readers accessing `B` observe either Inode `#200` (before transaction commit) or Inode `#100` (after transaction commit). Under no circumstances can `B` be missing or corrupted.

---

## 12. Atomic Write-Replace Protocol

The Object & Membership model relies on atomic file replacement (e.g., compaction of `context.graph` and registry updates).

### 12.1 Ring 3 Atomic Write Protocol (`libzero`)
```text
Step 1: File::create(dir_cap, "context.graph.tmp") ──► Returns tmp_handle
Step 2: File::write(tmp_handle, new_payload)
Step 3: File::sync(tmp_handle)                       ──► Dirty data flushed to disk
Step 4: sys_file_rename(dir_cap, "context.graph.tmp", 
                        dir_cap, "context.graph", 0) ──► Atomic Journal Commit
Step 5: File::close(tmp_handle)
```

### 12.2 Crash-Safety Guarantees
- If a crash occurs **before Step 4 commit**: On reboot, `context.graph` points to old inode payload. `context.graph.tmp` is garbage-collected or re-cleared.
- If a crash occurs **after Step 4 commit**: On reboot, `context.graph` points to new inode payload. Old target inode is completely unlinked.
- **Torn State Impossible**: Readers will NEVER observe a partially written `context.graph`.

---

## 13. ZeroFS Journal Integration & Transaction Bounds

### 13.1 Transaction Record Allocation
A complex `SYS_FILE_RENAME` (cross-directory rename-over) requires mutating:
1. Source parent directory block (remove entry `A`)
2. Destination parent directory block (update/insert entry `B`)
3. Destination target inode (decrement `link_count`)
4. Journal Commit Block

Total dirty blocks required for operation: **4 blocks**.

### 13.2 Substrate Compatibility Proof
Frozen Stage 3K enforces:
```rust
pub const MAX_ALLOCATED_BLOCKS_PER_TX: usize = 8;
```
Because the maximum mutation operation (`SYS_FILE_RENAME` with replacement across different directories) requires only **4 blocks**, all proposed filesystem mutations fit strictly within the frozen Stage 3K transaction bounds **without requiring any amendment to journal limits**.

---

## 14. Concurrency & Lock Hierarchy

To prevent lock inversion and deadlocks during concurrent multi-directory renames and unlinks, the kernel enforces a strict global lock hierarchy:

```text
VolumeLock
  └── ParentDirInodeLock (min(src_inode_id, dst_inode_id))
        └── ParentDirInodeLock (max(src_inode_id, dst_inode_id))
              └── TargetInodeLock
```

1. When acquiring locks for two directories (e.g., `SYS_FILE_RENAME`), the kernel orders acquisition strictly by ascending inode number.
2. Inode locks are held only for the duration of the in-memory mutation and journal log append.

---

## 15. Workspace Containment

### 15.1 Principle of Containment
Filesystem capabilities cannot grant access outside the subtree defined by their root directory capability.

```text
Root Workspace Capability: C_dir (/workspaces/ws_alpha/)
  │
  ├── Allowed: SYS_DIR_CREATE(C_dir, "src")           ──► /workspaces/ws_alpha/src/
  ├── Allowed: SYS_FILE_UNLINK(C_dir, "old.txt")       ──► /workspaces/ws_alpha/old.txt
  │
  └── DENIED: Attempting path traversal "../ws_beta"   ──► Err(KernelError::InvalidPath)
```

### 15.2 Invariant `I-FS-WORKSPACE-CONTAINMENT`
> **A filesystem capability derived for Workspace A (`/workspaces/ws_A/`) cannot be used to mutate, rename, or inspect any inode outside the `/workspaces/ws_A/` subtree.**

Path traversal tokens (`..`) attempting to escape the root directory of a capability handle are rejected during path resolution in `DirectoryManager::lookup`.

---

## 16. ObjectId & Event Boundary

The Filesystem Mutation Substrate provides physical storage and atomicity guarantees; it does NOT manage logical workspace identity or object graphs.

### 16.1 Observability Boundary
Kernel mutation operations emit raw, non-blocking kernel events to the kernel event ring buffer:

```rust
pub enum FsKernelEvent {
    FileCreated { parent_inode: u32, inode: u32, name: [u8; 56] },
    FileUnlinked { parent_inode: u32, inode: u32, name: [u8; 56] },
    FileRenamed { src_parent: u32, dst_parent: u32, src_name: [u8; 56], dst_name: [u8; 56], inode: u32 },
}
```

### 16.2 Logical Identity Integration
- `workspaced` subscribes to `FsKernelEvent` streams in Ring 3.
- Upon receiving `FileRenamed` or `FileUnlinked`, `workspaced` updates `object_id.registry` and `context.graph`.
- If `workspaced` crashes, boot reconciliation scans directory structures via `SYS_DIR_READ` to rebuild logical mappings.

---

## 17. Write Attribution Boundary

- **Kernel Responsibility**: The kernel records `creator_pid: u32` in `DiskInode` during `SYS_FILE_OPEN(O_CREATE)`.
- **Process Identity**: The kernel knows only the calling `PID` and `CapHandle`.
- **Workload / Identity Attribution**: Higher-level attribution (`WorkloadId`, `WorkspaceId`) is maintained exclusively by `workloadd` and `workspaced` in Ring 3. The filesystem substrate does NOT attempt to parse or enforce workload causality.

---

## 18. POSIX / Legacy Compatibility Layer

Legacy applications running on ZeroOS interact with the filesystem via standard C library (`libzero`) wrappers:

```rust
// libzero POSIX Abstraction Layer
pub fn mkdir(path: *const c_char, mode: mode_t) -> c_int {
    let (parent_handle, name) = resolve_parent_cap(path);
    match sys_dir_create(parent_handle, name.as_ptr(), name.len()) {
        Ok(_) => 0,
        Err(e) => errno_from_kernel(e),
    }
}

pub fn unlink(path: *const c_char) -> c_int {
    let (parent_handle, name) = resolve_parent_cap(path);
    match sys_file_unlink(parent_handle, name.as_ptr(), name.len()) {
        Ok(_) => 0,
        Err(e) => errno_from_kernel(e),
    }
}

pub fn rename(old_path: *const c_char, new_path: *const c_char) -> c_int {
    let (src_handle, src_name) = resolve_parent_cap(old_path);
    let (dst_handle, dst_name) = resolve_parent_cap(new_path);
    match sys_file_rename(src_handle, src_name.as_ptr(), src_name.len(),
                           dst_handle, dst_name.as_ptr(), dst_name.len(), 0) {
        Ok(_) => 0,
        Err(e) => errno_from_kernel(e),
    }
}
```

---

## 19. Security & Vulnerability Analysis

| Threat Vectors | Mitigating Architectural Mechanism |
|---|---|
| **Path Traversal (`../..`)** | `DirectoryManager` restricts component lookups to single directory child names; `..` traversal above capability root is blocked. |
| **TOCTOU Exploits** | Operations accept directory capability handles and execute lookup/mutation atomically under kernel inode locks. |
| **Capability Confusion** | `CapHandle` types are strictly checked in capability tables (`CAP_TYPE_DIRECTORY` vs `CAP_TYPE_FILE`). |
| **Cross-Workspace Renames** | `SYS_FILE_RENAME` validates both `src` and `dst` handles; operations across unauthorized workspace subtrees fail capability checks. |
| **Resource Exhaustion** | Inode and block allocation quotas are checked against volume limits before transaction commit. |

---

## 20. Failure & Crash Recovery Matrix

| Operation | Normal State | Crash Before Commit | Crash At Commit | Crash After Commit | Recovery State (`ZeroFS::mount`) |
|---|---|---|---|---|---|
| `SYS_DIR_CREATE` | Entry & Dir Inode created | Journal uncommitted | Journal replay completes | Replay skips (already done) | Directory exists; zero data loss |
| `SYS_FILE_UNLINK` | Entry removed, link decremented | Journal uncommitted; entry remains | Journal replay unlinks entry | Replay skips | Entry unlinked cleanly |
| `SYS_FILE_RENAME` | Entry moved | Journal uncommitted; src remains | Journal replay updates dst | Replay skips | Src moved to dst atomically |
| `SYS_FILE_RENAME` (Replace) | Dst overwritten, src moved | Journal uncommitted; dst intact | Journal replay completes swap | Replay skips | Dst replaced by src; old dst freed |
| `ATOMIC_REPLACE` | Temp written & renamed over target | Temp file exists; target untouched | Target replaced by temp | Temp unlinked | Target has either old OR new content |

---

## 21. ABI & Kernel Substrate Amendment Specification

To implement this architecture when approved, the Stage 3K kernel ABI will require the following exact additions:

### 21.1 New Syscall Opcodes
```rust
pub const SYS_DIR_CREATE:  usize = 17; // Opcode 0x11
pub const SYS_FILE_UNLINK: usize = 18; // Opcode 0x12
pub const SYS_FILE_RENAME: usize = 19; // Opcode 0x13
pub const SYS_DIR_READ:    usize = 20; // Opcode 0x14
```

### 21.2 Capability Rights Addition
```rust
pub const CAP_RIGHT_MUTATE: u32 = 0x04;
```

---

## 22. Architectural Invariants

The architecture enforces the following 11 formal invariants:

1. **`I-FS-MUTATION-AUTHORIZED`**: No directory mutation (`create`, `unlink`, `rename`) can execute without explicit `CAP_RIGHT_MUTATE` authority on the parent directory handle.
2. **`I-FS-DIRECTORY-CAPABILITY`**: Directory authority is conveyed strictly via `CapHandle`; path strings carry no ambient authority.
3. **`I-FS-RENAME-ATOMIC`**: File renames commit atomically via kernel WAL journal transactions.
4. **`I-FS-RENAME-OVER-DETERMINISTIC`**: Renaming over an existing file replaces the destination entry with the source inode in a single transaction.
5. **`I-FS-UNLINK-PENDING-DELETE`**: Unlinking an open file defers data block reclamation until all open handles are closed.
6. **`I-FS-ATOMIC-REPLACE`**: Write-replace protocols (`write -> sync -> rename`) guarantee that readers observe either the complete old file or the complete new file.
7. **`I-FS-CRASH-CONSISTENT`**: ZeroFS WAL journal replay restores filesystem metadata to a consistent state following sudden power loss.
8. **`I-FS-WORKSPACE-CONTAINMENT`**: Directory capabilities cannot execute mutations outside their authorized directory subtree.
9. **`I-FS-NO-PATH-AS-AUTHORITY`**: Raw path strings are strictly resolution parameters within a capability context.
10. **`I-FS-OBJECTID-EVENT-BOUNDARY`**: Filesystem mutations emit kernel events; identity and membership graphs are maintained in Ring 3.
11. **`I-FS-WRITER-IDENTITY-NOT-WORKLOAD-CAUSALITY`**: The kernel records calling process `PID` in inodes but does not infer high-level user intention.

---

## 23. Architectural Options Comparison

| Property | Option A: Stage 3K Syscall Extension (Selected) | Option B: Ring 3 Service (Rejected) | Option C: Hybrid Transact Ring (Rejected) |
|---|---|---|---|
| **Kernel Changes** | Minimal (4 Opcodes) | None (in theory) | Medium (1 Complex Opcode) |
| **Security Boundary** | Strong (Kernel capability check) | Weak (Daemon IPC spoofing risk) | Medium (Complex buffer parsing) |
| **Crash Atomicity** | Guaranteed (ZeroFS WAL Journal) | 🔴 FATAL (Unprotected daemon crashes) | Guaranteed (Kernel WAL Journal) |
| **Rename Semantics** | Atomic single-transaction | Non-atomic multi-step | Atomic multi-step buffer |
| **Workspace Containment** | Enforced by kernel VFS | Enforced by Ring 3 daemon | Enforced by kernel VFS |
| **Verification Complexity**| Low (4 clean signatures) | High (Inter-process state) | High (Complex opcode ring parser) |
| **Performance** | High (Direct syscall) | Low (IPC context switches) | Medium (Buffer serialization) |
| **ObjectId Integration** | Clean (Kernel event stream) | Clunky (Daemon-internal events)| Clean (Kernel event stream) |

---

## 24. Minimality Test

Each proposed syscall opcode was subjected to a strict removal test:

- **Can `SYS_DIR_CREATE` be removed?** No. Without it, Ring 3 cannot create directories (e.g., `/workspaces/<id>/`).
- **Can `SYS_FILE_UNLINK` be removed?** No. Without it, Ring 3 cannot delete files or remove temporary files.
- **Can `SYS_FILE_RENAME` be removed?** No. Without it, atomic write-replace (`context.graph.tmp -> context.graph`) is physically impossible.
- **Can `SYS_DIR_READ` be removed?** No. Without it, Ring 3 applications cannot enumerate directories or perform boot reconciliation.

Conclusion: The 4 proposed syscall additions represent the **absolute minimal substrate** required by ZeroOS.

---

## 25. Object & Membership Dependency Direction

The architectural dependency chain is strictly unidirectional:

```text
ZeroFS Layer 2 / Layer 4 Engine
      ↓
Stage 3K Substrate (Syscalls 11..20)
      ↓ (Provides atomic filesystem mutations & FsKernelEvents)
Object & Membership Model (workspaced / Ring 3)
      ↓ (Provides logical identity, workspace graphs, & attribution)
User Application Interface
```

The Object & Membership model depends on the Filesystem Mutation Substrate for atomic physical operations. The Filesystem Mutation Substrate does NOT depend on the Object & Membership model.

---

```text
FILESYSTEM MUTATION & DIRECTORY OPERATIONS ARCHITECTURE REV1

ARCHITECTURE:
DRAFT — PENDING ADVERSARIAL REVIEW

IMPLEMENTATION:
NOT STARTED

STAGE 3A–3N:
FROZEN / UNMODIFIED

OBJECT & MEMBERSHIP MODEL:
NOT FROZEN

NEXT:
EXTERNAL ADVERSARIAL REVIEW
```
