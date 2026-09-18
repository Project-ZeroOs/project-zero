//! Project Zero - Stage 3L Dedicated MMIO Aperture & VMM Integration
//!
//! Authoritative Contract: Stage 3L Architecture Rev3 (Approved & Frozen).
//!
//! Invariants:
//! - I-DEV-MMIO-1: No ordinary user mapping, ELF mapping, stack, or SHM may overlap DEVICE_MMIO_REGION.
//! - I-DEV-MMIO-PIN-1: While mapping_refs > 0, the underlying DeviceSlot remains pinned.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::dev::types::*;
use crate::mm::pmm::{PhysicalMemoryManager, PhysFrame, PAGE_SIZE};
use crate::mm::vmm::{
    ActivePageTable, Page, PageTableFlags, MappingDomain, VirtualAddress, get_active_geometry,
};

pub const USER_DEV_MMIO_BASE: u64 = 0x0000_6000_0000_0000; // 1 TiB aperture base (PML4 entry 192)
pub const USER_DEV_MMIO_END: u64  = 0x0000_7000_0000_0000; // 1 TiB aperture end  (PML4 entry 224)
pub const MAX_MMIO_MAP_SIZE: u64  = 16 * 1024 * 1024;       // 16 MiB max per mapping

static NEXT_MMIO_VADDR: AtomicU64 = AtomicU64::new(USER_DEV_MMIO_BASE);

/// Validates that a requested MMIO range strictly obeys I-DEV-MMIO-1.
pub fn validate_mmio_isolation(vaddr: u64, size: u64) -> Result<(), DeviceError> {
    if size == 0 || (size % PAGE_SIZE != 0) || (vaddr % PAGE_SIZE != 0) {
        return Err(DeviceError::InvalidParameter);
    }
    let end = vaddr.checked_add(size).ok_or(DeviceError::InvalidParameter)?;
    if vaddr < USER_DEV_MMIO_BASE || end > USER_DEV_MMIO_END {
        return Err(DeviceError::InvalidParameter);
    }
    Ok(())
}

/// Allocates a virtual address window from DEVICE_MMIO_REGION.
pub fn allocate_mmio_virtual_window(size: u64) -> Result<u64, DeviceError> {
    if size == 0 || (size % PAGE_SIZE != 0) || size > MAX_MMIO_MAP_SIZE {
        return Err(DeviceError::InvalidParameter);
    }
    let vaddr = NEXT_MMIO_VADDR.fetch_add(size, Ordering::SeqCst);
    if vaddr.saturating_add(size) > USER_DEV_MMIO_END {
        return Err(DeviceError::CapacityExceeded);
    }
    Ok(vaddr)
}

/// Maps a validated physical MMIO range into the active address space with uncacheable attributes.
pub fn map_device_mmio(
    active_table: &mut ActivePageTable,
    pmm: &mut PhysicalMemoryManager,
    phys_base: u64,
    size: u64,
    writable: bool,
) -> Result<u64, DeviceError> {
    if (phys_base % PAGE_SIZE != 0) || (size % PAGE_SIZE != 0) || size == 0 || size > MAX_MMIO_MAP_SIZE {
        return Err(DeviceError::InvalidParameter);
    }

    let vaddr = allocate_mmio_virtual_window(size)?;
    let geometry = get_active_geometry();

    // PTE flags: Present | User | NX | CacheDisable (PCD) | WriteThrough (PWT) (+ Writable)
    let mut flags = PageTableFlags::PRESENT
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::CACHE_DISABLE
        | PageTableFlags::WRITE_THROUGH;

    if writable {
        flags = flags | PageTableFlags::WRITABLE;
    }

    let page_count = size / PAGE_SIZE;
    for i in 0..page_count {
        let p_addr = phys_base + i * PAGE_SIZE;
        let v_addr = vaddr + i * PAGE_SIZE;

        let page = Page::from_start_address(VirtualAddress::new(v_addr), geometry)
            .map_err(|_| DeviceError::InvalidParameter)?;
        let frame = PhysFrame(p_addr);

        if let Err(_) = active_table.map_page(page, frame, flags, MappingDomain::User, pmm) {
            // Rollback previously mapped pages
            for j in 0..i {
                let prev_v = vaddr + j * PAGE_SIZE;
                if let Ok(prev_page) = Page::from_start_address(VirtualAddress::new(prev_v), geometry) {
                    let _ = active_table.unmap_page(prev_page, pmm);
                }
            }
            return Err(DeviceError::OutOfMemory);
        }
    }

    Ok(vaddr)
}

/// Unmaps an MMIO virtual window and invalidates CPU TLB entries.
pub fn unmap_device_mmio(
    active_table: &mut ActivePageTable,
    pmm: &mut PhysicalMemoryManager,
    vaddr: u64,
    size: u64,
) -> Result<(), DeviceError> {
    validate_mmio_isolation(vaddr, size)?;

    let geometry = get_active_geometry();
    let page_count = size / PAGE_SIZE;

    for i in 0..page_count {
        let v = vaddr + i * PAGE_SIZE;
        if let Ok(page) = Page::from_start_address(VirtualAddress::new(v), geometry) {
            let _ = active_table.unmap_page(page, pmm);
        }
    }

    Ok(())
}
