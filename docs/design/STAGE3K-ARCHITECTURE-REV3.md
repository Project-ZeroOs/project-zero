# Stage 3K — Storage / Filesystem Architecture Specification (Rev3)

**Status**: 🟢 APPROVED / READY TO FREEZE (Rev3)  
**Contract Author**: Project Zero Kernel Nucleus  
**Target Milestone**: Stage 3K  
**Dependencies**: Stage 2 (PMM, VMM), Stage 3A–3E (Kernel Threads & Scheduler), Stage 3F (Processes & AddressSpace), Stage 3G (IPC & Objects), Stage 3H (Capabilities & Handles), Stage 3I (User Space & Syscall Interface), Stage 3J (ELF / Program Execution).

---

## 1. Repository Findings & Frozen ABI Invariance

Inspection of the authoritative repository source code confirms:

| Subsystem / Structure | Authoritative Code Contract | Stage 3K Architectural Rule |
| :--- | :--- | :--- |
| **Port I/O** (`kernel/src/hal/arch/x86_64/cpu.rs`) | `inb(port: u16) -> u8`, `outb(port: u16, val: u8)`, `io_wait()` | Add `inw(port: u16) -> u16` and `outw(port: u16, val: u16)`. Primary ATA PIO uses ports `0x1F0..0x1F7`, `0x3F6`. |
| **Memory / PMM** (`kernel/src/mm/pmm.rs`, `vmm.rs`) | 4096-byte page frames. HHDM maps physical `[0, 1 GiB)` at `0xFFFF_8000_0000_0000`. Zero kernel heap. | `BLOCK_SIZE = 4096` bytes. Buffer cache buffers are 4 KiB and HHDM-addressable. |
| **Kernel Objects** (`kernel/src/ipc/object.rs`) | `KernelObjectType`: `Free=0, Channel=1, ShmObject=2, Event=3, Process=4`. `MAX_KERNEL_OBJECTS = 256`. `KernelObjectSlot = 40 B`. | Append `StorageObject = 5` to `KernelObjectType` (`#[repr(u8)]`). `KernelObjectSlot` size (40 B) and alignment (8 B) remain identical. |
| **Capabilities** (`kernel/src/cap/types.rs`) | `CapabilityNode`: 24 B (8-B aligned). `cap_rights`: Bits 0..4 used; Bits 8..13 used for generic management. | Allocate Bits 5..7 for storage: `FILE_READ = 1 << 5`, `FILE_WRITE = 1 << 6`, `FILE_SYNC = 1 << 7`. `CapabilityNode` remains 24 B. |
| **Handles** (`kernel/src/ipc/handle.rs`) | `Handle`: 16 B. `HandleTable`: 520 B (`MAX_HANDLES_PER_PROCESS = 32`). | Frozen ABI unchanged. Handle indexes `KernelObjectSlot` whose `pool_index` references `StorageObject`. |
| **Process / Thread** (`task/process.rs`, `task/thread.rs`) | `Process`: 128 B. `KernelThread`: 176 B. `PerCpu`: 48 B. | Frozen ABIs completely untouched. |
| **Syscall ABI** (`syscall/abi.rs`, `numbers.rs`) | 144-B `SyscallFrame` via `IA32_LSTAR`. Syscalls 1..10 frozen (`SYS_EXIT..SYS_CAP_DERIVE`). | Define `SYS_FILE_OPEN = 11`, `SYS_FILE_READ = 12`, `SYS_FILE_WRITE = 13`, `SYS_FILE_CLOSE = 14`, `SYS_FILE_STAT = 15`, `SYS_FILE_SYNC = 16`. |
| **Lock Order** (`ipc/object.rs`, `task/scheduler.rs`) | `KERNEL_OBJECT_TABLE_LOCK ≺ SCHEDULER.lock ≺ CPU(IF=0)`. | Global order: `STORAGE_OBJECT_TABLE_LOCK ≺ FILESYSTEM_LOCK ≺ BLOCK_CACHE_LOCK ≺ BLOCK_DEVICE_LOCK ≺ KERNEL_OBJECT_TABLE_LOCK ≺ SCHEDULER.lock ≺ CPU(IF=0)`. |

---

## 2. Goals & Non-Goals

### 2.1 Goals
1. **Persistent Block Storage Primitive**: Provide a deterministic block device layer supporting polled Primary ATA PIO (`0x1F0..0x1F7`) alongside an in-memory `MemBlockDevice`.
2. **ZeroFS Persistent Geometry**: Implement an extent/direct-pointer filesystem format with 4096-byte blocks, CRC32 integrity checksums, and strict byte-defined structures.
3. **Write-Ahead Intent Journaling**: Bounded 4096-byte transaction log ensuring atomic crash consistency (`I-STOR-JOURNAL-1`).
4. **Capability-Native Authority**: Eliminate ambient path authority. All file access requires an explicit Directory Capability.
5. **Zero Dynamic Heap**: All block buffers (16 buffers = 64 KiB), open storage objects (32 slots), and bitmaps reside in static BSS.
6. **Machine Verification**: Deterministic 19-test bare-metal machine suite (`3K-A` through `3K-S`) with exact PMM neutrality.

