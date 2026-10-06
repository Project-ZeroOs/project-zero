"""
WI-03 / zero-term-lib Test Suite: Terminal Application Integration Library
Authoritative Plan: ZEROOS-MVP-IMPLEMENTATION-PLAN-REV2.md (WI-03 / zero-term-lib)

Validations:
1. User-Space Libraries & Daemons Compilation:
   - `libzero` with `zero-term-lib` module compiles cleanly.
   - `workspaced` compiles cleanly.
   - `shelld` compiles cleanly.
2. Kernel Integration:
   - `run_wi03_verification` symbol exists in kernel ELF.
3. Machine Verification (Bare-metal QEMU Telemetry):
   - TERM-A: Terminal application launches.
   - TERM-B: Terminal surface registers successfully.
   - TERM-C: Terminal output is spatially observable.
   - TERM-D: Grounding returns correct terminal context.
   - TERM-E: Cross-workspace grounding is rejected.
   - TERM-F: Sensitive terminal content privacy masking.
   - TERM-G: Input reaches terminal through existing uids/focus enforcement.
   - TERM-H: Terminal/application crash containment.
   - TERM-I: Surface disconnect clean teardown.
   - TERM-J: Offline execution autonomy.
   - TERM-K: Physical Memory Manager (PMM) neutrality (0 frame leaks).
   - TERM-L: Stage 3A–3N production nucleus remains 100% byte-identical.
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


class TestWI03ZeroTermLib(unittest.TestCase):
    """WI-03 / zero-term-lib Application Integration Verification Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_wi03_service_crates_compilation(self):
        """Verify libzero and dependent daemons compile cleanly with zero-term-lib."""
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

    def test_wi03_kernel_symbols_exist(self):
        """Verify WI-03 verification entry point symbol exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_wi03_verification", raw_output, "run_wi03_verification symbol missing from kernel ELF")

    def test_wi03_baremetal_verification(self):
        """Execute QEMU telemetry and assert that all 12 TERM verification criteria pass."""
        markers = [
            "[WI-03: zero-term-lib Terminal Application Integration Library Verification]",
            "[Test TERM-A: Terminal Application Launch]: PASS",
            "[Test TERM-B: Terminal Surface Registration]: PASS",
            "[Test TERM-C: Terminal Output Spatial Observability]: PASS",
            "[Test TERM-D: Spatial Grounding Context Retrieval]: PASS",
            "[Test TERM-E: Cross-Workspace Grounding Rejection]: PASS",
            "[Test TERM-F: Sensitive Content Privacy Masking]: PASS",
            "[Test TERM-G: uids Input Routing Enforcement]: PASS",
            "[Test TERM-H: Application Crash Containment]: PASS",
            "[Test TERM-I: Surface Disconnect Clean Teardown]: PASS",
            "[Test TERM-J: Offline Execution Autonomy]: PASS",
            "[Test TERM-K: PMM Memory Neutrality]: PASS",
            "[Test TERM-L: Stage 3A-3N Nucleus Preservation Audit]: PASS",
            "[WI-03] ALL 12 TESTS PASSED",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"WI-03 QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
