# Stage 3K — Storage / Filesystem Architecture Specification (Rev5)

**Status**: 🟢 APPROVED / READY TO FREEZE (Rev5)  
**Contract Author**: Project Zero Kernel Nucleus  
**Target Milestone**: Stage 3K  
**Dependencies**: Stage 2 (PMM, VMM), Stage 3A–3E (Kernel Threads & Scheduler), Stage 3F (Processes & AddressSpace), Stage 3G (IPC & Objects), Stage 3H (Capabilities & Handles), Stage 3I (User Space & Syscall Interface), Stage 3J (ELF / Program Execution).

---

## 1. Executive Summary & Philosophy

Stage 3K establishes the persistent storage primitive for Project Zero / ZeroOS.

ZeroOS is **not** a Unix clone. It rejects Unix-style ambient path authority, UID/GID permissions, and monolithic filesystem hierarchies in favor of a typed, capability-authorized persistent resource graph:

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

Stage 3K establishes **ZeroFS**, a capability-native persistent filesystem with:
- Strict 4096-byte page-aligned block geometry.
- A physical/logical hybrid Write-Ahead Intent Journal (`Block 2`, 4096 bytes) storing complete pre- and post-transaction inode images (`DiskInode`, 256 B), directory entries (`DiskDirEntry`, 64 B), and allocated/freed block vectors.
- **Copy-on-Write (CoW) Indirect Metadata** (`ADR-3K-009`): Single-indirect metadata blocks are NEVER mutated in place on disk. All indirect block modifications allocate a new block and swap the pointer at commit, guaranteeing that uncommitted transactions leave pre-transaction indirect metadata 100% physically intact.
- Deterministic journal-directed bitmap reconciliation: Inode and Block bitmaps are rolled back or rolled forward using explicit journal record fields without inverse heuristic derivation.
- Strict 6-step transactional commit protocol guaranteeing invariant `I-STOR-JOURNAL-1`.
- Capability-authorized namespace traversal: opening any file requires an explicit Directory Capability. Ambient authority is eliminated.
- Zero dynamic kernel heap allocations: all tables bounded in static BSS (`MAX_OPEN_STORAGE_OBJECTS = 32`, `MAX_BUFFERS = 16`, `MAX_INODES = 256`).
- Polled Primary ATA PIO (`0x1F0..0x1F7`) with bounded timeouts (`MAX_ATA_POLL_ITERATIONS = 100_000`) alongside an in-memory `MemBlockDevice`.

---

## 2. Repository Findings & Frozen ABI Invariance

Inspection of the authoritative repository source code confirms:

| Subsystem / Structure | Authoritative Code Contract | Stage 3K Architectural Rule |
| :--- | :--- | :--- |
| **Port I/O** (`kernel/src/hal/arch/x86_64/cpu.rs`) | `inb(port: u16) -> u8`, `outb(port: u16, val: u8)`, `io_wait()` | Add `inw(port: u16) -> u16` and `outw(port: u16, val: u16)`. Primary ATA PIO uses ports `0x1F0..0x1F7`, `0x3F6`. |
| **Memory / PMM** (`kernel/src/mm/pmm.rs`, `vmm.rs`) | 4096-byte page frames. HHDM maps physical `[0, 1 GiB)` at `0xFFFF_8000_0000_0000`. Zero kernel heap. | `BLOCK_SIZE = 4096` bytes. Buffer cache buffers are 4 KiB and HHDM-addressable. |
| **Kernel Objects** (`kernel/src/ipc/object.rs`) | `KernelObjectType`: `Free=0, Channel=1, ShmObject=2, Event=3, Process=4`. `MAX_KERNEL_OBJECTS = 256`. `KernelObjectSlot = 40 B`. | Append `StorageObject = 5` to `KernelObjectType` (`#[repr(u8)]`). `KernelObjectSlot` size (40 B) and alignment (8 B) remain identical. |
| **Capabilities** (`kernel/src/cap/types.rs`) | `CapabilityNode`: 24 B (8-B aligned). `cap_rights`: Bits 0..4 used; Bits 8..13 used for generic management. | Allocate Bits 5..7 for storage: `FILE_READ = 1 << 5`, `FILE_WRITE = 1 << 6`, `FILE_SYNC = 1 << 7`. `CapabilityNode` remains 24 B. |
| **Handles** (`kernel/src/ipc/handle.rs`) | `Handle`: 16 B. `HandleTable`: 520 B (`MAX_HANDLES_PER_PROCESS = 32`). | Frozen ABI unchanged. Handle points to `KernelObjectSlot` whose `pool_index` references `StorageObject`. |
| **Process / Thread** (`task/process.rs`, `task/thread.rs`) | `Process`: 128 B. `KernelThread`: 176 B. `PerCpu`: 48 B. | Frozen ABIs completely untouched. |
| **Syscall ABI** (`syscall/abi.rs`, `numbers.rs`) | 144-B `SyscallFrame` via `IA32_LSTAR`. Syscalls 1..10 frozen (`SYS_EXIT..SYS_CAP_DERIVE`). | Define `SYS_FILE_OPEN = 11`, `SYS_FILE_READ = 12`, `SYS_FILE_WRITE = 13`, `SYS_FILE_CLOSE = 14`, `SYS_FILE_STAT = 15`, `SYS_FILE_SYNC = 16`. |
| **Lock Order** (`ipc/object.rs`, `task/scheduler.rs`) | `KERNEL_OBJECT_TABLE_LOCK ≺ SCHEDULER.lock ≺ CPU(IF=0)`. | Global order: `FILESYSTEM_LOCK ≺ STORAGE_OBJECT_TABLE_LOCK ≺ BLOCK_CACHE_LOCK ≺ BLOCK_DEVICE_LOCK ≺ KERNEL_OBJECT_TABLE_LOCK ≺ SCHEDULER.lock ≺ CPU(IF=0)`. |

---

## 3. Goals & Non-Goals

