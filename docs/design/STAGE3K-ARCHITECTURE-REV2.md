# Stage 3K — Storage / Filesystem Architecture Specification (Rev2)

**Status**: 🟢 APPROVED / READY TO FREEZE (Rev2)  
**Contract Author**: Project Zero Kernel Nucleus  
**Target Milestone**: Stage 3K  
**Dependencies**: Stage 2 (PMM, VMM), Stage 3A–3E (Kernel Threads & Scheduler), Stage 3F (Processes & AddressSpace), Stage 3G (IPC & Objects), Stage 3H (Capabilities & Handles), Stage 3I (User Space & Syscall Interface), Stage 3J (ELF / Program Execution).

---

## 1. Executive Summary & Philosophy

Stage 3K establishes the lowest correct persistent-storage primitive for Project Zero / ZeroOS.

ZeroOS is **not** a Unix clone. It rejects Unix-style ambient path authority, UID/GID permissions, and monolithic filesystem hierarchies in favor of a typed, capability-authorized resource graph.

```text
USER INTENT
    ↓
WORKSPACE
    ↓
WORKLOADS + AGENTS
    ↓
CAPABILITY SYSTEM
    ↓
FABRIC SCHEDULER
    ↓
RESOURCE GRAPH
    ↓
CPU / GPU / NPU / RAM / STORAGE / NETWORK / DEVICES
```

Stage 3K introduces **ZeroFS**, a capability-native, extent/direct-pointer filesystem with:
- Strict 4096-byte page-aligned blocks.
- A 1-block Write-Ahead Intent Journal guaranteeing transactional crash consistency (data-first, metadata-second, commit-third).
- Direct integration with Stage 3G Kernel Objects (`KernelObjectType::StorageObject = 5`) and Stage 3H Capabilities (`cap_rights::FILE_READ`, `FILE_WRITE`, `FILE_SYNC`).
- Capability-authorized namespace traversal (eliminating all ambient filesystem authority).
- Zero dynamic kernel heap allocations (`alloc` forbidden; all tables bounded in static BSS).
- Polled Primary ATA PIO (`0x1F0..0x1F7`) alongside a deterministic in-memory `MemBlockDevice`.

---

## 2. Repository Findings & Frozen Contracts

Inspection of the active implementation repository confirms:

| Area | Authoritative Repository Finding | Stage 3K Contract Invariant |
| :--- | :--- | :--- |
| **Port I/O** | `inb`, `outb`, `io_wait` in `hal/arch/x86_64/cpu.rs`. | Add `inw`, `outw` to `cpu.rs`. ATA driver uses polled PIO over ports `0x1F0..0x1F7`, `0x3F6`. |
| **Memory / PMM** | 4096-byte physical frames. HHDM maps `[0, 1 GiB)` at `0xFFFF_8000_0000_0000`. | `BLOCK_SIZE = 4096` bytes. Buffer cache buffers are 4 KiB and HHDM-addressable. |
| **Kernel Objects** | `KernelObjectType`: `Free=0, Channel=1, ShmObject=2, Event=3, Process=4`. `MAX_KERNEL_OBJECTS = 256`. `KernelObjectSlot = 40 B`. | Append `StorageObject = 5` to `KernelObjectType` (`#[repr(u8)]`). `KernelObjectSlot` remains exactly 40 bytes. |
| **Capabilities** | `CapabilityNode`: 24 bytes (8-byte aligned). `cap_rights`: Bits 0..4 used; Bits 8..13 used for generic management. | Allocate Bits 5..7 for storage rights: `FILE_READ = 1 << 5`, `FILE_WRITE = 1 << 6`, `FILE_SYNC = 1 << 7`. `CapabilityNode` remains 24 bytes. |
| **Handles** | `Handle`: 16 bytes. `HandleTable`: 520 bytes (`MAX_HANDLES_PER_PROCESS = 32`). | Frozen ABI unchanged. Handle points to `KernelObjectSlot` which references `StorageObject`. |
| **Process** | `Process`: 128 bytes. `KernelThread`: 176 bytes. `PerCpu`: 48 bytes. | Frozen ABIs unchanged. |
| **Syscall ABI** | 144-byte `SyscallFrame` via `IA32_LSTAR`. Syscalls 1..10 frozen (`SYS_EXIT..SYS_CAP_DERIVE`). | Define `SYS_FILE_OPEN = 11`, `SYS_FILE_READ = 12`, `SYS_FILE_WRITE = 13`, `SYS_FILE_CLOSE = 14`, `SYS_FILE_STAT = 15`, `SYS_FILE_SYNC = 16`. |
| **Lock Order** | `KERNEL_OBJECT_TABLE_LOCK ≺ SCHEDULER.lock ≺ CPU(IF=0)`. | Define `STORAGE_OBJECT_TABLE_LOCK ≺ FILESYSTEM_LOCK ≺ BLOCK_CACHE_LOCK ≺ BLOCK_DEVICE_LOCK ≺ KERNEL_OBJECT_TABLE_LOCK ≺ SCHEDULER.lock ≺ CPU(IF=0)`. |

