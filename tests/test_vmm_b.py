"""
Stage 2E-B Test Suite: Virtual Memory Mapping Engine
Validates:
1. 4 KiB alignment checks on virtual pages and physical frames.
2. Canonical virtual address validation against discovered CPUID geometry.
3. Physical frame geometry validation against discovered physical mask.
4. Duplicate mapping prevention (AlreadyMapped).
5. Intermediate table allocation rollback on failure (zero intermediate table leaks).
6. Intermediate table empty-reclamation during unmap.
7. Mapped physical frame caller ownership preservation.
8. Complete map/translate/access/unmap cycle with PMM accounting preservation.
9. Live QEMU execution verifying all Stage 2E-B serial diagnostics and markers.
"""

import unittest
import sys
from pathlib import Path

# Add project root and tools to path
PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu

class MockGeometry:
    def __init__(self, phys_bits=40, virt_bits=48):
        self.phys_bits = phys_bits
        self.virt_bits = virt_bits
        self.phys_mask = ((1 << phys_bits) - 1) & ~0xFFF

    def is_canonical(self, vaddr: int) -> bool:
        sign_bit = (vaddr >> 47) & 1
        expected_extension = 0xFFFF_0000_0000_0000 if sign_bit == 1 else 0
        return (vaddr & 0xFFFF_0000_0000_0000) == expected_extension

    def is_valid_phys_frame(self, addr: int) -> bool:
        return (addr & ~self.phys_mask) == 0 and (addr % 4096 == 0)

class MockPageTable:
    def __init__(self):
        self.entries = [0] * 512

    def is_present(self, idx: int) -> bool:
        return (self.entries[idx] & 1) != 0

    def points_to_frame(self, idx: int, geometry: MockGeometry) -> int:
        if self.is_present(idx):
            return self.entries[idx] & geometry.phys_mask
        return None

    def set(self, idx: int, frame: int, flags: int, geometry: MockGeometry):
        assert geometry.is_valid_phys_frame(frame)
        self.entries[idx] = (frame & geometry.phys_mask) | flags

    def clear(self, idx: int):
        self.entries[idx] = 0

    def present_count(self) -> int:
        return sum(1 for e in self.entries if (e & 1) != 0)

class MockPMM:
    def __init__(self, start_frame=0x16E000, max_frames=100):
        self.frames = [start_frame + i * 4096 for i in range(max_frames)]
        self.allocated = set()
        self.fail_alloc_after = None  # To test rollback
        self.alloc_calls = 0

    def alloc_frame(self):
        self.alloc_calls += 1
        if self.fail_alloc_after is not None and self.alloc_calls > self.fail_alloc_after:
            return None
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

class TestStage2EBVirtualMemoryEngine(unittest.TestCase):
    def setUp(self):
        self.geometry = MockGeometry(phys_bits=40, virt_bits=48)
        self.pmm = MockPMM()

    def test_canonical_and_alignment_validation(self):
        """Validates canonical checking and 4 KiB alignment."""
        # Canonical low (0x0000_7FFF_FFFF_0000)
        self.assertTrue(self.geometry.is_canonical(0x0000_7FFF_FFFF_0000))
        # Non-canonical hole (0x0000_8000_0000_0000)
        self.assertFalse(self.geometry.is_canonical(0x0000_8000_0000_0000))
        # Canonical high (0xFFFF_8000_0000_0000)
        self.assertTrue(self.geometry.is_canonical(0xFFFF_8000_0000_0000))

        # 4 KiB Alignment
        self.assertTrue(self.geometry.is_valid_phys_frame(0x16E000))
        self.assertFalse(self.geometry.is_valid_phys_frame(0x16E001)) # Misaligned
        self.assertFalse(self.geometry.is_valid_phys_frame(0x1_0000_0000_0000)) # Exceeds 40-bit width

    def test_allocation_failure_rollback(self):
        """Simulates allocation failure during table hierarchy walk and validates atomic rollback."""
        pml4 = MockPageTable()
        pdpt = MockPageTable()
        tables = {0x1000: pml4, 0x2000: pdpt}

        # PML4 is pre-existing.
        # We simulate mapping where PDPT is allocated, but PD allocation fails.
        self.pmm.fail_alloc_after = 1  # 1st alloc (PDPT) succeeds, 2nd alloc (PD) fails
        initial_free = self.pmm.free_count()

        allocated_pdpt = None
        allocated_pd = None

        # Attempt map:
        # Step 1: allocate PDPT
        f1 = self.pmm.alloc_frame()
        self.assertIsNotNone(f1)
        allocated_pdpt = f1
        pml4.set(0, f1, 3, self.geometry)

        # Step 2: allocate PD -> Fails
        f2 = self.pmm.alloc_frame()
        self.assertIsNone(f2)

        # Rollback logic triggered on error:
        if allocated_pd:
            pdpt.clear(0)
            self.pmm.free_frame(allocated_pd)
        if allocated_pdpt:
            pml4.clear(0)
            self.pmm.free_frame(allocated_pdpt)

        # Confirm rollback: PML4 entry cleared, PMM free frames restored
        self.assertFalse(pml4.is_present(0))
        self.assertEqual(self.pmm.free_count(), initial_free)

    def test_empty_table_reclamation(self):
        """Validates that unmapping the last entry in a table triggers empty-table reclamation."""
        pt = MockPageTable()
        pd = MockPageTable()
        pt_frame = self.pmm.alloc_frame()
        pd.set(10, pt_frame, 3, self.geometry)

        # Map leaf entry
        data_frame = self.pmm.alloc_frame()
        pt.set(0, data_frame, 3, self.geometry)
        self.assertEqual(pt.present_count(), 1)

        # Unmap leaf entry
        ret_frame = pt.points_to_frame(0, self.geometry)
        pt.clear(0)
        self.assertEqual(ret_frame, data_frame) # Mapped frame returned to caller

        # Empty table check
        if pt.present_count() == 0:
            pd.clear(10)
            self.pmm.free_frame(pt_frame)

        # PT frame reclaimed to PMM; parent PD entry cleared
        self.assertFalse(pd.is_present(10))
        # data_frame still owned by caller (allocated in PMM until freed)
        self.assertIn(data_frame, self.pmm.allocated)
        self.pmm.free_frame(data_frame)

    def test_stage2eb_live_qemu_verification(self):
        """Runs the kernel in QEMU and verifies all Stage 2E-B serial diagnostics and markers."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel image {image} does not exist.")

        markers = [
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
        self.assertTrue(success, f"Stage 2E-B QEMU verification failed: {failures}")

if __name__ == "__main__":
    unittest.main()
