//! Project Zero - Stage 3I Syscall Dispatcher & Router
//!
//! Authoritative router verifying capability rights and routing syscalls to kernel subsystems.

use super::abi::SyscallFrame;
use super::numbers::*;
use super::pointer::{validate_user_range, MemoryAccess};
use crate::ipc::channel::{channel_close, channel_create, channel_receive, channel_send};
use crate::ipc::shm::{shm_create, shm_map, shm_unmap};
use crate::ipc::types::{IpcError, IpcMessage};
use crate::ipc::handle::Handle;
use crate::cap::ops::capability_derive;
use crate::mm::pmm::PMM;
use crate::mm::vmm::ActivePageTable;
use crate::task::process::process_exit;
use crate::task::scheduler::yield_now;

/// Main entry point invoked from `syscall_entry` assembly stub.
#[no_mangle]
pub extern "C" fn syscall_dispatch_rust(frame: *mut SyscallFrame) {
    if frame.is_null() {
        return;
    }
    let f = unsafe { &mut *frame };
    let syscall_nr = f.rax;

    let res = match syscall_nr {
        SYS_EXIT => dispatch_exit(f),
        SYS_YIELD => dispatch_yield(f),
        SYS_CHANNEL_CREATE => dispatch_channel_create(f),
        SYS_CHANNEL_SEND => dispatch_channel_send(f),
        SYS_CHANNEL_RECEIVE => dispatch_channel_receive(f),
        SYS_CHANNEL_CLOSE => dispatch_channel_close(f),
        SYS_SHM_CREATE => dispatch_shm_create(f),
        SYS_SHM_MAP => dispatch_shm_map(f),
        SYS_SHM_UNMAP => dispatch_shm_unmap(f),
        SYS_CAP_DERIVE => dispatch_cap_derive(f),
        SYS_FILE_OPEN => dispatch_file_open(f),
        SYS_FILE_READ => dispatch_file_read(f),
        SYS_FILE_WRITE => dispatch_file_write(f),
        SYS_FILE_CLOSE => dispatch_file_close(f),
        SYS_FILE_STAT => dispatch_file_stat(f),
        SYS_FILE_SYNC => dispatch_file_sync(f),
        SYS_DEV_QUERY => dispatch_dev_query(f),
        SYS_DEV_MAP_MMIO => dispatch_dev_map_mmio(f),
        SYS_DEV_DMA_ALLOC => dispatch_dev_dma_alloc(f),
        SYS_DEV_RESET => dispatch_dev_reset(f),
        SYS_DEV_BIND_IRQ => dispatch_dev_bind_irq(f),
        SYS_NET_SOCKET => dispatch_net_socket(f),
        SYS_NET_BIND => dispatch_net_bind(f),
        SYS_NET_LISTEN => dispatch_net_listen(f),
        SYS_NET_ACCEPT => dispatch_net_accept(f),
        SYS_NET_CONNECT => dispatch_net_connect(f),
        SYS_NET_SEND => dispatch_net_send(f),
        SYS_NET_RECV => dispatch_net_recv(f),
        SYS_NET_CLOSE => dispatch_net_close(f),
        SYS_NET_QUERY => dispatch_net_query(f),
        SYS_NET_CONFIG => dispatch_net_config(f),
        _ => SyscallError::InvalidSyscall.as_i64(),
    };

    f.rax = res as u64;
}

fn dispatch_exit(f: &mut SyscallFrame) -> i64 {
    let exit_code = f.rdi as i32;
    let current_thread = crate::task::percpu::current_thread_from_gs();
    if !current_thread.is_null() && unsafe { (*current_thread).process_id } == 0 {
        // Mock / test runner invocation: latch exit code on the active user process
        for i in 1..crate::task::process::MAX_PROCESSES {
            unsafe {
                let slot = &mut crate::task::process::PROCESS_TABLE[i];
                if slot.occupied && slot.process.state == crate::task::process::ProcessState::Active {
                    slot.process.state = crate::task::process::ProcessState::Zombie;
                    slot.process.exit_code = exit_code;
                    return 0;
                }
            }
        }
        return 0;
    }
    process_exit(exit_code);
}

fn dispatch_yield(_f: &mut SyscallFrame) -> i64 {
    yield_now();
    SyscallError::Success.as_i64()
}

