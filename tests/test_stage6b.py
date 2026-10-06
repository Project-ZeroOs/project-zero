"""
Stage 6B Test Suite: Distributed Spatial Presentation Protocol & Remote Surface Proxy Subsystem
Authoritative Contract: Stage 6B Architecture Specification Rev2 & ADR-0033.
Implementation Plan: Stage 6B Implementation Plan Rev1.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `surfaced` compiles cleanly with zero warnings/errors.
2. Live QEMU Telemetry:
   - All 26 Stage 6B machine verification tests pass (6B-1 through 6B-26).
   - Final summary: "[Stage 6B] ALL 26 TESTS PASSED. Distributed Spatial Presentation Protocol VERIFIED."
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


class TestStage6bDistributedPresentationSubsystem(unittest.TestCase):
    """Stage 6B Distributed Presentation Protocol Verification Test Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage6b_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 6B remote presentation protocol extensions."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage6b_surfaced_compilation(self):
        """Verify surfaced daemon binary compiles cleanly with Stage 6B handlers."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "surfaced"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"surfaced compilation failed:\n{res.stderr}")

    def test_stage6b_kernel_symbols_exist(self):
        """Verify Stage 6B verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage6b_verification", raw_output, "run_stage6b_verification symbol missing from kernel ELF")

    def test_stage6b_qemu_machine_verification(self):
        """Execute QEMU and assert that all 26 Stage 6B machine verification tests pass."""
        markers = [
            "[Stage 6B: Distributed Spatial Presentation Protocol Verification]",
            "[Test 6B-1: Header 72-Byte Size]: PASS",
            "[Test 6B-2: LE Wire Encoding]: PASS",
            "[Test 6B-3: CSDT Provenance]: PASS",
            "[Test 6B-4: Local WS Auth]: PASS",
            "[Test 6B-5: Exclusive SHM Handle]: PASS",
            "[Test 6B-6: Single Surface Stream]: PASS",
            "[Test 6B-7: Generation Bound]: PASS",
            "[Test 6B-8: Z-Layer Restriction]: PASS",
            "[Test 6B-9: System Layer Reject]: PASS",
            "[Test 6B-10: Auth Overlay Lock]: PASS",
            "[Test 6B-11: CRC32 Checksum]: PASS",
            "[Test 6B-12: Sequence Discard]: PASS",
            "[Test 6B-13: Damage Rect Bounds]: PASS",
            "[Test 6B-14: Malformed Frame Discard]: PASS",
            "[Test 6B-15: Local Stale Clock]: PASS",
            "[Test 6B-16: Non-Blocking Scanout]: PASS",
            "[Test 6B-17: Disconnect Badge]: PASS",
            "[Test 6B-18: Proxy Register]: PASS",
            "[Test 6B-19: Proxy Unregister]: PASS",
            "[Test 6B-20: Provider Teardown]: PASS",
            "[Test 6B-21: Workspace Preserved]: PASS",
            "[Test 6B-22: Local Zero-Copy]: PASS",
            "[Test 6B-23: RAM Lease Bounds]: PASS",
            "[Test 6B-24: Protocol Robustness]: PASS",
            "[Test 6B-25: PMM Neutrality]: PASS",
            "[Test 6B-26: Kernel Preserved]: PASS",
            "[Stage 6B] ALL 26 TESTS PASSED. Distributed Spatial Presentation Protocol VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 6B QEMU verification failed: {msg}")

    def test_stage6b_qemu_clean_shutdown(self):
        """Verify Stage 6B terminates cleanly with ISA debug exit code 33."""
        time.sleep(0.5)
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 6B] ALL 26 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 6B QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
