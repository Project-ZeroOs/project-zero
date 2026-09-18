//! Project Zero - Kernel Nucleus (Stage 2)
//!
//! A Human-Centered, AI-Native, Distributed Operating System
//! "One personal computing environment, multiple physical devices."

#![no_std]
#![no_main]

#[macro_use]
pub mod hal;
pub mod mm;
pub mod task;
pub mod ipc;
pub mod cap;
pub mod syscall;
pub mod bench;
pub mod elf;
pub mod fs;
pub mod dev;
pub mod net;
pub mod smp;

use core::alloc::GlobalAlloc;
use core::panic::PanicInfo;
use hal::arch::x86_64::cpu::{self, CpuDiagnostics};
use hal::arch::x86_64::serial::COM1;
use hal::arch::x86_64::gdt::init_gdt;
use hal::arch::x86_64::idt::init_idt;
use mm::pmm::PMM;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    kprintln!("PROJECT ZERO KERNEL PANIC");
    kprintln!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    if let Some(location) = info.location() {
        kprintln!("Location: {}:{}:{}", location.file(), location.line(), location.column());
    }
    kprintln!("Message:  {}", info.message());
    kprintln!("Halting CPU.");

    unsafe { cpu::outb(0xF4, 0x01); }
    loop { cpu::hlt(); }
}

