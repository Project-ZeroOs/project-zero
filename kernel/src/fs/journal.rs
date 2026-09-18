//! Project Zero - Stage 3K Write-Ahead Intent Journal
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).

use crate::fs::types::*;
use crate::fs::buf::{read_block_cached, write_block_cached, sync_block, sync_all_buffers};

pub struct Journal;

impl Journal {
    /// Read and parse journal block from disk.
    pub fn read_journal_block(device_id: u8) -> Result<DiskJournalBlock, FsError> {
        let mut raw = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, JOURNAL_BLOCK, &mut raw)?;
        let jrn: DiskJournalBlock = unsafe { core::ptr::read(raw.as_ptr() as *const DiskJournalBlock) };
        Ok(jrn)
    }

    /// Write journal block to disk, enforcing immediate flush.
    pub fn write_journal_block(device_id: u8, mut jrn: DiskJournalBlock) -> Result<(), FsError> {
        jrn.recompute_checksum();
        let raw: [u8; BLOCK_SIZE] = unsafe { core::mem::transmute(jrn) };
        write_block_cached(device_id, JOURNAL_BLOCK, &raw)?;
        sync_block(device_id, JOURNAL_BLOCK)?;
        Ok(())
    }

    /// Clear journal record on disk.
    pub fn clear(device_id: u8, seq: u64) -> Result<(), FsError> {
        let mut empty = DiskJournalBlock::empty();
        empty.sequence = seq;
        Self::write_journal_block(device_id, empty)
    }

    /// Crash recovery pass executed during filesystem mount.
    /// Strictly enforces Invariant I-STOR-JOURNAL-1.
    pub fn recover(device_id: u8) -> Result<(), FsError> {
        let mut jrn = Self::read_journal_block(device_id)?;

        // If journal is empty, nothing to recover
        if jrn.state == (JournalState::Free as u32) {
            return Ok(());
        }

        // Validate magic and checksum
        if jrn.magic != JOURNAL_MAGIC || !jrn.is_valid_checksum() {
            // Torn write or corrupt journal - fail closed
            return Err(FsError::CorruptJournal);
        }

        match jrn.state {
            // State 1: Intent (Uncommitted Transaction) -> Rollback
            1 => {
                Self::rollback_transaction(device_id, &jrn)?;
                Self::clear(device_id, jrn.sequence)?;
            }
            // State 2: Committed Transaction -> Rollforward
            2 => {
                Self::rollforward_transaction(device_id, &jrn)?;
                Self::clear(device_id, jrn.sequence)?;
            }
            _ => {
                return Err(FsError::CorruptJournal);
            }
        }

        sync_all_buffers(device_id)?;
        Ok(())
    }

    /// Rollback uncommitted Intent transaction:
    /// - Reinstates old_inode_image into Inode Table.
    /// - Deallocates newly allocated blocks in Block Bitmap.
    /// - Reclaims newly allocated inode if Create.
    fn rollback_transaction(device_id: u8, jrn: &DiskJournalBlock) -> Result<(), FsError> {
        // 1. Restore old Inode image
        if jrn.target_inode_num != 0 && jrn.target_inode_num < (MAX_INODES as u32) {
            let inode_idx = jrn.target_inode_num as usize;
            let block_idx = INODE_TABLE_START + (inode_idx / INODES_PER_BLOCK) as u64;
            let offset_in_block = (inode_idx % INODES_PER_BLOCK) * 256;

            let mut raw_block = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, block_idx, &mut raw_block)?;

            unsafe {
                let dst = raw_block.as_mut_ptr().add(offset_in_block) as *mut DiskInode;
                core::ptr::write(dst, jrn.old_inode_image);
            }
            write_block_cached(device_id, block_idx, &raw_block)?;
        }

        // 2. Clear allocated blocks from Block Bitmap
        if jrn.allocated_blocks_count > 0 {
            let mut block_bitmap = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, BLOCK_BITMAP_BLOCK, &mut block_bitmap)?;

            let count = (jrn.allocated_blocks_count as usize).min(10);
            for &lba in &jrn.allocated_blocks[0..count] {
                if lba >= (DATA_BLOCK_START as u32) && (lba as u64) < MAX_VOLUME_BLOCKS {
                    let byte_idx = (lba as usize) / 8;
                    let bit_idx = (lba as usize) % 8;
                    block_bitmap[byte_idx] &= !(1 << bit_idx);
                }
            }
            write_block_cached(device_id, BLOCK_BITMAP_BLOCK, &block_bitmap)?;
        }

        // 3. If Create, clear target inode bit in Inode Bitmap
        if jrn.op_type == (JournalOpType::Create as u32) && jrn.target_inode_num != 0 {
            let mut inode_bitmap = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, INODE_BITMAP_BLOCK, &mut inode_bitmap)?;
            let inode_idx = jrn.target_inode_num as usize;
            inode_bitmap[inode_idx / 8] &= !(1 << (inode_idx % 8));
            write_block_cached(device_id, INODE_BITMAP_BLOCK, &inode_bitmap)?;
        }

        Ok(())
    }

    /// Rollforward committed transaction:
    /// - Writes new_inode_image into Inode Table.
    /// - Reconciles Block Bitmap (allocates allocated_blocks, frees freed_blocks).
    /// - Reconciles Inode Bitmap.
    /// - Updates Directory entry if mutated.
    fn rollforward_transaction(device_id: u8, jrn: &DiskJournalBlock) -> Result<(), FsError> {
        // 1. Write new Inode image
        if jrn.target_inode_num != 0 && jrn.target_inode_num < (MAX_INODES as u32) {
            let inode_idx = jrn.target_inode_num as usize;
            let block_idx = INODE_TABLE_START + (inode_idx / INODES_PER_BLOCK) as u64;
            let offset_in_block = (inode_idx % INODES_PER_BLOCK) * 256;

            let mut raw_block = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, block_idx, &mut raw_block)?;

            unsafe {
                let dst = raw_block.as_mut_ptr().add(offset_in_block) as *mut DiskInode;
                core::ptr::write(dst, jrn.new_inode_image);
            }
            write_block_cached(device_id, block_idx, &raw_block)?;
        }

        // 2. Update Directory entry if present
        if jrn.dir_entry_slot != 0xFFFF_FFFF {
            let slot = jrn.dir_entry_slot as usize;
            if slot < DIR_ENTRIES_PER_BLOCK {
                let dir_block_idx = if jrn.parent_dir_inode == ROOT_DIR_INODE {
                    ROOT_DIR_BLOCK
                } else {
                    ROOT_DIR_BLOCK
                };
                let mut dir_block = [0u8; BLOCK_SIZE];
                read_block_cached(device_id, dir_block_idx, &mut dir_block)?;

                unsafe {
                    let dst = dir_block.as_mut_ptr().add(slot * 64) as *mut DiskDirEntry;
                    core::ptr::write(dst, jrn.dir_entry_copy);
                }
                write_block_cached(device_id, dir_block_idx, &dir_block)?;
            }
        }

        // 3. Update Block Bitmap
        let mut block_bitmap = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, BLOCK_BITMAP_BLOCK, &mut block_bitmap)?;

        let alloc_count = (jrn.allocated_blocks_count as usize).min(10);
        for &lba in &jrn.allocated_blocks[0..alloc_count] {
            if lba >= (DATA_BLOCK_START as u32) && (lba as u64) < MAX_VOLUME_BLOCKS {
                let byte_idx = (lba as usize) / 8;
                let bit_idx = (lba as usize) % 8;
                block_bitmap[byte_idx] |= 1 << bit_idx;
            }
        }

        let free_count = (jrn.freed_blocks_count as usize).min(10);
        for &lba in &jrn.freed_blocks[0..free_count] {
            if lba >= (DATA_BLOCK_START as u32) && (lba as u64) < MAX_VOLUME_BLOCKS {
                let byte_idx = (lba as usize) / 8;
                let bit_idx = (lba as usize) % 8;
                block_bitmap[byte_idx] &= !(1 << bit_idx);
            }
        }
        write_block_cached(device_id, BLOCK_BITMAP_BLOCK, &block_bitmap)?;

        // 4. Update Inode Bitmap
        if jrn.target_inode_num != 0 && jrn.target_inode_num < (MAX_INODES as u32) {
            let mut inode_bitmap = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, INODE_BITMAP_BLOCK, &mut inode_bitmap)?;
            let inode_idx = jrn.target_inode_num as usize;

            if jrn.op_type == (JournalOpType::Create as u32) {
                inode_bitmap[inode_idx / 8] |= 1 << (inode_idx % 8);
            } else if jrn.op_type == (JournalOpType::Delete as u32) {
                inode_bitmap[inode_idx / 8] &= !(1 << (inode_idx % 8));
            }
            write_block_cached(device_id, INODE_BITMAP_BLOCK, &inode_bitmap)?;
        }

        Ok(())
    }
}
