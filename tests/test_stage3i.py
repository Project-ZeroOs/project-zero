"""
Stage 3I Test Suite: User Space & System Call Interface
Validations:
1. Static ELF Symbols (Assembly):
   - `syscall_entry` exists.
   - `user_thread_bootstrap_trampoline` exists.
   - `SYSCALL_SCRATCH_USER_RSP` exists (128 bytes in .bss).
2. Static ELF Symbols (Rust Core):
   - `init_syscall_hardware` exists.
   - `syscall_dispatch_rust` exists.
   - `syscall_validate_return_rust` exists.
   - `syscall_fail_closed_terminate` exists.
   - `run_stage3i_verification` exists.
3. User Payload Static Embedding:
   - `USER_INIT_PAYLOAD` exists and is embedded.
4. Live QEMU Telemetry:
   - Tests 3I-A through 3I-R (18 sequential machine tests) pass.
5. PMM Neutrality:
   - Zero net frame leakage across complete user process lifecycle.
6. Clean Hardware Shutdown:
   - isa-debug-exit returns code 33.
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


class TestStage3IUserSpaceSyscalls(unittest.TestCase):
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

    def test_stage3i_assembly_symbols(self):
        """Verify assembly entry, SMP scratch storage, and trampoline symbols in ELF."""
        symbols = self._get_elf_symbols()

        # 1. syscall_entry
        sym = next((s for s in symbols if s["name"] == "syscall_entry"), None)
        self.assertIsNotNone(sym, "syscall_entry symbol missing from ELF")

        # 2. user_thread_bootstrap_trampoline
        trampoline = next((s for s in symbols if s["name"] == "user_thread_bootstrap_trampoline"), None)
        self.assertIsNotNone(trampoline, "user_thread_bootstrap_trampoline symbol missing from ELF")

        # 3. SYSCALL_SCRATCH_USER_RSP (16 CPUs * 8 bytes = 128 bytes)
        scratch = next((s for s in symbols if s["name"] == "SYSCALL_SCRATCH_USER_RSP"), None)
        self.assertIsNotNone(scratch, "SYSCALL_SCRATCH_USER_RSP symbol missing from ELF")
        self.assertEqual(scratch["size"], 128, "SYSCALL_SCRATCH_USER_RSP must be 128 bytes (16 * 8 B)")

    def test_stage3i_rust_symbols(self):
        """Verify Rust dispatch, MSR init, and verification suite symbols in ELF."""
        symbols = self._get_elf_symbols()

        expected = [
            "init_syscall_hardware",
            "syscall_dispatch_rust",
            "syscall_validate_return_rust",
            "syscall_fail_closed_terminate",
            "run_stage3i_verification",
        ]
        for name in expected:
            sym = next((s for s in symbols if name in s["name"]), None)
            self.assertIsNotNone(sym, f"Required symbol '{name}' missing from ELF")

    def test_stage3i_user_payload_symbol(self):
        """Verify embedded user bootstrap payload symbol exists."""
        symbols = self._get_elf_symbols()
        payload = next((s for s in symbols if "USER_INIT_PAYLOAD" in s["name"]), None)
        self.assertIsNotNone(payload, "USER_INIT_PAYLOAD symbol missing from ELF")

    def test_stage3i_qemu_execution(self):
        """Execute QEMU and assert that all 18 Stage 3I bare-metal tests pass sequentially."""
        markers = [
            "[Stage 3I: User Space & System Call Interface Verification]",
            "[Test 3I-A: Fast Syscall MSR Configuration]: PASS",
            "[Test 3I-B: User Process & Address Space Isolation]: PASS",
            "[Test 3I-C: Strict W^X User Memory Permissions]: PASS",
            "[Test 3I-D: Initial Privilege Transition via iretq Trampoline]: PASS",
            "[Test 3I-E: User Space Execution in Ring 3]: PASS",
            "[Test 3I-F: Fast Syscall Hardware Transition & Context Capture]: PASS",
            "[Test 3I-G: sys_yield Cooperative Scheduling]: PASS",
            "[Test 3I-H: Unknown Syscall Rejection (-ENOSYS)]: PASS",
            "[Test 3I-I: User Pointer Null Guard Page Rejection (-EFAULT)]: PASS",
            "[Test 3I-J: Kernel Address Space Rejection (-EFAULT)]: PASS",
            "[Test 3I-K: Non-Canonical User Pointer Rejection (-EFAULT)]: PASS",
            "[Test 3I-L: User Pointer Integer Overflow Rejection (-EFAULT)]: PASS",
            "[Test 3I-M: User Pointer Multi-Page & Permission Verification]: PASS",
            "[Test 3I-N: Capability-Authorized IPC Channel Syscalls]: PASS",
            "[Test 3I-O: Forged Handle & Missing Rights Rejection]: PASS",
            "[Test 3I-P: Shared Memory Lifecycle Syscalls]: PASS",
            "[Test 3I-Q: sys_exit Process Termination & Zombie State]: PASS",
            "[Test 3I-R: Complete User Process PMM Neutrality]: PASS",
        ]
        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3I QEMU verification failed: {msg}")

    def test_stage3i_pmm_neutrality(self):
        """Verify strict PMM frame neutrality across complete user process lifecycle."""
        markers = [
            "[Stage 3I COMPLETE] All 18 tests passed. PMM Neutrality verified."
        ]
        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3I PMM neutrality failed: {msg}")

    def test_stage3i_clean_exit(self):
        """Verify clean shutdown with exit status code 33 (0x10 << 1 | 1)."""
        proc = subprocess.run(
            [
                str(run_qemu.resolve_qemu()),
                "-kernel", str(self.kernel32_elf),
                "-display", "none",
                "-serial", "stdio",
                "-monitor", "none",
                "-no-reboot",
                "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"
            ],
            capture_output=True, text=True, timeout=12
        )
        self.assertEqual(proc.returncode, 33, f"QEMU returncode expected 33, got {proc.returncode}")


if __name__ == "__main__":
    unittest.main()
