//! Project Zero - Stage 3H Capability Transfer & Delegation Subsystem
//!
//! Authoritative Contract: Stage 3H Architecture Rev3 & Implementation Plan Rev6.

use crate::ipc::types::IpcError;
use crate::ipc::handle::{Handle, MAX_HANDLES_PER_PROCESS, PROCESS_HANDLE_TABLES};
use crate::ipc::KERNEL_OBJECT_TABLE_LOCK;
use crate::cap::types::{cap_rights, MAX_PROCESSES, MAX_DERIVATION_DEPTH};
use crate::cap::node::{CAPABILITY_NODE_TABLE, allocate_capability_id};
use crate::cap::ops::{
    allocate_handle_slot_locked, install_moved_capability_locked, install_derived_capability_locked,
};

/// Transfers ownership of a capability from source process to destination process under lock.
///
/// Preflight Stage:
/// - Asserts sender holds TRANSFER right.
/// - Pre-allocates destination handle slot.
/// - Pre-allocates reserved capability ID (Invariant I-CAP-MOVE-3).
/// - If preflight fails, sender capability remains 100% untouched and live.
///
/// Commit Stage:
/// - Infallible installation via install_moved_capability_locked with reserved_id.
pub unsafe fn transfer_move_locked(
    src_pslot: usize,
    src_hslot: usize,
    dst_pslot: usize,
) -> Result<Handle, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(src_pslot < MAX_PROCESSES && src_hslot < MAX_HANDLES_PER_PROCESS);
    assert!(dst_pslot < MAX_PROCESSES);

    let src_entry = &PROCESS_HANDLE_TABLES[src_pslot].entries[src_hslot];
    if !src_entry.occupied {
        return Err(IpcError::InvalidHandle);
    }
    let src_node = &CAPABILITY_NODE_TABLE[src_pslot][src_hslot];
    if src_node.is_revoked {
        return Err(IpcError::CapabilityRevoked);
    }

    // Require TRANSFER right
    if (src_entry.rights & (cap_rights::TRANSFER | crate::ipc::types::rights::TRANSFER)) == 0 {
        return Err(IpcError::CapabilityNotTransferable);
    }

    // Preflight 1: Reserve empty handle slot in destination process
    let dst_hslot = allocate_handle_slot_locked(dst_pslot)?;

    // Preflight 2: Reserve unique capability ID (Invariant I-CAP-MOVE-3)
    let reserved_id = allocate_capability_id()?;

    // Commit: Infallible state transfer using reserved_id
    install_moved_capability_locked(
        dst_pslot,
        dst_hslot,
        src_pslot,
        src_hslot,
        reserved_id,
    )
}

/// Delegates authority by deriving a child capability in the destination process under lock.
///
/// Preflight Stage:
/// - Asserts sender holds BOTH TRANSFER and DUPLICATE rights.
/// - Validates rights attenuation (requested_rights ⊆ src_rights).
/// - Asserts derivation depth < MAX_DERIVATION_DEPTH.
/// - Pre-allocates destination handle slot.
///
/// Commit Stage:
/// - Installs child capability linked to sender in the CDT.
pub unsafe fn transfer_delegate_locked(
    src_pslot: usize,
    src_hslot: usize,
    dst_pslot: usize,
    requested_rights: u16,
) -> Result<Handle, IpcError> {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(src_pslot < MAX_PROCESSES && src_hslot < MAX_HANDLES_PER_PROCESS);
    assert!(dst_pslot < MAX_PROCESSES);

    let src_entry = &PROCESS_HANDLE_TABLES[src_pslot].entries[src_hslot];
    if !src_entry.occupied {
        return Err(IpcError::InvalidHandle);
    }
    let src_node = &CAPABILITY_NODE_TABLE[src_pslot][src_hslot];
    if src_node.is_revoked {
        return Err(IpcError::CapabilityRevoked);
    }

    // Require BOTH TRANSFER and DUPLICATE rights to delegate
    if (src_entry.rights & (cap_rights::TRANSFER | crate::ipc::types::rights::TRANSFER)) == 0 {
        return Err(IpcError::CapabilityNotTransferable);
    }
    if (src_entry.rights & (cap_rights::DUPLICATE | crate::ipc::types::rights::DUPLICATE)) == 0 {
        return Err(IpcError::CapabilityNotDuplicable);
    }

    // Invariant I-CAP-X: Monotonic Rights Attenuation
    if (requested_rights & !src_entry.rights) != 0 {
        return Err(IpcError::RightsAmplificationRejected);
    }

    // Invariant I-CAP-4: Bounded Derivation Depth
    if src_node.derivation_depth >= MAX_DERIVATION_DEPTH {
        return Err(IpcError::MaxDerivationDepthExceeded);
    }

    // Preflight: Reserve empty handle slot in destination process
    let dst_hslot = allocate_handle_slot_locked(dst_pslot)?;

    // Commit: Install derived child capability
    install_derived_capability_locked(
        dst_pslot,
        dst_hslot,
        src_pslot,
        src_hslot,
        requested_rights,
    )
}
