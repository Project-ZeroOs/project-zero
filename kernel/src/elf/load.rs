//! Project Zero - Stage 3J Transactional ELF Program Loader
//!
//! Authoritative Contract: Stage 3J Architecture Specification Rev2 (Approved & Frozen).
//! Invariants: I-ELF-6, I-ELF-7, I-ELF-8 (I-ELF-LOAD-TX-1, I-ELF-LOAD-TX-2), I-ELF-9, I-ELF-10.

use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager};
use crate::mm::vmm::{ActivePageTable, AddressSpace, MappingDomain, Page, PageTableFlags, PhysicalAddress, VirtualAddress};
use crate::task::process::{create_process, ProcessHandle, MAX_PROCESSES, PROCESS_TABLE};
use crate::task::thread::{create_user_thread, Priority};
use super::mmap::{begin_allocation, commit_allocation, record_page, rollback_allocation};
use super::types::*;
use super::validate::validate_elf;

fn abort_load_tx(
    proc_handle: &ProcessHandle,
    proc_slot: usize,
    pml4_frame: PhysFrame,
    pmm: &mut PhysicalMemoryManager,
    apt: &mut ActivePageTable,
    err: ElfError,
) -> ElfError {
    rollback_allocation(proc_slot, pmm, apt);
    let mut as_dest = AddressSpace::from_root(pml4_frame);
    let _ = as_dest.destroy(pmm);
    unsafe {
        let orig = crate::task::scheduler::SCHEDULER.lock.acquire();
        crate::task::process::cancel_process_threads_locked(proc_handle.process, core::ptr::null_mut());
        (*proc_handle.process).address_space = 0;
        (*proc_handle.process).state = crate::task::process::ProcessState::Free;
        PROCESS_TABLE[proc_slot].occupied = false;
        crate::task::scheduler::SCHEDULER.lock.unlock_restore(orig);
    }
    err
}