/// Stage 2C Kernel Nucleus Entry Point: Multiboot Memory-Map Discovery & Physical Memory Inventory.
#[no_mangle]
pub extern "C" fn kernel_main(boot_info_addr: u64, multiboot_magic: u64) -> ! {
    // 0. Zero the .bss section (uninitialized static tables)
    extern "C" {
        static mut __bss_start: u8;
        static mut __bss_end: u8;
    }
    unsafe {
        let bss_start = &raw mut __bss_start as *mut u8;
        let bss_end = &raw mut __bss_end as *mut u8;
        let bss_size = (bss_end as usize) - (bss_start as usize);
        core::ptr::write_bytes(bss_start, 0, bss_size);
    }

    // 1. Diagnostics & Banner
    COM1.init();
    kprintln!("============================================================");
    kprintln!("PROJECT ZERO");
    kprintln!("Kernel initialized.");
    kprintln!("============================================================");

    // 2. CPU Hardware Initialization (GDT & TSS, then IDT)
    init_gdt();
    init_idt();
    hal::arch::x86_64::gdt::verify_stage2fa_invariants();

    // 3. Print Stage 2B GDT, TSS, Segment Registers, and Stack Diagnostics
    hal::arch::x86_64::gdt::print_gdt_diagnostics();

    // 4. Regression Verification: Assert IDT and Exception Subsystem Remain Fully Functional
    kprintln!("\n[Verification: Executing Controlled CPU Exception Regression Suite]");

    let bp_ok = hal::arch::x86_64::idt::verify_breakpoint();
    assert!(bp_ok, "Breakpoint verification failed!");

    let ud_ok = hal::arch::x86_64::idt::verify_invalid_opcode();
    assert!(ud_ok, "Invalid opcode verification failed!");

    let de_ok = hal::arch::x86_64::idt::verify_divide_error();
    assert!(de_ok, "Divide error verification failed!");

    // Test controlled Page Fault (#PF) on unmapped address (1.5 GiB, beyond 1 GiB identity map)
    let unmapped_addr = 0x0000_0000_6000_0000u64;
    let pf_ok = hal::arch::x86_64::idt::verify_controlled_page_fault(unmapped_addr);
    assert!(pf_ok, "Controlled page fault verification failed!");

    // Verify 32/32 exception descriptors are installed and present in IDT
    let mut present_count = 0;
    for vec in 0..32 {
        if hal::arch::x86_64::idt::get_vector_present(vec) {
            present_count += 1;
        }
    }
    assert_eq!(present_count, 32, "Not all 32 exception vectors are present in IDT!");

    // 5. Stage 2C: Multiboot Memory Map Discovery & Physical Memory Inventory
    let inv_res = mm::inventory::parse_memory_map(boot_info_addr, multiboot_magic as u32);
    let inv = match inv_res {
        Ok(inventory) => inventory,
        Err(err) => {
            kprintln!("[FATAL] Multiboot Memory-Map Discovery Failed: {}", err);
            panic!("Multiboot memory map parsing failed!");
        }
    };

    mm::inventory::print_memory_inventory_diagnostics(boot_info_addr, multiboot_magic as u32);

    // Validate that discoverable usable memory was identified and reservations are disjoint
    assert!(inv.discoverable_byte_level_ram > 0, "No usable physical memory discovered!");
    kprintln!("\n[Stage 2C Architectural Status & Verification Summary]");
    kprintln!("  * Multiboot 1 boot contract validated (Magic: 0x{:08X}).", multiboot_magic);
    kprintln!("  * 64-bit physical addresses preserved across all inventory tables.");
    kprintln!("  * Critical Project Zero structures & boot metadata explicitly reserved.");
    kprintln!("  * Usable memory distinguished from firmware available RAM via reservation subtraction.");
    kprintln!("  * Pure discovery and read-only inventory baseline authoritative.");

    // 6. Stage 2D: Physical Frame Manager (PMM)
    let pmm_init_res = unsafe { PMM.init_from_inventory(inv) };
    if let Err(err) = pmm_init_res {
        kprintln!("[FATAL] PMM Initialization Failed: {:?}", err);
        panic!("PMM initialization failed!");
    }

    unsafe {
        PMM.print_diagnostics(inv);
        PMM.run_smoke_test(inv);
    }

    kprintln!("\n[Stage 2D Architectural Status & Verification Summary]");
    kprintln!("  * Consumes Stage 2C page-aligned candidate inventory exclusively.");
    kprintln!("  * Statically reserved metadata bootstrap eliminates circular dependency.");
    kprintln!("  * Fail-closed state tracking: all non-candidate frames remain Reserved/Unusable.");
    kprintln!("  * Explicit 4-state 2-bit model guarantees mutual exclusivity by construction.");
    kprintln!("  * Allocation and free operations preserve 4 KiB alignment and 64-bit frame identity.");
    kprintln!("  * Deterministic smoke test and all state accounting invariants verified.");

    // 7. Stage 2E-A: Architecture Foundations (Geometry, CR3 & Typed Tables)
    let geometry = mm::vmm::init_paging_hardware();
    mm::vmm::print_stage2e_diagnostics(&geometry);
    mm::vmm::run_stage2ea_verification(&geometry);

    kprintln!("\n[Stage 2E-A Architectural Status & Verification Summary]");
    kprintln!("  * Hardware address geometry discovered via CPUID (phys_mask: 0x{:016X}).", geometry.physical_mask);
    kprintln!("  * IA32_EFER.NXE enabled (No-Execute page execution protection active).");
    kprintln!("  * Strongly typed CR3 read/write abstraction verified with geometry mask.");
    kprintln!("  * 4 KiB-aligned PageTable and PageTableEntry structures verified.");
    kprintln!("  * Security domain policy enforces USER=0 throughout kernel hierarchies.");
    kprintln!("  * Active memory mappings remain completely untouched (Phase 2E-A instruments).");

    // 8. Stage 2E-B: Virtual Memory Mapping Engine
    unsafe {
        mm::vmm::run_stage2eb_verification(&mut PMM);
    }

    kprintln!("\n[Stage 2E-B Architectural Status & Verification Summary]");
    kprintln!("  * 4 KiB aligned, canonical virtual-address mapping engine verified.");
    kprintln!("  * Physical frame ownership strictly preserved across map/unmap operations.");
    kprintln!("  * Kernel domain enforces USER=0 across all four hierarchy levels.");
    kprintln!("  * Intermediate tables allocated on demand via PMM with zeroed contents.");
    kprintln!("  * Atomic allocation-failure rollback guarantees zero leaked intermediate tables.");
    kprintln!("  * Empty intermediate tables automatically reclaimed during unmap_page.");
    kprintln!("  * Complete map/unmap cycle restores PMM free-frame accounting to exact baseline.");
    kprintln!("  * Sparse virtual address (0x50000000) verified with live volatile read/write.");
    kprintln!("  * Existing boot kernel memory mappings remain fully intact.");

    // 9. Stage 2F-B: Dual Bootstrap Page-Table Construction Verification
    mm::vmm::run_stage2fb_verification(&geometry);

    kprintln!("\n[Stage 2F-B Architectural Status & Verification Summary]");
    kprintln!("  * Dual bootstrap page-table mappings verified across all hierarchy levels.");
    kprintln!("  * PML4[0] and PML4[511] dual-link to PDPT root (identity and higher-half).");
    kprintln!("  * PDPT[0] and PDPT[510] dual-link to PD table (identity 1 GiB and KERNEL_VIRT_BASE).");
    kprintln!("  * 512 x 2 MiB huge pages map first 1 GiB of physical address space continuously.");
    kprintln!("  * Live dual memory and code read tests verify identical physical frame translation.");
    kprintln!("  * Current execution remains identity-mapped (Stage 2F-C execution switch pending).");

    // 10. Stage 2F-C: Higher-Half Execution Switch & Stack Transition Verification
    mm::vmm::run_stage2fc_verification(&geometry);

    kprintln!("\n[Stage 2F-C Architectural Status & Verification Summary]");
    kprintln!("  * Kernel execution successfully switched to canonical higher-half VMA (0xFFFF_FFFF_8000_0000+).");
    kprintln!("  * Kernel stack transitioned to 16-byte aligned higher-half virtual window.");
    kprintln!("  * Dual mapping (PML4[0] identity + PML4[511] higher-half) remains fully intact.");
    kprintln!("  * All Stage 2 subsystems (IDT, GDT/TSS, PMM, VMM) verified operational in higher half.");

    // 11. Stage 2F-D: Higher-Half Direct Map (HHDM) Verification
    mm::vmm::run_stage2fd_verification(&geometry, unsafe { &mut *(&raw mut mm::pmm::PMM) });

    kprintln!("\n[Stage 2F-D Architectural Status & Verification Summary]");
    kprintln!("  * Higher-Half Direct Map (HHDM) established at 0xFFFF_8000_0000_0000 (PML4 index 256).");
    kprintln!("  * Dedicated hhdm_pdpt correctly integrated into .page_tables hierarchy.");
    kprintln!("  * 1 GiB physical RAM mapped 1:1 via 2 MiB huge pages at HHDM_BASE.");
    kprintln!("  * Strongly typed PhysicalAddress, VirtualAddress, and HhdmAddress abstractions enforced.");
    kprintln!("  * Dual virtual translation (kernel VMA and HHDM) verified for memory and code.");
    kprintln!("  * Live dynamic PMM frame write/read cycle verified via HHDM pointer.");
    kprintln!("  * Addressability vs PMM ownership decoupling strictly maintained.");
    kprintln!("  * Dual bootstrap mappings (PML4[0] identity and PML4[511] higher-half) preserved.");

    // 12. Stage 2F-E: Identity Mapping Removal
    mm::vmm::run_stage2fe_verification(&geometry, unsafe { &mut *(&raw mut mm::pmm::PMM) });

    kprintln!("\n[Stage 2F-E Architectural Status & Verification Summary]");
    kprintln!("  * Identity mapping (PML4[0]) permanently removed from active address space.");
    kprintln!("  * PML4[0] cleared via HHDM pointer; full TLB shootdown performed via CR3 reload.");
    kprintln!("  * PML4[0] verified ABSENT; PML4[256] (HHDM) and PML4[511] (kernel VMA) verified INTACT.");
    kprintln!("  * Kernel execution remains in canonical higher-half VMA before and after removal.");
    kprintln!("  * Controlled #PF at physical 0x00100000: vector 14, CR2 exact match, kernel error code.");
    kprintln!("  * Page-fault handler executes entirely in higher-half IDT/ISR/stack domain.");
    kprintln!("  * HHDM (0xFFFF800000100000) and kernel VMA (0xFFFFFFFF80100000) remain valid post-removal.");
    kprintln!("  * Post-removal VMM alloc/map/write/read/unmap/free cycle verified via HHDM seam.");
    kprintln!("  * physical address != kernel virtual address != HHDM virtual address (3-way domain proof).");
    kprintln!("  * Project Zero no longer requires identity mapping for normal kernel operation.");

    // 13. Stage 2F-F: 4 KiB Kernel Permission Splitting & W^X Enforcement
    mm::vmm::run_stage2ff_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) });

    kprintln!("\n[Stage 2F Memory Architecture Transition Complete]");
    kprintln!("  * Stage 2F-A: Linker & Multiboot higher-half ELF layout established.");
    kprintln!("  * Stage 2F-B: Dual bootstrap page-table mappings verified.");
    kprintln!("  * Stage 2F-C: Execution switched to canonical higher-half VMA (0xFFFFFFFF8010xxxx).");
    kprintln!("  * Stage 2F-D: HHDM (0xFFFF800000000000) active over [0, 4 GiB) physical RAM.");
    kprintln!("  * Stage 2F-E: Low identity mapping permanently removed; PML4[0] absent.");
    kprintln!("  * Stage 2F-F: 4 KiB granular permissions & W^X enforced on Kernel VMA.");
    kprintln!("  * Section Permissions: .text RX, .rodata R+NX, .data/.bss/.stack RW+NX, guard NOT PRESENT.");
    kprintln!("  * Hardware Protections: CR0.WP=1, IA32_EFER.NXE=1 verified active.");
    kprintln!("  * HHDM Privilege Aperture: Privileged RW aperture verified distinct from Kernel VMA immutability.");
    kprintln!("  * Project Zero Stage 2F Higher-Half & Physical Memory Architecture FULLY OPERATIONAL.");

    // 14. Stage 3A: Execution Primitives & Guarded Stacks
    task::run_stage3a_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &geometry);

    kprintln!("\n[Stage 3A Execution Primitives Complete]");
    kprintln!("  * Static KernelThread table established with stable addresses.");
    kprintln!("  * Dedicated Kernel Stack Arena active over [0xFFFFFFFF90000000, 0xFFFFFFFFA0000000).");
    kprintln!("  * 16 KiB stacks with 4 KiB unmapped guards backed transactionally by PMM/VMM.");
    kprintln!("  * Controlled Guard Page #PF verified with Vector 14 and exact CR2.");
    kprintln!("  * BSP PerCpu initialized via IA32_GS_BASE (offset 16 verified).");
    kprintln!("  * Forged initial cooperative activation frame verified.");

    // 15. Stage 3B: Cooperative Scheduler Core
    let mut vmm = mm::vmm::ActivePageTable::new();
    task::run_stage3b_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 16. Stage 3C Increment 2: LAPIC Discovery & MMIO Mapping
    let lapic_info = hal::arch::x86_64::lapic::init_lapic_mmio(
        unsafe { &mut *(&raw mut mm::pmm::PMM) },
        &mut vmm,
    );
    hal::arch::x86_64::lapic::print_lapic_diagnostics(&lapic_info);

    // 17. Stage 3C Increment 3: LAPIC Timer Programming & Non-Preemptive ISR Verification
    hal::arch::x86_64::lapic::run_stage3c_inc3_verification();

    // 18. Stage 3C Increment 4: Preemptive Context Assembly Primitives Verification
    task::run_stage3c_inc4_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 19. Stage 3C Increment 5: Scheduler Preemption Integration Verification
    task::run_stage3c_inc5_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 20. Stage 3D Increment 1: Core Block/Wake Foundation Verification
    task::run_stage3d_inc1_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 21. Stage 3D Increment 2: Kernel Mutex Primitive Verification
    task::run_stage3d_inc2_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 22. Stage 3D Increment 3: Condition Variable Primitive Verification
    task::run_stage3d_inc3_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 23. Stage 3D Increment 4: Timer Sleep Primitive Verification
    task::run_stage3d_inc4_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 24. Stage 3D Increment 5: Kernel Event Primitive Verification
    task::run_stage3d_inc5_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 25. Stage 3E: Thread Lifecycle & Resource Reclamation Verification
    task::run_stage3e_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 26. Stage 3F: Process Model, Address Spaces & Process Lifecycle Verification
    task::run_stage3f_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 27. Stage 3G: IPC & Kernel Object Semantics Verification
    ipc::run_stage3g_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 28. Stage 3H: Capability System & Kernel Authority Model Verification
    cap::run_stage3h_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 29. Stage 3I: User Space & System Call Interface Verification
    syscall::tests::run_stage3i_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 30. Stage 3J: ELF & Program Execution Verification
    elf::tests::run_stage3j_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 31. Stage 3K: Storage / Filesystem Verification
    fs::init(unsafe { &mut *(&raw mut mm::pmm::PMM) });
    fs::tests::run_stage3k_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 32. Stage 3L: Device / Hardware Model Verification
    dev::init();
    dev::tests::run_stage3l_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 33. Stage 3M: Native Networking Model Verification
    net::init(unsafe { &mut *(&raw mut mm::pmm::PMM) });
    net::tests::run_stage3m_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // 34. Stage 3N: SMP / Multi-Core Architecture Verification
    smp::tests::run_stage3n_verification(unsafe { &mut *(&raw mut mm::pmm::PMM) }, &mut vmm);

    // Signal success to QEMU isa-debug-exit (0x10 -> exit code 33)
    unsafe { cpu::outb(0xF4, 0x10); }

    loop { cpu::hlt(); }
}