---

## 3. Goals & Non-Goals

### 3.1 Goals
1. Establish a persistent, crash-consistent block storage primitive.
2. Provide a 4096-byte block layout with exact byte-level persistent structures and CRC32 verification.
3. Guarantee atomic write-ahead intent journaling with deterministic recovery.
4. Integrate with the Capability System such that file open requires explicit directory authority.
5. Provide a 16-buffer static buffer cache (64 KiB BSS footprint).
6. Verify via 19 bare-metal machine tests (`3K-A` through `3K-S`) with full PMM neutrality.

### 3.2 Non-Goals
- Dynamic disk partitioning (GPT/MBR partition tables).
- Interrupt-driven AHCI, NVMe, virtio-blk, or DMA.
- Multi-block B-trees or complex indirect chains beyond double-indirect.
- Unix ambient permissions (UID/GID, chmod, chown) or hard links.
- Loading executables directly from disk (deferred to Stage 3L/Workspace; Stage 3J continues loading from memory slices).

---

## 4. Block Device Architecture

### 4.1 BlockDevice Trait
```rust
pub const SECTOR_SIZE: usize = 512;
pub const BLOCK_SIZE: usize = 4096;
pub const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE; // 8

pub trait BlockDevice {
    fn device_id(&self) -> u8;
    fn block_count(&self) -> u64;
    fn read_block(&mut self, block_idx: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), DeviceError>;
    fn write_block(&mut self, block_idx: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), DeviceError>;
    fn flush(&mut self) -> Result<(), DeviceError>;
}
```

### 4.2 Hardware Drivers
1. **`AtaPioBlockDevice`**:
   - Primary ATA channel ports: Data `0x1F0`, Features/Error `0x1F1`, Sector Count `0x1F2`, LBA Low `0x1F3`, LBA Mid `0x1F4`, LBA High `0x1F5`, Device/Head `0x1F6`, Command/Status `0x1F7`, Control `0x3F6`.
   - 28-bit LBA addressing (up to 128 GiB).
   - Commands: `0xEC` (IDENTIFY), `0x20` (READ SECTORS), `0x30` (WRITE SECTORS), `0xE7` (CACHE FLUSH).
   - Polling with bounded timeout (`100,000` cycles) under `BLOCK_DEVICE_LOCK` with interrupts disabled.
2. **`MemBlockDevice`**:
   - 16 MiB in-memory mock block device in static BSS for deterministic bare-metal machine testing.

```rust
pub const MAX_BLOCK_DEVICES: usize = 2;
// Device 0: MemBlockDevice (Testing)
// Device 1: AtaPioBlockDevice (Hardware Primary ATA Master)
```

---

## 5. Exact Persistent Disk Layout (Blocker #1 Resolved)

ZeroFS disk layout is defined by exact, deterministic mathematical formulas:

Let $B = 4096$ bytes, $S = 512$ bytes ($8$ sectors/block), and $N_{\text{blocks}}$ be total volume blocks.
- **Minimum Volume Size**: 4 MiB ($1024$ blocks).
- **Standard Volume Size**: 16 MiB ($4096$ blocks) or 32 MiB ($8192$ blocks).
- **Maximum Volume Size**: 128 GiB ($2^{25}$ blocks, bounded by 28-bit LBA).

