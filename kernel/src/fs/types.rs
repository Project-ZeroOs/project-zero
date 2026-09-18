//! Project Zero - Stage 3K Filesystem Types & On-Disk Structures
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).

use core::mem::{align_of, size_of};

pub const SECTOR_SIZE: usize = 512;
pub const BLOCK_SIZE: usize = 4096;
pub const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE; // 8

pub const MAX_VOLUME_BLOCKS: u64 = 32_768; // 128 MiB volume
pub const MAX_INODES: usize = 256;
pub const INODES_PER_BLOCK: usize = BLOCK_SIZE / 256; // 16
pub const INODE_TABLE_BLOCKS: u64 = (MAX_INODES / INODES_PER_BLOCK) as u64; // 16

pub const SUPERBLOCK_BLOCK: u64 = 1;
pub const JOURNAL_BLOCK: u64 = 2;
pub const INODE_BITMAP_BLOCK: u64 = 3;
pub const BLOCK_BITMAP_BLOCK: u64 = 4;
pub const INODE_TABLE_START: u64 = 5;
pub const ROOT_DIR_BLOCK: u64 = 21;
pub const DATA_BLOCK_START: u64 = 22;

pub const ROOT_DIR_INODE: u32 = 1;
pub const NULL_INODE: u32 = 0;

pub const SUPERBLOCK_MAGIC: [u8; 8] = *b"ZERO_FS\0";
pub const JOURNAL_MAGIC: u64 = 0x5A45_524F_5F4A_524E; // "ZERO_JRN" in little-endian

pub const MAX_DATA_BLOCKS_PER_TX: usize = 8;        // 32 KiB user payload
pub const MAX_METADATA_BLOCKS_PER_TX: usize = 1;    // 1 single-indirect CoW block
pub const MAX_TOTAL_ALLOCATIONS_PER_TX: usize = 10;
pub const MAX_TOTAL_FREES_PER_TX: usize = 10;
pub const MAX_TRANSACTION_BYTES: usize = 32_768;    // 32 KiB

pub const MAX_FILENAME_LEN: usize = 55;
pub const DIR_ENTRIES_PER_BLOCK: usize = BLOCK_SIZE / 64; // 64

// Flags for DiskInode
pub const INODE_FLAG_IMMUTABLE: u16 = 1 << 0;
pub const INODE_FLAG_SYNC: u16      = 1 << 1;
pub const INODE_FLAG_PENDING_DELETE: u16 = 1 << 2;