### 3.1 Goals
1. **Persistent Block Storage Primitive**: Provide a deterministic block device layer supporting polled Primary ATA PIO (`0x1F0..0x1F7`) alongside an in-memory `MemBlockDevice`.
2. **ZeroFS Persistent Geometry**: Implement an extent/direct-pointer filesystem format with 4096-byte blocks, CRC32 integrity checksums, and strict byte-defined structures.
3. **Write-Ahead Intent Journaling & CoW Indirect Blocks**: Bounded 4096-byte transaction log with Copy-on-Write indirect blocks, guaranteeing invariant `I-STOR-JOURNAL-1`.
4. **Capability-Native Authority**: Eliminate ambient path authority. All file access requires an explicit Directory Capability.
5. **Zero Dynamic Heap**: All block buffers (16 buffers = 64 KiB), open storage objects (32 slots), and bitmaps reside in static BSS.
6. **Machine Verification**: Deterministic 20-test bare-metal machine suite (`3K-A` through `3K-T`) with exact PMM neutrality.

### 3.2 Non-Goals (Explicitly Deferred)
- Dynamic partitioning (GPT/MBR).
- Asynchronous DMA / interrupt-driven AHCI / NVMe / virtio-blk.
- Double-indirect block pointers (deferred to Stage 3L volume expansion per `ADR-3K-001`).
- Multi-user Unix permissions (UID/GID, chmod/chown) or POSIX hard links.
- Distributed / network storage.
- Loading ELF executables directly from disk (deferred to Stage 3L/Workspace; Stage 3J loader continues loading from memory slices).

---

## 4. ZeroFS Layered Architecture

```text
Layer 6: System Call Interface (sys_file_open, sys_file_read, sys_file_write, sys_file_close, sys_file_stat, sys_file_sync)
    ↓
Layer 5: Capability & Kernel Object Layer (Directory Capability -> HandleTable -> CapabilityNode -> StorageObject)
    ↓
Layer 4: Namespace & File Abstraction (Directory Entries, Relative Path Resolution, Inode Descriptors)
    ↓
Layer 3: ZeroFS Filesystem & Intent Log (Superblock, Bitmaps, Write-Ahead Journal, Inode Table, CoW Indirect Engine)
    ↓
Layer 2: Bounded Block Buffer Cache (16 static 4 KiB buffers, pin counts, LRU eviction, dirty tracking)
    ↓
Layer 1: Block Device Abstraction (BlockDevice trait: read_block, write_block, flush, block_count)
    ↓
Layer 0: Physical Hardware / Mock (Primary ATA PIO Master 0x1F0..0x1F7 / In-Memory MemBlockDevice)
```

---

## 5. Block Device Architecture

### 5.1 `BlockDevice` Trait
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

### 5.2 Hardware Drivers
1. **`AtaPioBlockDevice`**:
   - Primary ATA channel ports: Data `0x1F0`, Features/Error `0x1F1`, Sector Count `0x1F2`, LBA Low `0x1F3`, LBA Mid `0x1F4`, LBA High `0x1F5`, Device/Head `0x1F6`, Command/Status `0x1F7`, Control `0x3F6`.
   - 28-bit LBA addressing. Master drive selected via `0xE0 | ((lba >> 24) & 0x0F)`.
   - Commands: `0xEC` (IDENTIFY), `0x20` (READ SECTORS), `0x30` (WRITE SECTORS), `0xE7` (CACHE FLUSH).
   - Polling loop with bounded timeout (`MAX_ATA_POLL_ITERATIONS = 100_000` cycles) under `BLOCK_DEVICE_LOCK` with interrupts disabled (`IF=0`).
2. **`MemBlockDevice`**:
   - 16 MiB statically allocated in BSS. Provides deterministic in-kernel machine testing.

```rust
pub const MAX_BLOCK_DEVICES: usize = 2;
// Device 0: MemBlockDevice (Transient testing)
// Device 1: AtaPioBlockDevice (Persistent hardware)
```

---

## 6. Exact Persistent Disk Layout & Volume Capacity

### 6.1 Volume Capacity Definitions & Mathematical Proof
Let $B = 4096$ bytes, $S = 512$ bytes ($8$ sectors/block), and $N_{\text{blocks}}$ be total volume blocks.
- **Maximum Volume Blocks**: `MAX_VOLUME_BLOCKS: u64 = 32_768` filesystem blocks.
- **Maximum Volume Bytes**: `MAX_VOLUME_BYTES: u64 = 32_768 * 4096 = 134_217_728` bytes ($128\text{ MiB}$).
- **Minimum Volume Size**: 4 MiB ($1024$ blocks).
- **Standard Volume Size**: 16 MiB ($4096$ blocks).
- **Metadata Overhead**: `METADATA_BLOCKS: u64 = 22` blocks (Blocks 0..21).
- **Maximum Data Blocks**: `MAX_DATA_BLOCKS: u64 = MAX_VOLUME_BLOCKS - METADATA_BLOCKS = 32_746` blocks.
- **Maximum Data Bytes**: `MAX_DATA_BYTES: u64 = 32_746 * 4096 = 134_127_616` bytes ($\approx 127.91\text{ MiB}$).

