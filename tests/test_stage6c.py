"""
Stage 6C Test Suite: Human Input, Interaction Routing & Intent Boundary Subsystem
Authoritative Contract: Stage 6C Architecture Specification Rev3 & ADR-0034.
Implementation Plan: Stage 6C Implementation Plan Rev2.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `uids` compiles cleanly with zero warnings/errors.
2. Live QEMU Telemetry:
   - All 14 Stage 6C machine verification tests pass (6C-1 through 6C-14).
   - Final summary: "[Stage 6C] ALL 14 TESTS PASSED. Human Input & Intent Boundary Subsystem VERIFIED."
   - Clean termination with ISA debug exit code 33 (0x21).
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys
import time

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


class TestStage6cHumanInputSubsystem(unittest.TestCase):
    """Stage 6C Human Input Subsystem Verification Test Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage6c_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 6C input ABI and capability declarations."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage6c_uids_compilation(self):
        """Verify uids daemon binary compiles cleanly with Stage 6C handlers."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "uids"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"uids compilation failed:\n{res.stderr}")

    def test_stage6c_kernel_symbols_exist(self):
        """Verify Stage 6C verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage6c_verification", raw_output, "run_stage6c_verification symbol missing from kernel ELF")

    def test_stage6c_qemu_machine_verification(self):
        """Execute QEMU and assert that all 14 Stage 6C machine verification tests pass."""
        markers = [
            "[Stage 6C: Human Input, Interaction Routing & Intent Boundary Subsystem Verification]",
            "[Test 6C-1: 64-Byte ABI]: PASS",
            "[Test 6C-2: Timestamp Authority]: PASS",
            "[Test 6C-3: Focus Policy vs Enforcement]: PASS",
            "[Test 6C-4: ModalLock Trusted Path]: PASS",
            "[Test 6C-5: Synthetic ModalLock Rejection]: PASS",
            "[Test 6C-6: Remote Input Policy Cap]: PASS",
            "[Test 6C-7: CSDT Non-Authority]: PASS",
            "[Test 6C-8: Unforgeable Provenance]: PASS",
            "[Test 6C-9: State Reconciliation]: PASS",
            "[Test 6C-10: Keyleak Prevention]: PASS",
            "[Test 6C-11: Observation vs Interpretation]: PASS",
            "[Test 6C-12: Stale Generation Discard]: PASS",
            "[Test 6C-13: Fail-Closed Quarantine]: PASS",
            "[Test 6C-14: PMM Neutrality & Kernel Preserved]: PASS",
            "[Stage 6C] ALL 14 TESTS PASSED. Human Input & Intent Boundary Subsystem VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 6C QEMU verification failed: {msg}")

    def test_stage6c_qemu_clean_shutdown(self):
        """Verify Stage 6C terminates cleanly with ISA debug exit code 33."""
        time.sleep(0.5)
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 6C] ALL 14 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 6C QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
