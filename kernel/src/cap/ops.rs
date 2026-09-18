//! Project Zero - Stage 3H Capability Operations & Lifecycle Subsystem
//!
//! Authoritative Contract: Stage 3H Architecture Rev3 & Implementation Plan Rev6.

use crate::ipc::types::IpcError;
use crate::ipc::handle::{Handle, MAX_HANDLES_PER_PROCESS, PROCESS_HANDLE_TABLES};
use crate::ipc::object::{KERNEL_OBJECT_TABLE, MAX_KERNEL_OBJECTS, KernelObjectType};
use crate::ipc::KERNEL_OBJECT_TABLE_LOCK;
use crate::cap::types::{
    CapabilityNode, cap_rights, MAX_PROCESSES, MAX_DERIVATION_DEPTH,
};
use crate::cap::node::{
    CAPABILITY_NODE_TABLE, allocate_capability_id, add_child_locked,
    remove_child_locked, replace_child_locked,
};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;

/// Reserves an empty handle slot in the specified process's HandleTable under lock.
///
/// Invariant I-CAP-HANDLE-1:
/// - Does NOT mark the slot occupied or populate any capability state.
/// - Does NOT mutate HandleTable.count (Invariant I-CAP-HANDLE-2).
pub unsafe fn allocate_handle_slot_locked(
    process_slot: usize,
) -> Result<usize, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(process_slot < MAX_PROCESSES);

    let htable = &PROCESS_HANDLE_TABLES[process_slot];
    for slot_idx in 0..MAX_HANDLES_PER_PROCESS {
        if !htable.entries[slot_idx].occupied {
            return Ok(slot_idx);
        }
    }
    Err(IpcError::HandleTableFull)
}

/// Installs a primary root capability into a reserved handle slot under lock.
///
/// Exclusively used by primary object creation (channel_create, shm_create).
/// Enforces invariants: I-CAP-HANDLE-1, I-CAP-HANDLE-2, I-CAP-ID-1.
pub unsafe fn install_root_capability_locked(
    process_slot: usize,
    handle_slot: usize,
    object_index: usize,
    rights: u16,
    endpoint: u8,
) -> Result<Handle, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(process_slot < MAX_PROCESSES && handle_slot < MAX_HANDLES_PER_PROCESS);
    assert!(object_index < MAX_KERNEL_OBJECTS);

    let obj_slot = &mut KERNEL_OBJECT_TABLE[object_index];
    assert!(obj_slot.occupied);

    let cap_id = allocate_capability_id()?;

    let htable = &mut PROCESS_HANDLE_TABLES[process_slot];
    let entry = &mut htable.entries[handle_slot];
    assert!(!entry.occupied, "Reserved slot must be unoccupied");

    entry.occupied = true;
    entry.object_index = object_index as u16;
    entry.object_generation = obj_slot.generation;
    entry.rights = rights;
    entry.endpoint = endpoint;

    let node = &mut CAPABILITY_NODE_TABLE[process_slot][handle_slot];
    node.capability_id = cap_id;
    node.parent_pslot = 0xFF;
    node.parent_hslot = 0xFF;
    node.parent_gen = 0;
    node.derivation_depth = 0;
    node.is_revoked = false;
    node.flags = 0;
    node.first_child_pslot = 0xFF;
    node.first_child_hslot = 0xFF;
    node.next_sibling_pslot = 0xFF;
    node.next_sibling_hslot = 0xFF;

    htable.count += 1; // Invariant I-CAP-HANDLE-2
    obj_slot.header.handle_refs = obj_slot.header.handle_refs.checked_add(1)
        .ok_or(IpcError::HandleTableFull)?;

    Ok(Handle::new(handle_slot as u16, entry.handle_generation))
}

