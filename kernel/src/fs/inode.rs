//! Project Zero - Stage 3K Inode & Block Allocation Engine
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).
//! Implements Copy-on-Write for single-indirect metadata blocks (ADR-3K-009).

use crate::fs::types::*;
use crate::fs::buf::{read_block_cached, write_block_cached};

pub struct InodeManager;

impl InodeManager {
    /// Read an inode from the on-disk Inode Table.
    pub fn read_inode(device_id: u8, inode_num: u32) -> Result<DiskInode, FsError> {
        if inode_num == 0 || inode_num >= (MAX_INODES as u32) {
            return Err(FsError::InvalidArgument);
        }
        let inode_idx = inode_num as usize;
        let block_idx = INODE_TABLE_START + (inode_idx / INODES_PER_BLOCK) as u64;
        let offset = (inode_idx % INODES_PER_BLOCK) * 256;

        let mut raw = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, block_idx, &mut raw)?;

        let inode: DiskInode = unsafe {
            core::ptr::read(raw.as_ptr().add(offset) as *const DiskInode)
        };

        if inode.file_type != (InodeType::Free as u16) && !inode.is_valid_checksum() {
            return Err(FsError::CorruptMetadata);
        }
        Ok(inode)
    }

    /// Write an inode to the on-disk Inode Table.
    pub fn write_inode(device_id: u8, inode_num: u32, mut inode: DiskInode) -> Result<(), FsError> {
        if inode_num == 0 || inode_num >= (MAX_INODES as u32) {
            return Err(FsError::InvalidArgument);
        }
        inode.recompute_checksum();

        let inode_idx = inode_num as usize;
        let block_idx = INODE_TABLE_START + (inode_idx / INODES_PER_BLOCK) as u64;
        let offset = (inode_idx % INODES_PER_BLOCK) * 256;

        let mut raw = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, block_idx, &mut raw)?;

        unsafe {
            core::ptr::write(raw.as_mut_ptr().add(offset) as *mut DiskInode, inode);
        }
        write_block_cached(device_id, block_idx, &raw)
    }

    /// Allocate a free inode from the Inode Bitmap (Block 3).
    pub fn alloc_inode(device_id: u8, file_type: InodeType, creator_pid: u64) -> Result<u32, FsError> {
        let mut bitmap = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, INODE_BITMAP_BLOCK, &mut bitmap)?;

        // Inode 0 is NULL, Inode 1 is ROOT_DIR. Scan from 2..MAX_INODES
        for idx in 2..MAX_INODES {
            let byte = idx / 8;
            let bit = idx % 8;
            if (bitmap[byte] & (1 << bit)) == 0 {
                bitmap[byte] |= 1 << bit;
                write_block_cached(device_id, INODE_BITMAP_BLOCK, &bitmap)?;

                let mut inode = DiskInode::empty();
                inode.file_type = file_type as u16;
                inode.creator_pid = creator_pid;
                inode.links_count = 1;
                Self::write_inode(device_id, idx as u32, inode)?;
                return Ok(idx as u32);
            }
        }
        Err(FsError::NoInode)
    }

    /// Free an inode in the Inode Bitmap (Block 3).
    pub fn free_inode(device_id: u8, inode_num: u32) -> Result<(), FsError> {
        if inode_num == 0 || inode_num >= (MAX_INODES as u32) {
            return Err(FsError::InvalidArgument);
        }
        let mut bitmap = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, INODE_BITMAP_BLOCK, &mut bitmap)?;

        let idx = inode_num as usize;
        bitmap[idx / 8] &= !(1 << (idx % 8));
        write_block_cached(device_id, INODE_BITMAP_BLOCK, &bitmap)?;

        let empty = DiskInode::empty();
        Self::write_inode(device_id, inode_num, empty)
    }

    /// Allocate a single 4096-byte data block from the Block Bitmap (Block 4).
    pub fn alloc_block(device_id: u8) -> Result<u32, FsError> {
        let mut bitmap = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, BLOCK_BITMAP_BLOCK, &mut bitmap)?;

        // Data blocks start at DATA_BLOCK_START (22)
        for lba in (DATA_BLOCK_START as usize)..(MAX_VOLUME_BLOCKS as usize) {
            let byte = lba / 8;
            let bit = lba % 8;
            if (bitmap[byte] & (1 << bit)) == 0 {
                bitmap[byte] |= 1 << bit;
                write_block_cached(device_id, BLOCK_BITMAP_BLOCK, &bitmap)?;

                // Zero out the newly allocated block in cache
                let zero = [0u8; BLOCK_SIZE];
                write_block_cached(device_id, lba as u64, &zero)?;
                return Ok(lba as u32);
            }
        }
        Err(FsError::NoSpace)
    }

    /// Free a single 4096-byte data block in the Block Bitmap (Block 4).
    pub fn free_block(device_id: u8, lba: u32) -> Result<(), FsError> {
        if (lba as u64) < DATA_BLOCK_START || (lba as u64) >= MAX_VOLUME_BLOCKS {
            return Err(FsError::InvalidArgument);
        }
        let mut bitmap = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, BLOCK_BITMAP_BLOCK, &mut bitmap)?;

        let idx = lba as usize;
        bitmap[idx / 8] &= !(1 << (idx % 8));
        write_block_cached(device_id, BLOCK_BITMAP_BLOCK, &bitmap)
    }

    /// Map logical block index to physical LBA.
    /// Direct: 0..18. Single indirect: 18..1042.
    pub fn bmap(device_id: u8, inode: &DiskInode, logical_idx: usize) -> Result<Option<u32>, FsError> {
        if logical_idx < 18 {
            let lba = inode.direct_blocks[logical_idx];
            if lba == 0 {
                Ok(None)
            } else {
                Ok(Some(lba))
            }
        } else if logical_idx < 18 + 1024 {
            if inode.indirect_block == 0 {
                return Ok(None);
            }
            let mut indirect_data = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, inode.indirect_block as u64, &mut indirect_data)?;

            let slot = logical_idx - 18;
            let ptrs = unsafe {
                core::slice::from_raw_parts(indirect_data.as_ptr() as *const u32, 1024)
            };
            let lba = ptrs[slot];
            if lba == 0 {
                Ok(None)
            } else {
                Ok(Some(lba))
            }
        } else {
            Err(FsError::InvalidArgument)
        }
    }

    /// Map or allocate logical block index.
    /// Strictly adheres to Copy-on-Write for indirect blocks (ADR-3K-009).
    pub fn bmap_alloc(
        device_id: u8,
        inode: &mut DiskInode,
        logical_idx: usize,
        allocated_blocks: &mut [u32; 10],
        allocated_count: &mut usize,
        freed_blocks: &mut [u32; 10],
        freed_count: &mut usize,
    ) -> Result<u32, FsError> {
        if logical_idx < 18 {
            if inode.direct_blocks[logical_idx] != 0 {
                return Ok(inode.direct_blocks[logical_idx]);
            }
            let new_lba = Self::alloc_block(device_id)?;
            inode.direct_blocks[logical_idx] = new_lba;
            inode.blocks_count += 1;

            if *allocated_count < 10 {
                allocated_blocks[*allocated_count] = new_lba;
                *allocated_count += 1;
            }
            Ok(new_lba)
        } else if logical_idx < 18 + 1024 {
            let slot = logical_idx - 18;
            let new_data_lba = Self::alloc_block(device_id)?;

            if *allocated_count < 10 {
                allocated_blocks[*allocated_count] = new_data_lba;
                *allocated_count += 1;
            }

            if inode.indirect_block == 0 {
                // First allocation into indirect range: allocate fresh indirect block
                let new_indirect_lba = Self::alloc_block(device_id)?;
                if *allocated_count < 10 {
                    allocated_blocks[*allocated_count] = new_indirect_lba;
                    *allocated_count += 1;
                }

                let mut indirect_buf = [0u8; BLOCK_SIZE];
                let ptrs = unsafe {
                    core::slice::from_raw_parts_mut(indirect_buf.as_mut_ptr() as *mut u32, 1024)
                };
                ptrs[slot] = new_data_lba;
                write_block_cached(device_id, new_indirect_lba as u64, &indirect_buf)?;

                inode.indirect_block = new_indirect_lba;
                inode.blocks_count += 2; // 1 data block + 1 indirect block
            } else {
                // CoW rule (ADR-3K-009): Never mutate existing indirect block in place!
                let old_indirect_lba = inode.indirect_block;
                let new_indirect_lba = Self::alloc_block(device_id)?;

                if *allocated_count < 10 {
                    allocated_blocks[*allocated_count] = new_indirect_lba;
                    *allocated_count += 1;
                }
                if *freed_count < 10 {
                    freed_blocks[*freed_count] = old_indirect_lba;
                    *freed_count += 1;
                }

                // Copy old indirect block contents and update slot
                let mut indirect_buf = [0u8; BLOCK_SIZE];
                read_block_cached(device_id, old_indirect_lba as u64, &mut indirect_buf)?;
                let ptrs = unsafe {
                    core::slice::from_raw_parts_mut(indirect_buf.as_mut_ptr() as *mut u32, 1024)
                };
                ptrs[slot] = new_data_lba;
                write_block_cached(device_id, new_indirect_lba as u64, &indirect_buf)?;

                inode.indirect_block = new_indirect_lba;
                inode.blocks_count += 1; // +1 data block (indirect block replaced via CoW)
            }

            Ok(new_data_lba)
        } else {
            Err(FsError::InvalidArgument)
        }
    }

    /// Truncate file, freeing excess data and indirect blocks.
    pub fn truncate(
        device_id: u8,
        inode: &mut DiskInode,
        new_size: u64,
        allocated_blocks: &mut [u32; 10],
        allocated_count: &mut usize,
        freed_blocks: &mut [u32; 10],
        freed_count: &mut usize,
    ) -> Result<(), FsError> {
        let new_blocks_needed = if new_size == 0 {
            0
        } else {
            ((new_size + (BLOCK_SIZE as u64) - 1) / (BLOCK_SIZE as u64)) as usize
        };

        // 1. Direct blocks truncation
        for idx in new_blocks_needed..18 {
            if inode.direct_blocks[idx] != 0 {
                let lba = inode.direct_blocks[idx];
                Self::free_block(device_id, lba)?;
                inode.direct_blocks[idx] = 0;
                inode.blocks_count = inode.blocks_count.saturating_sub(1);
                if *freed_count < 10 {
                    freed_blocks[*freed_count] = lba;
                    *freed_count += 1;
                }
            }
        }

        // 2. Indirect blocks truncation
        if inode.indirect_block != 0 {
            if new_blocks_needed <= 18 {
                // Free all indirect data blocks and the indirect block itself
                let mut indirect_buf = [0u8; BLOCK_SIZE];
                read_block_cached(device_id, inode.indirect_block as u64, &mut indirect_buf)?;
                let ptrs = unsafe {
                    core::slice::from_raw_parts(indirect_buf.as_ptr() as *const u32, 1024)
                };

                for &lba in ptrs {
                    if lba != 0 {
                        Self::free_block(device_id, lba)?;
                        inode.blocks_count = inode.blocks_count.saturating_sub(1);
                        if *freed_count < 10 {
                            freed_blocks[*freed_count] = lba;
                            *freed_count += 1;
                        }
                    }
                }

                let old_indirect = inode.indirect_block;
                Self::free_block(device_id, old_indirect)?;
                inode.indirect_block = 0;
                inode.blocks_count = inode.blocks_count.saturating_sub(1);
                if *freed_count < 10 {
                    freed_blocks[*freed_count] = old_indirect;
                    *freed_count += 1;
                }
            } else if new_blocks_needed < 18 + 1024 {
                // CoW Truncation within indirect range (ADR-3K-009)
                let old_indirect = inode.indirect_block;
                let new_indirect = Self::alloc_block(device_id)?;

                if *allocated_count < 10 {
                    allocated_blocks[*allocated_count] = new_indirect;
                    *allocated_count += 1;
                }
                if *freed_count < 10 {
                    freed_blocks[*freed_count] = old_indirect;
                    *freed_count += 1;
                }

                let mut old_buf = [0u8; BLOCK_SIZE];
                read_block_cached(device_id, old_indirect as u64, &mut old_buf)?;
                let old_ptrs = unsafe {
                    core::slice::from_raw_parts(old_buf.as_ptr() as *const u32, 1024)
                };

                let mut new_buf = [0u8; BLOCK_SIZE];
                let new_ptrs = unsafe {
                    core::slice::from_raw_parts_mut(new_buf.as_mut_ptr() as *mut u32, 1024)
                };

                let keep_slots = new_blocks_needed - 18;
                for i in 0..keep_slots {
                    new_ptrs[i] = old_ptrs[i];
                }
                for i in keep_slots..1024 {
                    if old_ptrs[i] != 0 {
                        Self::free_block(device_id, old_ptrs[i])?;
                        inode.blocks_count = inode.blocks_count.saturating_sub(1);
                        if *freed_count < 10 {
                            freed_blocks[*freed_count] = old_ptrs[i];
                            *freed_count += 1;
                        }
                    }
                }

                write_block_cached(device_id, new_indirect as u64, &new_buf)?;
                inode.indirect_block = new_indirect;
            }
        }

        inode.size_bytes = new_size;
        Ok(())
    }
}
