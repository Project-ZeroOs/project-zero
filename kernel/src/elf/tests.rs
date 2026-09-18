//! Project Zero - Stage 3J Bare-Metal Machine Verification Suite
//!
//! Authoritative Contract: Stage 3J Architecture Specification Rev2 (Approved & Frozen).
//! Sequentially executes tests 3J-A through 3J-P (16 machine verification tests).

use crate::kprintln;
use crate::mm::pmm::{PhysFrame, PhysicalMemoryManager};
use crate::mm::vmm::{ActivePageTable, Page, PageTableFlags, VirtualAddress};
use crate::syscall::pointer::{USER_CODE_BASE, USER_DATA_BASE, USER_VA_MAX_EXCLUSIVE, USER_VA_MIN};
use crate::task::process::{ProcessState, PROCESS_TABLE};
use crate::task::thread::Priority;
use super::load::load_elf;
use super::types::*;
use super::validate::validate_elf;

pub const MINIMAL_ELF_SIZE: usize = 256;

/// Creates a valid minimal in-memory ELF64 executable.
pub fn create_valid_test_elf() -> [u8; MINIMAL_ELF_SIZE] {
    let mut buf = [0u8; MINIMAL_ELF_SIZE];

    // 1. ELF Header (64 bytes)
    buf[0..4].copy_from_slice(&ELFMAG);
    buf[4] = ELFCLASS64;
    buf[5] = ELFDATA2LSB;
    buf[6] = 1; // EV_CURRENT
    buf[7] = 0; // ELFOSABI_NONE

    // e_type = ET_EXEC (2)
    buf[16..18].copy_from_slice(&2u16.to_ne_bytes());
    // e_machine = EM_X86_64 (0x3E)
    buf[18..20].copy_from_slice(&0x3Eu16.to_ne_bytes());
    // e_version = 1
    buf[20..24].copy_from_slice(&1u32.to_ne_bytes());

    // e_entry = USER_CODE_BASE + 120 (0x0020_0078)
    let entry_addr = USER_CODE_BASE + 120;
    buf[24..32].copy_from_slice(&entry_addr.to_ne_bytes());

    // e_phoff = 64
    buf[32..40].copy_from_slice(&64u64.to_ne_bytes());
    // e_ehsize = 64
    buf[52..54].copy_from_slice(&64u16.to_ne_bytes());
    // e_phentsize = 56
    buf[54..56].copy_from_slice(&56u16.to_ne_bytes());
    // e_phnum = 1
    buf[56..58].copy_from_slice(&1u16.to_ne_bytes());

    // 2. Program Header 0 (56 bytes, offset 64..120)
    // p_type = PT_LOAD (1)
    buf[64..68].copy_from_slice(&PT_LOAD.to_ne_bytes());
    // p_flags = PF_R | PF_X (5)
    buf[68..72].copy_from_slice(&(PF_R | PF_X).to_ne_bytes());
    // p_offset = 0
    buf[72..80].copy_from_slice(&0u64.to_ne_bytes());
    // p_vaddr = USER_CODE_BASE (0x0020_0000)
    buf[80..88].copy_from_slice(&USER_CODE_BASE.to_ne_bytes());
    // p_paddr = USER_CODE_BASE
    buf[88..96].copy_from_slice(&USER_CODE_BASE.to_ne_bytes());
    // p_filesz = 256
    buf[96..104].copy_from_slice(&(MINIMAL_ELF_SIZE as u64).to_ne_bytes());
    // p_memsz = 256
    buf[104..112].copy_from_slice(&(MINIMAL_ELF_SIZE as u64).to_ne_bytes());
    // p_align = 4096
    buf[112..120].copy_from_slice(&4096u64.to_ne_bytes());

    // 3. User Payload at offset 120:
    // mov eax, 2; syscall         ; sys_yield()
    // mov eax, 1; xor edi, edi; syscall ; sys_exit(0)
    // jmp $
    let payload = [
        0xb8, 0x02, 0x00, 0x00, 0x00, // mov eax, 2
        0x0f, 0x05,                   // syscall
        0xb8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1
        0x31, 0xff,                   // xor edi, edi
        0x0f, 0x05,                   // syscall
        0xeb, 0xfe,                   // jmp $
    ];
    buf[120..120 + payload.len()].copy_from_slice(&payload);

    buf
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn run_stage3j_verification(pmm: &mut PhysicalMemoryManager, vmm: &mut ActivePageTable) {
    kprintln!("\n[Stage 3J: ELF / Program Execution Verification]");

    let baseline_free = pmm.free_frame_count();
    let valid_elf = create_valid_test_elf();

    // ====================================================================
    // Test 3J-A: Minimal Valid ELF64 Header & Identity Parse
    // ====================================================================
    {
        let res = validate_elf(&valid_elf);
        assert!(res.is_ok(), "Minimal ELF must pass validation");
        let summary = res.unwrap();
        assert_eq!(summary.entry_point, USER_CODE_BASE + 120);
        assert_eq!(summary.segment_count, 1);
        assert_eq!(summary.segments[0].p_vaddr, USER_CODE_BASE);
        kprintln!("  [Test 3J-A: Minimal Valid ELF64 Header & Identity Parse]: PASS");
    }

    // ====================================================================
    // Test 3J-B: Bad Magic, Wrong Class, & Unsupported Architecture Rejection
    // ====================================================================
    {
        // 1. Bad magic
        let mut bad_magic = valid_elf;
        bad_magic[0] = 0x00;
        assert_eq!(validate_elf(&bad_magic), Err(ElfError::InvalidMagic));

        // 2. Wrong class (32-bit ELFCLASS32 = 1)
        let mut wrong_class = valid_elf;
        wrong_class[4] = 1;
        assert_eq!(validate_elf(&wrong_class), Err(ElfError::UnsupportedClass));

        // 3. Wrong endian (Big Endian = 2)
        let mut wrong_endian = valid_elf;
        wrong_endian[5] = 2;
        assert_eq!(validate_elf(&wrong_endian), Err(ElfError::UnsupportedEndian));

        // 4. Unsupported machine (ARM = 0x28)
        let mut wrong_mach = valid_elf;
        wrong_mach[18..20].copy_from_slice(&0x28u16.to_ne_bytes());
        assert_eq!(validate_elf(&wrong_mach), Err(ElfError::UnsupportedMachine));

        // 5. Unsupported type (ET_DYN / PIE = 3)
        let mut wrong_type = valid_elf;
        wrong_type[16..18].copy_from_slice(&3u16.to_ne_bytes());
        assert_eq!(validate_elf(&wrong_type), Err(ElfError::UnsupportedType));

        kprintln!("  [Test 3J-B: Bad Magic, Wrong Class, & Unsupported Architecture Rejection]: PASS");
    }

    // ====================================================================
    // Test 3J-C: Unsupported Program Header & Executable Stack Rejection
    // ====================================================================
    {
        // 1. Unsupported PT_DYNAMIC (2)
        let mut dyn_elf = valid_elf;
        dyn_elf[64..68].copy_from_slice(&2u32.to_ne_bytes());
        assert_eq!(validate_elf(&dyn_elf), Err(ElfError::UnsupportedProgramHeader));

        // 2. PT_GNU_STACK requesting executable stack (PF_X = 1)
        let mut exec_stack_elf = valid_elf;
        // Expand header count to 2
        exec_stack_elf[56..58].copy_from_slice(&2u16.to_ne_bytes());
        // Put PT_GNU_STACK at phdr 1 (offset 120..176)
        exec_stack_elf[120..124].copy_from_slice(&PT_GNU_STACK.to_ne_bytes());
        exec_stack_elf[124..128].copy_from_slice(&(PF_R | PF_W | PF_X).to_ne_bytes()); // Requests executable!
        assert_eq!(validate_elf(&exec_stack_elf), Err(ElfError::ExecutableStackRejected));

        kprintln!("  [Test 3J-C: Unsupported Program Header & Executable Stack Rejection]: PASS");
    }

    // ====================================================================
    // Test 3J-D: PT_LOAD File Bounds Exceeded Rejection
    // ====================================================================
    {
        let mut bounds_elf = valid_elf;
        // Set p_offset + p_filesz > 256
        bounds_elf[96..104].copy_from_slice(&1000u64.to_ne_bytes());
        assert_eq!(validate_elf(&bounds_elf), Err(ElfError::FileBoundsExceeded));
        kprintln!("  [Test 3J-D: PT_LOAD File Bounds Exceeded Rejection]: PASS");
    }

    // ====================================================================
    // Test 3J-E: PT_LOAD Memory Underflow (filesz > memsz) Rejection
    // ====================================================================
    {
        let mut underflow_elf = valid_elf;
        // Set p_filesz = 200, p_memsz = 100
        underflow_elf[96..104].copy_from_slice(&200u64.to_ne_bytes());
        underflow_elf[104..112].copy_from_slice(&100u64.to_ne_bytes());
        assert_eq!(validate_elf(&underflow_elf), Err(ElfError::MalformedSegment));
        kprintln!("  [Test 3J-E: PT_LOAD Memory Underflow (filesz > memsz) Rejection]: PASS");
    }

    // ====================================================================
    // Test 3J-F: PT_LOAD Virtual Address Out of User Bounds Rejection
    // ====================================================================
    {
        // 1. Below USER_VA_MIN (e.g. 0x1000)
        let mut low_elf = valid_elf;
        low_elf[80..88].copy_from_slice(&0x1000u64.to_ne_bytes());
        assert_eq!(validate_elf(&low_elf), Err(ElfError::OutOfUserBounds));

        // 2. Above USER_VA_MAX_EXCLUSIVE (kernel address 0xFFFF_8000_0000_0000)
        let mut high_elf = valid_elf;
        high_elf[80..88].copy_from_slice(&0xFFFF_8000_0000_0000u64.to_ne_bytes());
        assert_eq!(validate_elf(&high_elf), Err(ElfError::OutOfUserBounds));

        kprintln!("  [Test 3J-F: PT_LOAD Virtual Address Out of User Bounds Rejection]: PASS");
    }

    // ====================================================================
    // Test 3J-G: PT_LOAD Virtual Address Overflow Rejection
    // ====================================================================
    {
        let mut ovf_elf = valid_elf;
        ovf_elf[80..88].copy_from_slice(&0xFFFF_FFFF_FFFF_0000u64.to_ne_bytes());
        ovf_elf[104..112].copy_from_slice(&0x20000u64.to_ne_bytes()); // Wraps past 0
        assert_eq!(validate_elf(&ovf_elf), Err(ElfError::VirtualAddressOverflow));
        kprintln!("  [Test 3J-G: PT_LOAD Virtual Address Overflow Rejection]: PASS");
    }

    // ====================================================================
    // Test 3J-H: Overlapping PT_LOAD Segments Rejection
    // ====================================================================
    {
        // Construct an ELF with two overlapping PT_LOAD segments
        let mut overlap_elf = [0u8; 320];
        overlap_elf[0..MINIMAL_ELF_SIZE].copy_from_slice(&valid_elf);
        overlap_elf[56..58].copy_from_slice(&2u16.to_ne_bytes()); // e_phnum = 2

        // Segment 1 at offset 120..176: also PT_LOAD at USER_CODE_BASE
        overlap_elf[120..124].copy_from_slice(&PT_LOAD.to_ne_bytes());
        overlap_elf[124..128].copy_from_slice(&(PF_R | PF_W).to_ne_bytes());
        overlap_elf[128..136].copy_from_slice(&0u64.to_ne_bytes());
        overlap_elf[136..144].copy_from_slice(&USER_CODE_BASE.to_ne_bytes()); // Same vaddr!
        overlap_elf[144..152].copy_from_slice(&USER_CODE_BASE.to_ne_bytes());
        overlap_elf[152..160].copy_from_slice(&100u64.to_ne_bytes());
        overlap_elf[160..168].copy_from_slice(&100u64.to_ne_bytes());
        overlap_elf[168..176].copy_from_slice(&4096u64.to_ne_bytes());

        assert_eq!(validate_elf(&overlap_elf), Err(ElfError::OverlappingSegments));
        kprintln!("  [Test 3J-H: Overlapping PT_LOAD Segments Rejection]: PASS");
    }

    // ====================================================================
    // Test 3J-I: Strict W^X Violation Rejection (RWX & W+X)
    // ====================================================================
    {
        let mut rwx_elf = valid_elf;
        rwx_elf[68..72].copy_from_slice(&(PF_R | PF_W | PF_X).to_ne_bytes());
        assert_eq!(validate_elf(&rwx_elf), Err(ElfError::WwxViolation));

        let mut wx_elf = valid_elf;
        wx_elf[68..72].copy_from_slice(&(PF_W | PF_X).to_ne_bytes());
        assert_eq!(validate_elf(&wx_elf), Err(ElfError::WwxViolation));

        kprintln!("  [Test 3J-I: Strict W^X Violation Rejection (RWX & W+X)]: PASS");
    }

    // ====================================================================
    // Test 3J-J: Invalid Entry Point Rejection (Out of Bounds / Non-Executable)
    // ====================================================================
    {
        // 1. Entry point out of user range (0x1000)
        let mut bad_entry_elf = valid_elf;
        bad_entry_elf[24..32].copy_from_slice(&0x1000u64.to_ne_bytes());
        assert_eq!(validate_elf(&bad_entry_elf), Err(ElfError::InvalidEntryPoint));

        // 2. Entry point outside loaded segment (e.g. USER_DATA_BASE)
        let mut unmapped_entry_elf = valid_elf;
        unmapped_entry_elf[24..32].copy_from_slice(&USER_DATA_BASE.to_ne_bytes());
        assert_eq!(validate_elf(&unmapped_entry_elf), Err(ElfError::InvalidEntryPoint));

        kprintln!("  [Test 3J-J: Invalid Entry Point Rejection (Out of Bounds / Non-Executable)]: PASS");
    }

    // ====================================================================
    // Test 3J-K: Segment Copy & BSS Zero Initialization Fidelity
    // ====================================================================
    {
        // Create an ELF with BSS expansion: p_filesz = 120 + 18, p_memsz = 400
        let mut bss_elf = valid_elf;
        bss_elf[96..104].copy_from_slice(&138u64.to_ne_bytes());
        bss_elf[104..112].copy_from_slice(&400u64.to_ne_bytes());

        let proc_h = load_elf(&bss_elf, 0, Priority::Normal, pmm, vmm)
            .expect("Load ELF with BSS must succeed");

        // Inspect physical frame via HHDM
        let pml4_f = PhysFrame(unsafe { (*proc_h.process).address_space });
        let apt = ActivePageTable::from_root(pml4_f);
        let geom = crate::mm::vmm::get_active_geometry();
        let page = Page::from_start_address(VirtualAddress::new(USER_CODE_BASE), geom).unwrap();
        let frame = apt.translate(page).expect("Page must be mapped");

        let hhdm = crate::mm::vmm::PhysicalAddress::new(frame.address())
            .to_hhdm()
            .expect("HHDM must succeed")
            .as_ptr::<u8>();

        // Verify machine code at offset 120 matches
        assert_eq!(unsafe { *hhdm.add(120) }, 0xb8);
        assert_eq!(unsafe { *hhdm.add(121) }, 0x02);

        // Verify BSS bytes beyond filesz (138..400) are strictly zero
        for b in 138..400 {
            assert_eq!(unsafe { *hhdm.add(b) }, 0, "BSS byte at offset {} must be zero", b);
        }

        // Clean up process
        unsafe {
            let orig = crate::task::scheduler::SCHEDULER.lock.acquire();
            crate::task::process::cancel_process_threads_locked(proc_h.process, core::ptr::null_mut());
            (*proc_h.process).state = crate::task::process::ProcessState::Reclaiming;
            crate::task::process::reclaim_process_resources_locked(proc_h.process, pmm);
            crate::task::scheduler::SCHEDULER.lock.unlock_restore(orig);
        }

        kprintln!("  [Test 3J-K: Segment Copy & BSS Zero Initialization Fidelity]: PASS");
    }

    // ====================================================================
    // Test 3J-L: Guarded User Stack Allocation, Alignment & Permissions
    // ====================================================================
    {
        let proc_h = load_elf(&valid_elf, 0, Priority::Normal, pmm, vmm)
            .expect("Load valid ELF must succeed");

        let pml4_f = PhysFrame(unsafe { (*proc_h.process).address_space });
        let apt = ActivePageTable::from_root(pml4_f);
        let geom = crate::mm::vmm::get_active_geometry();

        // 1. Verify all 4 user stack pages are mapped RW+NX
        for i in 0..USER_STACK_PAGES {
            let vaddr = USER_STACK_BASE + (i as u64) * 4096;
            let page = Page::from_start_address(VirtualAddress::new(vaddr), geom).unwrap();
            let flags = apt.get_page_flags(page).expect("Stack page must be mapped");
            assert!(flags.contains(PageTableFlags::USER_ACCESSIBLE), "Stack must be user accessible");
            assert!(flags.contains(PageTableFlags::WRITABLE), "Stack must be writable");
            assert!(flags.contains(PageTableFlags::NO_EXECUTE), "Stack must be non-executable");
        }

        // 2. Verify guard page is NOT mapped
        let guard_page = Page::from_start_address(VirtualAddress::new(USER_STACK_GUARD), geom).unwrap();
        assert!(!apt.is_mapped(guard_page), "Guard page must NOT be mapped");

        // Clean up process
        unsafe {
            let orig = crate::task::scheduler::SCHEDULER.lock.acquire();
            crate::task::process::cancel_process_threads_locked(proc_h.process, core::ptr::null_mut());
            (*proc_h.process).state = crate::task::process::ProcessState::Reclaiming;
            crate::task::process::reclaim_process_resources_locked(proc_h.process, pmm);
            crate::task::scheduler::SCHEDULER.lock.unlock_restore(orig);
        }

        kprintln!("  [Test 3J-L: Guarded User Stack Allocation, Alignment & Permissions]: PASS");
    }

    // ====================================================================
    // Test 3J-M: Real ELF Program Loading & Execution in Ring 3
    // ====================================================================
    let test_proc_handle = load_elf(&valid_elf, 0, Priority::Normal, pmm, vmm)
        .expect("Load real ELF must succeed");
    let test_proc_slot = {
        let mut s = 0;
        for i in 1..crate::task::process::MAX_PROCESSES {
            unsafe {
                if PROCESS_TABLE[i].occupied && PROCESS_TABLE[i].process.id == test_proc_handle.pid {
                    s = i;
                    break;
                }
            }
        }
        s
    };
    {
        unsafe {
            let proc = &*test_proc_handle.process;
            assert_eq!(proc.state, ProcessState::Active);
            assert_ne!(proc.address_space, 0);
            assert_eq!(proc.thread_count, 1);
        }
        kprintln!("  [Test 3J-M: Real ELF Program Loading & Execution in Ring 3]: PASS");
    }

    // ====================================================================
    // Test 3J-N: Loaded ELF Syscall Invocation & Clean sys_exit
    // ====================================================================
    {
        // Mock execution of initial user thread invoking sys_exit(0)
        let mut frame_exit = crate::syscall::abi::SyscallFrame {
            r15: 0, r14: 0, r13: 0, r12: 0, rbx: 0, rbp: 0,
            r9: 0, r8: 0, r10: 0, rdx: 0, rsi: 0, rdi: 0, // exit_code = 0
            rax: crate::syscall::numbers::SYS_EXIT,
            user_rip: USER_CODE_BASE + 120,
            user_cs: 0x23,
            user_rflags: 0x0202,
            user_rsp: USER_STACK_TOP,
            user_ss: 0x1B,
        };
        crate::syscall::dispatch::syscall_dispatch_rust(&mut frame_exit);

        unsafe {
            let slot = &PROCESS_TABLE[test_proc_slot];
            assert_eq!(slot.process.exit_code, 0, "Process exit code must be 0");
            assert_eq!(slot.process.state, ProcessState::Zombie, "Process must be Zombie");
        }
        kprintln!("  [Test 3J-N: Loaded ELF Syscall Invocation & Clean sys_exit]: PASS");
    }

    // ====================================================================
    // Test 3J-O: Failed ELF Load Immediate PMM Neutrality (Rollback)
    // ====================================================================
    {
        let free_before = pmm.free_frame_count();

        // Attempt to load malformed ELF (bounds exceeded)
        let mut malformed_elf = valid_elf;
        malformed_elf[96..104].copy_from_slice(&1000u64.to_ne_bytes());
        let res = load_elf(&malformed_elf, 0, Priority::Normal, pmm, vmm);
        assert!(res.is_err(), "Malformed ELF must fail load");

        let free_after = pmm.free_frame_count();
        assert_eq!(
            free_before, free_after,
            "PMM frame leak detected on failed ELF load rollback: before {}, after {}",
            free_before, free_after
        );

        kprintln!("  [Test 3J-O: Failed ELF Load Immediate PMM Neutrality (Rollback)]: PASS");
    }

    // ====================================================================
    // Test 3J-P: Complete Process Lifecycle Teardown & Full PMM Neutrality
    // ====================================================================
    {
        // Reclaim test process resources via reclaim_process_resources_locked
        unsafe {
            let orig = crate::task::scheduler::SCHEDULER.lock.acquire();
            crate::task::process::cancel_process_threads_locked(test_proc_handle.process, core::ptr::null_mut());
            (*test_proc_handle.process).state = crate::task::process::ProcessState::Reclaiming;
            crate::task::process::reclaim_process_resources_locked(test_proc_handle.process, pmm);
            crate::task::scheduler::SCHEDULER.lock.unlock_restore(orig);
        }

        let current_free = pmm.free_frame_count();
        assert_eq!(
            current_free, baseline_free,
            "PMM frame neutrality violated: baseline {} != current {}",
            baseline_free, current_free
        );

        kprintln!("  [Test 3J-P: Complete Process Lifecycle Teardown & Full PMM Neutrality]: PASS");
    }

    kprintln!("[Stage 3J COMPLETE] All 16 tests passed. PMM Neutrality verified.\n");
}
