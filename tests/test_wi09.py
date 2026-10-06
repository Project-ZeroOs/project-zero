"""
WI-09 Test Suite: init Daemon Process Spawning & Service Supervision
Authoritative Plan: ZEROOS-MVP-IMPLEMENTATION-PLAN-REV2.md (WI-09)

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly.
   - `init` compiles cleanly.
   - `brokerd`, `resourced`, `workspaced`, `intentd`, `groundd`, `surfaced`, `shelld` compile cleanly.
2. Kernel Integration:
   - `run_wi09_verification` exists in kernel ELF.
3. Machine Verification (Bare-metal QEMU Telemetry):
   - WI09-A: `init` starts successfully.
   - WI09-B: Required v1.0 service graph spawned (brokerd, resourced, workspaced, intentd, groundd, surfaced, shelld).
   - WI09-C: Services reach expected Running state.
   - WI09-D: `init` persistent supervision loop remains active.
   - WI09-E: Controlled service failure follows bounded retry policy (DEFAULT_MAX_RETRIES = 3).
   - WI09-F: Physical Memory Manager (PMM) neutrality (0 frame leaks).
   - WI09-G: Stage 3A–3N production nucleus remains byte-identical.
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


class TestWI09InitServiceSupervision(unittest.TestCase):
    """WI-09 init Service Spawning & Supervision Verification Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_wi09_service_crates_compilation(self):
        """Verify init and all 7 v1.0 service binaries compile cleanly."""
        crates = [
            "libzero",
            "init",
            "brokerd",
            "resourced",
            "workspaced",
            "intentd",
            "groundd",
            "surfaced",
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

    def test_wi09_kernel_symbols_exist(self):
        """Verify WI-09 verification entry point symbol exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_wi09_verification", raw_output, "run_wi09_verification symbol missing from kernel ELF")

    def test_wi09_baremetal_verification(self):
        """Execute QEMU telemetry and assert that all 7 WI-09 verification criteria pass."""
        markers = [
            "[WI-09: Init Service Daemon Spawning & Supervision Verification]",
            "[Test WI09-A: Init Supervisor Boot]: PASS",
            "[Test WI09-B: Minimum v1.0 Service Graph Spawning]: PASS",
            "[Test WI09-C: Services Reach Running State]: PASS",
            "[Test WI09-D: Init Persistent Supervision Invariant]: PASS",
            "[Test WI09-E: Controlled Service Failure Retry Policy]: PASS",
            "[Test WI09-F: PMM Memory Neutrality]: PASS",
            "[Test WI09-G: Stage 3A-3N Nucleus Preservation Audit]: PASS",
            "[WI-09] ALL 7 TESTS PASSED",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"WI-09 QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
