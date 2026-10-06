"""
WI-04 / zero-doc-lib Test Suite: Document & Artifact Integration Library
Authoritative Plan: ZEROOS-MVP-IMPLEMENTATION-PLAN-REV2.md (WI-04 / zero-doc-lib)

Validations:
1. User-Space Libraries & Daemons Compilation:
   - `libzero` with `zero-doc-lib` module compiles cleanly.
   - `workspaced` compiles cleanly.
   - `shelld` compiles cleanly.
2. Kernel Integration:
   - `run_wi04_verification` symbol exists in kernel ELF.
3. Machine Verification (Bare-metal QEMU Telemetry):
   - DOC-A: Artifact producer creates document output.
   - DOC-B: Document content reaches intended workspace context.
   - DOC-C: Presentation surface visibility.
   - DOC-D: Downstream workload capability pipe consumption.
   - DOC-E: Cross-workspace access rejection.
   - DOC-F: Malformed input rejection.
   - DOC-G: Producer crash containment.
   - DOC-H: Persistent artifact context survival.
   - DOC-I: Temporary file prohibition (0 /tmp files created).
   - DOC-J: Offline execution autonomy.
   - DOC-K: Physical Memory Manager (PMM) neutrality (0 frame leaks).
   - DOC-L: Stage 3A–3N production nucleus remains 100% byte-identical.
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


class TestWI04ZeroDocLib(unittest.TestCase):
    """WI-04 / zero-doc-lib Application Integration Verification Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_wi04_service_crates_compilation(self):
        """Verify libzero and dependent daemons compile cleanly with zero-doc-lib."""
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

    def test_wi04_kernel_symbols_exist(self):
        """Verify WI-04 verification entry point symbol exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_wi04_verification", raw_output, "run_wi04_verification symbol missing from kernel ELF")

    def test_wi04_baremetal_verification(self):
        """Execute QEMU telemetry and assert that all 12 DOC verification criteria pass."""
        markers = [
            "[WI-04: zero-doc-lib Document & Artifact Integration Library Verification]",
            "[Test DOC-A: Artifact Producer Creation]: PASS",
            "[Test DOC-B: Document Workspace Context Attachment]: PASS",
            "[Test DOC-C: Presentation Surface Visibility]: PASS",
            "[Test DOC-D: Downstream Workload Capability Pipe Consumption]: PASS",
            "[Test DOC-E: Cross-Workspace Access Rejection]: PASS",
            "[Test DOC-F: Malformed Input Rejection]: PASS",
            "[Test DOC-G: Producer Crash Containment]: PASS",
            "[Test DOC-H: Persistent Artifact Context Survival]: PASS",
            "[Test DOC-I: Temporary File Prohibition (0 /tmp files created)]: PASS",
            "[Test DOC-J: Offline Execution Autonomy]: PASS",
            "[Test DOC-K: PMM Memory Neutrality]: PASS",
            "[Test DOC-L: Stage 3A-3N Nucleus Preservation Audit]: PASS",
            "[WI-04] ALL 12 TESTS PASSED",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"WI-04 QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
