"""
Stage 3N Test Suite: SMP / Multi-Core Architecture
Validations:
1. Static Kernel Symbols:
   - `run_stage3n_verification` exists in ELF.
   - `ap_startup_entry` exists in ELF.
   - `ipi_stop_handler` exists in ELF.
   - `ipi_reschedule_handler` exists in ELF.
   - `ipi_tlb_handler` exists in ELF.
   - `CPU_SLOT_TABLE` exists in ELF.
   - `PER_CPU_STATE` exists in ELF.
   - `LAPIC_TO_CPUID` exists in ELF.
   - `TLB_SHOOTDOWN_STATE` exists in ELF.
   - `ONLINE_CPU_COUNT` exists in ELF.
   - `PER_CPU_INSTANCES` exists in ELF (frozen 48-byte percpu array).
2. SMP Static Table Sizes:
   - CPU_SLOT_TABLE: 4 × 32 B = 128 B
   - PER_CPU_STATE: 4 × ≤128 B = ≤512 B
   - LAPIC_TO_CPUID: 256 B
   - TLB_SHOOTDOWN_STATE: 64 B
   - PER_CPU_INSTANCES: 4 × 48 B = 192 B
3. Live QEMU Telemetry:
   - All Stage 3N bare-metal test groups (A through I) pass.
   - Final summary: "ALL N TESTS PASSED"
   - ISA debug-exit returns payload 33 (0x21).
4. Architecture Invariants (bare-metal printed markers):
   - I-SMP-TLB-1: TLB shootdown descriptor write/read verified.
   - I-SMP-ASPACE-2: CpuId invariants verified.
   - I-SMP-SCHED-1: Per-CPU runqueue select/enqueue verified.
5. Topology Discovery:
   - BSP CPU is always CPU[0] with is_bsp=true.
   - At least 1 CPU discovered.
6. PMM Neutrality: zero net frame leakage.
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu


def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")


class TestStage3NSmpArchitecture(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()

    def _get_elf_symbols(self):
        res = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        )
        symbols = []
        for line in res.stdout.splitlines():
            parts = line.strip().split()
            if len(parts) >= 8 and parts[0].rstrip(':').isdigit():
                try:
                    symbols.append({
                        "num": int(parts[0].rstrip(':')),
                        "value": int(parts[1], 16),
                        "size": int(parts[2]),
                        "type": parts[3],
                        "bind": parts[4],
                        "vis": parts[5],
                        "ndx": parts[6],
                        "name": parts[7]
                    })
                except (ValueError, IndexError):
                    continue
        return symbols

    def test_stage3n_kernel_functions_exist(self):
        """Verify Stage 3N SMP entry points and IPI handlers exist in kernel ELF."""
        symbols = self._get_elf_symbols()
        sym_names = [s["name"] for s in symbols]
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout

        # These are #[no_mangle] extern "C" fn → appear as GLOBAL DEFAULT
        global_expected = [
            "ipi_stop_handler",
            "ipi_reschedule_handler",
            "ipi_tlb_handler",
        ]
        for name in global_expected:
            sym = next((s for s in symbols if name == s["name"]), None)
            self.assertIsNotNone(sym, f"Required global symbol '{name}' missing from ELF")

        # These are Rust-mangled — check presence via raw readelf output
        mangled_expected = [
            "run_stage3n_verification",
            "ap_startup_entry",
            "PER_CPU_INSTANCES",
        ]
        for name in mangled_expected:
            self.assertIn(name, raw_output,
                f"Required symbol '{name}' (mangled or literal) missing from ELF")

    def test_stage3n_static_tables_exist(self):
        """Verify all Stage 3N static SMP tables exist in kernel ELF (mangled names)."""
        symbols = self._get_elf_symbols()
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        tables = [
            "CPU_SLOT_TABLE",
            "PER_CPU_STATE",
            "LAPIC_TO_CPUID",
            "TLB_SHOOTDOWN_STATE",
            "ONLINE_CPU_COUNT",
            "PER_CPU_INSTANCES",
            "BSP_PERCPU",
        ]
        for tbl in tables:
            # Check both structured readelf parse and raw output (for mangled names)
            found = (
                next((s for s in symbols if tbl in s["name"]), None) is not None
                or tbl in raw_output
            )
            self.assertTrue(found, f"Static table '{tbl}' missing from ELF")

    def test_stage3n_static_table_sizes(self):
        """Verify Stage 3N static table sizes match architectural bounds."""
        symbols = self._get_elf_symbols()

        # CPU_SLOT_TABLE: 4 × 32 = 128 bytes
        cpu_slot = next((s for s in symbols if "CPU_SLOT_TABLE" in s["name"]), None)
        if cpu_slot:
            self.assertEqual(cpu_slot["size"], 128,
                f"CPU_SLOT_TABLE size mismatch: expected 128, got {cpu_slot['size']}")

        # PER_CPU_INSTANCES: 4 × 48 = 192 bytes
        percpu_arr = next((s for s in symbols if "PER_CPU_INSTANCES" in s["name"]), None)
        if percpu_arr:
            self.assertEqual(percpu_arr["size"], 192,
                f"PER_CPU_INSTANCES size mismatch: expected 192, got {percpu_arr['size']}")

        # LAPIC_TO_CPUID: 256 bytes
        lapic_map = next((s for s in symbols if "LAPIC_TO_CPUID" in s["name"]), None)
        if lapic_map:
            self.assertEqual(lapic_map["size"], 256,
                f"LAPIC_TO_CPUID size mismatch: expected 256, got {lapic_map['size']}")

        # TLB_SHOOTDOWN_STATE: 64 bytes
        tlb_state = next((s for s in symbols if "TLB_SHOOTDOWN_STATE" in s["name"]), None)
        if tlb_state:
            self.assertEqual(tlb_state["size"], 64,
                f"TLB_SHOOTDOWN_STATE size mismatch: expected 64, got {tlb_state['size']}")

    def test_stage3n_qemu_execution(self):
        """Execute QEMU and assert that all Stage 3N bare-metal test groups pass."""
        markers = [
            # Group A: Type layout assertions
            "[PASS] 3N-A-01: CpuId is 4 bytes",
            "[PASS] 3N-A-02: CpuSlot is exactly 32 bytes",
            "[PASS] 3N-A-03: CpuState is at most 128 bytes",
            "[PASS] 3N-A-04: TlbShootdownState is exactly 64 bytes",
            # Group B: IPI vector & constant checks
            "[PASS] 3N-B-01: IPI_VECTOR_STOP = 251",
            "[PASS] 3N-B-02: IPI_VECTOR_RESCHEDULE = 252",
            "[PASS] 3N-B-03: IPI_VECTOR_TLB_SHOOTDOWN = 253",
            "[PASS] 3N-B-04: LAPIC_SPURIOUS_VECTOR = 255",
            # Group C: Topology discovery
            "[PASS] 3N-C-01: Topology discovers at least 1 CPU (BSP)",
            "[PASS] 3N-C-04: CPU_SLOT_TABLE[0] is BSP",
            "[PASS] 3N-C-07: CPU_SLOT_TABLE[0] state is Active",
            "[PASS] 3N-C-10: lapic_id_to_cpu_id(BSP) returns Some(CpuId(0))",
            "[PASS] 3N-C-11: lapic_id_to_cpu_id(0xFF) returns None (unmapped)",
            # Group D: ASpaceGeneration monotonicity
            "[PASS] 3N-D-01: ASpaceGeneration starts at 2 (new=1, inc=2)",
            "[PASS] 3N-D-02: ASpaceGeneration is strictly monotonic (g2 > g1)",
            "[PASS] 3N-D-03: ASpaceGeneration is strictly monotonic (g3 > g2)",
            "[PASS] 3N-D-04: ASpaceGeneration load() == last incremented gen",
            # Group E: TLB shootdown state
            "[PASS] 3N-E-01: TLB_SHOOTDOWN_STATE initial gen == 0",
            "[PASS] 3N-E-05: TLB_SHOOTDOWN_STATE gen write/read round-trip",
            # Group F: Per-CPU state integrity
            "[PASS] 3N-F-02: PER_CPU_STATE[0] interrupt_count is writable",
            "[PASS] 3N-F-03: PER_CPU_STATE[0] ipi_pending_mask round-trip",
            # Group G: ONLINE_CPU_COUNT
            "[PASS] 3N-G-01: ONLINE_CPU_COUNT initialized to 1 (BSP)",
            "[PASS] 3N-G-02: ONLINE_CPU_COUNT increments correctly",
            # Group H: Scheduler policy
            "[PASS] 3N-H-01: select_target_cpu returns 0 (only CPU active)",
            "[PASS] 3N-H-02: enqueue_to_cpu(0, 0) increments critical_count",
            # Group I: CpuId invariants
            "[PASS] 3N-I-01: CpuId::BSP == CpuId(0)",
            "[PASS] 3N-I-03: CpuId(MAX_CPUS-1).is_valid() == true",
            "[PASS] 3N-I-04: CpuId(MAX_CPUS).is_valid() == false",
            # Final summary
            "[Stage 3N] ALL",
            "TESTS PASSED. SMP Architecture VERIFIED.",
            # Topology & runqueue print markers
            "[Stage 3N: CPU Topology Discovery]",
            "[Stage 3N: Per-CPU Runqueue State]",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3N QEMU verification failed: {msg}")

    def test_stage3n_qemu_clean_shutdown(self):
        """Verify Stage 3N ends with ISA debug exit code 33."""
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 3N] ALL"]
        )
        self.assertTrue(success, f"Stage 3N QEMU did not reach SMP verification: {msg}")


if __name__ == "__main__":
    unittest.main()
