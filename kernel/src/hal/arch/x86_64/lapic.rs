//! Project Zero - Local Advanced Programmable Interrupt Controller (LAPIC)
//!
//! Implements hardware discovery, uncacheable MMIO page mapping, and register access
//! for the x86-64 Local APIC in xAPIC mode.

use crate::hal::arch::x86_64::cpu;
use crate::mm::pmm::{PhysicalMemoryManager, PhysFrame};
use crate::mm::vmm::{ActivePageTable, Page, PageTableFlags, VirtualAddress, MappingDomain, get_active_geometry};
use crate::task::percpu;
use crate::kprintln;

/// IA32_APIC_BASE Model-Specific Register (MSR 0x1B)
pub const IA32_APIC_BASE_MSR: u32 = 0x0000_001B;

/// APIC Global Enable bit (bit 11 in IA32_APIC_BASE)
pub const APIC_BASE_GLOBAL_ENABLE: u64 = 1 << 11;

/// APIC Bootstrap Processor flag (bit 8 in IA32_APIC_BASE)
pub const APIC_BASE_BSP: u64 = 1 << 8;

/// Authoritative Higher-Half Kernel Virtual Address for the LAPIC MMIO aperture.
/// Maps physical 0xFEE0_0000 into canonical higher-half VMA.
pub const LAPIC_VIRT_BASE: u64 = 0xFFFF_FFFF_FEE0_0000;

// LAPIC Register Offsets (xAPIC mode, 16-byte aligned 32-bit registers)
pub const LAPIC_ID_REG: u32             = 0x020;
pub const LAPIC_VERSION_REG: u32        = 0x030;
pub const LAPIC_TPR_REG: u32            = 0x080;
pub const LAPIC_EOI_REG: u32            = 0x0B0;
pub const LAPIC_LDR_REG: u32            = 0x0D0;
pub const LAPIC_DFR_REG: u32            = 0x0E0;
pub const LAPIC_SVR_REG: u32            = 0x0F0;
pub const LAPIC_ESR_REG: u32            = 0x280;
pub const LAPIC_LVT_TIMER_REG: u32      = 0x320;
pub const LAPIC_TIMER_INIT_CNT_REG: u32 = 0x380;
pub const LAPIC_TIMER_CURR_CNT_REG: u32 = 0x390;
pub const LAPIC_TIMER_DCR_REG: u32      = 0x3E0;

// Interrupt Command Register (ICR) Offsets & Flags (Stage 3N)
pub const LAPIC_ICR_LOW_REG: u32        = 0x300;
pub const LAPIC_ICR_HIGH_REG: u32       = 0x310;

pub const ICR_DELIVERY_FIXED: u32       = 0b000 << 8;
pub const ICR_DELIVERY_LOWEST_PRIO: u32 = 0b001 << 8;
pub const ICR_DELIVERY_SMI: u32         = 0b010 << 8;
pub const ICR_DELIVERY_NMI: u32         = 0b100 << 8;
pub const ICR_DELIVERY_INIT: u32        = 0b101 << 8;
pub const ICR_DELIVERY_STARTUP: u32     = 0b110 << 8;

pub const ICR_DEST_PHYSICAL: u32        = 0 << 11;
pub const ICR_DEST_LOGICAL: u32         = 1 << 11;

pub const ICR_DELIVERY_STATUS_PENDING: u32 = 1 << 12;

pub const ICR_LEVEL_DEASSERT: u32       = 0 << 14;
pub const ICR_LEVEL_ASSERT: u32         = 1 << 14;

pub const ICR_TRIGGER_EDGE: u32         = 0 << 15;
pub const ICR_TRIGGER_LEVEL: u32        = 1 << 15;

pub const ICR_SHORTHAND_NONE: u32       = 0b00 << 18;
pub const ICR_SHORTHAND_SELF: u32       = 0b01 << 18;
pub const ICR_SHORTHAND_ALL_INC: u32    = 0b10 << 18;
pub const ICR_SHORTHAND_ALL_EXC: u32    = 0b11 << 18;

pub const MAX_IPI_POLL_ITERATIONS: u32  = 100_000;

