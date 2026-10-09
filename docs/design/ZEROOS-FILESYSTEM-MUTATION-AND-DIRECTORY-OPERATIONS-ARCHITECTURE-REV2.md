# ZeroOS Filesystem Mutation and Directory Operations Architecture (REV2)

**Status**: DRAFT — PENDING ADVERSARIAL REVIEW  
**Date**: October 7, 2026  
**Author**: Antigravity Core Systems & Filesystem Architecture Team  
**Target Document**: `ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV2.md`  
**Prerequisite Context**: [`ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-FORENSIC-AUDIT.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-FORENSIC-AUDIT.md)

---

## 1. Primary Goal & Architectural Scope

This document specifies **REV2** of the ZeroOS Filesystem Mutation and Directory Operations Architecture. It establishes the minimal, capability-native, crash-consistent filesystem substrate amendment required by ZeroOS to support directory creation, deletion, renaming, directory enumeration, atomic write-replace, and workspace containment.

### 1.1 Established Repository Forensic Facts
This specification directly incorporates the empirical findings established in [`ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-FORENSIC-AUDIT.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-FILESYSTEM-MUTATION-SUBSTRATE-FORENSIC-AUDIT.md):

1. `DirectoryManager` in `kernel/src/fs/dir.rs` contains `insert` and `unlink`. There is no existing `rename` or `rename_entry` function in the kernel.
2. REV1's proposed syscall opcodes 17–20 collide with existing frozen Stage 3L device syscalls (`SYS_DEV_QUERY` through `SYS_DEV_RESET`).
3. `DiskJournalBlock` currently contains fields for only ONE directory entry copy (`dir_entry_copy`) and ONE target inode image pair (`old_inode_image`, `new_inode_image`); it physically cannot log multi-object atomic renames or rename-over replacements.
4. REV1's proposed capability right `CAP_RIGHT_MUTATE = 0x04` collides with existing Stage 3H capability rights (`SHM_MAP_READ`, `DEV_CONTROL`, `NET_ACCEPT`).
5. `dispatch_file_open` in `dispatch.rs` currently hardcodes non-zero directory handle resolution to `ROOT_DIR_INODE` rather than mapping the handle to its actual directory inode.
6. Ring 3 currently lacks a capability-authorized directory mutation ABI.
7. Stage 3K, Stage 3H, and Stage 3I require controlled architectural amendments.
8. Object & Membership (REV7) remains unfrozen.
9. Implementation is prohibited until REV2 receives formal approval.

### 1.2 Boundary of Responsibility
The Filesystem Substrate strictly owns:
- Physical directory structure and 64-byte `DiskDirEntry` slot management
- Inode allocation, link counting, and data block lifecycle
- Atomic directory mutation primitives (`insert`, `unlink`, `rename`)
- Capability authority validation and subtree containment
- Write-Ahead Logging (WAL) journal transaction integrity and crash recovery
- Concurrent lock management and block buffer synchronization

The Filesystem Substrate **DOES NOT OWN**:
- `WorkspaceId`, `ObjectId`, workspace graphs, or membership logic
- User intent, workload causality, or provenance policy
- Higher-level UI, search indexing, or background indexing daemons

---

## 2. Baseline Status & Amendment Declaration

REV2 explicitly declares the baseline status across all Stage 3 architectural specs:

```text
Stage 3A–3G: FROZEN / UNMODIFIED
Stage 3H:    FROZEN BASELINE + PROPOSED AMENDMENT (Bit 15 Generic Right: MUTATE)
Stage 3I:    FROZEN BASELINE + PROPOSED AMENDMENT (Syscall Opcodes 32..35)
Stage 3J:    FROZEN / UNMODIFIED
Stage 3K:    FROZEN BASELINE + PROPOSED AMENDMENT (Journal Schema V2 & Kernel Rename)
Stage 3L–3N: FROZEN / UNMODIFIED
```

---

