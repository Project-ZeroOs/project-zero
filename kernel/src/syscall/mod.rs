//! Project Zero - Stage 3I User Space & System Call Interface
//!
//! Authoritative Contract: Stage 3I Architecture Specification Rev4 (Approved & Frozen).

pub mod abi;
pub mod dispatch;
pub mod entry;
pub mod numbers;
pub mod pointer;
pub mod tests;

use crate::hal::arch::x86_64::cpu::{read_msr, write_msr};
use crate::task::percpu::BSP_PERCPU;
use crate::kprintln;

pub const IA32_EFER: u32           = 0xC000_0080;
pub const IA32_STAR: u32           = 0xC000_0081;
pub const IA32_LSTAR: u32          = 0xC000_0082;
pub const IA32_FMASK: u32          = 0xC000_0084;
pub const IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// Initializes hardware MSRs for fast `syscall` / `sysretq` operation (I-SYSCALL-3).
#[no_mangle]
#[inline(never)]
pub extern "C" fn init_syscall_hardware() {
    // 1. Enable System Call Extensions (SCE, bit 0) in IA32_EFER
    let efer = read_msr(IA32_EFER);
    write_msr(IA32_EFER, efer | 1);

    // 2. Configure IA32_STAR:
    // Bits [47:32]: Kernel CS selector base (0x08 -> CS=0x08, SS=0x10)
    // Bits [63:48]: User CS selector base (0x10 -> on sysretq 64-bit: CS=(0x10+16)|3=0x23, SS=(0x10+8)|3=0x1B)
    let star_val = (0x0010u64 << 48) | (0x0008u64 << 32);
    write_msr(IA32_STAR, star_val);

    // 3. Configure IA32_LSTAR: Address of assembly syscall_entry
    let lstar_addr = entry::syscall_entry as usize as u64;
    write_msr(IA32_LSTAR, lstar_addr);

    // 4. Configure IA32_FMASK: Mask IF (bit 9), TF (bit 8), DF (bit 10) on entry
    let fmask_val = 0x0000_0700u64;
    write_msr(IA32_FMASK, fmask_val);

    // 5. Configure IA32_KERNEL_GS_BASE: Points to BSP_PERCPU for swapgs
    unsafe {
        let bsp_percpu_addr = core::ptr::addr_of!(BSP_PERCPU) as u64;
        write_msr(IA32_KERNEL_GS_BASE, bsp_percpu_addr);
    }

    kprintln!("  [x] Fast syscall MSRs configured (SCE=1, STAR={:#018x}, LSTAR={:#018x}, FMASK={:#x}).",
        star_val, lstar_addr, fmask_val);
}
