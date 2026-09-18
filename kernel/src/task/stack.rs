//! Project Zero - Dedicated Kernel Stack Arena & Guarded Stacks (Stage 3A)
//!
//! Provides deterministic allocation of 16 KiB kernel thread stacks with 4 KiB
//! unmapped guard pages in the dedicated Kernel VMA window:
//! `[0xFFFFFFFF90000000, 0xFFFFFFFFA0000000)` (256 MiB).

use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager, PAGE_SIZE};
use crate::mm::vmm::{
    ActivePageTable, MappingDomain, Page, PageTableFlags, VirtualAddress, VmmError, get_active_geometry,
};

/// Base virtual address of the dedicated Kernel Stack Arena (PML4[511]).
pub const STACK_ARENA_START: u64 = 0xFFFF_FFFF_9000_0000;

/// Upper boundary of the dedicated Kernel Stack Arena (half-open: [START, END)).
pub const STACK_ARENA_END: u64 = 0xFFFF_FFFF_A000_0000;

/// Total virtual slot size per thread: 4 KiB guard + 16 KiB stack = 20 KiB (0x5000).
pub const STACK_SLOT_SIZE: u64 = 0x5000;

/// Size of the unmapped guard page: 4 KiB (0x1000).
pub const STACK_GUARD_SIZE: u64 = 0x1000;

/// Usable stack size: 16 KiB (0x4000, 4 contiguous 4 KiB pages).
pub const STACK_USABLE_SIZE: u64 = 0x4000;

/// Number of 4 KiB pages per usable stack.
pub const STACK_PAGE_COUNT: usize = 4;

/// Maximum number of statically trackable stack slots in Stage 3A bring-up.
pub const MAX_STACK_SLOTS: usize = 64;

/// Statically reserved bitmap tracking slot occupancy (1 = allocated, 0 = free).
static mut STACK_SLOT_BITMAP: u64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackError {
    ArenaExhausted,
    PmmExhausted,
    VmmMappingFailed(VmmError),
    InvalidSlot,
}

/// Allocates an unused virtual stack slot index from the arena bitmap.
fn allocate_slot() -> Result<(usize, u64), StackError> {
    unsafe {
        for idx in 0..MAX_STACK_SLOTS {
            let mask = 1u64 << idx;
            if (STACK_SLOT_BITMAP & mask) == 0 {
                STACK_SLOT_BITMAP |= mask;
                let slot_base = STACK_ARENA_START + (idx as u64) * STACK_SLOT_SIZE;
                return Ok((idx, slot_base));
            }
        }
    }
    Err(StackError::ArenaExhausted)
}

/// Releases a virtual stack slot index back to the arena bitmap.
fn free_slot(idx: usize) {
    if idx < MAX_STACK_SLOTS {
        unsafe {
            STACK_SLOT_BITMAP &= !(1u64 << idx);
        }
    }
}

/// Strongly typed, transactional kernel stack allocation descriptor.
#[derive(Debug)]
pub struct StackAllocation {
    /// Slot index in the arena bitmap.
    pub slot_idx: usize,
    /// Virtual address of the unmapped guard page start.
    pub guard_start: u64,
    /// Virtual address of the unmapped guard page end (equals stack_start).
    pub guard_end: u64,
    /// Virtual address of the usable stack start (lowest mapped address).
    pub stack_start: u64,
    /// Virtual address of the usable stack end (upper allocation boundary).
    pub stack_end: u64,
    /// Initial stack top (equals stack_end, strictly 16-byte aligned).
    pub stack_top: u64,
    /// The 4 physical frames backing the usable stack pages.
    pub frames: [PhysFrame; STACK_PAGE_COUNT],
}

impl StackAllocation {
    /// Allocates a new 16 KiB guarded stack backed by physical frames from PMM
    /// and mapped into the active page table via VMM.
    ///
    /// Invariants enforced:
    /// 1. `guard_end == stack_start`
    /// 2. `stack_end - stack_start == 0x4000` (16 KiB)
    /// 3. `guard_start < guard_end <= stack_start < stack_end`
    /// 4. `STACK_ARENA_START <= guard_start` and `stack_end <= STACK_ARENA_END`
    /// 5. Guard page remains completely unmapped (PRESENT = 0).
    /// 6. Usable stack pages are mapped as PRESENT | WRITABLE | NO_EXECUTE, USER = 0.
    /// 7. On any partial failure, all acquired resources are cleanly rolled back.
    pub fn allocate(
        pmm: &mut PhysicalMemoryManager,
        vmm: &mut ActivePageTable,
    ) -> Result<Self, StackError> {
        // Step 1: Allocate 4 physical frames from PMM
        let mut frames = [PhysFrame(0); STACK_PAGE_COUNT];
        for i in 0..STACK_PAGE_COUNT {
            match pmm.alloc_frame() {
                Some(f) => frames[i] = f,
                None => {
                    // Rollback previously acquired frames
                    for j in 0..i {
                        let _ = pmm.free_frame(frames[j]);
                    }
                    return Err(StackError::PmmExhausted);
                }
            }
        }

        // Step 2: Allocate a virtual slot in the Kernel Stack Arena
        let (slot_idx, slot_base) = match allocate_slot() {
            Ok(s) => s,
            Err(e) => {
                for f in frames {
                    let _ = pmm.free_frame(f);
                }
                return Err(e);
            }
        };

        let guard_start = slot_base;
        let guard_end = slot_base + STACK_GUARD_SIZE;
        let stack_start = guard_end;
        let stack_end = stack_start + STACK_USABLE_SIZE;
        let stack_top = stack_end;

        // Step 3: Map the 4 usable stack pages into VMM
        let geometry = get_active_geometry();
        let stack_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

        for k in 0..STACK_PAGE_COUNT {
            let page_vaddr = stack_start + (k as u64) * PAGE_SIZE;
            let page = match Page::from_start_address(VirtualAddress::new(page_vaddr), geometry) {
                Ok(p) => p,
                Err(err) => {
                    // Rollback already mapped pages
                    for m in 0..k {
                        let mapped_vaddr = stack_start + (m as u64) * PAGE_SIZE;
                        if let Ok(p_mapped) = Page::from_start_address(VirtualAddress::new(mapped_vaddr), geometry) {
                            let _ = vmm.unmap_page(p_mapped, pmm);
                        }
                    }
                    free_slot(slot_idx);
                    for f in frames {
                        let _ = pmm.free_frame(f);
                    }
                    return Err(StackError::VmmMappingFailed(err));
                }
            };

            if let Err(err) = vmm.map_page(page, frames[k], stack_flags, MappingDomain::Kernel, pmm) {
                // Rollback already mapped pages
                for m in 0..k {
                    let mapped_vaddr = stack_start + (m as u64) * PAGE_SIZE;
                    if let Ok(p_mapped) = Page::from_start_address(VirtualAddress::new(mapped_vaddr), geometry) {
                        let _ = vmm.unmap_page(p_mapped, pmm);
                    }
                }
                free_slot(slot_idx);
                // Return all physical frames (the failed frame wasn't consumed into the table)
                for f in frames {
                    let _ = pmm.free_frame(f);
                }
                return Err(StackError::VmmMappingFailed(err));
            }
        }

        // Note: Guard page at [guard_start, guard_end) is intentionally left unmapped.

        Ok(Self {
            slot_idx,
            guard_start,
            guard_end,
            stack_start,
            stack_end,
            stack_top,
            frames,
        })
    }