### 5.1 Volume Region Map
```text
Block 0:        Reserved / Boot Sector (4 KiB)           [offset 0x0000_0000]
Block 1:        Superblock (4 KiB)                      [offset 0x0000_1000]
Block 2:        Transaction Journal (4 KiB)             [offset 0x0000_2000]
Block 3:        Inode Allocation Bitmap (4 KiB)         [offset 0x0000_3000]
Block 4:        Block Allocation Bitmap (4 KiB)         [offset 0x0000_4000]
Blocks 5..20:   Inode Table (16 blocks = 64 KiB, 256 inodes) [offset 0x0000_5000]
Block 21:       Root Directory Data Block (4 KiB)       [offset 0x0001_5000]
Blocks 22..N-1: Data Blocks (N - 22 blocks)             [offset 0x0001_6000]
```

### 5.2 Deterministic Region Constants & Formulas
```rust
pub const SUPERBLOCK_BLOCK: u64 = 1;
pub const JOURNAL_BLOCK: u64 = 2;
pub const INODE_BITMAP_BLOCK: u64 = 3;
pub const INODE_BITMAP_BLOCKS: u64 = 1;
pub const BLOCK_BITMAP_BLOCK: u64 = 4;
pub const BLOCK_BITMAP_BLOCKS: u64 = 1;
pub const INODE_TABLE_START: u64 = 5;
pub const INODE_TABLE_BLOCKS: u64 = 16;
pub const MAX_INODES: usize = 256;
pub const INODES_PER_BLOCK: usize = 16;
pub const DATA_BLOCK_START: u64 = 21;
pub const ROOT_DIR_INODE: u32 = 1;
pub const NULL_INODE: u32 = 0;
```

---

## 6. Exact Persistent On-Disk Structures

### 6.1 `DiskSuperblock` (128 bytes, Block 1)
```rust
pub const FS_MAGIC: [u8; 8] = *b"ZERO_FS\0";
pub const FS_VERSION: u32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskSuperblock {
    pub magic: [u8; 8],                  // 0x00..0x08: "ZERO_FS\0"
    pub version: u32,                    // 0x08..0x0C: 0x0000_0001
    pub block_size: u32,                 // 0x0C..0x10: 4096
    pub total_blocks: u64,               // 0x10..0x18: Total blocks (e.g. 4096)
    pub free_blocks: u64,                // 0x18..0x20: Unallocated data blocks
    pub total_inodes: u32,               // 0x20..0x24: 256
    pub free_inodes: u32,                // 0x24..0x28: Unallocated inodes
    pub journal_block: u64,              // 0x28..0x30: 2
    pub block_bitmap_block: u64,         // 0x30..0x38: 4
    pub inode_bitmap_block: u64,         // 0x38..0x40: 3
    pub inode_table_block: u64,          // 0x40..0x48: 5
    pub inode_table_blocks: u32,         // 0x48..0x4C: 16
    pub root_dir_inode: u32,             // 0x4C..0x50: 1
    pub volume_state: u32,               // 0x50..0x54: 0=Clean, 1=Dirty, 2=Recovering, 3=Faulted
    pub mount_count: u64,                // 0x54..0x5C: Monotonic mount counter
    pub checksum: u32,                   // 0x5C..0x60: CRC32 over bytes 0x00..0x5C
    pub volume_label: [u8; 32],          // 0x60..0x80: UTF-8 volume label
}

const _: () = assert!(core::mem::size_of::<DiskSuperblock>() == 128);
const _: () = assert!(core::mem::align_of::<DiskSuperblock>() == 8);
```

