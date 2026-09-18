//! Project Zero - Capability Derivation Tree (CDT) Node Table & Registry
//!
//! Authoritative Contract: Stage 3H Architecture Rev3 & Implementation Plan Rev6.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::ipc::types::IpcError;
use crate::cap::types::{CapabilityNode, MAX_PROCESSES, MAX_HANDLES_PER_PROCESS};

/// Authoritative Capability Node Registry in .bss (16 * 32 * 24 = 12,288 bytes = 12 KiB).
pub static mut CAPABILITY_NODE_TABLE: [[CapabilityNode; MAX_HANDLES_PER_PROCESS]; MAX_PROCESSES] =
    [[CapabilityNode::empty(); MAX_HANDLES_PER_PROCESS]; MAX_PROCESSES];

/// Monotonically increasing capability identifier generator.
/// Invariant I-CAP-ID-1: Never wraps, terminates deterministically at u64::MAX.
pub static NEXT_CAPABILITY_ID: AtomicU64 = AtomicU64::new(1);

/// Allocates a unique monotonic 64-bit Capability ID with a deterministic terminal state.
///
/// Invariant I-CAP-ID-1:
/// - If NEXT_CAPABILITY_ID == u64::MAX, returns Err(CapabilityIdExhausted).
/// - Counter never wraps to 0 or reuses IDs across kernel lifetime.
pub fn allocate_capability_id() -> Result<u64, IpcError> {
    let mut current = NEXT_CAPABILITY_ID.load(Ordering::Relaxed);
    loop {
        if current == u64::MAX {
            return Err(IpcError::CapabilityIdExhausted);
        }
        let next = current + 1;
        match NEXT_CAPABILITY_ID.compare_exchange_weak(
            current,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(allocated_id) => return Ok(allocated_id),
            Err(actual) => current = actual,
        }
    }
}

/// Locates a CapabilityNode by its global capability_id via bounded scan.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF = 0).
/// Invariant I-CAP-ID-2: Complexity is bounded constant-time O(1) (<= 512 iterations).
pub unsafe fn lookup_capability_by_id_locked(
    target_id: u64,
) -> Result<(usize, usize, &'static mut CapabilityNode), IpcError> {
    if target_id == 0 {
        return Err(IpcError::InvalidCapability);
    }
    for p in 0..MAX_PROCESSES {
        for h in 0..MAX_HANDLES_PER_PROCESS {
            let handle_entry = &crate::ipc::handle::PROCESS_HANDLE_TABLES[p].entries[h];
            if handle_entry.occupied {
                let node = &mut CAPABILITY_NODE_TABLE[p][h];
                if !node.is_revoked && node.capability_id == target_id {
                    return Ok((p, h, node));
                }
            }
        }
    }
    Err(IpcError::InvalidCapability)
}

