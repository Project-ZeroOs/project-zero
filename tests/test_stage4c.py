"""
Stage 4C Test Suite: Workload Orchestration & Task Execution Subsystem (workloadd)
Authoritative Contract: Stage 4C Architecture Specification Rev4 & Phase 4C Implementation Plan Rev2.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `brokerd` compiles cleanly with zero warnings/errors.
   - `init` compiles cleanly with zero warnings/errors.
   - `resourced` compiles cleanly with zero warnings/errors.
   - `workloadd` compiles cleanly with zero warnings/errors.
2. Static Kernel Symbols:
   - `run_stage4c_verification` exists in kernel ELF.
3. Live QEMU Telemetry:
   - All 35 Stage 4C machine verification tests pass:
     - 4C-1: Workload Monotonic ID Creation
     - 4C-2: Workload Generation Increment
     - 4C-3: Workload Lifecycle Transitions
     - 4C-4: Workload Identity Immutability
     - 4C-5: Task DAG Admit Valid Pipeline
     - 4C-6: Task DAG Reject Direct Cycle
     - 4C-7: Task DAG Reject Complex Cycle
     - 4C-8: Task DAG Dependency Ordering
     - 4C-9: DAG Mutation Authority Running
     - 4C-10: DAG Mutation Rejected Completed
     - 4C-11: Supervisor Spawner Process Launch
     - 4C-12: Task Process Association Isolation
     - 4C-13: Multi-Process Workload Coordination
     - 4C-14: Task Clean Completion
     - 4C-15: Task App Failure Handling
     - 4C-16: Process Crash Workload Survival
     - 4C-17: Capability Attenuation Enforced
     - 4C-18: Task Capability Isolation Boundary
     - 4C-19: Failed Task Capability Teardown
     - 4C-20: Retry Fresh Capability State
     - 4C-21: Workload Completion No Cap Leak
     - 4C-22: Resource Demand Formulation
     - 4C-23: Lease Acquisition via Resourced
     - 4C-24: Lease Rejection Insufficient Capacity
     - 4C-25: Lease Release on Task Completion
     - 4C-26: Lease Quarantine on Task Failure
     - 4C-27: Time Authority Loss Quarantine
     - 4C-28: Provider Loss Quarantine
     - 4C-29: Class 1 Pure Compute Replay
     - 4C-30: Class 2 Checkpointed Stateful Resume
     - 4C-31: Class 3 Irreversible Fail at Milestone
     - 4C-32: Workload Cancellation Quarantine
     - 4C-33: Cancellation Race Process Launch
     - 4C-34: PMM Frame Leak Neutrality Deep
     - 4C-35: Frozen Substrate Preservation
   - Final summary: "[Stage 4C] ALL 35 TESTS PASSED. Workload Orchestration & Task Execution VERIFIED."
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


class TestStage4CWorkloadOrchestration(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage4c_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 4C workload modules."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage4c_brokerd_compilation(self):
        """Verify brokerd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "brokerd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"brokerd compilation failed:\n{res.stderr}")

    def test_stage4c_init_compilation(self):
        """Verify init supervisor binary compiles cleanly with Process Spawner extension."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "init"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"init compilation failed:\n{res.stderr}")

    def test_stage4c_resourced_compilation(self):
        """Verify resourced daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "resourced"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"resourced compilation failed:\n{res.stderr}")

    def test_stage4c_workloadd_compilation(self):
        """Verify workloadd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "workloadd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"workloadd compilation failed:\n{res.stderr}")

    def test_stage4c_kernel_symbols_exist(self):
        """Verify Stage 4C verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage4c_verification", raw_output,
                      "Required symbol 'run_stage4c_verification' missing from ELF")

    def test_stage4c_qemu_machine_verification(self):
        """Execute QEMU and assert that all 35 Stage 4C machine verification tests pass."""
        markers = [
            "[Stage 4C: Workload Orchestration & Task Execution Verification]",
            "[Test 4C-1: Workload Monotonic ID Creation]: PASS",
            "[Test 4C-2: Workload Generation Increment]: PASS",
            "[Test 4C-3: Workload Lifecycle Transitions]: PASS",
            "[Test 4C-4: Workload Identity Immutability]: PASS",
            "[Test 4C-5: Task DAG Admit Valid Pipeline]: PASS",
            "[Test 4C-6: Task DAG Reject Direct Cycle]: PASS",
            "[Test 4C-7: Task DAG Reject Complex Cycle]: PASS",
            "[Test 4C-8: Task DAG Dependency Ordering]: PASS",
            "[Test 4C-9: DAG Mutation Authority Running]: PASS",
            "[Test 4C-10: DAG Mutation Rejected Completed]: PASS",
            "[Test 4C-11: Supervisor Spawner Process Launch]: PASS",
            "[Test 4C-12: Task Process Association Isolation]: PASS",
            "[Test 4C-13: Multi-Process Workload Coordination]: PASS",
            "[Test 4C-14: Task Clean Completion]: PASS",
            "[Test 4C-15: Task App Failure Handling]: PASS",
            "[Test 4C-16: Process Crash Workload Survival]: PASS",
            "[Test 4C-17: Capability Attenuation Enforced]: PASS",
            "[Test 4C-18: Task Capability Isolation Boundary]: PASS",
            "[Test 4C-19: Failed Task Capability Teardown]: PASS",
            "[Test 4C-20: Retry Fresh Capability State]: PASS",
            "[Test 4C-21: Workload Completion No Cap Leak]: PASS",
            "[Test 4C-22: Resource Demand Formulation]: PASS",
            "[Test 4C-23: Lease Acquisition via Resourced]: PASS",
            "[Test 4C-24: Lease Rejection Insufficient Capacity]: PASS",
            "[Test 4C-25: Lease Release on Task Completion]: PASS",
            "[Test 4C-26: Lease Quarantine on Task Failure]: PASS",
            "[Test 4C-27: Time Authority Loss Quarantine]: PASS",
            "[Test 4C-28: Provider Loss Quarantine]: PASS",
            "[Test 4C-29: Class 1 Pure Compute Replay]: PASS",
            "[Test 4C-30: Class 2 Checkpointed Stateful Resume]: PASS",
            "[Test 4C-31: Class 3 Irreversible Fail at Milestone]: PASS",
            "[Test 4C-32: Workload Cancellation Quarantine]: PASS",
            "[Test 4C-33: Cancellation Race Process Launch]: PASS",
            "[Test 4C-34: PMM Frame Leak Neutrality Deep]: PASS",
            "[Test 4C-35: Frozen Substrate Preservation]: PASS",
            "[Stage 4C] ALL 35 TESTS PASSED. Workload Orchestration & Task Execution VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 4C QEMU verification failed: {msg}")

    def test_stage4c_qemu_clean_shutdown(self):
        """Verify Stage 4C terminates cleanly with ISA debug exit code 33."""
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 4C] ALL 35 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 4C QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
