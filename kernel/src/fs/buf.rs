//! Project Zero - Stage 3K Bounded Block Buffer Cache
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).

use core::sync::atomic::{AtomicBool, Ordering};
use crate::fs::types::{BLOCK_SIZE, FsError};
use crate::fs::dev::{BLOCK_DEVICE_LOCK, get_device};

pub const MAX_BUFFERS: usize = 4;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferState {
    Free = 0,
    Clean = 1,
    Dirty = 2,
    Pinned = 3,
    Writeback = 4,
}

pub struct BlockBuffer {
    pub device_id: u8,
    pub block_idx: u64,
    pub state: BufferState,
    pub pin_count: u16,
    pub lru_seq: u64,
    pub data: [u8; BLOCK_SIZE],
}

impl BlockBuffer {
    pub const fn empty() -> Self {
        Self {
            device_id: 0,
            block_idx: 0,
            state: BufferState::Free,
            pin_count: 0,
            lru_seq: 0,
            data: [0; BLOCK_SIZE],
        }
    }
}

pub struct BlockCacheLock {
    lock: AtomicBool,
}

impl BlockCacheLock {
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

pub static BLOCK_CACHE_LOCK: BlockCacheLock = BlockCacheLock::new();
static mut BUFFERS: [BlockBuffer; MAX_BUFFERS] = [
    BlockBuffer::empty(), BlockBuffer::empty(), BlockBuffer::empty(), BlockBuffer::empty(),
];
static mut NEXT_LRU_SEQ: u64 = 1;

pub fn init_cache() {
    BLOCK_CACHE_LOCK.acquire();
    unsafe {
        for buf in BUFFERS.iter_mut() {
            *buf = BlockBuffer::empty();
        }
        NEXT_LRU_SEQ = 1;
    }
    BLOCK_CACHE_LOCK.release();
}

/// Helper to evict or find an available buffer under BLOCK_CACHE_LOCK.
/// If a dirty buffer is evicted, acquires BLOCK_DEVICE_LOCK (Rank 4) to flush it.
unsafe fn allocate_slot(device_id: u8, block_idx: u64) -> Result<usize, FsError> {
    // 1. Check if already cached
    for (i, buf) in BUFFERS.iter_mut().enumerate() {
        if buf.state != BufferState::Free && buf.device_id == device_id && buf.block_idx == block_idx {
            NEXT_LRU_SEQ = NEXT_LRU_SEQ.saturating_add(1);
            buf.lru_seq = NEXT_LRU_SEQ;
            return Ok(i);
        }
    }

    // 2. Check for free slot
    for (i, buf) in BUFFERS.iter_mut().enumerate() {
        if buf.state == BufferState::Free {
            NEXT_LRU_SEQ = NEXT_LRU_SEQ.saturating_add(1);
            buf.device_id = device_id;
            buf.block_idx = block_idx;
            buf.state = BufferState::Clean;
            buf.pin_count = 0;
            buf.lru_seq = NEXT_LRU_SEQ;
            return Ok(i);
        }
    }

    // 3. LRU Eviction: find unpinned buffer with lowest lru_seq
    let mut victim: Option<usize> = None;
    let mut min_lru: u64 = u64::MAX;

    for (i, buf) in BUFFERS.iter().enumerate() {
        if buf.pin_count == 0 && buf.state != BufferState::Pinned && buf.state != BufferState::Writeback {
            if buf.lru_seq < min_lru {
                min_lru = buf.lru_seq;
                victim = Some(i);
            }
        }
    }

    let victim_idx = victim.ok_or(FsError::Busy)?;
    let buf = &mut BUFFERS[victim_idx];

    // If dirty, flush to block device under BLOCK_DEVICE_LOCK
    if buf.state == BufferState::Dirty {
        buf.state = BufferState::Writeback;
        BLOCK_DEVICE_LOCK.acquire();
        let dev = get_device(buf.device_id);
        let flush_res = match dev {
            Ok(d) => d.write_block(buf.block_idx, &buf.data),
            Err(e) => Err(e),
        };
        BLOCK_DEVICE_LOCK.release();
        flush_res?;
    }

    NEXT_LRU_SEQ = NEXT_LRU_SEQ.saturating_add(1);
    buf.device_id = device_id;
    buf.block_idx = block_idx;
    buf.state = BufferState::Clean;
    buf.pin_count = 0;
    buf.lru_seq = NEXT_LRU_SEQ;
    Ok(victim_idx)
}

/// Read block from cache, fetching from device if not present.
pub fn read_block_cached(device_id: u8, block_idx: u64, out: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
    BLOCK_CACHE_LOCK.acquire();
    unsafe {
        // Check if already in cache
        let mut found = None;
        for buf in BUFFERS.iter_mut() {
            if buf.state != BufferState::Free && buf.device_id == device_id && buf.block_idx == block_idx {
                NEXT_LRU_SEQ = NEXT_LRU_SEQ.saturating_add(1);
                buf.lru_seq = NEXT_LRU_SEQ;
                out.copy_from_slice(&buf.data);
                found = Some(());
                break;
            }
        }

        if found.is_some() {
            BLOCK_CACHE_LOCK.release();
            return Ok(());
        }

        // Need to load from device into an allocated slot
        let slot = match allocate_slot(device_id, block_idx) {
            Ok(s) => s,
            Err(e) => {
                BLOCK_CACHE_LOCK.release();
                return Err(e);
            }
        };

        let buf = &mut BUFFERS[slot];
        buf.state = BufferState::Pinned;
        buf.pin_count = 1;

        BLOCK_DEVICE_LOCK.acquire();
        let dev = get_device(device_id);
        let read_res = match dev {
            Ok(d) => d.read_block(block_idx, &mut buf.data),
            Err(e) => Err(e),
        };
        BLOCK_DEVICE_LOCK.release();

        if let Err(e) = read_res {
            buf.state = BufferState::Free;
            buf.pin_count = 0;
            BLOCK_CACHE_LOCK.release();
            return Err(e);
        }

        out.copy_from_slice(&buf.data);
        buf.state = BufferState::Clean;
        buf.pin_count = 0;
    }
    BLOCK_CACHE_LOCK.release();
    Ok(())
}

/// Write data into block cache, marking buffer Dirty.
pub fn write_block_cached(device_id: u8, block_idx: u64, data: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
    BLOCK_CACHE_LOCK.acquire();
    unsafe {
        let slot = match allocate_slot(device_id, block_idx) {
            Ok(s) => s,
            Err(e) => {
                BLOCK_CACHE_LOCK.release();
                return Err(e);
            }
        };

        let buf = &mut BUFFERS[slot];
        buf.data.copy_from_slice(data);
        buf.state = BufferState::Dirty;
    }
    BLOCK_CACHE_LOCK.release();
    Ok(())
}

/// Flush a specific cached block to disk if present and dirty.
pub fn sync_block(device_id: u8, block_idx: u64) -> Result<(), FsError> {
    BLOCK_CACHE_LOCK.acquire();
    unsafe {
        for buf in BUFFERS.iter_mut() {
            if buf.state == BufferState::Dirty && buf.device_id == device_id && buf.block_idx == block_idx {
                buf.state = BufferState::Writeback;
                BLOCK_DEVICE_LOCK.acquire();
                let dev = get_device(device_id);
                let res = match dev {
                    Ok(d) => {
                        let w = d.write_block(block_idx, &buf.data);
                        let f = d.flush();
                        w.and(f)
                    }
                    Err(e) => Err(e),
                };
                BLOCK_DEVICE_LOCK.release();
                buf.state = BufferState::Clean;
                BLOCK_CACHE_LOCK.release();
                return res;
            }
        }
    }
    BLOCK_CACHE_LOCK.release();
    Ok(())
}

/// Flush all dirty buffers for the specified device.
pub fn sync_all_buffers(device_id: u8) -> Result<(), FsError> {
    BLOCK_CACHE_LOCK.acquire();
    unsafe {
        for buf in BUFFERS.iter_mut() {
            if buf.state == BufferState::Dirty && (buf.device_id == device_id || device_id == 0xFF) {
                buf.state = BufferState::Writeback;
                BLOCK_DEVICE_LOCK.acquire();
                let dev = get_device(buf.device_id);
                let res = match dev {
                    Ok(d) => d.write_block(buf.block_idx, &buf.data),
                    Err(e) => Err(e),
                };
                BLOCK_DEVICE_LOCK.release();
                buf.state = BufferState::Clean;
                if let Err(e) = res {
                    BLOCK_CACHE_LOCK.release();
                    return Err(e);
                }
            }
        }

        // Flush hardware cache
        BLOCK_DEVICE_LOCK.acquire();
        let dev = get_device(device_id);
        let f_res = match dev {
            Ok(d) => d.flush(),
            Err(_) => Ok(()),
        };
        BLOCK_DEVICE_LOCK.release();
        f_res?;
    }
    BLOCK_CACHE_LOCK.release();
    Ok(())
}