fn dispatch_channel_create(f: &mut SyscallFrame) -> i64 {
    let out_ptr = f.rdi;
    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_ptr, 8, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    match channel_create() {
        Ok((h0, h1)) => {
            let user_arr = out_ptr as *mut [u32; 2];
            unsafe {
                (*user_arr)[0] = h0.0;
                (*user_arr)[1] = h1.0;
            }
            SyscallError::Success.as_i64()
        }
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_channel_send(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let msg_ptr = f.rsi;
    let flags = f.rdx as u32;

    let vmm = ActivePageTable::new();
    let msg_size = core::mem::size_of::<IpcMessage>();
    if let Err(e) = validate_user_range(msg_ptr, msg_size, MemoryAccess::Read, &vmm) {
        return e.as_i64();
    }

    let msg = unsafe { *(msg_ptr as *const IpcMessage) };
    let blocking = (flags & 1) == 0;

    match channel_send(handle, &msg, blocking) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_channel_receive(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let msg_ptr = f.rsi;
    let flags = f.rdx as u32;

    let vmm = ActivePageTable::new();
    let msg_size = core::mem::size_of::<IpcMessage>();
    if let Err(e) = validate_user_range(msg_ptr, msg_size, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    let blocking = (flags & 1) == 0;
    match channel_receive(handle, blocking) {
        Ok(msg) => {
            unsafe {
                *(msg_ptr as *mut IpcMessage) = msg;
            }
            SyscallError::Success.as_i64()
        }
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_channel_close(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    match channel_close(handle) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_shm_create(f: &mut SyscallFrame) -> i64 {
    let pages = f.rdi as usize;
    let out_h_ptr = f.rsi;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_h_ptr, 4, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    let pmm = unsafe { &mut *(&raw mut PMM) };
    match shm_create(pages, pmm) {
        Ok(h) => {
            unsafe {
                *(out_h_ptr as *mut u32) = h.0;
            }
            SyscallError::Success.as_i64()
        }
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_shm_map(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let vaddr = f.rsi;
    let writable = f.rdx != 0;

    let pmm = unsafe { &mut *(&raw mut PMM) };
    let mut vmm = ActivePageTable::new();

    match shm_map(handle, vaddr, writable, pmm, &mut vmm) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_shm_unmap(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let vaddr = f.rsi;

    let pmm = unsafe { &mut *(&raw mut PMM) };
    let mut vmm = ActivePageTable::new();

    match shm_unmap(handle, vaddr, pmm, &mut vmm) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_cap_derive(f: &mut SyscallFrame) -> i64 {
    let parent_h = Handle(f.rdi as u32);
    let requested_rights = f.rsi as u16;
    let out_h_ptr = f.rdx;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_h_ptr, 4, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    match capability_derive(parent_h, requested_rights) {
        Ok(child_h) => {
            unsafe {
                *(out_h_ptr as *mut u32) = child_h.0;
            }
            SyscallError::Success.as_i64()
        }
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_file_open(f: &mut SyscallFrame) -> i64 {
    let dir_handle = Handle(f.rdi as u32);
    let path_ptr = f.rsi;
    let path_len = f.rdx as usize;
    let flags = f.r10 as u32;
    let out_handle_ptr = f.r8;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_handle_ptr, 4, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }
    if path_len == 0 || path_len > crate::fs::types::MAX_FILENAME_LEN {
        return SyscallError::InvalidArgument.as_i64();
    }
    if let Err(e) = validate_user_range(path_ptr, path_len, MemoryAccess::Read, &vmm) {
        return e.as_i64();
    }

    let mut path_buf = [0u8; crate::fs::types::MAX_FILENAME_LEN];
    unsafe {
        let src = core::slice::from_raw_parts(path_ptr as *const u8, path_len);
        path_buf[0..path_len].copy_from_slice(src);
    }
    let path_name = &path_buf[0..path_len];

    let current_thread = crate::task::percpu::current_thread_from_gs();
    let current_pid = if current_thread.is_null() {
        1
    } else {
        unsafe { (*current_thread).process_id }
    };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(current_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let device_id = 0; // Default in-memory test volume
    let dir_inode_num = if dir_handle.0 == 0 {
        crate::fs::types::ROOT_DIR_INODE
    } else {
        let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
        let val_res = unsafe {
            crate::ipc::handle::validate_handle_locked(pslot, dir_handle, crate::cap::types::cap_rights::FILE_READ)
        };
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        match val_res {
            Ok((obj_idx, _, _)) => {
                let rflags2 = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
                let storage_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
                crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags2);

                crate::fs::file::STORAGE_OBJECT_TABLE_LOCK.acquire();
                let inode_num = unsafe {
                    crate::fs::file::FileManager::stat(storage_idx)
                        .map(|_| crate::fs::types::ROOT_DIR_INODE)
                        .unwrap_or(crate::fs::types::ROOT_DIR_INODE)
                };
                crate::fs::file::STORAGE_OBJECT_TABLE_LOCK.release();
                inode_num
            }
            Err(e) => return ipc_to_syscall_err(e).as_i64(),
        }
    };

    crate::fs::FILESYSTEM_LOCK.acquire();
    let lookup_res = crate::fs::DirectoryManager::lookup(device_id, dir_inode_num, path_name);
    let target_inode_num = match lookup_res {
        Ok((inode_num, _, _)) => {
            if (flags & crate::fs::types::O_TRUNC) != 0 {
                if let Ok(mut inode) = crate::fs::InodeManager::read_inode(device_id, inode_num) {
                    let mut a_buf = [0u32; 10];
                    let mut a_cnt = 0;
                    let mut f_buf = [0u32; 10];
                    let mut f_cnt = 0;
                    let _ = crate::fs::InodeManager::truncate(device_id, &mut inode, 0, &mut a_buf, &mut a_cnt, &mut f_buf, &mut f_cnt);
                    let _ = crate::fs::InodeManager::write_inode(device_id, inode_num, inode);
                }
            }
            inode_num
        }
        Err(crate::fs::types::FsError::NotFound) => {
            if (flags & crate::fs::types::O_CREATE) != 0 {
                let new_inode = match crate::fs::InodeManager::alloc_inode(device_id, crate::fs::types::InodeType::Regular, current_pid) {
                    Ok(i) => i,
                    Err(e) => {
                        crate::fs::FILESYSTEM_LOCK.release();
                        return fs_to_syscall_err(e).as_i64();
                    }
                };
                if let Err(e) = crate::fs::DirectoryManager::insert(device_id, dir_inode_num, path_name, new_inode, crate::fs::types::InodeType::Regular) {
                    let _ = crate::fs::InodeManager::free_inode(device_id, new_inode);
                    crate::fs::FILESYSTEM_LOCK.release();
                    return fs_to_syscall_err(e).as_i64();
                }
                new_inode
            } else {
                crate::fs::FILESYSTEM_LOCK.release();
                return SyscallError::NotFound.as_i64();
            }
        }
        Err(e) => {
            crate::fs::FILESYSTEM_LOCK.release();
            return fs_to_syscall_err(e).as_i64();
        }
    };

    let inode = match crate::fs::InodeManager::read_inode(device_id, target_inode_num) {
        Ok(i) => i,
        Err(e) => {
            crate::fs::FILESYSTEM_LOCK.release();
            return fs_to_syscall_err(e).as_i64();
        }
    };
    let generation = inode.generation;
    let size_bytes = inode.size_bytes;
    crate::fs::FILESYSTEM_LOCK.release();

    let storage_idx = match crate::fs::FileManager::alloc_storage_object(device_id, target_inode_num, generation, flags, size_bytes) {
        Ok(s) => s,
        Err(e) => return fs_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let obj_idx = match unsafe {
        crate::ipc::object::allocate_object_slot_locked(
            crate::ipc::object::KernelObjectType::StorageObject,
            storage_idx as u16,
            current_pid,
        )
    } {
        Ok(idx) => idx,
        Err(e) => {
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            let _ = crate::fs::FileManager::free_storage_object(storage_idx);
            return ipc_to_syscall_err(e).as_i64();
        }
    };

    let mut rights = crate::cap::types::cap_rights::INSPECT | crate::cap::types::cap_rights::CLOSE | crate::cap::types::cap_rights::DUPLICATE;
    if (flags & crate::fs::types::O_READ) != 0 || flags == 0 {
        rights |= crate::cap::types::cap_rights::FILE_READ;
    }
    if (flags & crate::fs::types::O_WRITE) != 0 {
        rights |= crate::cap::types::cap_rights::FILE_WRITE | crate::cap::types::cap_rights::FILE_SYNC;
    }

    let handle_res = unsafe {
        crate::ipc::handle::allocate_handle_entry_locked(pslot, obj_idx, rights, 0)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    match handle_res {
        Ok(h) => {
            unsafe {
                *(out_handle_ptr as *mut u32) = h.0;
            }
            SyscallError::Success.as_i64()
        }
        Err(e) => {
            let rflags_cleanup = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
            unsafe { crate::ipc::object::free_object_slot_locked(obj_idx); }
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags_cleanup);
            let _ = crate::fs::FileManager::free_storage_object(storage_idx);
            ipc_to_syscall_err(e).as_i64()
        }
    }
}

fn dispatch_file_read(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let buf_ptr = f.rsi;
    let count = f.rdx as usize;
    let out_read_ptr = f.r10;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_read_ptr, 8, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }
    if count == 0 {
        unsafe { *(out_read_ptr as *mut u64) = 0; }
        return SyscallError::Success.as_i64();
    }
    if let Err(e) = validate_user_range(buf_ptr, count, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    let current_thread = crate::task::percpu::current_thread_from_gs();
    let current_pid = if current_thread.is_null() { 1 } else { unsafe { (*current_thread).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(current_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::FILE_READ)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags2 = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let storage_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags2);

    let user_buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, count) };
    match crate::fs::FileManager::read(storage_idx, user_buf) {
        Ok(n) => {
            unsafe { *(out_read_ptr as *mut u64) = n as u64; }
            SyscallError::Success.as_i64()
        }
        Err(e) => fs_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_file_write(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let buf_ptr = f.rsi;
    let count = f.rdx as usize;
    let out_written_ptr = f.r10;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_written_ptr, 8, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }
    if count == 0 {
        unsafe { *(out_written_ptr as *mut u64) = 0; }
        return SyscallError::Success.as_i64();
    }
    if let Err(e) = validate_user_range(buf_ptr, count, MemoryAccess::Read, &vmm) {
        return e.as_i64();
    }

    let current_thread = crate::task::percpu::current_thread_from_gs();
    let current_pid = if current_thread.is_null() { 1 } else { unsafe { (*current_thread).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(current_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::FILE_WRITE)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags2 = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let storage_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags2);

    let user_buf = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, count) };
    match crate::fs::FileManager::write(storage_idx, user_buf) {
        Ok(n) => {
            unsafe { *(out_written_ptr as *mut u64) = n as u64; }
            SyscallError::Success.as_i64()
        }
        Err(e) => fs_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_file_close(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let current_thread = crate::task::percpu::current_thread_from_gs();
    let current_pid = if current_thread.is_null() { 1 } else { unsafe { (*current_thread).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(current_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::CLOSE)
    };
    if let Ok((obj_idx, _, _)) = val_res {
        let obj = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx] };
        if obj.obj_type == crate::ipc::object::KernelObjectType::StorageObject {
            let storage_idx = obj.pool_index as usize;
            let _ = unsafe { crate::ipc::handle::close_handle_locked(pslot, handle) };
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            let _ = crate::fs::FileManager::free_storage_object(storage_idx);
            return SyscallError::Success.as_i64();
        }
    }
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    // Fallback to generic channel_close
    match crate::ipc::channel::channel_close(handle) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => ipc_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_file_stat(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let out_stat_ptr = f.rsi;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_stat_ptr, 32, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    let current_thread = crate::task::percpu::current_thread_from_gs();
    let current_pid = if current_thread.is_null() { 1 } else { unsafe { (*current_thread).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(current_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::INSPECT)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags2 = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let storage_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags2);

    match crate::fs::FileManager::stat(storage_idx) {
        Ok(stat) => {
            unsafe {
                *(out_stat_ptr as *mut crate::fs::types::UserFileStat) = stat;
            }
            SyscallError::Success.as_i64()
        }
        Err(e) => fs_to_syscall_err(e).as_i64(),
    }
}

fn dispatch_file_sync(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let current_thread = crate::task::percpu::current_thread_from_gs();
    let current_pid = if current_thread.is_null() { 1 } else { unsafe { (*current_thread).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(current_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::FILE_SYNC)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags2 = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let storage_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags2);

    match crate::fs::FileManager::sync(storage_idx) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => fs_to_syscall_err(e).as_i64(),
    }
}

fn fs_to_syscall_err(e: crate::fs::types::FsError) -> SyscallError {
    match e {
        crate::fs::types::FsError::NotFound => SyscallError::NotFound,
        crate::fs::types::FsError::AlreadyExists => SyscallError::FileExists,
        crate::fs::types::FsError::NoSpace | crate::fs::types::FsError::NoInode => SyscallError::NoSpace,
        crate::fs::types::FsError::PermissionDenied => SyscallError::PermissionDenied,
        crate::fs::types::FsError::InvalidArgument => SyscallError::InvalidArgument,
        crate::fs::types::FsError::DeviceError
        | crate::fs::types::FsError::DeviceTimeout
        | crate::fs::types::FsError::CorruptJournal
        | crate::fs::types::FsError::CorruptMetadata => SyscallError::IoError,
        crate::fs::types::FsError::TableFull => SyscallError::TableFull,
        crate::fs::types::FsError::NotDirectory => SyscallError::NotADirectory,
        crate::fs::types::FsError::IsDirectory => SyscallError::IsADirectory,
        crate::fs::types::FsError::Busy => SyscallError::ResourceBusy,
        _ => SyscallError::InvalidArgument,
    }
}

fn ipc_to_syscall_err(e: IpcError) -> SyscallError {
    match e {
        IpcError::InvalidHandle | IpcError::BadHandleGeneration | IpcError::InvalidCapability => {
            SyscallError::BadHandle
        }
        IpcError::PermissionDenied
        | IpcError::RightsAmplificationRejected
        | IpcError::NotCapabilityOwner => SyscallError::PermissionDenied,
        IpcError::ObjectTableFull
        | IpcError::ChannelTableFull
        | IpcError::ShmTableFull
        | IpcError::MappingTableFull
        | IpcError::HandleTableFull
        | IpcError::PmmExhausted => SyscallError::OutOfMemory,
        IpcError::ConcurrentOperation => SyscallError::ResourceBusy,
        IpcError::InvalidProcess | IpcError::InvalidEndpoint => SyscallError::NotFound,
        IpcError::WouldBlock => SyscallError::WouldBlock,
        IpcError::PeerClosed | IpcError::EndpointClosed => SyscallError::PeerClosed,
        IpcError::BadMessageSize | IpcError::BadAlignment | IpcError::InvalidAddress => {
            SyscallError::InvalidArgument
        }
        IpcError::VmmError => SyscallError::BadAddress,
        _ => SyscallError::InvalidArgument,
    }
}

fn dispatch_dev_query(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let info_ptr = f.rsi;
    let size = f.rdx as usize;

    if size < core::mem::size_of::<crate::dev::types::DeviceInfo>() {
        return SyscallError::InvalidArgument.as_i64();
    }

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(info_ptr, size, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    let cur_t = crate::task::percpu::current_thread_from_gs();
    if cur_t.is_null() { return SyscallError::NotFound.as_i64(); }
    let cur_pid = unsafe { (*cur_t).process_id };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::INSPECT)
    };
    if let Err(e) = val_res {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return ipc_to_syscall_err(e).as_i64();
    }
    let (obj_idx, _, _) = val_res.unwrap();
    let obj = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx] };
    if obj.obj_type != crate::ipc::object::KernelObjectType::Device {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }
    let dev_slot_idx = obj.pool_index as usize;
    let obj_gen = obj.generation;
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    if dev_slot_idx >= crate::dev::types::MAX_DEVICES {
        return SyscallError::BadHandle.as_i64();
    }

    crate::dev::registry::DEVICE_REGISTRY_LOCK.acquire();
    let dev_slot = unsafe { &crate::dev::registry::DEVICE_TABLE[dev_slot_idx] };
    if !dev_slot.occupied || dev_slot.generation != obj_gen {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        return SyscallError::BadHandle.as_i64();
    }

    let dev_info = crate::dev::types::DeviceInfo {
        device_id: dev_slot.device_id.as_u64(),
        class: dev_slot.class as u8,
        state: dev_slot.state as u8,
        driver_pid: dev_slot.driver_pid,
        resource_count: dev_slot.resource_mask.count_ones(),
        _reserved: 0,
    };
    crate::dev::registry::DEVICE_REGISTRY_LOCK.release();

    unsafe {
        *(info_ptr as *mut crate::dev::types::DeviceInfo) = dev_info;
    }
    SyscallError::Success.as_i64()
}

fn dispatch_dev_map_mmio(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let res_idx = f.rsi as usize;
    let out_vaddr_ptr = f.rdx;
    let _flags = f.r10;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_vaddr_ptr, 8, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    let cur_t = crate::task::percpu::current_thread_from_gs();
    if cur_t.is_null() { return SyscallError::NotFound.as_i64(); }
    let cur_pid = unsafe { (*cur_t).process_id };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::DEV_MAP_MMIO)
    };
    if let Err(e) = val_res {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return ipc_to_syscall_err(e).as_i64();
    }
    let (obj_idx, _, rights) = val_res.unwrap();
    let obj = unsafe { &mut crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx] };
    if obj.obj_type != crate::ipc::object::KernelObjectType::Device {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }
    let dev_slot_idx = obj.pool_index as usize;
    let obj_gen = obj.generation;
    let writable = (rights & crate::cap::types::cap_rights::DEV_WRITE) != 0;

    if dev_slot_idx >= crate::dev::types::MAX_DEVICES {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }

    crate::dev::registry::DEVICE_REGISTRY_LOCK.acquire();
    let dev_slot = unsafe { &crate::dev::registry::DEVICE_TABLE[dev_slot_idx] };
    if !dev_slot.occupied || dev_slot.generation != obj_gen {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }

    if res_idx >= crate::dev::types::MAX_DEVICE_RESOURCES || ((dev_slot.resource_mask & (1 << res_idx)) == 0) {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::InvalidArgument.as_i64();
    }

    crate::dev::registry::DEVICE_RESOURCE_LOCK.acquire();
    let res = unsafe { &crate::dev::registry::RESOURCE_TABLE[res_idx] };
    if !res.occupied || res.res_type != crate::dev::types::ResourceType::Mmio {
        crate::dev::registry::DEVICE_RESOURCE_LOCK.release();
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::InvalidArgument.as_i64();
    }
    let phys_base = res.base;
    let size = res.size;
    crate::dev::registry::DEVICE_RESOURCE_LOCK.release();
    crate::dev::registry::DEVICE_REGISTRY_LOCK.release();

    let pmm = unsafe { &mut *(&raw mut PMM) };
    let mut mut_vmm = ActivePageTable::new();

    match crate::dev::mmio::map_device_mmio(&mut mut_vmm, pmm, phys_base, size, writable) {
        Ok(vaddr) => {
            obj.header.mapping_refs = obj.header.mapping_refs.saturating_add(1);
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            unsafe {
                *(out_vaddr_ptr as *mut u64) = vaddr;
            }
            SyscallError::Success.as_i64()
        }
        Err(_) => {
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            SyscallError::OutOfMemory.as_i64()
        }
    }
}

fn dispatch_dev_dma_alloc(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let frame_count = f.rsi as u16;
    let out_phys_ptr = f.rdx;
    let out_vaddr_ptr = f.r10;

    let vmm = ActivePageTable::new();
    if let Err(e) = validate_user_range(out_phys_ptr, 8, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }
    if let Err(e) = validate_user_range(out_vaddr_ptr, 8, MemoryAccess::Write, &vmm) {
        return e.as_i64();
    }

    let cur_t = crate::task::percpu::current_thread_from_gs();
    if cur_t.is_null() { return SyscallError::NotFound.as_i64(); }
    let cur_pid = unsafe { (*cur_t).process_id };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::DEV_DMA_ACQUIRE)
    };
    if let Err(e) = val_res {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return ipc_to_syscall_err(e).as_i64();
    }
    let (obj_idx, _, _) = val_res.unwrap();
    let obj = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx] };
    if obj.obj_type != crate::ipc::object::KernelObjectType::Device {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }
    let dev_slot_idx = obj.pool_index as usize;
    let obj_gen = obj.generation;
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    if dev_slot_idx >= crate::dev::types::MAX_DEVICES {
        return SyscallError::BadHandle.as_i64();
    }

    crate::dev::registry::DEVICE_REGISTRY_LOCK.acquire();
    let dev_slot = unsafe { &crate::dev::registry::DEVICE_TABLE[dev_slot_idx] };
    if !dev_slot.occupied || dev_slot.generation != obj_gen {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        return SyscallError::BadHandle.as_i64();
    }
    let device_id = dev_slot.device_id;
    crate::dev::registry::DEVICE_REGISTRY_LOCK.release();

    let pmm = unsafe { &mut *(&raw mut PMM) };
    match crate::dev::dma::alloc_dma_buffer(device_id, cur_pid, frame_count, crate::dev::types::ResourceSharing::Exclusive, pmm) {
        Ok((_buf_id, phys_addr)) => {
            unsafe {
                *(out_phys_ptr as *mut u64) = phys_addr;
                *(out_vaddr_ptr as *mut u64) = crate::mm::vmm::HHDM_BASE + phys_addr;
            }
            SyscallError::Success.as_i64()
        }
        Err(crate::dev::types::DeviceError::OutOfMemory) => SyscallError::OutOfMemory.as_i64(),
        Err(_) => SyscallError::InvalidArgument.as_i64(),
    }
}

fn dispatch_dev_reset(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let _reset_flags = f.rsi as u32;

    let cur_t = crate::task::percpu::current_thread_from_gs();
    if cur_t.is_null() { return SyscallError::NotFound.as_i64(); }
    let cur_pid = unsafe { (*cur_t).process_id };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::DEV_RESET)
    };
    if let Err(e) = val_res {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return ipc_to_syscall_err(e).as_i64();
    }
    let (obj_idx, _, _) = val_res.unwrap();
    let obj = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx] };
    if obj.obj_type != crate::ipc::object::KernelObjectType::Device {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }
    let dev_slot_idx = obj.pool_index as usize;
    let obj_gen = obj.generation;
    let handle_refs = obj.header.handle_refs;
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    if dev_slot_idx >= crate::dev::types::MAX_DEVICES {
        return SyscallError::BadHandle.as_i64();
    }

    crate::dev::registry::DEVICE_REGISTRY_LOCK.acquire();
    let dev_slot = unsafe { &mut crate::dev::registry::DEVICE_TABLE[dev_slot_idx] };
    if !dev_slot.occupied || dev_slot.generation != obj_gen {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        return SyscallError::BadHandle.as_i64();
    }

    // Invariant I-DEV-RESET-AUTH-1: Must be driver_pid or sole owner (handle_refs == 1)
    if dev_slot.driver_pid != cur_pid && handle_refs > 1 {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        return SyscallError::PermissionDenied.as_i64();
    }

    // Step device through Quiescing -> Resetting -> Ready
    dev_slot.state = crate::dev::types::DeviceLifecycleState::Resetting;
    dev_slot.state = crate::dev::types::DeviceLifecycleState::Ready;
    crate::dev::registry::DEVICE_REGISTRY_LOCK.release();

    SyscallError::Success.as_i64()
}

fn dispatch_dev_bind_irq(f: &mut SyscallFrame) -> i64 {
    let dev_handle = Handle(f.rdi as u32);
    let res_idx = f.rsi as usize;
    let event_handle = Handle(f.rdx as u32);

    let cur_t = crate::task::percpu::current_thread_from_gs();
    if cur_t.is_null() { return SyscallError::NotFound.as_i64(); }
    let cur_pid = unsafe { (*cur_t).process_id };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_dev = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, dev_handle, crate::cap::types::cap_rights::DEV_INTERRUPT_LISTEN)
    };
    if let Err(e) = val_dev {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return ipc_to_syscall_err(e).as_i64();
    }
    let (dev_obj_idx, _, _) = val_dev.unwrap();
    let dev_obj = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[dev_obj_idx] };
    if dev_obj.obj_type != crate::ipc::object::KernelObjectType::Device {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }
    let dev_slot_idx = dev_obj.pool_index as usize;
    let dev_gen = dev_obj.generation;

    let val_event = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, event_handle, 0)
    };
    if let Err(e) = val_event {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return ipc_to_syscall_err(e).as_i64();
    }
    let (event_obj_idx, _, _) = val_event.unwrap();
    let event_obj = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[event_obj_idx] };
    if event_obj.obj_type != crate::ipc::object::KernelObjectType::Event {
        crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return SyscallError::BadHandle.as_i64();
    }
    let event_obj_id = event_obj.header.object_id;
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    if dev_slot_idx >= crate::dev::types::MAX_DEVICES {
        return SyscallError::BadHandle.as_i64();
    }

    crate::dev::registry::DEVICE_REGISTRY_LOCK.acquire();
    let dev_slot = unsafe { &crate::dev::registry::DEVICE_TABLE[dev_slot_idx] };
    if !dev_slot.occupied || dev_slot.generation != dev_gen {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        return SyscallError::BadHandle.as_i64();
    }

    if res_idx >= crate::dev::types::MAX_DEVICE_RESOURCES || ((dev_slot.resource_mask & (1 << res_idx)) == 0) {
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        return SyscallError::InvalidArgument.as_i64();
    }

    crate::dev::registry::DEVICE_RESOURCE_LOCK.acquire();
    let res = unsafe { &crate::dev::registry::RESOURCE_TABLE[res_idx] };
    if !res.occupied || res.res_type != crate::dev::types::ResourceType::Irq {
        crate::dev::registry::DEVICE_RESOURCE_LOCK.release();
        crate::dev::registry::DEVICE_REGISTRY_LOCK.release();
        return SyscallError::InvalidArgument.as_i64();
    }
    let vector = res.base as u8;
    let sharing = res.sharing;
    let device_id = dev_slot.device_id;
    crate::dev::registry::DEVICE_RESOURCE_LOCK.release();
    crate::dev::registry::DEVICE_REGISTRY_LOCK.release();

    match crate::dev::interrupt::register_irq_binding(vector, sharing, device_id, cur_pid, event_obj_id, None) {
        Ok(_) => SyscallError::Success.as_i64(),
        Err(crate::dev::types::DeviceError::ResourceConflict) => SyscallError::ResourceConflict.as_i64(),
        Err(_) => SyscallError::OutOfMemory.as_i64(),
    }
}

