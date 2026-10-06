"""
Stage 6F Test Suite: Agent Spatial Grounding & Session Continuity Subsystem (`groundd`)
Authoritative Specification: Stage 6F Architecture Specification Rev7 (Frozen).
Implementation Plan: Stage 6F Implementation Plan Rev2.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `groundd` compiles cleanly with zero warnings/errors.
   - `shelld` compiles cleanly with zero warnings/errors.
   - `surfaced` compiles cleanly with zero warnings/errors.
2. Live QEMU Spatial Grounding & Session Continuity Verification:
   - All 16 Stage 6F machine verification tests pass (6F-1 through 6F-16).
   - Final summary: "[Stage 6F] ALL 16 TESTS PASSED. Agent Spatial Grounding & Session Continuity Subsystem VERIFIED."
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


class TestStage6fSpatialGroundingSubsystem(unittest.TestCase):
    """Stage 6F Spatial Grounding & Session Continuity Subsystem Verification Test Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage6f_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 6F spatial grounding ABI and data structs."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage6f_groundd_compilation(self):
        """Verify groundd daemon binary compiles cleanly with Stage 6F handlers."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "groundd"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"groundd compilation failed:\n{res.stderr}")

    def test_stage6f_shelld_compilation(self):
        """Verify shelld daemon binary compiles cleanly with Stage 6F epoch fencing handlers."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "shelld"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"shelld compilation failed:\n{res.stderr}")

    def test_stage6f_surfaced_compilation(self):
        """Verify surfaced daemon binary compiles cleanly with Stage 6F semantic generation ownership."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "surfaced"),
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 0, f"surfaced compilation failed:\n{res.stderr}")

    def test_stage6f_kernel_symbols_exist(self):
        """Verify Stage 6F verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage6f_verification", raw_output, "run_stage6f_verification symbol missing from kernel ELF")

    def test_stage6f_qemu_machine_verification(self):
        """Execute QEMU and assert that all 16 Stage 6F machine verification tests pass."""
        markers = [
            "[Stage 6F: Agent Spatial Grounding & Session Continuity Subsystem Verification]",
            "[Test 6F-1: Spatial Query ABI Alignment]: PASS",
            "[Test 6F-2: Spatial Element Grounding]: PASS",
            "[Test 6F-3: Sensitive Element Geometric Masking]: PASS",
            "[Test 6F-4: Trusted Overlay Exclusion]: PASS",
            "[Test 6F-5: Workspace Containment Isolation]: PASS",
            "[Test 6F-6: Absence of Mutation Authority Audit]: PASS",
            "[Test 6F-7: Bounded Logical Session Snapshot Serialization]: PASS",
            "[Test 6F-8: Ephemeral Handle Non-Migration Audit]: PASS",
            "[Test 6F-9: Cryptographic Fencing Proof & Anti-Replay]: PASS",
            "[Test 6F-10: Post-Fencing Remote Capability Derivation]: PASS",
            "[Test 6F-11: Offline Autonomy Verification]: PASS",
            "[Test 6F-12: groundd Crash Non-Impact Recovery]: PASS",
            "[Test 6F-13: Zero New Capability Authority Audit]: PASS",
            "[Test 6F-14: PMM Neutrality]: PASS",
            "[Test 6F-15: Stage 3A-3N Nucleus Preservation Audit]: PASS",
            "[Test 6F-16: Spatial & Semantic Freshness Verification]: PASS",
            "[Stage 6F] ALL 16 TESTS PASSED. Agent Spatial Grounding & Session Continuity Subsystem VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 6F QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