### 6.2 `DiskInode` (256 bytes, 16 Inodes per Block)
```rust
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    Free = 0,
    Regular = 1,
    Directory = 2,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskInode {
    pub file_type: InodeType,            // 0x00..0x02: 0=Free, 1=Regular, 2=Directory
    pub flags: u16,                      // 0x02..0x04: Bit 0: IMMUTABLE, Bit 1: SYNC, Bit 2: PENDING_DELETE
    pub links_count: u32,                // 0x04..0x08: Directory links count
    pub size_bytes: u64,                 // 0x08..0x10: Exact file size in bytes
    pub blocks_count: u64,               // 0x10..0x18: Total 4 KiB blocks allocated
    pub creator_pid: u64,                // 0x18..0x20: Process ID of creator
    pub ctime_ticks: u64,                // 0x20..0x28: Creation system ticks
    pub mtime_ticks: u64,                // 0x28..0x30: Last modification system ticks
    pub generation: u32,                 // 0x30..0x34: Monotonic generation counter
    pub checksum: u32,                   // 0x34..0x38: CRC32 over bytes 0x00..0x34
    pub _reserved: [u8; 8],              // 0x38..0x40: Alignment padding
    pub direct_blocks: [u64; 18],        // 0x40..0xD0: 18 direct block pointers (72 KiB)
    pub indirect_block: u64,             // 0xD0..0xD8: Single indirect pointer (2 MiB)
    pub double_indirect: u64,           // 0xD8..0xE0: Double indirect pointer (1 GiB)
    pub inline_data: [u8; 32],           // 0xE0..0x100: Reserved metadata expansion
}

const _: () = assert!(core::mem::size_of::<DiskInode>() == 256);
const _: () = assert!(core::mem::align_of::<DiskInode>() == 8);
```

### 6.3 `DiskDirEntry` (64 bytes, 64 entries per Block)
```rust
pub const MAX_FILENAME_LEN: usize = 55;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskDirEntry {
    pub inode_num: u32,                  // 0x00..0x04: Inode index (1..=255; 0 = unused/deleted)
    pub name_len: u8,                    // 0x04..0x05: Name length (1..=55)
    pub file_type: u8,                   // 0x05..0x06: 1=Regular, 2=Directory
    pub _pad: u16,                       // 0x06..0x08: Padding
    pub name: [u8; 55],                  // 0x08..0x3F: ASCII / UTF-8 filename (no nulls in body)
    pub _null_terminator: u8,            // 0x3F..0x40: Guarantees null-termination
}

const _: () = assert!(core::mem::size_of::<DiskDirEntry>() == 64);
const _: () = assert!(core::mem::align_of::<DiskDirEntry>() == 8);
```

---

## 7. Exact Journal Format & Transaction State Machine (Blocker #2 Resolved)

### 7.1 `DiskJournalBlock` (4096 bytes, Block 2)
```rust
pub const JOURNAL_MAGIC: [u8; 8] = *b"ZERO_JRN";

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalOpType {
    None = 0,
    CreateFile = 1,
    WriteFile = 2,
    TruncateFile = 3,
    DeleteFile = 4,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalState {
    Free = 0,       // No active transaction
    Intent = 1,     // Intent durable; blocks allocated but uncommitted
    Committed = 2,  // Data durable; transaction committed, ready to apply metadata
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskJournalBlock {
    pub magic: [u8; 8],                  // 0x00..0x08: "ZERO_JRN"
    pub sequence: u64,                   // 0x08..0x10: Monotonic transaction counter
    pub format_version: u32,             // 0x10..0x14: 1
    pub op_type: JournalOpType,          // 0x14..0x18: Active operation
    pub state: JournalState,             // 0x18..0x1C: Free / Intent / Committed
    pub target_inode: u32,               // 0x1C..0x20: Inode under modification
    pub parent_dir_inode: u32,           // 0x20..0x24: Directory inode (create/delete)
    pub dir_entry_slot: u32,             // 0x24..0x28: Directory block slot index
    pub old_size: u64,                   // 0x28..0x30: Pre-transaction file size
    pub new_size: u64,                   // 0x30..0x38: Post-transaction file size
    pub allocated_blocks_count: u32,     // 0x38..0x3C: Count of allocated blocks (0..8)
    pub freed_blocks_count: u32,         // 0x3C..0x40: Count of freed blocks (0..8)
    pub allocated_blocks: [u32; 8],      // 0x40..0x60: Block LBAs allocated in TX
    pub freed_blocks: [u32; 8],          // 0x60..0x80: Block LBAs freed in TX
    pub dir_entry_copy: DiskDirEntry,    // 0x80..0xC0: Exact 64-byte directory entry copy
    pub checksum: u32,                   // 0xC0..0xC4: CRC32 over bytes 0x00..0xC0
    pub _reserved: [u8; 3900],           // 0xC4..0x1000: Zero padding to 4096 bytes
}

const _: () = assert!(core::mem::size_of::<DiskJournalBlock>() == 4096);
```