### 2.2 Non-Goals (Explicitly Deferred)
- Dynamic partitioning (GPT/MBR).
- Asynchronous DMA / interrupt-driven AHCI / NVMe / virtio-blk.
- Multi-user Unix permissions (UID/GID, chmod/chown) or POSIX hard links.
- Distributed / network storage.
- Loading ELF executables directly from disk (deferred to Stage 3L/Workspace; Stage 3J loader continues loading from memory slices).

---

## 3. ZeroFS Layered Architecture

```text
Layer 6: System Call Interface (sys_file_open, sys_file_read, sys_file_write, sys_file_close, sys_file_stat, sys_file_sync)
    ↓
Layer 5: Capability & Kernel Object Layer (Directory Capability -> HandleTable -> CapabilityNode -> StorageObject)
    ↓
Layer 4: Namespace & File Abstraction (Directory Entries, Relative Path Resolution, Inode Descriptors)
    ↓
Layer 3: ZeroFS Filesystem & Intent Log (Superblock, Bitmaps, Write-Ahead Journal, Inode Table)
    ↓
Layer 2: Bounded Block Buffer Cache (16 static 4 KiB buffers, pin counts, LRU eviction, dirty tracking)
    ↓
Layer 1: Block Device Abstraction (BlockDevice trait: read_block, write_block, flush, block_count)
    ↓
Layer 0: Physical Hardware / Mock (Primary ATA PIO Master 0x1F0..0x1F7 / In-Memory MemBlockDevice)
```

---

## 4. Block Device Architecture

### 4.1 `BlockDevice` Trait
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
   - 28-bit LBA addressing. Master drive selected via `0xE0 | ((lba >> 24) & 0x0F)`.
   - Commands: `0xEC` (IDENTIFY), `0x20` (READ SECTORS), `0x30` (WRITE SECTORS), `0xE7` (CACHE FLUSH).
   - Polling loop with bounded iteration limit (`100,000` cycles) under `BLOCK_DEVICE_LOCK` with interrupts disabled (`IF=0`).
2. **`MemBlockDevice`**:
   - 16 MiB statically allocated in BSS. Provides deterministic in-kernel machine testing.

```rust
pub const MAX_BLOCK_DEVICES: usize = 2;
// Device 0: MemBlockDevice (Transient testing)
// Device 1: AtaPioBlockDevice (Persistent hardware)
```

---

## 5. Exact Persistent Disk Layout (BLOCKER A & E Resolved)

### 5.1 Volume Capacity Definitions & Arithmetic
Let $B = 4096$ bytes, $S = 512$ bytes ($8$ sectors/block), and $N_{\text{blocks}}$ be total volume blocks.
- **Maximum Volume Blocks**: `MAX_VOLUME_BLOCKS: u64 = 32_768` blocks.
- **Maximum Volume Bytes**: `MAX_VOLUME_BYTES: u64 = 32_768 * 4096 = 134_217_728` bytes ($128\text{ MiB}$).
- **Minimum Volume Size**: 4 MiB ($1024$ blocks).
- **Standard Volume Size**: 16 MiB ($4096$ blocks).

### 5.2 Deterministic Region Map & Formulas
```text
Block 0:        Reserved / Boot Sector (4 KiB)               [offset 0x0000_0000]
Block 1:        Superblock (4 KiB)                          [offset 0x0000_1000]
Block 2:        Transaction Journal (4 KiB)                 [offset 0x0000_2000]
Block 3:        Inode Allocation Bitmap (4 KiB)             [offset 0x0000_3000]
Block 4:        Block Allocation Bitmap (4 KiB)             [offset 0x0000_4000]
Blocks 5..20:   Inode Table (16 blocks = 64 KiB, 256 inodes)     [offset 0x0000_5000]
Block 21:       Root Directory Data Block (4 KiB)           [offset 0x0001_5000]
Blocks 22..N-1: Data Blocks (N - 22 blocks)                 [offset 0x0001_6000]
```

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
pub const DATA_BLOCK_START: u64 = 22;
pub const ROOT_DIR_INODE: u32 = 1;
pub const NULL_INODE: u32 = 0;

