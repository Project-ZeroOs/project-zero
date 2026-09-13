"""
Project Zero - Boot Subsystem Test Suite

Automated integration test for Project Zero bootable prototype:
- Verifies Multiboot 1 protocol compliance
- Confirms 64-bit Long Mode activation and Ring 0 privilege state
- Confirms serial port COM1 output and expected startup banner
- Confirms negative failure detection on corrupted payloads
"""

import sys
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

BOOT_MARKERS = [
    "BPLK",
    "PROJECT ZERO",
    "Kernel initialized.",
    "Privilege Level:    Ring 0 (DPL 0)",
    "Segment Registers:  CS=0x0008 SS=0x0010 DS=0x0010 ES=0x0010 FS=0x0010 GS=0x0010"
]


class TestBoot(unittest.TestCase):
    def test_boot_protocol_and_diagnostics(self):
        """Positive Test: Build kernel and verify boot protocol & early CPU diagnostics."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel binary {image} does not exist.")

        success, failures = run_qemu.test_qemu(image, markers=BOOT_MARKERS)
        self.assertTrue(success, f"Boot verification failed: {failures}")

    def test_negative_boot_detection(self):
        """Negative Test: Verify harness rejects invalid or corrupted binary."""
        dummy_file = run_qemu.BUILD_DIR / "dummy_boot_corrupt.bin"
        dummy_file.write_bytes(b"INVALID_BOOT_CONTAINER")
        try:
            success, failures = run_qemu.test_qemu(dummy_file, markers=BOOT_MARKERS)
            self.assertFalse(success, "Harness falsely claimed success on corrupted binary!")
            self.assertIsNotNone(failures)
        finally:
            if dummy_file.exists():
                dummy_file.unlink()

if __name__ == "__main__":
    unittest.main()
