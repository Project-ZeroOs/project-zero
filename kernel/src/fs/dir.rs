//! Project Zero - Stage 3K Directory Subsystem
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).
//! Manages 64-byte DiskDirEntry slots, name validation, and relative path lookup.

use crate::fs::types::*;
use crate::fs::buf::{read_block_cached, write_block_cached};
use crate::fs::inode::InodeManager;

pub struct DirectoryManager;

impl DirectoryManager {
    /// Bumps directory modification sequence counter using 32-bit wrapping addition skipping 0.
    pub fn bump_generation(inode: &mut DiskInode) {
        inode.generation = inode.generation.wrapping_add(1);
        if inode.generation == 0 {
            inode.generation = 1;
        }
    }

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

    /// Insert an entry into a directory, bumping directory generation.
    pub fn insert(
        device_id: u8,
        dir_inode_num: u32,
        name: &[u8],
        target_inode_num: u32,
        file_type: InodeType,
    ) -> Result<(usize, DiskDirEntry), FsError> {
        Self::validate_filename(name)?;

        if Self::lookup(device_id, dir_inode_num, name).is_ok() {
            return Err(FsError::AlreadyExists);
        }

        let mut dir_inode = InodeManager::read_inode(device_id, dir_inode_num)?;
        if dir_inode.file_type != (InodeType::Directory as u16) {
            return Err(FsError::NotDirectory);
        }

        let dir_block_idx = if dir_inode_num == ROOT_DIR_INODE {
            ROOT_DIR_BLOCK
        } else {
            if dir_inode.direct_blocks[0] == 0 {
                return Err(FsError::NoSpace);
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

                // Bump parent directory generation
                Self::bump_generation(&mut dir_inode);
                InodeManager::write_inode(device_id, dir_inode_num, dir_inode)?;

                return Ok((slot, new_entry));
            }
        }

        Err(FsError::NoSpace)
    }

    /// Create a new directory inside a parent directory.
    pub fn create_dir(
        device_id: u8,
        parent_dir_inode_num: u32,
        name: &[u8],
        caller_pid: u64,
    ) -> Result<(u32, usize, DiskDirEntry), FsError> {
        Self::validate_filename(name)?;

        let new_inode_num = InodeManager::alloc_inode(device_id, InodeType::Directory, caller_pid)?;
        let mut new_dir_inode = InodeManager::read_inode(device_id, new_inode_num)?;

        // Allocate a data block for the new directory
        let mut alloc_buf = [0u32; 10];
        let mut alloc_cnt = 0;
        let mut free_buf = [0u32; 10];
        let mut free_cnt = 0;

        let block_lba = match InodeManager::bmap_alloc(
            device_id,
            &mut new_dir_inode,
            0,
            &mut alloc_buf,
            &mut alloc_cnt,
            &mut free_buf,
            &mut free_cnt,
        ) {
            Ok(lba) => lba,
            Err(e) => {
                let _ = InodeManager::free_inode(device_id, new_inode_num);
                return Err(e);
            }
        };

        // Zero out directory block
        let zero_block = [0u8; BLOCK_SIZE];
        write_block_cached(device_id, block_lba as u64, &zero_block)?;

        new_dir_inode.links_count = 1;
        new_dir_inode.generation = 1;
        InodeManager::write_inode(device_id, new_inode_num, new_dir_inode)?;

        match Self::insert(device_id, parent_dir_inode_num, name, new_inode_num, InodeType::Directory) {
            Ok((slot, entry)) => Ok((new_inode_num, slot, entry)),
            Err(e) => {
                let _ = InodeManager::free_inode(device_id, new_inode_num);
                Err(e)
            }
        }
    }

    /// Unlink an entry from a directory, bumping directory generation and managing pending delete.
    pub fn unlink(
        device_id: u8,
        dir_inode_num: u32,
        name: &[u8],
    ) -> Result<(u32, usize, DiskDirEntry), FsError> {
        let (target_inode_num, ftype, slot) = Self::lookup(device_id, dir_inode_num, name)?;

        if ftype == InodeType::Directory {
            if !Self::is_dir_empty(device_id, target_inode_num)? {
                return Err(FsError::NotEmpty);
            }
        }

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

        let empty_entry = DiskDirEntry::empty();
        unsafe {
            core::ptr::write(raw.as_mut_ptr().add(offset) as *mut DiskDirEntry, empty_entry);
        }
        write_block_cached(device_id, dir_block_idx, &raw)?;

        // Bump parent directory generation
        let mut parent_inode = InodeManager::read_inode(device_id, dir_inode_num)?;
        Self::bump_generation(&mut parent_inode);
        InodeManager::write_inode(device_id, dir_inode_num, parent_inode)?;

        // Update target inode link count
        let mut target_inode = InodeManager::read_inode(device_id, target_inode_num)?;
        target_inode.links_count = target_inode.links_count.saturating_sub(1);

        if target_inode.links_count == 0 {
            if crate::fs::file::FileManager::has_open_references(device_id, target_inode_num) {
                target_inode.flags |= INODE_FLAG_PENDING_DELETE;
                InodeManager::write_inode(device_id, target_inode_num, target_inode)?;
            } else {
                let mut a_buf = [0u32; 10];
                let mut a_cnt = 0;
                let mut f_buf = [0u32; 10];
                let mut f_cnt = 0;
                let _ = InodeManager::truncate(device_id, &mut target_inode, 0, &mut a_buf, &mut a_cnt, &mut f_buf, &mut f_cnt);
                let _ = InodeManager::free_inode(device_id, target_inode_num);
            }
        } else {
            InodeManager::write_inode(device_id, target_inode_num, target_inode)?;
        }

        Ok((target_inode_num, slot, old_entry))
    }

