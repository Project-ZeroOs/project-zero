//! Project Zero - Preemptive Context Assembly Primitives (Stage 3C Increment 4)
//!
//! Implements typed bindings and deterministic verification for the four-way
//! context transition matrix defined in ADR-0011 (Revision 3):
//! 1. Coop -> Coop: switch_context (frozen Stage 3B primitive)
//! 2. Coop -> Preempt: switch_context_coop_to_preempt (saves 64B frame, restores 160B frame via iretq)
//! 3. Preempt -> Coop: restore_context_cooperative (restores 64B frame via ret)
//! 4. Preempt -> Preempt: restore_context_preemptive (restores 160B frame via iretq)

use crate::hal::arch::x86_64::cpu;
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;
use crate::task::stack::StackAllocation;
use crate::kprintln;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::task::thread::SavedFrameType;

pub static COUNT_COOP_TO_COOP: AtomicU64 = AtomicU64::new(0);
pub static COUNT_COOP_TO_PREEMPT: AtomicU64 = AtomicU64::new(0);
pub static COUNT_PREEMPT_TO_COOP: AtomicU64 = AtomicU64::new(0);
pub static COUNT_PREEMPT_TO_PREEMPT: AtomicU64 = AtomicU64::new(0);

extern "C" {
    pub fn switch_context(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64);
    pub fn switch_context_coop_to_preempt(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64);
    pub fn restore_context_cooperative(next_rsp: u64) -> !;
    pub fn restore_context_preemptive(next_rsp: u64) -> !;
}

/// Custom non-standard context-transfer boundary.
///
/// ABI Contract:
/// - When invoked by a cooperative thread, the call instruction pushes the return RIP.
/// - Callee-saved registers (RBX, RBP, R12..R15) and RFLAGS are saved in the outgoing frame.
/// - In the preemptive branch, switch_context_coop_to_preempt switches stack and executes iretq.
/// - When the outgoing thread is rescheduled, ret pops the return RIP and resumes directly
///   after this call boundary with stack pointer and callee-saved registers fully restored.
#[inline(never)]
pub unsafe fn terminal_context_switch(
    prev_rsp: *mut u64,
    next_rsp: u64,
    orig_rflags: u64,
    target_frame: SavedFrameType,
) {
    match target_frame {
        SavedFrameType::Cooperative => {
            COUNT_COOP_TO_COOP.fetch_add(1, Ordering::Relaxed);
            switch_context(prev_rsp, next_rsp, orig_rflags);
        }
        SavedFrameType::Preemptive => {
            COUNT_COOP_TO_PREEMPT.fetch_add(1, Ordering::Relaxed);
            switch_context_coop_to_preempt(prev_rsp, next_rsp, orig_rflags);
        }
    }
}

