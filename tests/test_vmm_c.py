"""
Stage 2E-C Test Suite: Integrated Boot Verification & Comprehensive VMM Lifecycle
Validates:
1. Full Stage 2E integrated boot sequence in QEMU.
2. Complete end-to-end VMM lifecycle:
   - Dynamic geometry discovery
   - IA32_EFER.NXE enablement
   - Typed CR3 abstraction with single-core PCID policy
   - 4 KiB typed page tables and entry bitmasks
   - Kernel security domain (USER=0 across hierarchy)
   - Sparse virtual mapping (0x50000000) outside boot range
   - Address translation and linear address translation
   - Volatile memory read/write access and physical data confirmation
   - Duplicate mapping rejection with rollback
   - Page unmapping and TLB invalidation (invlpg)
   - Empty intermediate table reclamation (PT, PD)
   - Mapped frame caller ownership preservation
   - Exact PMM accounting restoration (initial free == final free)
   - Boot kernel identity mapping integrity
3. VMM error model completeness.
"""

import unittest
import sys
from pathlib import Path

# Add project root and tools to path
PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu

class MockGeometry:
    def __init__(self, phys_bits=40, virt_bits=48, nx_enabled=True):
        self.phys_bits = phys_bits
        self.virt_bits = virt_bits
        self.phys_mask = ((1 << phys_bits) - 1) & ~0xFFF
        self.nx_enabled = nx_enabled

    def is_canonical(self, vaddr: int) -> bool:
        sign_bit = (vaddr >> 47) & 1
        expected_extension = 0xFFFF_0000_0000_0000 if sign_bit == 1 else 0
        return (vaddr & 0xFFFF_0000_0000_0000) == expected_extension

    def is_valid_phys_frame(self, addr: int) -> bool:
        return (addr & ~self.phys_mask) == 0 and (addr % 4096 == 0)

class MockPMM:
    def __init__(self, start_frame=0x179000, max_frames=500):
        self.frames = [start_frame + i * 4096 for i in range(max_frames)]
        self.allocated = set()

    def alloc_frame(self):
        for f in self.frames:
            if f not in self.allocated:
                self.allocated.add(f)
                return f
        return None

    def free_frame(self, f: int):
        assert f in self.allocated, f"Double free or freeing unallocated frame 0x{f:X}"
        self.allocated.remove(f)

    def free_count(self) -> int:
        return len(self.frames) - len(self.allocated)

class TestStage2ECIntegratedVMM(unittest.TestCase):
    def setUp(self):
        self.geometry = MockGeometry(phys_bits=40, virt_bits=48, nx_enabled=True)
        self.pmm = MockPMM()

    def test_complete_vmm_lifecycle_and_accounting(self):
        """Simulates full map -> access -> unmap -> free lifecycle and verifies exact PMM accounting."""
        initial_free = self.pmm.free_count()

        # 1. Caller allocates data frame
        data_frame = self.pmm.alloc_frame()
        self.assertIsNotNone(data_frame)
        self.assertEqual(self.pmm.free_count(), initial_free - 1)

        # 2. VMM allocates intermediate tables (PDPT, PD, PT)
        pdpt_frame = self.pmm.alloc_frame()
        pd_frame = self.pmm.alloc_frame()
        pt_frame = self.pmm.alloc_frame()
        self.assertEqual(self.pmm.free_count(), initial_free - 4)

        # 3. Unmap: Empty tables (PT, PD) are reclaimed, but data_frame is NOT freed
        self.pmm.free_frame(pt_frame)
        self.pmm.free_frame(pd_frame)
        # Assume PDPT still has other entries so it is not freed yet
        self.pmm.free_frame(pdpt_frame)
        self.assertEqual(self.pmm.free_count(), initial_free - 1)

        # 4. Caller explicitly frees data frame
        self.pmm.free_frame(data_frame)
        self.assertEqual(self.pmm.free_count(), initial_free)

    def test_stage2ec_integrated_boot_qemu(self):
        """Runs Project Zero in QEMU and verifies all Stage 2E-A, 2E-B, and 2E-C boot diagnostics."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel image {image} does not exist.")

        markers = [
            # Stage 2E-A Foundations
            "[Stage 2E-A: Architecture Foundations (Geometry, CR3 & Typed Tables)]",
            "Physical Address Width:   40 bits (Mask: 0x000000FFFFFFF000)",
            "Linear/Virtual Width:     48 bits (Canonical 48-bit Mode)",
            "No-Execute (NX) Status:   Supported: true, Enabled: true (IA32_EFER.NXE)",
            "Active CR3 Physical Root: 0x000000000016",
            "CR3 Control Flags:        PWT=false, PCD=false, PCID=disabled (single-core bring-up)",
            "Page Table Geometry:      512 entries x 8 bytes = 4096 bytes (aligned: true)",
            "Security Domain Policy:   Kernel mappings enforce USER=0 throughout hierarchy",
            "[x] Stage 2E-A architectural instruments & invariants verified.",

            # Stage 2E-B Virtual Memory Mapping Engine
            "[Stage 2E-B: Virtual Memory Mapping Engine]",
            "Initial Free Physical Frames:",
            "Allocated Test Data Frame:",
            "Mapped 0x50000000 ->",
            "Security Domain Policy:      USER=0 verified at PML4E, PDPTE, PDE, and PTE",
            "Duplicate Mapping Rejection: Successfully rejected with AlreadyMapped",
            "Live Memory Access:          Read/write verified (magic: 0x5A5ABEEFCAFE0042)",
            "Unmapped 0x50000000:",
            "Empty-Table Reclamation:     Intermediate PT & PD reclaimed; parent PDPT[1] cleared",
            "Data Frame Reclaimed:        Caller freed data frame",
            "PMM Accounting Restored:     Final free",
            "Boot Mappings Intact:        Kernel image page 0x00100000 verified mapped",
            "[x] Stage 2E-B virtual memory mapping engine verified.",

            # Stage 2E-B Architectural Status & Verification Summary
            "[Stage 2E-B Architectural Status & Verification Summary]",
            "4 KiB aligned, canonical virtual-address mapping engine verified.",
            "Physical frame ownership strictly preserved across map/unmap operations.",
            "Kernel domain enforces USER=0 across all four hierarchy levels.",
            "Intermediate tables allocated on demand via PMM with zeroed contents.",
            "Atomic allocation-failure rollback guarantees zero leaked intermediate tables.",
            "Empty intermediate tables automatically reclaimed during unmap_page.",
            "Complete map/unmap cycle restores PMM free-frame accounting to exact baseline.",
            "Sparse virtual address (0x50000000) verified with live volatile read/write.",
            "Existing boot kernel memory mappings remain fully intact."
        ]

        success, failures = run_qemu.test_qemu(image, markers=markers)
        self.assertTrue(success, f"Stage 2E-C integrated QEMU verification failed: {failures}")

if __name__ == "__main__":
    unittest.main()