    /// Atomically rename or move an entry across/within directory trees.
    pub fn rename(
        device_id: u8,
        src_dir_inode_num: u32,
        src_name: &[u8],
        dst_dir_inode_num: u32,
        dst_name: &[u8],
        flags: u32,
        caller_pid: u64,
    ) -> Result<(), FsError> {
        Self::validate_filename(src_name)?;
        Self::validate_filename(dst_name)?;

        let (src_target_inode_num, src_ftype, src_slot) = Self::lookup(device_id, src_dir_inode_num, src_name)?;

        let dst_lookup_res = Self::lookup(device_id, dst_dir_inode_num, dst_name);
        if let Ok((dst_target_inode_num, dst_ftype, dst_slot)) = dst_lookup_res {
            if (flags & 1) != 0 {
                return Err(FsError::AlreadyExists); // RENAME_EXCL
            }
            if src_ftype != dst_ftype {
                return Err(FsError::InvalidArgument);
            }
            if dst_ftype == InodeType::Directory && !Self::is_dir_empty(device_id, dst_target_inode_num)? {
                return Err(FsError::NotEmpty);
            }

            // Execute Rename-Over Atomic Overwrite
            let old_src_inode = InodeManager::read_inode(device_id, src_target_inode_num)?;
            let mut old_dst_inode = InodeManager::read_inode(device_id, dst_target_inode_num)?;

            let mut new_src_inode = old_src_inode;
            let mut new_dst_inode = old_dst_inode;
            new_dst_inode.links_count = new_dst_inode.links_count.saturating_sub(1);

            if new_dst_inode.links_count == 0 {
                if crate::fs::file::FileManager::has_open_references(device_id, dst_target_inode_num) {
                    new_dst_inode.flags |= INODE_FLAG_PENDING_DELETE;
                }
            }

            let mut src_parent_inode = InodeManager::read_inode(device_id, src_dir_inode_num)?;
            Self::bump_generation(&mut src_parent_inode);
            InodeManager::write_inode(device_id, src_dir_inode_num, src_parent_inode)?;

            if src_dir_inode_num != dst_dir_inode_num {
                let mut dst_parent_inode = InodeManager::read_inode(device_id, dst_dir_inode_num)?;
                Self::bump_generation(&mut dst_parent_inode);
                InodeManager::write_inode(device_id, dst_dir_inode_num, dst_parent_inode)?;
            }

            // Zero out source slot
            let src_block_idx = if src_dir_inode_num == ROOT_DIR_INODE { ROOT_DIR_BLOCK } else { InodeManager::read_inode(device_id, src_dir_inode_num)?.direct_blocks[0] as u64 };
            let mut raw_src = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, src_block_idx, &mut raw_src)?;
            unsafe {
                core::ptr::write(raw_src.as_mut_ptr().add(src_slot * 64) as *mut DiskDirEntry, DiskDirEntry::empty());
            }
            write_block_cached(device_id, src_block_idx, &raw_src)?;

            // Overwrite dst slot with src entry
            let dst_block_idx = if dst_dir_inode_num == ROOT_DIR_INODE { ROOT_DIR_BLOCK } else { InodeManager::read_inode(device_id, dst_dir_inode_num)?.direct_blocks[0] as u64 };
            let mut raw_dst = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, dst_block_idx, &mut raw_dst)?;
            let mut new_dst_entry = DiskDirEntry::empty();
            new_dst_entry.inode_num = src_target_inode_num;
            new_dst_entry.name_len = dst_name.len() as u8;
            new_dst_entry.file_type = src_ftype as u8;
            new_dst_entry.name[0..dst_name.len()].copy_from_slice(dst_name);

            unsafe {
                core::ptr::write(raw_dst.as_mut_ptr().add(dst_slot * 64) as *mut DiskDirEntry, new_dst_entry);
            }
            write_block_cached(device_id, dst_block_idx, &raw_dst)?;

            // Update dst inode
            if new_dst_inode.links_count == 0 && (new_dst_inode.flags & INODE_FLAG_PENDING_DELETE) == 0 {
                let mut a_buf = [0u32; 10];
                let mut a_cnt = 0;
                let mut f_buf = [0u32; 10];
                let mut f_cnt = 0;
                let _ = InodeManager::truncate(device_id, &mut new_dst_inode, 0, &mut a_buf, &mut a_cnt, &mut f_buf, &mut f_cnt);
                let _ = InodeManager::free_inode(device_id, dst_target_inode_num);
            } else {
                InodeManager::write_inode(device_id, dst_target_inode_num, new_dst_inode)?;
            }

