"""
Project Zero - Stage 2E-A Architecture Foundations Test Suite

Verifies Phase 2E-A acceptance criteria:
1. CPUID geometry discovery (physical bits, linear bits, authoritative mask).
2. Canonical address validation (canonical lower half, canonical higher half, non-canonical hole).
3. Physical address validation against discovered physical mask.
4. NX capability detection and IA32_EFER.NXE enablement.
5. Typed CR3 read/write representation and control flag policy (PCID disabled).
6. 4 KiB-aligned PageTable (512 x 64-bit entries).
7. Safe PageTableEntry address and flag encoding.
8. Reserved-bit protection on leaf and intermediate table entries.
9. Security domain abstraction (MappingDomain::Kernel enforces USER=0).
10. Live QEMU execution asserting Stage 2E-A diagnostics and invariant self-tests.
11. Invariant that active memory mappings remain untouched.
"""

import sys
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

PAGE_SIZE = 4096

class AddressSpaceGeometrySynthetic:
    """Python reference model replicating AddressSpaceGeometry."""
    def __init__(self, phys_bits=40, virt_bits=48, nx_supported=True, nx_enabled=True):
        self.physical_bits = phys_bits
        self.virtual_bits = virt_bits
        self.physical_mask = ((1 << phys_bits) - 1) & ~0xFFF
        self.nx_supported = nx_supported
        self.nx_enabled = nx_enabled

    def is_canonical(self, addr: int) -> bool:
        # In 48-bit mode, bits 63:47 must all be 0 or all be 1
        shift = self.virtual_bits - 1
        # Sign extend bit 47
        if addr & (1 << shift):
            expected_upper = (1 << 64) - (1 << shift)
            return (addr & expected_upper) == expected_upper
        else:
            expected_zero = ~((1 << shift) - 1) & 0xFFFF_FFFF_FFFF_FFFF
            return (addr & expected_zero) == 0

    def is_valid_phys_frame(self, addr: int) -> bool:
        return (addr % PAGE_SIZE == 0) and ((addr & ~self.physical_mask) == 0)


class PageTableFlagsSynthetic:
    PRESENT = 1 << 0
    WRITABLE = 1 << 1
    USER_ACCESSIBLE = 1 << 2
    WRITE_THROUGH = 1 << 3
    CACHE_DISABLE = 1 << 4
    ACCESSED = 1 << 5
    DIRTY = 1 << 6
    HUGE_PAGE = 1 << 7
    GLOBAL = 1 << 8
    NO_EXECUTE = 1 << 63

    @classmethod
    def validate_leaf(cls, flags: int, geometry: AddressSpaceGeometrySynthetic):
        if (flags & cls.NO_EXECUTE) and not geometry.nx_enabled:
            raise ValueError("NO_EXECUTE flag requires NXE enabled")
        if flags & cls.HUGE_PAGE:
            raise ValueError("HUGE_PAGE invalid at 4 KiB leaf entry")

    @classmethod
    def validate_intermediate(cls, flags: int):
        if flags & cls.DIRTY:
            raise ValueError("DIRTY bit is reserved at intermediate paging levels")


class TestStage2EAGeometryAndCanonical(unittest.TestCase):
    def setUp(self):
        self.geometry = AddressSpaceGeometrySynthetic(phys_bits=40, virt_bits=48)

    def test_canonical_address_validation(self):
        """Verify canonical address boundary checks."""
        # Lower half canonical
        self.assertTrue(self.geometry.is_canonical(0x0000_0000_0000_0000))
        self.assertTrue(self.geometry.is_canonical(0x0000_7FFF_FFFF_FFFF))

        # Non-canonical hole
        self.assertFalse(self.geometry.is_canonical(0x0000_8000_0000_0000))
        self.assertFalse(self.geometry.is_canonical(0x0000_FFFF_FFFF_FFFF))
        self.assertFalse(self.geometry.is_canonical(0xFFFF_7FFF_FFFF_FFFF))

        # Higher half canonical
        self.assertTrue(self.geometry.is_canonical(0xFFFF_8000_0000_0000))
        self.assertTrue(self.geometry.is_canonical(0xFFFF_FFFF_FFFF_FFFF))

    def test_physical_address_mask_enforcement(self):
        """Verify 40-bit physical address mask rejects out-of-range addresses."""
        # Valid 40-bit frame
        valid_frame = 0x0000_00FF_FFFF_F000
        self.assertTrue(self.geometry.is_valid_phys_frame(valid_frame))

        # Misaligned frame
        self.assertFalse(self.geometry.is_valid_phys_frame(0x0000_00FF_FFFF_F001))

        # Bit 40 set (exceeds 40-bit physical width)
        out_of_range = 0x0000_0100_0000_0000
        self.assertFalse(self.geometry.is_valid_phys_frame(out_of_range))


