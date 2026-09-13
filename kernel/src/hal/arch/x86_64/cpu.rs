//! Project Zero - CPU Hardware Primitives (x86-64)
//! 
//! Provides low-level port I/O, control register queries, and execution state assertions
//! without external crate dependencies.

use core::arch::asm;

/// Reads an 8-bit byte from the specified x86 I/O port.
#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!(
        "in al, dx",
        in("dx") port,
        out("al") value,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// Writes an 8-bit byte to the specified x86 I/O port.
#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Brief I/O delay by writing to an unused legacy motherboard port (0x80).
#[inline(always)]
pub unsafe fn io_wait() {
    outb(0x80, 0);
}

/// Halts the CPU until the next interrupt (or permanently if interrupts are disabled).
#[inline(always)]
pub fn hlt() {
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

/// Disables hardware maskable interrupts.
#[inline(always)]
pub fn cli() {
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

/// Enables hardware maskable interrupts.
#[inline(always)]
pub fn sti() {
    unsafe {
        asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

/// Reads the CR0 control register (Paging, Protection, Numeric Error).
#[inline(always)]
pub fn read_cr0() -> u64 {
    let val: u64;
    unsafe {
        asm!("mov {}, cr0", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Writes the raw 64-bit value to the CR0 control register.
#[inline(always)]
pub unsafe fn write_cr0(val: u64) {
    asm!("mov cr0, {}", in(reg) val, options(nostack, preserves_flags));
}

/// Enables the Write Protect (WP) bit (bit 16) in CR0.
///
/// When CR0.WP = 1, supervisor (Ring 0) writes to pages with WRITABLE = 0
/// trigger a Page Fault (#PF, Vector 14) with error code bit 1 set (W/R = 1).
#[inline(always)]
pub fn enable_write_protect() {
    let cr0 = read_cr0();
    unsafe {
        write_cr0(cr0 | (1 << 16));
    }
}

/// Reads the CR3 control register (PML4 Physical Base Address).
#[inline(always)]
pub fn read_cr3() -> u64 {
    let val: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Writes the raw 64-bit value to the CR3 control register.
#[inline(always)]
pub unsafe fn write_cr3_raw(val: u64) {
    asm!("mov cr3, {}", in(reg) val, options(nostack, preserves_flags));
}

/// Invalidates the TLB entry for the specified virtual address using invlpg instruction.
#[inline(always)]
pub fn invlpg(addr: u64) {
    unsafe {
        asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
    }
}

/// Reads CPUID registers for the specified leaf.
#[inline(always)]
pub fn raw_cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let res = core::arch::x86_64::__cpuid(leaf);
    (res.eax, res.ebx, res.ecx, res.edx)
}

/// Reads CPUID registers for the specified leaf and subleaf.
#[inline(always)]
pub fn raw_cpuid_count(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let res = core::arch::x86_64::__cpuid_count(leaf, subleaf);
    (res.eax, res.ebx, res.ecx, res.edx)
}

/// Reads the CR4 control register (PAE, OSFXSR, PSE).
#[inline(always)]
pub fn read_cr4() -> u64 {
    let val: u64;
    unsafe {
        asm!("mov {}, cr4", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Reads the current Code Segment (CS) register to determine privilege level.
#[inline(always)]
pub fn read_cs() -> u16 {
    let val: u16;
    unsafe {
        asm!("mov {:x}, cs", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Reads the current Stack Segment (SS) register.
#[inline(always)]
pub fn read_ss() -> u16 {
    let val: u16;
    unsafe {
        asm!("mov {:x}, ss", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Reads the current Data Segment (DS) register.
#[inline(always)]
pub fn read_ds() -> u16 {
    let val: u16;
    unsafe {
        asm!("mov {:x}, ds", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Reads the current Extra Segment (ES) register.
#[inline(always)]
pub fn read_es() -> u16 {
    let val: u16;
    unsafe {
        asm!("mov {:x}, es", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Reads the current FS segment register.
#[inline(always)]
pub fn read_fs() -> u16 {
    let val: u16;
    unsafe {
        asm!("mov {:x}, fs", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Reads the current GS segment register.
#[inline(always)]
pub fn read_gs() -> u16 {
    let val: u16;
    unsafe {
        asm!("mov {:x}, gs", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}

/// Reads the current Task Register (TR) via `str` instruction.
#[inline(always)]
pub fn read_tr() -> u16 {
    let val: u16;
    unsafe {
        asm!("str {:x}", out(reg) val, options(nomem, nostack, preserves_flags));
    }
    val
}


/// Reads the 64-bit RFLAGS register.
#[inline(always)]
pub fn read_rflags() -> u64 {
    let val: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) val, options(nomem, preserves_flags));
    }
    val
}

/// Reads a 64-bit Model-Specific Register (MSR).
#[inline(always)]
pub fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// Writes a 64-bit Model-Specific Register (MSR).
#[inline(always)]
pub fn write_msr(msr: u32, val: u64) {
    let low = val as u32;
    let high = (val >> 32) as u32;
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// IA32_EFER Model-Specific Register address
pub const IA32_EFER: u32 = 0xC0000080;

/// Enables the No-Execute Enable (NXE) bit (bit 11) in IA32_EFER.
#[inline(always)]
pub fn enable_nxe() {
    let efer = read_msr(IA32_EFER);
    write_msr(IA32_EFER, efer | (1 << 11));
}

/// CPU Hardware State Snapshot for Stage 1 diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct CpuDiagnostics {
    pub ring: u8,
    pub cs: u16,
    pub cr0: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub efer: u64,
    pub rflags: u64,
    pub paging_enabled: bool,
    pub pae_enabled: bool,
    pub cr0_wp_enabled: bool,
    pub lme_enabled: bool,
    pub lma_active: bool,
    pub interrupts_enabled: bool,
}

impl CpuDiagnostics {
    pub fn capture() -> Self {
        let cs = read_cs();
        let cr0 = read_cr0();
        let cr3 = read_cr3();
        let cr4 = read_cr4();
        let efer = read_msr(IA32_EFER);
        let rflags = read_rflags();

        Self {
            ring: (cs & 0x3) as u8,
            cs,
            cr0,
            cr3,
            cr4,
            efer,
            rflags,
            paging_enabled: (cr0 & (1 << 31)) != 0,
            pae_enabled: (cr4 & (1 << 5)) != 0,
            cr0_wp_enabled: (cr0 & (1 << 16)) != 0,
            lme_enabled: (efer & (1 << 8)) != 0,
            lma_active: (efer & (1 << 10)) != 0,
            interrupts_enabled: (rflags & (1 << 9)) != 0,
        }
    }
}
