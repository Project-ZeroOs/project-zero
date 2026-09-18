"""
Stage 3G Test Suite: IPC & Kernel Object Subsystem
Validations:
1. Static ELF Symbols:
   - `channel_create` exists.
   - `channel_send` exists.
   - `channel_receive` exists.
   - `channel_close` exists.
   - `shm_create` exists.
   - `shm_map` exists.
   - `shm_unmap` exists.
   - `shm_close` exists.
   - `run_stage3g_verification` exists.
2. QEMU Live Telemetry:
   - Tests 3G-A through 3G-AT (46 tests) verified.
   - Exact PMM frame neutrality preserved.
   - Stage 3G IPC & Kernel Object Semantics verified.
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


class TestStage3GIPC(unittest.TestCase):
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

    def test_elf_symbols_stage3g(self):
        """Verify authoritative Stage 3G symbols exist in compiled ELF."""
        symbols = self._get_elf_symbols()

        expected_funcs = [
            "channel_create",
            "channel_send",
            "channel_receive",
            "channel_close",
            "shm_create",
            "shm_map",
            "shm_unmap",
            "shm_close",
            "run_stage3g_verification",
        ]

        for func_name in expected_funcs:
            sym = next((s for s in symbols if func_name in s["name"]), None)
            self.assertIsNotNone(sym, f"{func_name} symbol missing from ELF")
            self.assertEqual(sym["type"], "FUNC", f"{func_name} must be a function")

    def test_qemu_stage3g_telemetry(self):
        """Execute QEMU and verify Stage 3G IPC & Kernel Object Semantics telemetry."""
        markers = [
            "[Stage 3G: IPC & Kernel Object Semantics Verification]",
            "Test 3G-A: Channel creation & reference accounting [VERIFIED]",
            "Test 3G-B: Channel destruction & lifecycle reclamation [VERIFIED]",
            "Test 3G-C: Basic 80-byte send/receive payload fidelity [VERIFIED]",
            "Test 3G-D: Strict FIFO message ring ordering [VERIFIED]",
            "Test 3G-E: Ring full backpressure & WouldBlock semantics [VERIFIED]",
            "Test 3G-F: Non-blocking receive on empty ring [VERIFIED]",
            "Test 3G-G: Blocking send/receive immediate availability [VERIFIED]",
            "Test 3G-H: Priority-ordered waiter wakeups [VERIFIED]",
            "Test 3G-I: Immediate PeerClosed observation on receiver [VERIFIED]",
            "Test 3G-J: Immediate PeerClosed observation on sender [VERIFIED]",
            "Test 3G-K & 3G-L: In-flight message drain before peer-close [VERIFIED]",
            "Test 3G-M: Stale handle generation rejection [VERIFIED]",
            "Test 3G-N: Rights mask permission enforcement [VERIFIED]",
            "Test 3G-O: ShmObject creation, mapping & volatile memory access [VERIFIED]",
            "Test 3G-P: ShmObject W^X unconditional NX enforcement [VERIFIED]",
            "Test 3G-Q: ShmObject mapping reference pins physical frames [VERIFIED]",
            "Test 3G-R: ShmObject exact PMM neutrality [VERIFIED]",
            "Test 3G-S: Opaque handle transfer descriptors [VERIFIED]",
            "Test 3G-T: Multi-process handle table isolation [VERIFIED]",
            "Test 3G-U: 128 stress create-destroy cycles [VERIFIED]",
            "Test 3G-V: PMM neutrality across lifecycle stress [VERIFIED]",
            "Test 3G-W: Handle slot generation monotonic advance [VERIFIED]",
            "Test 3G-X: Monotonic lock hierarchy sanity [VERIFIED]",
            "Test 3G-Y: Peer death sequence cleanly clears channels [VERIFIED]",
            "Test 3G-Z: Subsystem waitqueue cancellation for process [VERIFIED]",
            "Test 3G-AA: Cancelled in-flight IPC reference cleanup [VERIFIED]",
            "Test 3G-AB: SHM mapping & handle teardown sweep [VERIFIED]",
            "Test 3G-AC: Duplicate handle endpoint reference counting [VERIFIED]",
            "Test 3G-AD: Duplicate handles across both endpoints [VERIFIED]",
            "Test 3G-AE: Object lifecycle states and registry occupancy [VERIFIED]",
            "Test 3G-AF: Single in-flight IPC operation invariant (I-IPC-1) [VERIFIED]",
            "Test 3G-AG: SHM unmap wrong-handle permission rejection [VERIFIED]",
            "Test 3G-AH: Waiter lifetime pin (I-CHAN-4) [VERIFIED]",
            "Test 3G-AI: Reclamation blocked with active waiter [VERIFIED]",
            "Test 3G-AJ: Cancellation clears waiter + pin exactly once [VERIFIED]",
            "Test 3G-AK: External HandleTable & 128-byte Process ABI [VERIFIED]",
            "Test 3G-AL: Process slot reuse handle isolation [VERIFIED]",
            "Test 3G-AM: Object ID exhaustion terminal state [VERIFIED]",
            "Test 3G-AN: SMP/TLB shootdown contract invariants [VERIFIED]",
            "Test 3G-AO: Concurrent Object-ID CAS terminal exhaustion [VERIFIED]",
            "Test 3G-AP: PID vs process-table slot resolution isolation [VERIFIED]",
            "Test 3G-AR: Last endpoint handle close vs in-flight send [VERIFIED]",
            "Test 3G-AS: Last endpoint handle close vs in-flight receive [VERIFIED]",
            "Test 3G-AT: Last endpoint handle close wakes blocked waiters [VERIFIED]",
            "Stage 3G verified: frame neutrality preserved",
            "[x] Stage 3G IPC & Kernel Object Semantics verification complete (46/46 tests).",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3G QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


if __name__ == "__main__":
    unittest.main()