### 7.2 Six-Step Transaction Ordering Protocol
```text
Step 1: Write Journal INTENT Record (state = Intent, op details, CRC32) -> FLUSH
Step 2: Write Data Blocks to disk -> FLUSH (Unwritten data NEVER referenced by Inode)
Step 3: Write Journal COMMITTED Record (state = Committed, CRC32) -> FLUSH (COMMIT POINT)
Step 4: Write Metadata Blocks (Inode, Directory Entry, Bitmaps) -> FLUSH
Step 5: Write Superblock (updated free counts) -> FLUSH
Step 6: Clear Journal Record (state = Free) -> FLUSH
```

### 7.3 Crash Recovery Matrix
| Journal State | CRC32 Status | Recovery Action | Guarantee |
| :--- | :--- | :--- | :--- |
| `Free` | Valid | No action. Volume clean. | Clean mount. |
| `Intent` | Valid | **Rollback**: Free blocks in `allocated_blocks` from Block Bitmap. Target inode size remains `old_size`. Clear journal to `Free`. | Zero orphan blocks; zero data corruption. |
| `Committed` | Valid | **Rollforward**: Apply `dir_entry_copy` to directory; write `new_size` and block pointers to target inode; mark allocated blocks in bitmap; update superblock; clear journal to `Free`. | Zero data loss; fully durable commit. |
| Any | Invalid | **Fail-Closed**: If Superblock was dirty and journal CRC32 fails, volume marked `Faulted`. Mount fails with `Err(FsError::CorruptJournal)`. | Prevents corrupting disk from torn journal write. |

---

## 8. Kernel StorageObject & Lifetime Model (Blocker #3 Resolved)

### 8.1 What `StorageObject` Represents
`StorageObject` represents an **Open File Description** in kernel space.

```rust
pub const MAX_OPEN_STORAGE_OBJECTS: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StorageObject {
    pub occupied: bool,
    pub inode_num: u32,
    pub generation: u32,
    pub cursor_offset: u64,
    pub open_flags: u32,
    pub file_size: u64,
    pub in_flight_ops: u16,
    pub _pad: u16,
}
```

### 8.2 File Deletion vs Open Handles
1. When a file is unlinked/deleted:
   - The directory entry is removed from the directory block.
   - The on-disk `links_count` is decremented.
   - If `links_count == 0`:
     - If active `StorageObject` has `handle_refs > 0` or `in_flight_ops > 0`: the inode is marked `PENDING_DELETE`. Blocks are **not** reclaimed yet. Open processes can continue reading/writing.
     - When the last handle is closed (`handle_refs == 0 && in_flight_ops == 0`): blocks and inode are reclaimed and returned to bitmaps.
2. When a process exits:
   - Handles held by the process are closed.
   - `handle_refs` on `StorageObject` is decremented.
   - If `links_count > 0` (normal file): file and data remain completely intact on persistent media.
   - Persistent storage state **never** deletes simply because an opening process terminates.

---

## 9. Capability Authority & Namespace Traversal (Blocker #4 Resolved)

### 9.1 Elimination of Ambient Authority
In ZeroOS, there is **zero ambient filesystem authority**. A path string alone cannot open a file.
Opening a file requires an explicit **Directory Capability**:

```rust
sys_file_open(dir_handle, path_ptr, path_len, flags, out_handle_ptr)
```

- `dir_handle` must refer to an open directory `StorageObject` (e.g. root directory capability granted during process launch).
- Path traversal evaluates `path_ptr` **strictly relative to `dir_handle`**.
- Subtree restriction: A process possessing a capability only to `/workspace` can never traverse into `/system` or `/kernel`.

