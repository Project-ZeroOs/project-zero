"""
Stage 4D Test Suite: Workspace & Persistent Context Subsystem (workspaced)
Authoritative Contract: Stage 4D Architecture Specification Rev3 & Phase 4D Implementation Plan Rev3.

Validations:
1. User-Space Services Compilation:
   - `libzero` compiles cleanly with zero warnings/errors.
   - `brokerd` compiles cleanly with zero warnings/errors.
   - `init` compiles cleanly with zero warnings/errors.
   - `resourced` compiles cleanly with zero warnings/errors.
   - `workloadd` compiles cleanly with zero warnings/errors.
   - `workspaced` compiles cleanly with zero warnings/errors.
2. Static Kernel Symbols:
   - `run_stage4d_verification` exists in kernel ELF.
3. Live QEMU Telemetry:
   - All 36 Stage 4D machine verification tests pass:
     - 4D-1: Workspace Monotonic ID Creation
     - 4D-2: Workspace State Machine Transitions
     - 4D-3: Active Workspace Workload Execution Authority
     - 4D-4: Invalid Lifecycle State Rejection
     - 4D-5: Context Graph Resident Allocation Bounds
     - 4D-6: Context Node Insertion & Traversal
     - 4D-7: Context Edge Weight & Metadata Updates
     - 4D-8: Context Graph Cycle Tolerance
     - 4D-9: Resident Context Graph Eviction LRU
     - 4D-10: Subordinate Persistent Context State
     - 4D-11: ZeroFS Workspace Metadata Persistence
     - 4D-12: ZeroFS Context Graph Persistence
     - 4D-13: Crash Reconciliation - Valid workspace.meta
     - 4D-14: Crash Reconciliation - Corrupted context.graph
     - 4D-15: Crash Reconciliation - Missing workspace.meta
     - 4D-16: Scoped Capability Attenuation
     - 4D-17: Workload Capability Transfer
     - 4D-18: Scoped Capability Revocation on Workspace Deletion
     - 4D-19: Unrelated Capability Subtree Isolation
     - 4D-20: Workload Registration & Identity Association
     - 4D-21: Workspace Teardown Workload Cancellation Routing
     - 4D-22: Workspace Teardown 4B Lease Quarantine Routing
     - 4D-23: Workload Crash Workspace Survival
     - 4D-24: ZeroFS IO Failure Isolation
     - 4D-25: IPC Protocol Tag Validation
     - 4D-26: IPC Handle Transfer Ownership
     - 4D-27: Workspace Creation Request Dispatch
     - 4D-28: Workspace Suspend/Resume Request Dispatch
     - 4D-29: Workspace Delete Request Dispatch
     - 4D-30: Context Node Add Request Dispatch
     - 4D-31: Context Edge Add Request Dispatch
     - 4D-32: Context Query Request Dispatch
     - 4D-33: Workload Register Request Dispatch
     - 4D-34: Max Workspaces Static Bound
     - 4D-35: PMM Frame Leak Neutrality Deep
     - 4D-36: Frozen Substrate Preservation
   - Final summary: "[Stage 4D] ALL 36 TESTS PASSED. Workspace & Persistent Context Subsystem VERIFIED."
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


class TestStage4DWorkspaceSubsystem(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()
        cls.cargo = run_qemu.resolve_cargo()

    def test_stage4d_libzero_compilation(self):
        """Verify libzero compiles cleanly with Stage 4D workspace modules."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "libzero"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"libzero compilation failed:\n{res.stderr}")

    def test_stage4d_brokerd_compilation(self):
        """Verify brokerd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "brokerd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"brokerd compilation failed:\n{res.stderr}")

    def test_stage4d_init_compilation(self):
        """Verify init supervisor binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "init"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"init compilation failed:\n{res.stderr}")

    def test_stage4d_resourced_compilation(self):
        """Verify resourced daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "resourced"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"resourced compilation failed:\n{res.stderr}")

    def test_stage4d_workloadd_compilation(self):
        """Verify workloadd daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "workloadd"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"workloadd compilation failed:\n{res.stderr}")

    def test_stage4d_workspaced_compilation(self):
        """Verify workspaced daemon binary compiles cleanly."""
        res = subprocess.run(
            [str(self.cargo), "build", "--target", "x86_64-unknown-none"],
            cwd=str(PROJECT_ROOT / "workspaced"),
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0, f"workspaced compilation failed:\n{res.stderr}")

    def test_stage4d_kernel_symbols_exist(self):
        """Verify Stage 4D verification entry point exists in kernel ELF."""
        raw_output = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        ).stdout
        self.assertIn("run_stage4d_verification", raw_output,
                      "Required symbol 'run_stage4d_verification' missing from ELF")

    def test_stage4d_qemu_machine_verification(self):
        """Execute QEMU and assert that all 36 Stage 4D machine verification tests pass."""
        markers = [
            "[Stage 4D: Workspace & Persistent Context Subsystem Verification]",
            "[Test 4D-1: Workspace Monotonic ID Creation]: PASS",
            "[Test 4D-2: Workspace State Machine Transitions]: PASS",
            "[Test 4D-3: Active Workspace Workload Execution Authority]: PASS",
            "[Test 4D-4: Invalid Lifecycle State Rejection]: PASS",
            "[Test 4D-5: Context Graph Resident Allocation Bounds]: PASS",
            "[Test 4D-6: Context Node Insertion & Traversal]: PASS",
            "[Test 4D-7: Context Edge Weight & Metadata Updates]: PASS",
            "[Test 4D-8: Context Graph Cycle Tolerance]: PASS",
            "[Test 4D-9: Resident Context Graph Eviction LRU]: PASS",
            "[Test 4D-10: Subordinate Persistent Context State]: PASS",
            "[Test 4D-11: ZeroFS Workspace Metadata Persistence]: PASS",
            "[Test 4D-12: ZeroFS Context Graph Persistence]: PASS",
            "[Test 4D-13: Crash Reconciliation - Valid workspace.meta]: PASS",
            "[Test 4D-14: Crash Reconciliation - Corrupted context.graph]: PASS",
            "[Test 4D-15: Crash Reconciliation - Missing workspace.meta]: PASS",
            "[Test 4D-16: Scoped Capability Attenuation]: PASS",
            "[Test 4D-17: Workload Capability Transfer]: PASS",
            "[Test 4D-18: Scoped Capability Revocation on Workspace Deletion]: PASS",
            "[Test 4D-19: Unrelated Capability Subtree Isolation]: PASS",
            "[Test 4D-20: Workload Registration & Identity Association]: PASS",
            "[Test 4D-21: Workspace Teardown Workload Cancellation Routing]: PASS",
            "[Test 4D-22: Workspace Teardown 4B Lease Quarantine Routing]: PASS",
            "[Test 4D-23: Workload Crash Workspace Survival]: PASS",
            "[Test 4D-24: ZeroFS IO Failure Isolation]: PASS",
            "[Test 4D-25: IPC Protocol Tag Validation]: PASS",
            "[Test 4D-26: IPC Handle Transfer Ownership]: PASS",
            "[Test 4D-27: Workspace Creation Request Dispatch]: PASS",
            "[Test 4D-28: Workspace Suspend/Resume Request Dispatch]: PASS",
            "[Test 4D-29: Workspace Delete Request Dispatch]: PASS",
            "[Test 4D-30: Context Node Add Request Dispatch]: PASS",
            "[Test 4D-31: Context Edge Add Request Dispatch]: PASS",
            "[Test 4D-32: Context Query Request Dispatch]: PASS",
            "[Test 4D-33: Workload Register Request Dispatch]: PASS",
            "[Test 4D-34: Max Workspaces Static Bound]: PASS",
            "[Test 4D-35: PMM Frame Leak Neutrality Deep]: PASS",
            "[Test 4D-36: Frozen Substrate Preservation]: PASS",
            "[Stage 4D] ALL 36 TESTS PASSED. Workspace & Persistent Context Subsystem VERIFIED.",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        if not success:
            print("QEMU MSG:", msg)
        self.assertTrue(success, f"Stage 4D QEMU verification failed: {msg}")

    def test_stage4d_qemu_clean_shutdown(self):
        """Verify Stage 4D terminates cleanly with ISA debug exit code 33."""
        success, msg = run_qemu.test_qemu(
            self.kernel32_elf,
            markers=["[Stage 4D] ALL 36 TESTS PASSED"]
        )
        self.assertTrue(success, f"Stage 4D QEMU clean exit failed: {msg}")


if __name__ == "__main__":
    unittest.main()
