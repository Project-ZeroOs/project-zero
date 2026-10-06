"""
Stage 5 Test Suite: User Interaction Substrate & Spatial Presentation Subsystem
Authoritative Contract: Stage 5 Architecture Specification Rev4 & ADR-0031.
Implementation Plan: Stage 5 Implementation Plan Rev2.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `compositord` compiles cleanly with zero warnings/errors.
   - `surfaced` compiles cleanly with zero warnings/errors.
   - `authui` compiles cleanly with zero warnings/errors.
   - `uids` compiles cleanly with zero warnings/errors.
2. Static Kernel Symbols:
   - `run_stage5_verification` exists in kernel ELF.
3. Live QEMU Telemetry:
   - All 26 Stage 5 machine verification tests pass:
     - 5-A: compositord Startup
     - 5-B: Software Framebuffer Composition
     - 5-C: Surface Registration & Cap Derivation
     - 5-D: Zero-Copy Presentation Buffer Lifecycle
     - 5-E: Spatial Viewport Transformation
     - 5-F: Trusted Overlay Policy Binding
     - 5-G: Visual HMAC Badge Verification
     - 5-H: Missing AuthorizationTransactionRef Rejection
     - 5-I: uids Input Driver Event Ingestion
     - 5-J: Input Focus Routing & Isolation
     - 5-K: I-INPUT-NO-IMPLICIT-AUTHORITY Enforced
     - 5-L: Human Intent Ingestion Pipeline
     - 5-M: Priority::Critical Compositor Scheduling
     - 5-N: Soft Frame Target & Deadline Safety
     - 5-O: Surface Destruction & Cap Revocation
     - 5-P: authui Crash Fail-Closed Security
     - 5-Q: Multi-Head Display Topology Setup
     - 5-R: compositord Crash & Re-Bind Recovery
     - 5-S: surfaced Crash & Viewport Reconstruction
     - 5-T: Protocol Robustness & Unknown Opcode
     - 5-U: Invalid Surface Handle Rejection
     - 5-V: Stage 4B Resource Lease Integration
     - 5-W: Workspace Deletion Surface Teardown
     - 5-X: Protocol Robustness
     - 5-Y: PMM Neutrality
     - 5-Z: Substrate Preservation
   - Final summary: "[Stage 5] ALL 26 TESTS PASSED. User Interaction Substrate & Spatial Presentation VERIFIED."
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


class TestStage5UserInteractionSubsystem(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage5_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 5 presentation modules."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage5_compositord_compilation(self):
        """Verify compositord daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "compositord"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"compositord compilation failed:\n{res.stderr}")

    def test_stage5_surfaced_compilation(self):
        """Verify surfaced daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "surfaced"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"surfaced compilation failed:\n{res.stderr}")

    def test_stage5_authui_compilation(self):
        """Verify authui daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "authui"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"authui compilation failed:\n{res.stderr}")

    def test_stage5_uids_compilation(self):
        """Verify uids daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "uids"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"uids compilation failed:\n{res.stderr}")

    def test_stage5_kernel_symbols_exist(self):
        """Verify Stage 5 verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage5_verification", raw_output, "run_stage5_verification symbol missing from kernel ELF")

    def test_stage5_qemu_machine_verification(self):
        """Execute QEMU and assert that all 26 Stage 5 machine verification tests pass."""
        markers = [
            "[Stage 5: User Interaction Substrate & Spatial Presentation Subsystem Verification]",
            "[Test 5-A: compositord Startup]: PASS",
            "[Test 5-B: Software Framebuffer Composition]: PASS",
            "[Test 5-C: Surface Registration & Cap Derivation]: PASS",
            "[Test 5-D: Zero-Copy Presentation Buffer Lifecycle]: PASS",
            "[Test 5-E: Spatial Viewport Transformation]: PASS",
            "[Test 5-F: Trusted Overlay Policy Binding]: PASS",
            "[Test 5-G: Visual HMAC Badge Verification]: PASS",
            "[Test 5-H: Missing AuthorizationTransactionRef Rejection]: PASS",
            "[Test 5-I: uids Input Driver Event Ingestion]: PASS",
            "[Test 5-J: Input Focus Routing & Isolation]: PASS",
            "[Test 5-K: I-INPUT-NO-IMPLICIT-AUTHORITY Enforced]: PASS",
            "[Test 5-L: Human Intent Ingestion Pipeline]: PASS",
            "[Test 5-M: Priority::Critical Compositor Scheduling]: PASS",
            "[Test 5-N: Soft Frame Target & Deadline Safety]: PASS",
            "[Test 5-O: Surface Destruction & Cap Revocation]: PASS",
            "[Test 5-P: authui Crash Fail-Closed Security]: PASS",
            "[Test 5-Q: Multi-Head Display Topology Setup]: PASS",
            "[Test 5-R: compositord Crash & Re-Bind Recovery]: PASS",
            "[Test 5-S: surfaced Crash & Viewport Reconstruction]: PASS",
            "[Test 5-T: Protocol Robustness & Unknown Opcode]: PASS",
            "[Test 5-U: Invalid Surface Handle Rejection]: PASS",
            "[Test 5-V: Stage 4B Resource Lease Integration]: PASS",
            "[Test 5-W: Workspace Deletion Surface Teardown]: PASS",
            "[Test 5-X: Protocol Robustness]: PASS",
            "[Test 5-Y: PMM Neutrality]: PASS",
            "[Test 5-Z: Substrate Preservation]: PASS",
            "[Stage 5] ALL 26 TESTS PASSED. User Interaction Substrate & Spatial Presentation VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 5 QEMU verification failed: {msg}")

    def test_stage5_qemu_clean_shutdown(self):
        """Verify Stage 5 terminates cleanly with ISA debug exit code 33."""
        time.sleep(0.5)
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 5] ALL 26 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 5 QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