### 9.2 Capability Rights Mapping
```rust
pub mod cap_rights {
    // Type-Specific Storage Rights (Bits 5..7):
    pub const FILE_READ:  u16 = 1 << 5; // 0x0020: Read bytes from file / enumerate directory
    pub const FILE_WRITE: u16 = 1 << 6; // 0x0040: Write bytes to file / create/delete in directory
    pub const FILE_SYNC:  u16 = 1 << 7; // 0x0080: Flush dirty data to persistent storage

    // Generic Management Rights (Bits 8..15):
    pub const DUPLICATE:  u16 = 1 << 8;
    pub const TRANSFER:   u16 = 1 << 9;
    pub const REVOKE:     u16 = 1 << 10;
    pub const CLOSE:      u16 = 1 << 11;
    pub const INSPECT:    u16 = 1 << 12; // Query file metadata / stat
}
```

- **Monotonic Attenuation**: An open file capability derived from a directory capability cannot exceed the directory's rights:
  $$\text{Rights}(\text{file}) \subseteq \text{Rights}(\text{dir\_capability})$$
- Attempting to derive `FILE_WRITE` from a directory capability lacking `FILE_WRITE` returns `-EACCES`.

---

## 10. Exact System Call ABI Specification

Fast syscalls (11..16) conforming to the Stage 3I 144-byte `SyscallFrame`:

### 10.1 `SYS_FILE_OPEN` (11)
- `RAX`: `11`
- `RDI`: `dir_handle: u32` (Directory capability)
- `RSI`: `path_ptr: u64` (User pointer to relative filename)
- `RDX`: `path_len: u64` (1..=55 bytes)
- `R10`: `flags: u32` (`O_READ=1, O_WRITE=2, O_CREATE=4, O_TRUNC=8, O_APPEND=16`)
- `R8`: `out_handle_ptr: u64` (Pointer to write `u32` handle descriptor)
- Pointer validation: `validate_user_range(path_ptr, path_len, Read)`, `validate_user_range(out_handle_ptr, 4, Write)`.
- Returns: `0` on success, or negative `SyscallError` (`-EACCES`, `-ENOENT`, `-EEXIST`, `-ENOSPC`, `-ENFILE`, `-EFAULT`, `-EINVAL`).

### 10.2 `SYS_FILE_READ` (12)
- `RAX`: `12`
- `RDI`: `handle: u32`
- `RSI`: `buf_ptr: u64`
- `RDX`: `count: u64`
- `R10`: `out_read_ptr: u64` (Pointer to write `u64` actual bytes read)
- Required Right: `FILE_READ`.
- Pointer validation: `validate_user_range(buf_ptr, count, Write)`, `validate_user_range(out_read_ptr, 8, Write)`.
- EOF semantics: If `cursor_offset >= file_size`, returns 0 bytes read.
- Returns: `0` on success, or negative `SyscallError`.

### 10.3 `SYS_FILE_WRITE` (13)
- `RAX`: `13`
- `RDI`: `handle: u32`
- `RSI`: `buf_ptr: u64`
- `RDX`: `count: u64`
- `R10`: `out_written_ptr: u64` (Pointer to write `u64` actual bytes written)
- Required Right: `FILE_WRITE`.
- Pointer validation: `validate_user_range(buf_ptr, count, Read)`, `validate_user_range(out_written_ptr, 8, Write)`.
- Returns: `0` on success, or negative `SyscallError` (`-ENOSPC` if disk full).

### 10.4 `SYS_FILE_CLOSE` (14)
- `RAX`: `14`
- `RDI`: `handle: u32`
- Required Right: `CLOSE`.
- Returns: `0` on success, or negative `SyscallError`.

### 10.5 `SYS_FILE_STAT` (15)
- `RAX`: `15`
- `RDI`: `handle: u32`
- `RSI`: `out_stat_ptr: u64` (User pointer to write 32-byte `UserFileStat`)
- Required Right: `INSPECT`.
- Pointer validation: `validate_user_range(out_stat_ptr, 32, Write)`.
- Returns: `0` on success, or negative `SyscallError`.

