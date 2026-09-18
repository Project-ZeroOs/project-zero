//! Project Zero - Stage 3J ELF Header & Segment Validation
//!
//! Authoritative Contract: Stage 3J Architecture Specification Rev2 (Approved & Frozen).
//! Invariants: I-ELF-1 through I-ELF-5.

use crate::mm::vmm::VirtualAddress;
use crate::syscall::pointer::{USER_VA_MAX_EXCLUSIVE, USER_VA_MIN};
use super::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedSegment {
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub page_start: u64,
    pub page_end: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElfValidationSummary {
    pub entry_point: u64,
    pub segment_count: usize,
    pub segments: [ValidatedSegment; MAX_PROGRAM_HEADERS],
}

/// Validates an in-memory ELF64 binary according to frozen Rev2 invariants.
pub fn validate_elf(image: &[u8]) -> Result<ElfValidationSummary, ElfError> {
    // 1. Basic length check
    if image.len() < core::mem::size_of::<Elf64Ehdr>() {
        return Err(ElfError::ImageTooSmall);
    }

    // 2. Identity and magic checks (I-ELF-1)
    if image[0..4] != ELFMAG {
        return Err(ElfError::InvalidMagic);
    }
    if image[4] != ELFCLASS64 {
        return Err(ElfError::UnsupportedClass);
    }
    if image[5] != ELFDATA2LSB {
        return Err(ElfError::UnsupportedEndian);
    }
    if image[6] != 1 {
        return Err(ElfError::UnsupportedVersion);
    }

    let ehdr = unsafe { &*(image.as_ptr() as *const Elf64Ehdr) };

    if ehdr.e_type != ET_EXEC {
        return Err(ElfError::UnsupportedType);
    }
    if ehdr.e_machine != EM_X86_64 {
        return Err(ElfError::UnsupportedMachine);
    }
    if ehdr.e_version != 1 {
        return Err(ElfError::UnsupportedVersion);
    }
    if ehdr.e_ehsize != 64 || ehdr.e_phentsize != 56 {
        return Err(ElfError::InvalidHeader);
    }
    if ehdr.e_phnum == 0 || (ehdr.e_phnum as usize) > MAX_PROGRAM_HEADERS {
        return Err(ElfError::InvalidHeader);
    }

    // Check program header table file bounds
    let ph_table_end = (ehdr.e_phnum as u64)
        .checked_mul(56)
        .and_then(|sz| ehdr.e_phoff.checked_add(sz))
        .ok_or(ElfError::FileBoundsExceeded)?;
    if ph_table_end > image.len() as u64 {
        return Err(ElfError::FileBoundsExceeded);
    }

    let mut segments: [ValidatedSegment; MAX_PROGRAM_HEADERS] = [const {
        ValidatedSegment {
            p_flags: 0,
            p_offset: 0,
            p_vaddr: 0,
            p_filesz: 0,
            p_memsz: 0,
            page_start: 0,
            page_end: 0,
        }
    }; MAX_PROGRAM_HEADERS];
    let mut load_count = 0;
    let mut entry_covered = false;

    // 3. Process each program header (I-ELF-2, I-ELF-3, I-ELF-4)
    for i in 0..(ehdr.e_phnum as usize) {
        let ph_offset = (ehdr.e_phoff as usize) + i * 56;
        let ph = unsafe { &*(image.as_ptr().add(ph_offset) as *const Elf64Phdr) };

        match ph.p_type {
            PT_LOAD => {
                // Bounds & arithmetic validation
                let file_end = ph.p_offset
                    .checked_add(ph.p_filesz)
                    .ok_or(ElfError::FileBoundsExceeded)?;
                if file_end > image.len() as u64 {
                    return Err(ElfError::FileBoundsExceeded);
                }

                if ph.p_filesz > ph.p_memsz {
                    return Err(ElfError::MalformedSegment);
                }

                let mem_end = ph.p_vaddr
                    .checked_add(ph.p_memsz)
                    .ok_or(ElfError::VirtualAddressOverflow)?;
                if ph.p_vaddr < USER_VA_MIN || mem_end > USER_VA_MAX_EXCLUSIVE {
                    return Err(ElfError::OutOfUserBounds);
                }

                // Canonicality check
                let geom = crate::mm::vmm::get_active_geometry();
                if !VirtualAddress::new(ph.p_vaddr).is_canonical(geom)
                    || (ph.p_memsz > 0 && !VirtualAddress::new(mem_end - 1).is_canonical(geom))
                {
                    return Err(ElfError::OutOfUserBounds);
                }

                // Alignment contract (p_align)
                if ph.p_align > 1 {
                    if !ph.p_align.is_power_of_two() || ph.p_align < 4096 {
                        return Err(ElfError::InvalidAlignment);
                    }
                    if (ph.p_vaddr % ph.p_align) != (ph.p_offset % ph.p_align) {
                        return Err(ElfError::MisalignedOffsetCongruence);
                    }
                }
                if (ph.p_vaddr % 4096) != (ph.p_offset % 4096) {
                    return Err(ElfError::MisalignedOffsetCongruence);
                }

                // Strict W^X enforcement (I-ELF-4)
                if (ph.p_flags & PF_W != 0) && (ph.p_flags & PF_X != 0) {
                    return Err(ElfError::WwxViolation);
                }
                if (ph.p_flags & PF_R) == 0 {
                    return Err(ElfError::InvalidPermissions);
                }

                // Checked page rounding
                let page_start = ph.p_vaddr & !0xFFF;
                let page_end = (ph.p_vaddr
                    .checked_add(ph.p_memsz)
                    .ok_or(ElfError::VirtualAddressOverflow)?
                    .checked_add(0xFFF)
                    .ok_or(ElfError::VirtualAddressOverflow)?) & !0xFFF;

                // Non-intersection with user stack and guard window
                if page_start < USER_STACK_TOP && page_end > USER_STACK_GUARD {
                    return Err(ElfError::OverlappingSegments);
                }

                // Non-intersection with prior loadable segments
                for prev_idx in 0..load_count {
                    let prev = &segments[prev_idx];
                    if page_start < prev.page_end && page_end > prev.page_start {
                        return Err(ElfError::OverlappingSegments);
                    }
                }

                // Check entry point coverage
                if (ph.p_flags & PF_X != 0)
                    && ehdr.e_entry >= ph.p_vaddr
                    && ehdr.e_entry < mem_end
                {
                    entry_covered = true;
                }

                segments[load_count] = ValidatedSegment {
                    p_flags: ph.p_flags,
                    p_offset: ph.p_offset,
                    p_vaddr: ph.p_vaddr,
                    p_filesz: ph.p_filesz,
                    p_memsz: ph.p_memsz,
                    page_start,
                    page_end,
                };
                load_count += 1;
            }
            PT_PHDR | PT_NOTE => {
                let file_end = ph.p_offset
                    .checked_add(ph.p_filesz)
                    .ok_or(ElfError::FileBoundsExceeded)?;
                if file_end > image.len() as u64 {
                    return Err(ElfError::FileBoundsExceeded);
                }
            }
            PT_GNU_STACK => {
                // Must NOT request executable stack
                if (ph.p_flags & PF_X) != 0 {
                    return Err(ElfError::ExecutableStackRejected);
                }
            }
            _ => {
                return Err(ElfError::UnsupportedProgramHeader);
            }
        }
    }

    if load_count == 0 {
        return Err(ElfError::MalformedSegment);
    }

    // 4. Entry point validation (I-ELF-5)
    let geom = crate::mm::vmm::get_active_geometry();
    if !VirtualAddress::new(ehdr.e_entry).is_canonical(geom)
        || ehdr.e_entry < USER_VA_MIN
        || ehdr.e_entry >= USER_VA_MAX_EXCLUSIVE
        || !entry_covered
    {
        return Err(ElfError::InvalidEntryPoint);
    }

    Ok(ElfValidationSummary {
        entry_point: ehdr.e_entry,
        segment_count: load_count,
        segments,
    })
}
