"""
Project Zero - Stage 1 Integration Test & Negative Test Harness

Automated integration test for the Project Zero Stage 1 bootable kernel prototype.
Includes both positive validation and negative testing to confirm the test harness
does not falsely pass on corrupted/missing output.
"""

import sys
import unittest
from pathlib import Path

# Add tools directory to sys.path
TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

class TestStage1Boot(unittest.TestCase):
    def test_stage1_build_and_boot(self):
        """Positive Test: Build the Stage 1 kernel and verify deterministic boot in QEMU."""
        image = run_qemu.build_stage1()
        self.assertTrue(image.exists(), f"Kernel binary {image} does not exist.")

        success, failures = run_qemu.test_qemu(image)
        self.assertTrue(success, f"Stage 1 QEMU boot verification failed: {failures}")

    def test_negative_verification_detection(self):
        """Negative Test: Verify harness fails when given a non-kernel or corrupted binary."""
        dummy_file = run_qemu.BUILD_DIR / "corrupted_kernel.bin"
        dummy_file.write_bytes(b"NON_BOOTABLE_TRASH_DATA_FOR_NEGATIVE_TEST")
        try:
            success, failures = run_qemu.test_qemu(dummy_file)
            self.assertFalse(success, "Test harness falsely claimed success on corrupted binary!")
            self.assertIsNotNone(failures, "Failures list should not be empty on corrupted binary.")
        finally:
            if dummy_file.exists():
                dummy_file.unlink()

if __name__ == "__main__":
    unittest.main()
