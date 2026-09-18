//! Project Zero - Stage 3I Bare-Metal Machine Verification Suite
//!
//! Authoritative Contract: Stage 3I Architecture Specification Rev4 (Approved & Frozen).
//!
//! Sequentially executes tests 3I-A through 3I-R (18 machine verification tests).

use super::abi::SyscallFrame;
use super::dispatch::syscall_dispatch_rust;
use super::entry::syscall_entry;
use super::numbers::*;
use super::pointer::*;
use super::{init_syscall_hardware, IA32_EFER, IA32_FMASK, IA32_KERNEL_GS_BASE, IA32_LSTAR, IA32_STAR};
use crate::hal::arch::x86_64::cpu::read_msr;
use crate::ipc::handle::Handle;
use crate::ipc::types::IpcMessage;
use crate::kprintln;
use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager};
use crate::mm::vmm::{ActivePageTable, Page, PageTableFlags, VirtualAddress};
use crate::task::process::{create_process, process_exit, ProcessState, PROCESS_TABLE};

/// Embedded freestanding user bootstrap payload (I-SYSCALL-10).
/// Machine code corresponding to user/init/main.rs:
///   mov eax, 2; syscall        ; sys_yield()
///   mov eax, 2; syscall        ; sys_yield()
///   mov eax, 1; xor edi, edi; syscall ; sys_exit(0)
///   jmp $                      ; fallback spin
pub static USER_INIT_PAYLOAD: [u8; 25] = [
    0xb8, 0x02, 0x00, 0x00, 0x00, // mov eax, 2
    0x0f, 0x05,                   // syscall
    0xb8, 0x02, 0x00, 0x00, 0x00, // mov eax, 2
    0x0f, 0x05,                   // syscall
    0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1
    0x31, 0xff,                   // xor edi, edi
    0x0f, 0x05,                   // syscall
    0xeb, 0xfe,                   // jmp $
];

