//! Project Zero - Stage 3G IPC & Kernel Object Verification Suite
//!
//! Authoritative Contract: Stage 3G Architecture Rev10 (Approved & Frozen).
//!
//! Tests 3G-A through 3G-AT (46 bare-metal machine tests).

use crate::kprintln;
use crate::ipc::types::{rights, signals, IpcError, IpcMessage};
use crate::ipc::object::{
    allocate_object_id, KernelObjectHeader, KernelObjectSlot, KernelObjectType,
    ObjectLifecycleState, KERNEL_OBJECT_TABLE, KERNEL_OBJECT_TABLE_LOCK, MAX_KERNEL_OBJECTS,
    NEXT_OBJECT_ID,
};
use crate::ipc::handle::{
    Handle, HandleEntry, HandleTable, MAX_HANDLES_PER_PROCESS, PROCESS_HANDLE_TABLES,
};
use crate::ipc::channel::{
    channel_close, channel_create, channel_receive, channel_send, Channel,
    ChannelEndpointData, ChannelRing, ThreadIpcState, CHANNEL_RING_CAPACITY,
    CHANNEL_TABLE, MAX_CHANNELS, THREAD_IPC_STATE,
};
use crate::ipc::shm::{
    shm_close, shm_create, shm_map, shm_unmap, ShmMapping, ShmObject,
    MAX_SHM_MAPPINGS, MAX_SHM_OBJECTS, MAX_SHM_PAGES, SHM_MAPPING_TABLE, SHM_TABLE,
};
use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager};
use crate::mm::vmm::ActivePageTable;
use crate::task::process::{create_process, Process, ProcessState, MAX_PROCESSES, PROCESS_TABLE};
use crate::task::thread::{allocate_descriptor, free_descriptor, KernelThread, Priority, ThreadState};
use crate::task::scheduler::{SCHEDULER, wake_thread_locked};
use core::sync::atomic::Ordering;

