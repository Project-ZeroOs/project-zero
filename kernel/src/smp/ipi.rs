//! Stage 3N — IPI Dispatch Engine
//!
//! Provides bounded ICR delivery, LAPIC SVR configuration,
//! and the zero-lock ISR handlers for vectors 251, 252, 253, and 255.
//!
//! Frozen IPI vectors:
//!   0xFB (251) = IPI_VECTOR_STOP
//!   0xFC (252) = IPI_VECTOR_RESCHEDULE
//!   0xFD (253) = IPI_VECTOR_TLB_SHOOTDOWN
//!   0xFF (255) = LAPIC_SPURIOUS_VECTOR

use core::sync::atomic::Ordering;
use crate::hal::arch::x86_64::lapic::{
    read_lapic_reg, write_lapic_reg, lapic_eoi,
    LAPIC_SVR_REG, LAPIC_ICR_LOW_REG, LAPIC_ICR_HIGH_REG,
    ICR_DELIVERY_FIXED, ICR_LEVEL_ASSERT, ICR_TRIGGER_EDGE, ICR_SHORTHAND_NONE,
    ICR_DELIVERY_INIT, ICR_DELIVERY_STARTUP, ICR_DELIVERY_STATUS_PENDING,
    MAX_IPI_POLL_ITERATIONS as LAPIC_MAX_POLL,
};
use crate::hal::arch::x86_64::lapic::InterruptFrame;
use crate::kprintln;
use super::types::{
    MAX_CPUS, PER_CPU_STATE, TLB_SHOOTDOWN_STATE,
    ONLINE_CPU_COUNT, MAX_IPI_POLL_ITERATIONS,
    IPI_VECTOR_STOP, IPI_VECTOR_RESCHEDULE, IPI_VECTOR_TLB_SHOOTDOWN,
};
use crate::task::percpu;

// ====================================================================
// LAPIC Spurious Vector & SVR Configuration
// ====================================================================

/// Enables the LAPIC by writing SVR = 0x1FF (APIC enabled + spurious vector 0xFF).
/// Must be called on each AP after its LAPIC is mapped.
pub unsafe fn enable_lapic_with_svr() {
    let svr = read_lapic_reg(LAPIC_SVR_REG);
    // Bit 8: APIC Software Enable. Bits [7:0]: Spurious interrupt vector (0xFF).
    write_lapic_reg(LAPIC_SVR_REG, svr | (1 << 8) | 0xFF);
}

// ====================================================================
// ICR-Based IPI Dispatch
// ====================================================================

