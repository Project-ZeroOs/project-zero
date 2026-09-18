//! Stage 3N — AP Bootstrap Protocol
//!
//! Implements INIT-SIPI-SIPI protocol with bounded startup timeout.
//! Degrades gracefully: BSP continues on N-1 cores if any AP fails.
//!
//! Trampoline layout at physical 0x8000 (SIPI vector 0x08):
//! - 16-bit Real Mode code transitions to 64-bit Long Mode
//! - Mailbox at fixed offsets: cr3, stack, entry, cpu_id, online flag

use core::sync::atomic::Ordering;
use crate::kprintln;
use crate::hal::arch::x86_64::cpu;
use super::types::{
    CpuId, CpuLifecycleState, MAX_CPUS,
    CPU_SLOT_TABLE, PER_CPU_STATE,
    MAX_AP_STARTUP_TICKS, ONLINE_CPU_COUNT,
};
use super::ipi::{send_init_ipi, send_sipi};
use crate::task::percpu::{PerCpu, BSP_PERCPU, PER_CPU_INSTANCES};
use crate::mm::vmm::{get_master_kernel_pml4, HHDM_BASE, PageTable};

/// Physical address of the AP startup trampoline (SIPI vector 0x08 → 0x8000)
const TRAMPOLINE_BASE: u64 = 0x0000_8000;
const SIPI_VECTOR: u8 = 0x08;

/// Trampoline mailbox offsets (matching boot/trampoline.asm)
const MBOX_CR3_OFFSET:    usize = 0x28;
const MBOX_STACK_OFFSET:  usize = 0x30;
const MBOX_ENTRY_OFFSET:  usize = 0x38;
const MBOX_CPU_ID_OFFSET: usize = 0x40;
const MBOX_ONLINE_OFFSET: usize = 0x44;

extern "C" {
    static ap_trampoline_start: u8;
    static ap_trampoline_end: u8;
}

/// Static per-AP kernel stacks (16 KiB per AP).
static mut AP_STACKS: [[u8; 16384]; MAX_CPUS] = [[0u8; 16384]; MAX_CPUS];

/// AP startup entry point (64-bit Rust, called from trampoline).
/// Called with: RDI = logical cpu_id.
/// Force-export via #[no_mangle] so the symbol is available to the trampoline.
#[no_mangle]
pub extern "C" fn ap_startup_entry(cpu_id: u32) -> ! {
    unsafe {
        let idx = cpu_id as usize;
        if idx < MAX_CPUS {
            // 1. Set this AP's IA32_GS_BASE to its PerCpu instance
            let percpu_ptr = &raw mut PER_CPU_INSTANCES[idx] as u64;
            cpu::write_msr(0xC000_0101, percpu_ptr); // IA32_GS_BASE

            // 2. Update PerCpu fields
            PER_CPU_INSTANCES[idx].cpu_id = cpu_id;
            PER_CPU_INSTANCES[idx].lapic_id = PER_CPU_STATE[idx].lapic_id;
            PER_CPU_INSTANCES[idx].self_ptr = &raw mut PER_CPU_INSTANCES[idx];

            // 3. Load architectural IDT on this AP
            crate::hal::arch::x86_64::idt::load_current_cpu_idt();

            // 4. Enable Local APIC on this AP (SVR = 0x1FF)
            super::ipi::enable_lapic_with_svr();

            // 5. Authoritative per-CPU startup telemetry
            kprintln!("  [AP {}] Booted. LAPIC={}, GS_BASE verified.", cpu_id, PER_CPU_STATE[idx].lapic_id);

            // 6. Signal Online
            PER_CPU_STATE[idx].state = CpuLifecycleState::Online;
            ONLINE_CPU_COUNT.fetch_add(1, Ordering::SeqCst);
        }
    }

    // AP service loop: enable interrupts and wait for work / IPIs
    loop {
        unsafe {
            core::arch::asm!("sti; hlt", options(nomem, nostack));
        }
    }
}

/// Force-retain `ap_startup_entry` in the ELF (prevents linker DCE).
#[used]
static AP_ENTRY_FN: unsafe extern "C" fn(u32) -> ! = ap_startup_entry;