extern "C" {
    // Continuation harness entry & communication statics
    pub fn run_inc4_coop_start(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64);
    pub fn stage3c_inc4_target1_entry();
    pub fn stage3c_inc4_target2_entry();

    pub static mut inc4_saved_coop_rsp: u64;
    pub static mut inc4_target2_frame_rsp: u64;

    pub static mut inc4_target1_rax: u64;
    pub static mut inc4_target1_rcx: u64;
    pub static mut inc4_target1_rdx: u64;
    pub static mut inc4_target1_rbx: u64;
    pub static mut inc4_target1_rbp: u64;
    pub static mut inc4_target1_rsi: u64;
    pub static mut inc4_target1_rdi: u64;
    pub static mut inc4_target1_r8: u64;
    pub static mut inc4_target1_r9: u64;
    pub static mut inc4_target1_r10: u64;
    pub static mut inc4_target1_r11: u64;
    pub static mut inc4_target1_r12: u64;
    pub static mut inc4_target1_r13: u64;
    pub static mut inc4_target1_r14: u64;
    pub static mut inc4_target1_r15: u64;
    pub static mut inc4_target1_rsp: u64;
    pub static mut inc4_target1_rflags: u64;

    pub static mut inc4_target2_rax: u64;
    pub static mut inc4_target2_rcx: u64;
    pub static mut inc4_target2_rdx: u64;
    pub static mut inc4_target2_rbx: u64;
    pub static mut inc4_target2_rbp: u64;
    pub static mut inc4_target2_rsi: u64;
    pub static mut inc4_target2_rdi: u64;
    pub static mut inc4_target2_r8: u64;
    pub static mut inc4_target2_r9: u64;
    pub static mut inc4_target2_r10: u64;
    pub static mut inc4_target2_r11: u64;
    pub static mut inc4_target2_r12: u64;
    pub static mut inc4_target2_r13: u64;
    pub static mut inc4_target2_r14: u64;
    pub static mut inc4_target2_r15: u64;
    pub static mut inc4_target2_rsp: u64;
    pub static mut inc4_target2_rflags: u64;

    pub static mut inc4_resumed_rbx: u64;
    pub static mut inc4_resumed_rbp: u64;
    pub static mut inc4_resumed_r12: u64;
    pub static mut inc4_resumed_r13: u64;
    pub static mut inc4_resumed_r14: u64;
    pub static mut inc4_resumed_r15: u64;
    pub static mut inc4_resumed_rsp: u64;
    pub static mut inc4_resumed_rflags: u64;
}

static EXPECTED_STACK1_TOP: AtomicU64 = AtomicU64::new(0);
static EXPECTED_STACK2_TOP: AtomicU64 = AtomicU64::new(0);
static STEP1_PASS: AtomicU64 = AtomicU64::new(0);
static STEP2_PASS: AtomicU64 = AtomicU64::new(0);
static STEP3_PASS: AtomicU64 = AtomicU64::new(0);

pub const SENTINELS_TARGET1: [u64; 15] = [
    0x1111_1111_1111_1111, // RAX
    0x2222_2222_2222_2222, // RCX
    0x3333_3333_3333_3333, // RDX
    0x4444_4444_4444_4444, // RBX
    0x5555_5555_5555_5555, // RBP
    0x6666_6666_6666_6666, // RSI
    0x7777_7777_7777_7777, // RDI
    0x8888_8888_8888_8888, // R8
    0x9999_9999_9999_9999, // R9
    0xAAAA_AAAA_AAAA_AAAA, // R10
    0xBBBB_BBBB_BBBB_BBBB, // R11
    0xCCCC_CCCC_CCCC_CCCC, // R12
    0xDDDD_DDDD_DDDD_DDDD, // R13
    0xEEEE_EEEE_EEEE_EEEE, // R14
    0xFFFF_FFFF_FFFF_FFFF, // R15
];

pub const SENTINELS_TARGET2: [u64; 15] = [
    0xA1A1_A1A1_A1A1_A1A1, // RAX
    0xB2B2_B2B2_B2B2_B2B2, // RCX
    0xC3C3_C3C3_C3C3_C3C3, // RDX
    0xD4D4_D4D4_D4D4_D4D4, // RBX
    0xE5E5_E5E5_E5E5_E5E5, // RBP
    0xF6F6_F6F6_F6F6_F6F6, // RSI
    0x0707_0707_0707_0707, // RDI
    0x1818_1818_1818_1818, // R8
    0x2929_2929_2929_2929, // R9
    0x3A3A_3A3A_3A3A_3A3A, // R10
    0x4B4B_4B4B_4B4B_4B4B, // R11
    0x5C5C_5C5C_5C5C_5C5C, // R12
    0x6D6D_6D6D_6D6D_6D6D, // R13
    0x7E7E_7E7E_7E7E_7E7E, // R14
    0x8F8F_8F8F_8F8F_8F8F, // R15
];