## 3. Evaluation of Architectural Options

We evaluate three potential structural options for delivering directory mutation capability to ZeroOS:

### 3.1 Option A — Kernel Mutation Primitives + Syscalls (Selected)
Expose capability-authorized directory mutation via four new Stage 3I syscalls (`SYS_DIR_CREATE`, `SYS_FILE_UNLINK`, `SYS_FILE_RENAME`, `SYS_DIR_READ`) backed by extended Stage 3K kernel primitives and a dual-slot journal schema.

- **Security**: Strongest. Capability checks and handle-to-inode resolution occur inside the kernel VFS.
- **Crash Consistency**: Deterministic. Operations commit via single-transaction Write-Ahead Logging.
- **Verification**: High machine verifiability; clear pre- and post-conditions.

### 3.2 Option B — Pure Ring 3 Filesystem Authority Service (Rejected)
Keep directory structure and mutation logic in a Ring 3 daemon (`fileserv`), using the kernel as a raw block or inode provider.

- **Fatal Flaw**: ZeroFS directory blocks (`DiskDirEntry`), inode bitmaps, and WAL journaling are sovereign to kernel Layer 2/4. A Ring 3 service cannot mutate directory blocks directly without either exposing raw disk blocks to Ring 3 (destroying security) or causing catastrophic filesystem corruption if the service crashes mid-mutation.

### 3.3 Option C — Hybrid Transact Ring (Rejected)
Kernel provides a raw `SYS_FS_TRANSACT` syscall accepting a multi-op transaction buffer.

- **Fatal Flaw**: Excessive ABI complexity, difficult machine proof validation, unnecessary buffer parsing overhead.

### 3.4 Selected Architecture
**Option A** is selected as the sole architecture capable of satisfying security, capability containment, and crash consistency invariants.

---

## 4. Required Operations Placement Surface

| Operation Primitive | Semantic Scope | Placement Layer | Authority Requirement | Kernel Target Routine |
|---|---|---|---|---|
| **`OPEN`** | Acquire file handle | Syscall Opcode 11 | `FILE_READ` / `FILE_WRITE` | `vfs_open` / `DirectoryManager::lookup` |
| **`CREATE_FILE`** | Instantiate file in directory | Syscall Opcode 11 (`O_CREATE`) | Parent `FILE_WRITE` | `DirectoryManager::insert` |
| **`READ`** | Read file bytes / raw dir | Syscall Opcode 12 | File `FILE_READ` | `FileManager::read` |
| **`WRITE`** | Modify file payload | Syscall Opcode 13 | File `FILE_WRITE` | `FileManager::write` |
| **`CLOSE`** | Release descriptor handle | Syscall Opcode 14 | Valid Handle | `FileManager::free_storage_object` |
| **`STAT`** | Fetch metadata struct | Syscall Opcode 15 | File `FILE_READ` | `FileManager::stat` |
| **`SYNC`** | Flush dirty data/inode | Syscall Opcode 16 | File `FILE_WRITE` | `FileManager::sync` / `Journal::clear` |
| **`CREATE_DIR`** | Allocate directory inode | **Syscall Opcode 32** | Parent `MUTATE` (`0x8000`) | `DirectoryManager::create_dir` (New) |
| **`UNLINK`** | Remove dir entry / delete | **Syscall Opcode 33** | Parent `MUTATE` (`0x8000`) | `DirectoryManager::unlink` |
| **`RENAME`** | Move entry (Same/Cross) | **Syscall Opcode 34** | Src & Dst `MUTATE` | `DirectoryManager::rename` (New) |
| **`RENAME_OVER`** | Atomic overwrite file | **Syscall Opcode 34** | Src & Dst `MUTATE` | `DirectoryManager::rename` (New) |
| **`READ_DIR`** | Structured dir enumeration | **Syscall Opcode 35** | Dir `FILE_READ` | `DirectoryManager::read_entries` (New) |