/// Installs a derived capability into a reserved handle slot under lock.
///
/// Used by capability_derive, capability_duplicate, and TRANSFER_DELEGATE.
/// Enforces invariants: I-CAP-HANDLE-1, I-CAP-HANDLE-2, I-CAP-X, I-CAP-4.
pub unsafe fn install_derived_capability_locked(
    dst_pslot: usize,
    dst_hslot: usize,
    parent_pslot: usize,
    parent_hslot: usize,
    requested_rights: u16,
) -> Result<Handle, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(dst_pslot < MAX_PROCESSES && dst_hslot < MAX_HANDLES_PER_PROCESS);
    assert!(parent_pslot < MAX_PROCESSES && parent_hslot < MAX_HANDLES_PER_PROCESS);

    let parent_entry = &PROCESS_HANDLE_TABLES[parent_pslot].entries[parent_hslot];
    if !parent_entry.occupied {
        return Err(IpcError::InvalidHandle);
    }
    let parent_node = &CAPABILITY_NODE_TABLE[parent_pslot][parent_hslot];
    if parent_node.is_revoked {
        return Err(IpcError::CapabilityRevoked);
    }

    // Require DUPLICATE right to derive
    if (parent_entry.rights & (cap_rights::DUPLICATE | crate::ipc::types::rights::DUPLICATE)) == 0 {
        return Err(IpcError::CapabilityNotDuplicable);
    }

    // Invariant I-CAP-X: Monotonic Rights Attenuation (No Amplification)
    if (requested_rights & !parent_entry.rights) != 0 {
        return Err(IpcError::RightsAmplificationRejected);
    }

    // Invariant I-CAP-4: Bounded Derivation Depth
    if parent_node.derivation_depth >= MAX_DERIVATION_DEPTH {
        return Err(IpcError::MaxDerivationDepthExceeded);
    }

    let obj_idx = parent_entry.object_index as usize;
    if obj_idx >= MAX_KERNEL_OBJECTS {
        return Err(IpcError::InvalidHandle);
    }
    let obj_slot = &mut KERNEL_OBJECT_TABLE[obj_idx];
    if !obj_slot.occupied || obj_slot.generation != parent_entry.object_generation {
        return Err(IpcError::BadHandleGeneration);
    }

    let cap_id = allocate_capability_id()?;

    let parent_gen = parent_entry.handle_generation;
    let parent_obj_gen = parent_entry.object_generation;
    let parent_endpoint = parent_entry.endpoint;
    let new_depth = parent_node.derivation_depth + 1;

    let dst_htable = &mut PROCESS_HANDLE_TABLES[dst_pslot];
    let dst_entry = &mut dst_htable.entries[dst_hslot];
    assert!(!dst_entry.occupied, "Reserved slot must be unoccupied");

    dst_entry.occupied = true;
    dst_entry.object_index = obj_idx as u16;
    dst_entry.object_generation = parent_obj_gen;
    dst_entry.rights = requested_rights;
    dst_entry.endpoint = parent_endpoint;

    let dst_node = &mut CAPABILITY_NODE_TABLE[dst_pslot][dst_hslot];
    dst_node.capability_id = cap_id;
    dst_node.parent_pslot = parent_pslot as u8;
    dst_node.parent_hslot = parent_hslot as u8;
    dst_node.parent_gen = parent_gen;
    dst_node.derivation_depth = new_depth;
    dst_node.is_revoked = false;
    dst_node.flags = 0;
    dst_node.first_child_pslot = 0xFF;
    dst_node.first_child_hslot = 0xFF;
    dst_node.next_sibling_pslot = 0xFF;
    dst_node.next_sibling_hslot = 0xFF;

    add_child_locked(parent_pslot, parent_hslot, dst_pslot, dst_hslot);

    dst_htable.count += 1; // Invariant I-CAP-HANDLE-2
    obj_slot.header.handle_refs = obj_slot.header.handle_refs.checked_add(1)
        .ok_or(IpcError::HandleTableFull)?;

    Ok(Handle::new(dst_hslot as u16, dst_entry.handle_generation))
}

