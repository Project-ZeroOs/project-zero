//! Stage 3N — Synchronous TLB Shootdown Engine (`I-SMP-TLB-1`)
//!
//! Enforces:
//!  - Unified monotonic u64 generation namespace per AddressSpace
//!  - Exact ACK matching (== gen), not >= gen
//!  - Fail-closed panic on timeout (CRITICAL: TLB shootdown ACK timeout)
//!  - Deadlock freedom: ISR 253 acquires ZERO locks

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::kprintln;
use super::types::{
    MAX_CPUS, PER_CPU_STATE, TLB_SHOOTDOWN_STATE, TLB_SHOOTDOWN_REQUEST, TLB_SHOOTDOWN_LOCK,
    MAX_TLB_POLL_ITERATIONS, ASpaceGeneration,
};
use super::ipi::send_tlb_shootdown_ipi;

/// Performs a synchronous TLB shootdown for a virtual address in an address space.
///
/// Invariants enforced:
/// - `I-SMP-TLB-1`: Serialized by aspace.lock and TLB_SHOOTDOWN_LOCK.
/// - `I-SMP-TLB-2`: Binds ACK to (aspace_id, generation) tuple.
/// - `I-SMP-TLB-3`: Release/Acquire publication protocol on TlbTargetSlot.
/// - `I-SMP-TLB-4`: Monotonic non-reused aspace_id.
pub unsafe fn execute_tlb_shootdown(
    aspace_id: u64,
    gen: &ASpaceGeneration,
    active_cpu_mask: &AtomicU64,
    vaddr: u64,
    caller_cpu_id: usize,
) {
    // 1. Acquire TLB_SHOOTDOWN_LOCK (Level 10.5 in hierarchy)
    while TLB_SHOOTDOWN_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }

    // 2. Increment unified monotonic generation
    let g = gen.inc();

    // 3. Local invlpg
    if vaddr == 0 {
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack, preserves_flags));
    } else {
        core::arch::asm!("invlpg [{}]", in(reg) vaddr, options(nostack, preserves_flags));
    }

    // 4. Snapshot target_mask: active CPUs excluding self
    let target_mask = active_cpu_mask.load(Ordering::SeqCst) & !(1u64 << caller_cpu_id);

    if target_mask == 0 {
        TLB_SHOOTDOWN_LOCK.store(false, Ordering::Release);
        return;
    }

    // 5. Populate per-target request slots with Release publication (I-SMP-TLB-3)
    for k in 0..MAX_CPUS {
        if (target_mask & (1 << k)) != 0 {
            let slot = &TLB_SHOOTDOWN_REQUEST[k];
            slot.aspace_id.store(aspace_id, Ordering::Relaxed);
            slot.generation.store(g, Ordering::Relaxed);
            slot.vaddr.store(vaddr, Ordering::Relaxed);
            slot.active.store(true, Ordering::Release);
        }
    }

    // Also update legacy descriptor for telemetry/compatibility
    TLB_SHOOTDOWN_STATE.aspace_pid.store(aspace_id, Ordering::SeqCst);
    TLB_SHOOTDOWN_STATE.vaddr.store(vaddr, Ordering::SeqCst);
    TLB_SHOOTDOWN_STATE.target_mask.store(target_mask as u32, Ordering::SeqCst);
    TLB_SHOOTDOWN_STATE.gen.store(g, Ordering::SeqCst);

    // 6. Dispatch IPI to all target CPUs
    send_tlb_shootdown_ipi(target_mask as u32);

    // 7. Bounded wait: exact (aspace_id, generation) tuple matching (I-SMP-TLB-2)
    let mut acked_all = false;
    for _ in 0..MAX_TLB_POLL_ITERATIONS {
        let mut all_acked = true;
        for k in 0..MAX_CPUS {
            if (target_mask & (1 << k)) != 0 {
                let ack_gen = PER_CPU_STATE[k].tlb_ack_gen.load(Ordering::Acquire);
                let ack_aspace = PER_CPU_STATE[k].tlb_ack_aspace_id.load(Ordering::Acquire);
                if ack_gen != g || ack_aspace != aspace_id {
                    all_acked = false;
                    break;
                }
            }
        }
        if all_acked {
            acked_all = true;
            break;
        }
        core::hint::spin_loop();
    }

    // 8. Clear active flags on all target slots
    for k in 0..MAX_CPUS {
        if (target_mask & (1 << k)) != 0 {
            TLB_SHOOTDOWN_REQUEST[k].active.store(false, Ordering::Release);
        }
    }

    // 9. Release TLB_SHOOTDOWN_LOCK
    TLB_SHOOTDOWN_LOCK.store(false, Ordering::Release);

    if !acked_all {
        panic!("CRITICAL: TLB shootdown ACK timeout — unquiesced CPU (aspace={}, gen={})", aspace_id, g);
    }
}

/// Tracks AddressSpace activation per CPU.
pub unsafe fn register_cpu_in_aspace(
    cpu_id: usize,
    aspace_pid: u64,
    gen: u64,
    pml4_phys: u64,
) {
    core::arch::asm!(
        "mov cr3, {}",
        in(reg) pml4_phys,
        options(nomem, nostack, preserves_flags)
    );

    if cpu_id < MAX_CPUS {
        PER_CPU_STATE[cpu_id].tlb_ack_gen.store(gen, Ordering::Release);
        PER_CPU_STATE[cpu_id].tlb_ack_aspace_id.store(aspace_pid, Ordering::Release);
        PER_CPU_STATE[cpu_id].active_pid.store(aspace_pid, Ordering::Release);
    }
}

/// Clears this CPU's bit from the address space active_cpu_mask.
pub unsafe fn unregister_cpu_from_aspace(
    cpu_id: usize,
    active_cpu_mask: &AtomicU64,
) {
    active_cpu_mask.fetch_and(!(1u64 << cpu_id), Ordering::Release);
    if cpu_id < MAX_CPUS {
        PER_CPU_STATE[cpu_id].active_pid.store(0, Ordering::Release);
    }
}
