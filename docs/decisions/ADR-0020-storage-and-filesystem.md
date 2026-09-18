# ADR-0020: ZeroFS Persistent Storage & Filesystem Architecture (Rev3)

## Context
Project Zero / ZeroOS requires a persistent storage and filesystem primitive to support persistent workspace state, configuration, and future multi-agent workloads. Stage 3A through Stage 3J established kernel threads, preemptive scheduling, IPC, capabilities, user processes, fast syscalls, and native ELF execution.

ZeroOS is not a Unix clone. It rejects Unix-style ambient path authority, UID/GID permissions, and monolithic hierarchies in favor of a typed, capability-authorized persistent resource graph.

## Architectural Decisions

### ADR-3K-001: ZeroFS On-Disk Geometry & Exact Formulas
- Fixed 4096-byte blocks (`BLOCK_SIZE = 4096`, `SECTOR_SIZE = 512`, 8 sectors/block).
- Deterministic block map:
  - Block 0: Reserved / Boot Sector (4 KiB)
  - Block 1: Superblock (`DiskSuperblock`, 128 bytes, magic `"ZERO_FS\0"`, CRC32)
  - Block 2: Transaction Journal (`DiskJournalBlock`, 4096 bytes)
  - Block 3: Inode Allocation Bitmap (4 KiB, tracks 256 inodes)
  - Block 4: Block Allocation Bitmap (4 KiB, tracks 32,768 blocks = 128 MiB max volume)
  - Blocks 5..20: Inode Table (16 blocks, 256 inodes, 256 bytes per `DiskInode`)
  - Block 21: Root Directory data block (64-byte `DiskDirEntry`)
  - Blocks 22..N-1: Data Blocks (`DATA_BLOCK_START = 22`)
- Inode mapping: 18 direct pointers (72 KiB direct capacity) + 1 single-indirect pointer (1024 pointers × 4096 B = 4 MiB capacity). Double-indirect pointers deferred to Stage 3L volume expansion. Physical volume data capacity strictly dominates maximum file size.
- Architectural Volume Capacity vs. Test Device:
  - Architectural ZeroFS volume capacity bound: 32,768 blocks = 128 MiB max volume (tracked by 4 KiB Block Allocation Bitmap at Block 4; ATA supports up to 28-bit LBA).
  - Machine Verification Test Device: In-memory `MemBlockDevice` (Device 0) is configured with 256 blocks (1 MiB total, 234 data blocks) to execute all 20 verification tests while strictly fitting within the Stage 2F 2 MiB kernel bootstrap mapping window.

### ADR-3K-002: Write-Ahead Intent Journal & Crash Recovery
- Dedicated 1-block transaction log (`Block 2`, `DiskJournalBlock`).
- 6-step ordering: Intent -> Data Flush -> Commit Point -> Metadata Flush -> Superblock Flush -> Clear.
- Crash recovery on mount:
  - `Intent`: Rollback uncommitted allocated blocks.
  - `Committed`: Rollforward metadata and directory entry.
  - Zero orphan blocks, zero data corruption (`I-STOR-JOURNAL-1`).

### ADR-3K-003: StorageObject Lifecycle & Unlinked File Deletion
- `KernelObjectType::StorageObject = 5` represents an Open File Description in kernel space.
- File deletion unlinks directory entry and decrements `links_count`.
- If `links_count == 0` while open, inode is marked `PENDING_DELETE`. Blocks and inode are reclaimed only after the last handle closes.
- Process exit closes open handles; persistent on-disk data is never deleted due to process exit.

### ADR-3K-004: Namespace Capability Authority (No Ambient Authority)
- `SYS_FILE_OPEN` requires an explicit Directory Capability (`dir_handle`). Ambient path authority is eliminated.
- Relative path traversal strictly within the directory capability subtree.
- Rights monotonically attenuated: `FILE_READ (1 << 5)`, `FILE_WRITE (1 << 6)`, `FILE_SYNC (1 << 7)`.

### ADR-3K-005: Bounded Buffer Cache Lifecycle & Eviction
- Static 16-buffer pool (64 KiB BSS footprint).
- States: `Free`, `Clean`, `Dirty`, `Pinned`, `Writeback`.
- Deterministic pin counts; LRU sequence counter with linear rescaling pass to prevent overflow.

### ADR-3K-006: ATA PIO Driver & Device Failure Semantics
- Polled Primary ATA PIO over ports `0x1F0..0x1F7`, `0x3F6` with 28-bit LBA.
- Hardware errors (`ERR`) or timeouts (`100,000` cycles) transition device to `Faulted` and volume to `ReadOnly` fail-closed.

### ADR-3K-007: Zero-Heap Bounded Resource Policy
- All tables bounded in static BSS (`MAX_BLOCK_DEVICES = 2`, `MAX_OPEN_STORAGE_OBJECTS = 32`, `MAX_BUFFERS = 16`, `MAX_INODES = 256`).
- Zero kernel dynamic heap allocations.

### ADR-3K-008: File Offset Semantics on Capability Duplication
- `CAP_DUPLICATE` / `TRANSFER_DELEGATE`: Duplicating a capability handle creates a new handle pointing to the same `StorageObject`; processes share the same seek offset.
- Independent `SYS_FILE_OPEN`: Allocates a new `StorageObject` with independent offset.
- `TRANSFER_MOVE`: Unmaps sender handle; receiver adopts existing `StorageObject` and cursor.

### ADR-3K-009: Copy-on-Write Indirect Blocks & Deterministic Metadata Recovery
- On-disk single-indirect blocks are NEVER modified in place.
- Any modification, extension, or truncation of indirect block pointers allocates a new block $B_{\text{new}}$ and retains the old block $B_{\text{old}}$ until transaction commit.
- $B_{\text{new}}$ is flushed during Step 2 (Data & CoW Flush).
- Step 3 (Commit Point) makes the transaction durable.
- Rollback of an uncommitted transaction leaves $B_{\text{old}}$ 100% intact on disk and reclaims $B_{\text{new}}$ in the Block Bitmap via the journal's `allocated_blocks` vector.
- Rollforward of a committed transaction updates the inode to point to $B_{\text{new}}$ and frees $B_{\text{old}}$ in the Block Bitmap via the journal's `freed_blocks` vector.
- Both operations are strictly idempotent and guarantee zero metadata corruption.

## Status
🟢 APPROVED / READY TO FREEZE (Rev5)

## Consequences
- Preserves all frozen contracts and ABIs from Stages 3A–3J.
- Establishes an atomically crash-consistent, capability-native filesystem with zero dynamic kernel heap allocations.