### 6.2 Deterministic Region Map
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
```

---

## 7. Exact Persistent On-Disk Structures

### 7.1 `DiskSuperblock` (128 bytes, Block 1)
Located at Block 1 (offset `0x1000..0x1080`; remaining 3968 bytes reserved/zero):

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

### 7.2 `DiskInode` (256 bytes, 16 Inodes per Block)
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
| `0xD8..0x100`| 40 B | `_reserved_expansion`| Reserved for Stage 3L indirect expansion | Must be `0` |

```rust
const _: () = assert!(core::mem::size_of::<DiskInode>() == 256);
const _: () = assert!(core::mem::align_of::<DiskInode>() == 8);
```

### 7.3 `DiskDirEntry` (64 bytes, 64 entries per Block)
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

## 8. Exact Journal Byte Layout & Checksum

### 8.1 Exact Byte Table for `DiskJournalBlock` (Block 2, 4096 bytes)

The journal employs a **physical/logical hybrid format**: it stores complete pre-transaction and post-transaction inode images (`DiskInode`, 256 B each), the directory entry image (`DiskDirEntry`, 64 B), and exact allocated/freed block vectors:

| Offset | Size | Field | Endianness | Meaning | Valid Range |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `0x0000..0x0008` | 8 B | `magic` | N/A | ASCII `"ZERO_JRN"` | `[0x5A, 0x45, 0x52, 0x4F, 0x5F, 0x4A, 0x52, 0x4E]` |
| `0x0008..0x0010` | 8 B | `sequence` | Little-Endian | Monotonic transaction counter | `1..=u64::MAX` |
| `0x0010..0x0014` | 4 B | `format_version`| Little-Endian | Journal format version | Must equal `1` |
| `0x0014..0x0018` | 4 B | `op_type` | Little-Endian | `1=Create, 2=Write, 3=Truncate, 4=Delete` | `1..=4` |
| `0x0018..0x001C` | 4 B | `state` | Little-Endian | `0=Free, 1=Intent, 2=Committed` | `0..=2` |
| `0x001C..0x0020` | 4 B | `target_inode_num` | Little-Endian | Inode being mutated | `1..=255` |
| `0x0020..0x0024` | 4 B | `parent_dir_inode`| Little-Endian | Parent directory inode | `0` (N/A) or `1..=255` |
| `0x0024..0x0028` | 4 B | `dir_entry_slot` | Little-Endian | Slot index in directory block | `0..=63` or `0xFFFF_FFFF` |
| `0x0028..0x002C` | 4 B | `allocated_blocks_count` | Little-Endian | Count of allocated blocks | `0..=10` |
| `0x002C..0x0030` | 4 B | `freed_blocks_count` | Little-Endian | Count of freed blocks | `0..=10` |
| `0x0030..0x0058` | 40 B | `allocated_blocks` | Little-Endian | 10 $\times$ `u32` block LBAs | Each block `22..=32767` |
| `0x0058..0x0080` | 40 B | `freed_blocks` | Little-Endian | 10 $\times$ `u32` block LBAs | Each block `22..=32767` |
| `0x0080..0x00C0` | 64 B | `dir_entry_copy` | Mixed | 64-byte `DiskDirEntry` copy | Valid `DiskDirEntry` |
| `0x00C0..0x01C0` | 256 B| `old_inode_image` | Mixed | Exact pre-transaction `DiskInode` | Valid `DiskInode` |
| `0x01C0..0x02C0` | 256 B| `new_inode_image` | Mixed | Exact post-transaction `DiskInode`| Valid `DiskInode` |
| `0x02C0..0x02C4` | 4 B | `checksum` | Little-Endian | CRC32 over bytes `0x0000..0x02C0` | Verified on mount |
| `0x02C4..0x1000` | 3388 B| `_reserved` | N/A | Zero-padding to 4096 bytes | Must be all `0x00` |

### 8.2 Exact Checksum Coverage & Padding Arithmetic Proof
$$\text{Metadata Span} = [0x0000..0x02C0) = 704\text{ bytes}$$
$$\text{Checksum Field} = [0x02C0..0x02C4) = 4\text{ bytes}$$
$$\text{Reserved Padding} = [0x02C4..0x1000) = 3388\text{ bytes}$$
$$\text{Total Block Size} = 704 + 4 + 3388 = 4096\text{ bytes}$$
$$\text{CRC32 Coverage} = \text{CRC32}(\text{bytes}[0..704])$$
`assert!(core::mem::size_of::<DiskJournalBlock>() == 4096);`

---

## 9. Mutated Metadata Enumeration & Copy-on-Write Architecture (ADR-3K-009)

### 9.1 Exhaustive Physical Metadata Block Enumeration
Every filesystem transaction can mutate a strictly bounded set of physical metadata blocks on disk:

| Metadata Structure | Physical Disk Location | Modification Method | Crash Recovery Strategy | Maximum Mutated Blocks / TX |
| :--- | :--- | :--- | :--- | :--- |
| **Inode** | Block `5 + (target_inode / 16)` (offset `target_inode % 16 * 256`) | Full 256-byte snapshot blit | **Physical Image Blit**: `old_inode_image` on Rollback; `new_inode_image` on Rollforward | 1 block |
| **Directory Entry** | Root Dir (Block 21) or child dir block (offset `dir_entry_slot * 64`) | Full 64-byte snapshot blit | **Physical Entry Blit**: `dir_entry_copy` written/cleared based on `op_type` | 1 block |
| **Single-Indirect Block** | Physical data block (`22..=32767`) | **Copy-on-Write (CoW)** | **Never Mutated In-Place**: New block allocated; old block preserved. Rollback leaves old block intact; Rollforward swaps to new block | 1 block (CoW replacement) |
| **Block Allocation Bitmap** | Block 4 (4096 bytes) | Bit-level set/clear | **Journal-Directed Reconciliation**: Bits in `allocated_blocks` and `freed_blocks` reconciled deterministically | 1 block |
| **Inode Allocation Bitmap** | Block 3 (4096 bytes) | Bit-level set/clear | **Journal-Directed Reconciliation**: Bit `target_inode_num` set/cleared based on `op_type` | 1 block |
| **Superblock** | Block 1 (128 bytes) | Counter updates (`free_blocks`, `free_inodes`) | **Header Flush**: Counters updated at transaction completion (Step 5) | 1 block |

### 9.2 Copy-on-Write (CoW) Architecture for Indirect Blocks
To solve the crash recovery problem where an in-place modified indirect block could leave uncommitted pointers if flushed prior to commit:

1. **Principle of Non-In-Place Mutation**: An existing on-disk single-indirect block is **NEVER modified in place**.
2. **CoW Allocation Sequence**:
   - Whenever an indirect entry must be added, updated, or removed, the filesystem allocates a **new block** $B_{\text{new}}$ from the Block Bitmap.
   - The contents of the old indirect block $B_{\text{old}}$ are copied into memory, modified with the new/cleared pointers, and assigned to $B_{\text{new}}$.
   - $B_{\text{new}}$ is recorded in `allocated_blocks`.
   - $B_{\text{old}}$ is recorded in `freed_blocks`.
   - `old_inode_image.indirect_block` remains $B_{\text{old}}$.
   - `new_inode_image.indirect_block` is set to $B_{\text{new}}$.
3. **Write Ordering**:
   - $B_{\text{new}}$ is flushed to disk in **Step 2 (Data & CoW Flush)** along with user data blocks.
   - Because $B_{\text{new}}$ is an unreferenced block, flushing it does NOT modify any existing filesystem metadata!
   - $B_{\text{old}}$ on disk is NEVER written or touched during the transaction.
4. **Commit Point (Step 3)**:
   - Journal commits.
5. **Post-Commit Flush (Step 4)**:
   - Inode table updated on disk (`new_inode_image` points to $B_{\text{new}}$).
   - Block Bitmap updated on disk ($B_{\text{new}}$ marked used, $B_{\text{old}}$ marked free).

---

## 10. Transaction Capacity Bounds & Worst-Case Proof

### 10.1 Transaction Limits
```rust
pub const MAX_JOURNAL_RECORDS: usize = 1;
pub const MAX_DATA_BLOCKS_PER_TX: usize = 8;        // 32 KiB data payload
pub const MAX_METADATA_BLOCKS_PER_TX: usize = 1;    // 1 single-indirect block (CoW replacement)
pub const MAX_TOTAL_ALLOCATIONS_PER_TX: usize = 9;  // 8 data + 1 CoW indirect <= 10
pub const MAX_TOTAL_FREES_PER_TX: usize = 9;        // 8 data + 1 old indirect <= 10
pub const MAX_TRANSACTION_BYTES: usize = 32_768;    // 32 KiB
```

### 10.2 Mathematical Proof of Worst-Case Transaction Fit
We evaluate every filesystem mutation path against the 10-entry journal capacity:

1. **Case A: Direct Write (Offset $\le 72\text{ KiB}$)**:
   - User data allocations: $\le 8$ blocks.
   - Indirect allocations: 0 blocks.
   - Total allocations: $\le 8 \le 10$. Total frees: $0 \le 10$.
2. **Case B: Direct-to-Indirect Boundary Crossing**:
   - Extends file across the 18-block boundary (allocates $k$ direct blocks, $8 - k$ indirect data blocks).
   - Allocates 1 newly initialized single-indirect block.
   - Total allocations: $8 \text{ data} + 1 \text{ indirect} = 9 \le 10$. Total frees: $0 \le 10$.
3. **Case C: Inside Indirect Range (CoW Extension)**:
   - User data allocations: $\le 8$ blocks.
   - Indirect allocations: 1 newly allocated CoW indirect block ($B_{\text{new}}$).
   - Indirect frees: 1 old indirect block ($B_{\text{old}}$).
   - Total allocations: $8 + 1 = 9 \le 10$. Total frees: $0 + 1 = 1 \le 10$.
4. **Case D: Create File**:
   - Allocates 1 inode in Inode Bitmap.
   - If directory block expands: allocates 1 directory data block.
   - Total allocations: $\le 1 \le 10$. Total frees: $0 \le 10$.
5. **Case E: Truncate within Indirect Range**:
   - Frees $\le 8$ user data blocks.
   - Allocates 1 new CoW indirect block ($B_{\text{new}}$) containing remaining pointers.
   - Frees 1 old indirect block ($B_{\text{old}}$).
   - Total allocations: $1 \le 10$. Total frees: $8 + 1 = 9 \le 10$.
6. **Case F: Truncate Indirect to Zero (Revert to Direct Only)**:
   - Frees all remaining indirect data blocks ($\le 8$ in last chunk) plus the old indirect block ($B_{\text{old}}$).
   - Inode `new_inode_image.indirect_block` set to 0.
   - Total allocations: 0. Total frees: $8 + 1 = 9 \le 10$.
7. **Case G: Delete File**:
   - Frees remaining direct/indirect data blocks (chunked if $> 8$) and indirect block.
   - Total allocations: 0. Total frees: $\le 9 \le 10$.

**Conclusion**: In all possible operations, $\text{Required Allocations} \le 9 \le 10$ and $\text{Required Frees} \le 9 \le 10$. Bounded journal capacity is mathematically proven.

---

## 11. Six-Step Commit Protocol & Atomicity Guarantees

### 11.1 Commit Sequence
```text
Step 1: Write Journal INTENT Record
        - Populates DiskJournalBlock with state = Intent (1), op_type, old_inode_image,
          new_inode_image, allocated_blocks, freed_blocks, dir_entry_copy, and CRC32.
        - Executes ATA CACHE FLUSH.