/// Slices a new child capability node into the parent's child chain.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF = 0).
pub unsafe fn add_child_locked(
    parent_p: usize,
    parent_h: usize,
    child_p: usize,
    child_h: usize,
) {
    assert!(crate::ipc::KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(parent_p < MAX_PROCESSES && parent_h < MAX_HANDLES_PER_PROCESS);
    assert!(child_p < MAX_PROCESSES && child_h < MAX_HANDLES_PER_PROCESS);

    let parent = &mut CAPABILITY_NODE_TABLE[parent_p][parent_h];
    let old_first_p = parent.first_child_pslot;
    let old_first_h = parent.first_child_hslot;

    let child = &mut CAPABILITY_NODE_TABLE[child_p][child_h];
    child.next_sibling_pslot = old_first_p;
    child.next_sibling_hslot = old_first_h;

    parent.first_child_pslot = child_p as u8;
    parent.first_child_hslot = child_h as u8;
}

/// Unlinks a child capability node from its parent's child chain.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF = 0).
pub unsafe fn remove_child_locked(
    parent_p: usize,
    parent_h: usize,
    child_p: usize,
    child_h: usize,
) {
    assert!(crate::ipc::KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(parent_p < MAX_PROCESSES && parent_h < MAX_HANDLES_PER_PROCESS);
    assert!(child_p < MAX_PROCESSES && child_h < MAX_HANDLES_PER_PROCESS);

    let parent = &mut CAPABILITY_NODE_TABLE[parent_p][parent_h];
    if parent.first_child_pslot == child_p as u8 && parent.first_child_hslot == child_h as u8 {
        let child = &CAPABILITY_NODE_TABLE[child_p][child_h];
        parent.first_child_pslot = child.next_sibling_pslot;
        parent.first_child_hslot = child.next_sibling_hslot;
    } else {
        let mut curr_p = parent.first_child_pslot as usize;
        let mut curr_h = parent.first_child_hslot as usize;
        while curr_p < MAX_PROCESSES && curr_h < MAX_HANDLES_PER_PROCESS {
            let curr = &CAPABILITY_NODE_TABLE[curr_p][curr_h];
            let next_p = curr.next_sibling_pslot as usize;
            let next_h = curr.next_sibling_hslot as usize;
            if next_p == child_p && next_h == child_h {
                let child = &CAPABILITY_NODE_TABLE[child_p][child_h];
                let curr_mut = &mut CAPABILITY_NODE_TABLE[curr_p][curr_h];
                curr_mut.next_sibling_pslot = child.next_sibling_pslot;
                curr_mut.next_sibling_hslot = child.next_sibling_hslot;
                break;
            }
            curr_p = next_p;
            curr_h = next_h;
        }
    }

    let child = &mut CAPABILITY_NODE_TABLE[child_p][child_h];
    child.next_sibling_pslot = 0xFF;
    child.next_sibling_hslot = 0xFF;
}

/// Replaces an old child capability node with a new child capability node in parent's sibling chain.
///
/// SAFETY: Caller MUST hold KERNEL_OBJECT_TABLE_LOCK (IF = 0).
pub unsafe fn replace_child_locked(
    parent_p: usize,
    parent_h: usize,
    old_p: usize,
    old_h: usize,
    new_p: usize,
    new_h: usize,
) {
    assert!(crate::ipc::KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(parent_p < MAX_PROCESSES && parent_h < MAX_HANDLES_PER_PROCESS);
    assert!(old_p < MAX_PROCESSES && old_h < MAX_HANDLES_PER_PROCESS);
    assert!(new_p < MAX_PROCESSES && new_h < MAX_HANDLES_PER_PROCESS);

    let parent = &mut CAPABILITY_NODE_TABLE[parent_p][parent_h];
    if parent.first_child_pslot == old_p as u8 && parent.first_child_hslot == old_h as u8 {
        let old_node = &CAPABILITY_NODE_TABLE[old_p][old_h];
        let new_node = &mut CAPABILITY_NODE_TABLE[new_p][new_h];
        new_node.next_sibling_pslot = old_node.next_sibling_pslot;
        new_node.next_sibling_hslot = old_node.next_sibling_hslot;
        parent.first_child_pslot = new_p as u8;
        parent.first_child_hslot = new_h as u8;
    } else {
        let mut curr_p = parent.first_child_pslot as usize;
        let mut curr_h = parent.first_child_hslot as usize;
        while curr_p < MAX_PROCESSES && curr_h < MAX_HANDLES_PER_PROCESS {
            let curr = &CAPABILITY_NODE_TABLE[curr_p][curr_h];
            let next_p = curr.next_sibling_pslot as usize;
            let next_h = curr.next_sibling_hslot as usize;
            if next_p == old_p && next_h == old_h {
                let old_node = &CAPABILITY_NODE_TABLE[old_p][old_h];
                let new_node = &mut CAPABILITY_NODE_TABLE[new_p][new_h];
                new_node.next_sibling_pslot = old_node.next_sibling_pslot;
                new_node.next_sibling_hslot = old_node.next_sibling_hslot;

                let curr_mut = &mut CAPABILITY_NODE_TABLE[curr_p][curr_h];
                curr_mut.next_sibling_pslot = new_p as u8;
                curr_mut.next_sibling_hslot = new_h as u8;
                break;
            }
            curr_p = next_p;
            curr_h = next_h;
        }
    }
}
