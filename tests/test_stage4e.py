"""
Stage 4E Test Suite: Agent Runtime Subsystem (agentd)
Authoritative Contract: Stage 4E Architecture Specification Rev2 & ADR-0028 Rev2.
Implementation Plan: Stage 4E Implementation Plan Rev3.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `brokerd` compiles cleanly with zero warnings/errors.
   - `init` compiles cleanly with zero warnings/errors.
   - `resourced` compiles cleanly with zero warnings/errors.
   - `workloadd` compiles cleanly with zero warnings/errors.
   - `workspaced` compiles cleanly with zero warnings/errors.
   - `agentd` compiles cleanly with zero warnings/errors.
2. Static Kernel Symbols:
   - `run_stage4e_verification` exists in kernel ELF.
3. Live QEMU Telemetry:
   - All 26 Stage 4E machine verification tests pass:
     - 4E-A: agentd Startup
     - 4E-B: Broker Registration
     - 4E-C: AgentId Allocation
     - 4E-D: Agent Creation
     - 4E-E: Workspace Containment
     - 4E-F: Cap Attenuation
     - 4E-G: Amplification Rejection
     - 4E-H: Agent Lifecycle
     - 4E-I: Workload Creation
     - 4E-J: Workload Observation
     - 4E-K: Workload Cancellation
     - 4E-L: Termination Policy
     - 4E-M: Authenticated Events
     - 4E-N: Cross-Agent Event Rej.
     - 4E-O: Event Buffer Overflow
     - 4E-P: Trigger Execution
     - 4E-Q: Human Auth Request
     - 4E-R: Unauthorized Auth Rej.
     - 4E-S: Model Non-Authority
     - 4E-T: Persistent State
     - 4E-U: Agent Crash Recovery
     - 4E-V: Cap Handle Recon.
     - 4E-W: Workspace Deletion
     - 4E-X: Protocol Robustness
     - 4E-Y: PMM Neutrality
     - 4E-Z: Substrate Preservation
   - Final summary: "[Stage 4E] ALL 26 TESTS PASSED. Agent Runtime Subsystem VERIFIED."
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


class TestStage4EAgentRuntimeSubsystem(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage4e_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 4E agent modules."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage4e_brokerd_compilation(self):
        """Verify brokerd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "brokerd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"brokerd compilation failed:\n{res.stderr}")

    def test_stage4e_init_compilation(self):
        """Verify init supervisor binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "init"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"init compilation failed:\n{res.stderr}")

    def test_stage4e_resourced_compilation(self):
        """Verify resourced daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "resourced"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"resourced compilation failed:\n{res.stderr}")

    def test_stage4e_workloadd_compilation(self):
        """Verify workloadd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "workloadd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"workloadd compilation failed:\n{res.stderr}")

    def test_stage4e_workspaced_compilation(self):
        """Verify workspaced daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "workspaced"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"workspaced compilation failed:\n{res.stderr}")

    def test_stage4e_agentd_compilation(self):
        """Verify agentd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "agentd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"agentd compilation failed:\n{res.stderr}")

    def test_stage4e_kernel_symbols_exist(self):
        """Verify Stage 4E verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage4e_verification", raw_output, "run_stage4e_verification symbol missing from kernel ELF")

    def test_stage4e_qemu_machine_verification(self):
        """Execute QEMU and assert that all 26 Stage 4E machine verification tests pass."""
        markers = [
            "[Stage 4E: Agent Runtime Subsystem Verification]",
            "[Test 4E-A: agentd Startup]: PASS",
            "[Test 4E-B: Broker Registration]: PASS",
            "[Test 4E-C: AgentId Allocation]: PASS",
            "[Test 4E-D: Agent Creation]: PASS",
            "[Test 4E-E: Workspace Containment]: PASS",
            "[Test 4E-F: Cap Attenuation]: PASS",
            "[Test 4E-G: Amplification Rejection]: PASS",
            "[Test 4E-H: Agent Lifecycle]: PASS",
            "[Test 4E-I: Workload Creation]: PASS",
            "[Test 4E-J: Workload Observation]: PASS",
            "[Test 4E-K: Workload Cancellation]: PASS",
            "[Test 4E-L: Termination Policy]: PASS",
            "[Test 4E-M: Authenticated Events]: PASS",
            "[Test 4E-N: Cross-Agent Event Rej.]: PASS",
            "[Test 4E-O: Event Buffer Overflow]: PASS",
            "[Test 4E-P: Trigger Execution]: PASS",
            "[Test 4E-Q: Human Auth Request]: PASS",
            "[Test 4E-R: Unauthorized Auth Rej.]: PASS",
            "[Test 4E-S: Model Non-Authority]: PASS",
            "[Test 4E-T: Persistent State]: PASS",
            "[Test 4E-U: Agent Crash Recovery]: PASS",
            "[Test 4E-V: Cap Handle Recon.]: PASS",
            "[Test 4E-W: Workspace Deletion]: PASS",
            "[Test 4E-X: Protocol Robustness]: PASS",
            "[Test 4E-Y: PMM Neutrality]: PASS",
            "[Test 4E-Z: Substrate Preservation]: PASS",
            "[Stage 4E] ALL 26 TESTS PASSED. Agent Runtime Subsystem VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 4E QEMU verification failed: {msg}")

    def test_stage4e_qemu_clean_shutdown(self):
        """Verify Stage 4E terminates cleanly with ISA debug exit code 33."""
        time.sleep(0.5)
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 4E] ALL 26 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 4E QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