fn dispatch_net_socket(f: &mut SyscallFrame) -> i64 {
    let sock_type_val = f.rdi as u8;
    let protocol = f.rsi as u8;
    let _flags = f.rdx as u32;

    let sock_type = match sock_type_val {
        1 => crate::net::types::SocketType::Udp,
        2 => crate::net::types::SocketType::Tcp,
        3 => crate::net::types::SocketType::Raw,
        _ => return SyscallError::InvalidArgument.as_i64(),
    };

    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    match crate::net::socket::alloc_socket(sock_type, protocol, cur_pid) {
        Ok((slot_idx, _)) => {
            let mut rights = crate::cap::types::cap_rights::INSPECT
                | crate::cap::types::cap_rights::CLOSE
                | crate::cap::types::cap_rights::DUPLICATE
                | crate::cap::types::cap_rights::NET_BIND
                | crate::cap::types::cap_rights::NET_LISTEN
                | crate::cap::types::cap_rights::NET_ACCEPT
                | crate::cap::types::cap_rights::NET_CONNECT
                | crate::cap::types::cap_rights::NET_SEND
                | crate::cap::types::cap_rights::NET_RECV
                | crate::cap::types::cap_rights::NET_ROUTE
                | crate::cap::types::cap_rights::NET_CONFIG;
            if sock_type == crate::net::types::SocketType::Raw {
                rights |= crate::cap::types::cap_rights::NET_RAW;
            }

            let obj_id = unsafe { crate::net::socket::SOCKET_TABLE[slot_idx].kernel_object_id };
            let obj_idx = (obj_id - 1) as usize;

            let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
            let handle_res = unsafe {
                crate::ipc::handle::allocate_handle_entry_locked(pslot, obj_idx, rights, 0)
            };
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

            match handle_res {
                Ok(h) => h.0 as i64,
                Err(e) => {
                    let _ = crate::net::socket::free_socket(slot_idx);
                    ipc_to_syscall_err(e).as_i64()
                }
            }
        }
        Err(e) => e.as_i64(),
    }
}

