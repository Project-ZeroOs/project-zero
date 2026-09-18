"""
Stage 3F Test Suite: Process Model, Address Spaces & Process Lifecycle
Validations:
1. Static ELF Symbols:
   - `create_process` exists.
   - `process_exit` exists.
   - `reap_zombie_processes` exists.
   - `switch_address_space_locked` exists.
   - `cancel_process_threads_locked` exists.
   - `run_stage3f_verification` exists.
2. QEMU Live Telemetry:
   - Tests A through S verified.
   - Stack & PML4 Accounting exact match.
   - Stage 3F verified.
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


class TestStage3FProcess(unittest.TestCase):
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

    def test_elf_symbols_stage3f(self):
        """Verify authoritative Stage 3F symbols exist in compiled ELF."""
        symbols = self._get_elf_symbols()

        # Check for create_process
        create_proc_sym = next((s for s in symbols if "create_process" in s["name"]), None)
        self.assertIsNotNone(create_proc_sym, "create_process symbol missing from ELF")
        self.assertEqual(create_proc_sym["type"], "FUNC", "create_process must be a function")

        # Check for process_exit
        proc_exit_sym = next((s for s in symbols if "process_exit" in s["name"]), None)
        self.assertIsNotNone(proc_exit_sym, "process_exit symbol missing from ELF")
        self.assertEqual(proc_exit_sym["type"], "FUNC", "process_exit must be a function")

        # Check for reap_zombie_processes
        reap_proc_sym = next((s for s in symbols if "reap_zombie_processes" in s["name"]), None)
        self.assertIsNotNone(reap_proc_sym, "reap_zombie_processes symbol missing from ELF")
        self.assertEqual(reap_proc_sym["type"], "FUNC", "reap_zombie_processes must be a function")

        # Check for switch_address_space_locked
        cr3_switch_sym = next((s for s in symbols if "switch_address_space_locked" in s["name"]), None)
        self.assertIsNotNone(cr3_switch_sym, "switch_address_space_locked symbol missing from ELF")
        self.assertEqual(cr3_switch_sym["type"], "FUNC", "switch_address_space_locked must be a function")

        # Check for cancel_process_threads_locked
        cancel_sym = next((s for s in symbols if "cancel_process_threads_locked" in s["name"]), None)
        self.assertIsNotNone(cancel_sym, "cancel_process_threads_locked symbol missing from ELF")
        self.assertEqual(cancel_sym["type"], "FUNC", "cancel_process_threads_locked must be a function")

        # Check for run_stage3f_verification
        verify_sym = next((s for s in symbols if "run_stage3f_verification" in s["name"]), None)
        self.assertIsNotNone(verify_sym, "run_stage3f_verification symbol missing from ELF")
        self.assertEqual(verify_sym["type"], "FUNC", "run_stage3f_verification must be a function")

    def test_qemu_stage3f_telemetry(self):
        """Execute QEMU and verify Stage 3F Process Model & Address Spaces telemetry."""
        markers = [
            "[Stage 3F: Process Model, Address Spaces & Process Lifecycle]",
            "PID 0 (Master Kernel Process) established [VERIFIED]",
            "Test A: Process creation & distinct PML4 [VERIFIED]",
            "Test B: Kernel aperture cloning (USER=0) [VERIFIED]",
            "Test C: User aperture isolation [VERIFIED]",
            "Test D: Same-process CR3 preservation (0 CR3 writes) [VERIFIED]",
            "Test E: Cross-process CR3 switching [VERIFIED]",
            "Test F: Single-thread final exit (Active -> Terminating -> Zombie) [VERIFIED]",
            "Test G: Multi-thread final exit & thread group quiescence [VERIFIED]",
            "Test H: Event waiter cancellation [VERIFIED]",
            "Test I: Mutex waiter & owner cancellation with peer exclusion [VERIFIED]",
            "Test J: Condvar waiter cancellation [VERIFIED]",
            "Test K: SleepTable cancellation [VERIFIED]",
            "Test L: Process termination with mixed thread states [VERIFIED]",
            "Test M: Process join (late join pass-through) [VERIFIED]",
            "Test N: Process reaper scavenging [VERIFIED]",
            "Test O: Parent process abandonment [VERIFIED]",
            "Test P: Zombie process queue exactly-once membership [VERIFIED]",
            "Test Q: Active CR3 reclamation hazard elimination [VERIFIED]",
            "Test R: Monotonic ProcessId & stale-handle protection [VERIFIED]",
            "Stack & PML4 Accounting: free frames before=",
            "[EXACT MATCH]",
            "Test S: Exact PMM frame accounting baseline match [VERIFIED]",
            "Test T: Complete Stage 1-3E regression contract & system invariants [VERIFIED]",
            "[x] Stage 3F Process Model, Address Spaces & Lifecycle verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3F QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


if __name__ == "__main__":
    unittest.main()