/// Loads an ELF64 executable into a newly created isolated process.
///
/// Transactional semantics:
/// If ANY step fails prior to commit, all allocated frames are freed, mappings unmapped,
/// page tables destroyed, and process slot cleaned up with zero memory leak.
pub fn load_elf(
    image: &[u8],
    parent_pid: u64,
    priority: Priority,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) -> Result<ProcessHandle, ElfError> {
    // 1. Upfront static validation (zero allocations)
    let summary = validate_elf(image)?;

    // 2. Create isolated process with dedicated AddressSpace PML4
    let proc_handle = create_process(parent_pid, pmm)
        .map_err(|_| ElfError::ProcessCreationFailed)?;

    let pid = proc_handle.pid;
    let proc_slot = {
        let mut s = None;
        for i in 1..MAX_PROCESSES {
            unsafe {
                if PROCESS_TABLE[i].occupied && PROCESS_TABLE[i].process.id == pid {
                    s = Some(i);
                    break;
                }
            }
        }
        match s {
            Some(idx) => idx,
            None => {
                let pml4 = PhysFrame(unsafe { (*proc_handle.process).address_space });
                let mut as_dest = AddressSpace::from_root(pml4);
                let _ = as_dest.destroy(pmm);
                return Err(ElfError::ProcessCreationFailed);
            }
        }
    };

    let pml4_frame = PhysFrame(unsafe { (*proc_handle.process).address_space });
    let mut apt = ActivePageTable::from_root(pml4_frame);
    let geom = crate::mm::vmm::get_active_geometry();

    // 3. Begin transaction in companion memory map table (I-ELF-LOAD-TX-2)
    if let Err(e) = begin_allocation(proc_slot) {
        let mut as_dest = AddressSpace::from_root(pml4_frame);
        let _ = as_dest.destroy(pmm);
        unsafe {
            let orig = crate::task::scheduler::SCHEDULER.lock.acquire();
            crate::task::process::reclaim_process_resources_locked(proc_handle.process, pmm);
            crate::task::scheduler::SCHEDULER.lock.unlock_restore(orig);
        }
        return Err(e);
    }

    // 4. Map each PT_LOAD segment and copy file data + zero BSS (I-ELF-6)
    for seg_idx in 0..summary.segment_count {
        let seg = &summary.segments[seg_idx];

        // Derive PageTableFlags from ELF p_flags (I-ELF-4)
        let mut page_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        if (seg.p_flags & PF_W) != 0 {
            page_flags |= PageTableFlags::WRITABLE;
        }
        if (seg.p_flags & PF_X) == 0 {
            page_flags |= PageTableFlags::NO_EXECUTE;
        }

        let mut curr_vaddr = seg.page_start;
        while curr_vaddr < seg.page_end {
            let page = match Page::from_start_address(VirtualAddress::new(curr_vaddr), geom) {
                Ok(p) => p,
                Err(_) => {
                    return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::InvalidEntryPoint));
                }
            };

            let frame = match pmm.alloc_frame() {
                Some(f) => f,
                None => {
                    return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::OutOfMemory));
                }
            };

            if let Err(e) = record_page(proc_slot, page, frame) {
                let _ = pmm.free_frame(frame);
                return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, e));
            }

            if let Err(_) = apt.map_page(page, frame, page_flags, MappingDomain::User, pmm) {
                return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::MappingFailed));
            }

            // Zero entire 4 KiB frame via HHDM
            let hhdm_ptr = match PhysicalAddress::new(frame.address()).to_hhdm() {
                Ok(ptr) => ptr.as_mut_ptr::<u8>(),
                Err(_) => {
                    return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::MappingFailed));
                }
            };
            unsafe {
                core::ptr::write_bytes(hhdm_ptr, 0, 4096);
            }

            // Copy file-backed slice if segment intersects this page
            let seg_file_start = seg.p_vaddr;
            let seg_file_end = seg.p_vaddr + seg.p_filesz;
            let page_vaddr_end = curr_vaddr + 4096;

            let copy_start = core::cmp::max(curr_vaddr, seg_file_start);
            let copy_end = core::cmp::min(page_vaddr_end, seg_file_end);

            if copy_start < copy_end {
                let copy_len = (copy_end - copy_start) as usize;
                let frame_offset = (copy_start - curr_vaddr) as usize;
                let file_offset = (seg.p_offset + (copy_start - seg.p_vaddr)) as usize;

                unsafe {
                    core::ptr::copy_nonoverlapping(
                        image.as_ptr().add(file_offset),
                        hhdm_ptr.add(frame_offset),
                        copy_len,
                    );
                }
            }

            curr_vaddr += 4096;
        }
    }

    // 5. Construct user stack (I-ELF-7)
    let stack_flags = PageTableFlags::PRESENT
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE;

    for i in 0..USER_STACK_PAGES {
        let page_vaddr = USER_STACK_BASE + (i as u64) * 4096;
        let page = match Page::from_start_address(VirtualAddress::new(page_vaddr), geom) {
            Ok(p) => p,
            Err(_) => {
                return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::StackCreationFailed));
            }
        };

        let frame = match pmm.alloc_frame() {
            Some(f) => f,
            None => {
                return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::OutOfMemory));
            }
        };

        if let Err(e) = record_page(proc_slot, page, frame) {
            let _ = pmm.free_frame(frame);
            return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, e));
        }

        if let Err(_) = apt.map_page(page, frame, stack_flags, MappingDomain::User, pmm) {
            return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::StackCreationFailed));
        }

        let hhdm_ptr = match PhysicalAddress::new(frame.address()).to_hhdm() {
            Ok(ptr) => ptr.as_mut_ptr::<u8>(),
            Err(_) => {
                return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::StackCreationFailed));
            }
        };
        unsafe {
            core::ptr::write_bytes(hhdm_ptr, 0, 4096);
        }
    }

    // 6. Verify entry point is mapped and executable
    let entry_page_vaddr = summary.entry_point & !0xFFF;
    let entry_page = match Page::from_start_address(VirtualAddress::new(entry_page_vaddr), geom) {
        Ok(p) => p,
        Err(_) => {
            return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::InvalidEntryPoint));
        }
    };
    let entry_flags = match apt.get_page_flags(entry_page) {
        Ok(fl) => fl,
        Err(_) => {
            return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::InvalidEntryPoint));
        }
    };
    if entry_flags.contains(PageTableFlags::NO_EXECUTE) {
        return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::InvalidEntryPoint));
    }

    // 7. Create initial user thread (I-ELF-10)
    let _thread = match create_user_thread(
        proc_handle.process,
        summary.entry_point,
        USER_INITIAL_RSP,
        priority,
        pmm,
        vmm,
    ) {
        Ok(t) => t,
        Err(_) => {
            return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, ElfError::ThreadCreationFailed));
        }
    };

    // 8. Commit transaction (I-ELF-LOAD-TX-2)
    if let Err(e) = commit_allocation(proc_slot) {
        return Err(abort_load_tx(&proc_handle, proc_slot, pml4_frame, pmm, &mut apt, e));
    }

    Ok(proc_handle)
}
