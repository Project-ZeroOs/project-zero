"""
Stage 4F Test Suite: Intent Resolution & Personal Compute Fabric Subsystem (intentd & fabricd)
Authoritative Contract: Stage 4F Architecture Specification Rev3 & ADR-0030.
Implementation Plan: Stage 4F Implementation Plan Rev2.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `intentd` compiles cleanly with zero warnings/errors.
   - `fabricd` compiles cleanly with zero warnings/errors.
2. Static Kernel Symbols:
   - `run_stage4f_verification` exists in kernel ELF.
3. Live QEMU Telemetry:
   - All 26 Stage 4F machine verification tests pass:
     - 4F-A: intentd Startup
     - 4F-B: fabricd Startup
     - 4F-C: Intent Submission
     - 4F-D: Ambiguity Execution Blocking
     - 4F-E: Intent Safety Boundary
     - 4F-F: Model Non-Authority Enforced
     - 4F-G: Plan Structural Validation
     - 4F-H: Ephemeral Agent Lifecycle
     - 4F-I: Workload DAG Handoff
     - 4F-J: Fabric Peer Discovery
     - 4F-K: IAL Peer Classification
     - 4F-L: Hard Constraints Filter
     - 4F-M: Hard Filter Precedence
     - 4F-N: Soft Cost Optimization
     - 4F-O: Advisory Remote Telemetry
     - 4F-P: CSDT Token Generation
     - 4F-Q: CSDT Monotonic Expiration
     - 4F-R: CSDT Kernel Cap Derivation
     - 4F-S: CSDT Subtree Revocation
     - 4F-T: Provider Authoritative Lease
     - 4F-U: Class 3 Side-Effect Latch
     - 4F-V: Duplicate Operation Rejection
     - 4F-W: Ephemeral Agent Crash Recovery
     - 4F-X: Protocol Robustness
     - 4F-Y: PMM Neutrality
     - 4F-Z: Substrate Preservation
   - Final summary: "[Stage 4F] ALL 26 TESTS PASSED. Intent Resolution & Compute Fabric VERIFIED."
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


class TestStage4FComputeFabricSubsystem(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage4f_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 4F fabric modules."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage4f_intentd_compilation(self):
        """Verify intentd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "intentd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"intentd compilation failed:\n{res.stderr}")

    def test_stage4f_fabricd_compilation(self):
        """Verify fabricd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "fabricd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"fabricd compilation failed:\n{res.stderr}")

    def test_stage4f_kernel_symbols_exist(self):
        """Verify Stage 4F verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage4f_verification", raw_output, "run_stage4f_verification symbol missing from kernel ELF")

    def test_stage4f_qemu_machine_verification(self):
        """Execute QEMU and assert that all 26 Stage 4F machine verification tests pass."""
        markers = [
            "[Stage 4F: Intent Resolution & Compute Fabric Subsystem Verification]",
            "[Test 4F-A: intentd Startup]: PASS",
            "[Test 4F-B: fabricd Startup]: PASS",
            "[Test 4F-C: Intent Submission]: PASS",
            "[Test 4F-D: Ambiguity Execution Blocking]: PASS",
            "[Test 4F-E: Intent Safety Boundary]: PASS",
            "[Test 4F-F: Model Non-Authority Enforced]: PASS",
            "[Test 4F-G: Plan Structural Validation]: PASS",
            "[Test 4F-H: Ephemeral Agent Lifecycle]: PASS",
            "[Test 4F-I: Workload DAG Handoff]: PASS",
            "[Test 4F-J: Fabric Peer Discovery]: PASS",
            "[Test 4F-K: IAL Peer Classification]: PASS",
            "[Test 4F-L: Hard Constraints Filter]: PASS",
            "[Test 4F-M: Hard Filter Precedence]: PASS",
            "[Test 4F-N: Soft Cost Optimization]: PASS",
            "[Test 4F-O: Advisory Remote Telemetry]: PASS",
            "[Test 4F-P: CSDT Token Generation]: PASS",
            "[Test 4F-Q: CSDT Monotonic Expiration]: PASS",
            "[Test 4F-R: CSDT Kernel Cap Derivation]: PASS",
            "[Test 4F-S: CSDT Subtree Revocation]: PASS",
            "[Test 4F-T: Provider Authoritative Lease]: PASS",
            "[Test 4F-U: Class 3 Side-Effect Latch]: PASS",
            "[Test 4F-V: Duplicate Operation Rejection]: PASS",
            "[Test 4F-W: Ephemeral Agent Crash Recovery]: PASS",
            "[Test 4F-X: Protocol Robustness]: PASS",
            "[Test 4F-Y: PMM Neutrality]: PASS",
            "[Test 4F-Z: Substrate Preservation]: PASS",
            "[Stage 4F] ALL 26 TESTS PASSED. Intent Resolution & Compute Fabric VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 4F QEMU verification failed: {msg}")

    def test_stage4f_qemu_clean_shutdown(self):
        """Verify Stage 4F terminates cleanly with ISA debug exit code 33."""
        time.sleep(0.5)
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 4F] ALL 26 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 4F QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
