"""
WI-02 / zero-exec-lib Test Suite: CLI Workload Application Integration Library
Authoritative Plan: ZEROOS-MVP-IMPLEMENTATION-PLAN-REV2.md (WI-02 / zero-exec-lib)

Validations:
1. User-Space Libraries & Daemons Compilation:
   - `libzero` with `zero-exec-lib` module compiles cleanly.
   - `workspaced` compiles cleanly.
   - `shelld` compiles cleanly.
2. Kernel Integration:
   - `run_wi02_verification` symbol exists in kernel ELF.
3. Machine Verification (Bare-metal QEMU Telemetry):
   - EXEC-A: Successful process execution.
   - EXEC-B: stdin -> process -> stdout capability pipe.
   - EXEC-C: stderr stream propagation.
   - EXEC-D: EOF stream signal propagation.
   - EXEC-E: Non-zero process exit status.
   - EXEC-F: Workload cancellation process termination.
   - EXEC-G: Capability authorization rejection (zero capability handle).
   - EXEC-H: Workspace containment boundary enforcement.
   - EXEC-I: Temporary file prohibition (0 /tmp files created).
   - EXEC-J: Offline execution autonomy.
   - EXEC-K: Physical Memory Manager (PMM) neutrality (0 frame leaks).
   - EXEC-L: Stage 3A–3N production nucleus remains 100% byte-identical.
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


class TestWI02ZeroExecLib(unittest.TestCase):
    """WI-02 / zero-exec-lib Application Integration Verification Suite."""

    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_wi02_service_crates_compilation(self):
        """Verify libzero and dependent daemons compile cleanly with zero-exec-lib."""
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

    def test_wi02_kernel_symbols_exist(self):
        """Verify WI-02 verification entry point symbol exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_wi02_verification", raw_output, "run_wi02_verification symbol missing from kernel ELF")

    def test_wi02_baremetal_verification(self):
        """Execute QEMU telemetry and assert that all 12 EXEC verification criteria pass."""
        markers = [
            "[WI-02: zero-exec-lib Application Integration Library Verification]",
            "[Test EXEC-A: Successful Process Execution]: PASS",
            "[Test EXEC-B: stdin -> process -> stdout Capability Pipe]: PASS",
            "[Test EXEC-C: stderr Stream Propagation]: PASS",
            "[Test EXEC-D: EOF Stream Signal Propagation]: PASS",
            "[Test EXEC-E: Non-Zero Process Exit Status]: PASS",
            "[Test EXEC-F: Workload Cancellation Process Termination]: PASS",
            "[Test EXEC-G: Capability Authorization Rejection]: PASS",
            "[Test EXEC-H: Workspace Containment Boundary Enforcement]: PASS",
            "[Test EXEC-I: Temporary File Prohibition (0 /tmp files created)]: PASS",
            "[Test EXEC-J: Offline Execution Autonomy]: PASS",
            "[Test EXEC-K: PMM Memory Neutrality]: PASS",
            "[Test EXEC-L: Stage 3A-3N Nucleus Preservation Audit]: PASS",
            "[WI-02] ALL 12 TESTS PASSED",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"WI-02 QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