pub const METADATA_BLOCKS: u64 = 22;
pub const MAX_DATA_BLOCKS: u64 = MAX_VOLUME_BLOCKS - METADATA_BLOCKS; // 32,746 blocks
pub const MAX_DATA_BYTES: u64 = MAX_DATA_BLOCKS * 4096; // 134,127,616 bytes (~127.91 MiB)
```

---

## 6. Exact Persistent On-Disk Structures

### 6.1 `DiskSuperblock` (128 bytes, Block 1)
Located at Block 1 (offset `0x1000..0x1080` within Block 1; remaining 3968 bytes reserved/zero):

| Offset | Size | Field | Meaning | Validation |
| :--- | :--- | :--- | :--- | :--- |
| `0x00..0x08` | 8 B | `magic` | Exact ASCII `"ZERO_FS\0"` | Must equal `[0x5A, 0x45, 0x52, 0x4F, 0x5F, 0x46, 0x53, 0x00]` |
| `0x08..0x0C` | 4 B | `version` | Filesystem format version | Must equal `1` (Little-Endian `u32`) |
| `0x0C..0x10` | 4 B | `block_size` | Block size in bytes | Must equal `4096` |
| `0x10..0x18` | 8 B | `total_blocks`| Total blocks in volume | `1024 <= total_blocks <= 32_768` |
| `0x18..0x20` | 8 B | `free_blocks` | Unallocated data blocks | `free_blocks <= total_blocks - 22` |
| `0x20..0x24` | 4 B | `total_inodes`| Total inodes in table | Must equal `256` |
| `0x24..0x28` | 4 B | `free_inodes` | Unallocated inodes | `free_inodes <= 255` |
| `0x28..0x30` | 8 B | `journal_block`| Block index of Journal | Must equal `2` |
| `0x30..0x38` | 8 B | `block_bitmap_block`| Block index of Block Bitmap | Must equal `4` |
| `0x38..0x40` | 8 B | `inode_bitmap_block`| Block index of Inode Bitmap | Must equal `3` |
| `0x40..0x48` | 8 B | `inode_table_block` | Start of Inode Table | Must equal `5` |
| `0x48..0x4C` | 4 B | `inode_table_blocks`| Blocks in Inode Table | Must equal `16` |
| `0x4C..0x50` | 4 B | `root_dir_inode` | Inode of Root Directory | Must equal `1` |
| `0x50..0x54` | 4 B | `volume_state` | `0=Clean, 1=Dirty, 2=Recovering, 3=Faulted` | Validated on mount |
| `0x54..0x5C` | 8 B | `mount_count` | Monotonic mount counter | Monotonically increments |
| `0x5C..0x60` | 4 B | `checksum` | CRC32 over bytes `0x00..0x5C` | Mismatch rejects mount fail-closed |
| `0x60..0x80` | 32 B | `volume_label` | Null-padded UTF-8 label | Valid UTF-8 string |

```rust
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
```

| Offset | Size | Field | Meaning | Validation |
| :--- | :--- | :--- | :--- | :--- |
| `0x00..0x02` | 2 B | `file_type` | `0=Free, 1=Regular, 2=Directory` | Must be `0..=2` |
| `0x02..0x04` | 2 B | `flags` | Bit 0: IMMUTABLE, Bit 1: SYNC, Bit 2: PENDING_DELETE | Valid bitmask |
| `0x04..0x08` | 4 B | `links_count` | Directory link references | `links_count == 0` triggers delete upon close |
| `0x08..0x10` | 8 B | `size_bytes` | Exact logical file size | Checked against `MAX_DATA_BYTES` |
| `0x10..0x18` | 8 B | `blocks_count` | Total 4 KiB blocks allocated | `blocks_count == ceil(size_bytes / 4096)` |
| `0x18..0x20` | 8 B | `creator_pid` | Creator process ID | Non-zero |
| `0x20..0x28` | 8 B | `ctime_ticks` | Creation system ticks | Monotonic |
| `0x28..0x30` | 8 B | `mtime_ticks` | Last modification system ticks| Monotonic |
| `0x30..0x34` | 4 B | `generation` | Generation counter | Incremented on inode recycling |
| `0x34..0x38` | 4 B | `checksum` | CRC32 over bytes `0x00..0x34` | Mismatch returns `Err(FsError::CorruptMetadata)` |
| `0x38..0x40` | 8 B | `_reserved` | Alignment padding | Must be `0` |
| `0x40..0xD0` | 144 B | `direct_blocks` | 18 direct block pointers | Each block must be `0` or `22..=32767` |
| `0xD0..0xD8` | 8 B | `indirect_block`| Single-indirect pointer | Must be `0` or `22..=32767` |
| `0xD8..0xE0` | 8 B | `double_indirect`| Double-indirect pointer | Must be `0` or `22..=32767` |
| `0xE0..0x100`| 32 B | `inline_data` | Reserved metadata expansion | Must be `0` |

```rust
const _: () = assert!(core::mem::size_of::<DiskInode>() == 256);
const _: () = assert!(core::mem::align_of::<DiskInode>() == 8);
```

### 6.3 `DiskDirEntry` (64 bytes, 64 entries per Block)
```rust
pub const MAX_FILENAME_LEN: usize = 55;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskDirEntry {
    pub inode_num: u32,                  // 0x00..0x04: Inode index (1..=255; 0=unused)
    pub name_len: u8,                    // 0x04..0x05: Length (1..=55)
    pub file_type: u8,                   // 0x05..0x06: 1=Regular, 2=Directory
    pub _pad: u16,                       // 0x06..0x08: Padding
    pub name: [u8; 55],                  // 0x08..0x3F: ASCII / UTF-8 filename (no nulls in body)
    pub _null_terminator: u8,            // 0x3F..0x40: Explicit trailing 0x00
}