Step 2: Write Data & CoW Indirect Blocks to Disk
        - Writes user payload into freshly allocated data blocks.
        - If indirect entries modified, writes new CoW indirect block B_new to disk.
        - Executes ATA CACHE FLUSH.
        - INVARIANT: On-disk Inode Table still contains old_inode_image pointing to B_old!
          Uncommitted data or new indirect blocks are NEVER reachable from the filesystem root.

Step 3: Write Journal COMMITTED Record (THE COMMIT POINT)
        - Rewrites DiskJournalBlock with state = Committed (2) and updated CRC32.
        - Executes ATA CACHE FLUSH.
        - AT THIS EXACT MOMENT, THE TRANSACTION IS COMMITTED AND DURABLE.

Step 4: Write Metadata Blocks to Disk
        - Writes new_inode_image into Inode Table on disk (pointing to B_new).
        - Writes dir_entry_copy into Directory Block on disk (if Create/Delete).
        - Reconciles Block Bitmap (marks allocated_blocks used, freed_blocks free).
        - Reconciles Inode Bitmap (if Create/Delete).
        - Executes ATA CACHE FLUSH.

Step 5: Write Superblock
        - Updates free_blocks and free_inodes counters.
        - Executes ATA CACHE FLUSH.

Step 6: Clear Journal Record
        - Rewrites DiskJournalBlock with state = Free (0) and CRC32.
        - Executes ATA CACHE FLUSH.