// Open flags for sys_file_open
pub const O_READ: u32   = 1 << 0;
pub const O_WRITE: u32  = 1 << 1;
pub const O_CREATE: u32 = 1 << 2;
pub const O_TRUNC: u32  = 1 << 3;
pub const O_APPEND: u32 = 1 << 4;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeType {
    Free = 0,
    Regular = 1,
    Directory = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalOpType {
    None = 0,
    Create = 1,
    Write = 2,
    Truncate = 3,
    Delete = 4,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalState {
    Free = 0,
    Intent = 1,
    Committed = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    DeviceError,
    DeviceTimeout,
    InvalidSuperblock,
    CorruptJournal,
    CorruptMetadata,
    NoSpace,
    NoInode,
    NotFound,
    AlreadyExists,
    IsDirectory,
    NotDirectory,
    NotEmpty,
    InvalidArgument,
    PermissionDenied,
    TableFull,
    Busy,
}

/// Standard IEEE 802.3 CRC32 implementation.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// On-Disk Structures (Byte-exact, verified with static asserts)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskSuperblock {
    pub magic: [u8; 8],                  // 0x00..0x08: "ZERO_FS\0"
    pub version: u32,                    // 0x08..0x0C: 1
    pub block_size: u32,                 // 0x0C..0x10: 4096
    pub total_blocks: u64,               // 0x10..0x18: Total blocks in volume
    pub free_blocks: u64,                // 0x18..0x20: Free data blocks
    pub total_inodes: u32,               // 0x20..0x24: 256
    pub free_inodes: u32,                // 0x24..0x28: Free inodes
    pub journal_block: u64,              // 0x28..0x30: 2
    pub block_bitmap_block: u64,        // 0x30..0x38: 4
    pub inode_bitmap_block: u64,        // 0x38..0x40: 3
    pub inode_table_block: u64,         // 0x40..0x48: 5
    pub inode_table_blocks: u32,        // 0x48..0x4C: 16
    pub root_dir_inode: u32,            // 0x4C..0x50: 1
    pub volume_state: u32,              // 0x50..0x54: 0=Clean, 1=Dirty, 2=Recovering, 3=Faulted
    pub mount_count: u32,               // 0x54..0x58: Monotonic mount counter
    pub checksum: u32,                  // 0x58..0x5C: CRC32 over bytes 0x00..0x58
    pub _pad: u32,                      // 0x5C..0x60: Explicit 8-byte alignment padding
    pub volume_label: [u8; 32],         // 0x60..0x80: Null-padded UTF-8 label
}

const _: () = assert!(size_of::<DiskSuperblock>() == 128);
const _: () = assert!(align_of::<DiskSuperblock>() == 8);

impl DiskSuperblock {
    pub fn is_valid(&self) -> bool {
        if self.magic != SUPERBLOCK_MAGIC {
            return false;
        }
        if self.version != 1 || self.block_size != (BLOCK_SIZE as u32) {
            return false;
        }
        if self.total_blocks > MAX_VOLUME_BLOCKS || self.total_inodes != (MAX_INODES as u32) {
            return false;
        }
        let raw = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                0x58,
            )
        };
        crc32(raw) == self.checksum
    }

    pub fn recompute_checksum(&mut self) {
        let raw = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                0x58,
            )
        };
        self.checksum = crc32(raw);
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskInode {
    pub file_type: u16,                  // 0x00..0x02: InodeType (0=Free, 1=Regular, 2=Directory)
    pub flags: u16,                      // 0x02..0x04: INODE_FLAG_*
    pub links_count: u32,                // 0x04..0x08: Directory links referencing inode
    pub size_bytes: u64,                 // 0x08..0x10: Exact file size in bytes
    pub blocks_count: u64,               // 0x10..0x18: Total 4 KiB data blocks allocated
    pub creator_pid: u64,                // 0x18..0x20: Creator process ID
    pub ctime_ticks: u64,                // 0x20..0x28: Creation timestamp ticks
    pub mtime_ticks: u64,                // 0x28..0x30: Last modification ticks
    pub generation: u32,                 // 0x30..0x34: Recycling generation counter
    pub checksum: u32,                   // 0x34..0x38: CRC32 over bytes 0x00..0x34
    pub _reserved: [u8; 8],              // 0x38..0x40: Reserved padding
    pub direct_blocks: [u32; 18],        // 0x40..0x88: 18 direct block pointers (72 KiB)
    pub indirect_block: u32,             // 0x88..0x8C: Single-indirect block pointer
    pub _pad_indirect: u32,              // 0x8C..0x90: 4-byte padding to 8-byte align
    pub _reserved_expansion: [u8; 112],  // 0x90..0x100: Reserved for Stage 3L indirect expansion
}

const _: () = assert!(size_of::<DiskInode>() == 256);
const _: () = assert!(align_of::<DiskInode>() == 8);

impl DiskInode {
    pub const fn empty() -> Self {
        Self {
            file_type: 0,
            flags: 0,
            links_count: 0,
            size_bytes: 0,
            blocks_count: 0,
            creator_pid: 0,
            ctime_ticks: 0,
            mtime_ticks: 0,
            generation: 1,
            checksum: 0,
            _reserved: [0; 8],
            direct_blocks: [0; 18],
            indirect_block: 0,
            _pad_indirect: 0,
            _reserved_expansion: [0; 112],
        }
    }

    pub fn is_valid_checksum(&self) -> bool {
        let raw = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                0x34,
            )
        };
        crc32(raw) == self.checksum
    }

    pub fn recompute_checksum(&mut self) {
        let raw = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                0x34,
            )
        };
        self.checksum = crc32(raw);
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskDirEntry {
    pub inode_num: u32,                  // 0x00..0x04: Inode index (1..=255; 0=unused)
    pub name_len: u8,                    // 0x04..0x05: Filename length (1..=55)
    pub file_type: u8,                   // 0x05..0x06: 1=Regular, 2=Directory
    pub _pad: u16,                       // 0x06..0x08: Padding
    pub name: [u8; 55],                  // 0x08..0x3F: ASCII / UTF-8 filename
    pub _null_terminator: u8,            // 0x3F..0x40: Explicit trailing 0x00
}