/// Commits the installation of a moved capability into a reserved handle slot under lock.
///
/// Invariant I-CAP-MOVE-3:
/// - Uses the preflight-reserved capability_id explicitly.
/// - MUST NOT call allocate_capability_id().
/// Invariant I-CAP-MOVE-1:
/// - Inherits parent linkage from source node.
/// Invariant I-CAP-MOVE-2:
/// - Reparents all existing descendants of source node to destination node.
pub unsafe fn install_moved_capability_locked(
    dst_pslot: usize,
    dst_hslot: usize,
    src_pslot: usize,
    src_hslot: usize,
    reserved_capability_id: u64,
) -> Result<Handle, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(dst_pslot < MAX_PROCESSES && dst_hslot < MAX_HANDLES_PER_PROCESS);
    assert!(src_pslot < MAX_PROCESSES && src_hslot < MAX_HANDLES_PER_PROCESS);

    let src_entry = &PROCESS_HANDLE_TABLES[src_pslot].entries[src_hslot];
    assert!(src_entry.occupied);
    let src_node = &CAPABILITY_NODE_TABLE[src_pslot][src_hslot];
    assert!(!src_node.is_revoked);

    let obj_idx = src_entry.object_index as usize;
    let obj_gen = src_entry.object_generation;
    let rights = src_entry.rights;
    let endpoint = src_entry.endpoint;
    let parent_pslot = src_node.parent_pslot;
    let parent_hslot = src_node.parent_hslot;
    let parent_gen = src_node.parent_gen;
    let depth = src_node.derivation_depth;
    let flags = src_node.flags;
    let first_child_p = src_node.first_child_pslot;
    let first_child_h = src_node.first_child_hslot;

    // 1. Populate destination entry
    let dst_htable = &mut PROCESS_HANDLE_TABLES[dst_pslot];
    let dst_entry = &mut dst_htable.entries[dst_hslot];
    assert!(!dst_entry.occupied);

    dst_entry.occupied = true;
    dst_entry.object_index = obj_idx as u16;
    dst_entry.object_generation = obj_gen;
    dst_entry.rights = rights;
    dst_entry.endpoint = endpoint;
    let dst_gen = dst_entry.handle_generation;

    // 2. Populate destination node with preflight-reserved ID (I-CAP-MOVE-3)
    let dst_node = &mut CAPABILITY_NODE_TABLE[dst_pslot][dst_hslot];
    dst_node.capability_id = reserved_capability_id;
    dst_node.parent_pslot = parent_pslot;
    dst_node.parent_hslot = parent_hslot;
    dst_node.parent_gen = parent_gen;
    dst_node.derivation_depth = depth;
    dst_node.is_revoked = false;
    dst_node.flags = flags;
    dst_node.first_child_pslot = first_child_p;
    dst_node.first_child_hslot = first_child_h;
    dst_node.next_sibling_pslot = 0xFF;
    dst_node.next_sibling_hslot = 0xFF;

    // 3. Invariant I-CAP-MOVE-1: Splicing into parent's child list
    if parent_pslot != 0xFF && parent_hslot != 0xFF {
        replace_child_locked(
            parent_pslot as usize,
            parent_hslot as usize,
            src_pslot,
            src_hslot,
            dst_pslot,
            dst_hslot,
        );
    }

    // 4. Invariant I-CAP-MOVE-2: Reparent all existing children of src to dst
    let mut child_p = first_child_p as usize;
    let mut child_h = first_child_h as usize;
    while child_p < MAX_PROCESSES && child_h < MAX_HANDLES_PER_PROCESS {
        let child = &mut CAPABILITY_NODE_TABLE[child_p][child_h];
        child.parent_pslot = dst_pslot as u8;
        child.parent_hslot = dst_hslot as u8;
        child.parent_gen = dst_gen;
        child_p = child.next_sibling_pslot as usize;
        child_h = child.next_sibling_hslot as usize;
    }

    // 5. Invalidate source entry and node
    let src_htable = &mut PROCESS_HANDLE_TABLES[src_pslot];
    let src_entry_mut = &mut src_htable.entries[src_hslot];
    src_entry_mut.occupied = false;
    src_entry_mut.handle_generation = src_entry_mut.handle_generation.wrapping_add(1);
    if src_entry_mut.handle_generation == 0 {
        src_entry_mut.handle_generation = 1;
    }
    src_htable.count = src_htable.count.saturating_sub(1); // Invariant I-CAP-HANDLE-2
    CAPABILITY_NODE_TABLE[src_pslot][src_hslot] = CapabilityNode::empty();

    dst_htable.count += 1; // Invariant I-CAP-HANDLE-2

    Ok(Handle::new(dst_hslot as u16, dst_gen))
}