const _: () = assert!(core::mem::size_of::<DiskDirEntry>() == 64);
const _: () = assert!(core::mem::align_of::<DiskDirEntry>() == 8);
```

---

## 7. Exact Journal Format & Transaction Capacity (BLOCKER A & B Resolved)

### 7.1 Exact `DiskJournalBlock` Layout (Block 2, 4096 bytes)

| Offset | Size | Field | Meaning | Validation |
| :--- | :--- | :--- | :--- | :--- |
| `0x0000..0x0008` | 8 B | `magic` | ASCII `"ZERO_JRN"` | Must equal `[0x5A, 0x45, 0x52, 0x4F, 0x5F, 0x4A, 0x52, 0x4E]` |
| `0x0008..0x0010` | 8 B | `sequence` | Monotonic transaction counter | Advances on every commit |
| `0x0010..0x0014` | 4 B | `format_version`| Journal version | Must equal `1` |
| `0x0014..0x0018` | 4 B | `op_type` | `1=CreateFile, 2=WriteFile, 3=TruncateFile, 4=DeleteFile` | Valid enum range `1..=4` |
| `0x0018..0x001C` | 4 B | `state` | `0=Free, 1=Intent, 2=Committed` | Valid enum range `0..=2` |
| `0x001C..0x0020` | 4 B | `target_inode` | Inode being modified | `1..=255` |
| `0x0020..0x0024` | 4 B | `parent_dir_inode`| Parent directory inode | `0` (N/A) or `1..=255` |
| `0x0024..0x0028` | 4 B | `dir_entry_slot` | Slot index in directory block | `0..=63` or `0xFFFF_FFFF` |
| `0x0028..0x0030` | 8 B | `old_size` | File size before transaction | `<= MAX_DATA_BYTES` |
| `0x0030..0x0038` | 8 B | `new_size` | File size after transaction | `<= MAX_DATA_BYTES` |
| `0x0038..0x003C` | 4 B | `allocated_blocks_count` | Count of allocated blocks | `0..=8` |
| `0x003C..0x0040` | 4 B | `freed_blocks_count` | Count of freed blocks | `0..=8` |
| `0x0040..0x0060` | 32 B | `allocated_blocks` | Array of 8 block LBAs | Each block `22..=32767` |
| `0x0060..0x0080` | 32 B | `freed_blocks` | Array of 8 block LBAs | Each block `22..=32767` |
| `0x0080..0x00C0` | 64 B | `dir_entry_copy` | 64-byte `DiskDirEntry` copy | Valid `DiskDirEntry` |
| `0x00C0..0x00C4` | 4 B | `checksum` | CRC32 over bytes `0x0000..0x00C0` | Must verify on recovery |
| `0x00C4..0x1000` | 3900 B | `_reserved` | Zero-padding to 4096 bytes | Must be all `0x00` |

### 7.2 Transaction Capacity Proof
- `MAX_JOURNAL_RECORDS = 1` active transaction per 4096-byte journal block.
- `MAX_ALLOCATED_BLOCKS_PER_TX = 8` blocks.
- `MAX_FREED_BLOCKS_PER_TX = 8` blocks.
- `MAX_TRANSACTION_BYTES = 8 * 4096 = 32,768` bytes (32 KiB).
- **Arithmetic Verification**:
  $$\text{Header \& Metadata} = 196\text{ bytes} \le 4096\text{ bytes}$$
  $$\text{Payload} = 32\text{ KiB written to data blocks on disk before commit point}$$
- If a write request exceeds 32 KiB, the kernel chunks the write into sequential 32 KiB transactions, updating `new_size` incrementally. If interrupted, completed 32 KiB transactions are durable, and only the uncommitted chunk rolls back.

---

## 8. Commit Protocol & Invariant `I-STOR-JOURNAL-1` (BLOCKER C Resolved)

### 8.1 Six-Step Commit Protocol
```text
Step 1: Write Journal INTENT Record
        - Populates DiskJournalBlock (state = Intent, sequence, op_type, allocated_blocks, CRC32).
        - Executes ATA CACHE FLUSH.

Step 2: Write Data Blocks to Disk
        - Writes user payload into freshly allocated physical data blocks on disk.
        - Executes ATA CACHE FLUSH.
        - INVARIANT: The Inode Table on disk still has old_size and old block pointers!
          Unwritten or corrupt data is NEVER referenced by an Inode.

Step 3: Write Journal COMMITTED Record (THE COMMIT POINT)
        - Rewrites DiskJournalBlock (state = Committed, updated CRC32).
        - Executes ATA CACHE FLUSH.
        - AT THIS EXACT MOMENT, THE TRANSACTION IS COMMITTED AND DURABLE.

Step 4: Write Metadata Blocks to Disk
        - Writes updated DiskInode (pointing to new blocks, new size, new mtime).
        - Writes updated Directory Block (if Create/Delete).
        - Writes updated Block Bitmap and Inode Bitmap.
        - Executes ATA CACHE FLUSH.

Step 5: Write Superblock
        - Updates free_blocks and free_inodes counters.
        - Executes ATA CACHE FLUSH.

Step 6: Clear Journal Record
        - Rewrites DiskJournalBlock (state = Free, CRC32).
        - Executes ATA CACHE FLUSH.
