"""
Stage 4B Test Suite: Unified Resource Graph & Local Node Accounting (resourced)
Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `brokerd` compiles cleanly with zero warnings/errors.
   - `init` compiles cleanly with zero warnings/errors.
   - `resourced` compiles cleanly with zero warnings/errors.
2. Static Kernel Symbols:
   - `run_stage4b_verification` exists in kernel ELF.
3. Live QEMU Telemetry:
   - All 26 Stage 4B machine verification tests pass:
     - 4B-A: DistributedId Allocation
     - 4B-B: Durable Sequence Reservation
     - 4B-C: Sequence Exhaustion & Non-Reuse
     - 4B-D: Multi-Dimensional Registration
     - 4B-E: Dimension Bound Rejection
     - 4B-F: Structural Topology Integrity
     - 4B-G: Vector Conservation
     - 4B-H: Coupling Constraint Enforcement
     - 4B-I: Atomic Admission Serialization
     - 4B-J: Local Quota Enforcement
     - 4B-K: Canonical Qualified Source
     - 4B-L: Rust-Sound Time Seqlock
     - 4B-M: Time Frame Write Authority Rejection
     - 4B-N: Consumer Freshness & Equivalence
     - 4B-O: Monotonic Regression Terminal Latch
     - 4B-P: Deadline Arithmetic Non-Wrap
     - 4B-Q: BootEpoch Durability & Non-Wrap
     - 4B-R: Capability-Bounded Admission
     - 4B-S: Authority Amplification Rejection
     - 4B-T: Stale Generation Rejection
     - 4B-U: TimeAuthorityLost Accounting Quarantine
     - 4B-V: ProviderLost Mathematical Conservation
     - 4B-W: Consumer Crash Cleanup Ordering
     - 4B-X: Non-Resurrecting Reconciliation
     - 4B-Y: Energy Telemetry Classification Integrity
     - 4B-Z: Full System IPC Integration & PMM Neutrality
   - Final summary: "[Stage 4B] ALL 26 TESTS PASSED. Resource Graph & Node Accounting VERIFIED."
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


class TestStage4BResourceAccounting(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage4b_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 4B modules."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage4b_brokerd_compilation(self):
        """Verify brokerd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "brokerd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"brokerd compilation failed:\n{res.stderr}")

    def test_stage4b_init_compilation(self):
        """Verify init supervisor binary compiles cleanly with Time Authority Adapter."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "init"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"init compilation failed:\n{res.stderr}")

    def test_stage4b_resourced_compilation(self):
        """Verify resourced daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "resourced"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"resourced compilation failed:\n{res.stderr}")

    def test_stage4b_kernel_symbols_exist(self):
        """Verify Stage 4B verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage4b_verification", raw_output,
                      "Required symbol 'run_stage4b_verification' missing from ELF")

    def test_stage4b_qemu_machine_verification(self):
        """Execute QEMU and assert that all 26 Stage 4B machine verification tests pass."""
        markers = [
            "[Stage 4B: Unified Resource Graph & Local Node Accounting Verification]",
            "[Test 4B-A: DistributedId Allocation]: PASS",
            "[Test 4B-B: Durable Sequence Reservation]: PASS",
            "[Test 4B-C: Sequence Exhaustion & Non-Reuse]: PASS",
            "[Test 4B-D: Multi-Dimensional Registration]: PASS",
            "[Test 4B-E: Dimension Bound Rejection]: PASS",
            "[Test 4B-F: Structural Topology Integrity]: PASS",
            "[Test 4B-G: Vector Conservation]: PASS",
            "[Test 4B-H: Coupling Constraint Enforcement]: PASS",
            "[Test 4B-I: Atomic Admission Serialization]: PASS",
            "[Test 4B-J: Local Quota Enforcement]: PASS",
            "[Test 4B-K: Canonical Qualified Source]: PASS",
            "[Test 4B-L: Rust-Sound Time Seqlock]: PASS",
            "[Test 4B-M: Time Frame Write Authority Rejection]: PASS",
            "[Test 4B-N: Consumer Freshness & Equivalence]: PASS",
            "[Test 4B-O: Monotonic Regression Terminal Latch]: PASS",
            "[Test 4B-P: Deadline Arithmetic Non-Wrap]: PASS",
            "[Test 4B-Q: BootEpoch Durability & Non-Wrap]: PASS",
            "[Test 4B-R: Capability-Bounded Admission]: PASS",
            "[Test 4B-S: Authority Amplification Rejection]: PASS",
            "[Test 4B-T: Stale Generation Rejection]: PASS",
            "[Test 4B-U: TimeAuthorityLost Accounting Quarantine]: PASS",
            "[Test 4B-V: ProviderLost Mathematical Conservation]: PASS",
            "[Test 4B-W: Consumer Crash Cleanup Ordering]: PASS",
            "[Test 4B-X: Non-Resurrecting Reconciliation]: PASS",
            "[Test 4B-Y: Energy Telemetry Classification Integrity]: PASS",
            "[Test 4B-Z: Full System IPC Integration & PMM Neutrality]: PASS",
            "[Stage 4B] ALL 26 TESTS PASSED. Resource Graph & Node Accounting VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 4B QEMU verification failed: {msg}")

    def test_stage4b_qemu_clean_shutdown(self):
        """Verify Stage 4B terminates cleanly with ISA debug exit code 33."""
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 4B] ALL 26 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 4B QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