/// Derives a child capability from an existing handle in the current process.
pub fn capability_derive(
    src_handle: Handle,
    requested_rights: u16,
) -> Result<Handle, IpcError> {
    let cur_t = crate::task::percpu::current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = crate::ipc::handle::resolve_current_process_slot(cur_pid)?;

    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    let src_hslot = src_handle.slot_index();
    if src_hslot >= MAX_HANDLES_PER_PROCESS {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::InvalidHandle);
    }

    let dst_hslot = match unsafe { allocate_handle_slot_locked(proc_slot) } {
        Ok(s) => s,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    let res = unsafe {
        install_derived_capability_locked(
            proc_slot,
            dst_hslot,
            proc_slot,
            src_hslot,
            requested_rights,
        )
    };

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    res
}

/// Duplicates an existing handle with identical rights in the current process.
pub fn capability_duplicate(src_handle: Handle) -> Result<Handle, IpcError> {
    let cur_t = crate::task::percpu::current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = crate::ipc::handle::resolve_current_process_slot(cur_pid)?;

    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    let src_hslot = src_handle.slot_index();
    if src_hslot >= MAX_HANDLES_PER_PROCESS {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::InvalidHandle);
    }

    let src_entry = unsafe { &PROCESS_HANDLE_TABLES[proc_slot].entries[src_hslot] };
    if !src_entry.occupied || src_entry.handle_generation != src_handle.generation() {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::InvalidHandle);
    }
    let parent_rights = src_entry.rights;

    let dst_hslot = match unsafe { allocate_handle_slot_locked(proc_slot) } {
        Ok(s) => s,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    let res = unsafe {
        install_derived_capability_locked(
            proc_slot,
            dst_hslot,
            proc_slot,
            src_hslot,
            parent_rights,
        )
    };

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    res
}

