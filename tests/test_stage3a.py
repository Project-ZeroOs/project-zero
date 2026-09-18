"""
Stage 3A Test Suite: Execution Primitives & Guarded Kernel Stacks
Validates:
1. Static ELF Symbols & Structure:
   - `BSP_PERCPU` object symbol exists with exact size 40 bytes and 8-byte alignment.
   - `THREAD_TABLE` object symbol exists with exact size 1792 bytes (16 slots * 112 bytes) and 8-byte alignment.
   - `thread_bootstrap_entry` global trampoline symbol exists in executable .text.
2. QEMU Live Telemetry:
   - Descriptor table allocation verified with stable virtual addresses, unique IDs, and 8-byte alignment.
   - Dedicated Kernel Stack Arena verified:
     * Located within [0xFFFFFFFF90000000, 0xFFFFFFFFA0000000).
     * Guard page unmapped (is_mapped == false).
     * 16 KiB usable stack mapped with PRESENT | WRITABLE | NO_EXECUTE and USER=0.
     * Non-overlapping virtual ranges between distinct stack allocations.
   - Transactional rollback verified:
     * PMM frame count completely restored after simulated PMM exhaustion.
     * Partial VMM mapping rolled back and PMM frames restored to exact baseline.
   - CPU-observable guard page #PF trap & recovery:
     * Vector 14 (#PF) intercepted.
     * Faulting CR2 matches guard page virtual address exactly (0xFFFFFFFF90000000).
     * Error code non-present bit 0 is 0.
     * Safe recovery to higher-half execution without kernel panic.
   - BSP PerCpu initialization:
     * IA32_GS_BASE programmed with BSP PerCpu pointer.
     * GS-relative dereference at offset 16 yields valid stable pointer to BSP KernelThread.
   - Forged initial cooperative frame:
     * Matches exact `boot/context.asm` ABI.
     * saved_rsp == stack_top - 64.
     * RFLAGS = 0x0202, R12 = argument, RBX = entry function, Return RIP = thread_bootstrap_entry.
   - Stage 3A verification passed completion banner.
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu

STACK_ARENA_START = 0xFFFF_FFFF_9000_0000
STACK_ARENA_END   = 0xFFFF_FFFF_A000_0000
PERCPU_SIZE       = 48  # Stage 3C ABI extension: added need_resched (4B) + _pad (4B)
THREAD_TABLE_SIZE = 2944  # Stage 3E ABI extension: 16 slots * 184 bytes (176B KernelThread + 8B slot header) = 2944


def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")


class TestStage3AExecutionPrimitives(unittest.TestCase):
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
        symbols = []
        for line in res.stdout.splitlines():
            parts = line.strip().split()
            if len(parts) >= 8 and parts[0].rstrip(':').isdigit():
                try:
                    symbols.append({
                        "name": parts[7],
                        "addr": int(parts[1], 16),
                        "size": int(parts[2]),
                        "type": parts[3],
                        "bind": parts[4],
                    })
                except ValueError:
                    continue
        return symbols

    def test_elf_static_symbols(self):
        """Verify Stage 3A symbols exist in ELF with expected size, type, and alignment."""
        symbols = self._get_elf_symbols()

        bsp_percpu = next((s for s in symbols if "BSP_PERCPU" in s["name"]), None)
        self.assertIsNotNone(bsp_percpu, "BSP_PERCPU static symbol missing from ELF")
        self.assertEqual(bsp_percpu["size"], PERCPU_SIZE, f"BSP_PERCPU size must be {PERCPU_SIZE} bytes")
        self.assertEqual(bsp_percpu["addr"] % 8, 0, "BSP_PERCPU address must be 8-byte aligned")

        thread_table = next((s for s in symbols if "THREAD_TABLE" in s["name"]), None)
        self.assertIsNotNone(thread_table, "THREAD_TABLE static symbol missing from ELF")
        self.assertEqual(thread_table["size"], THREAD_TABLE_SIZE, f"THREAD_TABLE size must be {THREAD_TABLE_SIZE} bytes")
        self.assertEqual(thread_table["addr"] % 8, 0, "THREAD_TABLE address must be 8-byte aligned")

        trampoline = next((s for s in symbols if s["name"] == "thread_bootstrap_entry"), None)
        self.assertIsNotNone(trampoline, "thread_bootstrap_entry symbol missing from ELF")
        self.assertEqual(trampoline["type"], "FUNC", "thread_bootstrap_entry must be a function")
        self.assertGreaterEqual(trampoline["addr"], 0xFFFF_FFFF_8000_0000, "thread_bootstrap_entry must be in higher-half VMA")

    def test_qemu_stage3a_telemetry(self):
        """Execute QEMU and verify Stage 3A telemetry, guard-page #PF, and ABI verification."""
        markers = [
            "[Stage 3A: Execution Primitives & Guarded Stacks Verification]",
            "[x] Static descriptor table verified (stable addresses, unique IDs, 8-byte aligned).",
            "[x] Dedicated Kernel Stack Arena verified (guard is_mapped=false, 16 KiB stack RW+NX+supervisor).",
            "[x] Transactional rollback verified: exact PMM accounting restored on partial failure.",
            "[CONTROLLED EXCEPTION TRAP] Vector 14 (Page Fault (#PF)) trapped at RIP:",
            f"CR2 Fault Address: 0x{STACK_ARENA_START:016X}",
            "Error Code:        0x0000000000000000 (Non-present Read Kernel)",
            "[x] Controlled Guard Page #PF trapped with CR2 and non-present code; recovered successfully.",
            "[x] BSP PerCpu initialized via IA32_GS_BASE; GS+16 dereference verified.",
            "[x] Forged initial cooperative frame verified matching boot/context.asm ABI.",
            "[Stage 3A Execution Primitives Complete]",
            "* Static KernelThread table established with stable addresses.",
            "* Dedicated Kernel Stack Arena active over [0xFFFFFFFF90000000, 0xFFFFFFFFA0000000).",
            "* 16 KiB stacks with 4 KiB unmapped guards backed transactionally by PMM/VMM.",
            "* Controlled Guard Page #PF verified with Vector 14 and exact CR2.",
            "* BSP PerCpu initialized via IA32_GS_BASE (offset 16 verified).",
            "* Forged initial cooperative activation frame verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3A QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


if __name__ == "__main__":
    unittest.main()
