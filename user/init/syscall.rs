//! Project Zero - Freestanding Zero-Dependency User-Space Syscall Wrappers
#![no_std]

use core::arch::asm;

#[inline(always)]
pub unsafe fn sys_exit(code: i32) -> ! {
    asm!(
        "syscall",
        in("rax") 1, // SYS_EXIT
        in("rdi") code as u64,
        options(noreturn)
    );
}

#[inline(always)]
pub unsafe fn sys_yield() -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 2, // SYS_YIELD
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_channel_create(out_handles: *mut [u32; 2]) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 3, // SYS_CHANNEL_CREATE
        in("rdi") out_handles as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_channel_send(handle: u32, msg_ptr: *const u8, flags: u32) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 4, // SYS_CHANNEL_SEND
        in("rdi") handle as u64,
        in("rsi") msg_ptr as u64,
        in("rdx") flags as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_channel_receive(handle: u32, msg_ptr: *mut u8, flags: u32) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 5, // SYS_CHANNEL_RECEIVE
        in("rdi") handle as u64,
        in("rsi") msg_ptr as u64,
        in("rdx") flags as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_channel_close(handle: u32) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 6, // SYS_CHANNEL_CLOSE
        in("rdi") handle as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_shm_create(pages: usize, out_handle: *mut u32) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 7, // SYS_SHM_CREATE
        in("rdi") pages as u64,
        in("rsi") out_handle as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_shm_map(handle: u32, vaddr: u64, writable: bool) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 8, // SYS_SHM_MAP
        in("rdi") handle as u64,
        in("rsi") vaddr,
        in("rdx") if writable { 1u64 } else { 0u64 },
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_shm_unmap(handle: u32, vaddr: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 9, // SYS_SHM_UNMAP
        in("rdi") handle as u64,
        in("rsi") vaddr,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_cap_derive(handle: u32, rights: u32, out_handle: *mut u32) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") 10, // SYS_CAP_DERIVE
        in("rdi") handle as u64,
        in("rsi") rights as u64,
        in("rdx") out_handle as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}
