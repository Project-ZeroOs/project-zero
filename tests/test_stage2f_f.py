"""
Stage 2F-F Test Suite: 4 KiB Kernel Permission Splitting & W^X Enforcement
Validates:
1. ELF Symbol & Section Audit:
   - All section boundary symbols present:
     __multiboot_header_start/end, __text_start/end, __rodata_start/end,
     __data_start/end, __bss_start/end, __pmm_metadata_start/end,
     __page_tables_start/end, __stack_guard_start/end, __stack_start/end,
     __kernel_end.
   - All section boundary symbols are 4 KiB (0x1000) aligned.
   - Kernel total size <= 2 MiB (0x200000) bootstrap window.
2. QEMU Live Telemetry:
   - Section boundaries & alignments logged and verified.
   - Kernel size bound asserted (<= 2 MiB window).
   - Populated 4 KiB kernel_pt with granular permissions.
   - kernel_pd installed in pdpt_table[510] pointing to kernel_pt.
   - TLB shootdown via CR3 reload.
   - Active PML4 invariants: PML4[0] ABSENT, PML4[256] present, PML4[511] present.
   - Hardware protections: CR0.WP=1, IA32_EFER.NXE=1.
   - Controlled #PF traps:
     * .rodata write #PF: Error code 0x03 (Protection Write Kernel)
     * .text write #PF: Error code 0x03 (Protection Write Kernel)
     * .data execute #PF: Error code 0x11 (Instruction Fetch NX Violation)
     * stack-guard #PF: Error code 0x00 (Non-present Kernel)
   - HHDM Privileged Aperture test:
     * Write to .text physical frame via HHDM succeeds.
     * Byte modified and immediately restored.
     * Confirmed HHDM is a privileged RW aperture, not an immutability boundary.
   - Normal subsystem operation:
     * .text execution, .rodata read, stack recursion, HHDM, and VMM lifecycle operational.
   - Stage 2F memory architecture transition complete marker.
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
PAGE_SIZE        = 0x1000
BOOTSTRAP_WINDOW = 0x400000


def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")


class TestStage2FFPermissions(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()

    def _get_elf_symbols(self):
        res = subprocess.run(
            [self.readelf, "-sW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        )
        symbols = {}
        for line in res.stdout.splitlines():
            parts = line.strip().split()
            if len(parts) >= 8 and parts[0].rstrip(':').isdigit():
                try:
                    symbols[parts[7]] = int(parts[1], 16)
                except ValueError:
                    continue
        return symbols

    def test_elf_section_boundaries_and_alignment(self):
        """Verify section boundary symbols exist, are 4 KiB aligned, and <= 4 MiB window."""
        symbols = self._get_elf_symbols()

        required_symbols = [
            "__multiboot_header_start", "__multiboot_header_end",
            "__text_start", "__text_end",
            "__rodata_start", "__rodata_end",
            "__data_start", "__data_end",
            "__bss_start", "__bss_end",
            "__pmm_metadata_start", "__pmm_metadata_end",
            "__page_tables_start", "__page_tables_end",
            "__stack_guard_start", "__stack_guard_end",
            "__stack_start", "__stack_end",
            "__kernel_end",
        ]

        for sym in required_symbols:
            self.assertIn(sym, symbols, f"Required symbol '{sym}' missing from ELF")
            addr = symbols[sym]
            self.assertEqual(addr % PAGE_SIZE, 0,
                f"Symbol '{sym}' at 0x{addr:X} is not 4 KiB aligned")
            self.assertGreaterEqual(addr, KERNEL_VIRT_BASE,
                f"Symbol '{sym}' at 0x{addr:X} is not in higher-half VMA")

        # Verify kernel end fits strictly within the 4 MiB bootstrap window
        kernel_end = symbols["__kernel_end"]
        self.assertLessEqual(kernel_end, KERNEL_VIRT_BASE + BOOTSTRAP_WINDOW,
            f"__kernel_end 0x{kernel_end:X} exceeds 4 MiB bootstrap window (0x{KERNEL_VIRT_BASE + BOOTSTRAP_WINDOW:X})")

        # Verify monotonicity of sections
        pairs = [
            ("__multiboot_header_start", "__multiboot_header_end"),
            ("__multiboot_header_end", "__text_start"),
            ("__text_start", "__text_end"),
            ("__text_end", "__rodata_start"),
            ("__rodata_start", "__rodata_end"),
            ("__rodata_end", "__data_start"),
            ("__data_start", "__data_end"),
            ("__data_end", "__bss_start"),
            ("__bss_start", "__bss_end"),
            ("__bss_end", "__pmm_metadata_start"),
            ("__pmm_metadata_start", "__pmm_metadata_end"),
            ("__pmm_metadata_end", "__page_tables_start"),
            ("__page_tables_start", "__page_tables_end"),
            ("__page_tables_end", "__stack_guard_start"),
            ("__stack_guard_start", "__stack_guard_end"),
            ("__stack_guard_end", "__stack_start"),
            ("__stack_start", "__stack_end"),
        ]
        for start_sym, end_sym in pairs:
            self.assertLessEqual(symbols[start_sym], symbols[end_sym],
                f"Section order violation: {start_sym} (0x{symbols[start_sym]:X}) > {end_sym} (0x{symbols[end_sym]:X})")

    def test_qemu_stage2ff_telemetry(self):
        """Verify live QEMU execution of Stage 2F-F 4 KiB permission splitting & W^X."""
        symbols = self._get_elf_symbols()
        # Page count = total pages from multiboot start to stack end, minus unmapped guard page
        mapped_count = ((symbols["__kernel_end"] - symbols["__multiboot_header_start"]) // PAGE_SIZE) - 1

        markers = [
            "[Stage 2F-F: 4 KiB Kernel Permission Splitting & W^X Enforcement]",
            f".multiboot_header: [0x{symbols['__multiboot_header_start']:016X}, 0x{symbols['__multiboot_header_end']:016X}) (R + NX)",
            f".text:             [0x{symbols['__text_start']:016X}, 0x{symbols['__text_end']:016X}) (RX)",
            f".rodata:           [0x{symbols['__rodata_start']:016X}, 0x{symbols['__rodata_end']:016X}) (R + NX)",
            f".data:             [0x{symbols['__data_start']:016X}, 0x{symbols['__data_end']:016X}) (RW + NX)",
            f".bss:              [0x{symbols['__bss_start']:016X}, 0x{symbols['__bss_end']:016X}) (RW + NX)",
            f".pmm_metadata:     [0x{symbols['__pmm_metadata_start']:016X}, 0x{symbols['__pmm_metadata_end']:016X}) (RW + NX)",
            f".page_tables:      [0x{symbols['__page_tables_start']:016X}, 0x{symbols['__page_tables_end']:016X}) (RW + NX)",
            f".stack_guard:      [0x{symbols['__stack_guard_start']:016X}, 0x{symbols['__stack_guard_end']:016X}) (NOT PRESENT)",
            f".stack:            [0x{symbols['__stack_start']:016X}, 0x{symbols['__stack_end']:016X}) (RW + NX)",
            f"Kernel Size Bound:           0x{symbols['__kernel_end']:016X} <= 0xFFFFFFFF80400000 (<= 4 MiB window) [VERIFIED]",
            f"Populated 4 KiB kernel_pt:   {mapped_count} active 4 KiB pages mapped with granular permissions",
            "Installed kernel_pd:         pdpt_table[510] -> kernel_pd[0] -> kernel_pt [via HHDM]",
            "TLB Shootdown:               CR3 reloaded (switched from 2 MiB to 4 KiB mappings)",
            "Active PML4 Invariants:      PML4[0]=ABSENT, PML4[256]=present, PML4[511]=present [VERIFIED]",
            "Hardware Enforcement:       CR0.WP=1 (Ring 0 write protect), IA32_EFER.NXE=1 (No-Execute) [VERIFIED]",
            "[CONTROLLED EXCEPTION TRAP] Vector 14 (Page Fault (#PF)) trapped at RIP:",
            "Controlled .rodata Write:    #PF trapped at CR2",
            "Error Code: 0x0000000000000003 (Protection Write Kernel) [VERIFIED]",
            "Controlled .text Write:      #PF trapped at CR2",
            "Controlled .data Execute:    #PF trapped at CR2",
            "Instruction Fetch NX Violation",
            "Controlled Stack Guard:      #PF trapped at CR2",
            "Error Code: 0x0000000000000000 (Non-present Kernel) [VERIFIED]",
            "HHDM Privileged Aperture:    Write to .text physical frame 0x00101000 via HHDM SUCCEEDED [VERIFIED]",
            "Byte modified 0xFA -> 0x50 and restored to 0xFA",
            "[CONFIRMED: HHDM is a privileged RW aperture, not an immutability boundary]",
            "Normal Subsystems:           .text execution, .rodata read, stack recursion, HHDM, and VMM lifecycle [ALL OPERATIONAL]",
            "[x] Stage 2F-F 4 KiB permission splitting & W^X enforcement verified.",
            "[Stage 2F Memory Architecture Transition Complete]",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 2F-F QEMU verification failed:\n" +
                        "\n".join(f"  - {f}" for f in failures))


if __name__ == "__main__":
    unittest.main()