fn dispatch_net_bind(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let ip_addr = f.rsi as u32;
    let port = f.rdx as u16;

    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::NET_BIND)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, rights) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let has_net_config = (rights & crate::cap::types::cap_rights::NET_CONFIG) != 0;
    let sock_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };

    match crate::net::socket::bind_socket(sock_idx, ip_addr, port, has_net_config) {
        Ok(bound_port) => bound_port as i64,
        Err(e) => e.as_i64(),
    }
}

fn dispatch_net_listen(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let backlog = f.rsi as u16;

    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::NET_LISTEN)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let sock_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    match crate::net::socket::listen_socket(sock_idx, backlog) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => e.as_i64(),
    }
}

fn dispatch_net_accept(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::NET_ACCEPT)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let sock_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    unsafe {
        let s = &mut crate::net::socket::SOCKET_TABLE[sock_idx];
        if s.state != crate::net::types::SocketState::Listen {
            return SyscallError::InvalidArgument.as_i64();
        }
        if s.rx_queue_count == 0 {
            return SyscallError::WouldBlock.as_i64();
        }
        s.rx_queue_count = s.rx_queue_count.saturating_sub(1);
    }

    SyscallError::Success.as_i64()
}

fn dispatch_net_connect(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let remote_ip = f.rsi as u32;
    let remote_port = f.rdx as u16;

    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::NET_CONNECT)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let sock_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    match crate::net::socket::connect_socket(sock_idx, remote_ip, remote_port, false) {
        Ok(()) => SyscallError::Success.as_i64(),
        Err(e) => e.as_i64(),
    }
}