/// Private internal primitive for unlinking and revoking a derivation subtree.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF = 0).
/// Lock hierarchy: KERNEL_OBJECT_TABLE_LOCK ≺ SCHEDULER.lock ≺ CPU (IF=0).
/// SCHEDULER.lock is acquired nested ONLY to wake blocked waiters, then released immediately.
pub unsafe fn revoke_descendants_tree_locked(
    pslot: usize,
    hslot: usize,
    pmm: &mut PhysicalMemoryManager,
) -> usize {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(pslot < MAX_PROCESSES && hslot < MAX_HANDLES_PER_PROCESS);

    let node = &mut CAPABILITY_NODE_TABLE[pslot][hslot];

    // Collect all descendants into a fixed stack
    let mut worklist = [(0u8, 0u8); 512];
    let mut work_len = 0;

    let mut curr_p = node.first_child_pslot;
    let mut curr_h = node.first_child_hslot;
    while curr_p != 0xFF && curr_h != 0xFF {
        if work_len < worklist.len() {
            worklist[work_len] = (curr_p, curr_h);
            work_len += 1;
        }
        let child = &CAPABILITY_NODE_TABLE[curr_p as usize][curr_h as usize];
        curr_p = child.next_sibling_pslot;
        curr_h = child.next_sibling_hslot;
    }

    // Detach all children from parent
    node.first_child_pslot = 0xFF;
    node.first_child_hslot = 0xFF;

    let mut revoked_count = 0;
    let mut idx = 0;
    while idx < work_len {
        let (p, h) = worklist[idx];
        let p_idx = p as usize;
        let h_idx = h as usize;
        idx += 1;

        if p_idx >= MAX_PROCESSES || h_idx >= MAX_HANDLES_PER_PROCESS {
            continue;
        }

        let c_node = &mut CAPABILITY_NODE_TABLE[p_idx][h_idx];
        let mut sub_p = c_node.first_child_pslot;
        let mut sub_h = c_node.first_child_hslot;
        while sub_p != 0xFF && sub_h != 0xFF {
            if work_len < worklist.len() {
                worklist[work_len] = (sub_p, sub_h);
                work_len += 1;
            }
            let next_child = &CAPABILITY_NODE_TABLE[sub_p as usize][sub_h as usize];
            sub_p = next_child.next_sibling_pslot;
            sub_h = next_child.next_sibling_hslot;
        }

        // Revoke this descendant node
        c_node.is_revoked = true;
        c_node.first_child_pslot = 0xFF;
        c_node.first_child_hslot = 0xFF;
        c_node.next_sibling_pslot = 0xFF;
        c_node.next_sibling_hslot = 0xFF;

        let htable = &mut PROCESS_HANDLE_TABLES[p_idx];
        let entry = &mut htable.entries[h_idx];
        if entry.occupied {
            entry.occupied = false;
            entry.handle_generation = entry.handle_generation.wrapping_add(1);
            if entry.handle_generation == 0 {
                entry.handle_generation = 1;
            }
            htable.count = htable.count.saturating_sub(1); // Invariant I-CAP-HANDLE-2

            let obj_idx = entry.object_index as usize;
            let endpoint = entry.endpoint;

            if obj_idx < MAX_KERNEL_OBJECTS {
                let obj_slot = &mut KERNEL_OBJECT_TABLE[obj_idx];
                if obj_slot.occupied {
                    obj_slot.header.handle_refs = obj_slot.header.handle_refs.saturating_sub(1);

                    if obj_slot.obj_type == KernelObjectType::Channel {
                        let ch_idx = obj_slot.pool_index as usize;
                        let channel = &mut crate::ipc::channel::CHANNEL_TABLE[ch_idx];
                        let (my_ep, peer_ep) = if endpoint == 0 {
                            (&mut channel.endpoint_0, &mut channel.endpoint_1)
                        } else {
                            (&mut channel.endpoint_1, &mut channel.endpoint_0)
                        };

                        my_ep.handle_refs = my_ep.handle_refs.saturating_sub(1);

                        // Wake blocked waiters under SCHEDULER.lock
                        let sched_rflags = crate::task::scheduler::SCHEDULER.lock.acquire();
                        while let Some(w) = my_ep.waiters_rx.pop_highest_locked() {
                            crate::task::scheduler::wake_thread_locked(w);
                        }
                        while let Some(w) = my_ep.waiters_tx.pop_highest_locked() {
                            crate::task::scheduler::wake_thread_locked(w);
                        }
                        while let Some(w) = peer_ep.waiters_rx.pop_highest_locked() {
                            crate::task::scheduler::wake_thread_locked(w);
                        }
                        while let Some(w) = peer_ep.waiters_tx.pop_highest_locked() {
                            crate::task::scheduler::wake_thread_locked(w);
                        }
                        crate::task::scheduler::SCHEDULER.lock.unlock_restore(sched_rflags);

                        crate::ipc::channel::check_and_reclaim_channel_locked(obj_idx);
                    } else if obj_slot.obj_type == KernelObjectType::ShmObject {
                        crate::ipc::shm::check_and_reclaim_shm_locked(obj_idx, pmm);
                    }
                }
            }
            revoked_count += 1;
        }
    }

    revoked_count
}

/// User-authorized revocation. Requires caller to possess cap_rights::REVOKE.
///
/// Invariant I-CAP-Y: Target capability C remains LIVE; only descendants are revoked.
pub unsafe fn capability_revoke_descendants_locked(
    pslot: usize,
    hslot: usize,
    pmm: &mut PhysicalMemoryManager,
) -> Result<usize, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(pslot < MAX_PROCESSES && hslot < MAX_HANDLES_PER_PROCESS);

    let entry = &PROCESS_HANDLE_TABLES[pslot].entries[hslot];
    if !entry.occupied {
        return Err(IpcError::InvalidHandle);
    }
    let node = &CAPABILITY_NODE_TABLE[pslot][hslot];
    if node.is_revoked {
        return Err(IpcError::CapabilityRevoked);
    }

    // Caller MUST hold REVOKE right
    if (entry.rights & cap_rights::REVOKE) == 0 {
        return Err(IpcError::PermissionDenied);
    }

    let count = revoke_descendants_tree_locked(pslot, hslot, pmm);
    Ok(count)
}

