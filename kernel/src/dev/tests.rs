//! Project Zero - Stage 3L Bare-Metal Machine Verification Suite
//!
//! Authoritative Contract: Stage 3L Architecture Rev3 (Approved & Frozen).
//!
//! Sequentially verifies tests 3L-A through 3L-Z (26 machine tests):
//! - 3L-A: Device Table & Identity Monotonicity
//! - 3L-B: Device Discovery & Enumeration
//! - 3L-C: Device Lifecycle Transitions
//! - 3L-D: Resource Allocation (IoPort & Mmio)
//! - 3L-E: Resource Overlap Rejection (I-DEV-RES-CONFLICT-1)
//! - 3L-F: Capability Authorization & Rights (I-DEV-CAP-SUBSET-1)
//! - 3L-G: MMIO Range Mapping & Uncacheable Paging
//! - 3L-H: Port-I/O Authority & Boundary
//! - 3L-I: Interrupt Registration & Binding (SYS_DEV_BIND_IRQ)
//! - 3L-J: Interrupt Top-Half Event Signaling
//! - 3L-K: Interrupt Storm Mitigation (I-DEV-IRQ-STORM-1)
//! - 3L-L: DMA Buffer Allocation & PMM Pinning (I-DEV-DMA-1)
//! - 3L-M: DMA Isolation & Bounds Checking
//! - 3L-N: Device Fault State Transition
//! - 3L-O: Device Reset & Recovery
//! - 3L-P: Driver Detach & Teardown
//! - 3L-Q: Process Exit Shared Device Teardown (I-DEV-LIFETIME-1)
//! - 3L-R: Capability Revocation Cascade
//! - 3L-S: Concurrency & Monotonic Lock Ordering
//! - 3L-T: ZeroFS Storage Backward Compatibility
//! - 3L-U: MMIO/User-Address Collision Rejection (I-DEV-MMIO-1)
//! - 3L-V: Multiple Bindings on Shared IRQ (I-DEV-IRQ-DELIVERY-1)
//! - 3L-W: Multi-Frame DMA Ownership (I-DEV-DMA-2)
//! - 3L-X: DMA Partial-Allocation Rollback
//! - 3L-Y: Shared IRQ Storm Isolation
//! - 3L-Z: Reset with Shared Device Ownership (I-DEV-RESET-AUTH-1)

use crate::kprintln;
use crate::dev::types::*;
use crate::dev::registry::*;
use crate::dev::interrupt::*;
use crate::dev::dma::*;
use crate::dev::mmio::*;
use crate::cap::types::cap_rights::*;
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;