            // Journal V2 transaction logging
            let mut jrn = DiskJournalBlockV2::empty();
            jrn.sequence = 1;
            jrn.op_type = JournalOpType::Rename as u32;
            jrn.state = JournalState::Committed as u32;
            jrn.src_parent_dir_inode = src_dir_inode_num;
            jrn.src_dir_entry_slot = src_slot as u32;
            jrn.src_target_inode_num = src_target_inode_num;
            jrn.dst_parent_dir_inode = dst_dir_inode_num;
            jrn.dst_dir_entry_slot = dst_slot as u32;
            jrn.dst_target_inode_num = dst_target_inode_num;
            jrn.src_old_inode_image = old_src_inode;
            jrn.src_new_inode_image = new_src_inode;
            jrn.dst_old_inode_image = old_dst_inode;
            jrn.dst_new_inode_image = new_dst_inode;

            let _ = crate::fs::journal::Journal::write_journal_block_v2(device_id, jrn);
            return Ok(());
        }

        // Destination does not exist: normal rename/move
        let (dst_slot, new_dst_entry) = Self::insert(device_id, dst_dir_inode_num, dst_name, src_target_inode_num, src_ftype)?;
        let _ = Self::unlink(device_id, src_dir_inode_num, src_name)?;

        let mut jrn = DiskJournalBlockV2::empty();
        jrn.sequence = 1;
        jrn.op_type = JournalOpType::Rename as u32;
        jrn.state = JournalState::Committed as u32;
        jrn.src_parent_dir_inode = src_dir_inode_num;
        jrn.src_dir_entry_slot = src_slot as u32;
        jrn.src_target_inode_num = src_target_inode_num;
        jrn.dst_parent_dir_inode = dst_dir_inode_num;
        jrn.dst_dir_entry_slot = dst_slot as u32;
        jrn.dst_dir_entry_copy = new_dst_entry;
        let _ = crate::fs::journal::Journal::write_journal_block_v2(device_id, jrn);

        Ok(())
    }

    /// Structured directory entry iteration for `SYS_DIR_READ` with Policy A generation check.
    pub fn read_entries(
        device_id: u8,
        dir_inode_num: u32,
        entry_offset: u32,
        captured_gen: u32,
        out_buf: &mut [UserDirEntry],
    ) -> Result<(usize, u32), FsError> {
        let dir_inode = InodeManager::read_inode(device_id, dir_inode_num)?;
        if dir_inode.file_type != (InodeType::Directory as u16) {
            return Err(FsError::NotDirectory);
        }

        let active_gen = dir_inode.generation;

        // Policy A Generation Check
        if entry_offset > 0 && captured_gen != active_gen {
            return Err(FsError::Busy); // Maps to SyscallError::ResourceConflict (-EBUSY)
        }

        let dir_block_idx = if dir_inode_num == ROOT_DIR_INODE {
            ROOT_DIR_BLOCK
        } else {
            if dir_inode.direct_blocks[0] == 0 {
                return Ok((0, active_gen));
            }
            dir_inode.direct_blocks[0] as u64
        };

        let mut raw = [0u8; BLOCK_SIZE];
        read_block_cached(device_id, dir_block_idx, &mut raw)?;

        let start_slot = entry_offset as usize;
        let mut count = 0;

        for slot in start_slot..DIR_ENTRIES_PER_BLOCK {
            if count >= out_buf.len() {
                break;
            }

            let offset = slot * 64;
            let entry: DiskDirEntry = unsafe {
                core::ptr::read(raw.as_ptr().add(offset) as *const DiskDirEntry)
            };

            if entry.inode_num != 0 {
                let mut user_entry = UserDirEntry {
                    inode_number: entry.inode_num,
                    file_type: entry.file_type,
                    name_len: entry.name_len,
                    reserved: 0,
                    name: [0; 56],
                };
                let nlen = (entry.name_len as usize).min(55);
                user_entry.name[0..nlen].copy_from_slice(&entry.name[0..nlen]);
                out_buf[count] = user_entry;
                count += 1;
            }
        }

        Ok((count, active_gen))
    }

    /// Check if directory contains no active entries.
    pub fn is_dir_empty(device_id: u8, dir_inode_num: u32) -> Result<bool, FsError> {
        let dir_inode = InodeManager::read_inode(device_id, dir_inode_num)?;
        if dir_inode.file_type != (InodeType::Directory as u16) {
            return Err(FsError::NotDirectory);
        }

        let dir_block_idx = if dir_inode_num == ROOT_DIR_INODE {
            ROOT_DIR_BLOCK
        } else {
            if dir_inode.direct_blocks[0] == 0 {
                return Ok(true);
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
            if entry.inode_num != 0 {
                return Ok(false);
            }
        }

        Ok(true)
    }
}
