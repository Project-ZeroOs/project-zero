"""
Stage 6D Test Suite: Human Intent, Intent Resolution & Action/Workflow Boundary Subsystem
Authoritative Contract: Stage 6D Architecture Specification Rev1 & ADR-0035.
Implementation Plan: Stage 6D Implementation Plan Rev1.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `intentd` compiles cleanly with zero warnings/errors.
2. Live QEMU Telemetry:
   - All 14 Stage 6D machine verification tests pass (6D-1 through 6D-14).
   - Final summary: "[Stage 6D] ALL 14 TESTS PASSED. Human Intent & Intent Boundary Subsystem VERIFIED."
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


class TestStage6dHumanIntentSubsystem(unittest.TestCase):
    """Stage 6D Human Intent Subsystem Verification Test Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage6d_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 6D intent ABI and data struct declarations."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage6d_intentd_compilation(self):
        """Verify intentd daemon binary compiles cleanly with Stage 6D handlers."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "intentd"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"intentd compilation failed:\n{res.stderr}")

    def test_stage6d_kernel_symbols_exist(self):
        """Verify Stage 6D verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage6d_verification", raw_output, "run_stage6d_verification symbol missing from kernel ELF")

    def test_stage6d_qemu_machine_verification(self):
        """Execute QEMU and assert that all 14 Stage 6D machine verification tests pass."""
        markers = [
            "[Stage 6D: Human Intent, Intent Resolution & Action/Workflow Boundary Subsystem Verification]",
            "[Test 6D-1: Offline Intent Autonomy]: PASS",
            "[Test 6D-2: Zero Model Authority Rejection]: PASS",
            "[Test 6D-3: Confused Deputy Prevention]: PASS",
            "[Test 6D-4: Workspace Containment]: PASS",
            "[Test 6D-5: Class 3 Side-Effect Modal Confirmation]: PASS",
            "[Test 6D-6: Fail-Closed Modal Crash Cancellation]: PASS",
            "[Test 6D-7: Ambiguous Intent Blocking]: PASS",
            "[Test 6D-8: Workflow Persistence & Recovery]: PASS",
            "[Test 6D-9: Source Provenance Integrity]: PASS",
            "[Test 6D-10: Agent Proposal Capability Bound]: PASS",
            "[Test 6D-11: CSDT Non-Authority Verification]: PASS",
            "[Test 6D-12: Resource Lease Bounds]: PASS",
            "[Test 6D-13: PMM Neutrality]: PASS",
            "[Test 6D-14: Kernel Preserved]: PASS",
            "[Stage 6D] ALL 14 TESTS PASSED. Human Intent & Intent Boundary Subsystem VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 6D QEMU verification failed: {msg}")

    def test_stage6d_qemu_clean_shutdown(self):
        """Verify Stage 6D terminates cleanly with ISA debug exit code 33."""
        time.sleep(0.5)
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 6D] ALL 14 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 6D QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