const _: () = assert!(size_of::<DiskDirEntry>() == 64);
const _: () = assert!(align_of::<DiskDirEntry>() == 4);

impl DiskDirEntry {
    pub const fn empty() -> Self {
        Self {
            inode_num: 0,
            name_len: 0,
            file_type: 0,
            _pad: 0,
            name: [0; 55],
            _null_terminator: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DiskJournalBlock {
    pub magic: u64,                      // 0x0000..0x0008: ASCII "ZERO_JRN" (0x5A45524F5F4A524E)
    pub sequence: u64,                   // 0x0008..0x0010: Monotonic transaction sequence
    pub format_version: u32,             // 0x0010..0x0014: 1
    pub op_type: u32,                    // 0x0014..0x0018: JournalOpType (1=Create, 2=Write, 3=Truncate, 4=Delete)
    pub state: u32,                      // 0x0018..0x001C: JournalState (0=Free, 1=Intent, 2=Committed)
    pub target_inode_num: u32,           // 0x001C..0x0020: Target Inode index
    pub parent_dir_inode: u32,           // 0x0020..0x0024: Parent directory Inode (or 0)
    pub dir_entry_slot: u32,             // 0x0024..0x0028: Slot index in directory block
    pub allocated_blocks_count: u32,     // 0x0028..0x002C: Count of allocated blocks (0..=10)
    pub freed_blocks_count: u32,         // 0x002C..0x0030: Count of freed blocks (0..=10)
    pub allocated_blocks: [u32; 10],     // 0x0030..0x0058: Newly allocated block LBAs
    pub freed_blocks: [u32; 10],         // 0x0058..0x0080: Newly freed block LBAs
    pub dir_entry_copy: DiskDirEntry,    // 0x0080..0x00C0: 64-byte snapshot of mutated directory slot
    pub old_inode_image: DiskInode,      // 0x00C0..0x01C0: Exact pre-transaction DiskInode (256 B)
    pub new_inode_image: DiskInode,      // 0x01C0..0x02C0: Exact post-transaction DiskInode (256 B)
    pub checksum: u32,                   // 0x02C0..0x02C4: CRC32 over bytes 0x0000..0x02C0 (704 B)
    pub _reserved: [u8; 3388],           // 0x02C4..0x1000: Zero-padding to 4096 bytes
}

const _: () = assert!(size_of::<DiskJournalBlock>() == 4096);
const _: () = assert!(align_of::<DiskJournalBlock>() == 8);

impl DiskJournalBlock {
    pub const fn empty() -> Self {
        Self {
            magic: JOURNAL_MAGIC,
            sequence: 0,
            format_version: 1,
            op_type: 0,
            state: 0,
            target_inode_num: 0,
            parent_dir_inode: 0,
            dir_entry_slot: 0xFFFF_FFFF,
            allocated_blocks_count: 0,
            freed_blocks_count: 0,
            allocated_blocks: [0; 10],
            freed_blocks: [0; 10],
            dir_entry_copy: DiskDirEntry::empty(),
            old_inode_image: DiskInode::empty(),
            new_inode_image: DiskInode::empty(),
            checksum: 0,
            _reserved: [0; 3388],
        }
    }

    pub fn is_valid_checksum(&self) -> bool {
        let raw = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                0x02C0,
            )
        };
        crc32(raw) == self.checksum
    }

    pub fn recompute_checksum(&mut self) {
        let raw = unsafe {
            core::slice::from_raw_parts(
                self as *const Self as *const u8,
                0x02C0,
            )
        };
        self.checksum = crc32(raw);
    }
}

/// User space stat buffer returned by `SYS_FILE_STAT` (32 bytes, 8-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UserFileStat {
    pub size_bytes: u64,
    pub blocks_count: u64,
    pub file_type: u32,
    pub generation: u32,
    pub mtime_ticks: u64,
}

const _: () = assert!(size_of::<UserFileStat>() == 32);
const _: () = assert!(align_of::<UserFileStat>() == 8);