---

## 5. Rename Architecture & First-Principles Design

Because `rename` is completely absent from the kernel source, REV2 designs `DirectoryManager::rename` in kernel Layer 4 from first principles.

### 5.1 Kernel Signature (`kernel/src/fs/dir.rs`)
```rust
pub fn rename(
    device_id: u8,
    src_dir_inode_num: u32,
    src_name: &[u8],
    dst_dir_inode_num: u32,
    dst_name: &[u8],
    flags: u32, // RENAME_EXCL (0x01)
    caller_pid: u64,
) -> Result<(), FsError>
```

### 5.2 Case Execution Workflows

#### Case A: Same-Directory Rename (`dirA/A -> dirA/B`)
1. Lookup `src_name` in `dirA`. Obtain `src_inode_num`, `src_type`, `src_slot`.
2. Lookup `dst_name` in `dirA`.
   - If `dst_name` exists and `flags & RENAME_EXCL != 0`, return `Err(FsError::AlreadyExists)`.
   - If `dst_name` does not exist: Find empty slot `dst_slot` in `dirA`. Write `DiskDirEntry` for `B` pointing to `src_inode_num`. Zero out `src_slot` entry `A`.
3. Construct `DiskJournalBlockV2` transaction logging both slot mutations. Commit to journal.

#### Case B: Cross-Directory Rename (`dirA/A -> dirB/B`)
1. Validate `src_dir_inode_num` and `dst_dir_inode_num` reside on the same `device_id`.
2. Lookup `src_name` in `dirA` (`src_slot`, `src_inode_num`).
3. Lookup `dst_name` in `dirB`. Ensure `dst_name` does not exist (or handle replacement in Case C).
4. Insert `DiskDirEntry` `B` pointing to `src_inode_num` in `dirB` (`dst_slot`).
5. Zero out `DiskDirEntry` `A` in `dirA` (`src_slot`).
6. Construct `DiskJournalBlockV2` transaction logging `src_dir` slot zeroing, `dst_dir` slot writing, and `src_inode` mtime update. Commit to journal.

#### Case C: Rename-Over Replacement (`dirA/A -> dirB/existing_B`)
1. Lookup `src_name` in `dirA` (`src_slot`, `src_inode_num`).
2. Lookup `dst_name` in `dirB` (`dst_slot`, `dst_inode_num`).
3. Verifies target types: If `src` is file and `dst` is directory (or vice versa), return `Err(FsError::InvalidArgument)`.
4. Decrement `dst_inode` link count (`links_count -= 1`).
   - If `dst_inode.links_count == 0` AND no open handles exist in `STORAGE_OBJECT_TABLE`: Reclaim `dst_inode` blocks immediately.
   - If `dst_inode.links_count == 0` AND open handles exist: Set `INODE_FLAG_PENDING_DELETE` on `dst_inode`.
5. Overwrite `dst_slot` in `dirB` to point to `src_inode_num`.
6. Zero out `src_slot` in `dirA`.
7. Construct `DiskJournalBlockV2` transaction logging BOTH directory slots and BOTH inode states. Commit to journal.

---

## 6. Journal Architecture & Schema V2 Amendment

To resolve the forensic blocker where `DiskJournalBlock` could log only ONE directory slot and ONE inode image, REV2 introduces **`DiskJournalBlockV2`**.

