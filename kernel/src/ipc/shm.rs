//! Project Zero - Stage 3G Shared Memory (ShmObject) Subsystem
//!
//! Authoritative Contract: Stage 3G Architecture Rev10 (Approved & Frozen).
//!
//! Sizes and Alignments:
//!   - ShmObject: 144 bytes (align 8)
//!   - ShmMapping: 32 bytes (align 8)
//!   - SHM_TABLE: 64 * 144 = 9 KiB in .bss
//!   - SHM_MAPPING_TABLE: 64 * 32 = 2 KiB in .bss

use crate::ipc::types::{rights, IpcError};
use crate::ipc::object::{
    allocate_object_slot_locked, free_object_slot_locked, KernelObjectType,
    ObjectLifecycleState, KERNEL_OBJECT_TABLE, KERNEL_OBJECT_TABLE_LOCK, MAX_KERNEL_OBJECTS,
};
use crate::ipc::handle::{
    allocate_handle_entry_locked, close_handle_locked, resolve_current_process_slot,
    validate_handle_locked, Handle,
};
use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager};
use crate::mm::vmm::{ActivePageTable, Page, PageTableFlags, VirtualAddress};
use crate::task::percpu::current_thread_from_gs;

pub const MAX_SHM_OBJECTS: usize = 64;
pub const MAX_SHM_PAGES: usize = 16;
pub const MAX_SHM_MAPPINGS: usize = 64;

#[repr(C)]
#[derive(Debug)]
pub struct ShmObject {
    /// Number of 4 KiB physical pages (1..16).
    pub page_count: usize,
    /// Physical frames backing this shared memory object.
    pub frames: [PhysFrame; MAX_SHM_PAGES], // 16 * 8 = 128 bytes
    /// Creator process ID.
    pub creator_pid: u64,
}

const _: () = assert!(core::mem::size_of::<ShmObject>() == 144);
const _: () = assert!(core::mem::align_of::<ShmObject>() == 8);

impl ShmObject {
    pub const fn empty() -> Self {
        Self {
            page_count: 0,
            frames: [PhysFrame(0); MAX_SHM_PAGES],
            creator_pid: 0,
        }
    }
}

pub static mut SHM_TABLE: [ShmObject; MAX_SHM_OBJECTS] =
    [const { ShmObject::empty() }; MAX_SHM_OBJECTS];

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShmMapping {
    pub occupied: bool,
    pub _pad1: u8,
    pub object_index: u16,
    pub object_generation: u16,
    pub rights: u16,
    pub process_id: u64,
    pub virtual_base: u64,
    pub page_count: usize,
}

const _: () = assert!(core::mem::size_of::<ShmMapping>() == 32);
const _: () = assert!(core::mem::align_of::<ShmMapping>() == 8);

impl ShmMapping {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            _pad1: 0,
            object_index: 0,
            object_generation: 0,
            rights: 0,
            process_id: 0,
            virtual_base: 0,
            page_count: 0,
        }
    }
}

pub static mut SHM_MAPPING_TABLE: [ShmMapping; MAX_SHM_MAPPINGS] =
    [const { ShmMapping::empty() }; MAX_SHM_MAPPINGS];