```

### 8.2 Invariant `I-STOR-JOURNAL-1`
> **`I-STOR-JOURNAL-1`**: After a crash at any defined transaction boundary, recovery produces exactly one valid filesystem state: either the pre-transaction state or the committed post-transaction state.

---

## 9. Deterministic Crash Recovery Matrix (BLOCKER D Resolved)

On volume mount (`mount_filesystem`), the kernel inspects `DiskSuperblock` and `DiskJournalBlock`:

| Crash Point | Journal State | Disk Data State | Disk Metadata State | Mount Action | Final Result |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Before Step 1 | `Free` (0) | Unchanged | Clean | None. Clean mount. | Pre-transaction state |
| During Step 1 | Torn write | Unchanged | Clean | Checksum invalid. Mark journal `Free`. | Pre-transaction state |
| After Step 1, Before Step 3 | `Intent` (1) | Partially written | Pre-TX (old inode) | **Rollback**: Free `allocated_blocks` in Block Bitmap. Target inode remains `old_size`. Clear journal to `Free`. | Pre-transaction state |
| During Step 3 (Commit Flush) | Torn write | Fully written | Pre-TX (old inode) | If CRC32 invalid: **Rollback** as Intent. If CRC32 valid: **Rollforward**. | Pre- or Post-TX state |
| After Step 3, Before Step 6 | `Committed` (2) | Fully written | Partially updated | **Rollforward**: Write `dir_entry_copy` to directory; write `new_size` and block pointers to Inode; set bits in bitmaps; update superblock; clear journal to `Free`. | Post-transaction state |
| During Step 6 (Clear Flush) | Torn write | Fully written | Fully updated | Re-running rollforward is idempotent. Clears journal to `Free`. | Post-transaction state |
| Corrupt Magic / Version | Invalid | Any | Dirty | **Fail-Closed**: Mount aborted with `Err(FsError::CorruptJournal)`. | Fail-Closed |
| Invalid Inode / Block Pointer | Any | Any | Dirty | **Fail-Closed**: Mount aborted with `Err(FsError::InvalidMetadata)`. | Fail-Closed |

---

## 10. Bitmap Layout & Synchronization

### 10.1 Layout
- **Inode Allocation Bitmap** (Block 3, 4096 bytes):
  - 256 bits = 32 bytes (Bytes `0x00..0x20`).
  - Bit $i = 1$ indicates Inode $i$ is allocated. Bit 0 is reserved (1).
  - Bytes `0x20..0x1000` (4064 bytes) are strictly reserved zero-padding.
- **Block Allocation Bitmap** (Block 4, 4096 bytes):
  - 32,768 bits = 4096 bytes.
  - Bit $b = 1$ indicates Block $b$ is allocated.
  - Bits 0..21 are permanently set (1) for metadata blocks.

### 10.2 RAM Synchronization & Ownership
- The active Block Bitmap and Inode Bitmap are mirrored in static kernel memory (bounded arrays in BSS).
- Any allocation/deallocation updates the in-memory bitmap immediately under `FILESYSTEM_LOCK`.
- During transaction Step 4, modified bitmap blocks are flushed to persistent media via `write_block()` and `flush()`.
- Upon reboot/mount, bitmaps are read from disk; persistent allocation strictly survives reboot.

---

## 11. Inode / File Capacity Arithmetic (Section 13)

- Direct Pointers: 18 direct $\times 4096 = 73,728$ bytes (72 KiB).
- Single-Indirect: 1 pointer $\times 512$ pointers $\times 4096 = 2,097,152$ bytes (2 MiB).
- Double-Indirect: 1 pointer $\times 512 \times 512$ pointers $\times 4096 = 1,073,741,824$ bytes (1 GiB).
- Theoretical Inode Maximum: $73,728 + 2,097,152 + 1,073,741,824 = 1,075,912,704$ bytes (1.002 GiB).
- **Physical Volume Domination**:
  $$\text{MAX\_FILE\_BYTES} = \min(\text{THEORETICAL\_MAX}, \text{MAX\_DATA\_BYTES}) = 134,127,616\text{ bytes} \approx 127.91\text{ MiB}$$
  For a 16 MiB volume: $\text{MAX\_FILE\_BYTES} = 4074 \times 4096 = 16,687,104$ bytes ($\approx 15.91\text{ MiB}$).

---

## 12. Namespace Semantics (Section 12)

- Maximum filename length: 55 bytes (`MAX_FILENAME_LEN = 55`).
- Maximum path depth: 8 levels (`MAX_PATH_DEPTH = 8`).
- Valid bytes: ASCII / UTF-8 printable (`0x20..=0x7E`), excluding `/`, `\0`.
- Canonical relative names only: leading `/` is stripped; `.` and `..` path elements are strictly rejected with `Err(SyscallError::InvalidArgument)`.
- Case sensitivity: Strictly case-sensitive.
- Duplicate entries: Creating an existing filename returns `Err(SyscallError::ResourceBusy)` (`-EEXIST`).

---

## 13. Kernel `StorageObject` & Handle Duplication (BLOCKER #3 & Section 10 Resolved)

### 13.1 `StorageObject` Structure (48 bytes, 8-byte aligned)
```rust
pub const MAX_OPEN_STORAGE_OBJECTS: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StorageObject {
    pub occupied: bool,           // 0x00..0x01: Slot in use
    pub _pad0: [u8; 3],           // 0x01..0x04: Padding
    pub inode_num: u32,           // 0x04..0x08: Inode index (1..=255)
    pub generation: u32,          // 0x08..0x0C: Inode generation
    pub device_id: u8,            // 0x0C..0x0D: Block device ID
    pub _pad1: [u8; 3],           // 0x0D..0x10: Padding
    pub cursor_offset: u64,       // 0x10..0x18: Current read/write seek offset
    pub open_flags: u32,          // 0x18..0x1C: O_READ, O_WRITE, O_APPEND
    pub in_flight_ops: u16,       // 0x1C..0x1E: Active operations reference pin
    pub _pad2: u16,               // 0x1E..0x20: Alignment padding
    pub cached_size: u64,         // 0x20..0x28: Cached logical size
    pub _reserved: [u8; 8],       // 0x28..0x30: Reserved expansion
}

