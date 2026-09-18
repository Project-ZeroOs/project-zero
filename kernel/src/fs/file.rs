//! Project Zero - Stage 3K StorageObject & File Operations
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).
//! Manages open file descriptions, seek offsets, chunked 32 KiB transactional writes,
//! and pending deletion lifecycle.

use core::sync::atomic::{AtomicBool, Ordering};
use crate::fs::types::*;
use crate::fs::buf::{read_block_cached, write_block_cached, sync_all_buffers};
use crate::fs::inode::InodeManager;
use crate::fs::journal::Journal;

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
    pub open_flags: u32,          // 0x18..0x1C: O_READ, O_WRITE, O_APPEND, etc.
    pub in_flight_ops: u16,       // 0x1C..0x1E: Active operations reference pin
    pub _pad2: u16,               // 0x1E..0x20: Alignment padding
    pub cached_size: u64,         // 0x20..0x28: Cached logical size
    pub _reserved: [u8; 8],       // 0x28..0x30: Reserved expansion
}

const _: () = assert!(core::mem::size_of::<StorageObject>() == 48);
const _: () = assert!(core::mem::align_of::<StorageObject>() == 8);

impl StorageObject {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            _pad0: [0; 3],
            inode_num: 0,
            generation: 0,
            device_id: 0,
            _pad1: [0; 3],
            cursor_offset: 0,
            open_flags: 0,
            in_flight_ops: 0,
            _pad2: 0,
            cached_size: 0,
            _reserved: [0; 8],
        }
    }
}

pub struct StorageObjectTableLock {
    lock: AtomicBool,
}

impl StorageObjectTableLock {
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

pub static STORAGE_OBJECT_TABLE_LOCK: StorageObjectTableLock = StorageObjectTableLock::new();
pub static mut STORAGE_OBJECT_TABLE: [StorageObject; MAX_OPEN_STORAGE_OBJECTS] = [
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
    StorageObject::empty(), StorageObject::empty(), StorageObject::empty(), StorageObject::empty(),
];

pub struct FileManager;

impl FileManager {
    pub fn init() {
        STORAGE_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            for slot in STORAGE_OBJECT_TABLE.iter_mut() {
                *slot = StorageObject::empty();
            }
        }
        STORAGE_OBJECT_TABLE_LOCK.release();
    }

