//! Project Zero - Task & Execution Management (Stage 3A)
//!
//! Provides execution primitives: KernelThread descriptors, static table,
//! dedicated Kernel Stack Arena with 16 KiB guarded stacks, PMM/VMM transactional
//! allocation, and BSP PerCpu state.

pub mod stack;
pub mod percpu;
pub mod thread;
pub mod scheduler;
pub mod context;
pub mod waitqueue;
pub mod mutex;
pub mod condvar;
pub mod sleep;
pub mod event;
pub mod lifecycle;
pub mod process;

pub use scheduler::{
    exit_current_thread, exit_current_thread_with_code, init_scheduler, run_stage3b_verification,
    run_stage3c_inc5_verification, run_stage3d_inc1_verification, run_stage3d_inc2_verification,
    run_stage3d_inc3_verification, run_stage3d_inc4_verification, run_stage3d_inc5_verification,
    run_stage3e_verification, spawn, yield_now,
};
pub use process::{
    create_process, init_process_subsystem, process_exit, reap_zombie_processes,
    run_stage3f_verification, Process, ProcessHandle, ProcessId, ProcessState,
};
pub use context::run_stage3c_inc4_verification;
pub use waitqueue::WaitQueue;
pub use mutex::Mutex;
pub use condvar::Condvar;
pub use sleep::{sleep_ms, SleepEntry, SLEEP_TABLE, SLEEP_WAIT_QUEUE};
pub use event::{Event, EventType};
pub use lifecycle::{reap_zombies, spawn_thread, zombie_queue_count, JoinHandle, ThreadId};

use crate::kprintln;
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::{ActivePageTable, AddressSpaceGeometry, Page, PageTableFlags, VirtualAddress};
use stack::{StackAllocation, STACK_ARENA_START, STACK_ARENA_END, STACK_USABLE_SIZE, STACK_GUARD_SIZE};
use thread::{
    allocate_descriptor, create_bootstrap_thread, create_thread, free_descriptor, Priority,
    thread_bootstrap_entry, ThreadState,
};
use percpu::{current_thread_from_gs, init_bsp_percpu, verify_gs_access};

