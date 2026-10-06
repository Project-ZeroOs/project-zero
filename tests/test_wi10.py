"""
WI-10 Test Suite: VFS-Backed Session Snapshot Persistence
Authoritative Plan: ZEROOS-MVP-IMPLEMENTATION-PLAN-REV2.md (WI-10)

Validations:
1. User-Space Libraries & Daemons Compilation:
   - `libzero` compiles cleanly with VFS session snapshot journal.
   - `workspaced` compiles cleanly.
   - `shelld` compiles cleanly.
2. Kernel Integration:
   - `run_wi10_verification` symbol exists in kernel ELF.
3. Machine Verification (Bare-metal QEMU Telemetry):
   - WI10-A: Normal Snapshot Write & Recovery across simulated restart.
   - WI10-B: Multiple Consecutive Snapshots (latest valid restored).
   - WI10-C: Absent Snapshot Clean Handling.
   - WI10-D: Corrupted Snapshot Safe Rejection.
   - WI10-E: Interrupted Partial Write Rejection.
   - WI10-F: Hard Reset Checkpoint Recovery.
   - WI10-G: Physical Memory Manager (PMM) neutrality (0 frame leaks).
   - WI10-H: Stage 3A–3N production nucleus remains byte-identical.
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


class TestWI10VfsSessionPersistence(unittest.TestCase):
    """WI-10 VFS Session Snapshot Persistence Verification Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_wi10_service_crates_compilation(self):
        """Verify libzero and persistence-enabled daemons compile cleanly."""
        crates = [
            "libzero",
            "workspaced",
            "shelld",
        ]
        for crate in crates:
            res = subprocess.run(
                [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
                cwd=str(PROJECT_ROOT / crate),
                capture_output=True,
                text=True,
            )
            self.assertEqual(res.returncode, 0, f"Crate {crate} compilation failed:\n{res.stderr}")

    def test_wi10_kernel_symbols_exist(self):
        """Verify WI-10 verification entry point symbol exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_wi10_verification", raw_output, "run_wi10_verification symbol missing from kernel ELF")

    def test_wi10_baremetal_verification(self):
        """Execute QEMU telemetry and assert that all 8 WI-10 verification criteria pass."""
        markers = [
            "[WI-10: VFS-Backed Session Snapshot Persistence Verification]",
            "[Test WI10-C: Absent Snapshot Clean Handling]: PASS",
            "[Test WI10-A: Normal Snapshot Write & Recovery]: PASS",
            "[Test WI10-B: Multiple Consecutive Snapshots Latest Restored]: PASS",
            "[Test WI10-D: Corrupted Snapshot Safe Rejection]: PASS",
            "[Test WI10-E: Interrupted Partial Write Rejection]: PASS",
            "[Test WI10-F: Hard Reset Checkpoint Recovery]: PASS",
            "[Test WI10-G: PMM Memory Neutrality]: PASS",
            "[Test WI10-H: Stage 3A-3N Nucleus Preservation Audit]: PASS",
            "[WI-10] ALL 8 TESTS PASSED",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"WI-10 QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
