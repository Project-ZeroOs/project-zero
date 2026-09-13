"""
Project Zero - Stage 2C Multiboot Memory-Map Discovery & Physical Inventory Test Suite

Verifies:
1. Valid synthetic memory-map parsing logic.
2. Multiple memory regions classification (Available RAM vs Reserved).
3. 64-bit physical addresses (> 4 GiB preservation).
4. Zero-length and malformed entry rejection (entry.size < 20, truncated buffer).
5. Integer-overflow protection (base + length > 2^64 - 1).
6. Unsorted entries handling.
7. Adjacent regions handling.
8. Explicit reservation of Project Zero critical regions.
9. Multiboot metadata reservation (MBI structure and mmap buffer).
10. Negative/corrupted memory-map input rejection.
11. Live QEMU boot test validating all Stage 2C serial telemetry markers.
"""

import sys
import struct
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

MULTIBOOT_MAGIC = 0x2BADB002
MULTIBOOT_MEMORY_AVAILABLE = 1
MULTIBOOT_MEMORY_RESERVED = 2
MULTIBOOT_MEMORY_ACPI_RECLAIMABLE = 3
MULTIBOOT_MEMORY_NVS = 4
MULTIBOOT_MEMORY_BADRAM = 5


class MultibootMmapParserSynthetic:
    """
    Python reference model replicating kernel/src/mm/inventory.rs parsing
    and reservation interval subtraction logic for synthetic test verification.
    """

    @staticmethod
    def parse_mmap(buffer: bytes):
        """
        Parses raw bytes representing Multiboot 1 memory map entries.
        Each entry has:
          size: u32 (size of following struct, min 20)
          base_addr: u64
          length: u64
          type: u32
        """
        offset = 0
        total_len = len(buffer)
        regions = []

        while offset < total_len:
            if offset + 4 > total_len:
                # Truncated entry header
                break

            entry_size = struct.unpack_from("<I", buffer, offset)[0]
            if entry_size < 20:
                # Malformed entry size (must at least hold base_addr, length, type)
                break

            if offset + 4 + entry_size > total_len:
                # Truncated entry payload
                break

            base_addr, length, mem_type = struct.unpack_from("<QQI", buffer, offset + 4)

            # Integer overflow check
            if base_addr + length > 0xFFFFFFFFFFFFFFFF:
                # Discard overflow entry
                offset += 4 + entry_size
                continue

            if length == 0:
                # Skip zero-length entry
                offset += 4 + entry_size
                continue

            regions.append({
                "base": base_addr,
                "length": length,
                "end": base_addr + length,
                "type": mem_type,
                "is_available": (mem_type == MULTIBOOT_MEMORY_AVAILABLE),
            })

            offset += 4 + entry_size

        return regions

    @staticmethod
    def subtract_reservations(available_ranges, reservations):
        """
        Subtracts reservation intervals [res.start, res.end) from available intervals.
        """
        current_available = list(available_ranges)

        for res_start, res_end in reservations:
            if res_end <= res_start:
                continue

            next_available = []
            for cur_start, cur_end in current_available:
                # Case 1: No overlap (reservation strictly before)
                if res_end <= cur_start:
                    next_available.append((cur_start, cur_end))
                # Case 2: No overlap (reservation strictly after)
                elif res_start >= cur_end:
                    next_available.append((cur_start, cur_end))
                # Case 3: Reservation completely covers current range
                elif res_start <= cur_start and res_end >= cur_end:
                    continue
                # Case 4: Reservation overlaps start of current range
                elif res_start <= cur_start and res_end < cur_end:
                    next_available.append((res_end, cur_end))
                # Case 5: Reservation overlaps end of current range
                elif res_start > cur_start and res_end >= cur_end:
                    next_available.append((cur_start, res_start))
                # Case 6: Reservation punches a hole in the middle
                elif res_start > cur_start and res_end < cur_end:
                    next_available.append((cur_start, res_start))
                    next_available.append((res_end, cur_end))

            current_available = next_available

        return current_available


