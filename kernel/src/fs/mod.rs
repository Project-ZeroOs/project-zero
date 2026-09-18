//! Project Zero - Stage 3K Filesystem (ZeroFS)
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).

pub mod types;
pub mod dev;
pub mod buf;
pub mod journal;
pub mod inode;
pub mod dir;
pub mod file;
pub mod tests;

use crate::kprintln;
use core::sync::atomic::{AtomicBool, Ordering};
pub use types::*;
pub use dev::{BlockDevice, DeviceState, AtaPioBlockDevice, MemBlockDevice, get_device};
pub use buf::{init_cache, read_block_cached, write_block_cached, sync_all_buffers};
pub use journal::Journal;
pub use inode::InodeManager;
pub use dir::DirectoryManager;
pub use file::{StorageObject, FileManager, MAX_OPEN_STORAGE_OBJECTS};

pub struct FilesystemLock {
    lock: AtomicBool,
}

impl FilesystemLock {
    pub const fn new() -> Self {
        Self {
            lock: AtomicBool::new(false),
        }
    }

    #[inline(always)]
    pub fn acquire(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    pub fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

pub static FILESYSTEM_LOCK: FilesystemLock = FilesystemLock::new();

/// Global initialization of ZeroFS subsystem.
pub fn init(pmm: &mut crate::mm::pmm::PhysicalMemoryManager) {
    FILESYSTEM_LOCK.acquire();
    unsafe {
        dev::MEM_DEVICE.init_storage(pmm);
    }
    init_cache();
    FileManager::init();

    // Probe ATA PIO device
    unsafe {
        let _ = dev::ATA_DEVICE.probe();
    }
    FILESYSTEM_LOCK.release();
}

/// Format a block device with a clean ZeroFS persistent filesystem.
pub fn format_volume(device_id: u8, total_blocks: u64, label: &[u8]) -> Result<(), FsError> {
    if total_blocks > MAX_VOLUME_BLOCKS || total_blocks < 64 {
        return Err(FsError::InvalidArgument);
    }

    FILESYSTEM_LOCK.acquire();

    // 1. Zero out reserved boot block (Block 0)
    let zero = [0u8; BLOCK_SIZE];
    write_block_cached(device_id, 0, &zero)?;

    // 2. Format Superblock (Block 1)
    let mut sb = DiskSuperblock {
        magic: SUPERBLOCK_MAGIC,
        version: 1,
        block_size: BLOCK_SIZE as u32,
        total_blocks,
        free_blocks: total_blocks.saturating_sub(DATA_BLOCK_START),
        total_inodes: MAX_INODES as u32,
        free_inodes: (MAX_INODES - 2) as u32, // Inode 0 is NULL, Inode 1 is ROOT_DIR
        journal_block: JOURNAL_BLOCK,
        block_bitmap_block: BLOCK_BITMAP_BLOCK,
        inode_bitmap_block: INODE_BITMAP_BLOCK,
        inode_table_block: INODE_TABLE_START,
        inode_table_blocks: INODE_TABLE_BLOCKS as u32,
        root_dir_inode: ROOT_DIR_INODE,
        volume_state: 0, // Clean
        mount_count: 0,
        checksum: 0,
        _pad: 0,
        volume_label: [0; 32],
    };
    let label_len = label.len().min(31);
    sb.volume_label[0..label_len].copy_from_slice(&label[0..label_len]);
    sb.recompute_checksum();

    let mut sb_raw = [0u8; BLOCK_SIZE];
    unsafe {
        core::ptr::write(sb_raw.as_mut_ptr() as *mut DiskSuperblock, sb);
    }
    write_block_cached(device_id, SUPERBLOCK_BLOCK, &sb_raw)?;

    // 3. Format Journal (Block 2)
    let jrn = DiskJournalBlock::empty();
    let jrn_raw: [u8; BLOCK_SIZE] = unsafe { core::mem::transmute(jrn) };
    write_block_cached(device_id, JOURNAL_BLOCK, &jrn_raw)?;

    // 4. Format Inode Bitmap (Block 3): Mark Inode 0 and Inode 1 allocated
    let mut inode_bitmap = [0u8; BLOCK_SIZE];
    inode_bitmap[0] = 0x03; // Bits 0 and 1 set
    write_block_cached(device_id, INODE_BITMAP_BLOCK, &inode_bitmap)?;

    // 5. Format Block Bitmap (Block 4): Mark Blocks 0..21 allocated (Metadata overhead)
    let mut block_bitmap = [0u8; BLOCK_SIZE];
    // 22 blocks = 2 full bytes (16 bits) + 6 bits in third byte (0x3F)
    block_bitmap[0] = 0xFF;
    block_bitmap[1] = 0xFF;
    block_bitmap[2] = 0x3F; // Bits 16..21 set
    write_block_cached(device_id, BLOCK_BITMAP_BLOCK, &block_bitmap)?;

    // 6. Format Inode Table (Blocks 5..20): Clear all, initialize Inode 1 (Root Dir)
    for b in INODE_TABLE_START..(INODE_TABLE_START + INODE_TABLE_BLOCKS) {
        write_block_cached(device_id, b, &zero)?;
    }

    let mut root_inode = DiskInode::empty();
    root_inode.file_type = InodeType::Directory as u16;
    root_inode.links_count = 1;
    root_inode.blocks_count = 1;
    root_inode.size_bytes = BLOCK_SIZE as u64;
    root_inode.creator_pid = 1;
    root_inode.direct_blocks[0] = ROOT_DIR_BLOCK as u32;
    root_inode.recompute_checksum();
    InodeManager::write_inode(device_id, ROOT_DIR_INODE, root_inode)?;

    // 7. Format Root Directory block (Block 21)
    write_block_cached(device_id, ROOT_DIR_BLOCK, &zero)?;

    // Flush all metadata blocks to storage
    sync_all_buffers(device_id)?;

    FILESYSTEM_LOCK.release();
    Ok(())
}

/// Mount a ZeroFS volume: verifies superblock and performs crash recovery pass.
pub fn mount_volume(device_id: u8) -> Result<DiskSuperblock, FsError> {
    FILESYSTEM_LOCK.acquire();

    let mut sb_raw = [0u8; BLOCK_SIZE];
    read_block_cached(device_id, SUPERBLOCK_BLOCK, &mut sb_raw)?;

    let mut sb: DiskSuperblock = unsafe {
        core::ptr::read(sb_raw.as_ptr() as *const DiskSuperblock)
    };

    if !sb.is_valid() {
        FILESYSTEM_LOCK.release();
        return Err(FsError::InvalidSuperblock);
    }

    // Run Write-Ahead Journal crash recovery pass
    Journal::recover(device_id)?;

    sb.mount_count = sb.mount_count.saturating_add(1);
    sb.volume_state = 0; // Clean
    sb.recompute_checksum();

    unsafe {
        core::ptr::write(sb_raw.as_mut_ptr() as *mut DiskSuperblock, sb);
    }
    write_block_cached(device_id, SUPERBLOCK_BLOCK, &sb_raw)?;
    sync_all_buffers(device_id)?;

    FILESYSTEM_LOCK.release();
    Ok(sb)
}
