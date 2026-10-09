//! ZeroOS - libzero Freestanding Syscall Wrappers

use core::arch::asm;

/// Fast system call invocation numbers (harmonized with Stage 3I & 3L).
pub mod numbers {
    pub const SYS_EXIT: u64 = 1;
    pub const SYS_YIELD: u64 = 2;
    pub const SYS_CHANNEL_CREATE: u64 = 3;
    pub const SYS_CHANNEL_SEND: u64 = 4;
    pub const SYS_CHANNEL_RECEIVE: u64 = 5;
    pub const SYS_CHANNEL_CLOSE: u64 = 6;
    pub const SYS_SHM_CREATE: u64 = 7;
    pub const SYS_SHM_MAP: u64 = 8;
    pub const SYS_SHM_UNMAP: u64 = 9;
    pub const SYS_CAP_DERIVE: u64 = 10;
    pub const SYS_FILE_OPEN: u64 = 11;
    pub const SYS_FILE_READ: u64 = 12;
    pub const SYS_FILE_WRITE: u64 = 13;
    pub const SYS_FILE_CLOSE: u64 = 14;
    pub const SYS_FILE_STAT: u64 = 15;
    pub const SYS_FILE_SYNC: u64 = 16;
    pub const SYS_DEV_QUERY: u64 = 17;
    pub const SYS_DEV_MAP_MMIO: u64 = 18;
    pub const SYS_DEV_DMA_ALLOC: u64 = 19;
    pub const SYS_DEV_RESET: u64 = 20;
    pub const SYS_DEV_BIND_IRQ: u64 = 21;
    pub const SYS_DIR_CREATE: u64 = 32;
    pub const SYS_FILE_UNLINK: u64 = 33;
    pub const SYS_FILE_RENAME: u64 = 34;
    pub const SYS_DIR_READ: u64 = 35;
}

#[inline(always)]
pub unsafe fn sys_exit(code: i32) -> ! {
    asm!(
        "syscall",
        in("rax") numbers::SYS_EXIT,
        in("rdi") code as u64,
        options(noreturn)
    );
}

#[inline(always)]
pub unsafe fn sys_yield() -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_YIELD,
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
        in("rax") numbers::SYS_CHANNEL_CREATE,
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
        in("rax") numbers::SYS_CHANNEL_SEND,
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
        in("rax") numbers::SYS_CHANNEL_RECEIVE,
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
        in("rax") numbers::SYS_CHANNEL_CLOSE,
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
        in("rax") numbers::SYS_SHM_CREATE,
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
        in("rax") numbers::SYS_SHM_MAP,
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
        in("rax") numbers::SYS_SHM_UNMAP,
        in("rdi") handle as u64,
        in("rsi") vaddr,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserDirEntry {
    pub inode_num: u32,
    pub file_type: u8,
    pub name_len: u8,
    pub _reserved: u16,
    pub name: [u8; 56],
}

#[inline(always)]
pub unsafe fn sys_cap_derive(handle: u32, rights: u32, out_handle: *mut u32) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_CAP_DERIVE,
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

#[inline(always)]
pub unsafe fn sys_dev_map_mmio(handle: u32, res_idx: usize, out_vaddr_ptr: *mut u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_DEV_MAP_MMIO,
        in("rdi") handle as u64,
        in("rsi") res_idx as u64,
        in("rdx") out_vaddr_ptr as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_dev_reset(handle: u32, reset_flags: u32) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_DEV_RESET,
        in("rdi") handle as u64,
        in("rsi") reset_flags as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_dir_create(parent_handle: u32, name_ptr: *const u8, name_len: usize) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_DIR_CREATE,
        in("rdi") parent_handle as u64,
        in("rsi") name_ptr as u64,
        in("rdx") name_len as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_file_unlink(parent_handle: u32, name_ptr: *const u8, name_len: usize) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_FILE_UNLINK,
        in("rdi") parent_handle as u64,
        in("rsi") name_ptr as u64,
        in("rdx") name_len as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_file_rename(
    src_dir_handle: u32,
    src_name_ptr: *const u8,
    src_name_len: usize,
    dst_dir_handle: u32,
    dst_name_ptr: *const u8,
    dst_name_len: usize,
    flags: u32,
) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_FILE_RENAME,
        in("rdi") src_dir_handle as u64,
        in("rsi") src_name_ptr as u64,
        in("rdx") src_name_len as u64,
        in("r10") dst_dir_handle as u64,
        in("r8") dst_name_ptr as u64,
        in("r9") dst_name_len as u64,
        in("r11") flags as u64,
        out("rcx") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_dir_read(
    dir_handle: u32,
    entry_offset: u32,
    buf_ptr: *mut UserDirEntry,
    max_entries: usize,
    out_count_ptr: *mut u64,
    inout_captured_gen_ptr: *mut u32,
) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") numbers::SYS_DIR_READ,
        in("rdi") dir_handle as u64,
        in("rsi") entry_offset as u64,
        in("rdx") buf_ptr as u64,
        in("r10") max_entries as u64,
        in("r8") out_count_ptr as u64,
        in("r9") inout_captured_gen_ptr as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}