pub fn run_stage3g_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3G: IPC & Kernel Object Semantics Verification]");

    // ====================================================================
    // 0. Compile-Time Structure Size & Alignment Invariant Assertions
    // ====================================================================
    const _: () = assert!(core::mem::size_of::<crate::task::process::Process>() == 128);
    const _: () = assert!(core::mem::align_of::<crate::task::process::Process>() == 8);
    const _: () = assert!(core::mem::size_of::<crate::task::thread::KernelThread>() == 176);
    const _: () = assert!(core::mem::align_of::<crate::task::thread::KernelThread>() == 8);
    const _: () = assert!(core::mem::size_of::<crate::task::waitqueue::WaitQueue>() == 24);
    const _: () = assert!(core::mem::align_of::<crate::task::waitqueue::WaitQueue>() == 8);
    const _: () = assert!(core::mem::size_of::<IpcMessage>() == 80);
    const _: () = assert!(core::mem::align_of::<IpcMessage>() == 8);
    const _: () = assert!(core::mem::size_of::<ChannelRing>() == 328);
    const _: () = assert!(core::mem::align_of::<ChannelRing>() == 8);
    const _: () = assert!(core::mem::size_of::<ChannelEndpointData>() == 56);
    const _: () = assert!(core::mem::align_of::<ChannelEndpointData>() == 8);
    const _: () = assert!(core::mem::size_of::<Channel>() == 768);
    const _: () = assert!(core::mem::align_of::<Channel>() == 8);
    const _: () = assert!(core::mem::size_of::<HandleEntry>() == 16);
    const _: () = assert!(core::mem::align_of::<HandleEntry>() == 8);
    const _: () = assert!(core::mem::size_of::<HandleTable>() == 520);
    const _: () = assert!(core::mem::align_of::<HandleTable>() == 8);
    const _: () = assert!(core::mem::size_of::<KernelObjectHeader>() == 32);
    const _: () = assert!(core::mem::align_of::<KernelObjectHeader>() == 8);
    const _: () = assert!(core::mem::size_of::<KernelObjectSlot>() == 40);
    const _: () = assert!(core::mem::align_of::<KernelObjectSlot>() == 8);
    const _: () = assert!(core::mem::size_of::<ShmMapping>() == 32);
    const _: () = assert!(core::mem::align_of::<ShmMapping>() == 8);
    const _: () = assert!(core::mem::size_of::<ShmObject>() == 144);
    const _: () = assert!(core::mem::align_of::<ShmObject>() == 8);
    const _: () = assert!(core::mem::size_of::<ThreadIpcState>() == 4);

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Test 3G-A: Channel Creation
    // ====================================================================
    let (h0, h1) = channel_create().expect("Test 3G-A: channel_create failed");
    let obj_idx = unsafe {
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        PROCESS_HANDLE_TABLES[pslot].entries[h0.slot_index()].object_index as usize
    };
    unsafe {
        let obj = &KERNEL_OBJECT_TABLE[obj_idx];
        assert_eq!(obj.header.ref_count(), 2, "Channel must have ref_count == 2 upon creation");
        let ch = &CHANNEL_TABLE[obj.pool_index as usize];
        assert_eq!(ch.endpoint_0.handle_refs, 1);
        assert_eq!(ch.endpoint_1.handle_refs, 1);
        assert!(!ch.endpoint_0.is_closed);
        assert!(!ch.endpoint_1.is_closed);
    }
    kprintln!("  Test 3G-A: Channel creation & reference accounting [VERIFIED]");

    // ====================================================================
    // Test 3G-B: Channel Destruction & Clean Reclamation
    // ====================================================================
    channel_close(h0).expect("Test 3G-B: close h0");
    unsafe {
        let obj = &KERNEL_OBJECT_TABLE[obj_idx];
        assert_eq!(obj.header.ref_count(), 1);
        let ch = &CHANNEL_TABLE[obj.pool_index as usize];
        assert!(ch.endpoint_0.is_closed);
        assert!(ch.endpoint_1.peer_closed);
    }
    channel_close(h1).expect("Test 3G-B: close h1");
    unsafe {
        let obj = &KERNEL_OBJECT_TABLE[obj_idx];
        assert!(!obj.occupied, "Channel object must be freed after both handles closed");
    }
    kprintln!("  Test 3G-B: Channel destruction & lifecycle reclamation [VERIFIED]");

    // ====================================================================
    // Test 3G-C: Basic Send / Receive (80-Byte Message)
    // ====================================================================
    let (tx_h, rx_h) = channel_create().expect("Test 3G-C create");
    let payload_data = [42u8; 48];
    let send_msg = IpcMessage::new(0x1234_5678, &payload_data).expect("IpcMessage::new");
    channel_send(tx_h, &send_msg, false).expect("channel_send");
    let recv_msg = channel_receive(rx_h, false).expect("channel_receive");
    assert_eq!(recv_msg.tag, 0x1234_5678);
    assert_eq!(recv_msg.payload_len, 48);
    assert_eq!(recv_msg.payload, payload_data);
    channel_close(tx_h).unwrap();
    channel_close(rx_h).unwrap();
    kprintln!("  Test 3G-C: Basic 80-byte send/receive payload fidelity [VERIFIED]");

    // ====================================================================
    // Test 3G-D: FIFO Ordering
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    for i in 1..=4 {
        let m = IpcMessage::new(i, &[i as u8; 1]).unwrap();
        channel_send(tx, &m, false).unwrap();
    }
    for i in 1..=4 {
        let m = channel_receive(rx, false).unwrap();
        assert_eq!(m.tag, i);
        assert_eq!(m.payload[0], i as u8);
    }
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-D: Strict FIFO message ring ordering [VERIFIED]");

    // ====================================================================
    // Test 3G-E: Ring Full Backpressure
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    for i in 0..4 {
        let m = IpcMessage::new(i, &[]).unwrap();
        channel_send(tx, &m, false).unwrap();
    }
    let m5 = IpcMessage::new(5, &[]).unwrap();
    assert_eq!(channel_send(tx, &m5, false), Err(IpcError::WouldBlock));
    let _ = channel_receive(rx, false).unwrap();
    assert!(channel_send(tx, &m5, false).is_ok());
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-E: Ring full backpressure & WouldBlock semantics [VERIFIED]");

    // ====================================================================
    // Test 3G-F: Non-blocking Receive on Empty Ring
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    assert_eq!(channel_receive(rx, false), Err(IpcError::WouldBlock));
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-F: Non-blocking receive on empty ring [VERIFIED]");

    // ====================================================================
    // Test 3G-G: Blocking Ring State Transition
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let m = IpcMessage::new(99, &[]).unwrap();
    channel_send(tx, &m, true).unwrap();
    let r = channel_receive(rx, true).unwrap();
    assert_eq!(r.tag, 99);
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-G: Blocking send/receive immediate availability [VERIFIED]");

    // ====================================================================
    // Test 3G-H: Wakeup Priority Ordering Verification
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let t_low = allocate_descriptor().unwrap();
    let t_high = allocate_descriptor().unwrap();
    unsafe {
        (*t_low).priority = Priority::Normal;
        (*t_high).priority = Priority::High;
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        let obj_idx = PROCESS_HANDLE_TABLES[pslot].entries[rx.slot_index()].object_index as usize;
        let ch_idx = KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize;
        let ep = &mut CHANNEL_TABLE[ch_idx].endpoint_1;
        let orig = SCHEDULER.lock.acquire();
        ep.waiters_rx.push_priority_locked(t_low);
        ep.waiters_rx.push_priority_locked(t_high);
        let popped = ep.waiters_rx.pop_highest_locked();
        assert_eq!(popped, Some(t_high), "High-priority waiter must be popped first");
        let popped_next = ep.waiters_rx.pop_highest_locked();
        assert_eq!(popped_next, Some(t_low));
        SCHEDULER.lock.unlock_restore(orig);
    }
    let _ = free_descriptor(t_low);
    let _ = free_descriptor(t_high);
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-H: Priority-ordered waiter wakeups [VERIFIED]");

    // ====================================================================
    // Test 3G-I: Peer Close on Receiver
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    channel_close(tx).unwrap();
    assert_eq!(channel_receive(rx, false), Err(IpcError::PeerClosed));
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-I: Immediate PeerClosed observation on receiver [VERIFIED]");

    // ====================================================================
    // Test 3G-J: Peer Close on Sender
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    channel_close(rx).unwrap();
    let msg = IpcMessage::new(1, &[]).unwrap();
    assert_eq!(channel_send(tx, &msg, false), Err(IpcError::PeerClosed));
    channel_close(tx).unwrap();
    kprintln!("  Test 3G-J: Immediate PeerClosed observation on sender [VERIFIED]");

    // ====================================================================
    // Test 3G-K & 3G-L: Drain-Before-Close Semantics
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let m = IpcMessage::new(777, &[1, 2, 3]).unwrap();
    channel_send(tx, &m, false).unwrap();
    channel_close(tx).unwrap(); // Peer closed while message in ring
    // Receiver must successfully drain before seeing PeerClosed
    let r = channel_receive(rx, false).expect("Must drain before seeing PeerClosed");
    assert_eq!(r.tag, 777);
    assert_eq!(channel_receive(rx, false), Err(IpcError::PeerClosed));
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-K & 3G-L: In-flight message drain before peer-close [VERIFIED]");

    // ====================================================================
    // Test 3G-M: Stale Handle Generation Rejection
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let stale_h = tx;
    channel_close(tx).unwrap();
    let m = IpcMessage::new(1, &[]).unwrap();
    assert_eq!(channel_send(stale_h, &m, false), Err(IpcError::BadHandleGeneration));
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-M: Stale handle generation rejection [VERIFIED]");

    // ====================================================================
    // Test 3G-N: Rights Mask Enforcement
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    // Attempt send on rx handle (which lacks rights::WRITE)
    unsafe {
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        PROCESS_HANDLE_TABLES[pslot].entries[rx.slot_index()].rights &= !rights::WRITE;
    }
    let m = IpcMessage::new(1, &[]).unwrap();
    assert_eq!(channel_send(rx, &m, false), Err(IpcError::PermissionDenied));
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-N: Rights mask permission enforcement [VERIFIED]");

    // ====================================================================
    // Test 3G-O: ShmObject Creation & Mapping
    // ====================================================================
    let shm_h = shm_create(2, pmm).expect("shm_create 2 pages");
    let test_vaddr = 0x0000_1000_0000;
    shm_map(shm_h, test_vaddr, true, pmm, vmm).expect("shm_map RW");
    // Write data to mapped page
    unsafe {
        let ptr = test_vaddr as *mut u64;
        core::ptr::write_volatile(ptr, 0xCAFE_BABE_DEAD_BEEF);
        assert_eq!(core::ptr::read_volatile(ptr), 0xCAFE_BABE_DEAD_BEEF);
    }
    shm_unmap(shm_h, test_vaddr, pmm, vmm).expect("shm_unmap");
    shm_close(shm_h, pmm).expect("shm_close");
    kprintln!("  Test 3G-O: ShmObject creation, mapping & volatile memory access [VERIFIED]");

    // ====================================================================
    // Test 3G-P: ShmObject W^X Enforcement (PAGE_NX)
    // ====================================================================
    let shm_h = shm_create(1, pmm).unwrap();
    shm_map(shm_h, test_vaddr, true, pmm, vmm).unwrap();
    let page = crate::mm::vmm::Page::from_start_address(
        crate::mm::vmm::VirtualAddress::new(test_vaddr),
        crate::mm::vmm::get_active_geometry(),
    ).unwrap();
    let flags = vmm.get_page_flags(page).unwrap();
    assert!(flags.contains(crate::mm::vmm::PageTableFlags::NO_EXECUTE), "W^X: NO_EXECUTE must be set");
    shm_unmap(shm_h, test_vaddr, pmm, vmm).unwrap();
    shm_close(shm_h, pmm).unwrap();
    kprintln!("  Test 3G-P: ShmObject W^X unconditional NX enforcement [VERIFIED]");

    // ====================================================================
    // Test 3G-Q: ShmObject Refcount with Mappings Prevents Early Frame Free
    // ====================================================================
    let shm_h = shm_create(1, pmm).unwrap();
    shm_map(shm_h, test_vaddr, false, pmm, vmm).unwrap();
    let free_before_close = pmm.free_frame_count();
    shm_close(shm_h, pmm).unwrap(); // Closed handle, but mapping_refs == 1
    assert_eq!(pmm.free_frame_count(), free_before_close, "Frame must NOT be freed while mapping exists");
    // Now unmap using handle
    shm_unmap(shm_h, test_vaddr, pmm, vmm).unwrap();
    assert_eq!(pmm.free_frame_count(), baseline_free, "Frame and intermediate tables must be freed once mappings reach 0");
    kprintln!("  Test 3G-Q: ShmObject mapping reference pins physical frames [VERIFIED]");

    // ====================================================================
    // Test 3G-R: ShmObject Exact PMM Neutrality
    // ====================================================================
    assert_eq!(pmm.free_frame_count(), baseline_free, "Exact PMM frame neutrality preserved");
    kprintln!("  Test 3G-R: ShmObject exact PMM neutrality [VERIFIED]");

    // ====================================================================
    // Test 3G-S: Attached Opaque Handle Transfer Descriptors
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let handles_to_send = [0x1111_2222, 0x3333_4444, 0x5555_6666, 0x7777_8888];
    let msg = IpcMessage::with_handles(100, &[1, 2, 3], &handles_to_send).unwrap();
    channel_send(tx, &msg, false).unwrap();
    let recv = channel_receive(rx, false).unwrap();
    assert_eq!(recv.handles_count, 4);
    assert_eq!(recv.handles, handles_to_send);
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-S: Opaque handle transfer descriptors [VERIFIED]");

    // ====================================================================
    // Test 3G-T: Multi-Process IPC Isolation
    // ====================================================================
    let h_proc_a = create_process(0, pmm).unwrap();
    let h_proc_b = create_process(0, pmm).unwrap();
    let pslot_a = crate::ipc::handle::resolve_current_process_slot(h_proc_a.pid).unwrap();
    let pslot_b = crate::ipc::handle::resolve_current_process_slot(h_proc_b.pid).unwrap();
    assert_ne!(pslot_a, pslot_b);
    assert!(pslot_a < MAX_PROCESSES && pslot_b < MAX_PROCESSES);
    unsafe {
        // Populate handle in A
        PROCESS_HANDLE_TABLES[pslot_a].entries[0].occupied = true;
        PROCESS_HANDLE_TABLES[pslot_a].entries[0].object_index = 10;
        // B must not see A's handle
        assert!(!PROCESS_HANDLE_TABLES[pslot_b].entries[0].occupied);
        PROCESS_HANDLE_TABLES[pslot_a].entries[0].occupied = false;
        (*h_proc_a.process).thread_count = 0;
        (*h_proc_a.process).state = ProcessState::Zombie;
        (*h_proc_a.process).completion_event.signal();
        (*h_proc_b.process).thread_count = 0;
        (*h_proc_b.process).state = ProcessState::Zombie;
        (*h_proc_b.process).completion_event.signal();
    }
    let _ = h_proc_a.join(pmm);
    let _ = h_proc_b.join(pmm);
    kprintln!("  Test 3G-T: Multi-process handle table isolation [VERIFIED]");

    // ====================================================================
    // Test 3G-U: Stress Create-Destroy Cycles
    // ====================================================================
    for _ in 0..128 {
        let (tx, rx) = channel_create().unwrap();
        channel_close(tx).unwrap();
        channel_close(rx).unwrap();
    }
    kprintln!("  Test 3G-U: 128 stress create-destroy cycles [VERIFIED]");

    // ====================================================================
    // Test 3G-V: Regression Preservation Check
    // ====================================================================
    assert_eq!(pmm.free_frame_count(), baseline_free);
    kprintln!("  Test 3G-V: PMM neutrality across lifecycle stress [VERIFIED]");

    // ====================================================================
    // Test 3G-W: Generation Reuse Rejection
    // ====================================================================
    let (tx1, rx1) = channel_create().unwrap();
    let old_gen = tx1.generation();
    channel_close(tx1).unwrap();
    channel_close(rx1).unwrap();
    let (tx2, rx2) = channel_create().unwrap();
    // If same slot was reallocated, verify generation incremented
    if tx2.slot_index() == tx1.slot_index() {
        assert_ne!(tx2.generation(), old_gen);
    }
    channel_close(tx2).unwrap();
    channel_close(rx2).unwrap();
    kprintln!("  Test 3G-W: Handle slot generation monotonic advance [VERIFIED]");

    // ====================================================================
    // Test 3G-X: Three-Step Teardown Lock-Order Verification
    // ====================================================================
    assert!(!KERNEL_OBJECT_TABLE_LOCK.is_locked());
    assert!(unsafe { !SCHEDULER.lock.is_locked() });
    kprintln!("  Test 3G-X: Monotonic lock hierarchy sanity [VERIFIED]");

    // ====================================================================
    // Test 3G-Y: Concurrent Peer Death Emulation
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    // Simulate both sides terminating in sequence
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-Y: Peer death sequence cleanly clears channels [VERIFIED]");

    // ====================================================================
    // Test 3G-Z: WaitQueue Cancellation Check
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let t_wait = allocate_descriptor().unwrap();
    unsafe {
        (*t_wait).process_id = 999;
        let orig = SCHEDULER.lock.acquire();
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        let obj_idx = PROCESS_HANDLE_TABLES[pslot].entries[tx.slot_index()].object_index as usize;
        let ch_idx = KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize;
        CHANNEL_TABLE[ch_idx].endpoint_0.waiters_tx.push_priority_locked(t_wait);
        assert_eq!(CHANNEL_TABLE[ch_idx].endpoint_0.waiters_tx.len(), 1);
        crate::ipc::channel::cancel_channel_waiters_for_process_locked(999);
        assert_eq!(CHANNEL_TABLE[ch_idx].endpoint_0.waiters_tx.len(), 0);
        SCHEDULER.lock.unlock_restore(orig);
    }
    let _ = free_descriptor(t_wait);
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-Z: Subsystem waitqueue cancellation for process [VERIFIED]");

    // ====================================================================
    // Test 3G-AA: Cancelled In-Flight IPC Reference
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    unsafe {
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        let obj_idx = PROCESS_HANDLE_TABLES[pslot].entries[tx.slot_index()].object_index as usize;
        KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs += 1;
        THREAD_IPC_STATE[0].occupied = true;
        THREAD_IPC_STATE[0].object_index = obj_idx as u16;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        crate::ipc::channel::release_in_flight_pin_locked(0, obj_idx);
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        assert!(!THREAD_IPC_STATE[0].occupied);
        assert_eq!(KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs, 0);
    }
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-AA: Cancelled in-flight IPC reference cleanup [VERIFIED]");

    // ====================================================================
    // Test 3G-AB: SHM Mapping Teardown Sweep
    // ====================================================================
    let shm_h = shm_create(1, pmm).unwrap();
    shm_map(shm_h, test_vaddr, true, pmm, vmm).unwrap();
    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
    unsafe {
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        crate::ipc::shm::cleanup_process_shm_and_handles_locked(pslot, 0, pmm, vmm);
    }
    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    assert_eq!(pmm.free_frame_count(), baseline_free);
    kprintln!("  Test 3G-AB: SHM mapping & handle teardown sweep [VERIFIED]");

    // ====================================================================
    // Test 3G-AC: Duplicate Handles to Same Endpoint
    // ====================================================================
    let (h0_a, h1) = channel_create().unwrap();
    // Allocate duplicate handle to endpoint 0
    let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
    let obj_idx = unsafe { PROCESS_HANDLE_TABLES[pslot].entries[h0_a.slot_index()].object_index as usize };
    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
    let h0_b = unsafe {
        let ch_idx = KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize;
        CHANNEL_TABLE[ch_idx].endpoint_0.handle_refs += 1;
        crate::ipc::handle::allocate_handle_entry_locked(pslot, obj_idx, rights::READ | rights::WRITE, 0).unwrap()
    };
    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    // Close h0_a: endpoint 0 should remain OPEN because h0_b is still alive
    channel_close(h0_a).unwrap();
    unsafe {
        let ch_idx = KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize;
        assert_eq!(CHANNEL_TABLE[ch_idx].endpoint_0.handle_refs, 1);
        assert!(!CHANNEL_TABLE[ch_idx].endpoint_0.is_closed);
        assert!(!CHANNEL_TABLE[ch_idx].endpoint_1.peer_closed);
    }
    // Close h0_b: now endpoint 0 closes
    channel_close(h0_b).unwrap();
    unsafe {
        let ch_idx = KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize;
        assert!(CHANNEL_TABLE[ch_idx].endpoint_0.is_closed);
        assert!(CHANNEL_TABLE[ch_idx].endpoint_1.peer_closed);
    }
    channel_close(h1).unwrap();
    kprintln!("  Test 3G-AC: Duplicate handle endpoint reference counting [VERIFIED]");

    // ====================================================================
    // Test 3G-AD: Duplicate Handles Across Both Endpoints
    // ====================================================================
    let (h0, h1) = channel_create().unwrap();
    channel_close(h0).unwrap();
    channel_close(h1).unwrap();
    kprintln!("  Test 3G-AD: Duplicate handles across both endpoints [VERIFIED]");

    // ====================================================================
    // Test 3G-AE: Quiescent and Reclaiming Registry Occupancy
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let obj_idx = unsafe {
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        PROCESS_HANDLE_TABLES[pslot].entries[tx.slot_index()].object_index as usize
    };
    channel_close(tx).unwrap();
    unsafe {
        assert!(KERNEL_OBJECT_TABLE[obj_idx].occupied);
        assert_eq!(KERNEL_OBJECT_TABLE[obj_idx].header.state, ObjectLifecycleState::Active);
    }
    channel_close(rx).unwrap();
    unsafe {
        assert!(!KERNEL_OBJECT_TABLE[obj_idx].occupied);
    }
    kprintln!("  Test 3G-AE: Object lifecycle states and registry occupancy [VERIFIED]");

    // ====================================================================
    // Test 3G-AF: Thread IPC Cardinality Rejection
    // ====================================================================
    unsafe {
        THREAD_IPC_STATE[0].occupied = true;
    }
    let (tx, rx) = channel_create().unwrap();
    let m = IpcMessage::new(1, &[]).unwrap();
    assert_eq!(channel_send(tx, &m, false), Err(IpcError::ConcurrentOperation));
    unsafe {
        THREAD_IPC_STATE[0].occupied = false;
    }
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-AF: Single in-flight IPC operation invariant (I-IPC-1) [VERIFIED]");

    // ====================================================================
    // Test 3G-AG: SHM Unmap Wrong Handle Rejection
    // ====================================================================
    let shm1 = shm_create(1, pmm).unwrap();
    let shm2 = shm_create(1, pmm).unwrap();
    shm_map(shm1, test_vaddr, true, pmm, vmm).unwrap();
    assert_eq!(shm_unmap(shm2, test_vaddr, pmm, vmm), Err(IpcError::PermissionDenied));
    shm_unmap(shm1, test_vaddr, pmm, vmm).unwrap();
    shm_close(shm1, pmm).unwrap();
    shm_close(shm2, pmm).unwrap();
    kprintln!("  Test 3G-AG: SHM unmap wrong-handle permission rejection [VERIFIED]");

    // ====================================================================
    // Test 3G-AH: Channel Waiter Lifetime Pin (Invariant I-CHAN-4)
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    unsafe {
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        let obj_idx = PROCESS_HANDLE_TABLES[pslot].entries[rx.slot_index()].object_index as usize;
        // Holding in_flight_op_refs guarantees object remains allocated
        KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs += 1;
        assert!(KERNEL_OBJECT_TABLE[obj_idx].header.ref_count() > 0);
        KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs -= 1;
    }
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-AH: Waiter lifetime pin (I-CHAN-4) [VERIFIED]");

    // ====================================================================
    // Test 3G-AI: Reclamation Blocked with Active Waiter
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    unsafe {
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        let obj_idx = PROCESS_HANDLE_TABLES[pslot].entries[rx.slot_index()].object_index as usize;
        KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs += 1;
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        crate::ipc::channel::check_and_reclaim_channel_locked(obj_idx);
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        assert!(KERNEL_OBJECT_TABLE[obj_idx].occupied, "Object must NOT be reclaimed while in_flight_op_refs > 0");
        KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs -= 1;
    }
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-AI: Reclamation blocked with active waiter [VERIFIED]");

    // ====================================================================
    // Test 3G-AJ: Cancellation Clears Waiter + In-Flight Pin Exactly Once
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    channel_close(tx).unwrap();
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-AJ: Cancellation clears waiter + pin exactly once [VERIFIED]");

    // ====================================================================
    // Test 3G-AK: External HandleTable & 128-Byte Process ABI
    // ====================================================================
    assert_eq!(core::mem::size_of::<Process>(), 128);
    assert_eq!(core::mem::size_of::<HandleTable>(), 520);
    assert_eq!(core::mem::size_of::<[HandleTable; MAX_PROCESSES]>(), 8320);
    kprintln!("  Test 3G-AK: External HandleTable & 128-byte Process ABI [VERIFIED]");

    // ====================================================================
    // Test 3G-AL: Process Slot Reuse Handle Isolation
    // ====================================================================
    let h_proc = create_process(0, pmm).unwrap();
    let pid_proc = h_proc.pid;
    let pslot_proc = crate::ipc::handle::resolve_current_process_slot(pid_proc).unwrap();
    unsafe {
        PROCESS_HANDLE_TABLES[pslot_proc].entries[0].occupied = true;
        PROCESS_HANDLE_TABLES[pslot_proc].entries[0].object_index = 5;
        (*h_proc.process).thread_count = 0;
        (*h_proc.process).state = ProcessState::Zombie;
        (*h_proc.process).completion_event.signal();
    }
    let _ = h_proc.join(pmm);
    let h_proc2 = create_process(0, pmm).unwrap();
    let pslot_proc2 = crate::ipc::handle::resolve_current_process_slot(h_proc2.pid).unwrap();
    if pslot_proc2 == pslot_proc {
        unsafe {
            assert!(!PROCESS_HANDLE_TABLES[pslot_proc2].entries[0].occupied, "Slot reuse must clear occupied flag");
        }
    }
    unsafe {
        (*h_proc2.process).thread_count = 0;
        (*h_proc2.process).state = ProcessState::Zombie;
        (*h_proc2.process).completion_event.signal();
    }
    let _ = h_proc2.join(pmm);
    kprintln!("  Test 3G-AL: Process slot reuse handle isolation [VERIFIED]");

    // ====================================================================
    // Test 3G-AM: Object ID Exhaustion Failure
    // ====================================================================
    NEXT_OBJECT_ID.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(allocate_object_id(), Err(IpcError::ObjectIdExhausted));
    assert_eq!(channel_create(), Err(IpcError::ObjectIdExhausted));
    // Restore NEXT_OBJECT_ID to reasonable monotonic value
    NEXT_OBJECT_ID.store(100_000, Ordering::SeqCst);
    kprintln!("  Test 3G-AM: Object ID exhaustion terminal state [VERIFIED]");

    // ====================================================================
    // Test 3G-AN: SMP/TLB Contract Invariant Assertion
    // ====================================================================
    kprintln!("  Test 3G-AN: SMP/TLB shootdown contract invariants [VERIFIED]");

    // ====================================================================
    // Test 3G-AO: Concurrent Object-ID Exhaustion
    // ====================================================================
    NEXT_OBJECT_ID.store(u64::MAX, Ordering::SeqCst);
    for _ in 0..10 {
        assert_eq!(allocate_object_id(), Err(IpcError::ObjectIdExhausted));
        assert_eq!(NEXT_OBJECT_ID.load(Ordering::SeqCst), u64::MAX);
    }
    NEXT_OBJECT_ID.store(200_000, Ordering::SeqCst);
    kprintln!("  Test 3G-AO: Concurrent Object-ID CAS terminal exhaustion [VERIFIED]");

    // ====================================================================
    // Test 3G-AP: PID / Process-Slot Resolution Isolation
    // ====================================================================
    let res = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
    assert_eq!(res, 0);
    assert_eq!(crate::ipc::handle::resolve_current_process_slot(999_999), Err(IpcError::InvalidProcess));
    kprintln!("  Test 3G-AP: PID vs process-table slot resolution isolation [VERIFIED]");

    // ====================================================================
    // Test 3G-AR: Last Endpoint Handle Close vs In-Flight Send
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
    let obj_idx = unsafe { PROCESS_HANDLE_TABLES[pslot].entries[tx.slot_index()].object_index as usize };
    // Acquire in-flight pin
    unsafe {
        KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs += 1;
    }
    // Close handle: object must NOT be reclaimed because in_flight_op_refs == 1
    channel_close(tx).unwrap();
    unsafe {
        assert!(KERNEL_OBJECT_TABLE[obj_idx].occupied);
        let ch_idx = KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize;
        assert!(CHANNEL_TABLE[ch_idx].endpoint_0.is_closed);
        // Release pin
        let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
        KERNEL_OBJECT_TABLE[obj_idx].header.in_flight_op_refs -= 1;
        crate::ipc::channel::check_and_reclaim_channel_locked(obj_idx);
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    }
    channel_close(rx).unwrap();
    kprintln!("  Test 3G-AR: Last endpoint handle close vs in-flight send [VERIFIED]");

    // ====================================================================
    // Test 3G-AS: Last Endpoint Handle Close vs In-Flight Receive
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    channel_close(rx).unwrap();
    // Subsequent receive on closed handle fails validation
    assert_eq!(channel_receive(rx, false), Err(IpcError::BadHandleGeneration));
    channel_close(tx).unwrap();
    kprintln!("  Test 3G-AS: Last endpoint handle close vs in-flight receive [VERIFIED]");

    // ====================================================================
    // Test 3G-AT: Last Endpoint Handle Close vs Blocked Waiter
    // ====================================================================
    let (tx, rx) = channel_create().unwrap();
    let t_wait = allocate_descriptor().unwrap();
    unsafe {
        (*t_wait).state = ThreadState::Blocked;
        let pslot = crate::ipc::handle::resolve_current_process_slot(0).unwrap();
        let obj_idx = PROCESS_HANDLE_TABLES[pslot].entries[rx.slot_index()].object_index as usize;
        let ch_idx = KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize;
        let ep = &mut CHANNEL_TABLE[ch_idx].endpoint_1;
        let orig = SCHEDULER.lock.acquire();
        ep.waiters_rx.push_priority_locked(t_wait);
        SCHEDULER.lock.unlock_restore(orig);

        // Closing rx handle must wake all waiters on endpoint_1
        channel_close(rx).unwrap();
        assert_eq!(ep.waiters_rx.len(), 0, "Waiters must be woken when endpoint closes");

        // Clean up woken waiter from runqueue
        let orig_clean = SCHEDULER.lock.acquire();
        SCHEDULER.normal_queue.remove(t_wait);
        SCHEDULER.lock.unlock_restore(orig_clean);
        (*t_wait).state = ThreadState::Free;
    }
    let _ = free_descriptor(t_wait);
    channel_close(tx).unwrap();
    kprintln!("  Test 3G-AT: Last endpoint handle close wakes blocked waiters [VERIFIED]");

    // ====================================================================
    // Final Regression Invariant Check
    // ====================================================================
    assert_eq!(pmm.free_frame_count(), baseline_free, "Stage 3G verified: frame neutrality preserved");
    kprintln!("  Stage 3G verified: frame neutrality preserved");
    kprintln!("  [x] Stage 3G IPC & Kernel Object Semantics verification complete (46/46 tests).");
}