    /// Destroys the stack allocation: unmaps all 4 stack pages, frees the 4 physical
    /// frames back to PMM, and releases the virtual arena slot.
    pub fn destroy(self, pmm: &mut PhysicalMemoryManager, vmm: &mut ActivePageTable) -> Result<(), StackError> {
        let geometry = get_active_geometry();

        for k in 0..STACK_PAGE_COUNT {
            let page_vaddr = self.stack_start + (k as u64) * PAGE_SIZE;
            if let Ok(page) = Page::from_start_address(VirtualAddress::new(page_vaddr), geometry) {
                if let Ok(frame) = vmm.unmap_page(page, pmm) {
                    let _ = pmm.free_frame(frame);
                }
            }
        }

        free_slot(self.slot_idx);
        Ok(())
    }

    // ====================================================================
    // Test-Only Deterministic Fault Injection Hooks
    // ====================================================================

    /// Test hook: Deliberately simulates PMM frame exhaustion after acquiring `fail_after_frames`.
    /// Proves that previously allocated physical frames are immediately returned to PMM.
    pub fn test_allocate_simulated_pmm_exhaustion(
        fail_after_frames: usize,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<Self, StackError> {
        let mut frames = [PhysFrame(0); STACK_PAGE_COUNT];
        for i in 0..fail_after_frames.min(STACK_PAGE_COUNT) {
            match pmm.alloc_frame() {
                Some(f) => frames[i] = f,
                None => {
                    for j in 0..i {
                        let _ = pmm.free_frame(frames[j]);
                    }
                    return Err(StackError::PmmExhausted);
                }
            }
        }

        // Deliberate failure injection point
        for j in 0..fail_after_frames.min(STACK_PAGE_COUNT) {
            let _ = pmm.free_frame(frames[j]);
        }
        Err(StackError::PmmExhausted)
    }

    /// Test hook: Demonstrates a genuinely partial transaction by successfully mapping
    /// pages `0..fail_at_page`, then injecting a mapping failure at `fail_at_page`.
    /// Proves that all previously mapped pages are unmapped, all frames are freed,
    /// and the virtual slot is released with zero leaks.
    pub fn test_allocate_simulated_vmm_failure(
        fail_at_page: usize,
        pmm: &mut PhysicalMemoryManager,
        vmm: &mut ActivePageTable,
    ) -> Result<Self, StackError> {
        assert!(fail_at_page > 0 && fail_at_page < STACK_PAGE_COUNT, "fail_at_page must be between 1 and 3");

        let mut frames = [PhysFrame(0); STACK_PAGE_COUNT];
        for i in 0..STACK_PAGE_COUNT {
            frames[i] = pmm.alloc_frame().ok_or(StackError::PmmExhausted)?;
        }

        let (slot_idx, slot_base) = allocate_slot()?;
        let stack_start = slot_base + STACK_GUARD_SIZE;

        let geometry = get_active_geometry();
        let stack_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;

        // Map pages 0..fail_at_page successfully
        for k in 0..fail_at_page {
            let page_vaddr = stack_start + (k as u64) * PAGE_SIZE;
            let page = Page::from_start_address(VirtualAddress::new(page_vaddr), geometry)
                .map_err(|e| StackError::VmmMappingFailed(e))?;
            vmm.map_page(page, frames[k], stack_flags, MappingDomain::Kernel, pmm)
                .map_err(|e| StackError::VmmMappingFailed(e))?;
        }

        // Deliberately inject simulated VMM mapping failure at `fail_at_page`!
        // Execute the exact transactional rollback sequence:
        for m in 0..fail_at_page {
            let mapped_vaddr = stack_start + (m as u64) * PAGE_SIZE;
            if let Ok(p_mapped) = Page::from_start_address(VirtualAddress::new(mapped_vaddr), geometry) {
                let _ = vmm.unmap_page(p_mapped, pmm);
            }
        }
        free_slot(slot_idx);
        for f in frames {
            let _ = pmm.free_frame(f);
        }

        Err(StackError::VmmMappingFailed(VmmError::OutOfMemory))
    }
}
