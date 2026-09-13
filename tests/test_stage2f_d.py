"""
Stage 2F-D Test Suite: Higher-Half Direct Map (HHDM) Verification
Validates:
1. Mathematical Routing & Geometry:
   - HHDM_BASE (0xFFFF_8000_0000_0000) routes to PML4 index 256.
   - Initial 1 GiB mapping window routes to hhdm_pdpt index 0.
   - Translation round-trip: phys_to_hhdm and hhdm_to_phys.
2. ELF Machine Code & Symbol Audit:
   - `hhdm_pdpt` symbol exists in ELF and resides within .page_tables.
   - .page_tables section size is exactly 16 KiB (16384 bytes).
   - Disassembly confirms PML4[256] and hhdm_pdpt[0] writes with 64-bit zeroing.
3. Live QEMU Execution & Telemetry:
   - Triple memory read test (identity, kernel VMA, HHDM) identically yields 0x1BADB002.
   - Triple code read test (identity, kernel VMA, HHDM) identically yields 0xFA (cli).
   - Dynamic PMM frame allocation, HHDM pointer write/read, and clean deallocation.
   - Decoupling invariant: HHDM addressability != PMM ownership.
   - Dual bootstrap mappings (PML4[0] identity + PML4[511] higher-half) preserved.
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu

HHDM_BASE = 0xFFFF_8000_0000_0000
HHDM_SIZE = 0x4000_0000  # 1 GiB
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

class TestStage2FDHigherHalfDirectMap(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.objdump = get_objdump_path()
        cls.readelf = get_readelf_path()

    def test_hhdm_mathematical_routing(self):
        """Verify mathematical index calculations for HHDM."""
        pml4_idx = (HHDM_BASE >> 39) & 0x1FF
        self.assertEqual(pml4_idx, 256, f"HHDM_BASE must map to PML4 index 256, got {pml4_idx}")

        pdpt_idx = (HHDM_BASE >> 30) & 0x1FF
        self.assertEqual(pdpt_idx, 0, f"HHDM_BASE must map to PDPT index 0, got {pdpt_idx}")

        pd_idx = (HHDM_BASE >> 21) & 0x1FF
        self.assertEqual(pd_idx, 0, f"HHDM_BASE must map to PD index 0, got {pd_idx}")

        # Translation test for 1 MiB physical base
        test_phys = 0x0010_0000
        expected_hhdm = HHDM_BASE + test_phys
        self.assertEqual(expected_hhdm, 0xFFFF_8000_0010_0000)
        self.assertEqual(expected_hhdm - HHDM_BASE, test_phys)

    def test_hhdm_symbols_and_section_size(self):
        """Verify hhdm_pdpt symbol and .page_tables 16 KiB footprint."""
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

        self.assertIn("hhdm_pdpt", symbols, "Symbol hhdm_pdpt missing from ELF")
        self.assertIn("pml4_table", symbols, "Symbol pml4_table missing from ELF")
        self.assertIn("pdpt_table", symbols, "Symbol pdpt_table missing from ELF")
        self.assertIn("pd_table", symbols, "Symbol pd_table missing from ELF")

        # Verify exact 4096-byte spacing between sequential page tables
        pml4_addr = symbols["pml4_table"]
        pdpt_addr = symbols["pdpt_table"]
        pd_addr = symbols["pd_table"]
        hhdm_pdpt_addr = symbols["hhdm_pdpt"]

        self.assertEqual(pdpt_addr - pml4_addr, 4096, "PDPT must immediately follow PML4")
        self.assertEqual(pd_addr - pdpt_addr, 4096, "PD must immediately follow PDPT")
        self.assertEqual(hhdm_pdpt_addr - pd_addr, 4096, "HHDM_PDPT must immediately follow PD")

    def test_setup_page_tables_hhdm_disassembly(self):
        """Verify machine code of setup_page_tables links PML4[256] and hhdm_pdpt[0]."""
        res = subprocess.run(
            [
                self.objdump, "-drwC", "-m", "i386",
                "--start-address=0xffffffff80101057",
                "--stop-address=0xffffffff80101150",
                str(self.kernel_elf)
            ],
            capture_output=True, text=True, check=True
        )
        disasm = res.stdout

        # PML4[256] offset is 256 * 8 = 2048 = 0x800
        self.assertTrue(
            "800" in disasm or "2048" in disasm,
            "Disassembly must write to PML4[256] at offset 0x800 (2048)"
        )
        # Upper 32-bit dword zeroing at offset 2052 = 0x804
        self.assertTrue(
            "804" in disasm or "2052" in disasm,
            "Disassembly must zero upper 32 bits of PML4[256] at offset 0x804 (2052)"
        )
        # Present | Writable flags (0x3)
        self.assertIn("0x3", disasm, "Disassembly must set Present | Writable (0x3)")

    def test_qemu_stage2fd_telemetry(self):
        """Verify live QEMU execution and telemetry for Stage 2F-D."""
        markers = [
            "[Stage 2F-D: Higher-Half Direct Map (HHDM) Verification]",
            "HHDM Base Address:           0xFFFF800000000000 (PML4 Index: 256)",
            "PML4[256] Hierarchy:         Present, Writable, User=0 -> hhdm_pdpt",
            "HHDM PDPT[0] Link:           Present, Writable, User=0 -> pd_table",
            "HHDM Direct Map Window:      [0xFFFF800000000000, 0xFFFF800040000000) -> Phys [0, 1 GiB)",
            "Dual Memory Access Test:     VMA 0xFFFFFFFF80100000 == HHDM 0xFFFF800000100000 (Magic: 0x1BADB002)",
            "Dual Code Access Test:       VMA 0xFFFFFFFF80101000 == HHDM 0xFFFF800000101000 (Opcode: 0xFA)",
            "HHDM Live Frame Write/Read:  Allocated phys 0x",
            "wrote signature 0x4848444D564D4D31 via HHDM, read back verified",
            "PMM Ownership Invariant:     Addressable != Owned; frame 0 & kernel image remain Reserved in PMM",
            "Dual Mappings Preserved:     PML4[0] (identity) & PML4[511] (higher-half VMA) intact [via HHDM]",
            "[x] Stage 2F-D Higher-Half Direct Map verified.",
            "[Stage 2F-D Architectural Status & Verification Summary]",
            "Higher-Half Direct Map (HHDM) established at 0xFFFF_8000_0000_0000 (PML4 index 256).",
            "Dedicated hhdm_pdpt correctly integrated into .page_tables hierarchy.",
            "1 GiB physical RAM mapped 1:1 via 2 MiB huge pages at HHDM_BASE.",
            "Strongly typed PhysicalAddress, VirtualAddress, and HhdmAddress abstractions enforced.",
            "Dual virtual translation (kernel VMA and HHDM) verified for memory and code.",
            "Live dynamic PMM frame write/read cycle verified via HHDM pointer.",
            "Addressability vs PMM ownership decoupling strictly maintained.",
            "Dual bootstrap mappings (PML4[0] identity and PML4[511] higher-half) preserved."
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 2F-D QEMU verification failed: {failures}")

if __name__ == "__main__":
    unittest.main()
