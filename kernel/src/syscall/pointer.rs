//! Project Zero - Stage 3I User Pointer and Return State Validation
//!
//! Enforces I-SYSCALL-5, I-SYSCALL-7, I-SYSCALL-RETURN-1, and I-SYSCALL-RETURN-3.

use super::abi::SyscallFrame;
use super::numbers::SyscallError;
use crate::mm::vmm::{ActivePageTable, Page, PageTableFlags, VirtualAddress};

pub const USER_NULL_GUARD_START: u64   = 0x0000_0000_0000_0000;
pub const USER_NULL_GUARD_END: u64     = 0x0000_0000_0020_0000; // 2 MiB

pub const USER_VA_MIN: u64             = 0x0000_0000_0020_0000; // Inclusive lower boundary
pub const USER_CODE_BASE: u64          = 0x0000_0000_0020_0000; // Text segment base (RX)
pub const USER_DATA_BASE: u64          = 0x0000_0000_0040_0000; // Static data segment base (RW+NX)
pub const USER_SHM_BASE: u64           = 0x0000_0000_2000_0000; // Shared memory base (RW+NX or RO+NX)

pub const USER_STACK_BASE: u64         = 0x0000_7F7F_FFFC_0000; // Stack bottom (16 KiB / 4 pages)
pub const USER_STACK_TOP: u64          = 0x0000_7F7F_FFFF_0000; // Initial user RSP (16-B aligned)

/// Dedicated Device MMIO Virtual Aperture (Stage 3L Rev3, I-DEV-MMIO-1)
pub const USER_DEV_MMIO_BASE: u64      = 0x0000_6000_0000_0000; // 1 TiB MMIO window base
pub const USER_DEV_MMIO_END: u64       = 0x0000_7000_0000_0000; // 1 TiB MMIO window end

pub const USER_UPPER_GUARD_START: u64  = 0x0000_7F80_0000_0000; // 2 GiB upper guard window
pub const USER_UPPER_GUARD_END: u64    = 0x0000_8000_0000_0000; // Non-canonical boundary

pub const USER_VA_MAX_EXCLUSIVE: u64   = 0x0000_7F80_0000_0000; // Exclusive upper boundary

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAccess {
    Read,
    Write,
    Execute,
}

/// Validates that a requested virtual address range is strictly within the dedicated MMIO aperture (I-DEV-MMIO-1).
pub fn validate_mmio_range(vaddr: u64, len: usize) -> Result<(), SyscallError> {
    if len == 0 || (len % 4096) != 0 || (vaddr % 4096) != 0 {
        return Err(SyscallError::InvalidArgument);
    }
    let end = vaddr.checked_add(len as u64).ok_or(SyscallError::BadAddress)?;
    if vaddr < USER_DEV_MMIO_BASE || end > USER_DEV_MMIO_END {
        return Err(SyscallError::BadAddress);
    }
    Ok(())
}

/// Unified user buffer validation (I-SYSCALL-7).
pub fn validate_user_range(
    ptr: u64,
    len: usize,
    access: MemoryAccess,
    vmm: &ActivePageTable,
) -> Result<(), SyscallError> {
    if len == 0 {
        return Ok(());
    }

    // 1. Integer overflow check
    let end = ptr.checked_add(len as u64).ok_or(SyscallError::BadAddress)?;

    // 2. Canonical user address boundary check
    if ptr < USER_VA_MIN || end > USER_VA_MAX_EXCLUSIVE {
        return Err(SyscallError::BadAddress);
    }

    // 2b. Reject ordinary user pointer overlap with dedicated DEVICE_MMIO_REGION (I-DEV-MMIO-1)
    if ptr < USER_DEV_MMIO_END && end > USER_DEV_MMIO_BASE {
        return Err(SyscallError::BadAddress);
    }

    // 3. Multi-page traversal and permission check
    let start_page = ptr & !0xFFF;
    let end_page = (end - 1) & !0xFFF;

    let mut curr = start_page;
    loop {
        let page = Page::from_address(VirtualAddress::new(curr));
        let flags = vmm.get_page_flags(page).map_err(|_| SyscallError::BadAddress)?;

        if !flags.contains(PageTableFlags::PRESENT) || !flags.contains(PageTableFlags::USER_ACCESSIBLE) {
            return Err(SyscallError::BadAddress);
        }

        match access {
            MemoryAccess::Read => {
                // Present + UserAccessible is sufficient for Read
            }
            MemoryAccess::Write => {
                if !flags.contains(PageTableFlags::WRITABLE) {
                    return Err(SyscallError::BadAddress);
                }
            }
            MemoryAccess::Execute => {
                if flags.contains(PageTableFlags::NO_EXECUTE) {
                    return Err(SyscallError::BadAddress);
                }
            }
        }

        if curr == end_page {
            break;
        }
        curr = curr.checked_add(4096).ok_or(SyscallError::BadAddress)?;
    }

    Ok(())
}

