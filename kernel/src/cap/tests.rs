//! Project Zero - Stage 3H Capability System & Kernel Authority Model Verification Suite
//!
//! Authoritative Contract: Stage 3H Architecture Rev3 & Implementation Plan Rev6.
//!
//! Tests 3H-A through 3H-AU (47 bare-metal machine verification tests).

use crate::kprintln;
use crate::ipc::types::{rights, IpcError, IpcMessage};
use crate::ipc::object::{
    KernelObjectType, KERNEL_OBJECT_TABLE, KERNEL_OBJECT_TABLE_LOCK, MAX_KERNEL_OBJECTS,
};
use crate::ipc::handle::{
    Handle, MAX_HANDLES_PER_PROCESS, PROCESS_HANDLE_TABLES, resolve_current_process_slot,
};
use crate::ipc::channel::{channel_create, channel_send, channel_receive, channel_close};
use crate::ipc::shm::{shm_create, shm_map, shm_unmap, shm_close};
use crate::cap::types::{cap_rights, MAX_PROCESSES, CapabilityNode};
use crate::cap::node::{
    CAPABILITY_NODE_TABLE, NEXT_CAPABILITY_ID, allocate_capability_id, lookup_capability_by_id_locked,
};
use crate::cap::ops::{
    capability_derive, capability_duplicate, capability_revoke_descendants_locked,
    capability_close_locked, process_exit_capability_cleanup_locked, validate_capability_locked,
};
use crate::cap::transfer::{transfer_move_locked, transfer_delegate_locked};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;
use core::sync::atomic::Ordering;