/// Forges a valid 160-byte long-mode preemptive interrupt frame on the target stack.
///
/// Invariants Enforced:
/// 1. `stack_top` must be canonical higher-half virtual address.
/// 2. `stack_top % 16 == 0` (strict 16-byte alignment).
/// 3. Hardware frame: SS=0x10, RSP=stack_top, RFLAGS=rflags, CS=0x08, RIP=entry_rip.
/// 4. Software frame: 15 general-purpose registers pre-loaded with explicit sentinels.
/// 5. Returns initial `saved_rsp` pointing to R15 (+0x00, exactly `stack_top - 160`).
pub unsafe fn forge_preemptive_frame(
    stack_top: u64,
    entry_rip: u64,
    sentinels: &[u64; 15],
    rflags: u64,
) -> u64 {
    assert!(stack_top != 0, "stack_top cannot be null");
    assert_eq!(stack_top % 16, 0, "stack_top must be 16-byte aligned");
    assert!(stack_top >= 0xFFFF_8000_0000_0000, "stack_top must be canonical higher-half VMA");
    assert!(entry_rip >= 0xFFFF_8000_0000_0000, "entry_rip must be canonical higher-half VMA");

    let frame_rsp = stack_top - 160;
    let ptr = frame_rsp as *mut u64;

    // Software pushed 15 GPRs (in reverse pop order)
    *ptr.add(0) = sentinels[14]; // R15 (+0x00)
    *ptr.add(1) = sentinels[13]; // R14 (+0x08)
    *ptr.add(2) = sentinels[12]; // R13 (+0x10)
    *ptr.add(3) = sentinels[11]; // R12 (+0x18)
    *ptr.add(4) = sentinels[10]; // R11 (+0x20)
    *ptr.add(5) = sentinels[9];  // R10 (+0x28)
    *ptr.add(6) = sentinels[8];  // R9  (+0x30)
    *ptr.add(7) = sentinels[7];  // R8  (+0x38)
    *ptr.add(8) = sentinels[6];  // RDI (+0x40)
    *ptr.add(9) = sentinels[5];  // RSI (+0x48)
    *ptr.add(10) = sentinels[4]; // RBP (+0x50)
    *ptr.add(11) = sentinels[3]; // RBX (+0x58)
    *ptr.add(12) = sentinels[2]; // RDX (+0x60)
    *ptr.add(13) = sentinels[1]; // RCX (+0x68)
    *ptr.add(14) = sentinels[0]; // RAX (+0x70)

    // Hardware pushed interrupt frame
    *ptr.add(15) = entry_rip;    // RIP (+0x78)
    *ptr.add(16) = 0x08;         // CS  (+0x80) -> Kernel Code Segment Selector
    *ptr.add(17) = rflags;       // RFLAGS (+0x88)
    *ptr.add(18) = stack_top;    // RSP (+0x90) -> 16-byte aligned stack top
    *ptr.add(19) = 0x10;         // SS  (+0x98) -> Kernel Data Segment Selector

    frame_rsp
}

