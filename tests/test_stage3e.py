"""
Stage 3E Test Suite: Thread Lifecycle & Resource Reclamation
Validations:
1. Static ELF Symbols:
   - `spawn_thread` exists.
   - `reap_zombies` exists.
   - `exit_current_thread_with_code` exists.
   - `signal_locked` exists.
   - `claim_reap_locked` exists.
   - `run_stage3e_verification` exists.
2. QEMU Live Telemetry:
   - Tests A through P verified.
   - Stack Accounting exact match.
   - Stage 3E verified.
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


class TestStage3ELifecycle(unittest.TestCase):
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

    def test_elf_symbols_stage3e(self):
        """Verify authoritative Stage 3E symbols exist in compiled ELF."""
        symbols = self._get_elf_symbols()

        # Check for spawn_thread
        spawn_sym = next((s for s in symbols if "spawn_thread" in s["name"]), None)
        self.assertIsNotNone(spawn_sym, "spawn_thread symbol missing from ELF")
        self.assertEqual(spawn_sym["type"], "FUNC", "spawn_thread must be a function")

        # Check for reap_zombies
        reap_sym = next((s for s in symbols if "reap_zombies" in s["name"]), None)
        self.assertIsNotNone(reap_sym, "reap_zombies symbol missing from ELF")
        self.assertEqual(reap_sym["type"], "FUNC", "reap_zombies must be a function")

        # Check for exit_current_thread_with_code
        exit_code_sym = next((s for s in symbols if "exit_current_thread_with_code" in s["name"]), None)
        self.assertIsNotNone(exit_code_sym, "exit_current_thread_with_code symbol missing from ELF")
        self.assertEqual(exit_code_sym["type"], "FUNC", "exit_current_thread_with_code must be a function")

        # Check for signal_locked
        signal_locked_sym = next((s for s in symbols if "signal_locked" in s["name"]), None)
        self.assertIsNotNone(signal_locked_sym, "signal_locked symbol missing from ELF")
        self.assertEqual(signal_locked_sym["type"], "FUNC", "signal_locked must be a function")

        # Check for claim_reap_locked
        claim_sym = next((s for s in symbols if "claim_reap_locked" in s["name"]), None)
        self.assertIsNotNone(claim_sym, "claim_reap_locked symbol missing from ELF")
        self.assertEqual(claim_sym["type"], "FUNC", "claim_reap_locked must be a function")

        # Check for run_stage3e_verification
        verify_sym = next((s for s in symbols if "run_stage3e_verification" in s["name"]), None)
        self.assertIsNotNone(verify_sym, "run_stage3e_verification symbol missing from ELF")
        self.assertEqual(verify_sym["type"], "FUNC", "run_stage3e_verification must be a function")

    def test_qemu_stage3e_telemetry(self):
        """Execute QEMU and verify Stage 3E Thread Lifecycle & Resource Reclamation telemetry."""
        markers = [
            "[Stage 3E: Thread Lifecycle & Resource Reclamation]",
            "Test A: Synchronous exit & join fast path [VERIFIED]",
            "Test B: Late join pass-through [VERIFIED]",
            "Test C: Early join blocking [VERIFIED]",
            "Test D: Exit code boundary propagation [VERIFIED]",
            "Test E: Self-join rejection [VERIFIED]",
            "Test F: Multiple sequential join operations [VERIFIED]",
            "Test G: Concurrent worker joins [VERIFIED]",
            "Test H: Detached thread reaper scavenging [VERIFIED]",
            "Test I1: Compile-time linear ownership verification [VERIFIED]",
            "Test I2: Runtime race rejection & AlreadyReclaimed proof [VERIFIED]",
            "Test J: Abandoned handle scavenging on parent exit [VERIFIED]",
            "Test K: Slot reuse safety & monotonic ID verification [VERIFIED]",
            "Test L: Single ZOMBIE_QUEUE membership proof [VERIFIED]",
            "Test M: Idle thread scavenger integration [VERIFIED]",
            "Test N: Saved-frame invariant preservation across exit [VERIFIED]",
            "Test O: Non-voluntary preemption during exit preparation [VERIFIED]",
            "Stack Accounting: free frames before=",
            "[EXACT MATCH]",
            "[x] Stage 3E Thread Lifecycle & Resource Reclamation verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3E QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


if __name__ == "__main__":
    unittest.main()
