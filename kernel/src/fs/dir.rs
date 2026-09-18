//! Project Zero - Stage 3K Directory Subsystem
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).
//! Manages 64-byte DiskDirEntry slots, name validation, and relative path lookup.

use crate::fs::types::*;
use crate::fs::buf::{read_block_cached, write_block_cached};
use crate::fs::inode::InodeManager;

pub struct DirectoryManager;

impl DirectoryManager {
    /// Validates a single path component:
    /// - Length 1..=55 bytes
    /// - No null bytes
    /// - No '/' or '\\' separators (relative within directory capability)
    /// - Rejects "." and ".."
    pub fn validate_filename(name: &[u8]) -> Result<(), FsError> {
        if name.is_empty() || name.len() > MAX_FILENAME_LEN {
            return Err(FsError::InvalidArgument);
        }
        if name == b"." || name == b".." {
            return Err(FsError::InvalidArgument);
        }
        for &b in name {
            if b == 0 || b == b'/' || b == b'\\' {
                return Err(FsError::InvalidArgument);
            }
        }
        Ok(())
    }

    /// Lookup an entry by filename within a directory inode.
    pub fn lookup(device_id: u8, dir_inode_num: u32, name: &[u8]) -> Result<(u32, InodeType, usize), FsError> {
        Self::validate_filename(name)?;

        let dir_inode = InodeManager::read_inode(device_id, dir_inode_num)?;
        if dir_inode.file_type != (InodeType::Directory as u16) {
            return Err(FsError::NotDirectory);
        }

        // For Stage 3K, directory data is located in root dir block or direct block 0
        let dir_block_idx = if dir_inode_num == ROOT_DIR_INODE {
            ROOT_DIR_BLOCK
        } else {
            if dir_inode.direct_blocks[0] == 0 {
                return Err(FsError::NotFound);
            }
            dir_inode.direct_blocks[0] as u64
        };

        let mut raw = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, dir_block_idx, &mut raw)?;

        for slot in 0..DIR_ENTRIES_PER_BLOCK {
            let offset = slot * 64;
            let entry: DiskDirEntry = unsafe {
                core::ptr::read(raw.as_ptr().add(offset) as *const DiskDirEntry)
            };

            if entry.inode_num != 0 && (entry.name_len as usize) == name.len() {
                if &entry.name[0..name.len()] == name {
                    let ftype = match entry.file_type {
                        1 => InodeType::Regular,
                        2 => InodeType::Directory,
                        _ => InodeType::Free,
                    };
                    return Ok((entry.inode_num, ftype, slot));
                }
            }
        }

        Err(FsError::NotFound)
    }

    /// Insert an entry into a directory.
    pub fn insert(
        device_id: u8,
        dir_inode_num: u32,
        name: &[u8],
        target_inode_num: u32,
        file_type: InodeType,
    ) -> Result<(usize, DiskDirEntry), FsError> {
        Self::validate_filename(name)?;

        // Ensure not already present
        if Self::lookup(device_id, dir_inode_num, name).is_ok() {
            return Err(FsError::AlreadyExists);
        }

        let dir_inode = InodeManager::read_inode(device_id, dir_inode_num)?;
        if dir_inode.file_type != (InodeType::Directory as u16) {
            return Err(FsError::NotDirectory);
        }

        let dir_block_idx = if dir_inode_num == ROOT_DIR_INODE {
            ROOT_DIR_BLOCK
        } else {
            dir_inode.direct_blocks[0] as u64
        };

        let mut raw = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, dir_block_idx, &mut raw)?;

        for slot in 0..DIR_ENTRIES_PER_BLOCK {
            let offset = slot * 64;
            let entry: DiskDirEntry = unsafe {
                core::ptr::read(raw.as_ptr().add(offset) as *const DiskDirEntry)
            };

            if entry.inode_num == 0 {
                let mut new_entry = DiskDirEntry::empty();
                new_entry.inode_num = target_inode_num;
                new_entry.name_len = name.len() as u8;
                new_entry.file_type = file_type as u8;
                new_entry.name[0..name.len()].copy_from_slice(name);

                unsafe {
                    core::ptr::write(raw.as_mut_ptr().add(offset) as *mut DiskDirEntry, new_entry);
                }
                write_block_cached(device_id, dir_block_idx, &raw)?;
                return Ok((slot, new_entry));
            }
        }

        Err(FsError::NoSpace) // Directory block full
    }

    /// Unlink an entry from a directory.
    pub fn unlink(
        device_id: u8,
        dir_inode_num: u32,
        name: &[u8],
    ) -> Result<(u32, usize, DiskDirEntry), FsError> {
        let (target_inode_num, _, slot) = Self::lookup(device_id, dir_inode_num, name)?;

        let dir_block_idx = if dir_inode_num == ROOT_DIR_INODE {
            ROOT_DIR_BLOCK
        } else {
            let dir_inode = InodeManager::read_inode(device_id, dir_inode_num)?;
            dir_inode.direct_blocks[0] as u64
        };

        let mut raw = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, dir_block_idx, &mut raw)?;

        let offset = slot * 64;
        let old_entry: DiskDirEntry = unsafe {
            core::ptr::read(raw.as_ptr().add(offset) as *const DiskDirEntry)
        };

        // Zero slot
        let empty_entry = DiskDirEntry::empty();
        unsafe {
            core::ptr::write(raw.as_mut_ptr().add(offset) as *mut DiskDirEntry, empty_entry);
        }
        write_block_cached(device_id, dir_block_idx, &raw)?;

        Ok((target_inode_num, slot, old_entry))
    }
}
