"""
Project Zero - Stage 2 Nucleus Integration Test Suite

Automated test suite verifying the Stage 2 kernel nucleus:
- Positive test: builds and boots Stage 2 kernel, asserting all subsystems,
  exception handling, memory managers, threads, IPC, and performance benchmarks.
- Negative test: ensures corruption detection is active.
"""

import sys
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

class TestStage2Nucleus(unittest.TestCase):
    def test_stage2_kernel_nucleus(self):
        """Build and verify Stage 2C kernel nucleus in QEMU."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Stage 2 kernel binary {image} does not exist.")

        success, failures = run_qemu.test_qemu(image, markers=run_qemu.EXPECTED_STAGE2C_MARKERS)
        self.assertTrue(success, f"Stage 2C QEMU boot verification failed: {failures}")

    def test_negative_failure_detection(self):
        """Negative test: verify test harness detects corrupted binaries."""
        dummy_file = run_qemu.BUILD_DIR / "corrupted_stage2.bin"
        dummy_file.write_bytes(b"INVALID_STAGE2_PAYLOAD")
        try:
            success, failures = run_qemu.test_qemu(dummy_file, markers=run_qemu.EXPECTED_STAGE2C_MARKERS)
            self.assertFalse(success, "Harness falsely passed corrupted binary!")
            self.assertIsNotNone(failures)
        finally:
            if dummy_file.exists():
                dummy_file.unlink()



if __name__ == "__main__":
    unittest.main()