/// Creates a new ShmObject backed by `page_count` physical frames from PMM.
pub fn shm_create(page_count: usize, pmm: &mut PhysicalMemoryManager) -> Result<Handle, IpcError> {
    if page_count == 0 || page_count > MAX_SHM_PAGES {
        return Err(IpcError::BadMessageSize);
    }

    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;

    // Allocate physical frames transactionally
    let mut allocated_frames = [PhysFrame(0); MAX_SHM_PAGES];
    for i in 0..page_count {
        match pmm.allocate_frame() {
            Some(frame) => {
                // Zero frame via HHDM
                let phys = crate::mm::vmm::PhysicalAddress::new(frame.address());
                if let Ok(hhdm) = phys.to_hhdm() {
                    unsafe {
                        core::ptr::write_bytes(hhdm.as_mut_ptr::<u8>(), 0, 4096);
                    }
                }
                allocated_frames[i] = frame;
            }
            None => {
                // Rollback on PMM exhaustion
                for j in 0..i {
                    let _ = pmm.free_frame(allocated_frames[j]);
                }
                return Err(IpcError::PmmExhausted);
            }
        }
    }


    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    // Locate free slot in SHM_TABLE
    let mut shm_idx = None;
    unsafe {
        for i in 0..MAX_SHM_OBJECTS {
            if SHM_TABLE[i].page_count == 0 {
                // Check if any object points to it
                let mut in_use = false;
                for o in 0..MAX_KERNEL_OBJECTS {
                    let obj = &KERNEL_OBJECT_TABLE[o];
                    if obj.occupied && obj.obj_type == KernelObjectType::ShmObject && obj.pool_index == i as u16 {
                        in_use = true;
                        break;
                    }
                }
                if !in_use {
                    shm_idx = Some(i);
                    break;
                }
            }
        }
    }

    let s_idx = match shm_idx {
        Some(idx) => idx,
        None => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            for i in 0..page_count {
                let _ = pmm.free_frame(allocated_frames[i]);
            }
            return Err(IpcError::ShmTableFull);
        }
    };

    // Allocate KernelObjectSlot
    let obj_idx = match unsafe { allocate_object_slot_locked(KernelObjectType::ShmObject, s_idx as u16, cur_pid) } {
        Ok(idx) => idx,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            for i in 0..page_count {
                let _ = pmm.free_frame(allocated_frames[i]);
            }
            return Err(e);
        }
    };

    // Initialize ShmObject
    unsafe {
        let shm = &mut SHM_TABLE[s_idx];
        shm.page_count = page_count;
        shm.frames = allocated_frames;
        shm.creator_pid = cur_pid;
    }

    let default_rights = rights::READ | rights::WRITE | rights::MAP_RO | rights::MAP_RW | rights::TRANSFER | rights::DUPLICATE
        | crate::cap::types::cap_rights::SHM_MAP_READ
        | crate::cap::types::cap_rights::SHM_MAP_WRITE
        | crate::cap::types::cap_rights::SHM_UNMAP
        | crate::cap::types::cap_rights::DUPLICATE
        | crate::cap::types::cap_rights::TRANSFER
        | crate::cap::types::cap_rights::REVOKE
        | crate::cap::types::cap_rights::CLOSE
        | crate::cap::types::cap_rights::INSPECT
        | crate::cap::types::cap_rights::AUDIT;

    let handle = match unsafe { allocate_handle_entry_locked(proc_slot, obj_idx, default_rights, 0) } {
        Ok(h) => h,
        Err(e) => {
            unsafe {
                free_object_slot_locked(obj_idx);
                SHM_TABLE[s_idx] = ShmObject::empty();
            }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            for i in 0..page_count {
                let _ = pmm.free_frame(allocated_frames[i]);
            }
            return Err(e);
        }
    };

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    Ok(handle)
}

/// Maps a ShmObject into the calling process's active address space.
///
/// Enforces W^X (unconditional PAGE_NX).
pub fn shm_map(
    handle: Handle,
    vaddr: u64,
    writable: bool,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) -> Result<(), IpcError> {
    if vaddr % 4096 != 0 || vaddr >= 0x0000_8000_0000_0000 {
        return Err(IpcError::InvalidAddress);
    }

    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;
    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    let (obj_idx, _, rights_mask) = match unsafe { validate_handle_locked(proc_slot, handle, 0) } {
        Ok(res) => res,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    let required_right = if writable {
        rights::MAP_RW | crate::cap::types::cap_rights::SHM_MAP_WRITE
    } else {
        rights::MAP_RO | crate::cap::types::cap_rights::SHM_MAP_READ
    };

    if (rights_mask & required_right) == 0 {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::PermissionDenied);
    }

    let obj_slot = unsafe { &mut KERNEL_OBJECT_TABLE[obj_idx] };
    assert_eq!(obj_slot.obj_type, KernelObjectType::ShmObject);
    let s_idx = obj_slot.pool_index as usize;
    let shm = unsafe { &SHM_TABLE[s_idx] };
    let page_count = shm.page_count;
    let obj_generation = obj_slot.generation;

    // Locate free slot in SHM_MAPPING_TABLE
    let mut mapping_slot = None;
    unsafe {
        for i in 0..MAX_SHM_MAPPINGS {
            if !SHM_MAPPING_TABLE[i].occupied {
                mapping_slot = Some(i);
                break;
            }
        }
    }

    let m_idx = match mapping_slot {
        Some(idx) => idx,
        None => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::MappingTableFull);
        }
    };

    // Prepare page table flags: PAGE_USER | PAGE_PRESENT | PAGE_NX (+ PAGE_WRITABLE if writable)
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::NO_EXECUTE;
    if writable {
        flags |= PageTableFlags::WRITABLE;
    }

    // Map pages locklessly
    for i in 0..page_count {
        let page_vaddr = vaddr + (i as u64) * 4096;
        let page = Page::from_start_address(
            VirtualAddress::new(page_vaddr),
            crate::mm::vmm::get_active_geometry(),
        ).map_err(|_| {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            IpcError::InvalidAddress
        })?;

        match vmm.map_page(page, shm.frames[i], flags, crate::mm::vmm::MappingDomain::User, pmm) {
            Ok(_) => {}
            Err(_) => {
                // Rollback previously mapped pages
                for j in 0..i {
                    let prev_vaddr = vaddr + (j as u64) * 4096;
                    let prev_page = Page::from_start_address(
                        VirtualAddress::new(prev_vaddr),
                        crate::mm::vmm::get_active_geometry(),
                    ).unwrap();
                    let _ = vmm.unmap_page(prev_page, pmm);
                }
                KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
                return Err(IpcError::VmmError);
            }
        }
    }

    // Record mapping
    unsafe {
        SHM_MAPPING_TABLE[m_idx] = ShmMapping {
            occupied: true,
            _pad1: 0,
            object_index: obj_idx as u16,
            object_generation: obj_generation,
            rights: required_right,
            process_id: cur_pid,
            virtual_base: vaddr,
            page_count,
        };

        obj_slot.header.mapping_refs += 1;
    }

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    Ok(())
}