```

### 11.2 Transaction Atomicity vs Syscall Atomicity
- **Writes $\le 32\text{ KiB}$**: Executed as a **single atomic transaction**. If a crash occurs, the operation is either completely rolled back (pre-TX state) or committed (post-TX state).
- **Writes $> 32\text{ KiB}$**: Executed as a sequence of discrete, atomic 32 KiB transactions:
  $$\text{TX}_1 \rightarrow \text{TX}_2 \rightarrow \dots \rightarrow \text{TX}_k$$
  Each transaction $\text{TX}_i$ commits independently. If a crash occurs during $\text{TX}_m$:
  - Transactions $\text{TX}_1 \dots \text{TX}_{m-1}$ are committed and **fully durable**.
  - $\text{TX}_m$ was uncommitted and **rolls back** to $\text{TX}_{m-1}$'s committed state.
  - The resulting file size and contents reflect the end of $\text{TX}_{m-1}$.
- **Precise Crash Invariant**:
  > **`I-STOR-JOURNAL-1`**: Each committed transaction is durable. An uncommitted transaction is recovered to its defined pre-transaction state. A multi-transaction write may be partially completed up to the last committed transaction boundary.

---

## 12. Exhaustive Crash Recovery Matrix

| Crash Point | Journal State | On-Disk State | Recovery Action on Mount | Recovered State Guarantee |
| :--- | :--- | :--- | :--- | :--- |
| **Before Step 1** | `Free` (0) | Pre-TX | None. Clean mount. | Pre-TX state |
| **During Step 1** (Torn Intent) | Torn / Bad CRC | Pre-TX | CRC32 mismatch. Clear journal to `Free`. | Pre-TX state |
| **After Step 1, Before Step 3** (During Data/CoW write) | `Intent` (1) | Data/CoW partially written; Inode points to $B_{\text{old}}$ | **Physical Rollback**:<br>1. Blit `old_inode_image` into Inode Table.<br>2. Deallocate `allocated_blocks` in Block Bitmap.<br>3. Ensure `freed_blocks` remain allocated.<br>4. If Create: clear Inode Bitmap bit.<br>5. Clear journal to `Free`. | **Strict Pre-TX State**: $B_{\text{old}}$ was untouched and remains active. Uncommitted $B_{\text{new}}$ and data blocks freed. Zero block leaks. |
| **During Step 3** (Torn Commit) | Torn / Bad CRC | Data/CoW fully written; Inode points to $B_{\text{old}}$ | If CRC32 invalid: **Rollback** as Intent.<br>If CRC32 valid: **Rollforward** as Committed. | Consistent Pre-TX or Post-TX state |
| **After Step 3, Before Step 4** (Committed, before metadata write) | `Committed` (2) | Data/CoW fully written; Inode still points to $B_{\text{old}}$ | **Physical Rollforward**:<br>1. Blit `new_inode_image` into Inode Table (points to $B_{\text{new}}$).<br>2. Set `allocated_blocks` in Block Bitmap ($B_{\text{new}}$ and data marked used).<br>3. Clear `freed_blocks` in Block Bitmap ($B_{\text{old}}$ marked free).<br>4. If Create/Delete: blit `dir_entry_copy` and update Inode Bitmap.<br>5. Update Superblock counters.<br>6. Clear journal to `Free`. | **Strict Post-TX State**: $B_{\text{new}}$ adopted atomically; $B_{\text{old}}$ safely freed; metadata fully consistent. |
| **During Step 4** (Torn Inode/Bitmap write) | `Committed` (2) | Partially updated metadata | **Physical Rollforward**: Idempotent re-execution of all Step 4 operations. | Strict Post-TX State |
| **During Step 6** (Torn Clear) | Torn / Bad CRC | Fully updated metadata | Re-running rollforward is idempotent. Clears journal to `Free`. | Post-TX state |
| **Corrupt Magic / Checksum** | Invalid | Any | Fail-Closed: Mount aborted with `Err(FsError::CorruptJournal)`. | Fail-Closed |

---

## 13. StorageObject Structure & File Offset Semantics

### 13.1 `StorageObject` Structure (48 bytes, 8-byte aligned)
`StorageObject` represents an **Open File Description** in kernel memory:

```rust
pub const MAX_OPEN_STORAGE_OBJECTS: usize = 32;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct StorageObject {
    pub occupied: bool,           // 0x00..0x01: Slot in use
    pub _pad0: [u8; 3],           // 0x01..0x04: Padding
    pub inode_num: u32,           // 0x04..0x08: Inode index (1..=255)
    pub generation: u32,          // 0x08..0x0C: Inode generation counter
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

### 13.2 Handle Duplication & Seek Offset Semantics
- **`CAP_DUPLICATE` / `TRANSFER_DELEGATE`**: Duplicating a capability creates a new handle pointing to the **same `KernelObjectSlot` and same `StorageObject`**. Processes A and B **share the same `cursor_offset`**!
- **Independent Open**: Calling `SYS_FILE_OPEN` allocates a **new `StorageObject`** with an independent `cursor_offset = 0`.
- **`TRANSFER_MOVE`**: Unmaps sender handle; receiver adopts the existing `StorageObject` with its current `cursor_offset`.
- **File Deletion vs Open Handles**: Unlinking removes directory entry and sets `PENDING_DELETE`. Open handles continue reading/writing. When `handle_refs == 0 && in_flight_ops == 0`, data blocks and inode are reclaimed.
- **Process Exit**: Closes open handles (`handle_refs.saturating_sub(1)`). If `links_count > 0`, persistent on-disk data remains intact. Persistent files are **never** deleted due to process exit.

---

## 14. Capability Authority & Namespace Traversal

### 14.1 Directory Capability Requirement
`SYS_FILE_OPEN` mandates an explicit Directory Capability:

```rust
sys_file_open(dir_handle, path_ptr, path_len, flags, out_handle_ptr)
```

- Ambient path authority is eliminated.
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
- `RSI`: `buf_ptr: u64`
- `RDX`: `count: u64`
- `R10`: `out_read_ptr: u64` (Pointer to write `u64` actual bytes read)
- Required Right: `FILE_READ`.
- Validation: `validate_user_range(buf_ptr, count, Write)`, `validate_user_range(out_read_ptr, 8, Write)`.
- EOF / Zero-Length: If `count == 0`, writes 0 to `out_read_ptr` and returns 0. If `cursor_offset >= file_size`, writes 0 and returns 0 (EOF).
- Returns: `0` on success, negative on error.

### 15.3 `SYS_FILE_WRITE` (13)
- `RAX`: `13`
- `RDI`: `handle: u32`
- `RSI`: `buf_ptr: u64`
- `RDX`: `count: u64`
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

## 16. Global Lock Hierarchy & Acquisition Traces

### 16.1 Global Monotonic Lock Hierarchy
```text
FILESYSTEM_LOCK (Rank 1)
    ≺ STORAGE_OBJECT_TABLE_LOCK (Rank 2)
        ≺ BLOCK_CACHE_LOCK (Rank 3)
            ≺ BLOCK_DEVICE_LOCK (Rank 4)
                ≺ KERNEL_OBJECT_TABLE_LOCK (Rank 5, Frozen 3G)
                    ≺ SCHEDULER.lock (Rank 6, Frozen 3A–3E)
                        ≺ CPU (IF=0) (Rank 7)
```

### 16.2 Invariants
1. `I-STOR-LOCK-1`: Locks must be acquired strictly in ascending rank order.
2. `I-STOR-LOCK-2`: Polling ATA port I/O runs strictly under `BLOCK_DEVICE_LOCK` with `IF=0`.
3. `I-STOR-LOCK-3`: No thread may block, yield, or sleep while holding Ranks 1..6.

### 16.3 Exhaustive Lock Acquisition Traces (13 Operations)

| Operation | Lock Acquisition Trace | Release Order | Sleeping Possible? | Scheduler Lock? |
| :--- | :--- | :--- | :--- | :--- |
| **`file_open`** | 1. Acquire `FILESYSTEM_LOCK` (Rank 1) $\rightarrow$ acquire/release `BLOCK_CACHE_LOCK` (Rank 3) per directory block read. Lookup/create inode. Release `FILESYSTEM_LOCK`.<br>2. Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2) $\rightarrow$ allocate `StorageObject`. Release `STORAGE_OBJECT_TABLE_LOCK`.<br>3. Acquire `KERNEL_OBJECT_TABLE_LOCK` (Rank 5) $\rightarrow$ register `KernelObjectSlot`. Release `KERNEL_OBJECT_TABLE_LOCK`.<br>4. Acquire `SCHEDULER.lock` (Rank 6) $\rightarrow$ allocate handle in `HandleTable`. Release `SCHEDULER.lock`. | Phased; zero overlapping inversion | No | Yes (Phase 4 only) |
| **`file_read`** | 1. Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): read `cursor_offset`, increment `in_flight_ops += 1`. Release `STORAGE_OBJECT_TABLE_LOCK`.<br>2. Acquire `FILESYSTEM_LOCK` (Rank 1): translate file offset to block LBAs via inode block pointers.<br>3. Acquire `BLOCK_CACHE_LOCK` (Rank 3): read data blocks into buffers. Release `BLOCK_CACHE_LOCK`. Release `FILESYSTEM_LOCK`.<br>4. Copy data to user buffer.<br>5. Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): advance `cursor_offset += bytes_read`, decrement `in_flight_ops -= 1`. Release `STORAGE_OBJECT_TABLE_LOCK`. | Ascending in each phase | No | No |
| **`file_write`** | 1. Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): read `cursor_offset`, increment `in_flight_ops += 1`. Release `STORAGE_OBJECT_TABLE_LOCK`.<br>2. Acquire `FILESYSTEM_LOCK` (Rank 1): execute 6-step journal commit protocol (acquires `BLOCK_CACHE_LOCK` Rank 3 and `BLOCK_DEVICE_LOCK` Rank 4 during flushes). Release `FILESYSTEM_LOCK`.<br>3. Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): advance `cursor_offset += bytes_written`, update `cached_size`, decrement `in_flight_ops -= 1`. Release `STORAGE_OBJECT_TABLE_LOCK`. | Ascending in each phase | No | No |
| **`file_close`** | 1. Acquire `SCHEDULER.lock` (Rank 6): clear handle in `HandleTable`. Release `SCHEDULER.lock`.<br>2. Acquire `KERNEL_OBJECT_TABLE_LOCK` (Rank 5): decrement `handle_refs`. Release `KERNEL_OBJECT_TABLE_LOCK`.<br>3. Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): if `handle_refs == 0 && in_flight_ops == 0`, mark slot free. Check if `PENDING_DELETE`. Release `STORAGE_OBJECT_TABLE_LOCK`.<br>4. If `PENDING_DELETE`: Acquire `FILESYSTEM_LOCK` (Rank 1) $\rightarrow$ reclaim blocks in bitmap $\rightarrow$ free inode. Release `FILESYSTEM_LOCK`. | Phased | No | Yes (Phase 1 only) |
| **`file_sync`** | 1. Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): read `inode_num`. Release `STORAGE_OBJECT_TABLE_LOCK`.<br>2. Acquire `FILESYSTEM_LOCK` (Rank 1) $\rightarrow$ acquire `BLOCK_CACHE_LOCK` (Rank 3) $\rightarrow$ flush dirty blocks for inode to device via `BLOCK_DEVICE_LOCK` (Rank 4). Release in reverse order (4 $\rightarrow$ 3 $\rightarrow$ 1). | 1 $\rightarrow$ 3 $\rightarrow$ 4 | No | No |
| **`delete`** | 1. Acquire `FILESYSTEM_LOCK` (Rank 1): unlink directory entry via journal. Decrement on-disk `links_count`. If `links_count == 0`, check `STORAGE_OBJECT_TABLE` for open handles. If open handles exist, set `PENDING_DELETE`; if no open handles, reclaim blocks immediately. Release `FILESYSTEM_LOCK`. | 1 $\rightarrow$ 3 $\rightarrow$ 4 | No | No |
| **`process_exit`**| 1. Iterates process handles under `SCHEDULER.lock` (Rank 6).<br>2. For each storage handle, invokes `file_close` sequence (Traces 1..3). Persistent on-disk data remains intact. | Ascending per phase | No | Yes |
| **`storage-object creation`** | Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): allocate slot. Release `STORAGE_OBJECT_TABLE_LOCK`. | Single lock | No | No |
| **`storage-object destruction`**| Acquire `STORAGE_OBJECT_TABLE_LOCK` (Rank 2): clear slot (`occupied = false`). Release `STORAGE_OBJECT_TABLE_LOCK`. | Single lock | No | No |
| **`filesystem mount`** | 1. Acquire `FILESYSTEM_LOCK` (Rank 1) $\rightarrow$ acquire `BLOCK_CACHE_LOCK` (Rank 3) $\rightarrow$ read superblock and journal block via `BLOCK_DEVICE_LOCK` (Rank 4).<br>2. Validate CRC32 and execute crash recovery if dirty. Release in reverse order. | 1 $\rightarrow$ 3 $\rightarrow$ 4 | No | No |
| **`filesystem recovery`**| Runs strictly under `FILESYSTEM_LOCK` (Rank 1) $\rightarrow$ `BLOCK_CACHE_LOCK` (Rank 3) $\rightarrow$ `BLOCK_DEVICE_LOCK` (Rank 4). Rollback or rollforward applied. | 1 $\rightarrow$ 3 $\rightarrow$ 4 | No | No |
| **`buffer eviction`** | Runs under `BLOCK_CACHE_LOCK` (Rank 3). If dirty buffer selected, acquires `BLOCK_DEVICE_LOCK` (Rank 4) $\rightarrow$ writes block $\rightarrow$ flushes device. Release 4 then 3. | 3 $\rightarrow$ 4 | No | No |
| **`block allocation`** | Runs under `FILESYSTEM_LOCK` (Rank 1): scans in-memory Block Bitmap. Marks bit allocated. | Single lock | No | No |

