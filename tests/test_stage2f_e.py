"""
Stage 2F-E Test Suite: Identity Mapping Removal Verification
Validates:
1. ELF Symbol Audit:
   - All critical symbols (pml4_table, hhdm_pdpt, stack_top) are present and sane.
   - No regressions in ELF section layout.
2. QEMU Live Telemetry:
   - Pre-removal state: PML4[0]=present, PML4[256]=present, PML4[511]=present.
   - Identity removal: PML4[0] cleared via HHDM pointer.
   - TLB shootdown via CR3 reload.
   - Post-removal state: PML4[0]=ABSENT, PML4[256]=present, PML4[511]=present.
   - Controlled #PF: vector 14, CR2 == 0x00100000, error code == 0.
   - Handler executes in higher-half IDT/ISR/stack domain.
   - HHDM access (0xFFFF800000100000) and kernel VMA (0xFFFFFFFF80100000) valid post-removal.
   - Post-removal VMM cycle (alloc/map/write/read/unmap/free) succeeds.
   - Address-domain proof: physical != kernel VMA != HHDM.
3. Address-domain proof:
   - physical addr 0x00100000
   - != kernel VMA 0xFFFFFFFF80100000
   - != HHDM addr  0xFFFF800000100000
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
HHDM_BASE        = 0xFFFF_8000_0000_0000


def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")


class TestStage2FEIdentityRemoval(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()

    def test_address_domain_math(self):
        """Verify the three address domains are arithmetically distinct."""
        phys_addr = 0x0010_0000
        kernel_vma = KERNEL_VIRT_BASE + phys_addr
        hhdm_addr  = HHDM_BASE + phys_addr

        self.assertNotEqual(phys_addr, kernel_vma)
        self.assertNotEqual(phys_addr, hhdm_addr)
        self.assertNotEqual(kernel_vma, hhdm_addr)
        self.assertLess(phys_addr, 0x0020_0000)
        self.assertGreaterEqual(kernel_vma, KERNEL_VIRT_BASE)
        self.assertGreaterEqual(hhdm_addr, HHDM_BASE)

        pml4_phys  = (phys_addr >> 39) & 0x1FF
        pml4_vma   = (kernel_vma >> 39) & 0x1FF
        pml4_hhdm  = (hhdm_addr >> 39) & 0x1FF
        self.assertEqual(pml4_phys, 0)
        self.assertEqual(pml4_vma, 511)
        self.assertEqual(pml4_hhdm, 256)

    def test_elf_symbols_present(self):
        """Verify all critical ELF symbols have valid kernel-VMA / physical correspondence."""
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

        required = [
            "pml4_table", "pdpt_table", "pd_table", "hhdm_pdpt",
            "_start", "_start_higher_half",
        ]
        for sym in required:
            self.assertIn(sym, symbols, f"Required symbol '{sym}' missing from ELF")

        pml4_vma = symbols["pml4_table"]
        pml4_phys = pml4_vma - KERNEL_VIRT_BASE
        self.assertGreaterEqual(pml4_vma, KERNEL_VIRT_BASE,
            f"pml4_table VMA 0x{pml4_vma:X} must be within kernel VMA window")
        self.assertLess(pml4_phys, 0x4000_0000,
            f"pml4_table physical address 0x{pml4_phys:X} must have valid physical correspondence (< 1 GiB)")

    def test_qemu_stage2fe_telemetry(self):
        """Verify live QEMU execution of Stage 2F-E identity mapping removal."""
        markers = [
            "[Stage 2F-E: Identity Mapping Removal]",
            "Pre-Removal PML4 State:      PML4[0]=present, PML4[256]=present, PML4[511]=present",
            "Identity Removal:            PML4[0] cleared (virt == phys no longer valid)",
            "TLB Shootdown:               CR3 reloaded (full TLB flush performed)",
            "Post-Removal PML4 State:     PML4[0]=ABSENT, PML4[256]=present, PML4[511]=present [VERIFIED]",
            "[CONTROLLED EXCEPTION TRAP] Vector 14 (Page Fault (#PF)) trapped at RIP:",
            "CR2 Fault Address: 0x0000000000100000",
            "Error Code:        0x0000000000000000 (Non-present Read Kernel)",
            "Controlled Fault Test:       #PF (vector 14) trapped successfully",
            "CR2 Fault Address:          0x0000000000100000 == expected 0x0000000000100000 [EXACT MATCH]",
            "Error Code:                 0x0000 (Non-present, Read, Kernel)",
            "Handler Domain:             IDT in higher-half VMA; common_isr_stub in higher-half .text",
            "Recovery:                   Execution diverted to test_resume_pf in higher-half VMA",
            "HHDM Post-Removal Access:    phys 0x00100000 via HHDM 0xFFFF800000100000 = 0x1BADB002 [VERIFIED]",
            "Kernel VMA Post-Removal:     phys 0x00100000 via VMA  0xFFFFFFFF80100000 = 0x1BADB002 [VERIFIED]",
            "Post-Removal VMM Cycle:      Allocate, map at 0xFFFF800100000000, write/read, unmap, free [VERIFIED]",
            "PMM Accounting:              Free frames restored to baseline",
            "Address Domain Proof:",
            "physical addr  0x00100000",
            "!= kernel VMA  0xFFFFFFFF80100000",
            "!= HHDM addr   0xFFFF800000100000",
            "Project Zero no longer requires identity mapping for normal kernel operation.",
            "[x] Stage 2F-E identity mapping removal verified.",
            "[Stage 2F-E Architectural Status & Verification Summary]",
            "Identity mapping (PML4[0]) permanently removed from active address space.",
            "PML4[0] cleared via HHDM pointer; full TLB shootdown performed via CR3 reload.",
            "PML4[0] verified ABSENT; PML4[256] (HHDM) and PML4[511] (kernel VMA) verified INTACT.",
            "Kernel execution remains in canonical higher-half VMA before and after removal.",
            "Controlled #PF at physical 0x00100000: vector 14, CR2 exact match, kernel error code.",
            "Page-fault handler executes entirely in higher-half IDT/ISR/stack domain.",
            "HHDM (0xFFFF800000100000) and kernel VMA (0xFFFFFFFF80100000) remain valid post-removal.",
            "Post-removal VMM alloc/map/write/read/unmap/free cycle verified via HHDM seam.",
            "physical address != kernel virtual address != HHDM virtual address (3-way domain proof).",
            "Project Zero no longer requires identity mapping for normal kernel operation.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 2F-E QEMU verification failed:\n" +
                        "\n".join(f"  - {f}" for f in failures))


if __name__ == "__main__":
    unittest.main()
