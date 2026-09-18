//! Project Zero - Stage 3I Syscall Low-Level Entry & Assembly Glue
//!
//! Exposes external assembly stubs and Rust return validation / fail-closed handlers.

use super::abi::SyscallFrame;
use super::pointer::validate_user_return_state;
use crate::mm::vmm::ActivePageTable;
use crate::task::process::process_exit;
use crate::kprintln;

extern "C" {
    /// Assembly entry point for IA32_LSTAR (I-SYSCALL-3).
    pub fn syscall_entry();

    /// Initial Ring 3 drop trampoline executed via cooperative context switch (I-SYSCALL-11).
    pub fn user_thread_bootstrap_trampoline();

    /// Per-CPU scratch storage for user RSP during syscall entry (I-SYSCALL-STACK-1).
    pub static mut SYSCALL_SCRATCH_USER_RSP: [u64; 16];
}

/// Validates user return state prior to sysretq (I-SYSCALL-RETURN-1).
/// Returns 0 if valid, non-zero if invalid.
#[no_mangle]
pub extern "C" fn syscall_validate_return_rust(frame: *const SyscallFrame) -> u64 {
    if frame.is_null() {
        return 1;
    }
    let frame_ref = unsafe { &*frame };
    let vmm = ActivePageTable::new();
    match validate_user_return_state(frame_ref, &vmm) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/// Fail-closed termination when a return safety invariant is violated (I-SYSCALL-RETURN-2).
/// This is an unrecoverable security fault in the syscall return path.
#[no_mangle]
pub extern "C" fn syscall_fail_closed_terminate(frame: *const SyscallFrame) -> ! {
    kprintln!("\n[SECURITY VIOLATION] Invalid syscall return state detected! Aborting sysretq.");
    if !frame.is_null() {
        let f = unsafe { &*frame };
        kprintln!(
            "  user_rip: {:#018x}, user_rsp: {:#018x}, user_rflags: {:#018x}, CS: {:#x}, SS: {:#x}",
            f.user_rip, f.user_rsp, f.user_rflags, f.user_cs, f.user_ss
        );
    }
    kprintln!("  Terminating current user process fail-closed.");

    process_exit(-1);
}