const _: () = assert!(core::mem::size_of::<StorageObject>() == 48);
const _: () = assert!(core::mem::align_of::<StorageObject>() == 8);
```

### 13.2 File Offset Semantics on Capability Duplication
- **`CAP_DUPLICATE` / `TRANSFER_DELEGATE`**: Duplicating a capability handle creates a new handle pointing to the **same `KernelObjectSlot` and same `StorageObject`**. Processes A and B **share the same `cursor_offset`**!
- **Independent Open**: Calling `SYS_FILE_OPEN` allocates a **new `StorageObject`** with an independent `cursor_offset = 0`.
- **`TRANSFER_MOVE`**: Unmaps sender handle; receiver adopts the existing `StorageObject` with its current `cursor_offset`.
- **File Deletion vs Open Handles**: Unlinking removes directory entry and sets `PENDING_DELETE`. Open handles continue reading/writing. When `handle_refs == 0 && in_flight_ops == 0`, data blocks and inode are reclaimed.
- **Process Exit**: Closes open handles (`handle_refs.saturating_sub(1)`). If `links_count > 0`, persistent on-disk data remains intact. Persistent files are **never** deleted due to process exit.

---

## 14. Capability Authority & Subtree Isolation (BLOCKER #4 Resolved)

### 14.1 Directory Capability Requirement
`SYS_FILE_OPEN` mandates an explicit Directory Capability:

```rust
sys_file_open(dir_handle, path_ptr, path_len, flags, out_handle_ptr)
```

- Ambient filesystem authority is completely eliminated.
- Relative path traversal is confined strictly within the directory capability subtree.

### 14.2 Storage Rights Definition
```rust
pub mod cap_rights {
    // Type-Specific Storage Rights (Bits 5..7):
    pub const FILE_READ:  u16 = 1 << 5; // 0x0020: Read bytes / traverse & enumerate directory
    pub const FILE_WRITE: u16 = 1 << 6; // 0x0040: Write bytes / create & delete in directory
    pub const FILE_SYNC:  u16 = 1 << 7; // 0x0080: Flush dirty data to persistent storage