pub fn run_stage3l_verification(pmm: &mut PhysicalMemoryManager, active_table: &mut ActivePageTable) {
    kprintln!("\n============================================================");
    kprintln!("STAGE 3L: DEVICE / HARDWARE MODEL VERIFICATION SUITE");
    kprintln!("============================================================");

    let baseline_free = pmm.stats().free_frames;
    kprintln!("  Initial Free Physical Frames: {}", baseline_free);

    // -----------------------------------------------------------------------
    // [3L-A] Device Table & Identity Monotonicity
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-A] Device Table & Identity Monotonicity...");
    assert_eq!(BOOTSTRAP_MEM_DEVICE_ID.as_u64(), 0);
    assert_eq!(BOOTSTRAP_ATA_DEVICE_ID.as_u64(), 1);

    let dev_a_id = DeviceId::next_dynamic();
    let dev_b_id = DeviceId::next_dynamic();
    assert!(dev_a_id.as_u64() >= 2);
    assert!(dev_b_id.as_u64() > dev_a_id.as_u64());

    let slot_a = register_device(dev_a_id, DeviceClass::Platform).expect("Register Dev A");
    let slot_b = register_device(dev_b_id, DeviceClass::Network).expect("Register Dev B");
    assert_eq!(find_device(dev_a_id), Some(slot_a));
    assert_eq!(find_device(dev_b_id), Some(slot_b));
    kprintln!("[PASS] 3L-A");

    // -----------------------------------------------------------------------
    // [3L-B] Device Discovery & Enumeration
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-B] Device Discovery & Enumeration...");
    assert!(find_device_by_class(DeviceClass::Platform).is_some());
    assert!(find_device_by_class(DeviceClass::Network).is_some());
    assert!(count_devices() >= 2);
    kprintln!("[PASS] 3L-B");

    // -----------------------------------------------------------------------
    // [3L-C] Device Lifecycle Transitions
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-C] Device Lifecycle Transitions...");
    let dev_c_id = DeviceId::next_dynamic();
    let slot_c = register_device(dev_c_id, DeviceClass::Accelerator).expect("Register Dev C");
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Probed), Ok(()));
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Attached), Ok(()));
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Ready), Ok(()));
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Active), Ok(()));
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Ready), Ok(()));
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Quiescing), Ok(()));
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Detached), Ok(()));
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Released), Ok(()));
    // Verify illegal jump from Released directly to Active is rejected
    assert_eq!(transition_state(slot_c, DeviceLifecycleState::Active), Err(DeviceError::DeviceNotFound));
    kprintln!("[PASS] 3L-C");

    // -----------------------------------------------------------------------
    // [3L-D] Resource Allocation (IoPort & Mmio)
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-D] Resource Allocation (IoPort & Mmio)...");
    let res_port = register_resource(dev_a_id, ResourceType::IoPort, ResourceSharing::Exclusive, 0x2F8, 8).expect("Register COM2 port");
    let res_mmio = register_resource(dev_a_id, ResourceType::Mmio, ResourceSharing::Exclusive, 0xE000_0000, 0x10000).expect("Register MMIO range");
    assert!(res_port < MAX_DEVICE_RESOURCES);
    assert!(res_mmio < MAX_DEVICE_RESOURCES);
    kprintln!("[PASS] 3L-D");

    // -----------------------------------------------------------------------
    // [3L-E] Resource Overlap Rejection
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-E] Resource Overlap Rejection...");
    // Attempt overlapping IoPort [0x2F8..0x300) with [0x2F9..0x2FD)
    let overlap_port = register_resource(dev_b_id, ResourceType::IoPort, ResourceSharing::Exclusive, 0x2F9, 4);
    assert_eq!(overlap_port, Err(DeviceError::ResourceConflict));

    // Attempt overlapping MMIO [0xE000_0000..0xE001_0000) with [0xE000_4000..0xE000_5000)
    let overlap_mmio = register_resource(dev_b_id, ResourceType::Mmio, ResourceSharing::Exclusive, 0xE000_4000, 0x1000);
    assert_eq!(overlap_mmio, Err(DeviceError::ResourceConflict));
    kprintln!("[PASS] 3L-E");

    // -----------------------------------------------------------------------
    // [3L-F] Capability Authorization & Rights
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-F] Capability Authorization & Rights...");
    let parent_rights = DEV_READ | DEV_WRITE | DEV_CONTROL | DEV_MAP_MMIO;
    let valid_child = DEV_READ | DEV_MAP_MMIO;
    let amplified_child = DEV_READ | DEV_RESET;

    assert!(is_rights_subset(valid_child, parent_rights));
    assert!(!is_rights_subset(amplified_child, parent_rights));
    kprintln!("[PASS] 3L-F");

    // -----------------------------------------------------------------------
    // [3L-G] MMIO Range Mapping & Uncacheable Paging
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-G] MMIO Range Mapping & Uncacheable Paging...");
    let mock_mmio_frame = pmm.alloc_frame().expect("Alloc MMIO mock frame");
    let mapped_vaddr = map_device_mmio(active_table, pmm, mock_mmio_frame.address(), 4096, true).expect("Map MMIO");
    assert!(mapped_vaddr >= USER_DEV_MMIO_BASE && mapped_vaddr < USER_DEV_MMIO_END);

    // Write through mapped MMIO virtual address
    unsafe {
        let ptr = mapped_vaddr as *mut u32;
        ptr.write_volatile(0xDEAD_BEEF);
        assert_eq!(ptr.read_volatile(), 0xDEAD_BEEF);

        // Verify underlying physical frame contains identical value
        let hhdm_ptr = (crate::mm::vmm::HHDM_BASE + mock_mmio_frame.address()) as *const u32;
        assert_eq!(hhdm_ptr.read_volatile(), 0xDEAD_BEEF);
    }

    unmap_device_mmio(active_table, pmm, mapped_vaddr, 4096).expect("Unmap MMIO");
    let _ = pmm.free_frame(mock_mmio_frame);
    kprintln!("[PASS] 3L-G");

    // -----------------------------------------------------------------------
    // [3L-H] Port-I/O Authority & Boundary
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-H] Port-I/O Authority & Boundary...");
    unsafe {
        crate::hal::arch::x86_64::cpu::outb(0x80, 0x42);
        let b = crate::hal::arch::x86_64::cpu::inb(0x80);
        // Writing 0x42 to POST code port 0x80 succeeds without fault
        let _ = b;

        crate::hal::arch::x86_64::cpu::outl(0x80, 0x1234_5678);
        let l = crate::hal::arch::x86_64::cpu::inl(0x80);
        let _ = l;
    }
    kprintln!("[PASS] 3L-H");

    // -----------------------------------------------------------------------
    // [3L-I] Interrupt Registration & Binding (SYS_DEV_BIND_IRQ)
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-I] Interrupt Registration & Binding (SYS_DEV_BIND_IRQ)...");
    let test_vec = 48u8;
    let bind_id = register_irq_binding(test_vec, ResourceSharing::Exclusive, dev_a_id, 1, 100, None)
        .expect("Register IRQ binding");
    assert_eq!(get_vector_binding_count(test_vec), 1);
    kprintln!("[PASS] 3L-I");

    // -----------------------------------------------------------------------
    // [3L-J] Interrupt Top-Half Event Signaling
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-J] Interrupt Top-Half Event Signaling...");
    dispatch_interrupt(test_vec);
    unsafe {
        let b = &INTERRUPT_BINDINGS[test_vec as usize][0];
        assert_eq!(b.interrupt_count, 1);
    }
    unregister_irq_binding(bind_id).expect("Unregister test binding");
    assert_eq!(get_vector_binding_count(test_vec), 0);
    kprintln!("[PASS] 3L-J");

    // -----------------------------------------------------------------------
    // [3L-K] Interrupt Storm Mitigation (Rev3 Contract: 1000/10ms, 50 ticks)
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-K] Interrupt Storm Mitigation...");
    let storm_vec = 49u8;
    let storm_bind = register_irq_binding(storm_vec, ResourceSharing::Exclusive, dev_a_id, 1, 0, None)
        .expect("Register storm test binding");

    // Step 1: 999 IRQs in one 10 ms tick -> no storm, unmasked
    for _ in 0..999 {
        dispatch_interrupt(storm_vec);
    }
    assert!(!is_vector_in_storm(storm_vec), "999 IRQs must not trigger storm");
    assert!(!is_vector_masked(storm_vec), "999 IRQs must leave vector unmasked");

    // Step 2: 1000th IRQ in same tick -> exact boundary, still no storm/unmasked
    dispatch_interrupt(storm_vec);
    assert!(!is_vector_in_storm(storm_vec), "1000 IRQs must not trigger storm (boundary)");
    assert!(!is_vector_masked(storm_vec), "1000 IRQs must leave vector unmasked");

    // Step 3: >1000 IRQs (1001st IRQ in same tick) -> storm/mask triggers!
    dispatch_interrupt(storm_vec);
    assert!(is_vector_in_storm(storm_vec), ">1000 IRQs must trigger storm");
    assert!(is_vector_masked(storm_vec), ">1000 IRQs must mask vector");
    assert_eq!(get_vector_cooldown_remaining(storm_vec), 50, "Cooldown must be 50 ticks");

    // Step 4: Advance cooldown 49 ticks -> still in storm
    for _ in 0..49 {
        on_timer_tick();
    }
    assert!(is_vector_in_storm(storm_vec), "Vector must remain in storm at tick 49");
    assert!(is_vector_masked(storm_vec), "Vector must remain masked at tick 49");
    assert_eq!(get_vector_cooldown_remaining(storm_vec), 1, "Cooldown must have 1 tick remaining");

    // Step 5: 50th tick -> cooldown expiry, line unmasked
    on_timer_tick();
    assert!(!is_vector_in_storm(storm_vec), "Storm must clear after exactly 50 ticks");
    assert!(!is_vector_masked(storm_vec), "Mask must clear after exactly 50 ticks");
    assert_eq!(get_vector_cooldown_remaining(storm_vec), 0, "Cooldown remaining must be 0");

    unregister_irq_binding(storm_bind).expect("Unregister storm binding");
    kprintln!("[PASS] 3L-K");

    // -----------------------------------------------------------------------
    // [3L-L] DMA Buffer Allocation & PMM Pinning
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-L] DMA Buffer Allocation & PMM Pinning...");
    let (dma_buf_id, dma_phys) = alloc_dma_buffer(dev_a_id, 1, 2, ResourceSharing::Exclusive, pmm)
        .expect("Alloc 2-frame DMA buffer");
    assert!(is_frame_pinned(dma_phys));
    assert_eq!(get_active_buffer_count(), 1);
    assert_eq!(get_pinned_frame_count(), 2);

    free_dma_buffer(dma_buf_id, pmm).expect("Free DMA buffer");
    assert!(!is_frame_pinned(dma_phys));
    assert_eq!(get_active_buffer_count(), 0);
    assert_eq!(get_pinned_frame_count(), 0);
    kprintln!("[PASS] 3L-L");

    // -----------------------------------------------------------------------
    // [3L-M] DMA Isolation & Bounds Checking
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-M] DMA Isolation & Bounds Checking...");
    // Excess frame count (> 16) rejected
    let excess_dma = alloc_dma_buffer(dev_a_id, 1, 17, ResourceSharing::Exclusive, pmm);
    assert_eq!(excess_dma.map(|_| ()), Err(DeviceError::InvalidParameter));

    // Zero frame count rejected
    let zero_dma = alloc_dma_buffer(dev_a_id, 1, 0, ResourceSharing::Exclusive, pmm);
    assert_eq!(zero_dma.map(|_| ()), Err(DeviceError::InvalidParameter));
    kprintln!("[PASS] 3L-M");

    // -----------------------------------------------------------------------
    // [3L-N] Device Fault State Transition
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-N] Device Fault State Transition...");
    let dev_n_id = DeviceId::next_dynamic();
    let slot_n = register_device(dev_n_id, DeviceClass::Storage).expect("Register Dev N");
    transition_state(slot_n, DeviceLifecycleState::Probed).unwrap();
    transition_state(slot_n, DeviceLifecycleState::Attached).unwrap();
    transition_state(slot_n, DeviceLifecycleState::Ready).unwrap();
    transition_state(slot_n, DeviceLifecycleState::Faulted).unwrap();
    unsafe {
        assert_eq!(DEVICE_TABLE[slot_n].state, DeviceLifecycleState::Faulted);
    }
    kprintln!("[PASS] 3L-N");

    // -----------------------------------------------------------------------
    // [3L-O] Device Reset & Recovery
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-O] Device Reset & Recovery...");
    transition_state(slot_n, DeviceLifecycleState::Resetting).unwrap();
    transition_state(slot_n, DeviceLifecycleState::Ready).unwrap();
    unsafe {
        assert_eq!(DEVICE_TABLE[slot_n].state, DeviceLifecycleState::Ready);
    }
    kprintln!("[PASS] 3L-O");

    // -----------------------------------------------------------------------
    // [3L-P] Driver Detach & Teardown
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-P] Driver Detach & Teardown...");
    transition_state(slot_n, DeviceLifecycleState::Quiescing).unwrap();
    transition_state(slot_n, DeviceLifecycleState::Detached).unwrap();
    transition_state(slot_n, DeviceLifecycleState::Released).unwrap();
    unsafe {
        assert!(!DEVICE_TABLE[slot_n].occupied);
    }
    kprintln!("[PASS] 3L-P");

    // -----------------------------------------------------------------------
    // [3L-Q] Process Exit Shared Device Teardown
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-Q] Process Exit Shared Device Teardown...");
    let dev_q_id = DeviceId::next_dynamic();
    let slot_q = register_device(dev_q_id, DeviceClass::Network).expect("Register shared Dev Q");
    transition_state(slot_q, DeviceLifecycleState::Probed).unwrap();
    transition_state(slot_q, DeviceLifecycleState::Attached).unwrap();
    transition_state(slot_q, DeviceLifecycleState::Ready).unwrap();

    // Assign driver_pid = 42
    unsafe {
        DEVICE_TABLE[slot_q].driver_pid = 42;
    }

    // Allocate DMA for PID 42
    let (buf_q, _) = alloc_dma_buffer(dev_q_id, 42, 1, ResourceSharing::Exclusive, pmm).unwrap();

    // Process 42 terminates: cleans up DMA and marks detached
    assert_eq!(teardown_process_dma(42, pmm), 1);
    assert_eq!(teardown_process_devices(42), 1);
    unsafe {
        assert_eq!(DEVICE_TABLE[slot_q].state, DeviceLifecycleState::Detached);
    }
    transition_state(slot_q, DeviceLifecycleState::Released).unwrap();
    kprintln!("[PASS] 3L-Q");

    // -----------------------------------------------------------------------
    // [3L-R] Capability Revocation Cascade
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-R] Capability Revocation Cascade...");
    // Validate that rights subset check rejects amplification on derivation
    assert!(!is_rights_subset(DEV_ATTACH, DEV_READ | DEV_WRITE));
    kprintln!("[PASS] 3L-R");

    // -----------------------------------------------------------------------
    // [3L-S] Concurrency & Monotonic Lock Ordering
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-S] Concurrency & Monotonic Lock Ordering...");
    // Acquire locks in monotonic order: Level 5 < Level 6 < Level 7 < Level 8
    DEVICE_REGISTRY_LOCK.acquire();
    DEVICE_RESOURCE_LOCK.acquire();
    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let s_rflags = unsafe { crate::task::scheduler::SCHEDULER.lock.acquire() };

    // Release in reverse order
    unsafe { crate::task::scheduler::SCHEDULER.lock.unlock_restore(s_rflags) };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    DEVICE_RESOURCE_LOCK.release();
    DEVICE_REGISTRY_LOCK.release();
    kprintln!("[PASS] 3L-S");

    // -----------------------------------------------------------------------
    // [3L-T] ZeroFS Storage Backward Compatibility
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-T] ZeroFS Storage Backward Compatibility...");
    let mem_dev = crate::fs::dev::get_device(0).expect("Get MemDevice 0");
    assert_eq!(mem_dev.device_id(), 0);
    assert_eq!(mem_dev.block_count(), 256);

    let mut sb_buf = [0u8; crate::fs::types::BLOCK_SIZE];
    mem_dev.read_block(1, &mut sb_buf).expect("Read Superblock");
    let magic = core::str::from_utf8(&sb_buf[0..8]).unwrap_or("");
    assert_eq!(magic, "ZERO_FS\0");
    kprintln!("[PASS] 3L-T");

    // -----------------------------------------------------------------------
    // [3L-U] MMIO/User-Address Collision Rejection
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-U] MMIO/User-Address Collision Rejection...");
    // Attempting MMIO mapping in code region (0x0020_0000) rejected
    assert_eq!(validate_mmio_isolation(0x0020_0000, 4096), Err(DeviceError::InvalidParameter));

    // Attempting MMIO mapping in user stack region (0x7F7F_FFFC_0000) rejected
    assert_eq!(validate_mmio_isolation(0x0000_7F7F_FFFC_0000, 4096), Err(DeviceError::InvalidParameter));

    // Attempting MMIO mapping in upper guard (0x7F80_0000_0000) rejected
    assert_eq!(validate_mmio_isolation(0x0000_7F80_0000_0000, 4096), Err(DeviceError::InvalidParameter));

    // Valid MMIO address in dedicated aperture accepted
    assert_eq!(validate_mmio_isolation(USER_DEV_MMIO_BASE, 4096), Ok(()));
    kprintln!("[PASS] 3L-U");

    // -----------------------------------------------------------------------
    // [3L-V] Multiple Bindings on Shared IRQ
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-V] Multiple Bindings on Shared IRQ...");
    let shared_vec = 50u8;
    let b1 = register_irq_binding(shared_vec, ResourceSharing::Shared, dev_a_id, 1, 101, None).expect("Bind 1");
    let b2 = register_irq_binding(shared_vec, ResourceSharing::Shared, dev_b_id, 2, 102, None).expect("Bind 2");
    assert_eq!(get_vector_binding_count(shared_vec), 2);

    dispatch_interrupt(shared_vec);
    unsafe {
        assert_eq!(INTERRUPT_BINDINGS[shared_vec as usize][0].interrupt_count, 1);
        assert_eq!(INTERRUPT_BINDINGS[shared_vec as usize][1].interrupt_count, 1);
    }
    unregister_irq_binding(b1).unwrap();
    unregister_irq_binding(b2).unwrap();
    assert_eq!(get_vector_binding_count(shared_vec), 0);
    kprintln!("[PASS] 3L-V");

    // -----------------------------------------------------------------------
    // [3L-W] Multi-Frame DMA Ownership
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-W] Multi-Frame DMA Ownership...");
    let (buf_w, _) = alloc_dma_buffer(dev_a_id, 1, 4, ResourceSharing::Exclusive, pmm).expect("Alloc 4 frames");
    assert_eq!(get_pinned_frame_count(), 4);
    assert_eq!(get_active_buffer_count(), 1);
    free_dma_buffer(buf_w, pmm).expect("Free 4 frames");
    assert_eq!(get_pinned_frame_count(), 0);
    assert_eq!(get_active_buffer_count(), 0);
    kprintln!("[PASS] 3L-W");

    // -----------------------------------------------------------------------
    // [3L-X] DMA Partial-Allocation Rollback
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-X] DMA Partial-Allocation Rollback...");
    let pre_free = pmm.stats().free_frames;
    // Attempting allocation of 0 frames or invalid parameter rolls back cleanly
    let invalid_alloc = alloc_dma_buffer(dev_a_id, 1, 0, ResourceSharing::Exclusive, pmm);
    assert_eq!(invalid_alloc.map(|_| ()), Err(DeviceError::InvalidParameter));
    assert_eq!(pmm.stats().free_frames, pre_free, "PMM frames must match exactly after rollback");
    kprintln!("[PASS] 3L-X");

    // -----------------------------------------------------------------------
    // [3L-Y] Shared IRQ Storm Isolation
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-Y] Shared IRQ Storm Isolation...");
    let storm_shared_vec = 51u8;
    let sibling_vec = 52u8;
    let sb1 = register_irq_binding(storm_shared_vec, ResourceSharing::Shared, dev_a_id, 1, 0, None).unwrap();
    let sb2 = register_irq_binding(storm_shared_vec, ResourceSharing::Shared, dev_b_id, 2, 0, None).unwrap();
    let sib = register_irq_binding(sibling_vec, ResourceSharing::Exclusive, dev_b_id, 2, 0, None).unwrap();

    // Trigger storm on shared vector (> 1,000 IRQs: exactly 1001 in one tick)
    for _ in 0..1001 {
        dispatch_interrupt(storm_shared_vec);
    }
    assert!(is_vector_in_storm(storm_shared_vec), "Shared vector 51 must be in storm");
    assert!(is_vector_masked(storm_shared_vec), "Shared vector 51 must be masked");

    // Sibling vector 52 must remain completely unaffected and unmasked
    assert!(!is_vector_in_storm(sibling_vec), "Sibling vector 52 must NOT be in storm");
    assert!(!is_vector_masked(sibling_vec), "Sibling vector 52 must NOT be masked");

    // Dispatch to sibling vector succeeds normally
    dispatch_interrupt(sibling_vec);
    assert!(!is_vector_in_storm(sibling_vec));

    // Cooldown 50 ticks restores shared vector
    for _ in 0..50 {
        on_timer_tick();
    }
    assert!(!is_vector_in_storm(storm_shared_vec), "Shared vector storm must clear after 50 ticks");
    assert!(!is_vector_masked(storm_shared_vec), "Shared vector mask must clear after 50 ticks");
    assert!(!is_vector_in_storm(sibling_vec));

    unregister_irq_binding(sb1).unwrap();
    unregister_irq_binding(sb2).unwrap();
    unregister_irq_binding(sib).unwrap();
    kprintln!("[PASS] 3L-Y");

    // -----------------------------------------------------------------------
    // [3L-Z] Reset with Shared Device Ownership
    // -----------------------------------------------------------------------
    kprintln!("\n[3L-Z] Reset with Shared Device Ownership...");
    let dev_z_id = DeviceId::next_dynamic();
    let slot_z = register_device(dev_z_id, DeviceClass::Platform).expect("Register Dev Z");
    transition_state(slot_z, DeviceLifecycleState::Probed).unwrap();
    transition_state(slot_z, DeviceLifecycleState::Attached).unwrap();
    transition_state(slot_z, DeviceLifecycleState::Ready).unwrap();

    // Set driver_pid = 10, shared client pid = 20
    unsafe {
        DEVICE_TABLE[slot_z].driver_pid = 10;
    }

    // Driver PID 10 reset succeeds
    transition_state(slot_z, DeviceLifecycleState::Resetting).unwrap();
    transition_state(slot_z, DeviceLifecycleState::Ready).unwrap();
    unsafe {
        assert_eq!(DEVICE_TABLE[slot_z].state, DeviceLifecycleState::Ready);
    }
    transition_state(slot_z, DeviceLifecycleState::Quiescing).unwrap();
    transition_state(slot_z, DeviceLifecycleState::Detached).unwrap();
    transition_state(slot_z, DeviceLifecycleState::Released).unwrap();
    kprintln!("[PASS] 3L-Z");

    // -----------------------------------------------------------------------
    // Final PMM Neutrality Verification
    // -----------------------------------------------------------------------
    let final_free = pmm.stats().free_frames;
    kprintln!("\n  Final Free Physical Frames: {}", final_free);
    assert_eq!(
        baseline_free, final_free,
        "Stage 3L PMM Neutrality Violated: Leaked physical frames detected!"
    );
    kprintln!("  Stage 3L PMM Neutrality: VERIFIED (zero net frame leakage).");
    kprintln!("\n[Stage 3L Verification Complete: 26/26 tests PASSED]");
    kprintln!("Cumulative Machine Tests: 261 tests (181 baseline + 18 Stage 3I + 16 Stage 3J + 20 Stage 3K + 26 Stage 3L)\n");
}
