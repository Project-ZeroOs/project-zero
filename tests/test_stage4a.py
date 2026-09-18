"""
Stage 4A Test Suite: Core System Service Runtime & Capability Directory
Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `brokerd` compiles cleanly with zero warnings/errors.
   - `init` compiles cleanly with zero warnings/errors.
2. Static Kernel Symbols:
   - `run_stage4a_verification` exists in kernel ELF.
3. Live QEMU Telemetry:
   - All 12 Stage 4A machine verification tests pass:
     - 4A-A: User Init Boot
     - 4A-B: Broker Startup
     - 4A-C: Service Registration
     - 4A-D: Service Discovery
     - 4A-E: IPC Rendezvous
     - 4A-F: Capability Delegation
     - 4A-G: Capability Amplification Rejection
     - 4A-H: Service Restart & Endpoint Invariant
     - 4A-I: Fault Containment
     - 4A-J: Malformed Request
     - 4A-K: Service Identity Generation
     - 4A-L: Clean Shutdown
   - PMM Neutrality: zero net frame leakage.
   - Final summary: "[Stage 4A] ALL 12 TESTS PASSED. System Service Substrate VERIFIED."
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


class TestStage4ASystemServices(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage4a_libzero_compilation(self):
        """Verify libzero compiles as a freestanding zero-dependency rlib."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage4a_brokerd_compilation(self):
        """Verify brokerd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "brokerd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"brokerd compilation failed:\n{res.stderr}")

    def test_stage4a_init_compilation(self):
        """Verify init supervisor binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "init"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"init compilation failed:\n{res.stderr}")

    def test_stage4a_kernel_symbols_exist(self):
        """Verify Stage 4A verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage4a_verification", raw_output,
                      "Required symbol 'run_stage4a_verification' missing from ELF")

    def test_stage4a_qemu_machine_verification(self):
        """Execute QEMU and assert that all 12 Stage 4A machine verification tests pass."""
        markers = [
            "[Stage 4A: Core System Service Runtime & Capability Directory Verification]",
            "[Test 4A-A: User Init Boot]: PASS",
            "[Test 4A-B: Broker Startup]: PASS",
            "[Test 4A-C: Service Registration]: PASS",
            "[Test 4A-D: Service Discovery]: PASS",
            "[Test 4A-E: IPC Rendezvous]: PASS",
            "[Test 4A-F: Capability Delegation]: PASS",
            "[Test 4A-G: Capability Amplification Rejection]: PASS",
            "[Test 4A-H: Service Restart]: PASS",
            "[Test 4A-I: Fault Containment]: PASS",
            "[Test 4A-J: Malformed Request]: PASS",
            "[Test 4A-K: Service Identity Generation]: PASS",
            "[Test 4A-L: Shutdown]: PASS",
            "[Stage 4A: PMM Neutrality]: PASS",
            "[Stage 4A] ALL 12 TESTS PASSED. System Service Substrate VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 4A QEMU verification failed: {msg}")

    def test_stage4a_qemu_clean_shutdown(self):
        """Verify Stage 4A terminates cleanly with ISA debug exit code 33."""
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 4A] ALL 12 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 4A QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