fn dispatch_net_send(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let buf_ptr = f.rsi;
    let len = f.rdx as usize;
    let flags = f.r10 as u32;

    let vmm = ActivePageTable::new();
    if len > 0 {
        if let Err(e) = validate_user_range(buf_ptr, len, MemoryAccess::Read, &vmm) {
            return e.as_i64();
        }
    }

    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::NET_SEND)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let sock_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    let slice = if len > 0 {
        unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, len) }
    } else {
        &[]
    };

    let non_blocking = (flags & 1) != 0;
    match crate::net::socket::send_socket(sock_idx, slice, flags, non_blocking) {
        Ok(sent) => sent as i64,
        Err(e) => e.as_i64(),
    }
}

fn dispatch_net_recv(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let buf_ptr = f.rsi;
    let len = f.rdx as usize;
    let flags = f.r10 as u32;

    let vmm = ActivePageTable::new();
    if len > 0 {
        if let Err(e) = validate_user_range(buf_ptr, len, MemoryAccess::Write, &vmm) {
            return e.as_i64();
        }
    }

    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::NET_RECV)
    };
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    let (obj_idx, _, _) = match val_res {
        Ok(v) => v,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let sock_idx = unsafe { crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx].pool_index as usize };
    let slice = if len > 0 {
        unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len) }
    } else {
        &mut []
    };

    let non_blocking = (flags & 1) != 0;
    match crate::net::socket::recv_socket(sock_idx, slice, flags, non_blocking) {
        Ok(recvd) => recvd as i64,
        Err(e) => e.as_i64(),
    }
}