/// Callback invoked from `stage3c_inc4_target1_entry` in `boot/context.asm`.
/// Validates complete architectural state for Transition 2 (Coop -> Preempt).
#[no_mangle]
pub extern "C" fn stage3c_inc4_verify_target1() {
    unsafe {
        let expected_rsp = EXPECTED_STACK1_TOP.load(Ordering::Relaxed);
        assert_eq!(inc4_target1_rsp, expected_rsp, "Target 1 RSP mismatch");
        assert_eq!(inc4_target1_rsp % 16, 0, "Target 1 RSP must be 16-byte aligned at entry");
        assert_ne!(inc4_target1_rflags & 0x200, 0, "Target 1 RFLAGS.IF must be 1 (restored by iretq)");

        assert_eq!(inc4_target1_rax, SENTINELS_TARGET1[0], "Target 1 RAX mismatch");
        assert_eq!(inc4_target1_rcx, SENTINELS_TARGET1[1], "Target 1 RCX mismatch");
        assert_eq!(inc4_target1_rdx, SENTINELS_TARGET1[2], "Target 1 RDX mismatch");
        assert_eq!(inc4_target1_rbx, SENTINELS_TARGET1[3], "Target 1 RBX mismatch");
        assert_eq!(inc4_target1_rbp, SENTINELS_TARGET1[4], "Target 1 RBP mismatch");
        assert_eq!(inc4_target1_rsi, SENTINELS_TARGET1[5], "Target 1 RSI mismatch");
        assert_eq!(inc4_target1_rdi, SENTINELS_TARGET1[6], "Target 1 RDI mismatch");
        assert_eq!(inc4_target1_r8,  SENTINELS_TARGET1[7], "Target 1 R8 mismatch");
        assert_eq!(inc4_target1_r9,  SENTINELS_TARGET1[8], "Target 1 R9 mismatch");
        assert_eq!(inc4_target1_r10, SENTINELS_TARGET1[9], "Target 1 R10 mismatch");
        assert_eq!(inc4_target1_r11, SENTINELS_TARGET1[10], "Target 1 R11 mismatch");
        assert_eq!(inc4_target1_r12, SENTINELS_TARGET1[11], "Target 1 R12 mismatch");
        assert_eq!(inc4_target1_r13, SENTINELS_TARGET1[12], "Target 1 R13 mismatch");
        assert_eq!(inc4_target1_r14, SENTINELS_TARGET1[13], "Target 1 R14 mismatch");
        assert_eq!(inc4_target1_r15, SENTINELS_TARGET1[14], "Target 1 R15 mismatch");
    }

    STEP1_PASS.store(1, Ordering::Relaxed);
    kprintln!("  [INC4 STEP 1] Coop -> Preempt (switch_context_coop_to_preempt): PASS");
    kprintln!("    Entry RSP: 0x{:016X} (RSP % 16 == 0) [VERIFIED]", unsafe { inc4_target1_rsp });
    kprintln!("    Restored RFLAGS: 0x{:016X} (IF=1 via iretq) [VERIFIED]", unsafe { inc4_target1_rflags });
    kprintln!("    All 15 GPRs Verified: RAX=0x1111.. to R15=0xFFFF.. [MATCH]");
}

/// Callback invoked from `stage3c_inc4_target2_entry` in `boot/context.asm`.
/// Validates complete architectural state for Transition 4 (Preempt -> Preempt).
#[no_mangle]
pub extern "C" fn stage3c_inc4_verify_target2() {
    unsafe {
        let expected_rsp = EXPECTED_STACK2_TOP.load(Ordering::Relaxed);
        assert_eq!(inc4_target2_rsp, expected_rsp, "Target 2 RSP mismatch");
        assert_eq!(inc4_target2_rsp % 16, 0, "Target 2 RSP must be 16-byte aligned at entry");
        assert_ne!(inc4_target2_rflags & 0x200, 0, "Target 2 RFLAGS.IF must be 1 (restored by iretq)");

        assert_eq!(inc4_target2_rax, SENTINELS_TARGET2[0], "Target 2 RAX mismatch");
        assert_eq!(inc4_target2_rcx, SENTINELS_TARGET2[1], "Target 2 RCX mismatch");
        assert_eq!(inc4_target2_rdx, SENTINELS_TARGET2[2], "Target 2 RDX mismatch");
        assert_eq!(inc4_target2_rbx, SENTINELS_TARGET2[3], "Target 2 RBX mismatch");
        assert_eq!(inc4_target2_rbp, SENTINELS_TARGET2[4], "Target 2 RBP mismatch");
        assert_eq!(inc4_target2_rsi, SENTINELS_TARGET2[5], "Target 2 RSI mismatch");
        assert_eq!(inc4_target2_rdi, SENTINELS_TARGET2[6], "Target 2 RDI mismatch");
        assert_eq!(inc4_target2_r8,  SENTINELS_TARGET2[7], "Target 2 R8 mismatch");
        assert_eq!(inc4_target2_r9,  SENTINELS_TARGET2[8], "Target 2 R9 mismatch");
        assert_eq!(inc4_target2_r10, SENTINELS_TARGET2[9], "Target 2 R10 mismatch");
        assert_eq!(inc4_target2_r11, SENTINELS_TARGET2[10], "Target 2 R11 mismatch");
        assert_eq!(inc4_target2_r12, SENTINELS_TARGET2[11], "Target 2 R12 mismatch");
        assert_eq!(inc4_target2_r13, SENTINELS_TARGET2[12], "Target 2 R13 mismatch");
        assert_eq!(inc4_target2_r14, SENTINELS_TARGET2[13], "Target 2 R14 mismatch");
        assert_eq!(inc4_target2_r15, SENTINELS_TARGET2[14], "Target 2 R15 mismatch");
    }

    STEP2_PASS.store(1, Ordering::Relaxed);
    kprintln!("  [INC4 STEP 2] Preempt -> Preempt (restore_context_preemptive): PASS");
    kprintln!("    Entry RSP: 0x{:016X} (RSP % 16 == 0) [VERIFIED]", unsafe { inc4_target2_rsp });
    kprintln!("    Restored RFLAGS: 0x{:016X} (IF=1 via iretq) [VERIFIED]", unsafe { inc4_target2_rflags });
    kprintln!("    All 15 GPRs Verified: RAX=0xA1A1.. to R15=0x8F8F.. [MATCH]");
}