/// Unmaps a ShmObject from the calling process's active address space.
pub fn shm_unmap(
    handle: Handle,
    vaddr: u64,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) -> Result<(), IpcError> {
    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;

    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    // 1. Locate entry in SHM_MAPPING_TABLE
    let mut mapping_idx = None;
    unsafe {
        for i in 0..MAX_SHM_MAPPINGS {
            let m = &SHM_MAPPING_TABLE[i];
            if m.occupied && m.process_id == cur_pid && m.virtual_base == vaddr {
                mapping_idx = Some(i);
                break;
            }
        }
    }

    let m_idx = match mapping_idx {
        Some(idx) => idx,
        None => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::InvalidAddress);
        }
    };

    let mapping = unsafe { &mut SHM_MAPPING_TABLE[m_idx] };

    let obj_idx = mapping.object_index as usize;
    if obj_idx >= MAX_KERNEL_OBJECTS {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::InvalidHandle);
    }
    let obj_slot = unsafe { &KERNEL_OBJECT_TABLE[obj_idx] };
    if !obj_slot.occupied || obj_slot.generation != mapping.object_generation {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::InvalidHandle);
    }

    // 2. Validate authorization with handle if handle is active
    if let Ok((h_obj_idx, _, _)) = unsafe { validate_handle_locked(proc_slot, handle, 0) } {
        if h_obj_idx != obj_idx {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::PermissionDenied);
        }
    }

    // 3. Unmap pages
    let page_count = mapping.page_count;
    for i in 0..page_count {
        let page_vaddr = vaddr + (i as u64) * 4096;
        if let Ok(page) = Page::from_start_address(
            VirtualAddress::new(page_vaddr),
            crate::mm::vmm::get_active_geometry(),
        ) {
            let _ = vmm.unmap_page(page, pmm);
        }
    }

    // 4. Invalidate mapping entry and decrement mapping_refs
    mapping.occupied = false;
    let obj_slot = unsafe { &mut KERNEL_OBJECT_TABLE[obj_idx] };
    obj_slot.header.mapping_refs = obj_slot.header.mapping_refs.saturating_sub(1);

    unsafe {
        check_and_reclaim_shm_locked(obj_idx, pmm);
    }

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    Ok(())
}

/// Closes a user handle to a ShmObject.
pub fn shm_close(handle: Handle, pmm: &mut PhysicalMemoryManager) -> Result<(), IpcError> {
    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;

    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    let (obj_idx, _) = match unsafe { close_handle_locked(proc_slot, handle) } {
        Ok(res) => res,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    unsafe {
        check_and_reclaim_shm_locked(obj_idx, pmm);
    }

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    Ok(())
}

/// Checks if a ShmObject can be reclaimed and freed to PMM under KERNEL_OBJECT_TABLE_LOCK.
pub unsafe fn check_and_reclaim_shm_locked(obj_idx: usize, pmm: &mut PhysicalMemoryManager) {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    let obj_slot = &mut KERNEL_OBJECT_TABLE[obj_idx];
    if !obj_slot.occupied || obj_slot.obj_type != KernelObjectType::ShmObject {
        return;
    }

    // Reclamation requires ref_count == 0
    if obj_slot.header.ref_count() == 0 {
        obj_slot.header.state = ObjectLifecycleState::Reclaiming;
        let s_idx = obj_slot.pool_index as usize;
        let shm = &mut SHM_TABLE[s_idx];

        // Free physical frames back to PMM
        for i in 0..shm.page_count {
            let _ = pmm.free_frame(shm.frames[i]);
        }

        *shm = ShmObject::empty();
        free_object_slot_locked(obj_idx);
    }
}

/// Cleans up all SHM mappings and handles for a terminating process.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF=0).
pub unsafe fn cleanup_process_shm_and_handles_locked(
    process_slot: usize,
    pid: u64,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    crate::cap::ops::process_exit_capability_cleanup_locked(process_slot, pid, pmm, vmm);
}