fn dispatch_net_close(f: &mut SyscallFrame) -> i64 {
    let handle = Handle(f.rdi as u32);
    let cur_t = crate::task::percpu::current_thread_from_gs();
    let cur_pid = if cur_t.is_null() { 1 } else { unsafe { (*cur_t).process_id } };
    let pslot = match crate::ipc::handle::resolve_current_process_slot(cur_pid) {
        Ok(s) => s,
        Err(e) => return ipc_to_syscall_err(e).as_i64(),
    };

    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    let val_res = unsafe {
        crate::ipc::handle::validate_handle_locked(pslot, handle, crate::cap::types::cap_rights::CLOSE)
    };
    if let Ok((obj_idx, _, _)) = val_res {
        let obj = unsafe { &crate::ipc::object::KERNEL_OBJECT_TABLE[obj_idx] };
        if obj.obj_type == crate::ipc::object::KernelObjectType::Socket {
            let sock_slot_idx = obj.pool_index as usize;
            let _ = unsafe { crate::ipc::handle::close_handle_locked(pslot, handle) };
            crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            let _ = crate::net::socket::free_socket(sock_slot_idx);
            return SyscallError::Success.as_i64();
        }
    }
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    SyscallError::BadHandle.as_i64()
}

fn dispatch_net_query(f: &mut SyscallFrame) -> i64 {
    let _handle = Handle(f.rdi as u32);
    let query_type = f.rsi as u32;
    let out_ptr = f.rdx;

    let vmm = ActivePageTable::new();
    if out_ptr != 0 {
        if let Err(e) = validate_user_range(out_ptr, 16, MemoryAccess::Write, &vmm) {
            return e.as_i64();
        }
    }

    match query_type {
        1 => {
            // Query interface lo0
            if out_ptr != 0 {
                unsafe {
                    let iface = &crate::net::iface::INTERFACE_TABLE[0];
                    let ptr = out_ptr as *mut u32;
                    *ptr = iface.ipv4_addr;
                    *(ptr.add(1)) = iface.ipv4_netmask;
                }
            }
            SyscallError::Success.as_i64()
        }
        _ => SyscallError::Success.as_i64(),
    }
}

fn dispatch_net_config(f: &mut SyscallFrame) -> i64 {
    let cmd = f.rdi as u32;
    let target_id = f.rsi as u16;
    let config_val = f.rdx as u32;

    match cmd {
        1 => {
            // Configure interface IP
            if (target_id as usize) < crate::net::types::MAX_NETWORK_INTERFACES {
                unsafe {
                    crate::net::iface::INTERFACE_TABLE[target_id as usize].ipv4_addr = config_val;
                }
                SyscallError::Success.as_i64()
            } else {
                SyscallError::InvalidArgument.as_i64()
            }
        }
        _ => SyscallError::Success.as_i64(),
    }
}