### 10.6 `SYS_FILE_SYNC` (16)
- `RAX`: `16`
- `RDI`: `handle: u32`
- Required Right: `FILE_SYNC`.
- Behavior: Commits dirty buffers and executes ATA `CACHE FLUSH`.
- Returns: `0` on success, or negative `SyscallError`.

---

## 11. Bounded Buffer Cache & Lifecycle

```rust
pub const MAX_BUFFERS: usize = 16; // 16 × 4096 = 64 KiB BSS footprint

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferState {
    Free,
    Clean,
    Dirty,
    Pinned,
    Writeback,
}
```

- **Pinning**: `pin_count` prevents eviction while operation active.
- **Eviction**: Unpinned buffer (`pin_count == 0`) with lowest `lru_seq`. Dirty buffers written out prior to eviction.
- **Exhaustion**: If all 16 buffers are pinned, returns `Err(FsError::BufferPoolExhausted)` (`-EBUSY`) deterministically.
- **LRU Rescale**: When `clock_seq == u32::MAX`, sequence counters are rescaled linearly to prevent overflow.

---

## 12. Monotonic Lock Hierarchy

Zero-deadlock ordering across all subsystems:

```text
STORAGE_OBJECT_TABLE_LOCK (Rank 1)
    ≺ FILESYSTEM_LOCK (Rank 2)
        ≺ BLOCK_CACHE_LOCK (Rank 3)
            ≺ BLOCK_DEVICE_LOCK (Rank 4)
                ≺ KERNEL_OBJECT_TABLE_LOCK (Rank 5, Frozen 3G)
                    ≺ SCHEDULER.lock (Rank 6, Frozen 3A–3E)
                        ≺ CPU (IF=0) (Rank 7)
```

- Polling port I/O runs strictly under `BLOCK_DEVICE_LOCK` with `IF=0`.
- No thread ever blocks or sleeps holding Ranks 1..6.

---

## 13. Device Failure Model

```rust
pub enum DeviceState {
    Offline,
    Probing,
    Ready,
    Faulted,
}
```

- If ATA status register asserts `ERR` or timeout exceeds 100,000 cycles: device enters `Faulted`.
- Mounted volume transitions to `ReadOnly` / `Faulted`. In-flight writes fail with `-EIO`.
- Memory corruption is strictly prevented.

---

## 14. Resource Bounds (Zero-Heap Compliance)

All tables statically bounded:
- `MAX_BLOCK_DEVICES`: 2
- `MAX_OPEN_STORAGE_OBJECTS`: 32 (32 × 32 B = 1024 B)
- `MAX_BUFFERS`: 16 (16 × 4096 B = 65,536 B)
- `MAX_INODES`: 256
- `MAX_TRANSACTION_RECORDS`: 1
- `MAX_TRANSACTION_BLOCKS`: 8 (32 KiB maximum single transaction write payload)

Zero dynamic heap allocations in kernel runtime.

---

## 15. Security & Validation Invariants

- `I-STOR-SEC-1`: Every block number, inode index, and offset read from disk is verified within volume bounds before memory indexing or port I/O.
- `I-STOR-SEC-2`: Superblock and Inode CRC32 checksums verified upon read; mismatch immediately fails closed (`Err(FsError::CorruptMetadata)`).
- `I-STOR-SEC-3`: Filenames restricted to 1..=55 bytes, no null bytes in body, no `..` traversal.

---

## 16. Verification Architecture (19 Tests: 3K-A through 3K-S)

