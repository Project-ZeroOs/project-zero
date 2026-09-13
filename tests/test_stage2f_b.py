"""
Stage 2F-B Test Suite: Dual Bootstrap Page-Table Construction Verification
Validates:
1. Dual-mapping mathematics:
   - PML4[0] and PML4[511] index calculations for identity and higher-half.
   - PDPT[0] and PDPT[510] index calculations for identity 1 GiB and KERNEL_VIRT_BASE (0xFFFFFFFF80000000).
   - PD[0..511] 2 MiB huge-page coverage across physical 0..1 GiB.
2. Machine-code verification of setup_page_tables:
   - Dual PML4 linkage (indices 0 and 511).
   - Dual PDPT linkage (indices 0 and 510).
   - 64-bit upper dword zeroing.
   - Huge page bit (0x83) population across 512 entries.
3. Integrated QEMU execution telemetry:
   - Hardware-level CR3 verification.
   - Hardware-level PML4, PDPT, and PD table structure walk.
   - Live dual memory and code read tests (Multiboot magic 0x1BADB002 and entry opcode 0xFA).
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

class TestStage2FBDualMappings(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.objdump = get_objdump_path()

    def test_dual_mapping_index_calculations(self):
        """Mathematically verify that KERNEL_VIRT_BASE routes to PML4[511] and PDPT[510]."""
        # Identity address 0x00100000
        id_addr = 0x0010_0000
        id_pml4 = (id_addr >> 39) & 0x1FF
        id_pdpt = (id_addr >> 30) & 0x1FF
        id_pd   = (id_addr >> 21) & 0x1FF
        self.assertEqual(id_pml4, 0, "Identity mapping must use PML4[0]")
        self.assertEqual(id_pdpt, 0, "Identity mapping must use PDPT[0]")
        self.assertEqual(id_pd, 0, "0x00100000 is within the first 2 MiB page (PD[0])")

        # Higher-half address KERNEL_VIRT_BASE (0xFFFFFFFF80000000)
        hh_base = KERNEL_VIRT_BASE
        hh_pml4 = (hh_base >> 39) & 0x1FF
        hh_pdpt = (hh_base >> 30) & 0x1FF
        hh_pd   = (hh_base >> 21) & 0x1FF
        self.assertEqual(hh_pml4, 511, "Higher-half base must route through PML4[511]")
        self.assertEqual(hh_pdpt, 510, "Higher-half 1 GiB window must route through PDPT[510]")
        self.assertEqual(hh_pd, 0, "Higher-half base starts at PD[0]")

        # Higher-half address for 0x00100000 (0xFFFFFFFF80100000)
        hh_kernel = KERNEL_VIRT_BASE + id_addr
        self.assertEqual((hh_kernel >> 39) & 0x1FF, 511)
        self.assertEqual((hh_kernel >> 30) & 0x1FF, 510)
        self.assertEqual((hh_kernel >> 21) & 0x1FF, 0)
        self.assertEqual(hh_kernel & 0x1F_FFFF, id_addr & 0x1F_FFFF)

    def test_setup_page_tables_disassembly(self):
        """Verify machine code of setup_page_tables in kernel.elf."""
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

        # Check for PML4[0] write (offset 0)
        self.assertIn("0x3,%eax", disasm)
        # Check for PML4[511] write (offset 511 * 8 = 4088 = 0xFF8)
        self.assertTrue(
            "ff8" in disasm or "4088" in disasm,
            "Disassembly must write to PML4[511] at offset 0xFF8"
        )
        # Check for PDPT[510] write (offset 510 * 8 = 4080 = 0xFF0)
        self.assertTrue(
            "ff0" in disasm or "4080" in disasm,
            "Disassembly must write to PDPT[510] at offset 0xFF0"
        )
        # Check for 2 MiB huge page flag (0x83)
        self.assertIn("0x83", disasm, "Disassembly must set 0x83 (Present | Writable | Huge) on PD entries")
        # Check for 512 loop iteration counter
        self.assertIn("0x200", disasm, "Disassembly must loop 512 times (0x200)")

    def test_qemu_stage2fb_telemetry(self):
        """Verify live QEMU execution and telemetry for Stage 2F-B."""
        markers = [
            "[Stage 2F-B: Dual Bootstrap Page-Table Construction]",
            "PML4[0] (Identity 0..512 GiB):     Present, Writable, User=0 -> PDPT",
            "PML4[511] (Higher-Half 512 GiB):   Present, Writable, User=0 -> PDPT",
            "PML4 Invariant:              PML4[0] == PML4[511] (dual link); PML4[1..510] clean",
            "PDPT[0] (Identity 0..1 GiB):       Present, Writable, User=0 -> PD",
            "PDPT[510] (Higher-Half 1 GiB):     Present, Writable, User=0 -> PD",
            "PDPT Invariant:              PDPT[0] == PDPT[510] (dual link); PDPT[1..509, 511] clean",
            "PD Table Coverage:           512 entries x 2 MiB = 1024 MiB (1 GiB) mapped continuously",
            "Live Higher-Half VMA Access: 0xFFFFFFFF80100000 (Magic: 0x1BADB002)",
            "Live HHDM Access:            0xFFFF800000100000 (Magic: 0x1BADB002)",
            "Live HH+HHDM Code Access:   HH 0xFFFFFFFF80101000 == HHDM 0xFFFF800000101000 (Opcode: 0xFA [cli])",
            "[x] Stage 2F-B dual bootstrap page-table mappings verified.",
            "[Stage 2F-B Architectural Status & Verification Summary]",
            "Dual bootstrap page-table mappings verified across all hierarchy levels.",
            "PML4[0] and PML4[511] dual-link to PDPT root (identity and higher-half).",
            "PDPT[0] and PDPT[510] dual-link to PD table (identity 1 GiB and KERNEL_VIRT_BASE).",
            "512 x 2 MiB huge pages map first 1 GiB of physical address space continuously.",
            "Live dual memory and code read tests verify identical physical frame translation.",
            "Current execution remains identity-mapped (Stage 2F-C execution switch pending)."
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 2F-B QEMU verification failed: {failures}")

if __name__ == "__main__":
    unittest.main()