/// Sends a fixed-delivery IPI to a single destination LAPIC ID.
/// Bounded wait: up to MAX_IPI_POLL_ITERATIONS for delivery status clear.
/// Returns true if delivery completed; false on timeout (caller determines severity).
pub unsafe fn send_ipi_to_lapic(target_lapic_id: u32, vector: u8) -> bool {
    // 1. Write ICR High: destination LAPIC ID in bits [31:24]
    write_lapic_reg(LAPIC_ICR_HIGH_REG, target_lapic_id << 24);

    // 2. Write ICR Low: Fixed delivery | edge trigger | assert | vector
    let icr_low = ICR_DELIVERY_FIXED
        | ICR_LEVEL_ASSERT
        | ICR_TRIGGER_EDGE
        | ICR_SHORTHAND_NONE
        | (vector as u32);
    write_lapic_reg(LAPIC_ICR_LOW_REG, icr_low);

    // 3. Poll delivery status (bit 12 = pending); bounded wait
    for _ in 0..MAX_IPI_POLL_ITERATIONS {
        let status = read_lapic_reg(LAPIC_ICR_LOW_REG);
        if (status & ICR_DELIVERY_STATUS_PENDING) == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

/// Sends INIT IPI (for AP startup sequence).
pub unsafe fn send_init_ipi(target_lapic_id: u32) {
    write_lapic_reg(LAPIC_ICR_HIGH_REG, target_lapic_id << 24);
    write_lapic_reg(LAPIC_ICR_LOW_REG, ICR_DELIVERY_INIT | ICR_LEVEL_ASSERT | ICR_TRIGGER_EDGE);
    for _ in 0..LAPIC_MAX_POLL {
        if (read_lapic_reg(LAPIC_ICR_LOW_REG) & ICR_DELIVERY_STATUS_PENDING) == 0 { break; }
        core::hint::spin_loop();
    }
}

/// Sends Startup IPI (SIPI) with the given page vector (0x08 → physical 0x8000).
pub unsafe fn send_sipi(target_lapic_id: u32, sipi_vector: u8) {
    write_lapic_reg(LAPIC_ICR_HIGH_REG, target_lapic_id << 24);
    write_lapic_reg(LAPIC_ICR_LOW_REG, ICR_DELIVERY_STARTUP | ICR_LEVEL_ASSERT | ICR_TRIGGER_EDGE | (sipi_vector as u32));
    for _ in 0..LAPIC_MAX_POLL {
        if (read_lapic_reg(LAPIC_ICR_LOW_REG) & ICR_DELIVERY_STATUS_PENDING) == 0 { break; }
        core::hint::spin_loop();
    }
}

/// Dispatches IPI_VECTOR_STOP (251) to a target CPU.
/// Returns true if delivery succeeded within MAX_IPI_POLL_ITERATIONS.
pub unsafe fn send_stop_ipi(target_lapic_id: u32) -> bool {
    send_ipi_to_lapic(target_lapic_id, IPI_VECTOR_STOP)
}

/// Dispatches IPI_VECTOR_RESCHEDULE (252) to a target CPU.
/// Returns true if delivery succeeded.
pub unsafe fn send_reschedule_ipi(target_lapic_id: u32) -> bool {
    send_ipi_to_lapic(target_lapic_id, IPI_VECTOR_RESCHEDULE)
}

/// Dispatches IPI_VECTOR_TLB_SHOOTDOWN (253) to all CPUs in target_mask.
/// Returns bitmask of CPUs that were successfully dispatched.
pub unsafe fn send_tlb_shootdown_ipi(target_mask: u32) -> u32 {
    let mut dispatched = 0u32;
    for i in 0..MAX_CPUS {
        if (target_mask & (1 << i)) != 0 {
            let lapic_id = PER_CPU_STATE[i].lapic_id;
            if send_ipi_to_lapic(lapic_id, IPI_VECTOR_TLB_SHOOTDOWN) {
                dispatched |= 1 << i;
            }
        }
    }
    dispatched
}

// ====================================================================
// IPI ISR Handlers (called from boot/isr.asm stubs)
// Zero-lock guarantees for ISR 253 (TLB shootdown).
// ====================================================================

/// ISR 251: IPI_VECTOR_STOP — Emergency halt.
#[no_mangle]
pub extern "C" fn ipi_stop_handler(_frame: *mut InterruptFrame) {
    unsafe {
        lapic_eoi();
        let cpu_id = percpu::current_cpu_id() as usize;
        if cpu_id < MAX_CPUS {
            PER_CPU_STATE[cpu_id].state = super::types::CpuLifecycleState::Offline;
        }
    }
    loop { unsafe { core::arch::asm!("cli; hlt"); } }
}

/// ISR 252: IPI_VECTOR_RESCHEDULE — Deferred reschedule request.
#[no_mangle]
pub extern "C" fn ipi_reschedule_handler(_frame: *mut InterruptFrame) {
    unsafe {
        let percpu = percpu::current_percpu_ptr();
        (*percpu).need_resched = 1;
        let cpu_id = percpu::current_cpu_id() as usize;
        if cpu_id < MAX_CPUS {
            PER_CPU_STATE[cpu_id].interrupt_count.fetch_add(1, Ordering::Relaxed);
        }
        lapic_eoi();
    }
}

/// ISR 253: IPI_VECTOR_TLB_SHOOTDOWN — ZERO LOCK.
/// Invariants:
/// - `I-SMP-TLB-2`: Binds ACK to (aspace_id, generation) tuple.
/// - `I-SMP-TLB-3`: Reads active with Acquire; writes ack_gen with Release.
#[no_mangle]
pub extern "C" fn ipi_tlb_handler(_frame: *mut InterruptFrame) {
    unsafe {
        let cpu_id = percpu::current_cpu_id() as usize;
        if cpu_id < MAX_CPUS {
            let slot = &super::types::TLB_SHOOTDOWN_REQUEST[cpu_id];
            if slot.active.load(Ordering::Acquire) {
                let vaddr = slot.vaddr.load(Ordering::Relaxed);
                let aspace_id = slot.aspace_id.load(Ordering::Relaxed);
                let gen = slot.generation.load(Ordering::Relaxed);

                if vaddr == 0 {
                    let cr3: u64;
                    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
                    core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack, preserves_flags));
                } else {
                    core::arch::asm!("invlpg [{}]", in(reg) vaddr, options(nostack, preserves_flags));
                }

                PER_CPU_STATE[cpu_id].tlb_ack_aspace_id.store(aspace_id, Ordering::Relaxed);
                PER_CPU_STATE[cpu_id].tlb_ack_gen.store(gen, Ordering::Release);
                PER_CPU_STATE[cpu_id].interrupt_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        lapic_eoi();
    }
}