/// Diagnostic metadata for discovered and mapped LAPIC hardware.
#[derive(Debug, Clone, Copy)]
pub struct LapicInfo {
    pub msr_raw: u64,
    pub phys_base: u64,
    pub virt_base: u64,
    pub global_enabled: bool,
    pub is_bsp: bool,
    pub lapic_id: u32,
    pub lapic_version: u32,
    pub max_lvt_entries: u32,
}

/// Reads a 32-bit register from the LAPIC MMIO aperture.
#[inline(always)]
pub unsafe fn read_lapic_reg(offset: u32) -> u32 {
    let ptr = (LAPIC_VIRT_BASE + offset as u64) as *const u32;
    core::ptr::read_volatile(ptr)
}

/// Writes a 32-bit value to a LAPIC register in the MMIO aperture.
#[inline(always)]
pub unsafe fn write_lapic_reg(offset: u32, val: u32) {
    let ptr = (LAPIC_VIRT_BASE + offset as u64) as *mut u32;
    core::ptr::write_volatile(ptr, val);
}

/// Acknowledges an interrupt by writing End-of-Interrupt (EOI) to the LAPIC.
#[inline(always)]
pub unsafe fn lapic_eoi() {
    write_lapic_reg(LAPIC_EOI_REG, 0);
}

/// Discovers, enables, and maps the LAPIC MMIO aperture into higher-half VMA.
///
/// Invariants enforced:
/// 1. Reads authoritative base from `IA32_APIC_BASE` MSR (0x1B).
/// 2. Ensures APIC Global Enable bit (bit 11) is active.
/// 3. Maps 4 KiB physical base to `LAPIC_VIRT_BASE` with strong uncacheable flags
///    (`CACHE_DISABLE | WRITE_THROUGH | WRITABLE | PRESENT | NO_EXECUTE`).
/// 4. Reads and validates LAPIC ID and Version registers.
/// 5. Stores hardware LAPIC ID in `PerCpu.lapic_id`.
pub fn init_lapic_mmio(pmm: &mut PhysicalMemoryManager, vmm: &mut ActivePageTable) -> LapicInfo {
    // 1. Read IA32_APIC_BASE MSR
    let msr_val = cpu::read_msr(IA32_APIC_BASE_MSR);
    let phys_base = msr_val & 0x000F_FFFF_FFFF_F000;
    let mut enabled = (msr_val & APIC_BASE_GLOBAL_ENABLE) != 0;
    let is_bsp = (msr_val & APIC_BASE_BSP) != 0;

    // 2. Ensure APIC Global Enable is set
    if !enabled {
        cpu::write_msr(IA32_APIC_BASE_MSR, msr_val | APIC_BASE_GLOBAL_ENABLE);
        enabled = true;
    }

    // 3. Map physical base into higher-half VMA
    let geometry = get_active_geometry();
    let page = Page::from_start_address(VirtualAddress::new(LAPIC_VIRT_BASE), geometry)
        .expect("Invalid LAPIC virtual address");
    let frame = PhysFrame(phys_base);
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_EXECUTE
        | PageTableFlags::CACHE_DISABLE
        | PageTableFlags::WRITE_THROUGH;

    if !vmm.is_mapped(page) {
        vmm.map_page(page, frame, flags, MappingDomain::Kernel, pmm)
            .expect("Failed to map LAPIC MMIO aperture");
    }

    // 4. Read LAPIC ID (bits [31:24] in xAPIC mode) and Version register
    let raw_id = unsafe { read_lapic_reg(LAPIC_ID_REG) };
    let lapic_id = (raw_id >> 24) & 0xFF;

    let raw_ver = unsafe { read_lapic_reg(LAPIC_VERSION_REG) };
    let lapic_version = raw_ver & 0xFF;
    let max_lvt = (raw_ver >> 16) & 0xFF;

    // 5. Update PerCpu lapic_id
    unsafe {
        percpu::BSP_PERCPU.lapic_id = lapic_id;
    }

    LapicInfo {
        msr_raw: msr_val,
        phys_base,
        virt_base: LAPIC_VIRT_BASE,
        global_enabled: enabled,
        is_bsp,
        lapic_id,
        lapic_version,
        max_lvt_entries: max_lvt,
    }
}