---

## 17. ATA Bounded Polling & Device Failure Semantics

```rust
pub const MAX_ATA_POLL_ITERATIONS: u32 = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Offline = 0,
    Probing = 1,
    Ready = 2,
    Faulted = 3,
}
```

### Bounded Polling Contracts:
1. **BSY Timeout**: If `(inb(0x1F7) & 0x80) != 0` for `100,000` iterations, return `Err(DeviceError::Timeout)`.
2. **DRQ Timeout**: If `(inb(0x1F7) & 0x08) == 0` for `100,000` iterations, return `Err(DeviceError::Timeout)`.
3. **ERR-bit Handling**: If `(inb(0x1F7) & 0x01) != 0`, read Error Register `inb(0x1F1)`, return `Err(DeviceError::HardwareError(err_code))`.
4. **IDENTIFY Probing**: Floating bus (`0xFF`) or status `0x00` $\rightarrow$ `Err(DeviceError::NoDevice)`.
5. **Fault Transition**: Upon timeout or hardware error, device transitions to `DeviceState::Faulted`. Mounted volume transitions to `volume_state = 3 (Faulted)`. In-flight writes fail with `-EIO`. Dirty buffers for device discarded. Zero kernel memory corruption.

---

## 18. MemBlockDevice vs Persistent Disk