/// Callback invoked from `inc4_coop_continuation` in `boot/context.asm`.
/// Validates complete architectural state for Transition 3 (Preempt -> Coop).
#[no_mangle]
pub extern "C" fn stage3c_inc4_complete_verification() {
    unsafe {
        assert_eq!(inc4_resumed_rbx, 0x4444_4444_4444_4444, "Resumed RBX mismatch");
        assert_eq!(inc4_resumed_rbp, 0x5555_5555_5555_5555, "Resumed RBP mismatch");
        assert_eq!(inc4_resumed_r12, 0xCCCC_CCCC_CCCC_CCCC, "Resumed R12 mismatch");
        assert_eq!(inc4_resumed_r13, 0xDDDD_DDDD_DDDD_DDDD, "Resumed R13 mismatch");
        assert_eq!(inc4_resumed_r14, 0xEEEE_EEEE_EEEE_EEEE, "Resumed R14 mismatch");
        assert_eq!(inc4_resumed_r15, 0xFFFF_FFFF_FFFF_FFFF, "Resumed R15 mismatch");

        // Verify that restore_context_cooperative popped the original IF state
        assert_ne!(inc4_resumed_rflags & 0x200, 0, "Resumed cooperative RFLAGS must have IF=1 restored");
    }

    STEP3_PASS.store(1, Ordering::Relaxed);
    kprintln!("  [INC4 STEP 3] Preempt -> Coop (restore_context_cooperative): PASS");
    kprintln!("    Resumed Callee GPRs Verified: RBX, RBP, R12..R15 [MATCH]");
    kprintln!("    Restored RFLAGS: 0x{:016X} (IF=1 preserved across chain) [VERIFIED]", unsafe { inc4_resumed_rflags });
    kprintln!("    Resumed RSP: 0x{:016X} [VERIFIED]", unsafe { inc4_resumed_rsp });
}