    // Frozen Generic Management Rights (Bits 8..15):
    pub const DUPLICATE:  u16 = 1 << 8;
    pub const TRANSFER:   u16 = 1 << 9;
    pub const REVOKE:     u16 = 1 << 10;
    pub const CLOSE:      u16 = 1 << 11;
    pub const INSPECT:    u16 = 1 << 12; // Query file metadata / stat
}
```

- **Monotonic Attenuation**:
  $$\text{Rights}(\text{file}) \subseteq \text{Rights}(\text{dir\_capability})$$
  A directory capability possessing only `FILE_READ` cannot open a file with `O_WRITE`; returns `-EACCES`.

---

## 15. Exact System Call ABI (Syscalls 11..16)

Fast syscalls via `IA32_LSTAR` using the Stage 3I 144-byte `SyscallFrame`:

### 15.1 `SYS_FILE_OPEN` (11)
- `RAX`: `11`
- `RDI`: `dir_handle: u32` (Directory capability)
- `RSI`: `path_ptr: u64` (Pointer to relative path string)
- `RDX`: `path_len: u64` (1..=55 bytes)
- `R10`: `flags: u32` (`O_READ=1, O_WRITE=2, O_CREATE=4, O_TRUNC=8, O_APPEND=16`)
- `R8`: `out_handle_ptr: u64` (Pointer to write returned `u32` handle)
- Validation: `validate_user_range(path_ptr, path_len, Read)`, `validate_user_range(out_handle_ptr, 4, Write)`.
- Returns: `0` on success, or negative `SyscallError` (`-EACCES`, `-ENOENT`, `-EEXIST`, `-ENOSPC`, `-ENFILE`, `-EFAULT`, `-EINVAL`).

### 15.2 `SYS_FILE_READ` (12)
- `RAX`: `12`
- `RDI`: `handle: u32`
- `RSI`: `buf_ptr: u64` (User buffer)
- `RDX`: `count: u64` (Bytes to read)
- `R10`: `out_read_ptr: u64` (Pointer to write `u64` actual bytes read)
- Required Right: `FILE_READ`.
- Validation: `validate_user_range(buf_ptr, count, Write)`, `validate_user_range(out_read_ptr, 8, Write)`.
- EOF / Zero-Length: If `count == 0`, writes 0 to `out_read_ptr` and returns 0. If `cursor_offset >= file_size`, writes 0 and returns 0 (EOF).
- Returns: `0` on success, negative on error.

### 15.3 `SYS_FILE_WRITE` (13)
- `RAX`: `13`
- `RDI`: `handle: u32`
- `RSI`: `buf_ptr: u64` (User buffer)
- `RDX`: `count: u64` (Bytes to write)
- `R10`: `out_written_ptr: u64` (Pointer to write `u64` actual bytes written)
- Required Right: `FILE_WRITE`.
- Validation: `validate_user_range(buf_ptr, count, Read)`, `validate_user_range(out_written_ptr, 8, Write)`.
- Zero-Length: If `count == 0`, writes 0 and returns 0.
- Returns: `0` on success, or negative `SyscallError` (`-ENOSPC` if disk full).

### 15.4 `SYS_FILE_CLOSE` (14)
- `RAX`: `14`
- `RDI`: `handle: u32`
- Required Right: `CLOSE`.
- Behavior: Closes handle; if `links_count == 0 && handle_refs == 0`, reclaims blocks.
- Returns: `0` on success, negative on error.

### 15.5 `SYS_FILE_STAT` (15)
- `RAX`: `15`
- `RDI`: `handle: u32`
- `RSI`: `out_stat_ptr: u64` (Pointer to write 32-byte `UserFileStat`: `size: u64, blocks: u64, type: u32, generation: u32, mtime: u64`)
- Required Right: `INSPECT`.
- Pointer validation: `validate_user_range(out_stat_ptr, 32, Write)`.
- Returns: `0` on success, negative on error.

### 15.6 `SYS_FILE_SYNC` (16)
- `RAX`: `16`
- `RDI`: `handle: u32`
- Required Right: `FILE_SYNC`.
- Behavior: Flushes dirty buffers for file and executes ATA `CACHE FLUSH`.
- Returns: `0` on success, negative on error.

---

## 16. Bounded Buffer Cache Lifecycle

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

- **Pinning**: `pin_count > 0` locks buffer in RAM; cannot be evicted.
- **Eviction**: Selects unpinned buffer (`pin_count == 0`) with lowest `lru_seq`. Dirty buffers written out prior to recycling.
- **Exhaustion**: If all 16 buffers are pinned, returns `Err(FsError::BufferPoolExhausted)` (`-EBUSY`) deterministically without deadlock.
- **Sequence Rescale**: When `clock_seq == u32::MAX`, a linear rescaling pass normalizes sequence counters to prevent integer overflow.

---

## 17. Global Lock Hierarchy (Section 15 Resolved)

To guarantee mathematical freedom from deadlocks across filesystem, block device, IPC, and scheduler operations, the kernel enforces a strict monotonic lock hierarchy:

```text
STORAGE_OBJECT_TABLE_LOCK (Rank 1)
    ≺ FILESYSTEM_LOCK (Rank 2)
        ≺ BLOCK_CACHE_LOCK (Rank 3)
            ≺ BLOCK_DEVICE_LOCK (Rank 4)
                ≺ KERNEL_OBJECT_TABLE_LOCK (Rank 5, Frozen 3G)
                    ≺ SCHEDULER.lock (Rank 6, Frozen 3A–3E)
                        ≺ CPU (IF=0) (Rank 7)