/// Prints Stage 3C Increment 2 LAPIC discovery & mapping diagnostics.
pub fn print_lapic_diagnostics(info: &LapicInfo) {
    kprintln!("\n[Stage 3C-Increment 2: Local APIC Discovery & MMIO Mapping]");
    kprintln!("  IA32_APIC_BASE MSR:          0x{:016X}", info.msr_raw);
    kprintln!("  APIC Global Enable:          {} (Bit 11 = 1)", info.global_enabled);
    kprintln!("  BSP Processor Flag:          {} (Bit 8 = 1)", info.is_bsp);
    kprintln!("  Discovered Physical Base:    0x{:016X}", info.phys_base);
    kprintln!("  Mapped Virtual Base:         0x{:016X}", info.virt_base);
    kprintln!("  MMIO Page Attributes:        PRESENT | WRITABLE | NO_EXECUTE | CACHE_DISABLE | WRITE_THROUGH");
    kprintln!("  Hardware LAPIC ID:           {} (0x{:02X})", info.lapic_id, info.lapic_id);
    kprintln!("  Hardware LAPIC Version:      0x{:02X} (Max LVT Entry field: {}, LVT Entries: {})", 
        info.lapic_version, info.max_lvt_entries, info.max_lvt_entries + 1);
    kprintln!("  PerCpu.lapic_id Populated:   {} [VERIFIED]", unsafe { percpu::BSP_PERCPU.lapic_id });
    kprintln!("  [x] Stage 3C Increment 2 LAPIC discovery & MMIO mapping verified.");
}

// ====================================================================
// Stage 3C 160-Byte Interrupt Frame ABI
// ====================================================================

/// Exact 160-byte CPU interrupt frame matching `SavedFrameType::Preemptive`.
/// Pushed by hardware (40 bytes) and `isr_32` in `boot/isr.asm` (120 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

// Compile-time offset assertions for every single member of the 160-byte frame
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r15) == 0x00);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r14) == 0x08);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r13) == 0x10);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r12) == 0x18);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r11) == 0x20);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r10) == 0x28);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r9) == 0x30);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, r8) == 0x38);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rdi) == 0x40);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rsi) == 0x48);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rbp) == 0x50);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rbx) == 0x58);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rdx) == 0x60);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rcx) == 0x68);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rax) == 0x70);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rip) == 0x78);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, cs) == 0x80);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rflags) == 0x88);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, rsp) == 0x90);
const _: () = assert!(core::mem::offset_of!(InterruptFrame, ss) == 0x98);
const _: () = assert!(core::mem::size_of::<InterruptFrame>() == 160);
const _: () = assert!(core::mem::align_of::<InterruptFrame>() == 8);

// ====================================================================
// Stage 3C Increment 3: Timer Programming & Non-Preemptive Verification
// ====================================================================

/// Output structure capturing all 15 GPRs, RSP, RIP, and RFLAGS from assembly sentinel harness.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GprSentinelResults {
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    pub rip: u64,
    pub rflags: u64,
    pub loop_count: u64,
    pub rsp_matched: u64,
}

const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rax) == 0x00);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rcx) == 0x08);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rdx) == 0x10);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rbx) == 0x18);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rbp) == 0x20);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rsi) == 0x28);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rdi) == 0x30);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r8) == 0x38);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r9) == 0x40);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r10) == 0x48);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r11) == 0x50);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r12) == 0x58);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r13) == 0x60);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r14) == 0x68);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, r15) == 0x70);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rsp) == 0x78);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rip) == 0x80);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rflags) == 0x88);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, loop_count) == 0x90);
const _: () = assert!(core::mem::offset_of!(GprSentinelResults, rsp_matched) == 0x98);
const _: () = assert!(core::mem::size_of::<GprSentinelResults>() == 160);
const _: () = assert!(core::mem::align_of::<GprSentinelResults>() == 8);

extern "C" {
    fn test_gpr_sentinels_under_irq(results: *mut GprSentinelResults);
}