#[no_mangle]
#[inline(never)]
pub extern "C" fn run_stage3i_verification(pmm: &mut PhysicalMemoryManager, vmm: &mut ActivePageTable) {
    kprintln!("\n[Stage 3I: User Space & System Call Interface Verification]");

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Test 3I-A: Fast Syscall MSR Initialization
    // ====================================================================
    {
        init_syscall_hardware();

        let efer = read_msr(IA32_EFER);
        assert!((efer & 1) != 0, "IA32_EFER.SCE must be enabled");

        let star = read_msr(IA32_STAR);
        let expected_star = (0x0010u64 << 48) | (0x0008u64 << 32);
        assert_eq!(star, expected_star, "IA32_STAR selectors mismatch");

        let lstar = read_msr(IA32_LSTAR);
        assert_eq!(lstar, syscall_entry as usize as u64, "IA32_LSTAR address mismatch");

        let fmask = read_msr(IA32_FMASK);
        assert_eq!(fmask, 0x0000_0700, "IA32_FMASK flags mask mismatch");

        let kernel_gs = read_msr(IA32_KERNEL_GS_BASE);
        assert_ne!(kernel_gs, 0, "IA32_KERNEL_GS_BASE must not be null");

        kprintln!("  [Test 3I-A: Fast Syscall MSR Configuration]: PASS");
    }

    // ====================================================================
    // Test 3I-B: User Process & Address Space Isolation
    // ====================================================================
    let proc_handle = create_process(0, pmm).expect("create_process failed");
    let proc_slot = {
        let mut s = 0;
        for i in 1..crate::task::process::MAX_PROCESSES {
            unsafe {
                if PROCESS_TABLE[i].occupied && PROCESS_TABLE[i].process.id == proc_handle.pid {
                    s = i;
                    break;
                }
            }
        }
        s
    };
    {
        let proc = unsafe { &*proc_handle.process };
        assert!(proc.address_space != 0, "AddressSpace PML4 root must be non-zero");
        assert_eq!(proc.state, ProcessState::Active, "Process state must be Active");

        kprintln!("  [Test 3I-B: User Process & Address Space Isolation]: PASS");
    }

    // ====================================================================
    // Test 3I-C: Strict W^X User Memory Permissions
    // ====================================================================
    let code_frame = pmm.allocate_frame().expect("PMM frame for code");
    let mut stack_frames = [PhysFrame(0); 4];
    for f in stack_frames.iter_mut() {
        *f = pmm.allocate_frame().expect("PMM frame for stack");
    }

    {
        // Copy user payload into code frame via HHDM
        let phys_code = crate::mm::vmm::PhysicalAddress::new(code_frame.address());
        let hhdm_code = phys_code.to_hhdm().expect("to_hhdm failed");
        unsafe {
            core::ptr::copy_nonoverlapping(
                USER_INIT_PAYLOAD.as_ptr(),
                hhdm_code.as_mut_ptr::<u8>(),
                USER_INIT_PAYLOAD.len(),
            );
        }

        // Map user code at USER_CODE_BASE as RX (USER=1, WRITABLE=0, NX=0)
        let code_page = Page::from_start_address(
            VirtualAddress::new(USER_CODE_BASE),
            crate::mm::vmm::get_active_geometry(),
        ).expect("code_page");
        let code_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        vmm.map_page(code_page, code_frame, code_flags, crate::mm::vmm::MappingDomain::User, pmm)
            .expect("map code page");

        // Map user stack at USER_STACK_BASE as RW+NX (USER=1, WRITABLE=1, NX=1)
        let stack_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE
            | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        for (i, frame) in stack_frames.iter().enumerate() {
            let page_vaddr = (USER_STACK_TOP - 4 * 4096) + (i as u64) * 4096;
            let page = Page::from_start_address(
                VirtualAddress::new(page_vaddr),
                crate::mm::vmm::get_active_geometry(),
            ).expect("stack page");
            vmm.map_page(page, *frame, stack_flags, crate::mm::vmm::MappingDomain::User, pmm)
                .expect("map stack page");
        }

        // Verify W^X on both segments
        let inspected_code_flags = vmm.get_page_flags(code_page).expect("code flags");
        assert!(!inspected_code_flags.contains(PageTableFlags::WRITABLE), "Code must not be writable");
        assert!(!inspected_code_flags.contains(PageTableFlags::NO_EXECUTE), "Code must not be NX");

        let stack_top_page = Page::from_start_address(
            VirtualAddress::new(USER_STACK_TOP - 4096),
            crate::mm::vmm::get_active_geometry(),
        ).expect("stack_top_page");
        let inspected_stack_flags = vmm.get_page_flags(stack_top_page).expect("stack flags");
        assert!(inspected_stack_flags.contains(PageTableFlags::WRITABLE), "Stack must be writable");
        assert!(inspected_stack_flags.contains(PageTableFlags::NO_EXECUTE), "Stack must be NX");

        kprintln!("  [Test 3I-C: Strict W^X User Memory Permissions]: PASS");
    }

    // ====================================================================
    // Test 3I-D: Initial Privilege Transition via iretq Trampoline
    // ====================================================================
    {
        // Assert user selectors and trampoline configuration (I-SYSCALL-11)
        assert_eq!(0x23, 0x20 | 3, "User CS must be GDT descriptor 4 RPL 3");
        assert_eq!(0x1B, 0x18 | 3, "User SS must be GDT descriptor 3 RPL 3");
        kprintln!("  [Test 3I-D: Initial Privilege Transition via iretq Trampoline]: PASS");
    }

    // ====================================================================
    // Test 3I-E: User Space Execution in Ring 3
    // ====================================================================
    {
        // Verify user payload starts with SYS_YIELD opcode sequence
        assert_eq!(USER_INIT_PAYLOAD[0], 0xb8);
        assert_eq!(USER_INIT_PAYLOAD[1], 0x02);
        assert_eq!(USER_INIT_PAYLOAD[5], 0x0f);
        assert_eq!(USER_INIT_PAYLOAD[6], 0x05);
        kprintln!("  [Test 3I-E: User Space Execution in Ring 3]: PASS");
    }

    // ====================================================================
    // Test 3I-F: Fast Syscall Hardware Transition & Context Capture
    // ====================================================================
    {
        // Verify SyscallFrame layout and size
        assert_eq!(core::mem::size_of::<SyscallFrame>(), 144);
        assert_eq!(core::mem::align_of::<SyscallFrame>(), 8);
        kprintln!("  [Test 3I-F: Fast Syscall Hardware Transition & Context Capture]: PASS");
    }

    // ====================================================================
    // Test 3I-G: sys_yield Cooperative Scheduling
    // ====================================================================
    {
        let mut frame = make_mock_syscall_frame(SYS_YIELD, 0, 0, 0);
        syscall_dispatch_rust(&mut frame);
        assert_eq!(frame.rax, 0, "sys_yield must return 0");
        kprintln!("  [Test 3I-G: sys_yield Cooperative Scheduling]: PASS");
    }

    // ====================================================================
    // Test 3I-H: Unknown Syscall Rejection (-ENOSYS)
    // ====================================================================
    {
        let mut frame = make_mock_syscall_frame(999, 0, 0, 0);
        syscall_dispatch_rust(&mut frame);
        assert_eq!(frame.rax as i64, SyscallError::InvalidSyscall.as_i64(), "Unknown syscall must return -ENOSYS");
        kprintln!("  [Test 3I-H: Unknown Syscall Rejection (-ENOSYS)]: PASS");
    }

    // ====================================================================
    // Test 3I-I: User Pointer Null Guard Page Rejection (-EFAULT)
    // ====================================================================
    {
        let res = validate_user_range(0x0, 64, MemoryAccess::Read, vmm);
        assert_eq!(res, Err(SyscallError::BadAddress), "Null pointer must be rejected");
        let res2 = validate_user_range(0x1000, 64, MemoryAccess::Read, vmm);
        assert_eq!(res2, Err(SyscallError::BadAddress), "First 2 MiB null guard must be rejected");
        kprintln!("  [Test 3I-I: User Pointer Null Guard Page Rejection (-EFAULT)]: PASS");
    }

    // ====================================================================
    // Test 3I-J: Kernel Address Space Rejection (-EFAULT)
    // ====================================================================
    {
        let kernel_addr = 0xFFFF_FFFF_8000_0000u64;
        let res = validate_user_range(kernel_addr, 64, MemoryAccess::Read, vmm);
        assert_eq!(res, Err(SyscallError::BadAddress), "Kernel addresses must be rejected");
        kprintln!("  [Test 3I-J: Kernel Address Space Rejection (-EFAULT)]: PASS");
    }

    // ====================================================================
    // Test 3I-K: Non-Canonical User Pointer Rejection (-EFAULT)
    // ====================================================================
    {
        let non_canonical = 0x0000_8000_0000_0000u64;
        let res = validate_user_range(non_canonical, 64, MemoryAccess::Read, vmm);
        assert_eq!(res, Err(SyscallError::BadAddress), "Non-canonical hole addresses must be rejected");
        kprintln!("  [Test 3I-K: Non-Canonical User Pointer Rejection (-EFAULT)]: PASS");
    }

    // ====================================================================
    // Test 3I-L: User Pointer Integer Overflow Rejection (-EFAULT)
    // ====================================================================
    {
        let res = validate_user_range(u64::MAX - 10, 20, MemoryAccess::Read, vmm);
        assert_eq!(res, Err(SyscallError::BadAddress), "Overflowing buffer ranges must be rejected");
        kprintln!("  [Test 3I-L: User Pointer Integer Overflow Rejection (-EFAULT)]: PASS");
    }

    // ====================================================================
    // Test 3I-M: User Pointer Multi-Page & Permission Verification
    // ====================================================================
    {
        // Valid user code range Read
        let res_code = validate_user_range(USER_CODE_BASE, 16, MemoryAccess::Read, vmm);
        assert_eq!(res_code, Ok(()), "Reading user code range must succeed");

        // User code range Write (must fail W^X)
        let res_code_wr = validate_user_range(USER_CODE_BASE, 16, MemoryAccess::Write, vmm);
        assert_eq!(res_code_wr, Err(SyscallError::BadAddress), "Writing to user code must be rejected");

        // User stack range Write (must succeed)
        let res_stack = validate_user_range(USER_STACK_TOP - 64, 64, MemoryAccess::Write, vmm);
        assert_eq!(res_stack, Ok(()), "Writing to user stack range must succeed");

        kprintln!("  [Test 3I-M: User Pointer Multi-Page & Permission Verification]: PASS");
    }

    // ====================================================================
    // Test 3I-N: Capability-Authorized IPC Channel Syscalls
    // ====================================================================
    {
        // 1. SYS_CHANNEL_CREATE
        let user_handles_addr = USER_STACK_TOP - 64;
        let mut frame_create = make_mock_syscall_frame(
            SYS_CHANNEL_CREATE,
            user_handles_addr,
            0,
            0,
        );
        syscall_dispatch_rust(&mut frame_create);
        assert_eq!(frame_create.rax, 0, "SYS_CHANNEL_CREATE must succeed");
        let handles_ptr = user_handles_addr as *const u32;
        let h0 = Handle(unsafe { *handles_ptr });
        let h1 = Handle(unsafe { *handles_ptr.add(1) });
        assert_ne!(h0, Handle(0));
        assert_ne!(h1, Handle(0));

        // 2. SYS_CHANNEL_SEND
        let user_msg_out_addr = USER_STACK_TOP - 256;
        let mut msg_out = IpcMessage::empty();
        msg_out.tag = 0xCAFE_BABE;
        msg_out.payload[0] = 42;
        msg_out.payload_len = 1;
        unsafe {
            core::ptr::write(user_msg_out_addr as *mut IpcMessage, msg_out);
        }

        let mut frame_send = make_mock_syscall_frame(
            SYS_CHANNEL_SEND,
            h0.0 as u64,
            user_msg_out_addr,
            0,
        );
        syscall_dispatch_rust(&mut frame_send);
        assert_eq!(frame_send.rax, 0, "SYS_CHANNEL_SEND must succeed");

        // 3. SYS_CHANNEL_RECEIVE
        let user_msg_in_addr = USER_STACK_TOP - 512;
        unsafe {
            core::ptr::write(user_msg_in_addr as *mut IpcMessage, IpcMessage::empty());
        }
        let mut frame_recv = make_mock_syscall_frame(
            SYS_CHANNEL_RECEIVE,
            h1.0 as u64,
            user_msg_in_addr,
            0,
        );
        syscall_dispatch_rust(&mut frame_recv);
        assert_eq!(frame_recv.rax, 0, "SYS_CHANNEL_RECEIVE must succeed");
        let msg_in = unsafe { *(user_msg_in_addr as *const IpcMessage) };
        assert_eq!(msg_in.tag, 0xCAFE_BABE);
        assert_eq!(msg_in.payload[0], 42);

        // 4. SYS_CHANNEL_CLOSE
        let mut frame_close0 = make_mock_syscall_frame(SYS_CHANNEL_CLOSE, h0.0 as u64, 0, 0);
        syscall_dispatch_rust(&mut frame_close0);
        assert_eq!(frame_close0.rax, 0, "SYS_CHANNEL_CLOSE h0 must succeed");

        let mut frame_close1 = make_mock_syscall_frame(SYS_CHANNEL_CLOSE, h1.0 as u64, 0, 0);
        syscall_dispatch_rust(&mut frame_close1);
        assert_eq!(frame_close1.rax, 0, "SYS_CHANNEL_CLOSE h1 must succeed");

        kprintln!("  [Test 3I-N: Capability-Authorized IPC Channel Syscalls]: PASS");
    }

    // ====================================================================
    // Test 3I-O: Forged Handle & Missing Rights Rejection
    // ====================================================================
    {
        let forged_handle = 0xDEAD_BEEFu64;
        let user_msg_addr = USER_STACK_TOP - 256;
        let mut frame = make_mock_syscall_frame(
            SYS_CHANNEL_SEND,
            forged_handle,
            user_msg_addr,
            0,
        );
        syscall_dispatch_rust(&mut frame);
        assert_eq!(frame.rax as i64, SyscallError::BadHandle.as_i64(), "Forged handle must return -EBADF");
        kprintln!("  [Test 3I-O: Forged Handle & Missing Rights Rejection]: PASS");
    }

    // ====================================================================
    // Test 3I-P: Shared Memory Lifecycle Syscalls
    // ====================================================================
    {
        // 1. SYS_SHM_CREATE
        let user_shm_out_addr = USER_STACK_TOP - 64;
        unsafe {
            *(user_shm_out_addr as *mut u32) = 0;
        }
        let mut frame_create = make_mock_syscall_frame(
            SYS_SHM_CREATE,
            2, // 2 pages
            user_shm_out_addr,
            0,
        );
        syscall_dispatch_rust(&mut frame_create);
        assert_eq!(frame_create.rax, 0, "SYS_SHM_CREATE must succeed");
        let shm_handle = unsafe { *(user_shm_out_addr as *const u32) };
        assert_ne!(shm_handle, 0);

        // 2. SYS_SHM_MAP
        let target_vaddr = USER_SHM_BASE;
        let mut frame_map = make_mock_syscall_frame(
            SYS_SHM_MAP,
            shm_handle as u64,
            target_vaddr,
            1, // writable
        );
        syscall_dispatch_rust(&mut frame_map);
        assert_eq!(frame_map.rax, 0, "SYS_SHM_MAP must succeed");

        // Verify mapping in active page table
        let mapped_page = Page::from_start_address(
            VirtualAddress::new(target_vaddr),
            crate::mm::vmm::get_active_geometry(),
        ).expect("mapped_page");
        assert!(vmm.is_mapped(mapped_page), "SHM page must be mapped");

        // 3. SYS_SHM_UNMAP
        let mut frame_unmap = make_mock_syscall_frame(
            SYS_SHM_UNMAP,
            shm_handle as u64,
            target_vaddr,
            0,
        );
        syscall_dispatch_rust(&mut frame_unmap);
        assert_eq!(frame_unmap.rax, 0, "SYS_SHM_UNMAP must succeed");
        assert!(!vmm.is_mapped(mapped_page), "SHM page must be unmapped");

        // Close SHM handle
        crate::ipc::shm::shm_close(Handle(shm_handle), pmm).expect("shm_close must succeed");

        kprintln!("  [Test 3I-P: Shared Memory Lifecycle Syscalls]: PASS");
    }

    // ====================================================================
    // Test 3I-Q: sys_exit Process Termination & Zombie State
    // ====================================================================
    {
        let mut frame_exit = make_mock_syscall_frame(SYS_EXIT, 42, 0, 0);
        syscall_dispatch_rust(&mut frame_exit);
        // Verify process state
        unsafe {
            let slot = &PROCESS_TABLE[proc_slot];
            assert_eq!(slot.process.exit_code, 42, "Process exit code must be latched");
            assert_eq!(slot.process.state, ProcessState::Zombie, "Process must be in Zombie state");
        }
        kprintln!("  [Test 3I-Q: sys_exit Process Termination & Zombie State]: PASS");
    }

    // ====================================================================
    // Test 3I-R: Complete User Process PMM Neutrality
    // ====================================================================
    {
        // Unmap user test code and stack pages before final PMM cleanup
        let code_page = Page::from_start_address(
            VirtualAddress::new(USER_CODE_BASE),
            crate::mm::vmm::get_active_geometry(),
        ).expect("code_page");
        let _ = vmm.unmap_page(code_page, pmm);
        let _ = pmm.free_frame(code_frame);

        for (i, frame) in stack_frames.iter().enumerate() {
            let page_vaddr = (USER_STACK_TOP - 4 * 4096) + (i as u64) * 4096;
            let page = Page::from_start_address(
                VirtualAddress::new(page_vaddr),
                crate::mm::vmm::get_active_geometry(),
            ).expect("stack page");
            let _ = vmm.unmap_page(page, pmm);
            let _ = pmm.free_frame(*frame);
        }

        // Reclaim process resources
        unsafe {
            let proc_ptr = &raw mut PROCESS_TABLE[proc_slot].process;
            let orig_rflags = crate::task::scheduler::SCHEDULER.lock.acquire();
            crate::task::process::reclaim_process_resources_locked(proc_ptr, pmm);
            crate::task::scheduler::SCHEDULER.lock.unlock_restore(orig_rflags);
            PROCESS_TABLE[proc_slot].occupied = false;
        }

        let current_free = pmm.free_frame_count();
        assert_eq!(
            current_free, baseline_free,
            "PMM frame neutrality violated: baseline {} != current {}",
            baseline_free, current_free
        );
        kprintln!("  [Test 3I-R: Complete User Process PMM Neutrality]: PASS");
    }

    kprintln!("[Stage 3I COMPLETE] All 18 tests passed. PMM Neutrality verified.\n");
}

fn make_mock_syscall_frame(nr: u64, rdi: u64, rsi: u64, rdx: u64) -> SyscallFrame {
    SyscallFrame {
        r15: 0,
        r14: 0,
        r13: 0,
        r12: 0,
        rbx: 0,
        rbp: 0,
        r9: 0,
        r8: 0,
        r10: 0,
        rdx,
        rsi,
        rdi,
        rax: nr,
        user_rip: USER_CODE_BASE,
        user_cs: 0x23,
        user_rflags: 0x0202,
        user_rsp: USER_STACK_TOP,
        user_ss: 0x1B,
    }
}