/// Relinquishes a capability, reparenting live descendants under Option B.
///
/// Invariant I-CAP-CLOSE-1:
/// - Closing C unlinks C from the CDT.
/// - Reparents direct children of C to C's parent (or promotes them to roots if C was root).
/// - Decrements object handle_refs and clears slot C.
pub unsafe fn capability_close_locked(
    process_slot: usize,
    handle_slot: usize,
    pmm: &mut PhysicalMemoryManager,
) -> Result<(usize, u8), IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(process_slot < MAX_PROCESSES && handle_slot < MAX_HANDLES_PER_PROCESS);

    let htable = &mut PROCESS_HANDLE_TABLES[process_slot];
    let entry = &mut htable.entries[handle_slot];
    if !entry.occupied {
        return Err(IpcError::InvalidHandle);
    }

    let obj_idx = entry.object_index as usize;
    let endpoint = entry.endpoint;

    // Invalidate HandleEntry
    entry.occupied = false;
    entry.handle_generation = entry.handle_generation.wrapping_add(1);
    if entry.handle_generation == 0 {
        entry.handle_generation = 1;
    }
    htable.count = htable.count.saturating_sub(1); // Invariant I-CAP-HANDLE-2

    let node = &mut CAPABILITY_NODE_TABLE[process_slot][handle_slot];
    let parent_p = node.parent_pslot;
    let parent_h = node.parent_hslot;
    let parent_gen = node.parent_gen;
    let first_child_p = node.first_child_pslot;
    let first_child_h = node.first_child_hslot;

    // Option B Grandparent Reparenting (I-CAP-CLOSE-1)
    if parent_p != 0xFF && parent_h != 0xFF {
        let parent_p_idx = parent_p as usize;
        let parent_h_idx = parent_h as usize;

        // Unlink C from parent's child list
        remove_child_locked(parent_p_idx, parent_h_idx, process_slot, handle_slot);

        let parent_depth = CAPABILITY_NODE_TABLE[parent_p_idx][parent_h_idx].derivation_depth;

        // Reparent all direct children of C to C's parent
        let mut curr_p = first_child_p as usize;
        let mut curr_h = first_child_h as usize;
        let mut last_child_p = 0xFF;
        let mut last_child_h = 0xFF;

        while curr_p < MAX_PROCESSES && curr_h < MAX_HANDLES_PER_PROCESS {
            let child = &mut CAPABILITY_NODE_TABLE[curr_p][curr_h];
            child.parent_pslot = parent_p;
            child.parent_hslot = parent_h;
            child.parent_gen = parent_gen;
            child.derivation_depth = parent_depth.saturating_add(1);

            last_child_p = curr_p;
            last_child_h = curr_h;
            curr_p = child.next_sibling_pslot as usize;
            curr_h = child.next_sibling_hslot as usize;
        }

        // Splice C's children into parent's child list
        if last_child_p != 0xFF && last_child_h != 0xFF {
            let parent_node = &mut CAPABILITY_NODE_TABLE[parent_p_idx][parent_h_idx];
            let old_first_p = parent_node.first_child_pslot;
            let old_first_h = parent_node.first_child_hslot;

            let last_child = &mut CAPABILITY_NODE_TABLE[last_child_p][last_child_h];
            last_child.next_sibling_pslot = old_first_p;
            last_child.next_sibling_hslot = old_first_h;

            parent_node.first_child_pslot = first_child_p;
            parent_node.first_child_hslot = first_child_h;
        }
    } else {
        // C was a root capability: promote direct children to independent roots
        let mut curr_p = first_child_p as usize;
        let mut curr_h = first_child_h as usize;
        while curr_p < MAX_PROCESSES && curr_h < MAX_HANDLES_PER_PROCESS {
            let child = &mut CAPABILITY_NODE_TABLE[curr_p][curr_h];
            child.parent_pslot = 0xFF;
            child.parent_hslot = 0xFF;
            child.parent_gen = 0;
            child.derivation_depth = 0;
            curr_p = child.next_sibling_pslot as usize;
            curr_h = child.next_sibling_hslot as usize;
        }
    }

    // Reset capability node
    *node = CapabilityNode::empty();

    // Decrement object handle_refs
    if obj_idx < MAX_KERNEL_OBJECTS {
        let obj_slot = &mut KERNEL_OBJECT_TABLE[obj_idx];
        if obj_slot.occupied {
            obj_slot.header.handle_refs = obj_slot.header.handle_refs.saturating_sub(1);
        }
    }

    Ok((obj_idx, endpoint))
}