class TestStage2EAPageTableFlags(unittest.TestCase):
    def test_leaf_flag_validation(self):
        """Verify leaf PTE flag validation and NX protection."""
        geo_nx_on = AddressSpaceGeometrySynthetic(nx_enabled=True)
        geo_nx_off = AddressSpaceGeometrySynthetic(nx_enabled=False)

        valid_flags = PageTableFlagsSynthetic.PRESENT | PageTableFlagsSynthetic.WRITABLE
        PageTableFlagsSynthetic.validate_leaf(valid_flags, geo_nx_on)

        # Huge page bit at leaf level is rejected
        huge_flags = valid_flags | PageTableFlagsSynthetic.HUGE_PAGE
        with self.assertRaises(ValueError):
            PageTableFlagsSynthetic.validate_leaf(huge_flags, geo_nx_on)

        # NO_EXECUTE when NXE is disabled is rejected
        nx_flags = valid_flags | PageTableFlagsSynthetic.NO_EXECUTE
        with self.assertRaises(ValueError):
            PageTableFlagsSynthetic.validate_leaf(nx_flags, geo_nx_off)

        # NO_EXECUTE when NXE is enabled succeeds
        PageTableFlagsSynthetic.validate_leaf(nx_flags, geo_nx_on)

    def test_intermediate_flag_validation(self):
        """Verify intermediate paging entries reject reserved bits."""
        valid_inter = PageTableFlagsSynthetic.PRESENT | PageTableFlagsSynthetic.WRITABLE
        PageTableFlagsSynthetic.validate_intermediate(valid_inter)

        # Dirty bit reserved at intermediate levels
        dirty_inter = valid_inter | PageTableFlagsSynthetic.DIRTY
        with self.assertRaises(ValueError):
            PageTableFlagsSynthetic.validate_intermediate(dirty_inter)


class TestStage2EALiveQemu(unittest.TestCase):
    def test_stage2ea_boot_diagnostics(self):
        """Build and execute Project Zero in QEMU, verifying Stage 2E-A diagnostics and self-tests."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel binary {image} does not exist.")

        markers = [
            "[Stage 2E-A: Architecture Foundations (Geometry, CR3 & Typed Tables)]",
            "Physical Address Width:",
            "Linear/Virtual Width:",
            "No-Execute (NX) Status:   Supported: true, Enabled: true (IA32_EFER.NXE)",
            "Active CR3 Physical Root:",
            "CR3 Control Flags:        PWT=false, PCD=false, PCID=disabled",
            "Page Table Geometry:      512 entries x 8 bytes = 4096 bytes (aligned: true)",
            "Security Domain Policy:   Kernel mappings enforce USER=0 throughout hierarchy",
            "Active Mapping State:     Untouched (Phase 2E-A instrument verification only)",
            "[x] Stage 2E-A architectural instruments & invariants verified.",
            "[Stage 2E-A Architectural Status & Verification Summary]",
            "Hardware address geometry discovered via CPUID",
            "IA32_EFER.NXE enabled (No-Execute page execution protection active).",
            "Strongly typed CR3 read/write abstraction verified with geometry mask.",
            "4 KiB-aligned PageTable and PageTableEntry structures verified.",
            "Security domain policy enforces USER=0 throughout kernel hierarchies.",
            "Active memory mappings remain completely untouched (Phase 2E-A instruments)."
        ]

        success, failures = run_qemu.test_qemu(image, markers=markers)
        self.assertTrue(success, f"Stage 2E-A QEMU execution failed: {failures}")


if __name__ == "__main__":
    unittest.main()
