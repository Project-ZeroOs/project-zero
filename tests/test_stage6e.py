"""
Stage 6E Test Suite: Human-Agent Telemetry, Interactive Feedback & Workflow Synthesis Subsystem
Authoritative Contract: Stage 6E Architecture Specification Rev1 & ADR-0036.
Implementation Plan: Stage 6E Implementation Plan Rev1.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `observed` compiles cleanly with zero warnings/errors.
2. Live QEMU Telemetry:
   - All 14 Stage 6E machine verification tests pass (6E-1 through 6E-14).
   - Final summary: "[Stage 6E] ALL 14 TESTS PASSED. Human-Agent Telemetry & Interactive Feedback Subsystem VERIFIED."
   - Clean termination with ISA debug exit code 33 (0x21).
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


class TestStage6eTelemetrySubsystem(unittest.TestCase):
    """Stage 6E Telemetry, Feedback & Workflow Synthesis Subsystem Verification Test Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage6e_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 6E telemetry ABI and data struct declarations."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage6e_observed_compilation(self):
        """Verify observed daemon binary compiles cleanly with Stage 6E handlers."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "observed"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"observed compilation failed:\n{res.stderr}")

    def test_stage6e_kernel_symbols_exist(self):
        """Verify Stage 6E verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage6e_verification", raw_output, "run_stage6e_verification symbol missing from kernel ELF")

    def test_stage6e_qemu_machine_verification(self):
        """Execute QEMU and assert that all 14 Stage 6E machine verification tests pass."""
        markers = [
            "[Stage 6E: Human-Agent Telemetry, Interactive Feedback & Workflow Synthesis Subsystem Verification]",
            "[Test 6E-1: 64-Byte ABI Alignment]: PASS",
            "[Test 6E-2: Telemetry Frame Delivery]: PASS",
            "[Test 6E-3: Feedback Prompt Fail-Closed Timeout]: PASS",
            "[Test 6E-4: Class-3 Security Interception]: PASS",
            "[Test 6E-5: Sensitive Input Scrubbing]: PASS",
            "[Test 6E-6: Proposal Handoff to intentd]: PASS",
            "[Test 6E-7: Workspace Containment Isolation]: PASS",
            "[Test 6E-8: Offline Operation]: PASS",
            "[Test 6E-9: Memory Ring Buffer Accounting]: PASS",
            "[Test 6E-10: Daemon Crash Recovery]: PASS",
            "[Test 6E-11: Synthetic Input Playback Authority Check]: PASS",
            "[Test 6E-12: Zero Capability Grant Verification]: PASS",
            "[Test 6E-13: PMM Neutrality]: PASS",
            "[Test 6E-14: Kernel Preserved]: PASS",
            "[Stage 6E] ALL 14 TESTS PASSED. Human-Agent Telemetry & Interactive Feedback Subsystem VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 6E QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
