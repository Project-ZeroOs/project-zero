# ZeroOS Filesystem Mutation and Directory Operations Architecture (REV3)

**Status**: 🟢 FROZEN FOR IMPLEMENTATION  
**Date**: October 7, 2026  
**Author**: Antigravity Core Systems & Filesystem Architecture Team  
**Target Document**: `ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV3.md`  
**Prerequisite Context**:  
- [`ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV2.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV2.md)  
- [`ZEROOS-FILESYSTEM-MUTATION-REV2-BLOCKER-CLOSURE-AUDIT.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-FILESYSTEM-MUTATION-REV2-BLOCKER-CLOSURE-AUDIT.md)

---

## 1. Executive Summary & Scope

This specification defines **REV3** of the ZeroOS Filesystem Mutation and Directory Operations Architecture. REV3 is a surgical amendment to REV2 with the sole purpose of closing **BLK-5 (SYS_DIR_READ Concurrent Mutation Semantics)**.

All five previously closed architectural blockers remain 100% CLOSED:
- **BLK-1 (Journal V2)**: CLOSED (`DiskJournalBlockV2` dual-slot 4096-byte WAL descriptor).
- **BLK-2 (Rename Primitive)**: CLOSED (`DirectoryManager::rename` with ascending lock ordering).
- **BLK-3 (Containment & Fallback)**: CLOSED (`resolve_directory_inode` replacing root-inode fallback).
- **BLK-4 (Capability Rights)**: CLOSED (`MUTATE = 0x8000` generic management right).
- **BLK-6 (Disk Format Compatibility)**: CLOSED (Superblock v1 preserved; journal `ZERO_JR2` v2 header).

REV3 resolves BLK-5 by introducing a persistent directory modification sequence counter (`dir_inode.generation`), an explicit `SYS_DIR_READ` generation-matching ABI protocol, and **Policy A (Fail on Generation Mismatch)** to deliver deterministic, race-free directory enumeration under concurrent mutations.

---

## 2. Baseline Architecture & Amendment Declaration

```text
Stage 3A–3G: FROZEN BASELINE
Stage 3H:    FROZEN BASELINE + PROPOSED ADDITIVE AMENDMENT (Bit 15 Generic Right: MUTATE = 0x8000)
Stage 3I:    FROZEN BASELINE + PROPOSED ADDITIVE AMENDMENT (Syscall Opcodes 32..35)
Stage 3J:    FROZEN BASELINE
Stage 3K:    FROZEN BASELINE + PROPOSED ADDITIVE AMENDMENT (Journal Schema V2, Kernel Rename, & Dir Gen)
Stage 3L–3N: FROZEN BASELINE
```

---

## 3. Directory Inode Generation Specification (`dir_inode.generation`)

