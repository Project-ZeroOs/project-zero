//! Project Zero - Stage 3I Syscall Frame ABI (I-SYSCALL-FRAME-1)
//!
//! Preserves user context across hardware `syscall` / `sysretq` boundary.
//! Distinct from frozen Stage 3C CooperativeFrame (64 B) and PreemptiveFrame (160 B).

/// Complete 18-field machine layout of saved user context across syscall_entry.
/// Exactly 144 bytes ($18 \times 8$ bytes), naturally 16-byte aligned on stack top.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallFrame {
    // ------------------------------------------------------------------------
    // [0..48] Callee-Saved Registers (kernel-saved to guarantee C/Rust ABI safety)
    // ------------------------------------------------------------------------
    pub r15: u64,         // Offset 0x00 (0)  - Callee-saved R15
    pub r14: u64,         // Offset 0x08 (8)  - Callee-saved R14
    pub r13: u64,         // Offset 0x10 (16) - Callee-saved R13
    pub r12: u64,         // Offset 0x18 (24) - Callee-saved R12
    pub rbx: u64,         // Offset 0x20 (32) - Callee-saved RBX
    pub rbp: u64,         // Offset 0x28 (40) - Callee-saved RBP

    // ------------------------------------------------------------------------
    // [48..96] Dispatcher-Visible Syscall Arguments (passed in registers)
    // ------------------------------------------------------------------------
    pub r9:  u64,         // Offset 0x30 (48) - Argument 5
    pub r8:  u64,         // Offset 0x38 (56) - Argument 4
    pub r10: u64,         // Offset 0x40 (64) - Argument 3 (passed in R10 by user)
    pub rdx: u64,         // Offset 0x48 (72) - Argument 2
    pub rsi: u64,         // Offset 0x50 (80) - Argument 1
    pub rdi: u64,         // Offset 0x58 (88) - Argument 0 (Handle or pointer)

    // ------------------------------------------------------------------------
    // [96..104] Syscall Number & Return Value
    // ------------------------------------------------------------------------
    pub rax: u64,         // Offset 0x60 (96) - In: Syscall number; Out: Return code

    // ------------------------------------------------------------------------
    // [104..144] Hardware-Captured User Execution Context
    // ------------------------------------------------------------------------
    pub user_rip:    u64, // Offset 0x68 (104) - Captured from RCX by hardware syscall
    pub user_cs:     u64, // Offset 0x70 (112) - Ring 3 Code Selector (0x23)
    pub user_rflags: u64, // Offset 0x78 (120) - Captured from R11 by hardware syscall
    pub user_rsp:    u64, // Offset 0x80 (128) - User stack pointer
    pub user_ss:     u64, // Offset 0x88 (136) - Ring 3 Data Selector (0x1B)
}

// ====================================================================
// Compile-Time Machine Offset & Size Assertions (Frozen Contract)
// ====================================================================
const _: () = assert!(core::mem::size_of::<SyscallFrame>() == 144);
const _: () = assert!(core::mem::align_of::<SyscallFrame>() == 8);

const _: () = assert!(core::mem::offset_of!(SyscallFrame, r15) == 0);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r14) == 8);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r13) == 16);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r12) == 24);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rbx) == 32);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rbp) == 40);

const _: () = assert!(core::mem::offset_of!(SyscallFrame, r9) == 48);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r8) == 56);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r10) == 64);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rdx) == 72);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rsi) == 80);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rdi) == 88);

const _: () = assert!(core::mem::offset_of!(SyscallFrame, rax) == 96);

const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_rip) == 104);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_cs) == 112);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_rflags) == 120);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_rsp) == 128);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_ss) == 136);