- **`MemBlockDevice`** (Transient Test Device): 16 MiB in static BSS. Verifies `I-STOR-LIFETIME-1` (Process-Exit Persistence: process exits, handles closed, file remains intact on volume).
- **`AtaPioBlockDevice`** (Real Persistent Storage): Communicates via port I/O with QEMU's primary IDE drive (`-drive file=disk.img,format=raw,if=ide`). Verifies `I-STOR-LIFETIME-2` (Reboot Persistence: data survives machine power-down and restart).

---

## 19. Reclamation & Lifecycle Ownership

| Resource | Sole Owner of Reclamation | Reclamation Event |
| :--- | :--- | :--- |
| `BlockBuffer` | Buffer Cache Manager | When evicting unpinned clean/flushed buffer |
| `DiskInode` | Filesystem Allocator | When `links_count == 0` AND all open handles closed |
| `StorageObject` | Object Manager | When `handle_refs == 0 && in_flight_ops == 0` |
| Handle Descriptor | Process Handle Table | Upon `SYS_FILE_CLOSE` or `process_exit()` |
| Data Blocks | Block Bitmap Allocator | When file truncated or unlinked inode reclaimed |
| Journal Record | Journal Engine | Cleared upon transaction Step 6 completion |

---

## 20. Security & Untrusted Disk Validation

- `I-STOR-SEC-1`: Every block number, inode index, and offset read from disk is verified against volume bounds before memory indexing or port I/O.
- `I-STOR-SEC-2`: Superblock and Inode CRC32 checksums verified upon read; mismatch immediately fails closed (`Err(FsError::CorruptMetadata)`).
- `I-STOR-SEC-3`: Filenames restricted to 1..=55 bytes, no null bytes in body, no `..` traversal.
- `I-STOR-SEC-4`: All disk arithmetic uses checked operations before addition, multiplication, or alignment.

---

## 21. Zero-Heap Bounded Resources

