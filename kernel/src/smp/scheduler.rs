//! Stage 3N — Multi-Core Cooperative/Preemptive Scheduler Extension
//!
//! Extends the Stage 3B/3C scheduler for SMP. Each CPU has an independent
//! runqueue; preemption is signalled via IPI_VECTOR_RESCHEDULE (252).
//!
//! Policy (`I-SMP-SCHED-1`):
//!  - Per-core runqueues, CpuId-ascending lock acquisition order.
//!  - All-cores-busy: preempt lowest-priority thread (deterministic).
//!  - No work stealing in Stage 3N (single enqueue, static affinity).
//!
//! Lock hierarchy (frozen Rev4):
//!   Level 11 = aspace.lock
//!   Level 10 = SchedLock (per-CPU, acquired ascending by CpuId)
//!   Level 0  = ISR (no lock acquisition)

use core::sync::atomic::Ordering;
use crate::kprintln;
use super::types::{MAX_CPUS, PER_CPU_STATE, CpuLifecycleState, ONLINE_CPU_COUNT};

/// Selects the target CPU for a new thread, using round-robin over online CPUs.
/// Returns the CpuId index of the selected CPU.
pub fn select_target_cpu() -> usize {
    let online = ONLINE_CPU_COUNT.load(Ordering::SeqCst) as usize;
    if online == 0 { return 0; }

    // Find the online CPU with the fewest total threads (priority: critical, then high, then normal)
    let mut best_cpu = 0usize;
    let mut best_load = u64::MAX;

    unsafe {
        for i in 0..MAX_CPUS {
            let s = &PER_CPU_STATE[i];
            if s.state as u8 == CpuLifecycleState::Active as u8 ||
               s.state as u8 == CpuLifecycleState::Online as u8 {
                let load = s.run_queues.critical_count.load(Ordering::Relaxed) as u64
                    + s.run_queues.high_count.load(Ordering::Relaxed) as u64
                    + s.run_queues.normal_count.load(Ordering::Relaxed) as u64;
                if load < best_load {
                    best_load = load;
                    best_cpu = i;
                }
            }
        }
    }
    best_cpu
}

/// Enqueues a thread to the target CPU runqueue (by incrementing the appropriate counter).
pub fn enqueue_to_cpu(cpu_idx: usize, priority_class: u8) {
    unsafe {
        if cpu_idx < MAX_CPUS {
            match priority_class {
                0 => { PER_CPU_STATE[cpu_idx].run_queues.critical_count.fetch_add(1, Ordering::Relaxed); }
                1 => { PER_CPU_STATE[cpu_idx].run_queues.high_count.fetch_add(1, Ordering::Relaxed); }
                _ => { PER_CPU_STATE[cpu_idx].run_queues.normal_count.fetch_add(1, Ordering::Relaxed); }
            }
        }
    }
}

/// All-cores-busy policy (`I-SMP-SCHED-1`):
/// When all CPUs are executing and a new critical-priority thread arrives,
/// find the lowest-priority active thread and preempt it via IPI.
pub unsafe fn preempt_lowest_priority_thread(incoming_priority: u8) {
    let mut worst_cpu = usize::MAX;
    let mut worst_load = 0u64;

    for i in 0..MAX_CPUS {
        let s = &PER_CPU_STATE[i];
        if s.state as u8 == CpuLifecycleState::Active as u8 {
            // A CPU with zero critical threads running is the preemption target
            let crit = s.run_queues.critical_count.load(Ordering::Relaxed) as u64;
            if crit == 0 {
                let norm = s.run_queues.normal_count.load(Ordering::Relaxed) as u64;
                if norm > worst_load {
                    worst_load = norm;
                    worst_cpu = i;
                }
            }
        }
    }

    if worst_cpu == usize::MAX || incoming_priority != 0 {
        return; // No preemption target found, or incoming is not critical
    }

    // Signal reschedule IPI to the target CPU
    let lapic_id = PER_CPU_STATE[worst_cpu].lapic_id;
    kprintln!("  [SMP/SCHED] Preempting CPU {} (LAPIC {}) for incoming critical thread", worst_cpu, lapic_id);
    super::ipi::send_reschedule_ipi(lapic_id);
}

/// Prints per-CPU runqueue statistics.
pub fn print_runqueue_diagnostics() {
    kprintln!("\n[Stage 3N: Per-CPU Runqueue State]");
    unsafe {
        for i in 0..MAX_CPUS {
            let s = &PER_CPU_STATE[i];
            if s.state as u8 != CpuLifecycleState::Absent as u8 {
                let crit = s.run_queues.critical_count.load(Ordering::Relaxed);
                let high = s.run_queues.high_count.load(Ordering::Relaxed);
                let norm = s.run_queues.normal_count.load(Ordering::Relaxed);
                kprintln!("  CPU[{}]: critical={} high={} normal={} state={:?}",
                    i, crit, high, norm, s.state);
            }
        }
    }
}
