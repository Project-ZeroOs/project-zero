//! ZeroOS - Stage 4A Machine Verification Suite
//!
//! Authoritative Contract: Stage 4 Architecture Rev6 & Phase 4A Implementation Plan Rev2.
//! Sequentially executes tests 4A-A through 4A-L (12 machine verification tests).

use crate::kprintln;
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;

use libzero::broker::{ServiceName, OP_REGISTER_SERVICE};
use libzero::error::ZeroError;
use libzero::registry::{ServiceDirectoryState, ServiceRegistry};
use libzero::supervisor::{ServiceLifecycleState, Supervisor, DEFAULT_MAX_RETRIES};

use crate::cap::ops::capability_derive;
use crate::cap::types::cap_rights;
use crate::ipc::channel::{channel_close, channel_create, channel_receive, channel_send};
use crate::ipc::handle::{resolve_current_process_slot, Handle};
use crate::ipc::types::{signals, IpcError, IpcMessage as KernelIpcMessage};

#[inline(always)]
fn to_libzero_msg(msg: &KernelIpcMessage) -> libzero::ipc::IpcMessage {
    unsafe { core::mem::transmute(*msg) }
}

#[inline(always)]
fn to_kernel_msg(msg: &libzero::ipc::IpcMessage) -> KernelIpcMessage {
    unsafe { core::mem::transmute(*msg) }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn run_stage4a_verification(pmm: &mut PhysicalMemoryManager, _vmm: &mut ActivePageTable) {
    kprintln!("\n[Stage 4A: Core System Service Runtime & Capability Directory Verification]");

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Test 4A-A: User Init Boot
    // ====================================================================
    let mut supervisor = Supervisor::new();
    {
        // 1. Initial supervisor state
        assert_eq!(supervisor.next_service_id, 1);

        // 2. Declare a test service
        let test_name = ServiceName::from_str("test_service");
        let svc_idx = supervisor
            .declare_service(test_name.as_bytes(), DEFAULT_MAX_RETRIES)
            .expect("declare_service must succeed");

        assert_eq!(
            supervisor.services[svc_idx].state,
            ServiceLifecycleState::Declared,
            "Service must start in Declared state"
        );

        // 3. Transition: Declared -> Starting -> Running
        supervisor
            .transition_starting(svc_idx)
            .expect("transition_starting must succeed");
        assert_eq!(
            supervisor.services[svc_idx].state,
            ServiceLifecycleState::Starting
        );

        supervisor
            .transition_running(svc_idx)
            .expect("transition_running must succeed");
        assert_eq!(
            supervisor.services[svc_idx].state,
            ServiceLifecycleState::Running
        );

        kprintln!("  [Test 4A-A: User Init Boot]: PASS");
    }

    // ====================================================================
    // Test 4A-B: Broker Startup
    // ====================================================================
    let mut registry = ServiceRegistry::new();
    let broker_idx = {
        // Init supervisor declares and starts brokerd
        let broker_name = ServiceName::from_str("brokerd");
        let idx = supervisor
            .declare_service(broker_name.as_bytes(), DEFAULT_MAX_RETRIES)
            .expect("brokerd declare must succeed");

        supervisor
            .transition_starting(idx)
            .expect("brokerd starting must succeed");
        supervisor
            .transition_running(idx)
            .expect("brokerd running must succeed");
        assert_eq!(supervisor.services[idx].state, ServiceLifecycleState::Running);

        // Verify brokerd directory state initialization
        assert_eq!(registry.next_service_id, 100, "ServiceId must start decoupled at 100");
        for entry in registry.entries.iter() {
            assert!(!entry.occupied, "Initial registry entries must be empty");
        }

        kprintln!("  [Test 4A-B: Broker Startup]: PASS");
        idx
    };

    // ====================================================================
    // Test 4A-C: Service Registration
    // ====================================================================
    let (srv_rx, srv_tx) = channel_create().expect("channel_create for service");
    let storage_name = ServiceName::from_str("com.zero.storage");
    let (storage_service_id, gen1) = {
        let req_rights = cap_rights::CHANNEL_SEND | cap_rights::CHANNEL_RECEIVE;
        let res = registry.register(storage_name.as_bytes(), srv_rx.0, req_rights);
        assert!(res.is_ok(), "Service registration must succeed");
        let (s_id, gen) = res.unwrap();
        assert_eq!(s_id, 100, "First service ID must be 100");
        assert_eq!(gen, 1, "Initial generation must be 1");

        let entry = registry.lookup(storage_name.as_bytes()).expect("lookup must succeed");
        assert_eq!(entry.service_id, 100);
        assert_eq!(entry.generation, 1);
        assert_eq!(entry.state, ServiceDirectoryState::Active);
        assert_eq!(entry.broker_local_endpoint, srv_rx.0);

        kprintln!("  [Test 4A-C: Service Registration]: PASS");
        (s_id, gen)
    };

    // ====================================================================
    // Test 4A-D: Service Discovery
    // ====================================================================
    {
        let entry = registry
            .lookup(storage_name.as_bytes())
            .expect("Service lookup must succeed");
        assert_eq!(entry.service_id, storage_service_id);
        assert_eq!(entry.generation, gen1);
        assert_eq!(entry.broker_local_endpoint, srv_rx.0);

        // Non-existent service returns NotFound
        let fake_name = ServiceName::from_str("com.zero.nonexistent");
        let res = registry.lookup(fake_name.as_bytes());
        assert_eq!(res, Err(ZeroError::NotFound), "Non-existent service must return NotFound");

        kprintln!("  [Test 4A-D: Service Discovery]: PASS");
    }

    // ====================================================================
    // Test 4A-E: IPC Rendezvous
    // ====================================================================
    {
        // Client sends PING to service via srv_tx
        let ping_msg = KernelIpcMessage::new(0x4001, b"PING_STAGE4A").expect("new msg");
        channel_send(srv_tx, &ping_msg, false).expect("channel_send ping");

        // Service receives message on srv_rx
        let rec_msg = channel_receive(srv_rx, false).expect("channel_receive ping");
        assert_eq!(rec_msg.tag, 0x4001);
        assert_eq!(rec_msg.payload_len, 12);
        assert_eq!(&rec_msg.payload[0..12], b"PING_STAGE4A");

        // Service sends PONG reply back via srv_rx
        let pong_msg = KernelIpcMessage::new(0x4002, b"PONG_STAGE4A").expect("new msg");
        channel_send(srv_rx, &pong_msg, false).expect("channel_send pong");

        // Client receives PONG reply on srv_tx
        let rec_reply = channel_receive(srv_tx, false).expect("channel_receive pong");
        assert_eq!(rec_reply.tag, 0x4002);
        assert_eq!(rec_reply.payload_len, 12);
        assert_eq!(&rec_reply.payload[0..12], b"PONG_STAGE4A");

        kprintln!("  [Test 4A-E: IPC Rendezvous]: PASS");
    }

    // ====================================================================
    // Test 4A-F: Capability Delegation
    // ====================================================================
    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid).expect("proc_slot");

    let (cap_parent, cap_derived) = {
        // Parent channel has DUPLICATE | READ | WRITE
        let (p_rx, p_tx) = channel_create().expect("channel_create for cap test");
        let _ = channel_close(p_tx);

        // Derive child with attenuated rights: READ only (0x0001)
        let requested_rights = cap_rights::CHANNEL_RECEIVE; // 0x0001
        let child_res = capability_derive(p_rx, requested_rights);
        assert!(child_res.is_ok(), "capability_derive must succeed with subset rights");
        let child_h = child_res.unwrap();

        // Verify child rights ⊆ parent rights
        let rflags = crate::ipc::KERNEL_OBJECT_TABLE_LOCK.acquire();
        let (obj_idx, endpoint, rights) = unsafe {
            crate::cap::ops::validate_capability_locked(proc_slot, child_h, requested_rights)
                .expect("derived cap must be valid")
        };
        crate::ipc::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        assert_eq!(rights, requested_rights, "Derived rights must exactly match requested subset");
        assert_ne!(obj_idx, usize::MAX);
        assert_eq!(endpoint, 0);

        kprintln!("  [Test 4A-F: Capability Delegation]: PASS");
        (p_rx, child_h)
    };

    // ====================================================================
    // Test 4A-G: Capability Amplification Rejection
    // ====================================================================
    {
        // cap_derived has only CHANNEL_RECEIVE (0x0001).
        // Attempting to derive CHANNEL_SEND (0x0002) or without DUPLICATE right must FAIL.
        let illegal_rights = cap_rights::CHANNEL_RECEIVE | cap_rights::CHANNEL_SEND;
        let res = capability_derive(cap_derived, illegal_rights);
        assert!(
            res.is_err(),
            "Attempting to amplify rights without DUPLICATE or exceeding parent must FAIL"
        );
        match res {
            Err(IpcError::RightsAmplificationRejected) | Err(IpcError::CapabilityNotDuplicable) => {
                // Expected kernel authoritative rejection
            }
            other => panic!("Unexpected derivation result: {:?}", other),
        }

        // Clean up derived capabilities
        let _ = channel_close(cap_derived);
        let _ = channel_close(cap_parent);

        kprintln!("  [Test 4A-G: Capability Amplification Rejection]: PASS");
    }

    // ====================================================================
    // Test 4A-H: Service Restart & Endpoint Invariant
    // ====================================================================
    let (srv2_rx, srv2_tx) = {
        // 1. Simulate service crash
        let _ = supervisor.handle_failure(broker_idx);
        assert_eq!(
            supervisor.services[broker_idx].state,
            ServiceLifecycleState::Restarting
        );

        // 2. Service restarts with a new endpoint channel
        let (new_rx, new_tx) = channel_create().expect("new channel for restart");

        // Close old srv_rx and srv_tx
        let _ = channel_close(srv_rx);
        let _ = channel_close(srv_tx);

        // Re-register existing service name with broker
        let req_rights = cap_rights::CHANNEL_SEND | cap_rights::CHANNEL_RECEIVE;
        let (s_id2, gen2) = registry
            .register(storage_name.as_bytes(), new_rx.0, req_rights)
            .expect("re-registration must succeed");

        assert_eq!(s_id2, storage_service_id, "ServiceId must remain stable across restart");
        assert_eq!(gen2, 2, "Generation must advance to 2");
        assert_ne!(gen1, gen2, "Generation 1 must not equal Generation 2");

        let entry = registry.lookup(storage_name.as_bytes()).expect("lookup");
        assert_eq!(entry.generation, 2);
        assert_eq!(entry.broker_local_endpoint, new_rx.0);
        assert_ne!(
            entry.broker_local_endpoint, srv_rx.0,
            "Old endpoint handle must not be reused as service identity"
        );

        // Supervisor resumes service
        let _ = supervisor.transition_starting(broker_idx);
        let _ = supervisor.transition_running(broker_idx);
        assert_eq!(supervisor.services[broker_idx].state, ServiceLifecycleState::Running);

        kprintln!("  [Test 4A-H: Service Restart]: PASS");
        (new_rx, new_tx)
    };

    // ====================================================================
    // Test 4A-I: Fault Containment
    // ====================================================================
    {
        // Create ephemeral client channel
        let (cli_ep, srv_ep) = channel_create().expect("ephemeral channel");

        // Simulate sudden client crash by immediately closing client endpoint
        let _ = channel_close(cli_ep);

        // Service checks its endpoint: peer_closed signal must be asserted
        let rflags = crate::ipc::KERNEL_OBJECT_TABLE_LOCK.acquire();
        let srv_entry = unsafe { &crate::ipc::handle::PROCESS_HANDLE_TABLES[proc_slot].entries[srv_ep.slot_index()] };
        assert!(srv_entry.occupied);
        let obj_slot = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[srv_entry.object_index as usize] };
        assert!((obj_slot.header.signals & signals::PEER_CLOSED) != 0, "PEER_CLOSED must be asserted");
        crate::ipc::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

        // Broker registry remains fully operational
        let entry = registry.lookup(storage_name.as_bytes()).expect("lookup");
        assert_eq!(entry.service_id, storage_service_id);

        let _ = channel_close(srv_ep);

        kprintln!("  [Test 4A-I: Fault Containment]: PASS");
    }

    // ====================================================================
    // Test 4A-J: Malformed Request
    // ====================================================================
    {
        // 1. Unknown opcode 0x9999
        let bad_tag_msg = libzero::ipc::IpcMessage::new(0x9999, b"CORRUPT").unwrap();
        let resp1 = registry.handle_message(&bad_tag_msg);
        let err1 = i32::from_le_bytes([resp1.payload[0], resp1.payload[1], resp1.payload[2], resp1.payload[3]]);
        assert_eq!(err1, ZeroError::InvalidRequest.as_i32());

        // 2. OP_REGISTER_SERVICE with missing handles (payload only)
        let bad_reg_msg = libzero::ipc::IpcMessage::new(OP_REGISTER_SERVICE, b"SHORT").unwrap();
        let resp2 = registry.handle_message(&bad_reg_msg);
        let err2 = i32::from_le_bytes([resp2.payload[0], resp2.payload[1], resp2.payload[2], resp2.payload[3]]);
        assert_eq!(err2, ZeroError::InvalidRequest.as_i32());

        // Broker registry remains healthy
        let entry = registry.lookup(storage_name.as_bytes()).expect("lookup");
        assert_eq!(entry.generation, 2);

        kprintln!("  [Test 4A-J: Malformed Request]: PASS");
    }

    // ====================================================================
    // Test 4A-K: Service Identity Generation
    // ====================================================================
    {
        // Attempt unregister with stale generation 1 (current is 2)
        let unreg_stale = registry.unregister(storage_service_id, 1);
        assert_eq!(
            unreg_stale,
            Err(ZeroError::GenerationMismatch),
            "Stale generation token must be rejected with GenerationMismatch"
        );

        // Service remains active
        let entry = registry.lookup(storage_name.as_bytes()).expect("lookup");
        assert_eq!(entry.generation, 2);

        // Unregister with correct generation 2 succeeds
        let unreg_ok = registry.unregister(storage_service_id, 2);
        assert!(unreg_ok.is_ok(), "Unregister with valid generation must succeed");

        // Service is now gone
        assert_eq!(registry.lookup(storage_name.as_bytes()), Err(ZeroError::NotFound));

        // Clean up srv2 channels
        let _ = channel_close(srv2_rx);
        let _ = channel_close(srv2_tx);

        kprintln!("  [Test 4A-K: Service Identity Generation]: PASS");
    }

    // ====================================================================
    // Test 4A-L: Clean Shutdown
    // ====================================================================
    {
        // Supervisor initiates orderly shutdown
        supervisor.shutdown_all();
        for svc in supervisor.services.iter() {
            if svc.occupied {
                assert_eq!(svc.state, ServiceLifecycleState::Stopped);
            }
        }

        kprintln!("  [Test 4A-L: Shutdown]: PASS");
    }

    // ====================================================================
    // PMM Neutrality Audit
    // ====================================================================
    {
        let current_free = pmm.free_frame_count();
        assert_eq!(
            baseline_free, current_free,
            "PMM frame neutrality violated: baseline {} != current {}",
            baseline_free, current_free
        );
        kprintln!("  [Stage 4A: PMM Neutrality]: PASS");
    }

    kprintln!("[Stage 4A] ALL 12 TESTS PASSED. System Service Substrate VERIFIED.\n");
}