/// Closes a capability and performs full object-level endpoint and memory reclamation.
///
/// Used by process lifecycle teardown and generic handle destruction.
pub unsafe fn close_and_reclaim_capability_locked(
    process_slot: usize,
    handle_slot: usize,
    pmm: &mut PhysicalMemoryManager,
) -> Result<(), IpcError> {
    let (obj_idx, endpoint) = capability_close_locked(process_slot, handle_slot, pmm)?;
    if obj_idx < MAX_KERNEL_OBJECTS {
        let obj_slot = &mut KERNEL_OBJECT_TABLE[obj_idx];
        if obj_slot.occupied {
            if obj_slot.obj_type == KernelObjectType::Channel {
                let ch_idx = obj_slot.pool_index as usize;
                let channel = &mut crate::ipc::channel::CHANNEL_TABLE[ch_idx];
                let (my_ep, peer_ep) = if endpoint == 0 {
                    (&mut channel.endpoint_0, &mut channel.endpoint_1)
                } else {
                    (&mut channel.endpoint_1, &mut channel.endpoint_0)
                };

                my_ep.handle_refs = my_ep.handle_refs.saturating_sub(1);
                if my_ep.handle_refs == 0 {
                    my_ep.is_closed = true;
                    peer_ep.peer_closed = true;
                    obj_slot.header.signals |= crate::ipc::types::signals::PEER_CLOSED;

                    let sched_rflags = crate::task::scheduler::SCHEDULER.lock.acquire();
                    while let Some(w) = my_ep.waiters_rx.pop_highest_locked() {
                        crate::task::scheduler::wake_thread_locked(w);
                    }
                    while let Some(w) = my_ep.waiters_tx.pop_highest_locked() {
                        crate::task::scheduler::wake_thread_locked(w);
                    }
                    while let Some(w) = peer_ep.waiters_rx.pop_highest_locked() {
                        crate::task::scheduler::wake_thread_locked(w);
                    }
                    while let Some(w) = peer_ep.waiters_tx.pop_highest_locked() {
                        crate::task::scheduler::wake_thread_locked(w);
                    }
                    crate::task::scheduler::SCHEDULER.lock.unlock_restore(sched_rflags);
                }
                crate::ipc::channel::check_and_reclaim_channel_locked(obj_idx);
            } else if obj_slot.obj_type == KernelObjectType::ShmObject {
                crate::ipc::shm::check_and_reclaim_shm_locked(obj_idx, pmm);
            }
        }
    }
    Ok(())
}

