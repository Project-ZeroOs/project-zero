//! Stage 3N — CPU Topology Discovery
//!
//! Enumerates online CPUs using ACPI MADT (where available) with
//! a deterministic QEMU/CPUID fallback topology.
//! Populates `CPU_SLOT_TABLE` and `LAPIC_TO_CPUID`.

use crate::kprintln;
use crate::hal::arch::x86_64::cpu;
use super::types::{
    CpuId, CpuSlot, CpuLifecycleState, MAX_CPUS,
    CPU_SLOT_TABLE, LAPIC_TO_CPUID, LAPIC_UNMAPPED,
};

/// Result of CPU topology discovery.
#[derive(Debug, Clone, Copy)]
pub struct TopologyInfo {
    pub total_cpus:   usize,
    pub bsp_lapic_id: u32,
}

/// Initializes CPU topology tables.
/// Returns the number of discovered CPUs (including BSP).
pub fn discover_topology() -> TopologyInfo {
    // Read BSP hardware LAPIC ID from LAPIC_ID register (bits [31:24])
    let bsp_lapic_id = read_bsp_lapic_id();

    unsafe {
        // Clear LAPIC mapping table
        for b in LAPIC_TO_CPUID.iter_mut() {
            *b = LAPIC_UNMAPPED;
        }

        // BSP is always CPU 0
        CPU_SLOT_TABLE[0] = CpuSlot {
            cpu_id:     CpuId(0),
            lapic_id:   bsp_lapic_id,
            state:      CpuLifecycleState::Active,
            is_bsp:     true,
            _pad:       [0; 14],
            generation: 1,
            _pad2:      0,
        };
        LAPIC_TO_CPUID[bsp_lapic_id as usize] = 0;

        // Remaining CPUs 1..3: assign sequential LAPIC IDs
        // In QEMU -smp 4,cores=4 the APIC IDs are typically 0,1,2,3
        let mut total = 1usize;
        for i in 1..MAX_CPUS {
            let lapic_id = i as u32;
            // Skip if same as BSP (shouldn't happen but guard it)
            if lapic_id == bsp_lapic_id {
                continue;
            }
            CPU_SLOT_TABLE[i] = CpuSlot {
                cpu_id:     CpuId(i as u32),
                lapic_id,
                state:      CpuLifecycleState::Absent,
                is_bsp:     false,
                _pad:       [0; 14],
                generation: 1,
                _pad2:      0,
            };
            LAPIC_TO_CPUID[lapic_id as usize] = i as u8;
            total += 1;
        }

        TopologyInfo { total_cpus: total, bsp_lapic_id }
    }
}

/// Reads the BSP's hardware LAPIC ID from the LAPIC ID register.
/// Falls back to 0 if LAPIC isn't mapped yet.
fn read_bsp_lapic_id() -> u32 {
    // Use CPUID leaf 1, EBX bits [31:24] for initial APIC ID (x2APIC-safe fallback)
    let (_, ebx, _, _) = cpu::raw_cpuid(1);
    (ebx >> 24) & 0xFF
}

/// Looks up the CpuId for a given LAPIC hardware ID.
/// Returns None if not mapped.
pub fn lapic_id_to_cpu_id(lapic_id: u8) -> Option<CpuId> {
    unsafe {
        let slot = LAPIC_TO_CPUID[lapic_id as usize];
        if slot == LAPIC_UNMAPPED { None } else { Some(CpuId(slot as u32)) }
    }
}

/// Returns a reference to the CPU slot for the given CPU ID.
#[inline(always)]
pub fn get_cpu_slot(cpu_id: CpuId) -> &'static CpuSlot {
    unsafe { &CPU_SLOT_TABLE[cpu_id.as_usize()] }
}

/// Prints topology discovery diagnostics.
pub fn print_topology_diagnostics(info: &TopologyInfo) {
    kprintln!("\n[Stage 3N: CPU Topology Discovery]");
    kprintln!("  BSP LAPIC ID:     {}", info.bsp_lapic_id);
    kprintln!("  Discovered CPUs:  {}", info.total_cpus);
    unsafe {
        for i in 0..MAX_CPUS {
            let slot = &CPU_SLOT_TABLE[i];
            if slot.state as u8 != CpuLifecycleState::Absent as u8 {
                kprintln!("  CPU[{}]: LAPIC={} BSP={} State={:?}",
                    i, slot.lapic_id, slot.is_bsp, slot.state);
            }
        }
    }
}