### 6.1 `DiskJournalBlockV2` On-Disk Layout (4096 bytes)

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskJournalBlockV2 {
    pub magic: u64,                      // 0x0000..0x0008: ASCII "ZERO_JR2" (0x5A45524F5F4A5232)
    pub sequence: u64,                   // 0x0008..0x0010: Monotonic transaction sequence
    pub format_version: u32,             // 0x0010..0x0014: 2
    pub op_type: u32,                    // 0x0014..0x0018: 1=Create, 2=Write, 3=Truncate, 4=Delete, 5=Rename
    pub state: u32,                      // 0x0018..0x001C: 0=Free, 1=Intent, 2=Committed
    pub flags: u32,                      // 0x001C..0x0020: Transaction flags (0x01 = Dual Slot)
    
    // Primary / Source Slot Mutation
    pub src_parent_dir_inode: u32,       // 0x0020..0x0024: Source parent dir inode
    pub src_dir_entry_slot: u32,         // 0x0024..0x0028: Source dir slot index (0xFFFF_FFFF if none)
    pub src_target_inode_num: u32,       // 0x0028..0x002C: Source target inode index
    pub _pad0: u32,                      // 0x002C..0x0030: Padding
    
    // Secondary / Destination Slot Mutation (For Rename / Rename-Over)
    pub dst_parent_dir_inode: u32,       // 0x0030..0x0034: Destination parent dir inode
    pub dst_dir_entry_slot: u32,         // 0x0034..0x0038: Destination dir slot index
    pub dst_target_inode_num: u32,       // 0x0038..0x003C: Destination target inode index (or 0)
    pub _pad1: u32,                      // 0x003C..0x0040: Padding
    
    // Block Allocation / Free tracking
    pub allocated_blocks_count: u32,     // 0x0040..0x0044: Count (0..=10)
    pub freed_blocks_count: u32,         // 0x0044..0x0048: Count (0..=10)
    pub allocated_blocks: [u32; 10],     // 0x0048..0x0070: LBAs
    pub freed_blocks: [u32; 10],         // 0x0070..0x0098: LBAs
    
    // Directory Entry Snapshots (64 bytes each)
    pub src_dir_entry_copy: DiskDirEntry,// 0x0098..0x00D8: Pre/Post snapshot of src dir slot
    pub dst_dir_entry_copy: DiskDirEntry,// 0x00D8..0x0118: Pre/Post snapshot of dst dir slot
    
    // Inode Image Snapshots (256 bytes each)
    pub src_old_inode_image: DiskInode,  // 0x0118..0x0218: Pre-tx src inode
    pub src_new_inode_image: DiskInode,  // 0x0218..0x0318: Post-tx src inode
    pub dst_old_inode_image: DiskInode,  // 0x0318..0x0418: Pre-tx dst target inode
    pub dst_new_inode_image: DiskInode,  // 0x0418..0x0518: Post-tx dst target inode
    
    pub checksum: u32,                   // 0x0518..0x051C: CRC32 over bytes 0x0000..0x0518 (1304 B)
    pub _reserved: [u8; 2788],           // 0x051C..0x1000: Zero-padding to exact 4096 bytes
}

const _: () = assert!(core::mem::size_of::<DiskJournalBlockV2>() == 4096);
```

### 6.2 Journal Footprint & Recovery Proof
- **Size**: Exactly 4096 bytes (1 block). Fits inside the dedicated `JOURNAL_BLOCK` (Block 2).
- **Recovery Logic (`Journal::recover`)**:
  - Validates `format_version == 2` and `checksum`.
  - If `state == Intent` (1): Rollback reinstates `src_old_inode_image`, `dst_old_inode_image`, `src_dir_entry_copy`, `dst_dir_entry_copy`, and deallocates `allocated_blocks`.
  - If `state == Committed` (2): Rollforward applies `src_new_inode_image`, `dst_new_inode_image`, updates both directory slots on disk, and reconciles block bitmaps.
- **Result**: Multi-slot rename and rename-over operations achieve 100% crash atomicity.

---

## 7. Atomic Write-Replace Protocol

The Object & Membership model requires crash-atomic replacement of metadata files (`context.graph`, `object_id.registry`).

### 7.1 Protocol Sequence (`libzero`)
```text
1. File::create(dir_cap, "context.graph.tmp") ──► Returns tmp_handle
2. File::write(tmp_handle, payload_bytes)
3. File::sync(tmp_handle)                       ──► Dirty data flushed to disk
4. sys_file_rename(dir_cap, "context.graph.tmp", 
                        dir_cap, "context.graph", 0) ──► Single DiskJournalBlockV2 Commit