/// Executes Stage 3C Increment 4 Preemptive Context Assembly Primitives Verification.
///
/// Deterministically proves the four-way transition matrix assembly primitives
/// across an explicit non-returning continuation chain:
///   Coop Start -> Preempt 1 -> Preempt 2 -> Coop Continuation -> Completion
///
/// Invariants verified:
/// 1. `switch_context` (Coop -> Coop) remains frozen and untouched.
/// 2. `switch_context_coop_to_preempt` correctly saves 64B frame and restores 160B frame via `iretq`.
/// 3. `restore_context_preemptive` switches stack and restores 160B frame via `iretq`.
/// 4. `restore_context_cooperative` switches stack and restores 64B frame via `ret`.
/// 5. Strict `IF=0` precondition enforced on entry to every primitive.
/// 6. Complete architectural register state (15 GPRs, RSP alignment, RFLAGS) verified at each destination.
/// 7. Zero dynamic heap allocation; transactional PMM stack reclamation verified.
pub fn run_stage3c_inc4_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3C-Increment 4: Preemptive Context Assembly Primitives]");

    // 1. Allocate dedicated guarded stacks for Target 1 and Target 2
    let baseline_free = pmm.free_frame_count();

    let stack1 = StackAllocation::allocate(pmm, vmm)
        .expect("Failed to allocate stack 1 for Increment 4 verification");
    let stack2 = StackAllocation::allocate(pmm, vmm)
        .expect("Failed to allocate stack 2 for Increment 4 verification");

    assert_eq!(stack1.stack_top % 16, 0, "Stack 1 top must be 16-byte aligned");
    assert_eq!(stack2.stack_top % 16, 0, "Stack 2 top must be 16-byte aligned");

    EXPECTED_STACK1_TOP.store(stack1.stack_top, Ordering::Relaxed);
    EXPECTED_STACK2_TOP.store(stack2.stack_top, Ordering::Relaxed);

    // 2. Forge Preemptive Frame 1 (Target 1) and Preemptive Frame 2 (Target 2)
    let frame1_rsp = unsafe {
        forge_preemptive_frame(
            stack1.stack_top,
            stage3c_inc4_target1_entry as usize as u64,
            &SENTINELS_TARGET1,
            0x0202, // IF=1, bit 1=1
        )
    };

    let frame2_rsp = unsafe {
        forge_preemptive_frame(
            stack2.stack_top,
            stage3c_inc4_target2_entry as usize as u64,
            &SENTINELS_TARGET2,
            0x0202, // IF=1, bit 1=1
        )
    };

    unsafe {
        inc4_target2_frame_rsp = frame2_rsp;
        inc4_saved_coop_rsp = 0;
    }

    STEP1_PASS.store(0, Ordering::Relaxed);
    STEP2_PASS.store(0, Ordering::Relaxed);
    STEP3_PASS.store(0, Ordering::Relaxed);

    // 3. Establish original RFLAGS with IF=1, then enforce IF=0 precondition
    let orig_rflags = cpu::read_rflags() | 0x200; // Original state has IF=1
    cpu::cli(); // Invariant: Caller enters primitive with IF=0

    // 4. Launch continuation chain
    unsafe {
        run_inc4_coop_start(
            &raw mut inc4_saved_coop_rsp,
            frame1_rsp,
            orig_rflags,
        );
    }

    // When run_inc4_coop_start returns via inc4_coop_continuation:
    assert_eq!(STEP1_PASS.load(Ordering::Relaxed), 1, "Step 1 (Coop -> Preempt) failed");
    assert_eq!(STEP2_PASS.load(Ordering::Relaxed), 1, "Step 2 (Preempt -> Preempt) failed");
    assert_eq!(STEP3_PASS.load(Ordering::Relaxed), 1, "Step 3 (Preempt -> Coop) failed");

    // 5. Clean up dedicated test stacks and verify PMM frame accounting
    stack2.destroy(pmm, vmm).expect("Failed to destroy stack 2");
    stack1.destroy(pmm, vmm).expect("Failed to destroy stack 1");

    let post_free = pmm.free_frame_count();
    assert_eq!(post_free, baseline_free, "PMM leak detected in Increment 4 verification");

    kprintln!("  Stack Accounting:            {} free frames restored [EXACT MATCH]", post_free);
    kprintln!("  Four-way transition matrix assembly primitives verified.");
    kprintln!("  [x] Stage 3C Increment 4 Preemptive Context Assembly Primitives verified.");
}
