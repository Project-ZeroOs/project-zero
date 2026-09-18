//! Project Zero - Stage 3J Process Memory Map Companion Table
//!
//! Authoritative Contract: Stage 3J Architecture Specification Rev2 (Approved & Frozen).
//! Invariants: I-ELF-MEMMAP-1 through I-ELF-MEMMAP-6 and I-ELF-MEM-OWNERSHIP-1.

use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager};
use crate::mm::vmm::{ActivePageTable, Page};
use crate::task::process::MAX_PROCESSES;
use super::types::ElfError;

pub const MAX_PROCESS_MAPPED_PAGES: usize = 64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapState {
    Free = 0,
    Allocating = 1,
    Active = 2,
    Reclaiming = 3,
}

pub struct ProcessMemoryMap {
    pub state: MemoryMapState,
    pub page_count: usize,
    pub pages: [(Page, PhysFrame); MAX_PROCESS_MAPPED_PAGES],
}

impl ProcessMemoryMap {
    pub const fn empty() -> Self {
        Self {
            state: MemoryMapState::Free,
            page_count: 0,
            pages: [(Page::from_address(crate::mm::vmm::VirtualAddress::new(0)), PhysFrame(0)); MAX_PROCESS_MAPPED_PAGES],
        }
    }
}

pub static mut PROCESS_MEMORY_MAPS: [ProcessMemoryMap; MAX_PROCESSES] =
    [const { ProcessMemoryMap::empty() }; MAX_PROCESSES];

/// Begins transactional allocation for a given process slot.
pub fn begin_allocation(slot_idx: usize) -> Result<(), ElfError> {
    if slot_idx >= MAX_PROCESSES {
        return Err(ElfError::ProcessCreationFailed);
    }
    unsafe {
        let mmap = &mut PROCESS_MEMORY_MAPS[slot_idx];
        if mmap.state != MemoryMapState::Free {
            return Err(ElfError::CapacityExceeded);
        }
        mmap.state = MemoryMapState::Allocating;
        mmap.page_count = 0;
        for entry in mmap.pages.iter_mut() {
            *entry = (Page::from_address(crate::mm::vmm::VirtualAddress::new(0)), PhysFrame(0));
        }
    }
    Ok(())
}

/// Records a successfully mapped leaf page and physical frame during loading.
pub fn record_page(slot_idx: usize, page: Page, frame: PhysFrame) -> Result<(), ElfError> {
    if slot_idx >= MAX_PROCESSES {
        return Err(ElfError::ProcessCreationFailed);
    }
    unsafe {
        let mmap = &mut PROCESS_MEMORY_MAPS[slot_idx];
        if mmap.state != MemoryMapState::Allocating {
            return Err(ElfError::ProcessCreationFailed);
        }
        if mmap.page_count >= MAX_PROCESS_MAPPED_PAGES {
            return Err(ElfError::CapacityExceeded);
        }
        mmap.pages[mmap.page_count] = (page, frame);
        mmap.page_count += 1;
    }
    Ok(())
}

/// Commits the memory map from Allocating to Active state upon successful load.
pub fn commit_allocation(slot_idx: usize) -> Result<(), ElfError> {
    if slot_idx >= MAX_PROCESSES {
        return Err(ElfError::ProcessCreationFailed);
    }
    unsafe {
        let mmap = &mut PROCESS_MEMORY_MAPS[slot_idx];
        if mmap.state != MemoryMapState::Allocating {
            return Err(ElfError::ProcessCreationFailed);
        }
        mmap.state = MemoryMapState::Active;
    }
    Ok(())
}

/// Rolls back an in-progress load failure: unmaps and frees all recorded leaf frames.
pub fn rollback_allocation(
    slot_idx: usize,
    pmm: &mut PhysicalMemoryManager,
    apt: &mut ActivePageTable,
) {
    if slot_idx >= MAX_PROCESSES {
        return;
    }
    unsafe {
        let mmap = &mut PROCESS_MEMORY_MAPS[slot_idx];
        if mmap.state == MemoryMapState::Free {
            return;
        }
        mmap.state = MemoryMapState::Reclaiming;
        for i in 0..mmap.page_count {
            let (page, frame) = mmap.pages[i];
            if frame.address() != 0 {
                let _ = apt.unmap_page(page, pmm);
                let _ = pmm.free_frame(frame);
            }
        }
        mmap.page_count = 0;
        mmap.state = MemoryMapState::Free;
    }
}

/// Reclaims all leaf frames during process teardown under SCHEDULER.lock (Invariant I-ELF-MEM-OWNERSHIP-1).
pub unsafe fn reclaim_process_memory_map_locked(
    slot_idx: usize,
    pmm: &mut PhysicalMemoryManager,
    apt: &mut ActivePageTable,
) {
    if slot_idx >= MAX_PROCESSES {
        return;
    }
    let mmap = &mut PROCESS_MEMORY_MAPS[slot_idx];
    if mmap.state == MemoryMapState::Free {
        return;
    }
    mmap.state = MemoryMapState::Reclaiming;
    for i in 0..mmap.page_count {
        let (page, frame) = mmap.pages[i];
        if frame.address() != 0 {
            let _ = apt.unmap_page(page, pmm);
            let _ = pmm.free_frame(frame);
        }
    }
    mmap.page_count = 0;
    mmap.state = MemoryMapState::Free;
}
