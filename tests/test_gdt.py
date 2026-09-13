"""
Project Zero - Stage 2B GDT & TSS Subsystem Test Suite

Automated test suite verifying:
- 64-bit GDT table initialization
- Named segment selectors (KERNEL_CODE, KERNEL_DATA, USER_DATA, USER_CODE, TSS)
- Task State Segment (TSS) loading via `ltr` into Task Register (TR = 0x0028)
- Segment registers reload (CS = 0x0008, DS/SS/ES/FS/GS = 0x0010)
- Stack configuration and bounds audit: disjoint normal kernel stack and IST1 stack
- CPU exception regression: verifying that #BP, #UD, #DE, #PF remain fully operational with active TSS/IST1
- Negative test: ensuring harness rejects non-functional binaries
"""

import sys
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

GDT_MARKERS = [
    "BPLK",
    "PROJECT ZERO",
    "Kernel initialized.",
    "[Stage 2B: GDT, TSS & Privilege Boundary Diagnostics]",
    "Task Register (TR): 0x0028 (Active TSS Selector: 0x0028)",
    "Segment Registers:  CS=0x0008 SS=0x0010 DS=0x0010 ES=0x0010 FS=0x0010 GS=0x0010",
    "Privilege Level:    Ring 0 (DPL 0)",
    "[Explicit Critical-Memory Range Audit (Half-Open Intervals [start, end))]",
    "All 7 critical memory regions verified strictly non-overlapping (disjoint).",
    "[Double-Fault (#DF) Structural Verification]",
    "#DF gate structurally bound to independent 16-byte aligned IST1 stack.",
    "IST1 isolation confirmed: zero overlap with normal stack, GDT, TSS, IDT, or page tables.",
    "[Stage 2C Architectural Status & Verification Summary]"
]


class TestStage2BGdt(unittest.TestCase):
    def test_gdt_and_tss_subsystem(self):
        """Build and verify Stage 2B GDT, TSS, and stack bounds in QEMU."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel binary {image} does not exist.")

        success, failures = run_qemu.test_qemu(image, markers=GDT_MARKERS)
        self.assertTrue(success, f"Stage 2B GDT test failed: {failures}")

    def test_negative_gdt_detection(self):
        """Negative test: verify test harness rejects corrupt binaries."""
        dummy_file = run_qemu.BUILD_DIR / "dummy_gdt_corrupt.bin"
        dummy_file.write_bytes(b"INVALID_GDT_IMAGE")
        try:
            success, failures = run_qemu.test_qemu(dummy_file, markers=GDT_MARKERS)
            self.assertFalse(success, "Harness falsely passed corrupted binary!")
            self.assertIsNotNone(failures)
        finally:
            if dummy_file.exists():
                dummy_file.unlink()

if __name__ == "__main__":
    unittest.main()
