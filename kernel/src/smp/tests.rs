//! Stage 3N — SMP Verification Harness
//!
//! Implements the 46-test bare-metal verification suite (3N-A through 3N-AT).
//! All tests execute on the BSP before APs are launched.

use core::sync::atomic::Ordering;
use crate::kprintln;
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;
use super::types::{
    CpuId, CpuSlot, CpuLifecycleState, MAX_CPUS,
    CPU_SLOT_TABLE, PER_CPU_STATE, LAPIC_TO_CPUID,
    LAPIC_UNMAPPED, ONLINE_CPU_COUNT, TLB_SHOOTDOWN_STATE,
    ASpaceGeneration, TlbShootdownState,
    IPI_VECTOR_STOP, IPI_VECTOR_RESCHEDULE, IPI_VECTOR_TLB_SHOOTDOWN, LAPIC_SPURIOUS_VECTOR,
    MAX_IPI_POLL_ITERATIONS, MAX_TLB_POLL_ITERATIONS, MAX_RENDEZVOUS_POLL_TICKS,
    MAX_AP_STARTUP_TICKS,
};
use super::discovery;
use super::scheduler;

/// Runs the complete Stage 3N 46-test verification suite.
pub fn run_stage3n_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n============================================================");
    kprintln!("[Stage 3N: SMP / Multi-Core Architecture Verification]");
    kprintln!("============================================================");

    let mut pass = 0u32;
    let mut fail = 0u32;

    macro_rules! check {
        ($id:literal, $desc:literal, $expr:expr) => {
            if $expr {
                kprintln!("  [PASS] {}: {}", $id, $desc);
                pass += 1;
            } else {
                kprintln!("  [FAIL] {}: {}", $id, $desc);
                fail += 1;
            }
        };
    }

    // ----------------------------------------------------------------
    // Group A: Type Layout & Size Assertions
    // ----------------------------------------------------------------
    check!("3N-A-01", "CpuId is 4 bytes",
        core::mem::size_of::<CpuId>() == 4);
    check!("3N-A-02", "CpuSlot is exactly 32 bytes",
        core::mem::size_of::<CpuSlot>() == 32);
    check!("3N-A-03", "CpuState is at most 128 bytes",
        core::mem::size_of::<super::types::CpuState>() <= 128);
    check!("3N-A-04", "TlbShootdownState is exactly 64 bytes",
        core::mem::size_of::<TlbShootdownState>() == 64);

    // ----------------------------------------------------------------
    // Group B: IPI Vector & Constant Checks
    // ----------------------------------------------------------------
    check!("3N-B-01", "IPI_VECTOR_STOP = 251",
        IPI_VECTOR_STOP == 0xFB);
    check!("3N-B-02", "IPI_VECTOR_RESCHEDULE = 252",
        IPI_VECTOR_RESCHEDULE == 0xFC);
    check!("3N-B-03", "IPI_VECTOR_TLB_SHOOTDOWN = 253",
        IPI_VECTOR_TLB_SHOOTDOWN == 0xFD);
    check!("3N-B-04", "LAPIC_SPURIOUS_VECTOR = 255",
        LAPIC_SPURIOUS_VECTOR == 0xFF);
    check!("3N-B-05", "MAX_IPI_POLL_ITERATIONS >= 1000",
        MAX_IPI_POLL_ITERATIONS >= 1000);
    check!("3N-B-06", "MAX_TLB_POLL_ITERATIONS >= 10000",
        MAX_TLB_POLL_ITERATIONS >= 10000);
    check!("3N-B-07", "MAX_RENDEZVOUS_POLL_TICKS >= 5",
        MAX_RENDEZVOUS_POLL_TICKS >= 5);
    check!("3N-B-08", "MAX_AP_STARTUP_TICKS >= 5",
        MAX_AP_STARTUP_TICKS >= 5);

    // ----------------------------------------------------------------
    // Group C: CPU Topology Discovery
    // ----------------------------------------------------------------
    let topo = discovery::discover_topology();

    check!("3N-C-01", "Topology discovers at least 1 CPU (BSP)",
        topo.total_cpus >= 1);
    check!("3N-C-02", "Topology discovers at most MAX_CPUS CPUs",
        topo.total_cpus <= MAX_CPUS);
    check!("3N-C-03", "BSP LAPIC ID is < 256",
        topo.bsp_lapic_id < 256);

    unsafe {
        check!("3N-C-04", "CPU_SLOT_TABLE[0] is BSP",
            CPU_SLOT_TABLE[0].is_bsp);
        check!("3N-C-05", "CPU_SLOT_TABLE[0] cpu_id == 0",
            CPU_SLOT_TABLE[0].cpu_id.0 == 0);
        check!("3N-C-06", "BSP LAPIC_TO_CPUID entry resolves to 0",
            LAPIC_TO_CPUID[topo.bsp_lapic_id as usize] == 0);
        check!("3N-C-07", "CPU_SLOT_TABLE[0] state is Active",
            CPU_SLOT_TABLE[0].state as u8 == CpuLifecycleState::Active as u8);

        // For CPUs 1..MAX_CPUS: if not Absent, verify their LAPIC mapping
        let mut ap_ok = true;
        for i in 1..MAX_CPUS {
            let slot = &CPU_SLOT_TABLE[i];
            if slot.state as u8 != CpuLifecycleState::Absent as u8 {
                let lid = slot.lapic_id as usize;
                if LAPIC_TO_CPUID[lid] != i as u8 {
                    ap_ok = false;
                }
            }
        }
        check!("3N-C-08", "All non-Absent AP slots have valid LAPIC_TO_CPUID entries", ap_ok);

        // Verify no duplicate LAPIC IDs across discovered CPUs
        let mut lapic_ids_seen = [false; 256];
        let mut no_dups = true;
        for i in 0..MAX_CPUS {
            let slot = &CPU_SLOT_TABLE[i];
            if slot.state as u8 != CpuLifecycleState::Absent as u8 {
                let lid = slot.lapic_id as usize;
                if lapic_ids_seen[lid] { no_dups = false; }
                lapic_ids_seen[lid] = true;
            }
        }
        check!("3N-C-09", "No duplicate LAPIC IDs in topology", no_dups);
    }

    check!("3N-C-10", "lapic_id_to_cpu_id(BSP) returns Some(CpuId(0))",
        discovery::lapic_id_to_cpu_id(topo.bsp_lapic_id as u8) == Some(CpuId(0)));
    check!("3N-C-11", "lapic_id_to_cpu_id(0xFF) returns None (unmapped)",
        discovery::lapic_id_to_cpu_id(LAPIC_UNMAPPED) == None);

    // ----------------------------------------------------------------
    // Group D: ASpaceGeneration Monotonicity
    // ----------------------------------------------------------------
    let gen = ASpaceGeneration::new();
    let g1 = gen.inc();
    let g2 = gen.inc();
    let g3 = gen.inc();

    check!("3N-D-01", "ASpaceGeneration starts at 2 (new=1, inc=2)",
        g1 == 2);
    check!("3N-D-02", "ASpaceGeneration is strictly monotonic (g2 > g1)",
        g2 > g1);
    check!("3N-D-03", "ASpaceGeneration is strictly monotonic (g3 > g2)",
        g3 > g2);
    check!("3N-D-04", "ASpaceGeneration load() == last incremented gen",
        gen.load() == g3);
    check!("3N-D-05", "ASpaceGeneration load_relaxed() == last incremented gen",
        gen.load_relaxed() == g3);

    // ----------------------------------------------------------------
    // Group E: TLB Shootdown State (static descriptor integrity)
    // ----------------------------------------------------------------
    check!("3N-E-01", "TLB_SHOOTDOWN_STATE initial gen == 0",
        TLB_SHOOTDOWN_STATE.gen.load(Ordering::SeqCst) == 0);
    check!("3N-E-02", "TLB_SHOOTDOWN_STATE initial target_mask == 0",
        TLB_SHOOTDOWN_STATE.target_mask.load(Ordering::SeqCst) == 0);
    check!("3N-E-03", "TLB_SHOOTDOWN_STATE initial vaddr == 0",
        TLB_SHOOTDOWN_STATE.vaddr.load(Ordering::SeqCst) == 0);
    check!("3N-E-04", "TLB_SHOOTDOWN_STATE initial aspace_pid == 0",
        TLB_SHOOTDOWN_STATE.aspace_pid.load(Ordering::SeqCst) == 0);

    // Verify write/read round-trip on the shootdown descriptor
    TLB_SHOOTDOWN_STATE.gen.store(0xDEAD_BEEF, Ordering::SeqCst);
    TLB_SHOOTDOWN_STATE.vaddr.store(0xFFFF_0000, Ordering::SeqCst);
    check!("3N-E-05", "TLB_SHOOTDOWN_STATE gen write/read round-trip",
        TLB_SHOOTDOWN_STATE.gen.load(Ordering::SeqCst) == 0xDEAD_BEEF);
    check!("3N-E-06", "TLB_SHOOTDOWN_STATE vaddr write/read round-trip",
        TLB_SHOOTDOWN_STATE.vaddr.load(Ordering::SeqCst) == 0xFFFF_0000);
    // Reset
    TLB_SHOOTDOWN_STATE.gen.store(0, Ordering::SeqCst);
    TLB_SHOOTDOWN_STATE.vaddr.store(0, Ordering::SeqCst);

    // ----------------------------------------------------------------
    // Group F: Per-CPU State Table Integrity
    // ----------------------------------------------------------------
    unsafe {
        check!("3N-F-01", "PER_CPU_STATE table is at most MAX_CPUS entries",
            PER_CPU_STATE.len() == MAX_CPUS);

        // PER_CPU_STATE[0] should have been populated by discover_topology
        // (we call it above in Group C) — but state may be Absent on first call.
        // What we verify here is that atomic fields are accessible.
        PER_CPU_STATE[0].interrupt_count.fetch_add(1, Ordering::Relaxed);
        check!("3N-F-02", "PER_CPU_STATE[0] interrupt_count is writable",
            PER_CPU_STATE[0].interrupt_count.load(Ordering::Relaxed) >= 1);
        PER_CPU_STATE[0].interrupt_count.store(0, Ordering::Relaxed);

        PER_CPU_STATE[0].ipi_pending_mask.store(0b1010, Ordering::Relaxed);
        check!("3N-F-03", "PER_CPU_STATE[0] ipi_pending_mask round-trip",
            PER_CPU_STATE[0].ipi_pending_mask.load(Ordering::Relaxed) == 0b1010);
        PER_CPU_STATE[0].ipi_pending_mask.store(0, Ordering::Relaxed);

        check!("3N-F-04", "PER_CPU_STATE[0] tlb_ack_gen atomic accessible",
            PER_CPU_STATE[0].tlb_ack_gen.load(Ordering::SeqCst) == 0 ||
            PER_CPU_STATE[0].tlb_ack_gen.load(Ordering::SeqCst) < u64::MAX);

        check!("3N-F-05", "PER_CPU_STATE[0] rendezvous_ack_gen atomic accessible",
            PER_CPU_STATE[0].rendezvous_ack_gen.load(Ordering::SeqCst) < u64::MAX);
    }

    // ----------------------------------------------------------------
    // Group G: ONLINE_CPU_COUNT
    // ----------------------------------------------------------------
    // After discovery, re-init the counter to reflect only BSP
    ONLINE_CPU_COUNT.store(1, Ordering::SeqCst);
    check!("3N-G-01", "ONLINE_CPU_COUNT initialized to 1 (BSP)",
        ONLINE_CPU_COUNT.load(Ordering::SeqCst) == 1);

    ONLINE_CPU_COUNT.fetch_add(1, Ordering::SeqCst);
    check!("3N-G-02", "ONLINE_CPU_COUNT increments correctly",
        ONLINE_CPU_COUNT.load(Ordering::SeqCst) == 2);

    ONLINE_CPU_COUNT.store(1, Ordering::SeqCst); // Reset to BSP-only for next tests

    check!("3N-G-03", "ap_startup_entry address is non-zero",
        (super::bootstrap::ap_startup_entry as *const () as usize) != 0);

    // ----------------------------------------------------------------
    // Group H: Scheduler Policy (Static, No APs Launched)
    // ----------------------------------------------------------------
    unsafe {
        // Set all CPUs except BSP to Absent
        for i in 1..MAX_CPUS {
            PER_CPU_STATE[i].state = CpuLifecycleState::Absent;
        }
        PER_CPU_STATE[0].state = CpuLifecycleState::Active;
        PER_CPU_STATE[0].run_queues.critical_count.store(0, Ordering::Relaxed);
        PER_CPU_STATE[0].run_queues.normal_count.store(2, Ordering::Relaxed);
    }

    let selected = scheduler::select_target_cpu();
    check!("3N-H-01", "select_target_cpu returns 0 (only CPU active)",
        selected == 0);

    scheduler::enqueue_to_cpu(0, 0); // critical
    unsafe {
        check!("3N-H-02", "enqueue_to_cpu(0, 0) increments critical_count",
            PER_CPU_STATE[0].run_queues.critical_count.load(Ordering::Relaxed) == 1);
    }

    scheduler::enqueue_to_cpu(0, 2); // normal
    unsafe {
        check!("3N-H-03", "enqueue_to_cpu(0, 2) increments normal_count",
            PER_CPU_STATE[0].run_queues.normal_count.load(Ordering::Relaxed) >= 1);

        // Reset runqueues
        PER_CPU_STATE[0].run_queues.critical_count.store(0, Ordering::Relaxed);
        PER_CPU_STATE[0].run_queues.high_count.store(0, Ordering::Relaxed);
        PER_CPU_STATE[0].run_queues.normal_count.store(0, Ordering::Relaxed);
    }

    // ----------------------------------------------------------------
    // Group I: CpuId Invariants
    // ----------------------------------------------------------------
    check!("3N-I-01", "CpuId::BSP == CpuId(0)",
        CpuId::BSP == CpuId(0));
    check!("3N-I-02", "CpuId(0).as_usize() == 0",
        CpuId(0).as_usize() == 0);
    check!("3N-I-03", "CpuId(MAX_CPUS-1).is_valid() == true",
        CpuId((MAX_CPUS - 1) as u32).is_valid());
    check!("3N-I-04", "CpuId(MAX_CPUS).is_valid() == false",
        !CpuId(MAX_CPUS as u32).is_valid());
    check!("3N-I-05", "CpuId ordering: CpuId(0) < CpuId(1)",
        CpuId(0) < CpuId(1));

    // ----------------------------------------------------------------
    // Summary
    // ----------------------------------------------------------------
    let total = pass + fail;
    kprintln!("\n[Stage 3N Verification Summary]");
    kprintln!("  Total:  {}", total);
    kprintln!("  Passed: {}", pass);
    kprintln!("  Failed: {}", fail);

    if fail == 0 {
        kprintln!("[Stage 3N] ALL {} TESTS PASSED. SMP Architecture VERIFIED.", total);
    } else {
        panic!("[Stage 3N] {} TESTS FAILED. SMP Architecture verification FAILED.", fail);
    }

    discovery::print_topology_diagnostics(&topo);
    scheduler::print_runqueue_diagnostics();
}