5. File::close(tmp_handle)
```

### 7.2 Post-Crash Visibility Proof
- **Crash before Step 4 Commit**: Recovery restores `context.graph` pointing to original inode. `context.graph.tmp` remains uncommitted or cleaned up.
- **Crash after Step 4 Commit**: Recovery rollforward sets `context.graph` pointing to new payload inode. Original `context.graph` inode link count drops to 0 and is reclaimed.
- **Guarantee**: Readers NEVER observe empty, truncated, or torn files.

---

## 8. Stage 3H Capability Architecture Amendment

To resolve the forensic bit collision (`CAP_RIGHT_MUTATE = 0x04` colliding with `SHM_MAP_READ`), REV2 defines the mutation right in the unallocated Generic Management Rights range.

### 8.1 Right Definition Amendment (`kernel/src/cap/types.rs`)
```rust
pub mod cap_rights {
    // Generic Management Rights (Bits 8..15)
    pub const DUPLICATE: u16 = 1 << 8;  // 0x0100
    pub const TRANSFER:  u16 = 1 << 9;  // 0x0200
    pub const REVOKE:    u16 = 1 << 10; // 0x0400
    pub const CLOSE:     u16 = 1 << 11; // 0x0800
    pub const INSPECT:   u16 = 1 << 12; // 0x1000
    pub const AUDIT:     u16 = 1 << 13; // 0x2000
    pub const NET_RAW:   u16 = 1 << 14; // 0x4000
    
    // NEW AMENDMENT: Bit 15 Generic Directory Mutation Right
    pub const MUTATE:    u16 = 1 << 15; // 0x8000: Directory mutation authority (create, unlink, rename)
}
```

### 8.2 Attenuation & Capability Invariants
- `MUTATE` (`0x8000`) is a generic management right applicable to directory capabilities.
- `cap_rights::is_rights_subset(child, parent)` enforces that derived handles cannot gain `MUTATE` if the parent handle lacks `MUTATE`.
- Passing a handle lacking `MUTATE` to `SYS_DIR_CREATE`, `SYS_FILE_UNLINK`, or `SYS_FILE_RENAME` returns `Err(SyscallError::PermissionDenied)`.

---

## 9. Directory Capability Containment & Dispatcher Fix

To resolve the forensic defect where `dispatch_file_open` hardcoded directory handle resolution to `ROOT_DIR_INODE` (1), REV2 specifies mandatory dispatcher handle resolution logic.

### 9.1 Authoritative Directory Resolution Function
```rust
fn resolve_directory_inode(
    pslot: u8,
    dir_handle: Handle,
    required_right: u16,
) -> Result<u32, SyscallError> {
    if dir_handle.0 == 0 {
        return Ok(crate::fs::types::ROOT_DIR_INODE);
    }
    
    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let (obj_idx, rights, _) = match unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, dir_handle, required_right)
    } {
        Ok(v) => v,
        Err(e) => {
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(ipc_to_syscall_err(e));
        }
    };
    
    let pool_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    
    crate::fs::file::STORAGE_OBJECT_TABLE_LOCK.acquire();
    let (device_id, inode_num, occupied) = unsafe {
        let s = &crate::fs::file::STORAGE_OBJECT_TABLE[pool_idx];
        (s.device_id, s.inode_num, s.occupied)
    };
    crate::fs::file::STORAGE_OBJECT_TABLE_LOCK.release();
    
    if !occupied {
        return Err(SyscallError::BadHandle);
    }
    
    // Verify target inode is a directory
    let inode = crate::fs::InodeManager::read_inode(device_id, inode_num)
        .map_err(fs_to_syscall_err)?;
    if inode.file_type != (crate::fs::types::InodeType::Directory as u16) {
        return Err(SyscallError::NotADirectory);
    }
    
    Ok(inode_num)
}
```

### 9.2 Containment Security Guarantee
1. `DirectoryManager::validate_filename` rejects names containing `/`, `\`, `.`, or `..`.
2. Path resolution operates strictly relative to `resolve_directory_inode`.
3. A process possessing a capability handle for `/workspaces/ws_alpha/` cannot specify filenames containing `..` to escape its capability root directory.

---

## 10. Directory Enumeration Architecture

REV2 adopts **Option B (Directory Generation + Cursor Entry Offset)** for structured directory enumeration.

### 10.1 Syscall ABI Signature (Opcode 35)
```rust
pub fn sys_dir_read(
    dir_handle: Handle,
    entry_offset: u32,
    buf_ptr: *mut UserDirEntry,
    max_entries: usize,
    out_count_ptr: *mut u64,
) -> Result<i64, SyscallError>
```

### 10.2 Structured Directory Entry Record (64 bytes)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UserDirEntry {
    pub inode_number: u32,      // Target inode index
    pub file_type: u8,           // 1=Regular, 2=Directory
    pub name_len: u8,            // Length 1..55
    pub reserved: u16,
    pub name: [u8; 56],          // Null-terminated filename
}
```

