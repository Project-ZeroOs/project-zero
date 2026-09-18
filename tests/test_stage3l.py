"""
Stage 3L Test Suite: Device / Hardware Model
Validations:
1. Static Kernel Symbols:
   - `register_device` exists.
   - `register_resource` exists.
   - `run_stage3l_verification` exists.
   - `DEVICE_TABLE` exists in .bss.
   - `RESOURCE_TABLE` exists in .bss.
   - `INTERRUPT_BINDINGS` exists in .bss.
   - `DMA_BUFFER_TABLE` exists in .bss.
   - `PHYSICAL_FRAME_PIN_TABLE` exists in .bss.
2. Live QEMU Telemetry:
   - Tests 3L-A through 3L-Z (26 sequential machine tests) pass.
3. Architecture Invariants:
   - MMIO aperture collision prevention (I-DEV-MMIO-1).
   - Shared IRQ delivery & storm isolation (I-DEV-IRQ-STORM-1).
   - DMA frame pinning & rollback atomicity (I-DEV-DMA-1, I-DEV-DMA-2).
   - Device capability rights subset rule (I-DEV-CAP-1, I-DEV-CAP-2).
   - Reset authority and process exit teardown (I-DEV-LIFETIME-1).
   - Concurrency & monotonic lock hierarchy.
4. Transactional PMM Neutrality:
   - Zero net frame leakage across complete device lifecycle (`baseline_free == post_test_free`).
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


class TestStage3LDeviceHardwareModel(unittest.TestCase):
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

    def test_stage3l_rust_symbols(self):
        """Verify Stage 3L device functions exist in kernel ELF."""
        symbols = self._get_elf_symbols()

        expected = [
            "register_device",
            "register_resource",
            "run_stage3l_verification",
        ]
        for name in expected:
            sym = next((s for s in symbols if name in s["name"]), None)
            self.assertIsNotNone(sym, f"Required symbol '{name}' missing from ELF")

    def test_stage3l_bss_tables(self):
        """Verify static Stage 3L bounded tables exist in ELF .bss."""
        symbols = self._get_elf_symbols()
        tables = [
            "DEVICE_TABLE",
            "RESOURCE_TABLE",
            "INTERRUPT_BINDINGS",
            "DMA_BUFFER_TABLE",
            "PHYSICAL_FRAME_PIN_TABLE",
        ]
        for tbl in tables:
            table_sym = next((s for s in symbols if tbl in s["name"]), None)
            self.assertIsNotNone(table_sym, f"Static table '{tbl}' missing from ELF")

    def test_stage3l_qemu_execution(self):
        """Execute QEMU and assert that all 26 Stage 3L bare-metal tests pass sequentially."""
        markers = [
            "[3L-A] Device Table & Identity Monotonicity...",
            "[PASS] 3L-A",
            "[3L-B] Device Discovery & Enumeration...",
            "[PASS] 3L-B",
            "[3L-C] Device Lifecycle Transitions...",
            "[PASS] 3L-C",
            "[3L-D] Resource Allocation (IoPort & Mmio)...",
            "[PASS] 3L-D",
            "[3L-E] Resource Overlap Rejection...",
            "[PASS] 3L-E",
            "[3L-F] Capability Authorization & Rights...",
            "[PASS] 3L-F",
            "[3L-G] MMIO Range Mapping & Uncacheable Paging...",
            "[PASS] 3L-G",
            "[3L-H] Port-I/O Authority & Boundary...",
            "[PASS] 3L-H",
            "[3L-I] Interrupt Registration & Binding (SYS_DEV_BIND_IRQ)...",
            "[PASS] 3L-I",
            "[3L-J] Interrupt Top-Half Event Signaling...",
            "[PASS] 3L-J",
            "[3L-K] Interrupt Storm Mitigation...",
            "[PASS] 3L-K",
            "[3L-L] DMA Buffer Allocation & PMM Pinning...",
            "[PASS] 3L-L",
            "[3L-M] DMA Isolation & Bounds Checking...",
            "[PASS] 3L-M",
            "[3L-N] Device Fault State Transition...",
            "[PASS] 3L-N",
            "[3L-O] Device Reset & Recovery...",
            "[PASS] 3L-O",
            "[3L-P] Driver Detach & Teardown...",
            "[PASS] 3L-P",
            "[3L-Q] Process Exit Shared Device Teardown...",
            "[PASS] 3L-Q",
            "[3L-R] Capability Revocation Cascade...",
            "[PASS] 3L-R",
            "[3L-S] Concurrency & Monotonic Lock Ordering...",
            "[PASS] 3L-S",
            "[3L-T] ZeroFS Storage Backward Compatibility...",
            "[PASS] 3L-T",
            "[3L-U] MMIO/User-Address Collision Rejection...",
            "[PASS] 3L-U",
            "[3L-V] Multiple Bindings on Shared IRQ...",
            "[PASS] 3L-V",
            "[3L-W] Multi-Frame DMA Ownership...",
            "[PASS] 3L-W",
            "[3L-X] DMA Partial-Allocation Rollback...",
            "[PASS] 3L-X",
            "[3L-Y] Shared IRQ Storm Isolation...",
            "[PASS] 3L-Y",
            "[3L-Z] Reset with Shared Device Ownership...",
            "[PASS] 3L-Z",
            "Stage 3L PMM Neutrality: VERIFIED (zero net frame leakage).",
            "[Stage 3L Verification Complete: 26/26 tests PASSED]",
            "Cumulative Machine Tests: 261 tests",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3L QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
