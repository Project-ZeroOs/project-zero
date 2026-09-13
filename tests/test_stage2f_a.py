"""
Stage 2F-A Test Suite: Physical Memory Disjointness & ELF Higher-Half Layout Verification
Validates:
1. ELF header entry point e_entry == 0x00101000 (physical bootstrap entry point).
2. All LOAD program headers satisfy VMA = LMA + 0xFFFFFFFF80000000.
3. Authoritative linker symbols / section headers are extracted:
   - .bss
   - .pmm_metadata
   - .page_tables
   - .stack_guard
   - .stack
4. Explicit proof of pairwise disjointness:
   - .bss ∩ .stack = ∅
   - .bss ∩ .stack_guard = ∅
   - .bss ∩ .page_tables = ∅
   - .bss ∩ .pmm_metadata = ∅
   - .stack_guard ∩ .stack = ∅
   - .stack_guard.end == .stack.start
   - Complete bootstrap reservation set is pairwise disjoint.
5. Live QEMU boot telemetry verifies that kernel-internal assertion passes.
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu

KERNEL_VIRT_OFFSET = 0xFFFF_FFFF_8000_0000

class Interval:
    def __init__(self, name: str, start: int, end: int):
        assert start <= end, f"Invalid interval for {name}: [0x{start:X}, 0x{end:X})"
        self.name = name
        self.start = start
        self.end = end

    def overlaps(self, other: "Interval") -> bool:
        return max(self.start, other.start) < min(self.end, other.end)

    def intersection(self, other: "Interval"):
        s = max(self.start, other.start)
        e = min(self.end, other.end)
        if s < e:
            return (s, e)
        return None

    def __repr__(self):
        return f"{self.name}: [0x{self.start:08X}, 0x{self.end:08X}) (len={self.end - self.start} / 0x{self.end - self.start:X})"

def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")

class TestStage2FAPhysicalLayout(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.kernel32_elf = run_qemu.build_stage2()
        cls.kernel_elf = PROJECT_ROOT / "build" / "kernel.elf"
        cls.readelf = get_readelf_path()

    def test_elf_entry_point(self):
        """Verify e_entry is the physical bootstrap entry (0x00101000)."""
        res = subprocess.run(
            [self.readelf, "-hW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        )
        entry_line = [line for line in res.stdout.splitlines() if "Entry point address:" in line]
        self.assertTrue(entry_line, "Entry point address not found in readelf output")
        entry_addr = int(entry_line[0].split()[-1], 16)
        self.assertEqual(entry_addr, 0x101000, f"Expected e_entry 0x101000, got 0x{entry_addr:X}")

    def test_load_segment_vma_lma_offset(self):
        """Verify all LOAD segments maintain VMA = LMA + 0xFFFFFFFF80000000."""
        res = subprocess.run(
            [self.readelf, "-lW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        )
        load_segments = []
        for line in res.stdout.splitlines():
            parts = line.strip().split()
            if len(parts) >= 6 and parts[0] == "LOAD":
                vaddr = int(parts[2], 16)
                paddr = int(parts[3], 16)
                filesz = int(parts[4], 16)
                memsz = int(parts[5], 16)
                load_segments.append((vaddr, paddr, filesz, memsz))

        self.assertTrue(load_segments, "No LOAD segments found")
        for vaddr, paddr, filesz, memsz in load_segments:
            expected_vaddr = paddr + KERNEL_VIRT_OFFSET
            self.assertEqual(
                vaddr, expected_vaddr,
                f"LOAD segment VMA 0x{vaddr:X} != expected LMA + offset 0x{expected_vaddr:X}"
            )

    def test_section_pairwise_disjointness(self):
        """Verify .bss, .pmm_metadata, .page_tables, .stack_guard, and .stack are strictly disjoint."""
        res = subprocess.run(
            [self.readelf, "-SW", str(self.kernel_elf)],
            capture_output=True, text=True, check=True
        )
        sections = {}
        for line in res.stdout.splitlines():
            parts = line.replace("[", " ").replace("]", " ").split()
            if len(parts) >= 6 and parts[0].isdigit():
                sec_name = parts[1]
                try:
                    vma = int(parts[3], 16)
                    size = int(parts[5], 16)
                    lma = vma - KERNEL_VIRT_OFFSET if vma >= KERNEL_VIRT_OFFSET else vma
                    sections[sec_name] = Interval(sec_name, lma, lma + size)
                except (ValueError, IndexError):
                    continue

        required_sections = [".text", ".rodata", ".data", ".bss", ".pmm_metadata", ".page_tables", ".stack_guard", ".stack"]
        for sec in required_sections:
            self.assertIn(sec, sections, f"Missing section {sec} in ELF")

        bss = sections[".bss"]
        pmm_meta = sections[".pmm_metadata"]
        page_tables = sections[".page_tables"]
        guard = sections[".stack_guard"]
        stack = sections[".stack"]

        print("\n--- Physical Layout Intervals ---")
        for sec in required_sections:
            print(f"  {sections[sec]}")

        # 1. Primary User Invariant: .bss ∩ stack = ∅
        self.assertFalse(bss.overlaps(stack), f"CRITICAL BUG: .bss overlaps .stack! Intersection: {bss.intersection(stack)}")
        # 2. .bss ∩ guard = ∅
        self.assertFalse(bss.overlaps(guard), f"CRITICAL BUG: .bss overlaps .stack_guard! Intersection: {bss.intersection(guard)}")
        # 3. .bss ∩ page_tables = ∅
        self.assertFalse(bss.overlaps(page_tables), f"CRITICAL BUG: .bss overlaps .page_tables! Intersection: {bss.intersection(page_tables)}")
        # 4. .bss ∩ pmm_metadata = ∅
        self.assertFalse(bss.overlaps(pmm_meta), f"CRITICAL BUG: .bss overlaps .pmm_metadata! Intersection: {bss.intersection(pmm_meta)}")
        # 5. guard ∩ stack = ∅
        self.assertFalse(guard.overlaps(stack), f"CRITICAL BUG: .stack_guard overlaps .stack! Intersection: {guard.intersection(stack)}")

        # 6. Guard page adjacency: Guard immediately precedes stack
        self.assertEqual(guard.end, stack.start, f"Guard page end (0x{guard.end:X}) does not match stack start (0x{stack.start:X})")

        # 7. Stack guard size is 4 KiB
        self.assertEqual(guard.end - guard.start, 4096, "Stack guard page size is not 4096 bytes")

        # 8. Stack size is 64 KiB
        self.assertEqual(stack.end - stack.start, 65536, "Stack size is not 65536 bytes")

        # 9. Page tables size is 16 KiB (PML4, PDPT, PD, HHDM_PDPT)
        self.assertEqual(page_tables.end - page_tables.start, 16384, "Page tables size is not 16384 bytes")

        # 10. Pairwise disjointness across all kernel sections
        all_secs = [sections[s] for s in required_sections]
        for i in range(len(all_secs)):
            for j in range(i + 1, len(all_secs)):
                s1 = all_secs[i]
                s2 = all_secs[j]
                self.assertFalse(
                    s1.overlaps(s2),
                    f"Overlapping sections detected: {s1.name} [0x{s1.start:X}, 0x{s1.end:X}) and {s2.name} [0x{s2.start:X}, 0x{s2.end:X})"
                )

    def test_qemu_kernel_layout_assertions(self):
        """Run QEMU to verify kernel-internal assertion output."""
        markers = [
            "[x] Stage 2F-A physical layout verified: .bss, pmm_meta, guard, stack, and page tables strictly disjoint."
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Kernel failed to verify Stage 2F-A physical layout in QEMU: {failures}")

if __name__ == "__main__":
    unittest.main()
