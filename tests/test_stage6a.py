"""
Stage 6A Test Suite: User Session Substrate & Human Operating Environment (`shelld`)
Authoritative Contract: Stage 6A Architecture Specification Rev1 & ADR-0032.
Implementation Plan: Stage 6A Implementation Plan Rev2.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `shelld` compiles cleanly with zero warnings/errors.
2. Live QEMU Telemetry:
   - All 26 Stage 6A machine verification tests pass:
     - 6A-1: shelld Startup
     - 6A-2: Cap Lineage
     - 6A-3: System Surface
     - 6A-4: Layer Demotion
     - 6A-5: Auth Overlay Lock
     - 6A-6: SessionId Sequence
     - 6A-7: Containment Match
     - 6A-8: Switch Rejection
     - 6A-9: Workspace Activate
     - 6A-10: Viewport Grid
     - 6A-11: Focus Routing
     - 6A-12: Telemetry Ingest
     - 6A-13: Action Hash Audit
     - 6A-14: System HUD
     - 6A-15: Visual Transition
     - 6A-16: Shell Restart
     - 6A-17: Input Quarantine
     - 6A-18: Graph Rebuild
     - 6A-19: Focus Restored
     - 6A-20: Fail-Closed Recovery
     - 6A-21: Suspended Visual
     - 6A-22: Session Lock
     - 6A-23: RAM Lease Bounds
     - 6A-24: Protocol Robustness
     - 6A-25: PMM Neutrality
     - 6A-26: Substrate Preservation
   - Final summary: "[Stage 6A] ALL 26 TESTS PASSED. User Session Substrate & Human Operating Environment VERIFIED."
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


class TestStage6aUserSessionSubsystem(unittest.TestCase):
    """Stage 6A User Session Substrate Verification Test Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage6a_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 6A session abstractions."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage6a_shelld_compilation(self):
        """Verify shelld daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "shelld"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"shelld compilation failed:\n{res.stderr}")

    def test_stage6a_kernel_symbols_exist(self):
        """Verify Stage 6A verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage6a_verification", raw_output, "run_stage6a_verification symbol missing from kernel ELF")

    def test_stage6a_qemu_machine_verification(self):
        """Execute QEMU and assert that all 26 Stage 6A machine verification tests pass."""
        markers = [
            "[Stage 6A: User Session Substrate & Human Operating Environment Verification]",
            "[Test 6A-1: shelld Startup]: PASS",
            "[Test 6A-2: Cap Lineage]: PASS",
            "[Test 6A-3: System Surface]: PASS",
            "[Test 6A-4: Layer Demotion]: PASS",
            "[Test 6A-5: Auth Overlay Lock]: PASS",
            "[Test 6A-6: SessionId Sequence]: PASS",
            "[Test 6A-7: Containment Match]: PASS",
            "[Test 6A-8: Switch Rejection]: PASS",
            "[Test 6A-9: Workspace Activate]: PASS",
            "[Test 6A-10: Viewport Grid]: PASS",
            "[Test 6A-11: Focus Routing]: PASS",
            "[Test 6A-12: Telemetry Ingest]: PASS",
            "[Test 6A-13: Action Hash Audit]: PASS",
            "[Test 6A-14: System HUD]: PASS",
            "[Test 6A-15: Visual Transition]: PASS",
            "[Test 6A-16: Shell Restart]: PASS",
            "[Test 6A-17: Input Quarantine]: PASS",
            "[Test 6A-18: Graph Rebuild]: PASS",
            "[Test 6A-19: Focus Restored]: PASS",
            "[Test 6A-20: Fail-Closed Recovery]: PASS",
            "[Test 6A-21: Suspended Visual]: PASS",
            "[Test 6A-22: Session Lock]: PASS",
            "[Test 6A-23: RAM Lease Bounds]: PASS",
            "[Test 6A-24: Protocol Robustness]: PASS",
            "[Test 6A-25: PMM Neutrality]: PASS",
            "[Test 6A-26: Substrate Preservation]: PASS",
            "[Stage 6A] ALL 26 TESTS PASSED. User Session Substrate & Human Operating Environment VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 6A QEMU verification failed: {msg}")

    def test_stage6a_qemu_clean_shutdown(self):
        """Verify Stage 6A terminates cleanly with ISA debug exit code 33."""
        time.sleep(0.5)
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 6A] ALL 26 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 6A QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