/// Cleans up all SHM mappings and capabilities for a terminating process.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF = 0).
/// Enforces invariants: I-CAP-TEARDOWN-1, I-CAP-TEARDOWN-2, I-CAP-TEARDOWN-3, I-CAP-HANDLE-2.
/// Narrow table access pattern avoids overlapping mutable borrows across capability_close_locked.
pub unsafe fn process_exit_capability_cleanup_locked(
    process_slot: usize,
    pid: u64,
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(process_slot < MAX_PROCESSES);

    // 1. Sweep and unmap SHM mappings owned by this process
    for i in 0..crate::ipc::shm::MAX_SHM_MAPPINGS {
        let m = &mut crate::ipc::shm::SHM_MAPPING_TABLE[i];
        if m.occupied && m.process_id == pid {
            let page_count = m.page_count;
            let vbase = m.virtual_base;
            let obj_idx = m.object_index as usize;

            for p in 0..page_count {
                let page_vaddr = vbase + (p as u64) * 4096;
                if let Ok(page) = crate::mm::vmm::Page::from_start_address(
                    crate::mm::vmm::VirtualAddress::new(page_vaddr),
                    crate::mm::vmm::get_active_geometry(),
                ) {
                    let _ = vmm.unmap_page(page, pmm);
                }
            }

            m.occupied = false;
            let obj_slot = &mut crate::ipc::KERNEL_OBJECT_TABLE[obj_idx];
            obj_slot.header.mapping_refs = obj_slot.header.mapping_refs.saturating_sub(1);
            crate::ipc::shm::check_and_reclaim_shm_locked(obj_idx, pmm);
        }
    }

    // 2. Reclaim capabilities owned strictly by this process slot (I-CAP-TEARDOWN-3)
    // Narrow per-iteration access avoids overlapping mutable borrows
    for slot_idx in 0..MAX_HANDLES_PER_PROCESS {
        let occupied = PROCESS_HANDLE_TABLES[process_slot].entries[slot_idx].occupied;
        if occupied {
            // Relinquishes capability, reparenting peer descendants (I-CAP-TEARDOWN-2)
            let _ = close_and_reclaim_capability_locked(process_slot, slot_idx, pmm);
        }
    }

    // 3. Postcondition Verification (I-CAP-TEARDOWN-1 & I-CAP-HANDLE-2)
    let htable = &PROCESS_HANDLE_TABLES[process_slot];
    assert_eq!(htable.count, 0, "Teardown invariant failed: htable.count != 0");
    for slot_idx in 0..MAX_HANDLES_PER_PROCESS {
        assert!(!htable.entries[slot_idx].occupied, "Teardown invariant failed: entry occupied");
        let node = &CAPABILITY_NODE_TABLE[process_slot][slot_idx];
        assert_eq!(node.capability_id, 0, "Teardown invariant failed: node ID nonzero");
        assert_eq!(node.first_child_pslot, 0xFF, "Teardown invariant failed: node has child");
    }
}

/// Validates a capability for the specified process slot under lock.
pub unsafe fn validate_capability_locked(
    process_slot_index: usize,
    handle: Handle,
    required_rights: u16,
) -> Result<(usize, u8, u16), IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(process_slot_index < MAX_PROCESSES);

    let slot_idx = handle.slot_index();
    if slot_idx >= MAX_HANDLES_PER_PROCESS {
        return Err(IpcError::InvalidHandle);
    }

    let htable = &PROCESS_HANDLE_TABLES[process_slot_index];
    let entry = &htable.entries[slot_idx];

    if entry.handle_generation != handle.generation() {
        return Err(IpcError::BadHandleGeneration);
    }
    if !entry.occupied {
        return Err(IpcError::InvalidHandle);
    }

    let node = &CAPABILITY_NODE_TABLE[process_slot_index][slot_idx];
    if node.is_revoked {
        return Err(IpcError::CapabilityRevoked);
    }

    let obj_idx = entry.object_index as usize;
    if obj_idx >= MAX_KERNEL_OBJECTS {
        return Err(IpcError::InvalidHandle);
    }

    let obj_slot = &KERNEL_OBJECT_TABLE[obj_idx];
    if !obj_slot.occupied || obj_slot.generation != entry.object_generation {
        return Err(IpcError::BadHandleGeneration);
    }

    if required_rights != 0 && (entry.rights & required_rights) != required_rights {
        return Err(IpcError::PermissionDenied);
    }

    Ok((obj_idx, entry.endpoint, entry.rights))
}