| Resource | Maximum Bound | Structure Size | Total Static Footprint |
| :--- | :--- | :--- | :--- |
| `MAX_BLOCK_DEVICES` | 2 | ~128 B | 256 B |
| `MAX_OPEN_STORAGE_OBJECTS` | 32 | 48 B | 1536 B |
| `MAX_BUFFERS` | 16 | 4128 B | 66,048 B |
| `MAX_INODES` | 256 | 256 B | Inode Table on disk; 32 B RAM bitmap |
| `MAX_TRANSACTION_PAYLOAD`| 8 blocks | 4096 B | 32 KiB chunk buffer |

Zero dynamic kernel heap allocations in kernel runtime.

---

## 22. Machine-Level Verification Suite (20 Tests: 3K-A through 3K-T)

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
| **3K-J** | Indirect Block Mapping | Write/read single-indirect block via CoW | `I-STOR-INODE-2` |
| **3K-K** | Tail Zero Padding | Verify unwritten block tail is zeroed | `I-STOR-INODE-3` |
| **3K-L** | Directory Insertion | Insert entries; reject duplicates | `I-STOR-DIR-1` |
| **3K-M** | File Truncation | Truncate file; verify block reclamation | `I-STOR-RECLAIM-1` |
| **3K-N** | Process-Exit Persistence | Process exits; verify on-disk file remains readable | `I-STOR-LIFETIME-1` |
| **3K-O** | Reboot Persistence | Verify on-disk file survives machine restart | `I-STOR-LIFETIME-2` |
| **3K-P** | Corruption Rejection | Corrupt superblock CRC32; verify mount fails closed | `I-STOR-SEC-1` |
| **3K-Q** | Uncommitted Crash Recovery | Crash during Intent; rollback data & CoW indirect block | `I-STOR-JOURNAL-1` |
| **3K-R** | Committed Crash Recovery | Crash after Commit; rollforward Inode & CoW indirect block | `I-STOR-JOURNAL-2` |
| **3K-S** | Post-Metadata-Write Crash | Crash after partial Inode/Bitmap flush; verify idempotent recovery | `I-STOR-JOURNAL-3` |
| **3K-T** | Capability Enforcement & PMM Neutrality | Verify `FILE_READ` vs `FILE_WRITE` and `baseline_free == post_test_free` | `I-STOR-CAP-1`, `I-STOR-PMM-1` |

### Machine Test Arithmetic:
- Baseline Stages 1–3H: 181
- Stage 3I: 18
- Stage 3J: 16
- Stage 3K: 20
- **Total In-Kernel Machine Tests**: **235 tests**.

### Host Pytest Suite:
- Baseline: 27 files, 104 tests.
- Stage 3K: 1 file (`tests/test_stage3k.py`), 6 tests.
- **Total Host Pytests**: 28 files, 110 tests.

---

## 23. PMM Neutrality Contract

> **`I-STOR-PMM-1`**: Transient kernel allocations during filesystem operations must satisfy `baseline_free == post_test_free`.
- Distinguishes RAM/PMM frame neutrality from persistent on-disk block consumption.
- On load failure or transaction rollback, all transient frames return to baseline with zero memory leak.

---

## 24. Architecture Decision Records (ADRs)

- **ADR-3K-001**: ZeroFS On-Disk Geometry & Exact Formulas
- **ADR-3K-002**: Write-Ahead Intent Journal & Crash Recovery
- **ADR-3K-003**: StorageObject Lifecycle & Unlinked File Deletion
- **ADR-3K-004**: Namespace Capability Authority (Elimination of Ambient Authority)
- **ADR-3K-005**: Bounded Buffer Cache Lifecycle & Eviction
- **ADR-3K-006**: ATA PIO Driver & Device Failure Semantics
- **ADR-3K-007**: Zero-Heap Bounded Resource Policy
- **ADR-3K-008**: File Offset Semantics on Capability Duplication
- **ADR-3K-009**: Copy-on-Write Indirect Blocks & Deterministic Metadata Recovery

---

## 25. Adversarial Review Classifications

| Threat / Attack Surface | Classification | Architectural Defense |
| :--- | :--- | :--- |
| **Torn In-Place Indirect Block** | 🟢 Resolved | CoW indirect blocks (`ADR-3K-009`): old block is never mutated; new block flushed in Step 2; swapped at commit. |
| **Bitmap Inconsistency on Crash** | 🟢 Resolved | Bitmaps reconciled deterministically from `allocated_blocks` and `freed_blocks` vectors. |
| **Corrupt Journal Checksum** | 🟢 Resolved | Checksum mismatch rejects journal entry fail-closed; volume marked `Faulted`. |
| **Torn Data Block Write** | 🟢 Resolved | Unwritten data is never referenced by Inode; commit point occurs only after data flush. |
| **Path Traversal Escape** | 🟢 Resolved | `SYS_FILE_OPEN` requires Directory Capability; relative paths only; `..` strictly rejected. |
| **Capability Escalation** | 🟢 Resolved | Monotonic attenuation enforced; read-only directory capability cannot yield write handle. |
| **Deadlock via Lock Inversion**| 🟢 Resolved | Monotonic 7-rank lock hierarchy enforced with explicit non-overlapping lock acquisition traces for all 13 kernel operations; no spinlock held across blocking switch. |
| **Double Free of Unlinked Inode**| 🟢 Resolved | Inode marked `PENDING_DELETE`; reclaimed only when `handle_refs == 0 && in_flight_ops == 0`. |
| **Bitmap Buffer Pool Exhaustion**| 🟢 Resolved | When all 16 buffers pinned, returns `-EBUSY` deterministically without deadlock. |
| **ATA Hardware Lockup** | 🟢 Resolved | Bounded polling (`MAX_ATA_POLL_ITERATIONS = 100_000`) returns `-EIO` on timeout. |

---

## 26. Freeze Criteria & Final Verdict

All freeze criteria, exact byte tables, transaction capacity limits, CoW metadata specifications, recovery proofs, lock traces, and verification matrices are completely defined with zero open RED blockers.

```text
🟢 STAGE 3K ARCHITECTURE APPROVED / READY TO FREEZE (Rev5)
```