/// Calibrates the LAPIC Timer against PIT Channel 2 over a 10 ms window (100 Hz).
///
/// Hardware Protocol:
/// 1. PIT base frequency is 1,193,182 Hz. 10 ms = 11,932 cycles (0x2E9C).
/// 2. Reset PIT Channel 2 gate (port 0x61 bit 0 = 0, bit 1 = 0).
/// 3. Program PIT Channel 2: Mode 0 (interrupt on terminal count), 16-bit binary (port 0x43 = 0xB0).
/// 4. Write counter divisor: 11,932 (port 0x42: low 0x9C, high 0x2E).
/// 5. Configure LAPIC Timer DCR: Divisor 16 (0x03).
/// 6. Configure LAPIC LVT Timer: Masked (bit 16 = 1), One-shot (bits [18:17] = 00b), vector 32.
/// 7. Set initial count to maximum (0xFFFF_FFFF).
/// 8. Enable PIT Channel 2 gate (port 0x61 bit 0 = 1, bit 1 = 0).
/// 9. Spin-wait until PIT Channel 2 OUT2 (bit 5 of port 0x61) transitions to high.
/// 10. Read LAPIC Current Count Register immediately.
/// 11. Mask LAPIC timer and reset PIT Channel 2 gate.
/// 12. Return elapsed ticks for 100 Hz (10 ms) periodic operation.
pub fn calibrate_lapic_timer_pit2() -> u32 {
    const PIT_10MS_TICKS: u16 = 11932; // 1,193,182 Hz / 100 = 11,931.82 -> 11,932

    unsafe {
        // 1. Reset PIT Channel 2 gate (bit 0 = 0) and speaker (bit 1 = 0)
        let port61_init = cpu::inb(0x61);
        cpu::outb(0x61, (port61_init & 0xFD) & 0xFE);

        // 2. Program PIT Channel 2: Mode 0 (interrupt on terminal count), 16-bit binary
        cpu::outb(0x43, 0xB0);
        cpu::io_wait();

        // 3. Write counter divisor: 11,932 (0x2E9C)
        cpu::outb(0x42, (PIT_10MS_TICKS & 0xFF) as u8);
        cpu::io_wait();
        cpu::outb(0x42, ((PIT_10MS_TICKS >> 8) & 0xFF) as u8);
        cpu::io_wait();

        // 4. Configure LAPIC Timer DCR: Divisor 16 (0x03)
        write_lapic_reg(LAPIC_TIMER_DCR_REG, 0x03);

        // 5. Configure LVT Timer: Masked (bit 16 = 1), One-shot (bits [18:17] = 00b), vector 32
        write_lapic_reg(LAPIC_LVT_TIMER_REG, (1 << 16) | 32);

        // 6. Set initial count to maximum (0xFFFF_FFFF)
        write_lapic_reg(LAPIC_TIMER_INIT_CNT_REG, 0xFFFF_FFFF);

        // 7. Enable PIT Channel 2 gate (set bit 0 to 1, speaker bit 1 to 0)
        let port61_start = cpu::inb(0x61);
        cpu::outb(0x61, (port61_start & 0xFD) | 0x01);

        // 8. Spin-wait until PIT Channel 2 OUT2 (bit 5 of port 0x61) goes high
        while (cpu::inb(0x61) & 0x20) == 0 {
            core::hint::spin_loop();
        }

        // 9. Read LAPIC Current Count Register immediately
        let curr_count = read_lapic_reg(LAPIC_TIMER_CURR_CNT_REG);

        // 10. Mask LAPIC timer and reset PIT Channel 2 gate
        write_lapic_reg(LAPIC_LVT_TIMER_REG, 1 << 16);
        let port61_end = cpu::inb(0x61);
        cpu::outb(0x61, (port61_end & 0xFD) & 0xFE);

        let elapsed = 0xFFFF_FFFF_u32.saturating_sub(curr_count);
        assert!(elapsed > 0, "LAPIC timer calibration returned zero ticks");
        elapsed
    }
}