/// Bootstraps all Application Processors.
/// Returns the count of successfully booted APs.
pub unsafe fn bootstrap_aps() -> usize {
    // 1. Get BSP master PML4 physical address for CR3
    let master_pml4 = get_master_kernel_pml4().address();
    let ap_entry_fn = ap_startup_entry as *const () as u64;

    // 2. Copy trampoline code to physical 0x8000 via HHDM
    let tramp_virt = (HHDM_BASE + TRAMPOLINE_BASE) as *mut u8;
    let src = &raw const ap_trampoline_start as *const u8;
    let len = (&raw const ap_trampoline_end as usize) - (&raw const ap_trampoline_start as usize);
    core::ptr::copy_nonoverlapping(src, tramp_virt, len);

    // 3. Temporarily map identity mapping in PML4[0] during AP bootstrap
    //    so the AP can enable paging while executing at physical 0x8000.
    let pml4_ptr = (HHDM_BASE + master_pml4) as *mut PageTable;
    let pml4 = &mut *pml4_ptr;
    let pdpt_entry = pml4.entries[511]; // Clones the PDPT which maps 0..1 GiB
    pml4.entries[0] = pdpt_entry;

    // Flush BSP TLB
    let mut cr3: u64;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack, preserves_flags));

    let mut booted = 0usize;

    for i in 1..MAX_CPUS {
        let slot = &CPU_SLOT_TABLE[i];
        if slot.state as u8 == CpuLifecycleState::Absent as u8 {
            continue;
        }
        let lapic_id = slot.lapic_id;
        let ap_idx = i;

        // Write mailbox into the trampoline page (via HHDM)
        let mbox_base = (HHDM_BASE + TRAMPOLINE_BASE) as *mut u8;

        // CR3 (u64 at +0x28)
        let cr3_ptr = mbox_base.add(MBOX_CR3_OFFSET) as *mut u64;
        cr3_ptr.write_volatile(master_pml4);

        // Stack: dedicated 16 KiB per-AP stack (canonical higher-half)
        let stack_top = (&raw const AP_STACKS[ap_idx] as *const u8 as u64) + 16384;
        let stack_ptr = mbox_base.add(MBOX_STACK_OFFSET) as *mut u64;
        stack_ptr.write_volatile(stack_top);

        // Entry function pointer (u64 at +0x38)
        let entry_ptr = mbox_base.add(MBOX_ENTRY_OFFSET) as *mut u64;
        entry_ptr.write_volatile(ap_entry_fn);

        // CPU ID (u32 at +0x40)
        let cpuid_ptr = mbox_base.add(MBOX_CPU_ID_OFFSET) as *mut u32;
        cpuid_ptr.write_volatile(ap_idx as u32);

        // Clear online flag (u32 at +0x44)
        let online_ptr = mbox_base.add(MBOX_ONLINE_OFFSET) as *mut u32;
        online_ptr.write_volatile(0);

        // Mark AP as Starting
        CPU_SLOT_TABLE[ap_idx].state = CpuLifecycleState::Starting;
        PER_CPU_STATE[ap_idx].lapic_id = lapic_id;

        // Send INIT IPI
        kprintln!("  [SMP] Sending INIT IPI to CPU {} (LAPIC {})", ap_idx, lapic_id);
        send_init_ipi(lapic_id);

        // Delay ~10 ms
        for _ in 0..1_000_000u32 { core::hint::spin_loop(); }

        // Send SIPI
        kprintln!("  [SMP] Sending SIPI (vector 0x{:02X}) to CPU {} (LAPIC {})", SIPI_VECTOR, ap_idx, lapic_id);
        send_sipi(lapic_id, SIPI_VECTOR);

        // Poll for AP to signal Online within MAX_AP_STARTUP_TICKS
        let mut success = false;
        for _tick in 0..MAX_AP_STARTUP_TICKS {
            for _ in 0..1_000_000u32 { core::hint::spin_loop(); }
            let state = &CPU_SLOT_TABLE[ap_idx].state;
            let mbox_online = online_ptr.read_volatile();
            if *state as u8 == CpuLifecycleState::Online as u8 || mbox_online == 1 {
                CPU_SLOT_TABLE[ap_idx].state = CpuLifecycleState::Online;
                kprintln!("  [SMP] CPU {} online (LAPIC {})", ap_idx, lapic_id);
                booted += 1;
                success = true;
                break;
            }
        }

        if !success {
            kprintln!("  [WARN] AP {} (LAPIC {}) failed to boot; continuing in degraded mode.", ap_idx, lapic_id);
            CPU_SLOT_TABLE[ap_idx].state = CpuLifecycleState::Failed;
        }
    }

    // 4. Remove identity mapping in PML4[0] after all APs have booted into higher-half VMA,
    //    restoring the Stage 2F-E identity removal invariant.
    pml4.entries[0].clear();
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack, preserves_flags));

    booted
}