Source Reference: [`kernel/src/fs/types.rs:173-189`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/fs/types.rs#L173-L189)

### 3.1 Field Specification
In Stage 3K, `DiskInode` already includes an existing 32-bit field at offset `0x30..0x34`:

```rust
pub struct DiskInode {
    // ...
    pub generation: u32,  // 0x30..0x34: Inode recycling & directory modification sequence counter
    // ...
}
```

- **Field Name**: `generation`
- **Type**: `u32` (32-bit unsigned integer)
- **Initial Value**: `1` (assigned when inode is allocated during `alloc_inode`)
- **Persistence**: **Persistent on-disk**. `generation` is stored inside `DiskInode` on persistent media and logged in `src_new_inode_image` / `dst_new_inode_image` within `DiskJournalBlockV2`.
- **In-Memory Caching**: `StorageObject` caches the active `generation` in memory for rapid handle checking.

### 3.2 Increment Rule & Overflow Semantics
- **Increment Condition**: Every operation that inserts, unlinks, or renames a directory slot increments the parent directory's `generation` counter.
- **Overflow Rule**: Uses 32-bit wrapping addition skipping zero:
  ```rust
  pub fn bump_generation(&mut self) {
      self.generation = self.generation.wrapping_add(1);
      if self.generation == 0 {
          self.generation = 1; // 0 is reserved as uncaptured/uninitialized
      }
  }
  ```

---

## 4. Exhaustive Generation Increment Matrix

| Filesystem Mutation Operation | Source Dir `generation` Increment? | Destination Dir `generation` Increment? | Execution & Transaction Site |
|---|---|---|---|
| **`CREATE_FILE`** (`SYS_FILE_OPEN` `O_CREATE`) | Yes (`parent_dir.generation++`) | N/A | Inside `DirectoryManager::insert` before journal commit |
| **`CREATE_DIR`** (`SYS_DIR_CREATE`) | Yes (`parent_dir.generation++`) | N/A | Inside `DirectoryManager::create_dir` before journal commit |
| **`UNLINK`** (`SYS_FILE_UNLINK`) | Yes (`parent_dir.generation++`) | N/A | Inside `DirectoryManager::unlink` before journal commit |
| **Same-Dir `RENAME`** (`dirA/A -> dirA/B`) | Yes (`dirA.generation++`) | N/A (Same dir) | Incremented **once** inside `DirectoryManager::rename` |
| **Cross-Dir `RENAME`** (`dirA/A -> dirB/B`)| Yes (`src_dir.generation++`) | Yes (`dst_dir.generation++`) | Incremented for **both** parent directories |
| **Rename-Over** (`dirA/A -> dirB/existing`)| Yes (`src_dir.generation++`) | Yes (`dst_dir.generation++`) | Incremented for **both** parent directories |
| **Mount Recovery Replay** (`Journal::recover`)| Yes (Applies `new_inode_image`)| Yes | Rollforward applies persisted post-tx generation |

---

## 5. `SYS_DIR_READ` ABI & Policy Specification

### 5.1 Syscall ABI Signature (Opcode 35)
```rust
pub fn sys_dir_read(
    dir_handle: Handle,
    entry_offset: u32,
    buf_ptr: *mut UserDirEntry,
    max_entries: usize,
    out_count_ptr: *mut u64,
    inout_captured_gen_ptr: *mut u32,
) -> Result<i64, SyscallError>
```

### 5.2 Selection & Justification of Mutation Policy: **Policy A (Fail on Generation Mismatch)**

We evaluated three potential concurrency policies:
- **Policy A (Fail on Generation Mismatch)**: Kernel checks `*inout_captured_gen_ptr == dir_inode.generation`. If mismatched on `entry_offset > 0`, returns `Err(SyscallError::ResourceConflict)` (`-EBUSY`). Caller resets `entry_offset = 0` and restarts iteration.
- **Policy B (Automatic Kernel Restart)**: Kernel resets `entry_offset = 0` internally. (Rejected: User buffer gets unexpected slot 0 data when expecting slot 12, causing index corruption).
- **Policy C (Kernel Copy-on-Write Snapshot)**: Kernel buffers directory snapshot in memory. (Rejected: High OOM risk, violates Stage 3K zero-dynamic-allocation memory constraints).

**Selected Policy**: **Policy A (Fail on Generation Mismatch)**. Policy A is 100% deterministic, zero-allocation, robust against race conditions, and simple for Ring 3 libraries (`libzero::readdir`) to handle.

---

## 6. `SYS_DIR_READ` Control Flow & State Machine

```text
Ring 3 Call: sys_dir_read(dir_handle, entry_offset, buf_ptr, max_entries, out_count, inout_captured_gen)
  │
  ├── 1. Resolve dir_handle via resolve_directory_inode(pslot, dir_handle, FILE_READ)
  │      └── Obtains dir_inode_num & validates dir_inode.file_type == Directory
  │
  ├── 2. Lock FILESYSTEM_LOCK spinlock & Read dir_inode
  │
  ├── 3. Generation Check (Policy A):
  │      ├── IF entry_offset == 0:
  │      │     └── Capture current generation: *inout_captured_gen_ptr = dir_inode.generation
  │      │
  │      └── IF entry_offset > 0:
  │            └── IF *inout_captured_gen_ptr != dir_inode.generation:
  │                  ├── Release FILESYSTEM_LOCK
  │                  └── RETURN Err(SyscallError::ResourceConflict)  // -EBUSY: Dir mutated!
  │
  ├── 4. Slot Scanning & Entry Formatting:
  │      ├── Read directory block starting from slot = entry_offset
  │      ├── Filter zeroed slots (inode_num == 0)
  │      └── Copy up to max_entries UserDirEntry structs to buf_ptr
  │
  ├── 5. Update out_count_ptr with count of valid entries copied
  ├── 6. Release FILESYSTEM_LOCK spinlock
  └── 7. RETURN SyscallError::Success (0)
```

---

## 7. Concurrency Scenarios & Execution Proofs

### Scenario 1 — Unlink during Enumeration
- **Initial State**: `DIR1` contains entries `A` (slot 0), `B` (slot 1), `C` (slot 2). `DIR1.generation = 10`.
- **Step 1**: Process 1 calls `sys_dir_read(offset = 0)`. Reads `A`, captures `gen = 10`. `offset` becomes `1`.
- **Step 2**: Process 2 calls `sys_file_unlink("B")`. Slot 1 zeroed. `DIR1.generation` bumps to `11`.
- **Step 3**: Process 1 calls `sys_dir_read(offset = 1, gen = 10)`.
- **Kernel Result**: Mismatch detected (`10 != 11`). Kernel returns `Err(SyscallError::ResourceConflict)`.
- **Ring 3 Result (`libzero`)**: Detects `ResourceConflict`, resets `offset = 0`, updates `gen = 11`, restarts iteration cleanly. Reads `A` and `C`. No entries missed or duplicated.

### Scenario 2 — Insertion during Enumeration
- **Initial State**: `DIR1` contains `A` (slot 0), `B` (slot 1). `DIR1.generation = 5`.
- **Step 1**: Process 1 reads slot 0 (`A`), captured `gen = 5`.
- **Step 2**: Process 2 calls `sys_file_create("X")`. `X` inserted in slot 2. `DIR1.generation` bumps to `6`.
- **Step 3**: Process 1 calls `sys_dir_read(offset = 1, gen = 5)`.
- **Kernel Result**: Mismatch detected (`5 != 6`). Returns `Err(SyscallError::ResourceConflict)`.
- **Ring 3 Result**: Restarts iteration cleanly. Reads `A`, `B`, `X`.

### Scenario 3 — Same-Directory Rename during Enumeration
- **Initial State**: `DIR1` contains `A` (slot 0), `B` (slot 1). `DIR1.generation = 20`.
- **Step 1**: Process 1 reads slot 0 (`A`), captured `gen = 20`.
- **Step 2**: Process 2 calls `sys_file_rename("B", "Z")`. `DIR1.generation` bumps to `21`.
- **Step 3**: Process 1 calls `sys_dir_read(offset = 1, gen = 20)`.
- **Kernel Result**: Mismatch detected (`20 != 21`). Returns `Err(SyscallError::ResourceConflict)`.
- **Ring 3 Result**: Restarts iteration cleanly. Reads `A` and `Z`.

### Scenario 4 — Cross-Directory Rename during Enumeration
- **Initial State**: `DIR1` has `A` (gen 10); `DIR2` has `B` (gen 15).
- **Step 1**: Process 2 calls `sys_file_rename("DIR1/A", "DIR2/A")`.
- **Kernel Result**: `DIR1.generation` bumps to 11; `DIR2.generation` bumps to 16.
- **Ring 3 Result**: Any active reader of `DIR1` or `DIR2` receives `ResourceConflict` on next read and restarts cleanly.

### Scenario 5 — Rename-Over Replacement during Enumeration
- **Initial State**: `DIR1` has `A` (slot 0); `DIR2` has `B` (slot 0). `DIR1.gen = 4`, `DIR2.gen = 8`.
- **Step 1**: Process 2 calls `sys_file_rename("DIR1/A", "DIR2/B")`.
- **Kernel Result**: `DIR1.generation` bumps to 5; `DIR2.generation` bumps to 9.
- **Ring 3 Result**: Readers of both `DIR1` and `DIR2` receive `ResourceConflict` and restart cleanly.

---

## 8. Generation Overflow & Recovery Specification

### 8.1 Overflow Handling Rule
When `generation` reaches `0xFFFF_FFFF` (4,294,967,295 mutations):
```rust
self.generation = self.generation.wrapping_add(1);
if self.generation == 0 {
    self.generation = 1; // 0 is reserved as uncaptured/uninitialized marker
}
```
Because `0` is reserved, a captured generation of `0` passed by Ring 3 always triggers a fresh generation capture on `entry_offset == 0`.

### 8.2 Crash & Reboot Recovery
- `DiskInode.generation` is written to the inode table block during Step 4 of the 6-step transaction commit protocol and logged in `DiskJournalBlockV2`.
- **Reboot Behavior**: Upon reboot, open process handle tables (`STORAGE_OBJECT_TABLE`) are cleared. Outstanding Ring 3 iteration handles are invalidated. Subsequent `sys_dir_read` calls start with `entry_offset == 0` and capture the persisted reboot generation.

---

## 9. Exact REV2 → REV3 Delta

```text
REV2 ──► REV3 Delta Analysis

ADDED / AMENDED IN REV3:
- Added DiskInode.generation persistent modification sequence counter definition (Sec 3)
- Added Exhaustive Generation Increment Matrix for all directory mutations (Sec 4)
- Added Policy A (Fail on Generation Mismatch) for SYS_DIR_READ concurrency (Sec 5)
- Added inout_captured_gen_ptr parameter to SYS_DIR_READ (Sec 5.1)
- Added Generation Overflow & Reboot Recovery rules (Sec 8)
- Formally CLOSED BLK-5 (Sec 11)

UNCHANGED FROM REV2 (100% PRESERVED):
- Journal V2 (DiskJournalBlockV2 4096-byte WAL descriptor layout) [BLK-1 CLOSED]
- DirectoryManager::rename primitive & ascending lock hierarchy [BLK-2 CLOSED]
- resolve_directory_inode handle resolution & subtree containment [BLK-3 CLOSED]
- MUTATE = 0x8000 generic management capability right [BLK-4 CLOSED]
- Disk format compatibility (Superblock v1, ZERO_JR2 v2 journal magic) [BLK-6 CLOSED]
```

---

## 10. Claim / Evidence Table

| Claim | Source / Specification Location | Evidence / Mechanism | Status |
|---|---|---|---|
| `DiskInode.generation` exists at offset 0x30 | [`kernel/src/fs/types.rs:182`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/fs/types.rs#L182) | `pub generation: u32` in `DiskInode` struct | 🟢 **VERIFIED** |
| `generation` is persistent on disk | [`kernel/src/fs/types.rs:173-189`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/fs/types.rs#L173-L189) | Stored inside 256-byte `DiskInode` table block | 🟢 **VERIFIED** |
| `generation` increments on create/unlink | REV3 Spec Sec 4 | `bump_generation()` called before tx commit | 🟢 **DESIGN DECISION** |
| `generation` increments on cross-dir rename | REV3 Spec Sec 4 | Increments both `src_dir` and `dst_dir` generations | 🟢 **DESIGN DECISION** |
| `SYS_DIR_READ` Policy A enforces mismatch check | REV3 Spec Sec 5.2 | Returns `Err(SyscallError::ResourceConflict)` (`-EBUSY`) | 🟢 **DESIGN DECISION** |
| Wrapping addition skips 0 | REV3 Spec Sec 8.1 | `wrapping_add(1)` with `if gen == 0 { gen = 1 }` | 🟢 **DESIGN DECISION** |
| Journal V2 layout unchanged | [`REV2 Arch Spec: Sec 6.1`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV2.md) | 4096-byte `DiskJournalBlockV2` layout preserved | 🟢 **VERIFIED** |
| `MUTATE = 0x8000` unchanged | [`kernel/src/cap/types.rs:15-66`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/cap/types.rs#L15-L66) | Bit 15 unassigned in generic management rights | 🟢 **VERIFIED** |
| Syscall opcodes 32..35 unchanged | [`kernel/src/syscall/numbers.rs:3-34`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/syscall/numbers.rs#L3-L34) | Opcodes 32..35 unassigned in syscall table | 🟢 **VERIFIED** |

---

## 11. Final Architectural Verdict & Closure Status

```text
BLK-1 (Journal V2):                 CLOSED
BLK-2 (Rename Primitive):           CLOSED
BLK-3 (Containment & Fallback):     CLOSED
BLK-4 (Capability Rights):          CLOSED
BLK-5 (SYS_DIR_READ Concurrency):   CLOSED
BLK-6 (Disk Format Compatibility):  CLOSED

REV3 STATUS:
🟢 FROZEN FOR IMPLEMENTATION
```