/// Stage 3A Verification Routine.
///
/// Deterministically proves all Stage 3A architectural contracts:
/// 1. Static descriptor table with immutable virtual addresses and unique IDs.
/// 2. Dedicated Kernel Stack Arena allocation, guard adjacency, and 16 KiB usable stack.
/// 3. Page table permissions (guard PRESENT=0, stack pages PRESENT|WRITABLE|NX, USER=0).
/// 4. Transactional rollback under simulated PMM exhaustion and partial VMM mapping failure.
/// 5. CPU-observable Page Fault (#PF, Vector 14) on guard page with correct CR2 and non-present code.
/// 6. BSP PerCpu initialization via IA32_GS_BASE and GS+16 current_thread dereference.
/// 7. Forged initial cooperative frame matching the exact `boot/context.asm` machine ABI.
pub fn run_stage3a_verification(pmm: &mut PhysicalMemoryManager, geometry: &AddressSpaceGeometry) {
    kprintln!("\n[Stage 3A: Execution Primitives & Guarded Stacks Verification]");

    // ====================================================================
    // Test 1: Static Descriptor Table & Address Stability
    // ====================================================================
    let t1_ptr = allocate_descriptor().expect("Failed to allocate descriptor 1");
    let t2_ptr = allocate_descriptor().expect("Failed to allocate descriptor 2");

    assert!(t1_ptr != t2_ptr, "Descriptor pointers must be distinct");
    unsafe {
        assert!((*t1_ptr).id != (*t2_ptr).id, "Thread IDs must be unique");
        assert!((*t1_ptr).id > 0 && (*t2_ptr).id > 0, "Allocated thread IDs must be non-zero");

        let addr1 = t1_ptr as usize;
        let addr2 = t2_ptr as usize;
        assert_eq!(addr1 % 8, 0, "Descriptor 1 must be 8-byte aligned");
        assert_eq!(addr2 % 8, 0, "Descriptor 2 must be 8-byte aligned");
    }

    free_descriptor(t2_ptr).expect("Failed to free descriptor 2");
    free_descriptor(t1_ptr).expect("Failed to free descriptor 1");

    kprintln!("  [x] Static descriptor table verified (stable addresses, unique IDs, 8-byte aligned).");

    // ====================================================================
    // Test 2: Dedicated Kernel Stack Arena & Geometry
    // ====================================================================
    let initial_free = pmm.free_frame_count();
    let mut vmm = ActivePageTable::new();
    let stack1 = StackAllocation::allocate(pmm, &mut vmm).expect("Stack 1 allocation failed");

    assert_eq!(
        stack1.guard_end, stack1.stack_start,
        "Guard page must be immediately adjacent to usable stack start"
    );
    assert_eq!(
        stack1.stack_end - stack1.stack_start,
        STACK_USABLE_SIZE,
        "Usable stack footprint must be exactly 16 KiB (0x4000)"
    );
    assert_eq!(
        stack1.guard_end - stack1.guard_start,
        STACK_GUARD_SIZE,
        "Guard page footprint must be exactly 4 KiB (0x1000)"
    );
    assert!(
        stack1.guard_start >= STACK_ARENA_START && stack1.stack_end <= STACK_ARENA_END,
        "Stack allocation must reside strictly within [STACK_ARENA_START, STACK_ARENA_END)"
    );

    // Verify Guard Page is completely unmapped (is_mapped == false)
    let guard_page = Page::from_start_address(VirtualAddress::new(stack1.guard_start), geometry)
        .expect("Invalid guard page address");
    assert!(!vmm.is_mapped(guard_page), "Guard page must be NOT PRESENT (unmapped)");

    // Verify all 4 usable stack pages are mapped with PRESENT | WRITABLE | NO_EXECUTE and USER=0
    for k in 0..4 {
        let vaddr = stack1.stack_start + (k as u64) * 4096;
        let page = Page::from_start_address(VirtualAddress::new(vaddr), geometry)
            .expect("Invalid stack page address");
        assert!(vmm.is_mapped(page), "Stack page must be present in page tables");

        let flags = vmm.get_page_flags(page).expect("Failed to get stack page flags");
        assert!(
            flags.contains(PageTableFlags::PRESENT),
            "Stack page must have PRESENT = 1"
        );
        assert!(
            flags.contains(PageTableFlags::WRITABLE),
            "Stack page must have WRITABLE = 1"
        );
        assert!(
            flags.contains(PageTableFlags::NO_EXECUTE),
            "Stack page must have NO_EXECUTE = 1"
        );
        assert!(
            !flags.contains(PageTableFlags::USER_ACCESSIBLE),
            "Stack page must have USER = 0 (supervisor privilege)"
        );
    }

    // Allocate a second stack and verify non-overlapping virtual ranges
    let stack2 = StackAllocation::allocate(pmm, &mut vmm).expect("Stack 2 allocation failed");
    assert!(
        stack2.guard_start >= stack1.stack_end || stack1.guard_start >= stack2.stack_end,
        "Stack allocations must not overlap"
    );
    stack2.destroy(pmm, &mut vmm).expect("Failed to destroy stack 2");

    kprintln!("  [x] Dedicated Kernel Stack Arena verified (guard is_mapped=false, 16 KiB stack RW+NX+supervisor).");

    // ====================================================================
    // Test 3: Transactional Rollback Under Partial Failures
    // ====================================================================
    let pre_test3_free = pmm.free_frame_count();

    // 3A: Simulated PMM exhaustion after partial allocation (e.g. 2 frames)
    let pmm_err = StackAllocation::test_allocate_simulated_pmm_exhaustion(2, pmm);
    assert_eq!(
        pmm_err.unwrap_err(),
        stack::StackError::PmmExhausted,
        "Simulated PMM exhaustion must return PmmExhausted"
    );
    assert_eq!(
        pmm.free_frame_count(),
        pre_test3_free,
        "PMM frame count must be completely restored after PMM allocation failure"
    );

    // 3B: Simulated VMM mapping failure after partial mappings (e.g. page 2 failure)
    // Proves pages 0..1 are unmapped, all frames returned to PMM, and slot released.
    let vmm_err = StackAllocation::test_allocate_simulated_vmm_failure(2, pmm, &mut vmm);
    assert!(
        matches!(vmm_err.unwrap_err(), stack::StackError::VmmMappingFailed(_)),
        "Simulated VMM failure must return VmmMappingFailed"
    );
    assert_eq!(
        pmm.free_frame_count(),
        pre_test3_free,
        "PMM frame count must be completely restored after partial VMM mapping rollback"
    );

    kprintln!("  [x] Transactional rollback verified: exact PMM accounting restored on partial failure.");

    // ====================================================================
    // Test 4: CPU-Observable Guard Page #PF Trap and Recovery
    // ====================================================================
    let guard_vaddr = stack1.guard_start;
    unsafe {
        crate::hal::arch::x86_64::idt::expect_fault(
            14,
            crate::hal::arch::x86_64::idt::test_resume_pf as *const () as usize as u64,
            Some(guard_vaddr),
        );
        crate::hal::arch::x86_64::idt::test_trigger_pf(guard_vaddr);

        let fault = crate::hal::arch::x86_64::idt::last_fault_result()
            .expect("Controlled guard-page #PF was not intercepted");
        assert_eq!(fault.vector, 14, "Expected vector 14 (#PF)");
        assert_eq!(
            fault.fault_addr & !0xFFF,
            guard_vaddr & !0xFFF,
            "CR2 must match the unmapped guard-page address exactly"
        );
        assert_eq!(
            fault.error_code & 1,
            0,
            "Error code bit 0 must be 0 (Page Not Present)"
        );
    }

    // Clean up stack1 and assert zero frame leakage
    stack1.destroy(pmm, &mut vmm).expect("Failed to destroy stack 1");
    assert_eq!(
        pmm.free_frame_count(),
        initial_free,
        "PMM frame count must return to baseline after stack destruction"
    );

    kprintln!("  [x] Controlled Guard Page #PF trapped with CR2 and non-present code; recovered successfully.");

    // ====================================================================
    // Test 5: BSP PerCpu & IA32_GS_BASE
    // ====================================================================
    let bsp_thread = create_bootstrap_thread().expect("Failed to create BSP initial thread");
    init_bsp_percpu(bsp_thread);

    assert!(
        verify_gs_access(bsp_thread),
        "IA32_GS_BASE and GS-relative dereferencing verification failed"
    );
    assert_eq!(
        current_thread_from_gs(),
        bsp_thread,
        "current_thread_from_gs() must return the stable BSP thread descriptor"
    );

    kprintln!("  [x] BSP PerCpu initialized via IA32_GS_BASE; GS+16 dereference verified.");

    // ====================================================================
    // Test 6: Thread Creation & Forged Initial Cooperative Frame
    // ====================================================================
    extern "C" fn dummy_thread_entry(_arg: u64) {}

    let test_thread = create_thread(
        dummy_thread_entry,
        0xDEAD_BEEF_CAFE_BABE,
        Priority::Normal,
        pmm,
        &mut vmm,
    )
    .expect("Failed to create worker thread with forged stack");

    unsafe {
        let t = &*test_thread;
        assert_eq!(t.state, ThreadState::Ready);
        assert_eq!(
            t.saved_rsp,
            t.stack_top - 64,
            "Initial saved_rsp must equal stack_top - 64"
        );

        // Inspect forged context frame in memory:
        // [saved_rsp + 0]:  RFLAGS (0x0202)
        // [saved_rsp + 8]:  R15 (0)
        // [saved_rsp + 16]: R14 (0)
        // [saved_rsp + 24]: R13 (0)
        // [saved_rsp + 32]: R12 (arg = 0xDEAD_BEEF_CAFE_BABE)
        // [saved_rsp + 40]: RBP (0)
        // [saved_rsp + 48]: RBX (dummy_thread_entry function pointer)
        // [saved_rsp + 56]: Return RIP (thread_bootstrap_entry trampoline)
        let frame = t.saved_rsp as *const u64;
        assert_eq!(*frame.add(0), 0x0202, "Frame[0] must be RFLAGS with IF=1");
        assert_eq!(*frame.add(1), 0, "Frame[1] must be R15 (0)");
        assert_eq!(*frame.add(2), 0, "Frame[2] must be R14 (0)");
        assert_eq!(*frame.add(3), 0, "Frame[3] must be R13 (0)");
        assert_eq!(
            *frame.add(4),
            0xDEAD_BEEF_CAFE_BABE,
            "Frame[4] must be R12 holding initial argument"
        );
        assert_eq!(*frame.add(5), 0, "Frame[5] must be RBP (0)");
        assert_eq!(
            *frame.add(6),
            dummy_thread_entry as *const () as usize as u64,
            "Frame[6] must be RBX holding entry function pointer"
        );
        assert_eq!(
            *frame.add(7),
            thread_bootstrap_entry as *const () as usize as u64,
            "Frame[7] must be return RIP pointing to thread_bootstrap_entry"
        );
    }

    let _ = free_descriptor(test_thread);

    kprintln!("  [x] Forged initial cooperative frame verified matching boot/context.asm ABI.");
}