class TestMultibootMmap(unittest.TestCase):
    """Synthetic unit tests for Multiboot memory-map parser edge cases."""

    def test_valid_memory_map_parsing(self):
        """Test 1: Valid memory-map parsing with basic available and reserved regions."""
        raw = b"".join([
            struct.pack("<IQQI", 20, 0x0, 0x9FC00, 1),
            struct.pack("<IQQI", 20, 0x9FC00, 0x400, 2),
            struct.pack("<IQQI", 20, 0x100000, 0x7EE0000, 1),
        ])
        regions = MultibootMmapParserSynthetic.parse_mmap(raw)
        self.assertEqual(len(regions), 3)
        self.assertEqual(regions[0]["base"], 0x0)
        self.assertEqual(regions[0]["length"], 0x9FC00)
        self.assertTrue(regions[0]["is_available"])
        self.assertFalse(regions[1]["is_available"])
        self.assertEqual(regions[2]["base"], 0x100000)
        self.assertTrue(regions[2]["is_available"])

    def test_multiple_memory_regions_and_types(self):
        """Test 2 & 3: Multiple memory regions and preserving ACPI/NVS types."""
        raw = b"".join([
            struct.pack("<IQQI", 20, 0x0, 0x1000, 1),
            struct.pack("<IQQI", 20, 0x1000, 0x1000, 2),
            struct.pack("<IQQI", 20, 0x2000, 0x1000, 3), # ACPI Reclaimable
            struct.pack("<IQQI", 20, 0x3000, 0x1000, 4), # ACPI NVS
            struct.pack("<IQQI", 20, 0x4000, 0x1000, 5), # BadRAM
        ])
        regions = MultibootMmapParserSynthetic.parse_mmap(raw)
        self.assertEqual(len(regions), 5)
        self.assertTrue(regions[0]["is_available"])
        self.assertFalse(regions[1]["is_available"])
        self.assertEqual(regions[2]["type"], 3)
        self.assertFalse(regions[2]["is_available"])
        self.assertEqual(regions[3]["type"], 4)
        self.assertFalse(regions[3]["is_available"])
        self.assertEqual(regions[4]["type"], 5)
        self.assertFalse(regions[4]["is_available"])

    def test_64bit_physical_addresses(self):
        """Test 4: 64-bit physical addresses above 4 GiB are preserved without 32-bit truncation."""
        addr_64 = 0x000000FD00000000 # ~1012 GiB PCI reserved region seen in QEMU
        len_64 =  0x0000000300000000 # 12 GiB
        raw = struct.pack("<IQQI", 20, addr_64, len_64, 2)
        regions = MultibootMmapParserSynthetic.parse_mmap(raw)
        self.assertEqual(len(regions), 1)
        self.assertEqual(regions[0]["base"], addr_64)
        self.assertEqual(regions[0]["length"], len_64)
        self.assertEqual(regions[0]["end"], 0x0000010000000000)
        self.assertGreater(regions[0]["base"], 0xFFFFFFFF)

    def test_zero_length_and_malformed_entries(self):
        """Test 5: Zero-length entries and malformed/variable size handling."""
        raw = b"".join([
            struct.pack("<IQQI", 20, 0x1000, 0, 1), # Zero length
            struct.pack("<IQQI", 16, 0x2000, 0x1000, 1), # Invalid size (< 20)
            struct.pack("<IQQI", 24, 0x3000, 0x1000, 1) + b"\x00\x00\x00\x00", # Extended 24B size
        ])
        regions = MultibootMmapParserSynthetic.parse_mmap(raw)
        # Entry 1 skipped (len 0), Entry 2 rejected (size 16 < 20 stops parsing)
        self.assertEqual(len(regions), 0)

        # Test valid extended entry alone
        raw_ext = struct.pack("<IQQI", 24, 0x4000, 0x1000, 1) + b"\xDE\xAD\xBE\xEF"
        regions_ext = MultibootMmapParserSynthetic.parse_mmap(raw_ext)
        self.assertEqual(len(regions_ext), 1)
        self.assertEqual(regions_ext[0]["base"], 0x4000)

    def test_integer_overflow_protection(self):
        """Test 6: Integer-overflow protection when base + length exceeds 2^64 - 1."""
        raw = struct.pack("<IQQI", 20, 0xFFFFFFFFFFFFF000, 0x2000, 1)
        regions = MultibootMmapParserSynthetic.parse_mmap(raw)
        self.assertEqual(len(regions), 0, "Parser must discard entry whose end overflows 64 bits")

    def test_unsorted_and_adjacent_regions(self):
        """Test 7 & 8: Parsing unsorted entries and adjacent contiguous regions."""
        raw = b"".join([
            struct.pack("<IQQI", 20, 0x200000, 0x100000, 1),
            struct.pack("<IQQI", 20, 0x100000, 0x100000, 1), # Adjacent, unsorted
        ])
        regions = MultibootMmapParserSynthetic.parse_mmap(raw)
        self.assertEqual(len(regions), 2)
        self.assertEqual(regions[0]["base"], 0x200000)
        self.assertEqual(regions[1]["base"], 0x100000)

    def test_project_zero_critical_reservations_subtraction(self):
        """Test 9 & 10: Reservation subtraction of Project Zero kernel structures and boot metadata."""
        # Available firmware region: [0x100000, 0x8000000) (1 MiB to 128 MiB)
        available = [(0x100000, 0x8000000)]

        # Project Zero reservations
        reservations = [
            (0x100000, 0x13C000), # Kernel Image
            (0x13D000, 0x14D000), # Normal Stack
            (0x14E000, 0x151000), # Page Tables
            (0x151030, 0x152030), # IDT
            (0x152070, 0x156070), # IST1
            (0x156070, 0x1560A8), # GDT
            (0x1560A8, 0x156110), # TSS
        ]

        usable = MultibootMmapParserSynthetic.subtract_reservations(available, reservations)
        
        # Verify usable ranges do not overlap any reservation
        for u_start, u_end in usable:
            self.assertLess(u_start, u_end)
            for r_start, r_end in reservations:
                overlap = max(u_start, r_start) < min(u_end, r_end)
                self.assertFalse(overlap, f"Usable range [{u_start:#x}, {u_end:#x}) overlaps [{r_start:#x}, {r_end:#x})")

        # Range after TSS up to 128 MiB must be preserved
        self.assertEqual(usable[-1][0], 0x156110)
        self.assertEqual(usable[-1][1], 0x8000000)

    def test_negative_corrupted_memory_map(self):
        """Test 11: Negative test ensuring corrupted or truncated memory maps are rejected."""
        corrupted_bytes = b"\x01\x02\x03\x04\x05" # Incomplete header
        regions = MultibootMmapParserSynthetic.parse_mmap(corrupted_bytes)
        self.assertEqual(len(regions), 0)

    def test_reservation_union_no_double_counting(self):
        """Test 13: Overlapping reservation objects (e.g. MBI and mmap inside low-memory) are not double-counted."""
        # Low memory: [0, 0x100000)
        # MBI inside low memory: [0x9500, 0x9578)
        # MMap inside low memory: [0x9000, 0x90A8)
        # Normal stack outside: [0x13F000, 0x14F000)
        raw_reservations = [
            (0x0, 0x100000),
            (0x9500, 0x9578),
            (0x9000, 0x90A8),
            (0x13F000, 0x14F000),
        ]

        # Compute union of intervals
        disjoint_union = []
        for cur_s, cur_e in raw_reservations:
            s, e = cur_s, cur_e
            next_union = []
            for ex_s, ex_e in disjoint_union:
                if s <= ex_e and e >= ex_s:
                    s = min(s, ex_s)
                    e = max(e, ex_e)
                else:
                    next_union.append((ex_s, ex_e))
            next_union.append((s, e))
            disjoint_union = next_union

        # The union must consist of exactly 2 disjoint intervals: [0, 0x100000) and [0x13F000, 0x14F000)
        self.assertEqual(len(disjoint_union), 2)
        total_union_bytes = sum(e - s for s, e in disjoint_union)
        expected_union_bytes = 0x100000 + (0x14F000 - 0x13F000) # 1 MiB + 64 KiB
        self.assertEqual(total_union_bytes, expected_union_bytes)
        # Verify MBI and mmap did NOT inflate the union
        self.assertLess(total_union_bytes, sum(e - s for s, e in raw_reservations))

    def test_byte_level_accounting_invariant(self):
        """Test 14: Verify Type1_RAM - Intersection(Type1_RAM, Union(Reservations)) == UsableRAM."""
        # Type 1 firmware RAM: [0x0, 0x9FC00) and [0x100000, 0x8000000)
        type1_regions = [(0x0, 0x9FC00), (0x100000, 0x8000000)]
        type1_total = sum(e - s for s, e in type1_regions)

        # Disjoint reservations
        reservations = [
            (0x0, 0x100000),       # covers first region completely and goes to 1 MiB
            (0x100000, 0x140000),   # covers first 256 KiB of second region
        ]

        # Compute intersection of Union(Reservations) with Type 1 RAM
        intersection_bytes = 0
        for t_s, t_e in type1_regions:
            for r_s, r_e in reservations:
                max_s = max(t_s, r_s)
                min_e = min(t_e, r_e)
                if max_s < min_e:
                    intersection_bytes += (min_e - max_s)

        # Usable ranges after subtraction
        usable = MultibootMmapParserSynthetic.subtract_reservations(type1_regions, reservations)
        usable_bytes = sum(e - s for s, e in usable)

        # Invariant check
        self.assertEqual(type1_total - intersection_bytes, usable_bytes)
        self.assertEqual(usable, [(0x140000, 0x8000000)])

    def test_page_alignment_and_subpage_remainder(self):
        """Test 15: Non-aligned usable start aligned upward, non-aligned usable end aligned downward, sub-page remainder computed."""
        # Non-aligned usable interval: [0x100100, 0x200500)
        usable_start = 0x100100
        usable_end = 0x200500
        usable_size = usable_end - usable_start # 0x100400 = 1,049,600 bytes

        # Align start UP: (start + 4095) & !4095
        frame_start = (usable_start + 4095) & ~4095
        self.assertEqual(frame_start, 0x101000) # Aligned up from 0x100100

        # Align end DOWN: end & !4095
        frame_end = usable_end & ~4095
        self.assertEqual(frame_end, 0x200000) # Aligned down from 0x200500

        self.assertLess(frame_start, frame_end)
        frame_candidate_bytes = frame_end - frame_start
        subpage_remainder = usable_size - frame_candidate_bytes

        # Remainder = 0xF00 (at start) + 0x500 (at end) = 0x1400 (5120 bytes)
        self.assertEqual(subpage_remainder, (0x101000 - 0x100100) + (0x200500 - 0x200000))
        self.assertEqual(frame_candidate_bytes % 4096, 0)

    def test_subpage_usable_fragments_rejected_as_frames(self):
        """Test 16: Small sub-page usable fragments (< 4 KiB or crossing single boundary) yield zero frames."""
        # Fragment smaller than 4 KiB within a single page: [0x100010, 0x100500)
        u_start = 0x100010
        u_end = 0x100500
        frame_start = (u_start + 4095) & ~4095 # 0x101000
        frame_end = u_end & ~4095              # 0x100000
        # frame_start > frame_end -> zero frames!
        self.assertFalse(frame_start < frame_end)


class TestStage2CLiveBoot(unittest.TestCase):
    """Live QEMU integration test for Stage 2C Multiboot memory-map discovery and inventory."""

    def test_stage2c_live_qemu_inventory(self):
        """Test 17: Build and boot Stage 2C in QEMU, asserting all discovery, accounting & inventory telemetry."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel binary {image} does not exist.")

        success, failures = run_qemu.test_qemu(image, markers=run_qemu.EXPECTED_STAGE2C_MARKERS)
        self.assertTrue(success, f"Stage 2C live QEMU boot test failed: {failures}")


if __name__ == "__main__":
    unittest.main()

