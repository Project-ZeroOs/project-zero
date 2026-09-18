//! Project Zero - Stage 3L Two-Tier DMA Architecture & Physical Frame Isolation
//!
//! Authoritative Contract: Stage 3L Architecture Rev3 (Approved & Frozen).
//!
//! Invariants:
//! - I-DEV-DMA-1: A physical frame cannot transition to Free until all DMA owners release it.
//! - I-DEV-DMA-2: A physical frame cannot become Free while any active DmaBufferDescriptor retains ownership.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use crate::dev::types::*;
use crate::mm::pmm::{PhysicalMemoryManager, PhysFrame, PAGE_SIZE};

pub static mut DMA_BUFFER_TABLE: [DmaBufferDescriptor; MAX_DMA_BUFFERS] =
    [DmaBufferDescriptor::empty(); MAX_DMA_BUFFERS];

pub static mut PHYSICAL_FRAME_PIN_TABLE: [PhysicalFramePinRecord; MAX_PINNED_DMA_FRAMES] =
    [PhysicalFramePinRecord::empty(); MAX_PINNED_DMA_FRAMES];

static DMA_LOCK: AtomicBool = AtomicBool::new(false);
static NEXT_BUFFER_ID: AtomicU32 = AtomicU32::new(1);

#[inline(always)]
fn acquire_dma_lock() {
    while DMA_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn release_dma_lock() {
    DMA_LOCK.store(false, Ordering::Release);
}

/// Initializes DMA tables.
pub fn init() {
    acquire_dma_lock();
    unsafe {
        for buf in DMA_BUFFER_TABLE.iter_mut() {
            *buf = DmaBufferDescriptor::empty();
        }
        for pin in PHYSICAL_FRAME_PIN_TABLE.iter_mut() {
            *pin = PhysicalFramePinRecord::empty();
        }
    }
    release_dma_lock();
}

/// Allocates a multi-frame logical DMA buffer with transactional rollback on failure (I-DEV-DMA-2).
pub fn alloc_dma_buffer(
    device_id: DeviceId,
    owning_pid: u64,
    frame_count: u16,
    sharing: ResourceSharing,
    pmm: &mut PhysicalMemoryManager,
) -> Result<(u32, u64), DeviceError> {
    if frame_count == 0 || (frame_count as usize) > MAX_FRAMES_PER_BUFFER {
        return Err(DeviceError::InvalidParameter);
    }

    acquire_dma_lock();

    // 1. Locate free DmaBufferDescriptor slot
    let mut buf_slot_idx: Option<usize> = None;
    unsafe {
        for (idx, buf) in DMA_BUFFER_TABLE.iter().enumerate() {
            if !buf.occupied {
                buf_slot_idx = Some(idx);
                break;
            }
        }
    }

    let b_idx = match buf_slot_idx {
        Some(idx) => idx,
        None => {
            release_dma_lock();
            return Err(DeviceError::CapacityExceeded);
        }
    };

    // 2. Allocate frames from PMM with transactional tracking
    let mut allocated_frames: [PhysFrame; MAX_FRAMES_PER_BUFFER] = [PhysFrame(0); MAX_FRAMES_PER_BUFFER];
    let mut pin_indices: [u16; MAX_FRAMES_PER_BUFFER] = [0xFFFF; MAX_FRAMES_PER_BUFFER];

    for i in 0..(frame_count as usize) {
        let frame = match pmm.alloc_frame() {
            Some(f) => f,
            None => {
                // Transactional Rollback: Free all previously allocated frames
                for j in 0..i {
                    let pin_idx = pin_indices[j] as usize;
                    unsafe {
                        PHYSICAL_FRAME_PIN_TABLE[pin_idx] = PhysicalFramePinRecord::empty();
                    }
                    let _ = pmm.free_frame(allocated_frames[j]);
                }
                release_dma_lock();
                return Err(DeviceError::OutOfMemory);
            }
        };

        // Find pin slot in PHYSICAL_FRAME_PIN_TABLE
        let mut pin_slot: Option<usize> = None;
        unsafe {
            for (p_idx, pin) in PHYSICAL_FRAME_PIN_TABLE.iter().enumerate() {
                if !pin.occupied {
                    pin_slot = Some(p_idx);
                    break;
                }
            }
        }

        let p_idx = match pin_slot {
            Some(idx) => idx,
            None => {
                // Rollback current frame + previous frames
                let _ = pmm.free_frame(frame);
                for j in 0..i {
                    let prev_p = pin_indices[j] as usize;
                    unsafe {
                        PHYSICAL_FRAME_PIN_TABLE[prev_p] = PhysicalFramePinRecord::empty();
                    }
                    let _ = pmm.free_frame(allocated_frames[j]);
                }
                release_dma_lock();
                return Err(DeviceError::CapacityExceeded);
            }
        };

        unsafe {
            PHYSICAL_FRAME_PIN_TABLE[p_idx].occupied = true;
            PHYSICAL_FRAME_PIN_TABLE[p_idx].phys_addr = frame.address();
            PHYSICAL_FRAME_PIN_TABLE[p_idx].pin_count = 1;
            PHYSICAL_FRAME_PIN_TABLE[p_idx].owning_buffer_mask = 1u32 << (b_idx as u32);
        }

        allocated_frames[i] = frame;
        pin_indices[i] = p_idx as u16;
    }

    let buffer_id = NEXT_BUFFER_ID.fetch_add(1, Ordering::SeqCst);
    let phys_base = allocated_frames[0].address();

    unsafe {
        let desc = &mut DMA_BUFFER_TABLE[b_idx];
        desc.occupied = true;
        desc.buffer_id = buffer_id;
        desc.frame_count = frame_count;
        desc.sharing = sharing;
        desc.device_id = device_id;
        desc.owning_pid = owning_pid;
        desc.phys_base = phys_base;
        desc.user_virt_addr = 0; // Configured upon VMM mapping
        desc.frame_indices = pin_indices;
    }

    release_dma_lock();
    Ok((buffer_id, phys_base))
}

/// Frees a logical DMA buffer and unpins its constituent physical frames (I-DEV-DMA-1).
pub fn free_dma_buffer(buffer_id: u32, pmm: &mut PhysicalMemoryManager) -> Result<(), DeviceError> {
    acquire_dma_lock();
    unsafe {
        for desc in DMA_BUFFER_TABLE.iter_mut() {
            if desc.occupied && desc.buffer_id == buffer_id {
                let count = desc.frame_count as usize;
                for i in 0..count {
                    let pin_idx = desc.frame_indices[i] as usize;
                    if pin_idx < MAX_PINNED_DMA_FRAMES {
                        let pin = &mut PHYSICAL_FRAME_PIN_TABLE[pin_idx];
                        if pin.occupied {
                            pin.pin_count = pin.pin_count.saturating_sub(1);
                            if pin.pin_count == 0 {
                                let phys = PhysFrame(pin.phys_addr);
                                *pin = PhysicalFramePinRecord::empty();
                                let _ = pmm.free_frame(phys);
                            }
                        }
                    }
                }
                *desc = DmaBufferDescriptor::empty();
                release_dma_lock();
                return Ok(());
            }
        }
    }
    release_dma_lock();
    Err(DeviceError::DeviceNotFound)
}

/// Checks if a physical frame address is currently pinned by any DMA buffer.
pub fn is_frame_pinned(phys_addr: u64) -> bool {
    acquire_dma_lock();
    unsafe {
        for pin in PHYSICAL_FRAME_PIN_TABLE.iter() {
            if pin.occupied && pin.phys_addr == phys_addr && pin.pin_count > 0 {
                release_dma_lock();
                return true;
            }
        }
    }
    release_dma_lock();
    false
}

/// Cleans up all DMA buffers owned by a terminating process.
pub fn teardown_process_dma(pid: u64, pmm: &mut PhysicalMemoryManager) -> usize {
    let mut cleaned = 0;
    acquire_dma_lock();
    unsafe {
        for desc in DMA_BUFFER_TABLE.iter_mut() {
            if desc.occupied && desc.owning_pid == pid {
                let count = desc.frame_count as usize;
                for i in 0..count {
                    let pin_idx = desc.frame_indices[i] as usize;
                    if pin_idx < MAX_PINNED_DMA_FRAMES {
                        let pin = &mut PHYSICAL_FRAME_PIN_TABLE[pin_idx];
                        if pin.occupied {
                            pin.pin_count = pin.pin_count.saturating_sub(1);
                            if pin.pin_count == 0 {
                                let phys = PhysFrame(pin.phys_addr);
                                *pin = PhysicalFramePinRecord::empty();
                                let _ = pmm.free_frame(phys);
                            }
                        }
                    }
                }
                *desc = DmaBufferDescriptor::empty();
                cleaned += 1;
            }
        }
    }
    release_dma_lock();
    cleaned
}

/// Returns the count of occupied DMA buffer descriptors.
pub fn get_active_buffer_count() -> usize {
    acquire_dma_lock();
    let mut count = 0;
    unsafe {
        for desc in DMA_BUFFER_TABLE.iter() {
            if desc.occupied {
                count += 1;
            }
        }
    }
    release_dma_lock();
    count
}

/// Returns the count of currently pinned physical frames.
pub fn get_pinned_frame_count() -> usize {
    acquire_dma_lock();
    let mut count = 0;
    unsafe {
        for pin in PHYSICAL_FRAME_PIN_TABLE.iter() {
            if pin.occupied && pin.pin_count > 0 {
                count += 1;
            }
        }
    }
    release_dma_lock();
    count
}