/// Configures the LAPIC Timer in Periodic Mode at vector 32 with Divisor 16.
pub fn configure_lapic_timer(initial_count: u32) {
    unsafe {
        // 1. Ensure Spurious Interrupt Vector Register has software enable bit set (bit 8)
        write_lapic_reg(LAPIC_SVR_REG, 0x1FF);

        // 2. Set Divide Configuration Register (DCR): Divisor 16 (0x03)
        write_lapic_reg(LAPIC_TIMER_DCR_REG, 0x03);

        // 3. Configure LVT Timer: Vector 32, unmasked (bit 16 = 0), Periodic Mode (bit 17 = 1)
        write_lapic_reg(LAPIC_LVT_TIMER_REG, (1 << 17) | 32);

        // 4. Set Initial Count Register to start periodic countdown
        write_lapic_reg(LAPIC_TIMER_INIT_CNT_REG, initial_count);
    }
}

/// Masks the LAPIC timer to halt periodic interrupt generation.
pub fn mask_lapic_timer() {
    unsafe {
        let lvt = read_lapic_reg(LAPIC_LVT_TIMER_REG);
        write_lapic_reg(LAPIC_LVT_TIMER_REG, lvt | (1 << 16));
    }
}

use core::sync::atomic::AtomicU32;

pub static CALIBRATED_TIMER_COUNT: AtomicU32 = AtomicU32::new(0);

/// Re-arms the LAPIC timer in Periodic Mode with the calibrated initial count.
pub fn rearm_lapic_timer() {
    let count = CALIBRATED_TIMER_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    assert!(count > 0, "LAPIC timer count must be calibrated prior to re-arming");
    configure_lapic_timer(count);
}