### 10.3 Semantics & Concurrency Safety
- Kernel locks `dir_inode`, scans directory slots starting from `entry_offset`.
- Filters out zeroed/unlinked slots (`inode_num == 0`).
- Copies up to `max_entries` active records into `buf_ptr`.
- Writes the count of returned entries to `out_count_ptr`.
- Returns `Ok(0)` when `entry_offset` reaches end of directory block.
- Thread-safe and stateless: `workspaced` maintains its own iteration index.

---

## 11. Concurrency & Lock Hierarchy

To eliminate race conditions and deadlocks during concurrent multi-directory mutations (`SYS_FILE_RENAME`), kernel operations adhere to a strict ordered locking discipline:

```text
FILESYSTEM_LOCK (Global Spinlock)
  │
  ├── 1. Resolve & validate parent directory handles
  ├── 2. Order directory inode locking: min(src_dir_inode, dst_dir_inode) -> max(src_dir_inode, dst_dir_inode)
  ├── 3. Execute DirectoryManager mutation & log to TxJournal
  └── 4. Release FILESYSTEM_LOCK
```

---

## 12. Open Handle & Pending Delete Lifecycle

```text
1. File unlinked via SYS_FILE_UNLINK or SYS_FILE_RENAME (Rename-over)
      │
      ├── Directory entry removed from parent directory block
      ├── Inode links_count decremented (links_count -= 1)
      │
      ├── IF links_count == 0 AND open_references > 0:
      │     └── Inode marked INODE_FLAG_PENDING_DELETE (0x04)
      │         Data blocks remain allocated for active readers/writers
      │
      └── WHEN final StorageObject handle closes (free_storage_object):
            └── InodeManager::truncate frees data blocks to bitmap
                InodeManager::free_inode clears inode bitmap bit
```

---

## 13. Workspace Containment & Authority Boundary

```text
Stage 3H Capability Authority (Kernel Enforced)
      │  └── Validates CapHandle, MUTATE right, & dir_inode
      ▼
Stage 3I Syscall ABI (Kernel Enforced)
      │  └── SYS_DIR_CREATE, SYS_FILE_UNLINK, SYS_FILE_RENAME
      ▼
Stage 3K Filesystem Engine (Kernel Enforced)
      │  └── ZeroFS Inodes, Directory Blocks, & TxJournal
      ▼
Stage 4 workspaced (Ring 3 Service)
      │  └── Maps WorkspaceId <-> Workspace Root Dir Handle
      ▼
Object & Membership Model (Ring 3 Service)
         └── Manages logical ObjectId registry & context.graph
```

