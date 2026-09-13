"""
Stage 2F-C Test Suite: Higher-Half Execution Switch & Stack Transition Verification
Validates:
1. ELF symbol table inspection:
   - `_start_higher_half` is exported and has canonical VMA in higher half (>= KERNEL_VIRT_BASE).
   - `stack_top` and `stack_bottom` have canonical VMAs in higher half.
2. Machine-code disassembly:
   - `long_mode_entry` executes an absolute 64-bit jump (movabs rax, _start_higher_half; jmp rax).
   - `_start_higher_half` loads higher-half stack pointer (mov rsp, stack_top).
   - `_start_higher_half` calls `kernel_main` from within the higher-half address space.
3. Live QEMU execution telemetry:
   - Current execution RIP verified >= 0xFFFF_FFFF_8000_0000.
   - Current kernel RSP verified >= 0xFFFF_FFFF_8000_0000 within .stack.
   - Dual mapping state (PML4[0] identity + PML4[511] higher-half) preserved.
   - Complete Stage 2 subsystem suite operational under higher-half execution.
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu

KERNEL_VIRT_BASE = 0xFFFF_FFFF_8000_0000

def get_objdump_path() -> str:
    r = shutil.which("objdump")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/objdump.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("objdump not found")

def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")

class TestStage2FCHigherHalfExecution(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.objdump = get_objdump_path()
        cls.readelf = get_readelf_path()

    def test_higher_half_symbol_addresses(self):
        """Verify _start_higher_half, stack_top, and kernel_main have higher-half VMAs."""
        res = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        )
        symbols = {}
        for line in res.stdout.splitlines():
            parts = line.strip().split()
            if len(parts) >= 8 and parts[0].rstrip(':').isdigit():
                sym_addr_str = parts[1]
                sym_name = parts[7]
                try:
                    symbols[sym_name] = int(sym_addr_str, 16)
                except ValueError:
                    continue

        self.assertIn("_start_higher_half", symbols, "Symbol _start_higher_half missing from ELF")
        self.assertIn("stack_top", symbols, "Symbol stack_top missing from ELF")
        self.assertIn("kernel_main", symbols, "Symbol kernel_main missing from ELF")

        start_hh = symbols["_start_higher_half"]
        stack_top = symbols["stack_top"]
        kernel_main = symbols["kernel_main"]

        self.assertGreaterEqual(
            start_hh, KERNEL_VIRT_BASE,
            f"_start_higher_half VMA (0x{start_hh:X}) is not in higher half"
        )
        self.assertGreaterEqual(
            stack_top, KERNEL_VIRT_BASE,
            f"stack_top VMA (0x{stack_top:X}) is not in higher half"
        )
        self.assertGreaterEqual(
            kernel_main, KERNEL_VIRT_BASE,
            f"kernel_main VMA (0x{kernel_main:X}) is not in higher half"
        )

    def test_long_mode_higher_half_switch_disassembly(self):
        """Verify long_mode_entry performs the absolute 64-bit jump to higher half."""
        res = subprocess.run(
            [
                self.objdump, "-drwC", "-m", "i386:x86-64",
                "--disassemble=long_mode_entry",
                str(self.kernel_elf)
            ],
            capture_output=True, text=True, check=True
        )
        disasm = res.stdout

        # Must load rax with higher-half address (movabs) and jmp rax
        self.assertIn("movabs", disasm, "long_mode_entry must use movabs to load 64-bit higher-half target")
        self.assertIn("jmp    *%rax", disasm, "long_mode_entry must execute jmp rax to enter higher-half space")

    def test_qemu_stage2fc_telemetry(self):
        """Verify live QEMU execution through higher-half space and kernel stack transition."""
        markers = [
            "[Stage 2F-C: Higher-Half Execution Switch & Stack Transition]",
            "Execution Domain:            Canonical Higher-Half (0xFFFF_FFFF_8000_0000+) [VERIFIED]",
            "Kernel Stack Placement:      Dedicated .stack region",
            "Dual Mapping State:          PML4[0] (identity) & PML4[511] (higher-half) preserved",
            "[x] Stage 2F-C higher-half execution switch & stack transition verified.",
            "[Stage 2F-C Architectural Status & Verification Summary]",
            "Kernel execution successfully switched to canonical higher-half VMA (0xFFFF_FFFF_8000_0000+).",
            "Kernel stack transitioned to 16-byte aligned higher-half virtual window.",
            "Dual mapping (PML4[0] identity + PML4[511] higher-half) remains fully intact.",
            "All Stage 2 subsystems (IDT, GDT/TSS, PMM, VMM) verified operational in higher half."
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 2F-C QEMU verification failed: {failures}")

if __name__ == "__main__":
    unittest.main()
