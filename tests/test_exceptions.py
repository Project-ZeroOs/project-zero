"""
Project Zero - Stage 2A Exception & IDT Subsystem Test Suite

Automated test suite specifically verifying:
- IDT 256-vector table initialization
- Breakpoint (#BP, Vector 3) trap and recovery
- Invalid Opcode (#UD, Vector 6) controlled interception and safe resumption
- Divide Error (#DE, Vector 0) controlled interception and safe resumption
- Page Fault (#PF, Vector 14) controlled interception, CR2 register decoding, and safe resumption
- Negative verification: ensuring test harness rejects corrupted binaries
"""

import sys
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

EXCEPTION_MARKERS = [
    "BPLK",
    "PROJECT ZERO",
    "Kernel initialized.",
    "[Verification: Executing Controlled CPU Exception Regression Suite]",
    "Breakpoint (#BP) trapped successfully",
    "Breakpoint trap handled; execution resumed smoothly.",
    "Vector 6 (Invalid Opcode (#UD)) trapped",
    "Invalid Opcode (#UD) intercepted and recovered successfully.",
    "Vector 0 (Divide Error (#DE)) trapped",
    "Divide Error (#DE) intercepted and recovered successfully.",
    "Vector 14 (Page Fault (#PF)) trapped",
    "CR2 Fault Address: 0x0000000060000000",
    "Page Fault (#PF) trapped with correct CR2; execution resumed smoothly.",
    "[Stage 2C Architectural Status & Verification Summary]"
]


class TestStage2AExceptions(unittest.TestCase):
    def test_stage2a_exceptions_subsystem(self):
        """Build and verify Stage 2 IDT and CPU exception handling."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel image {image} does not exist.")

        success, failures = run_qemu.test_qemu(image, markers=EXCEPTION_MARKERS)
        self.assertTrue(success, f"Stage 2 exception test failed: {failures}")

    def test_negative_exception_detection(self):
        """Negative test: verify test harness rejects non-functional binaries."""
        dummy_file = run_qemu.BUILD_DIR / "dummy_corrupt.bin"
        dummy_file.write_bytes(b"INVALID_EXCEPTION_IMAGE")
        try:
            success, failures = run_qemu.test_qemu(dummy_file, markers=EXCEPTION_MARKERS)
            self.assertFalse(success, "Harness falsely passed corrupted binary!")
            self.assertIsNotNone(failures)
        finally:
            if dummy_file.exists():
                dummy_file.unlink()


if __name__ == "__main__":
    unittest.main()