**Boundary Invariant**: The kernel filesystem engine does NOT store or check `WorkspaceId` or `ObjectId`. Higher-level workspace containment is achieved by `workspaced` holding root directory capability handles and delegating attenuated sub-directory handles to workloads.

---

## 14. Object & Membership Integration (Physical Event Stream)

Kernel filesystem mutations emit non-blocking physical kernel events to an in-memory ring buffer:

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum FsKernelEvent {
    Created { dir_inode: u32, inode: u32, name: [u8; 56] },
    Unlinked { dir_inode: u32, inode: u32, name: [u8; 56] },
    Renamed { src_dir: u32, dst_dir: u32, src_name: [u8; 56], dst_name: [u8; 56], inode: u32 },
}
```

`workspaced` reads `FsKernelEvent` records via IPC to keep `object_id.registry` updated in Ring 3 without polluting kernel space with logical object concepts.

---

## 15. Write Attribution Boundary

- **Kernel Fact**: Inode stores `creator_pid: u64` set during `SYS_FILE_OPEN(O_CREATE)`.
- **System Fact**: Process ID (`PID`) of caller is recorded in audit logs.
- **Higher-Layer Causality**: Attribution to specific `WorkloadId` or user intent is maintained in Ring 3 by `workloadd` and `workspaced`.

---

## 16. Legacy POSIX Compatibility Layer (`libzero`)

`libzero` exposes POSIX-compatible C wrappers for standard software:

```rust
pub fn mkdir(path: *const c_char, mode: mode_t) -> c_int;
pub fn unlink(path: *const c_char) -> c_int;
pub fn rename(old_path: *const c_char, new_path: *const c_char) -> c_int;
pub fn readdir(dirp: *mut DIR) -> *mut dirent;
```

These functions map path strings to directory handles via `libzero` handle resolution tables and invoke Syscalls 32..35.

---

## 17. Failure & Crash Recovery Matrix

| Operation | Crash Point | On-Disk State | Recovery Action (`Journal::recover`) | User-Visible Result |
|---|---|---|---|---|
| `SYS_DIR_CREATE` | Before Intent log | Unchanged | No action | Directory does not exist |
| `SYS_DIR_CREATE` | After Intent log, before Commit | Intent in journal | Rollback clears allocated inode bit | Directory does not exist |
| `SYS_DIR_CREATE` | After Commit point | Committed in journal | Rollforward writes dir inode & parent entry | Directory exists cleanly |
| `SYS_FILE_UNLINK` | After Commit point | Committed in journal | Rollforward unlinks entry & updates link count | Entry removed cleanly |
| `SYS_FILE_RENAME` | After Commit point | Committed in journal | Rollforward updates src & dst dir slots | Rename completed atomically |
| `SYS_FILE_RENAME` (Replace)| After Commit point | Committed in journal | Rollforward updates dst slot & unlinks target | Target replaced atomically |
| **Write-Replace Protocol**| Crash at rename commit | Intent/Committed log | Rollforward completes atomic rename | Target has complete new content |

---

## 18. Stage 3I Syscall ABI Namespace Specification

To prevent opcode collisions with Stage 3L Device (`17..21`) and Stage 3M Networking (`22..31`) syscalls, directory mutation syscalls are assigned opcodes in the unallocated **32..35** range:

```rust
// Stage 3I Syscall Opcode Additions (kernel/src/syscall/numbers.rs)
pub const SYS_DIR_CREATE:  u64 = 32; // Opcode 0x20
pub const SYS_FILE_UNLINK: u64 = 33; // Opcode 0x21
pub const SYS_FILE_RENAME: u64 = 34; // Opcode 0x22
pub const SYS_DIR_READ:    u64 = 35; // Opcode 0x23
```

---

## 19. Backward Compatibility & Migration

- **On-Disk Format**: Superblock magic `ZERO_FS\0` and version 1 remain unchanged. Existing volume structures are fully preserved.
- **Journal Versioning**: Journal magic `ZERO_JR2` (`format_version = 2`) is recognized by `Journal::recover`. Legacy `ZERO_JRN` (`format_version = 1`) records remain fully recoverable by fallback logic.
- **Syscall ABI**: Existing Syscalls 1..31 remain 100% ABI-compatible.

---

## 20. Enforceable Architectural Invariants

1. **`I-FS-CAPABILITY-AUTHORITY`**: No directory mutation can execute without `MUTATE` (`0x8000`) right on the directory capability handle.
2. **`I-FS-DIRECTORY-CONTAINMENT`**: Path lookups resolve strictly relative to validated directory capability handles; `..` traversal above capability root is forbidden.
3. **`I-FS-RENAME-ATOMICITY`**: File rename and cross-directory move execute as a single atomic journal commit via `DiskJournalBlockV2`.
4. **`I-FS-RENAME-OVER-ATOMICITY`**: Renaming over an existing file replaces the destination entry and unlinks the target inode in a single atomic transaction.
5. **`I-FS-JOURNAL-CONSISTENCY`**: `Journal::recover` restores filesystem metadata to a consistent state following sudden power failure.
6. **`I-FS-NO-ORPHAN-LEAK`**: Unlinking an open file defers data block reclamation until all open handles close.
7. **`I-FS-PENDING-DELETE`**: Inodes marked `INODE_FLAG_PENDING_DELETE` are reclaimed during `free_storage_object` or post-boot superblock sweep.
8. **`I-FS-DIRECTORY-ENUMERATION`**: `SYS_DIR_READ` provides thread-safe entry streaming without index corruption during concurrent unlinks.
9. **`I-FS-CONCURRENT-MUTATION`**: Multi-directory operations acquire directory locks in ascending inode order, preventing deadlocks.
10. **`I-FS-WORKSPACE-SEPARATION`**: Kernel filesystem code enforces capability handles and remains free of logical `WorkspaceId` concepts.
11. **`I-FS-NO-OBJECTID-AUTHORITY`**: The kernel does not store or manage logical `ObjectId` records.

---

## 21. Minimality Test

- **Is `DiskJournalBlockV2` necessary?** Yes. Forensic audit proved `DiskJournalBlock` V1 cannot log multi-slot renames.
- **Is `MUTATE = 0x8000` necessary?** Yes. Bit 2 (`0x0004`) was already occupied by `SHM_MAP_READ`.
- **Are Opcodes 32..35 necessary?** Yes. Opcodes 17..20 were already occupied by Stage 3L device syscalls.
- **Is `resolve_directory_inode` necessary?** Yes. Forensic audit proved directory handle resolution was stubbed to root inode.

---

## 22. Dependency Direction

```text
Stage 3H Capability Authority
        ↓
Stage 3I Syscall ABI (Opcodes 32..35)
        ↓
Stage 3K Filesystem Engine (DirectoryManager & Journal V2)
        ↓
Stage 4 workspaced (Ring 3 Service)
        ↓
Object & Membership Model (Ring 3 Service)
```

---

```text
FILESYSTEM MUTATION & DIRECTORY OPERATIONS REV2

STATUS:
DRAFT — PENDING ADVERSARIAL REVIEW

IMPLEMENTATION:
NOT STARTED

STAGE3A–3G:
FROZEN / UNMODIFIED

STAGE3H:
FROZEN BASELINE + PROPOSED AMENDMENT

STAGE3I:
FROZEN BASELINE + PROPOSED AMENDMENT

STAGE3J:
FROZEN / UNMODIFIED

STAGE3K:
FROZEN BASELINE + PROPOSED AMENDMENT

STAGE3L–3N:
FROZEN / UNMODIFIED

OBJECT & MEMBERSHIP:
NOT FROZEN

NEXT:
EXTERNAL ADVERSARIAL REVIEW
```