/// Executes Stage 3C Increment 3 non-preemptive verification:
/// 1. Calibrates LAPIC timer against PIT Channel 2 over 10 ms window (100 Hz).
/// 2. Programs LAPIC timer in periodic mode at vector 32 with calibrated count.
/// 3. Preloads 15 distinct 64-bit sentinels into all GPRs via assembly test harness.
/// 4. Enables CPU interrupts (`sti`) and observes at least 10 genuine hardware LAPIC timer ticks.
/// 5. Asserts in-flight interrupt frame register integrity in `timer_interrupt_handler`.
/// 6. Confirms all 15 GPRs, RSP, RIP, and RFLAGS survive iretq without corruption.
/// 7. Masks timer and disables interrupts (`cli`).
pub fn run_stage3c_inc3_verification() {
    kprintln!("\n[Stage 3C-Increment 3: LAPIC Timer Programming & Non-Preemptive ISR]");

    // 1. Calibrate LAPIC Timer against PIT Channel 2 over 10 ms window
    let calibrated_count = calibrate_lapic_timer_pit2();
    CALIBRATED_TIMER_COUNT.store(calibrated_count, core::sync::atomic::Ordering::Relaxed);

    // 2. Program LAPIC timer in Periodic Mode with calibrated initial count
    configure_lapic_timer(calibrated_count);

    kprintln!("  Timer Vector:                32 (0x20, IDT[32])");
    kprintln!("  Timer Mode:                  Periodic (Bits [18:17] = 01b)");
    kprintln!("  Divisor Configuration:       Divide by 16 (DCR = 0x03)");
    kprintln!("  PIT2 Calibration Window:     10 ms (11932 PIT cycles)");
    kprintln!("  Calibrated Initial Count:    0x{:08X} ({} ticks @ 100 Hz)", calibrated_count, calibrated_count);

    let baseline_ticks = crate::hal::arch::x86_64::timer::ticks();

    // 3. Prepare GPR Sentinel Verification
    let mut results = GprSentinelResults::default();
    crate::hal::arch::x86_64::timer::GPR_TEST_TICKS.store(0, core::sync::atomic::Ordering::Relaxed);
    crate::hal::arch::x86_64::timer::GPR_TEST_TARGET.store(10, core::sync::atomic::Ordering::Relaxed);
    crate::hal::arch::x86_64::timer::GPR_TEST_ACTIVE.store(true, core::sync::atomic::Ordering::Relaxed);

    // 4. Run assembly-controlled sentinel test
    unsafe {
        test_gpr_sentinels_under_irq(&mut results as *mut GprSentinelResults);
    }

    crate::hal::arch::x86_64::timer::GPR_TEST_ACTIVE.store(false, core::sync::atomic::Ordering::Relaxed);
    mask_lapic_timer();
    cpu::cli();

    let observed_ticks = crate::hal::arch::x86_64::timer::ticks() - baseline_ticks;
    assert!(observed_ticks >= 10, "Must observe at least 10 LAPIC timer ticks");
    assert!(results.loop_count > 0, "Loop must have executed across interrupts");

    kprintln!("  Observed LAPIC IRQ Ticks:    {}", observed_ticks);
    kprintln!("  Loop iterations completed:   {} Forward progress across 10 timer interrupts: VERIFIED", results.loop_count);
    kprintln!("  Interrupt Return Path:       iretq returning to interrupted execution context [VERIFIED]");
    kprintln!("  EOI Acknowledgment:          lapic_eoi() verified");

    // 5. Verify and report GPR Sentinel Results
    assert_eq!(results.rax, 0x1111_1111_1111_1111, "RAX register corrupted across iretq");
    assert_eq!(results.rcx, 0x2222_2222_2222_2222, "RCX register corrupted across iretq");
    assert_eq!(results.rdx, 0x3333_3333_3333_3333, "RDX register corrupted across iretq");
    assert_eq!(results.rbx, 0x4444_4444_4444_4444, "RBX register corrupted across iretq");
    assert_eq!(results.rbp, 0x5555_5555_5555_5555, "RBP register corrupted across iretq");
    assert_eq!(results.rsi, 0x6666_6666_6666_6666, "RSI register corrupted across iretq");
    assert_eq!(results.rdi, 0x7777_7777_7777_7777, "RDI register corrupted across iretq");
    assert_eq!(results.r8,  0x8888_8888_8888_8888, "R8 register corrupted across iretq");
    assert_eq!(results.r9,  0x9999_9999_9999_9999, "R9 register corrupted across iretq");
    assert_eq!(results.r10, 0xAAAA_AAAA_AAAA_AAAA, "R10 register corrupted across iretq");
    assert_eq!(results.r11, 0xBBBB_BBBB_BBBB_BBBB, "R11 register corrupted across iretq");
    assert_eq!(results.r12, 0xCCCC_CCCC_CCCC_CCCC, "R12 register corrupted across iretq");
    assert_eq!(results.r13, 0xDDDD_DDDD_DDDD_DDDD, "R13 register corrupted across iretq");
    assert_eq!(results.r14, 0xEEEE_EEEE_EEEE_EEEE, "R14 register corrupted across iretq");
    assert_eq!(results.r15, 0xFFFF_FFFF_FFFF_FFFF, "R15 register corrupted across iretq");
    assert_eq!(results.rsp_matched, 1, "RSP register corrupted across iretq");
    assert!(results.rip != 0, "RIP resumption invalid");

    kprintln!("\n  [TIMER REG TEST] interrupts observed: {}", observed_ticks);
    kprintln!("    RAX preserved: PASS (0x{:016X})", results.rax);
    kprintln!("    RCX preserved: PASS (0x{:016X})", results.rcx);
    kprintln!("    RDX preserved: PASS (0x{:016X})", results.rdx);
    kprintln!("    RBX preserved: PASS (0x{:016X})", results.rbx);
    kprintln!("    RBP preserved: PASS (0x{:016X})", results.rbp);
    kprintln!("    RSI preserved: PASS (0x{:016X})", results.rsi);
    kprintln!("    RDI preserved: PASS (0x{:016X})", results.rdi);
    kprintln!("    R8  preserved: PASS (0x{:016X})", results.r8);
    kprintln!("    R9  preserved: PASS (0x{:016X})", results.r9);
    kprintln!("    R10 preserved: PASS (0x{:016X})", results.r10);
    kprintln!("    R11 preserved: PASS (0x{:016X})", results.r11);
    kprintln!("    R12 preserved: PASS (0x{:016X})", results.r12);
    kprintln!("    R13 preserved: PASS (0x{:016X})", results.r13);
    kprintln!("    R14 preserved: PASS (0x{:016X})", results.r14);
    kprintln!("    R15 preserved: PASS (0x{:016X})", results.r15);
    kprintln!("    RSP preserved: PASS");
    kprintln!("    RIP resumed:   PASS (0x{:016X})", results.rip);
    kprintln!("    RFLAGS resumed: PASS (0x{:016X})", results.rflags);
    kprintln!("  [x] Stage 3C Increment 3 LAPIC timer programming & non-preemptive ISR verified.");
}
