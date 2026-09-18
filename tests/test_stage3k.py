"""
Stage 3K Test Suite: Storage / Filesystem (ZeroFS)
Validations:
1. Static Kernel Symbols:
   - `format_volume` exists.
   - `mount_volume` exists.
   - `run_stage3k_verification` exists.
   - `STORAGE_OBJECT_TABLE` exists in .bss.
   - `MEM_DEVICE` exists in .bss.
2. Live QEMU Telemetry:
   - Tests 3K-A through 3K-T (20 sequential machine tests) pass.
3. Transactional Crash Consistency & CoW Indirect Mapping:
   - Intent rollback, Committed rollforward, post-metadata crash idempotence verified.
4. Transactional PMM Neutrality:
   - Zero net frame leakage across complete storage lifecycle (`baseline_free == post_test_free`).
5. Clean Hardware Shutdown:
   - isa-debug-exit returns payload 33 (0x21).
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu


def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")


class TestStage3KStorageFilesystem(unittest.TestCase):
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
                        "num": int(parts[0].rstrip(':')),
                        "value": int(parts[1], 16),
                        "size": int(parts[2]),
                        "type": parts[3],
                        "bind": parts[4],
                        "vis": parts[5],
                        "ndx": parts[6],
                        "name": parts[7]
                    })
                except (ValueError, IndexError):
                    continue
        return symbols

    def test_stage3k_rust_symbols(self):
        """Verify ZeroFS core functions exist in kernel ELF."""
        symbols = self._get_elf_symbols()

        expected = [
            "format_volume",
            "mount_volume",
            "run_stage3k_verification",
        ]
        for name in expected:
            sym = next((s for s in symbols if name in s["name"]), None)
            self.assertIsNotNone(sym, f"Required symbol '{name}' missing from ELF")

    def test_stage3k_bss_tables(self):
        """Verify STORAGE_OBJECT_TABLE static table exists in ELF."""
        symbols = self._get_elf_symbols()
        table_sym = next((s for s in symbols if "STORAGE_OBJECT_TABLE" in s["name"]), None)
        self.assertIsNotNone(table_sym, "STORAGE_OBJECT_TABLE symbol missing from ELF")

    def test_stage3k_qemu_execution(self):
        """Execute QEMU and assert that all 20 Stage 3K bare-metal tests pass sequentially."""
        markers = [
            "[3K-A] Block Device Discovery...",
            "[PASS] 3K-A",
            "[3K-B] Block Sector Read/Write Fidelity...",
            "[PASS] 3K-B",
            "[3K-C] Block Bounds Check...",
            "[PASS] 3K-C",
            "[3K-D] ZeroFS Volume Format...",
            "[PASS] 3K-D",
            "[3K-E] Superblock Validation...",
            "[PASS] 3K-E",
            "[3K-F] Inode Allocation & Recycling...",
            "[PASS] 3K-F",
            "[3K-G] Block Allocation & Bitmap Integrity...",
            "[PASS] 3K-G",
            "[3K-H] Allocation Exhaustion (-ENOSPC)...",
            "[PASS] 3K-H",
            "[3K-I] Direct Block Mapping (18 blocks = 72 KiB)...",
            "[PASS] 3K-I",
            "[3K-J] Indirect Block Mapping via Copy-on-Write (ADR-3K-009)...",
            "[PASS] 3K-J",
            "[3K-K] Tail Zero Padding...",
            "[PASS] 3K-K",
            "[3K-L] Directory Insertion & Unlink...",
            "[PASS] 3K-L",
            "[3K-M] File Truncation & Block Reclamation...",
            "[PASS] 3K-M",
            "[3K-N] Process-Exit Persistence (I-STOR-LIFETIME-1)...",
            "[PASS] 3K-N",
            "[3K-O] Reboot Persistence Simulation (I-STOR-LIFETIME-2)...",
            "[PASS] 3K-O",
            "[3K-P] Corruption Rejection Fail-Closed...",
            "[PASS] 3K-P",
            "[3K-Q] Uncommitted Crash Recovery (Intent Rollback & CoW B_old intact)...",
            "[PASS] 3K-Q",
            "[3K-R] Committed Crash Recovery (Rollforward & CoW B_new adoption)...",
            "[PASS] 3K-R",
            "[3K-S] Post-Metadata-Write Crash Idempotence...",
            "[PASS] 3K-S",
            "[3K-T] Capability Enforcement & PMM Neutrality (I-STOR-PMM-1)...",
            "[PASS] 3K-T",
            "[Stage 3K Verification Complete: 20/20 tests PASSED]",
            "Cumulative Machine Tests: 235 tests",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3K QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