| Test ID | Title | Verification Objective | Invariant Mapped |
| :--- | :--- | :--- | :--- |
| **3K-A** | Block Device Discovery | Probe MemBlock and ATA PIO block devices | `I-STOR-DEVICE-1` |
| **3K-B** | Block Sector Read/Write | Raw sector and block read/write fidelity | `I-STOR-DEVICE-2` |
| **3K-C** | Block Bounds Check | Reject block requests $\ge total\_blocks$ | `I-STOR-BLOCK-1` |
| **3K-D** | ZeroFS Volume Format | Format volume, create superblock, bitmaps, root dir | `I-STOR-FS-1` |
| **3K-E** | Superblock Validation | Parse clean mount, magic `"ZERO_FS\0"`, CRC32 | `I-STOR-META-1` |
| **3K-F** | Inode Allocation | Allocate/free inodes; verify generation counter | `I-STOR-ALLOC-1` |
| **3K-G** | Block Allocation | Allocate/free blocks; verify bitmap bits | `I-STOR-ALLOC-2` |
| **3K-H** | Allocation Exhaustion | Verify `-ENOSPC` when block bitmap full | `I-STOR-ALLOC-3` |
| **3K-I** | Direct Block Mapping | Write/read 72 KiB direct blocks | `I-STOR-INODE-1` |
| **3K-J** | Indirect Block Mapping | Write/read single-indirect block (2 MiB) | `I-STOR-INODE-2` |
| **3K-K** | Tail Zero Padding | Verify unwritten block tail is zeroed | `I-STOR-INODE-3` |
| **3K-L** | Directory Insertion | Insert entries; reject duplicates | `I-STOR-DIR-1` |
| **3K-M** | File Truncation | Truncate file; verify block reclamation | `I-STOR-RECLAIM-1` |
| **3K-N** | Process-Exit Persistence | Process exits; verify on-disk file remains readable | `I-STOR-LIFETIME-1` |
| **3K-O** | Reboot Persistence | Verify on-disk file survives machine restart | `I-STOR-LIFETIME-2` |
| **3K-P** | Corruption Rejection | Corrupt superblock CRC32; verify mount fails closed | `I-STOR-SEC-1` |
| **3K-Q** | Journal Crash Recovery | Simulate crash; verify atomic rollback of intent | `I-STOR-JOURNAL-1` |
| **3K-R** | Capability Rights Check | Verify `FILE_READ` vs `FILE_WRITE` enforcement | `I-STOR-CAP-1` |
| **3K-S** | PMM Frame Neutrality | Verify `baseline_free == post_test_free` | `I-STOR-PMM-1` |

### Machine Test Arithmetic:
- Baseline Stages 1–3H: 181
- Stage 3I: 18
- Stage 3J: 16
- Stage 3K: 19
- **Total In-Kernel Machine Tests**: **234 tests**.

### Host Pytest Suite:
- Baseline: 27 files, 104 tests.
- Stage 3K: 1 file (`tests/test_stage3k.py`), 6 tests.
- **Total Host Pytests**: 28 files, 110 tests.

---

## 17. Architecture Decision Records (ADRs)

- **ADR-3K-001**: ZeroFS On-Disk Geometry & Exact Formulas
- **ADR-3K-002**: Write-Ahead Intent Journal & Crash Recovery
- **ADR-3K-003**: StorageObject Kernel Semantics & Unlinked File Lifetime
- **ADR-3K-004**: Capability-Based Namespace Traversal (No Ambient Authority)
- **ADR-3K-005**: Bounded Buffer Cache Lifecycle & Eviction
- **ADR-3K-006**: ATA PIO Driver & Device Failure Semantics
- **ADR-3K-007**: Zero-Heap Bounded Resource Policy

---

## 18. Adversarial Review Summary

- **Memory Safety**: Block indices strictly validated against `total_blocks`; CRC32 checksums verified on every read.
- **Crash Consistency**: Data blocks flushed before Inode points to them; Write-Ahead Journal intent records rolled back on crash.
- **Capability Security**: Directory capability required for open; rights monotonically attenuated; ambient authority eliminated.
- **Lifetime**: Inodes marked pending-delete remain accessible to active handles until last handle closed; process exit does not delete files.
- **Concurrency**: Strict 7-rank lock hierarchy eliminates cycles; no spinlock held across blocking switch.

---

## 19. Required Final Verdict

All freeze criteria, byte layouts, crash state machines, capability rules, and lock hierarchies are completely defined with zero open RED blockers.

```text
🟢 STAGE 3K ARCHITECTURE APPROVED / READY TO FREEZE
```