    /// Allocate a StorageObject slot for an open file description.
    pub fn alloc_storage_object(
        device_id: u8,
        inode_num: u32,
        generation: u32,
        open_flags: u32,
        cached_size: u64,
    ) -> Result<usize, FsError> {
        STORAGE_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            for (idx, slot) in STORAGE_OBJECT_TABLE.iter_mut().enumerate() {
                if !slot.occupied {
                    slot.occupied = true;
                    slot.device_id = device_id;
                    slot.inode_num = inode_num;
                    slot.generation = generation;
                    slot.open_flags = open_flags;
                    slot.cursor_offset = if (open_flags & O_APPEND) != 0 { cached_size } else { 0 };
                    slot.in_flight_ops = 0;
                    slot.cached_size = cached_size;
                    STORAGE_OBJECT_TABLE_LOCK.release();
                    return Ok(idx);
                }
            }
        }
        STORAGE_OBJECT_TABLE_LOCK.release();
        Err(FsError::TableFull)
    }

    /// Check whether any open StorageObject references the specified inode.
    pub fn has_open_references(device_id: u8, inode_num: u32) -> bool {
        STORAGE_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            for slot in STORAGE_OBJECT_TABLE.iter() {
                if slot.occupied && slot.device_id == device_id && slot.inode_num == inode_num {
                    STORAGE_OBJECT_TABLE_LOCK.release();
                    return true;
                }
            }
        }
        STORAGE_OBJECT_TABLE_LOCK.release();
        false
    }

    /// Release StorageObject slot. If PENDING_DELETE and links_count == 0, reclaims on-disk blocks.
    pub fn free_storage_object(idx: usize) -> Result<(), FsError> {
        if idx >= MAX_OPEN_STORAGE_OBJECTS {
            return Err(FsError::InvalidArgument);
        }

        STORAGE_OBJECT_TABLE_LOCK.acquire();
        let (device_id, inode_num, occupied) = unsafe {
            let s = &STORAGE_OBJECT_TABLE[idx];
            (s.device_id, s.inode_num, s.occupied)
        };

        if !occupied {
            STORAGE_OBJECT_TABLE_LOCK.release();
            return Ok(());
        }

        unsafe {
            STORAGE_OBJECT_TABLE[idx] = StorageObject::empty();
        }
        STORAGE_OBJECT_TABLE_LOCK.release();

        // Check if file is marked for pending deletion and no other handles remain
        if !Self::has_open_references(device_id, inode_num) {
            if let Ok(mut inode) = InodeManager::read_inode(device_id, inode_num) {
                if (inode.flags & INODE_FLAG_PENDING_DELETE) != 0 || inode.links_count == 0 {
                    let mut alloc_buf = [0u32; 10];
                    let mut alloc_count = 0;
                    let mut free_buf = [0u32; 10];
                    let mut free_count = 0;
                    let _ = InodeManager::truncate(
                        device_id,
                        &mut inode,
                        0,
                        &mut alloc_buf,
                        &mut alloc_count,
                        &mut free_buf,
                        &mut free_count,
                    );
                    let _ = InodeManager::free_inode(device_id, inode_num);
                }
            }
        }

        Ok(())
    }

    /// Read bytes from an open file at current cursor offset.
    pub fn read(idx: usize, buf: &mut [u8]) -> Result<usize, FsError> {
        if idx >= MAX_OPEN_STORAGE_OBJECTS || buf.is_empty() {
            return Ok(0);
        }

        STORAGE_OBJECT_TABLE_LOCK.acquire();
        let (device_id, inode_num, cursor_offset, cached_size) = unsafe {
            let slot = &mut STORAGE_OBJECT_TABLE[idx];
            if !slot.occupied {
                STORAGE_OBJECT_TABLE_LOCK.release();
                return Err(FsError::NotFound);
            }
            slot.in_flight_ops += 1;
            (slot.device_id, slot.inode_num, slot.cursor_offset, slot.cached_size)
        };
        STORAGE_OBJECT_TABLE_LOCK.release();

        if cursor_offset >= cached_size {
            STORAGE_OBJECT_TABLE_LOCK.acquire();
            unsafe {
                STORAGE_OBJECT_TABLE[idx].in_flight_ops = STORAGE_OBJECT_TABLE[idx].in_flight_ops.saturating_sub(1);
            }
            STORAGE_OBJECT_TABLE_LOCK.release();
            return Ok(0); // EOF
        }

        let max_read = (cached_size - cursor_offset) as usize;
        let to_read = buf.len().min(max_read);

        let inode = InodeManager::read_inode(device_id, inode_num)?;
        let mut bytes_read = 0;

        while bytes_read < to_read {
            let current_pos = cursor_offset + (bytes_read as u64);
            let logical_block = (current_pos / (BLOCK_SIZE as u64)) as usize;
            let offset_in_block = (current_pos % (BLOCK_SIZE as u64)) as usize;
            let chunk_len = (BLOCK_SIZE - offset_in_block).min(to_read - bytes_read);

            let lba = match InodeManager::bmap(device_id, &inode, logical_block)? {
                Some(l) => l,
                None => {
                    // Sparse hole: return zeros
                    buf[bytes_read..bytes_read + chunk_len].fill(0);
                    bytes_read += chunk_len;
                    continue;
                }
            };

            let mut block_data = [0u8; BLOCK_SIZE];
            read_block_cached(device_id, lba as u64, &mut block_data)?;
            buf[bytes_read..bytes_read + chunk_len].copy_from_slice(&block_data[offset_in_block..offset_in_block + chunk_len]);
            bytes_read += chunk_len;
        }

        STORAGE_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let slot = &mut STORAGE_OBJECT_TABLE[idx];
            slot.cursor_offset += bytes_read as u64;
            slot.in_flight_ops = slot.in_flight_ops.saturating_sub(1);
        }
        STORAGE_OBJECT_TABLE_LOCK.release();

        Ok(bytes_read)
    }

    /// Write bytes to an open file. Chunks writes > 32 KiB into sequential atomic transactions.
    pub fn write(idx: usize, data: &[u8]) -> Result<usize, FsError> {
        if idx >= MAX_OPEN_STORAGE_OBJECTS || data.is_empty() {
            return Ok(0);
        }

        STORAGE_OBJECT_TABLE_LOCK.acquire();
        let (device_id, inode_num, cursor_offset) = unsafe {
            let slot = &mut STORAGE_OBJECT_TABLE[idx];
            if !slot.occupied {
                STORAGE_OBJECT_TABLE_LOCK.release();
                return Err(FsError::NotFound);
            }
            if (slot.open_flags & O_WRITE) == 0 {
                STORAGE_OBJECT_TABLE_LOCK.release();
                return Err(FsError::PermissionDenied);
            }
            slot.in_flight_ops += 1;
            (slot.device_id, slot.inode_num, slot.cursor_offset)
        };
        STORAGE_OBJECT_TABLE_LOCK.release();

        let mut total_written = 0;

        // Process write in 32 KiB chunks (MAX_DATA_BLOCKS_PER_TX = 8 blocks)
        for chunk in data.chunks(MAX_TRANSACTION_BYTES) {
            let chunk_written = match Self::write_chunk_tx(device_id, inode_num, cursor_offset + (total_written as u64), chunk) {
                Ok(w) => w,
                Err(e) => {
                    // Update cursor with whatever was committed before failure
                    STORAGE_OBJECT_TABLE_LOCK.acquire();
                    unsafe {
                        let slot = &mut STORAGE_OBJECT_TABLE[idx];
                        slot.cursor_offset += total_written as u64;
                        slot.cached_size = slot.cached_size.max(slot.cursor_offset);
                        slot.in_flight_ops = slot.in_flight_ops.saturating_sub(1);
                    }
                    STORAGE_OBJECT_TABLE_LOCK.release();
                    if total_written > 0 {
                        return Ok(total_written);
                    } else {
                        return Err(e);
                    }
                }
            };
            total_written += chunk_written;
        }

        STORAGE_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let slot = &mut STORAGE_OBJECT_TABLE[idx];
            slot.cursor_offset += total_written as u64;
            slot.cached_size = slot.cached_size.max(slot.cursor_offset);
            slot.in_flight_ops = slot.in_flight_ops.saturating_sub(1);
        }
        STORAGE_OBJECT_TABLE_LOCK.release();

        Ok(total_written)
    }

    /// Execute a single atomic 32 KiB transaction using the 6-step commit protocol.
    fn write_chunk_tx(device_id: u8, inode_num: u32, start_pos: u64, chunk: &[u8]) -> Result<usize, FsError> {
        let old_inode = InodeManager::read_inode(device_id, inode_num)?;
        let mut new_inode = old_inode;

        let mut allocated_blocks = [0u32; 10];
        let mut allocated_count = 0;
        let mut freed_blocks = [0u32; 10];
        let mut freed_count = 0;

        let mut bytes_mapped = 0;
        let mut block_lbas = [0u32; 8];
        let mut num_blocks = 0;

        while bytes_mapped < chunk.len() {
            let current_pos = start_pos + (bytes_mapped as u64);
            let logical_block = (current_pos / (BLOCK_SIZE as u64)) as usize;
            let offset_in_block = (current_pos % (BLOCK_SIZE as u64)) as usize;
            let write_len = (BLOCK_SIZE - offset_in_block).min(chunk.len() - bytes_mapped);

            let lba = InodeManager::bmap_alloc(
                device_id,
                &mut new_inode,
                logical_block,
                &mut allocated_blocks,
                &mut allocated_count,
                &mut freed_blocks,
                &mut freed_count,
            )?;

            if num_blocks < 8 {
                block_lbas[num_blocks] = lba;
                num_blocks += 1;
            }
            bytes_mapped += write_len;
        }

        new_inode.size_bytes = new_inode.size_bytes.max(start_pos + (bytes_mapped as u64));
        new_inode.recompute_checksum();

        // -------------------------------------------------------------------
        // 6-STEP TRANSACTION COMMIT PROTOCOL (ADR-3K-002, ADR-3K-009)
        // -------------------------------------------------------------------

        // Step 1: Write Journal INTENT Record
        let mut jrn = DiskJournalBlock::empty();
        jrn.sequence = 1;
        jrn.op_type = JournalOpType::Write as u32;
        jrn.state = JournalState::Intent as u32;
        jrn.target_inode_num = inode_num;
        jrn.allocated_blocks_count = allocated_count as u32;
        jrn.freed_blocks_count = freed_count as u32;
        jrn.allocated_blocks = allocated_blocks;
        jrn.freed_blocks = freed_blocks;
        jrn.old_inode_image = old_inode;
        jrn.new_inode_image = new_inode;
        Journal::write_journal_block(device_id, jrn)?;

        // Step 2: Write Data & CoW Indirect Blocks to Disk and Flush
        let mut bytes_written = 0;
        let mut blk_data = [0u8; BLOCK_SIZE];
        let mut blk_idx = 0;
        while bytes_written < chunk.len() {
            let current_pos = start_pos + (bytes_written as u64);
            let offset_in_block = (current_pos % (BLOCK_SIZE as u64)) as usize;
            let write_len = (BLOCK_SIZE - offset_in_block).min(chunk.len() - bytes_written);
            let lba = block_lbas[blk_idx];
            blk_idx += 1;

            if offset_in_block > 0 || write_len < BLOCK_SIZE {
                read_block_cached(device_id, lba as u64, &mut blk_data)?;
            }
            blk_data[offset_in_block..offset_in_block + write_len].copy_from_slice(&chunk[bytes_written..bytes_written + write_len]);
            write_block_cached(device_id, lba as u64, &blk_data)?;

            bytes_written += write_len;
        }
        sync_all_buffers(device_id)?;

        // Step 3: Write Journal COMMITTED Record (COMMIT POINT)
        jrn.state = JournalState::Committed as u32;
        Journal::write_journal_block(device_id, jrn)?;

        // Step 4: Write Metadata Blocks (Inode Table & Bitmap) and Flush
        InodeManager::write_inode(device_id, inode_num, new_inode)?;
        sync_all_buffers(device_id)?;

        // Step 5: Superblock Update
        // (Free blocks updated during alloc/free)

        // Step 6: Clear Journal
        Journal::clear(device_id, jrn.sequence + 1)?;

        Ok(bytes_written)
    }

    /// Query file metadata.
    pub fn stat(idx: usize) -> Result<UserFileStat, FsError> {
        if idx >= MAX_OPEN_STORAGE_OBJECTS {
            return Err(FsError::InvalidArgument);
        }
        STORAGE_OBJECT_TABLE_LOCK.acquire();
        let (device_id, inode_num, occupied) = unsafe {
            let s = &STORAGE_OBJECT_TABLE[idx];
            (s.device_id, s.inode_num, s.occupied)
        };
        STORAGE_OBJECT_TABLE_LOCK.release();

        if !occupied {
            return Err(FsError::NotFound);
        }

        let inode = InodeManager::read_inode(device_id, inode_num)?;
        Ok(UserFileStat {
            size_bytes: inode.size_bytes,
            blocks_count: inode.blocks_count,
            file_type: inode.file_type as u32,
            generation: inode.generation,
            mtime_ticks: inode.mtime_ticks,
        })
    }

    /// Sync file dirty buffers to persistent storage.
    pub fn sync(idx: usize) -> Result<(), FsError> {
        if idx >= MAX_OPEN_STORAGE_OBJECTS {
            return Err(FsError::InvalidArgument);
        }
        STORAGE_OBJECT_TABLE_LOCK.acquire();
        let (device_id, occupied) = unsafe {
            let s = &STORAGE_OBJECT_TABLE[idx];
            (s.device_id, s.occupied)
        };
        STORAGE_OBJECT_TABLE_LOCK.release();

        if !occupied {
            return Err(FsError::NotFound);
        }

        sync_all_buffers(device_id)
    }
}
