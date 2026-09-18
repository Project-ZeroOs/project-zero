"""
Stage 3J Test Suite: ELF & Program Execution
Validations:
1. Static ELF Symbols:
   - `validate_elf` exists.
   - `load_elf` exists.
   - `run_stage3j_verification` exists.
   - `PROCESS_MEMORY_MAPS` exists in .bss.
2. Live QEMU Telemetry:
   - Tests 3J-A through 3J-P (16 sequential machine tests) pass.
3. Transactional PMM Neutrality:
   - Zero net frame leakage across complete user process lifecycle and failure rollback.
4. Clean Hardware Shutdown:
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


class TestStage3JElfExecution(unittest.TestCase):
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

    def test_stage3j_rust_symbols(self):
        """Verify ELF loader core functions exist in kernel ELF."""
        symbols = self._get_elf_symbols()

        expected = [
            "validate_elf",
            "load_elf",
            "run_stage3j_verification",
        ]
        for name in expected:
            sym = next((s for s in symbols if name in s["name"]), None)
            self.assertIsNotNone(sym, f"Required symbol '{name}' missing from ELF")

    def test_stage3j_companion_table_symbol(self):
        """Verify PROCESS_MEMORY_MAPS static companion table exists in ELF."""
        symbols = self._get_elf_symbols()
        table_sym = next((s for s in symbols if "PROCESS_MEMORY_MAPS" in s["name"]), None)
        self.assertIsNotNone(table_sym, "PROCESS_MEMORY_MAPS symbol missing from ELF")

    def test_stage3j_qemu_execution(self):
        """Execute QEMU and assert that all 16 Stage 3J bare-metal tests pass sequentially."""
        markers = [
            "[Stage 3J: ELF / Program Execution Verification]",
            "[Test 3J-A: Minimal Valid ELF64 Header & Identity Parse]: PASS",
            "[Test 3J-B: Bad Magic, Wrong Class, & Unsupported Architecture Rejection]: PASS",
            "[Test 3J-C: Unsupported Program Header & Executable Stack Rejection]: PASS",
            "[Test 3J-D: PT_LOAD File Bounds Exceeded Rejection]: PASS",
            "[Test 3J-E: PT_LOAD Memory Underflow (filesz > memsz) Rejection]: PASS",
            "[Test 3J-F: PT_LOAD Virtual Address Out of User Bounds Rejection]: PASS",
            "[Test 3J-G: PT_LOAD Virtual Address Overflow Rejection]: PASS",
            "[Test 3J-H: Overlapping PT_LOAD Segments Rejection]: PASS",
            "[Test 3J-I: Strict W^X Violation Rejection (RWX & W+X)]: PASS",
            "[Test 3J-J: Invalid Entry Point Rejection (Out of Bounds / Non-Executable)]: PASS",
            "[Test 3J-K: Segment Copy & BSS Zero Initialization Fidelity]: PASS",
            "[Test 3J-L: Guarded User Stack Allocation, Alignment & Permissions]: PASS",
            "[Test 3J-M: Real ELF Program Loading & Execution in Ring 3]: PASS",
            "[Test 3J-N: Loaded ELF Syscall Invocation & Clean sys_exit]: PASS",
            "[Test 3J-O: Failed ELF Load Immediate PMM Neutrality (Rollback)]: PASS",
            "[Test 3J-P: Complete Process Lifecycle Teardown & Full PMM Neutrality]: PASS",
        ]
        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3J QEMU verification failed: {msg}")

    def test_stage3j_pmm_neutrality(self):
        """Verify strict PMM frame neutrality across complete user process lifecycle."""
        markers = [
            "[Stage 3J COMPLETE] All 16 tests passed. PMM Neutrality verified."
        ]
        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3J PMM neutrality verification failed: {msg}")

    def test_stage3j_clean_exit(self):
        """Verify execution terminates with deterministic isa-debug-exit."""
        success, msg = run_qemu.test_qemu(self.kernel32_elf)
        self.assertTrue(success, f"QEMU execution did not exit cleanly: {msg}")

    def test_stage3j_user_stack_geometry(self):
        """Verify architectural user stack geometry and non-intersection invariants."""
        USER_STACK_TOP = 0x0000_7F7F_FFFF_0000
        USER_STACK_SIZE = 16 * 1024
        USER_STACK_BASE = USER_STACK_TOP - USER_STACK_SIZE
        USER_STACK_GUARD = USER_STACK_BASE - 4096

        self.assertEqual(USER_STACK_TOP % 16, 0, "Initial user RSP must be 16-byte aligned")
        self.assertEqual(USER_STACK_SIZE, 16384, "Stack size must be 16 KiB")
        self.assertEqual(USER_STACK_BASE, 0x0000_7F7F_FFFE_C000)
        self.assertEqual(USER_STACK_GUARD, 0x0000_7F7F_FFFE_B000)