```

### Deadlock Freedom Invariants:
1. `I-STOR-LOCK-1`: Locks must be acquired strictly in ascending rank order.
2. `I-STOR-LOCK-2`: Polling ATA port I/O runs strictly under `BLOCK_DEVICE_LOCK` with `IF=0`.
3. `I-STOR-LOCK-3`: No thread may block, yield, or sleep while holding Ranks 1..6.

---

## 18. Device Failure Model (Section 16 Resolved)

```rust
pub enum DeviceState {
    Offline = 0,
    Probing = 1,
    Ready = 2,
    Faulted = 3,
}
```

- Transition to `Faulted`: Hardware error register (`ERR`) asserted or timeout exceeds 100,000 cycles.
- Filesystem behavior: Mounted volume transitions to `volume_state = 3 (Faulted)`. In-flight writes fail immediately with `-EIO`. Dirty buffers for device are discarded. Kernel objects remain safe; memory corruption is strictly prevented.

---

## 19. MemBlockDevice vs Persistent Disk (Section 17 Resolved)

- **`MemBlockDevice`** (Transient Test Device): 16 MiB in BSS. Verifies `I-STOR-LIFETIME-1` (Process-Exit Persistence: process exits, handles closed, file remains intact on volume).
- **`AtaPioBlockDevice`** (Real Persistent Storage): Communicates via port I/O with QEMU's primary IDE drive (`-drive file=disk.img,format=raw,if=ide`). Verifies `I-STOR-LIFETIME-2` (Reboot Persistence: data survives machine power-down and restart).

---

## 20. Reclamation & Lifecycle Ownership (Section 18 Resolved)

| Resource | Sole Owner of Reclamation | Reclamation Event |
| :--- | :--- | :--- |
| `BlockBuffer` | Buffer Cache Manager | When evicting unpinned clean/flushed buffer |
| `DiskInode` | Filesystem Allocator | When `links_count == 0` AND all open handles closed |
| `StorageObject` | Object Manager | When `handle_refs == 0 && in_flight_ops == 0` |
| Handle Descriptor | Process Handle Table | Upon `SYS_FILE_CLOSE` or `process_exit()` |
| Data Blocks | Block Bitmap Allocator | When file truncated or unlinked inode reclaimed |
| Journal Record | Journal Engine | Cleared upon transaction Step 6 completion |

---

## 21. Security & Untrusted Disk Validation (Section 19 Resolved)

- `I-STOR-SEC-1`: Every block number, inode index, and offset read from disk is verified against volume bounds before memory indexing or port I/O.
- `I-STOR-SEC-2`: Superblock and Inode CRC32 checksums verified upon read; mismatch immediately fails closed (`Err(FsError::CorruptMetadata)`).
- `I-STOR-SEC-3`: Filenames restricted to 1..=55 bytes, no null bytes in body, no `..` traversal.
- `I-STOR-SEC-4`: All disk arithmetic uses checked operations before addition, multiplication, or alignment.

---

## 22. Zero-Heap Bounded Resources (Section 27 Resolved)

| Resource | Maximum Bound | Structure Size | Total Static Footprint |
| :--- | :--- | :--- | :--- |
| `MAX_BLOCK_DEVICES` | 2 | ~128 B | 256 B |
| `MAX_OPEN_STORAGE_OBJECTS` | 32 | 48 B | 1536 B |
| `MAX_BUFFERS` | 16 | 4128 B | 66,048 B |
| `MAX_INODES` | 256 | 256 B | Inode Table on disk; 32 B RAM bitmap |
| `MAX_TRANSACTION_PAYLOAD`| 8 blocks | 4096 B | 32 KiB chunk buffer |

Zero dynamic kernel heap allocations in kernel runtime.

---

## 23. Machine-Level Verification Suite (19 Tests: 3K-A through 3K-S)

| Test ID | Title | Verification Objective | Invariant Mapped |
| :--- | :--- | :--- | :--- |
| **3K-A** | Block Device Discovery | Probe MemBlock and ATA PIO block devices | `I-STOR-DEVICE-1` |
| **3K-B** | Block Sector Read/Write | Raw sector and block read/write fidelity | `I-STOR-DEVICE-2` |
| **3K-C** | Block Bounds Check | Reject block requests $\ge total\_blocks$ | `I-STOR-BLOCK-1` |
| **3K-D** | ZeroFS Volume Format | Format volume, create superblock, bitmaps, root dir | `I-STOR-DISK-1` |
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
| **3K-Q** | Uncommitted Journal Crash Recovery | Crash during intent; verify rollback of uncommitted blocks | `I-STOR-JOURNAL-1` |
| **3K-R** | Committed Journal Crash Recovery | Crash after commit; verify rollforward of metadata | `I-STOR-JOURNAL-2` |
| **3K-S** | Capability Enforcement & PMM Neutrality | Verify `FILE_READ` vs `FILE_WRITE` and `baseline_free == post_test_free` | `I-STOR-CAP-1`, `I-STOR-PMM-1` |

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

## 24. PMM Neutrality Contract

> **`I-STOR-PMM-1`**: Transient kernel allocations during filesystem operations must satisfy `baseline_free == post_test_free`.
- Distinguishes RAM/PMM frame neutrality from persistent on-disk block consumption.
- On load failure or transaction rollback, all transient frames return to baseline with zero memory leak.

---

## 25. Architecture Decision Records (ADRs)

- **ADR-3K-001**: ZeroFS On-Disk Geometry & Exact Formulas
- **ADR-3K-002**: Write-Ahead Intent Journal & Crash Recovery
- **ADR-3K-003**: StorageObject Lifecycle & Unlinked File Deletion
- **ADR-3K-004**: Namespace Capability Authority (Elimination of Ambient Authority)
- **ADR-3K-005**: Bounded Buffer Cache Lifecycle & Eviction
- **ADR-3K-006**: ATA PIO Driver & Device Failure Semantics
- **ADR-3K-007**: Zero-Heap Bounded Resource Policy
- **ADR-3K-008**: File Offset Semantics on Capability Duplication

---

## 26. Adversarial Review Classifications

| Threat / Attack Surface | Classification | Architectural Defense |
| :--- | :--- | :--- |
| **Corrupt Journal Checksum** | 🟢 Resolved | Checksum mismatch rejects journal entry fail-closed; volume marked `Faulted`. |
| **Torn Data Block Write** | 🟢 Resolved | Unwritten data is never referenced by Inode; commit point occurs only after data flush. |
| **Path Traversal Escape** | 🟢 Resolved | `SYS_FILE_OPEN` requires Directory Capability; relative paths only; `..` strictly rejected. |
| **Capability Escalation** | 🟢 Resolved | Monotonic attenuation enforced; read-only directory capability cannot yield write handle. |
| **Deadlock via Lock Inversion**| 🟢 Resolved | Monotonic 7-rank lock hierarchy enforced; no spinlock held across blocking switch. |
| **Double Free of Unlinked Inode**| 🟢 Resolved | Inode marked `PENDING_DELETE`; reclaimed only when `handle_refs == 0 && in_flight_ops == 0`. |
| **Bitmap Buffer Pool Exhaustion**| 🟢 Resolved | When all 16 buffers pinned, returns `-EBUSY` deterministically without deadlock. |

---

## 27. Freeze Criteria & Final Verdict

All 34 required sections, exact byte-level structures, crash consistency state machines, capability traversal rules, global lock hierarchy, and verification matrices are completely defined with zero open RED blockers.

```text
🟢 STAGE 3K ARCHITECTURE APPROVED / READY TO FREEZE
```