/// Defensive machine validation of user return state before executing sysretq (I-SYSCALL-RETURN-1).
pub fn validate_user_return_state(
    frame: &SyscallFrame,
    vmm: &ActivePageTable,
) -> Result<(), SyscallError> {
    // 1. Validate User RIP
    if frame.user_rip < USER_VA_MIN || frame.user_rip >= USER_VA_MAX_EXCLUSIVE {
        return Err(SyscallError::BadAddress);
    }
    let rip_page = Page::from_address(VirtualAddress::new(frame.user_rip));
    let rip_flags = vmm.get_page_flags(rip_page).map_err(|_| SyscallError::BadAddress)?;
    if !rip_flags.contains(PageTableFlags::PRESENT)
        || !rip_flags.contains(PageTableFlags::USER_ACCESSIBLE)
        || rip_flags.contains(PageTableFlags::NO_EXECUTE)
    {
        return Err(SyscallError::BadAddress);
    }

    // 2. Validate User RSP (must be 8-byte aligned, canonical, user space, writable page)
    if (frame.user_rsp & 7) != 0 {
        return Err(SyscallError::BadAddress);
    }
    if frame.user_rsp < USER_VA_MIN || frame.user_rsp > USER_VA_MAX_EXCLUSIVE {
        return Err(SyscallError::BadAddress);
    }
    // Check page for stack access (user_rsp - 8 if rsp > USER_VA_MIN, or user_rsp itself)
    let check_rsp = if frame.user_rsp >= USER_VA_MIN + 8 {
        frame.user_rsp - 8
    } else {
        frame.user_rsp
    };
    let rsp_page = Page::from_address(VirtualAddress::new(check_rsp));
    let rsp_flags = vmm.get_page_flags(rsp_page).map_err(|_| SyscallError::BadAddress)?;
    if !rsp_flags.contains(PageTableFlags::PRESENT)
        || !rsp_flags.contains(PageTableFlags::USER_ACCESSIBLE)
        || !rsp_flags.contains(PageTableFlags::WRITABLE)
    {
        return Err(SyscallError::BadAddress);
    }

    // 3. Validate User RFLAGS
    // IF (bit 9) must be 1
    if (frame.user_rflags & 0x0200) == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    // Reserved bit 1 must be 1
    if (frame.user_rflags & 0x0002) == 0 {
        return Err(SyscallError::InvalidArgument);
    }
    // Sensitive / forbidden flags must be 0:
    // IOPL (bits 12..13): 0x3000
    // NT (bit 14): 0x4000
    // TF (bit 8): 0x0100
    // VM (bit 17): 0x0002_0000
    // VIP (bit 20): 0x0010_0000
    // VIF (bit 19): 0x0008_0000
    let forbidden_mask = 0x001A_7100;
    if (frame.user_rflags & forbidden_mask) != 0 {
        return Err(SyscallError::InvalidArgument);
    }

    // 4. Validate Selectors
    if frame.user_cs != 0x23 {
        return Err(SyscallError::InvalidArgument);
    }
    if frame.user_ss != 0x1B {
        return Err(SyscallError::InvalidArgument);
    }

    Ok(())
}