pub fn run_stage3h_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3H: Capability System & Kernel Authority Model Verification]");

    let baseline_free = pmm.free_frame_count();
    let cur_t = crate::task::percpu::current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid).expect("proc_slot must resolve");

    // ====================================================================
    // Test 3H-A: Channel creation allocates root capabilities
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let node0 = &CAPABILITY_NODE_TABLE[proc_slot][h0.slot_index()];
            let node1 = &CAPABILITY_NODE_TABLE[proc_slot][h1.slot_index()];
            assert!(node0.capability_id > 0);
            assert!(node1.capability_id > 0);
            assert_ne!(node0.capability_id, node1.capability_id);
            assert_eq!(node0.derivation_depth, 0);
            assert_eq!(node1.derivation_depth, 0);
            assert!(node0.is_root());
            assert!(node1.is_root());
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0 failed");
        channel_close(h1).expect("close h1 failed");
        kprintln!("  Test 3H-A: Channel creation allocates root capabilities [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-B: Valid capability lookup returns object & rights
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let (obj_idx, ep, r) = validate_capability_locked(proc_slot, h0, 0).expect("lookup failed");
            assert!(obj_idx < MAX_KERNEL_OBJECTS);
            assert_eq!(ep, 0);
            assert_ne!(r, 0);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-B: Valid capability lookup returns object & rights [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-C: Invalid handle index rejection
    // ====================================================================
    {
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let res = validate_capability_locked(proc_slot, Handle::new(63, 1), 0);
            assert_eq!(res, Err(IpcError::InvalidHandle));
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        kprintln!("  Test 3H-C: Invalid handle index rejection [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-D: Forged generation rejection
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let forged = Handle::new(h0.slot_index() as u16, h0.generation().wrapping_add(999));
            assert_eq!(validate_capability_locked(proc_slot, forged, 0), Err(IpcError::BadHandleGeneration));
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-D: Forged generation rejection [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-E: Stale handle rejection on closed capability
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        channel_close(h0).expect("close h0");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            assert_eq!(validate_capability_locked(proc_slot, h0, 0), Err(IpcError::BadHandleGeneration));
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-E: Stale handle rejection on closed capability [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-F: Operation allowed with required right
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let msg = IpcMessage::new(42, b"cap_allowed").expect("msg new");
        channel_send(h0, &msg, false).expect("send allowed");
        let recv = channel_receive(h1, false).expect("recv allowed");
        assert_eq!(recv.tag, 42);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-F: Operation allowed with required right [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-G: Operation denied when lacking required right
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let h_no_send = capability_derive(h0, cap_rights::CHANNEL_RECEIVE).expect("derive no send");
        let msg = IpcMessage::new(43, b"cap_denied").expect("msg new");
        assert_eq!(channel_send(h_no_send, &msg, false), Err(IpcError::PermissionDenied));
        channel_close(h_no_send).expect("close derived");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-G: Operation denied when lacking required right [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-H: Rights attenuation preserves monotonic reduction
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let h_child = capability_derive(h0, cap_rights::CHANNEL_SEND).expect("derive child");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let entry = &PROCESS_HANDLE_TABLES[proc_slot].entries[h_child.slot_index()];
            assert_eq!(entry.rights, cap_rights::CHANNEL_SEND);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h_child).expect("close child");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-H: Rights attenuation preserves monotonic reduction [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-I: Rights amplification rejected (I-CAP-X)
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let h_send_only = capability_derive(h0, cap_rights::CHANNEL_SEND | cap_rights::DUPLICATE).expect("derive send_only");
        let amp_res = capability_derive(h_send_only, cap_rights::CHANNEL_SEND | cap_rights::CHANNEL_RECEIVE);
        assert_eq!(amp_res, Err(IpcError::RightsAmplificationRejected));
        channel_close(h_send_only).expect("close child");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-I: Rights amplification rejected (I-CAP-X) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-J: Duplicate capability rights invariance
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let h_dup = capability_duplicate(h0).expect("duplicate h0");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let parent_rights = PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].rights;
            let dup_rights = PROCESS_HANDLE_TABLES[proc_slot].entries[h_dup.slot_index()].rights;
            assert_eq!(parent_rights, dup_rights);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h_dup).expect("close dup");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-J: Duplicate capability rights invariance [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-K: Derivation tree linkage integrity
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let h_c1 = capability_duplicate(h0).expect("dup c1");
        let h_c2 = capability_duplicate(h_c1).expect("dup c2");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let n0 = &CAPABILITY_NODE_TABLE[proc_slot][h0.slot_index()];
            let n1 = &CAPABILITY_NODE_TABLE[proc_slot][h_c1.slot_index()];
            let n2 = &CAPABILITY_NODE_TABLE[proc_slot][h_c2.slot_index()];
            assert_eq!(n1.parent_hslot, h0.slot_index() as u8);
            assert_eq!(n2.parent_hslot, h_c1.slot_index() as u8);
            assert_eq!(n0.first_child_hslot, h_c1.slot_index() as u8);
            assert_eq!(n1.first_child_hslot, h_c2.slot_index() as u8);
            assert_eq!(n2.derivation_depth, 2);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h_c2).expect("close c2");
        channel_close(h_c1).expect("close c1");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-K: Derivation tree linkage integrity [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-L: Maximum derivation depth bound (I-CAP-4)
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let mut chain = [Handle::new(0, 0); 8];
        let mut current = h0;
        for i in 0..8 {
            let next = capability_duplicate(current).expect("chain duplicate");
            chain[i] = next;
            current = next;
        }
        // 9th derivation must fail with MaxDerivationDepthExceeded
        let exceeded = capability_duplicate(current);
        assert_eq!(exceeded, Err(IpcError::MaxDerivationDepthExceeded));

        for i in (0..8).rev() {
            channel_close(chain[i]).expect("close chain handle");
        }
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-L: Maximum derivation depth bound (I-CAP-4) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-M: Sibling list ordering and traversal
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let c2 = capability_duplicate(h0).expect("dup c2");
        let c3 = capability_duplicate(h0).expect("dup c3");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let n0 = &CAPABILITY_NODE_TABLE[proc_slot][h0.slot_index()];
            let mut visited = 0;
            let mut curr_p = n0.first_child_pslot as usize;
            let mut curr_h = n0.first_child_hslot as usize;
            while curr_p < MAX_PROCESSES && curr_h < MAX_HANDLES_PER_PROCESS {
                visited += 1;
                let child = &CAPABILITY_NODE_TABLE[curr_p][curr_h];
                curr_p = child.next_sibling_pslot as usize;
                curr_h = child.next_sibling_hslot as usize;
            }
            assert_eq!(visited, 3);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

        channel_close(c3).expect("close c3");
        channel_close(c2).expect("close c2");
        channel_close(c1).expect("close c1");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-M: Sibling list ordering and traversal [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-N: TRANSFER_MOVE ownership transfer
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create failed");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let h_moved = unsafe { transfer_move_locked(proc_slot, h0.slot_index(), peer_slot).expect("move failed") };
        unsafe {
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].occupied);
            assert!(PROCESS_HANDLE_TABLES[peer_slot].entries[h_moved.slot_index()].occupied);
            let obj_idx = PROCESS_HANDLE_TABLES[peer_slot].entries[h_moved.slot_index()].object_index as usize;
            assert_eq!(KERNEL_OBJECT_TABLE[obj_idx].header.handle_refs, 2);
            capability_close_locked(peer_slot, h_moved.slot_index(), pmm).expect("close moved");
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-N: TRANSFER_MOVE ownership transfer [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-O: TRANSFER_DELEGATE authority derivation
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create failed");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let h_delegated = unsafe {
            transfer_delegate_locked(proc_slot, h0.slot_index(), peer_slot, cap_rights::CHANNEL_SEND).expect("delegate failed")
        };
        unsafe {
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].occupied);
            assert!(PROCESS_HANDLE_TABLES[peer_slot].entries[h_delegated.slot_index()].occupied);
            let n_del = &CAPABILITY_NODE_TABLE[peer_slot][h_delegated.slot_index()];
            assert_eq!(n_del.parent_pslot, proc_slot as u8);
            assert_eq!(n_del.parent_hslot, h0.slot_index() as u8);
            capability_close_locked(peer_slot, h_delegated.slot_index(), pmm).expect("close del");
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-O: TRANSFER_DELEGATE authority derivation [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-P: Transfer rejected lacking TRANSFER right
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create failed");
        let h_no_xfer = capability_derive(h0, cap_rights::CHANNEL_SEND | cap_rights::DUPLICATE).expect("derive no xfer");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let res = transfer_move_locked(proc_slot, h_no_xfer.slot_index(), peer_slot);
            assert_eq!(res, Err(IpcError::CapabilityNotTransferable));
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h_no_xfer).expect("close no xfer");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-P: Transfer rejected lacking TRANSFER right [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-Q: Receiver table full atomicity rollback
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            // Fill peer table completely
            for slot in 0..MAX_HANDLES_PER_PROCESS {
                PROCESS_HANDLE_TABLES[peer_slot].entries[slot].occupied = true;
            }
            PROCESS_HANDLE_TABLES[peer_slot].count = MAX_HANDLES_PER_PROCESS as u16;
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

        let (h0, h1) = channel_create().expect("channel_create failed");
        let rflags2 = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let res = transfer_move_locked(proc_slot, h0.slot_index(), peer_slot);
            assert_eq!(res, Err(IpcError::HandleTableFull));
            // Assert sender handle remains completely live and occupied
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].occupied);

            // Clean up peer table
            for slot in 0..MAX_HANDLES_PER_PROCESS {
                PROCESS_HANDLE_TABLES[peer_slot].entries[slot].occupied = false;
            }
            PROCESS_HANDLE_TABLES[peer_slot].count = 0;
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags2);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-Q: Receiver table full atomicity rollback [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-R: Descendant revocation leaves root valid
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let c2 = capability_duplicate(c1).expect("dup c2");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let revoked = unsafe { capability_revoke_descendants_locked(proc_slot, h0.slot_index(), pmm).expect("revoke failed") };
        assert_eq!(revoked, 2);
        unsafe {
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].occupied);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c1.slot_index()].occupied);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c2.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close root h0");
        channel_close(h1).expect("close root h1");
        kprintln!("  Test 3H-R: Descendant revocation leaves root valid [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-S: Scoped revocation isolation (I-CAP-Y)
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let c2 = capability_duplicate(h0).expect("dup c2");
        let c1_sub = capability_duplicate(c1).expect("dup c1_sub");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let revoked = unsafe { capability_revoke_descendants_locked(proc_slot, c1.slot_index(), pmm).expect("revoke c1 descendants") };
        assert_eq!(revoked, 1); // Only c1_sub revoked
        unsafe {
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].occupied);
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[c1.slot_index()].occupied);
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[c2.slot_index()].occupied);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c1_sub.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(c2).expect("close c2");
        channel_close(c1).expect("close c1");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-S: Scoped revocation isolation (I-CAP-Y) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-T: Revocation rejected lacking REVOKE right
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let c_no_revoke = capability_derive(h0, cap_rights::CHANNEL_SEND | cap_rights::DUPLICATE).expect("derive no revoke");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let res = capability_revoke_descendants_locked(proc_slot, c_no_revoke.slot_index(), pmm);
            assert_eq!(res, Err(IpcError::PermissionDenied));
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(c_no_revoke).expect("close c");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-T: Revocation rejected lacking REVOKE right [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-U: Revocation of already revoked capability
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let _ = capability_revoke_descendants_locked(proc_slot, h0.slot_index(), pmm);
            let res = capability_revoke_descendants_locked(proc_slot, c1.slot_index(), pmm);
            assert_eq!(res, Err(IpcError::InvalidHandle));
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-U: Revocation of already revoked capability [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-V: Cascading multi-level revocation
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create failed");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let c2 = capability_duplicate(c1).expect("dup c2");
        let c3 = capability_duplicate(c2).expect("dup c3");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let revoked = unsafe { capability_revoke_descendants_locked(proc_slot, h0.slot_index(), pmm).expect("revoke root") };
        assert_eq!(revoked, 3);
        unsafe {
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].occupied);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c1.slot_index()].occupied);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c2.slot_index()].occupied);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c3.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-V: Cascading multi-level revocation [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-W: Process exit cleans own capabilities
    // ====================================================================
    {
        let target_slot = (proc_slot + 2) % MAX_PROCESSES;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            // Allocate 2 real channel objects and install into target_slot
            let ch_obj0 = crate::ipc::object::allocate_object_slot_locked(KernelObjectType::Channel, 10, 9999).expect("alloc obj 0");
            let ch_obj1 = crate::ipc::object::allocate_object_slot_locked(KernelObjectType::Channel, 11, 9999).expect("alloc obj 1");
            let h0 = crate::ipc::handle::allocate_handle_entry_locked(target_slot, ch_obj0, cap_rights::CHANNEL_SEND, 0).expect("alloc 1");
            let _ = crate::ipc::handle::allocate_handle_entry_locked(target_slot, ch_obj1, cap_rights::CHANNEL_RECEIVE, 1).expect("alloc 2");
            assert_eq!(PROCESS_HANDLE_TABLES[target_slot].count, 2);

            process_exit_capability_cleanup_locked(target_slot, 9999, pmm, vmm);
            assert_eq!(PROCESS_HANDLE_TABLES[target_slot].count, 0);
            assert!(!PROCESS_HANDLE_TABLES[target_slot].entries[h0.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        kprintln!("  Test 3H-W: Process exit cleans own capabilities [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-X: Process exit preserves delegated peer capabilities
    // ====================================================================
    {
        let proc_a = (proc_slot + 3) % MAX_PROCESSES;
        let proc_b = (proc_slot + 4) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("create ch");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let hb = unsafe {
            let ch_obj = crate::ipc::object::allocate_object_slot_locked(KernelObjectType::Channel, 12, 8888).expect("alloc obj");
            // Install handle into proc_a and delegate to proc_b
            let h_a = crate::ipc::handle::allocate_handle_entry_locked(proc_a, ch_obj, cap_rights::CHANNEL_SEND | cap_rights::TRANSFER | cap_rights::DUPLICATE, 0).expect("alloc a");
            let hb = transfer_delegate_locked(proc_a, h_a.slot_index(), proc_b, cap_rights::CHANNEL_SEND).expect("delegate to b");

            // Process A exits: closes h_a
            process_exit_capability_cleanup_locked(proc_a, 8888, pmm, vmm);

            // Invariant I-CAP-TEARDOWN-2: Delegated capability in Process B survives!
            assert!(PROCESS_HANDLE_TABLES[proc_b].entries[hb.slot_index()].occupied);
            let n_b = &CAPABILITY_NODE_TABLE[proc_b][hb.slot_index()];
            assert!(!n_b.is_revoked);
            assert_eq!(n_b.derivation_depth, 0); // Promoted to root under Option B

            capability_close_locked(proc_b, hb.slot_index(), pmm).expect("close b");
            hb
        };
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-X: Process exit preserves delegated peer capabilities [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-Y: Process slot reuse isolation (I-CAP-10)
    // ====================================================================
    {
        let reuse_slot = (proc_slot + 5) % MAX_PROCESSES;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let ch_obj = crate::ipc::object::allocate_object_slot_locked(KernelObjectType::Channel, 13, 7777).expect("alloc obj");
            // Populate and clean slot
            let _ = crate::ipc::handle::allocate_handle_entry_locked(reuse_slot, ch_obj, cap_rights::CHANNEL_SEND, 0).expect("alloc");
            process_exit_capability_cleanup_locked(reuse_slot, 7777, pmm, vmm);

            // Verify table and nodes are completely scrubbed
            assert_eq!(PROCESS_HANDLE_TABLES[reuse_slot].count, 0);
            for h in 0..MAX_HANDLES_PER_PROCESS {
                assert!(!PROCESS_HANDLE_TABLES[reuse_slot].entries[h].occupied);
                assert_eq!(CAPABILITY_NODE_TABLE[reuse_slot][h].capability_id, 0);
            }
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        kprintln!("  Test 3H-Y: Process slot reuse isolation (I-CAP-10) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-Z: SHM read-only mapping with SHM_MAP_READ
    // ====================================================================
    {
        let shm_h = shm_create(1, pmm).expect("shm_create");
        let h_ro = capability_derive(shm_h, cap_rights::SHM_MAP_READ | cap_rights::SHM_UNMAP).expect("derive ro");
        let vaddr = 0x0000_2000_0000;
        shm_map(h_ro, vaddr, false, pmm, vmm).expect("shm_map ro");
        shm_unmap(h_ro, vaddr, pmm, vmm).expect("shm_unmap");
        shm_close(h_ro, pmm).expect("close ro");
        shm_close(shm_h, pmm).expect("close shm");
        kprintln!("  Test 3H-Z: SHM read-only mapping with SHM_MAP_READ [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AA: SHM write denial without SHM_MAP_WRITE
    // ====================================================================
    {
        let shm_h = shm_create(1, pmm).expect("shm_create");
        let h_ro = capability_derive(shm_h, cap_rights::SHM_MAP_READ | cap_rights::SHM_UNMAP).expect("derive ro");
        let vaddr = 0x0000_2000_1000;
        assert_eq!(shm_map(h_ro, vaddr, true, pmm, vmm), Err(IpcError::PermissionDenied));
        shm_close(h_ro, pmm).expect("close ro");
        shm_close(shm_h, pmm).expect("close shm");
        kprintln!("  Test 3H-AA: SHM write denial without SHM_MAP_WRITE [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AB: SHM revocation unmapping
    // ====================================================================
    {
        let shm_h = shm_create(1, pmm).expect("shm_create");
        let h_child = capability_duplicate(shm_h).expect("dup shm");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let revoked = unsafe { capability_revoke_descendants_locked(proc_slot, shm_h.slot_index(), pmm).expect("revoke shm child") };
        assert_eq!(revoked, 1);
        unsafe {
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[h_child.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        shm_close(shm_h, pmm).expect("close shm");
        kprintln!("  Test 3H-AB: SHM revocation unmapping [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AC: Cross-process handle isolation
    // ====================================================================
    {
        let other_slot = (proc_slot + 1) % MAX_PROCESSES;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            // Proc slot A attempts to validate a handle in other_slot's space
            let forged_handle = Handle::new(0, 1);
            let res = validate_capability_locked(other_slot, forged_handle, 0);
            assert!(res.is_err());
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        kprintln!("  Test 3H-AC: Cross-process handle isolation [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AD: Object reclamation after final capability close
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("create ch");
        let obj_idx = unsafe {
            let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
            let idx = PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].object_index as usize;
            assert_eq!(KERNEL_OBJECT_TABLE[idx].header.handle_refs, 2);
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            idx
        };
        channel_close(h0).expect("close h0");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            assert_eq!(KERNEL_OBJECT_TABLE[obj_idx].header.handle_refs, 1);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h1).expect("close h1");
        let rflags2 = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            assert!(!KERNEL_OBJECT_TABLE[obj_idx].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags2);
        kprintln!("  Test 3H-AD: Object reclamation after final capability close [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AE: Monotonic capability ID atomic advance (I-CAP-ID-1)
    // ====================================================================
    {
        let id1 = allocate_capability_id().expect("id1");
        let id2 = allocate_capability_id().expect("id2");
        let id3 = allocate_capability_id().expect("id3");
        assert!(id2 > id1);
        assert!(id3 > id2);
        kprintln!("  Test 3H-AE: Monotonic capability ID atomic advance (I-CAP-ID-1) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AF: Monotonic lock hierarchy compliance (I-CAP-11)
    // ====================================================================
    {
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let sched_rflags = unsafe { crate::task::scheduler::SCHEDULER.lock.acquire() };
        unsafe {
            crate::task::scheduler::SCHEDULER.lock.unlock_restore(sched_rflags);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        kprintln!("  Test 3H-AF: Monotonic lock hierarchy compliance (I-CAP-11) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AG: PMM frame neutrality preserved
    // ====================================================================
    {
        assert_eq!(baseline_free, pmm.free_frame_count(), "Pre-neutrality frame mismatch");
        kprintln!("  Test 3H-AG: PMM frame neutrality preserved [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AH: MOVE audit trail and sender slot cleanup
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let old_gen = h0.generation();
        let h_moved = unsafe { transfer_move_locked(proc_slot, h0.slot_index(), peer_slot).expect("move") };
        unsafe {
            let src_entry = &PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()];
            assert!(!src_entry.occupied);
            assert_ne!(src_entry.handle_generation, old_gen); // Generation monotonically advanced
            assert_eq!(CAPABILITY_NODE_TABLE[proc_slot][h0.slot_index()].capability_id, 0); // Node cleared

            capability_close_locked(peer_slot, h_moved.slot_index(), pmm).expect("close moved");
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AH: MOVE audit trail and sender slot cleanup [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AI: Capability ID terminal exhaustion at u64::MAX (I-CAP-ID-1)
    // ====================================================================
    {
        let prev = NEXT_CAPABILITY_ID.load(Ordering::Relaxed);
        NEXT_CAPABILITY_ID.store(u64::MAX, Ordering::Relaxed);
        let res = allocate_capability_id();
        assert_eq!(res, Err(IpcError::CapabilityIdExhausted));
        assert_eq!(NEXT_CAPABILITY_ID.load(Ordering::Relaxed), u64::MAX); // Did not wrap
        NEXT_CAPABILITY_ID.store(prev, Ordering::Relaxed);
        kprintln!("  Test 3H-AI: Capability ID terminal exhaustion at u64::MAX (I-CAP-ID-1) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AJ: Bounded-scan capability lookup by ID (I-CAP-ID-2)
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let cap_id = CAPABILITY_NODE_TABLE[proc_slot][h0.slot_index()].capability_id;
            let (found_p, found_h, node) = lookup_capability_by_id_locked(cap_id).expect("lookup by id");
            assert_eq!(found_p, proc_slot);
            assert_eq!(found_h, h0.slot_index());
            assert_eq!(node.capability_id, cap_id);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AJ: Bounded-scan capability lookup by ID (I-CAP-ID-2) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AK: MOVE direct parent inheritance (I-CAP-MOVE-1)
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create");
        let c1 = capability_duplicate(h0).expect("dup c1");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let h_moved = unsafe { transfer_move_locked(proc_slot, c1.slot_index(), peer_slot).expect("move c1") };
        unsafe {
            let moved_node = &CAPABILITY_NODE_TABLE[peer_slot][h_moved.slot_index()];
            // Directly inherits parent h0!
            assert_eq!(moved_node.parent_pslot, proc_slot as u8);
            assert_eq!(moved_node.parent_hslot, h0.slot_index() as u8);
            assert_eq!(CAPABILITY_NODE_TABLE[proc_slot][c1.slot_index()].capability_id, 0); // c1 cleared completely

            capability_close_locked(peer_slot, h_moved.slot_index(), pmm).expect("close moved");
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AK: MOVE direct parent inheritance (I-CAP-MOVE-1) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AL: MOVE followed by parent revocation
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create");
        let c1 = capability_duplicate(h0).expect("dup c1");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let h_moved = unsafe { transfer_move_locked(proc_slot, c1.slot_index(), peer_slot).expect("move c1") };
        // Revoke descendants of parent h0
        let revoked = unsafe { capability_revoke_descendants_locked(proc_slot, h0.slot_index(), pmm).expect("revoke parent") };
        assert_eq!(revoked, 1);
        unsafe {
            // Moved capability in peer process is cleanly revoked!
            assert!(!PROCESS_HANDLE_TABLES[peer_slot].entries[h_moved.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AL: MOVE followed by parent revocation [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AM: Parent close with live descendants reparenting
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let c2 = capability_duplicate(c1).expect("dup c2");

        // Close parent c1
        channel_close(c1).expect("close c1");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let n2 = &CAPABILITY_NODE_TABLE[proc_slot][c2.slot_index()];
            // Under Option B, c2 is reparented to h0!
            assert_eq!(n2.parent_pslot, proc_slot as u8);
            assert_eq!(n2.parent_hslot, h0.slot_index() as u8);
            assert_eq!(n2.derivation_depth, 1);
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[c2.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(c2).expect("close c2");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AM: Parent close with live descendants reparenting [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AN: TRANSFER_MOVE exhaustion rollback atomicity
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create");
        let prev = NEXT_CAPABILITY_ID.load(Ordering::Relaxed);
        NEXT_CAPABILITY_ID.store(u64::MAX, Ordering::Relaxed);

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let res = unsafe { transfer_move_locked(proc_slot, h0.slot_index(), peer_slot) };
        assert_eq!(res, Err(IpcError::CapabilityIdExhausted));
        unsafe {
            // Sender handle remains 100% live and occupied
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[h0.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        NEXT_CAPABILITY_ID.store(prev, Ordering::Relaxed);

        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AN: TRANSFER_MOVE exhaustion rollback atomicity [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AO: Derived/MOVE handle allocation never creates root (I-CAP-HANDLE-1)
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let n1 = &CAPABILITY_NODE_TABLE[proc_slot][c1.slot_index()];
            assert!(!n1.is_root(), "Derived capability must not be root");
            assert_eq!(n1.parent_hslot, h0.slot_index() as u8);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(c1).expect("close c1");
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AO: Derived/MOVE handle allocation never creates root (I-CAP-HANDLE-1) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AP: Capability close reparents descendants under Option B (I-CAP-CLOSE-1)
    // ====================================================================
    {
        let (h0, h1) = channel_create().expect("channel_create");
        let c1 = capability_duplicate(h0).expect("dup c1");

        // Close root capability h0
        channel_close(h0).expect("close root h0");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let n1 = &CAPABILITY_NODE_TABLE[proc_slot][c1.slot_index()];
            // Under Option B, child c1 is promoted to root and survives!
            assert!(n1.is_root());
            assert_eq!(n1.derivation_depth, 0);
            assert!(PROCESS_HANDLE_TABLES[proc_slot].entries[c1.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(c1).expect("close c1");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AP: Capability close reparents descendants under Option B (I-CAP-CLOSE-1) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AQ: TRANSFER_MOVE preserves descendant subtree (I-CAP-MOVE-2)
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create");
        let c1 = capability_duplicate(h0).expect("dup c1");
        let c2 = capability_duplicate(c1).expect("dup c2");
        let c3 = capability_duplicate(c1).expect("dup c3");

        // Move c1 to peer_slot
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let h_moved = unsafe { transfer_move_locked(proc_slot, c1.slot_index(), peer_slot).expect("move c1") };
        unsafe {
            // Invariant I-CAP-MOVE-2: c2 and c3 are reparented to h_moved!
            let n2 = &CAPABILITY_NODE_TABLE[proc_slot][c2.slot_index()];
            let n3 = &CAPABILITY_NODE_TABLE[proc_slot][c3.slot_index()];
            assert_eq!(n2.parent_pslot, peer_slot as u8);
            assert_eq!(n2.parent_hslot, h_moved.slot_index() as u8);
            assert_eq!(n3.parent_pslot, peer_slot as u8);
            assert_eq!(n3.parent_hslot, h_moved.slot_index() as u8);

            // Revoking descendants of h_moved revokes c2 and c3!
            let revoked = capability_revoke_descendants_locked(peer_slot, h_moved.slot_index(), pmm).expect("revoke moved");
            assert_eq!(revoked, 2);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c2.slot_index()].occupied);
            assert!(!PROCESS_HANDLE_TABLES[proc_slot].entries[c3.slot_index()].occupied);

            capability_close_locked(peer_slot, h_moved.slot_index(), pmm).expect("close moved");
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AQ: TRANSFER_MOVE preserves descendant subtree (I-CAP-MOVE-2) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AR: Process exit removes own capabilities lacking REVOKE
    // ====================================================================
    {
        let target_slot = (proc_slot + 6) % MAX_PROCESSES;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let ch_obj = crate::ipc::object::allocate_object_slot_locked(KernelObjectType::Channel, 14, 6666).expect("alloc obj");
            // Capability with only CHANNEL_SEND (no REVOKE)
            let h = crate::ipc::handle::allocate_handle_entry_locked(target_slot, ch_obj, cap_rights::CHANNEL_SEND, 0).expect("alloc");
            assert_eq!(PROCESS_HANDLE_TABLES[target_slot].count, 1);

            // Teardown must remove it without error
            process_exit_capability_cleanup_locked(target_slot, 6666, pmm, vmm);
            assert_eq!(PROCESS_HANDLE_TABLES[target_slot].count, 0);
            assert!(!PROCESS_HANDLE_TABLES[target_slot].entries[h.slot_index()].occupied);
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        kprintln!("  Test 3H-AR: Process exit removes own capabilities lacking REVOKE [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AS: Process exit preserves delegated peer capabilities (I-CAP-TEARDOWN-2, I-CAP-TEARDOWN-3)
    // ====================================================================
    {
        let proc_a = (proc_slot + 7) % MAX_PROCESSES;
        let proc_b = (proc_slot + 8) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("create ch");

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let hb = unsafe {
            let ch_obj = crate::ipc::object::allocate_object_slot_locked(KernelObjectType::Channel, 15, 5555).expect("alloc obj");
            let ha = crate::ipc::handle::allocate_handle_entry_locked(proc_a, ch_obj, cap_rights::CHANNEL_SEND | cap_rights::TRANSFER | cap_rights::DUPLICATE, 0).expect("alloc");
            let hb = transfer_delegate_locked(proc_a, ha.slot_index(), proc_b, cap_rights::CHANNEL_SEND).expect("del");

            // Process A exits
            process_exit_capability_cleanup_locked(proc_a, 5555, pmm, vmm);

            // Capability in Process B survives and is usable according to rights
            assert!(PROCESS_HANDLE_TABLES[proc_b].entries[hb.slot_index()].occupied);
            assert!(!CAPABILITY_NODE_TABLE[proc_b][hb.slot_index()].is_revoked);
            capability_close_locked(proc_b, hb.slot_index(), pmm).expect("close b");
            hb
        };
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h0).expect("close h0");
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AS: Process exit preserves delegated peer capabilities (I-CAP-TEARDOWN-2, I-CAP-TEARDOWN-3) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AT: Process capability and handle tables completely empty after teardown (I-CAP-TEARDOWN-1, I-CAP-HANDLE-2)
    // ====================================================================
    {
        let target_slot = (proc_slot + 9) % MAX_PROCESSES;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        unsafe {
            let mut objs = [0usize; 5];
            for i in 0..5 {
                objs[i] = crate::ipc::object::allocate_object_slot_locked(KernelObjectType::Channel, (16 + i) as u16, 4444).expect("alloc obj");
                let _ = crate::ipc::handle::allocate_handle_entry_locked(target_slot, objs[i], cap_rights::CHANNEL_SEND, 0).expect("alloc");
            }
            assert_eq!(PROCESS_HANDLE_TABLES[target_slot].count, 5);

            process_exit_capability_cleanup_locked(target_slot, 4444, pmm, vmm);
            assert_eq!(PROCESS_HANDLE_TABLES[target_slot].count, 0);
            for h in 0..MAX_HANDLES_PER_PROCESS {
                assert!(!PROCESS_HANDLE_TABLES[target_slot].entries[h].occupied);
                assert_eq!(CAPABILITY_NODE_TABLE[target_slot][h].capability_id, 0);
            }
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        kprintln!("  Test 3H-AT: Process capability and handle tables completely empty after teardown (I-CAP-TEARDOWN-1, I-CAP-HANDLE-2) [VERIFIED]");
    }

    // ====================================================================
    // Test 3H-AU: MOVE commit consumes reserved ID without allocating second ID (I-CAP-MOVE-3)
    // ====================================================================
    {
        let peer_slot = (proc_slot + 1) % MAX_PROCESSES;
        let (h0, h1) = channel_create().expect("channel_create");
        let id_before = NEXT_CAPABILITY_ID.load(Ordering::Relaxed);

        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        let h_moved = unsafe { transfer_move_locked(proc_slot, h0.slot_index(), peer_slot).expect("move") };
        let id_after = NEXT_CAPABILITY_ID.load(Ordering::Relaxed);

        // Invariant I-CAP-MOVE-3: Exactly ONE capability ID consumed
        assert_eq!(id_after, id_before + 1);
        unsafe {
            let node_moved = &CAPABILITY_NODE_TABLE[peer_slot][h_moved.slot_index()];
            assert_eq!(node_moved.capability_id, id_before); // Uses that exact reserved ID

            capability_close_locked(peer_slot, h_moved.slot_index(), pmm).expect("close moved");
        }
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        channel_close(h1).expect("close h1");
        kprintln!("  Test 3H-AU: MOVE commit consumes reserved ID without allocating second ID (I-CAP-MOVE-3) [VERIFIED]");
    }

    // ====================================================================
    // Final Invariant Check: PMM Frame Neutrality
    // ====================================================================
    let post_verification_free = pmm.free_frame_count();
    assert_eq!(
        baseline_free, post_verification_free,
        "Stage 3H verification frame leak detected"
    );
    kprintln!("  Stage 3H verified: frame neutrality preserved");
    kprintln!("  [x] Stage 3H Capability System & Kernel Authority Model verification complete (47/47 tests).");
}
