//! Project Zero - Stage 3M Packet Buffer Pool & DMA Frame Slicing
//!
//! Authoritative Invariants:
//! - I-NET-DMA-2: A physical frame remains pinned while either slice is active.
//! - I-NET-BUF-1: Every packet slice belongs to exactly one ownership domain.
//! - I-NET-DMA-1: Network DMA ownership is governed exclusively via Stage 3L DMA tracker.

use core::sync::atomic::{AtomicBool, Ordering};
use crate::net::types::*;

pub static mut PACKET_BUFFER_TABLE: [PacketBufferSlot; MAX_PACKET_BUFFERS] =
    [PacketBufferSlot::empty(); MAX_PACKET_BUFFERS];

pub static mut DMA_BACKING_BUFFER_ID: u32 = 0;
pub static mut DMA_BACKING_BASE_PHYS: u64 = 0;

static BUF_LOCK: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn acquire_buf_lock() {
    while BUF_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn release_buf_lock() {
    BUF_LOCK.store(false, Ordering::Release);
}

/// Initializes the 32 packet buffers from 16 physical frames allocated via Stage 3L alloc_dma_buffer.
pub fn init_packet_buffer_pool(dma_buf_id: u32, base_phys: u64) {
    acquire_buf_lock();
    unsafe {
        DMA_BACKING_BUFFER_ID = dma_buf_id;
        DMA_BACKING_BASE_PHYS = base_phys;

        for k in 0..16 {
            let frame_phys = base_phys + (k as u64) * 4096;
            // Slice 0: buffer 2k (offset 0..2048)
            let idx0 = 2 * k;
            PACKET_BUFFER_TABLE[idx0] = PacketBufferSlot::empty();
            PACKET_BUFFER_TABLE[idx0].buffer_id = (1 << 16) | (idx0 as u32);
            PACKET_BUFFER_TABLE[idx0].dma_buffer_id = dma_buf_id;
            PACKET_BUFFER_TABLE[idx0].phys_addr = frame_phys;
            PACKET_BUFFER_TABLE[idx0].state = PacketBufferState::Free;

            // Slice 1: buffer 2k + 1 (offset 2048..4096)
            let idx1 = 2 * k + 1;
            PACKET_BUFFER_TABLE[idx1] = PacketBufferSlot::empty();
            PACKET_BUFFER_TABLE[idx1].buffer_id = (1 << 16) | (idx1 as u32);
            PACKET_BUFFER_TABLE[idx1].dma_buffer_id = dma_buf_id;
            PACKET_BUFFER_TABLE[idx1].phys_addr = frame_phys + (PACKET_BUFFER_SIZE as u64);
            PACKET_BUFFER_TABLE[idx1].state = PacketBufferState::Free;
        }
    }
    release_buf_lock();
}

/// Allocates an unused packet buffer slice.
pub fn alloc_packet_buffer(state: PacketBufferState) -> Option<usize> {
    acquire_buf_lock();
    unsafe {
        for (idx, slot) in PACKET_BUFFER_TABLE.iter_mut().enumerate() {
            if slot.state == PacketBufferState::Free {
                slot.state = state;
                slot.data_offset = PACKET_HEADROOM as u16;
                slot.data_len = 0;
                slot.next_buffer_idx = 0xFFFF;
                release_buf_lock();
                return Some(idx);
            }
        }
    }
    release_buf_lock();
    None
}

/// Recycles a packet buffer slice back to FreePool.
pub fn free_packet_buffer(slot_idx: usize) -> Result<(), ()> {
    if slot_idx >= MAX_PACKET_BUFFERS {
        return Err(());
    }
    acquire_buf_lock();
    unsafe {
        let slot = &mut PACKET_BUFFER_TABLE[slot_idx];
        slot.state = PacketBufferState::Free;
        slot.data_offset = PACKET_HEADROOM as u16;
        slot.data_len = 0;
        slot.next_buffer_idx = 0xFFFF;
        // Bump generation in upper 16 bits
        let gen = (slot.buffer_id >> 16).wrapping_add(1);
        slot.buffer_id = (gen << 16) | (slot_idx as u32);
    }
    release_buf_lock();
    Ok(())
}

/// Checks whether any slice in physical frame k (0..15) is active (I-NET-DMA-2).
pub fn is_frame_slice_active(frame_k: usize) -> bool {
    if frame_k >= 16 {
        return false;
    }
    acquire_buf_lock();
    let res = unsafe {
        let s0 = PACKET_BUFFER_TABLE[2 * frame_k].state != PacketBufferState::Free;
        let s1 = PACKET_BUFFER_TABLE[2 * frame_k + 1].state != PacketBufferState::Free;
        s0 || s1
    };
    release_buf_lock();
    res
}

/// Returns the number of currently allocated (non-Free) packet buffers.
pub fn get_allocated_buffer_count() -> usize {
    acquire_buf_lock();
    let mut count = 0;
    unsafe {
        for slot in PACKET_BUFFER_TABLE.iter() {
            if slot.state != PacketBufferState::Free {
                count += 1;
            }
        }
    }
    release_buf_lock();
    count
}

/// Returns an immutable slice to the packet buffer payload in HHDM virtual memory.
pub fn get_packet_data(slot_idx: usize) -> Option<&'static [u8]> {
    if slot_idx >= MAX_PACKET_BUFFERS {
        return None;
    }
    unsafe {
        let slot = &PACKET_BUFFER_TABLE[slot_idx];
        if slot.state == PacketBufferState::Free || slot.phys_addr == 0 {
            return None;
        }
        let ptr = (crate::mm::vmm::HHDM_BASE + slot.phys_addr + slot.data_offset as u64) as *const u8;
        Some(core::slice::from_raw_parts(ptr, slot.data_len as usize))
    }
}

/// Returns a mutable slice to the packet buffer payload in HHDM virtual memory.
pub fn get_packet_data_mut(slot_idx: usize) -> Option<&'static mut [u8]> {
    if slot_idx >= MAX_PACKET_BUFFERS {
        return None;
    }
    unsafe {
        let slot = &mut PACKET_BUFFER_TABLE[slot_idx];
        if slot.state == PacketBufferState::Free || slot.phys_addr == 0 {
            return None;
        }
        let ptr = (crate::mm::vmm::HHDM_BASE + slot.phys_addr + slot.data_offset as u64) as *mut u8;
        Some(core::slice::from_raw_parts_mut(ptr, slot.data_len as usize))
    }
}

/// Sets packet buffer payload data from a slice.
pub fn set_packet_payload(slot_idx: usize, data: &[u8]) -> Result<(), ()> {
    if slot_idx >= MAX_PACKET_BUFFERS || data.len() > (PACKET_BUFFER_SIZE - PACKET_HEADROOM) {
        return Err(());
    }
    unsafe {
        let slot = &mut PACKET_BUFFER_TABLE[slot_idx];
        let ptr = (crate::mm::vmm::HHDM_BASE + slot.phys_addr + slot.data_offset as u64) as *mut u8;
        core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        slot.data_len = data.len() as u16;
    }
    Ok(())
}
