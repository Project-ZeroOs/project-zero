"""
Stage 3H Test Suite: Capability System & Kernel Authority Model
Validations:
1. Static ELF Symbols:
   - `capability_derive` exists.
   - `capability_duplicate` exists.
   - `capability_revoke_descendants_locked` exists.
   - `capability_close_locked` exists.
   - `transfer_move_locked` exists.
   - `transfer_delegate_locked` exists.
   - `run_stage3h_verification` exists.
   - `CAPABILITY_NODE_TABLE` exists.
2. QEMU Live Telemetry:
   - Tests 3H-A through 3H-AU (47 tests) verified.
   - Exact PMM frame neutrality preserved.
   - Stage 3H Capability System & Kernel Authority Model verified.
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


class TestStage3HCapability(unittest.TestCase):
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

    def test_elf_symbols_stage3h(self):
        """Verify authoritative Stage 3H symbols exist in compiled ELF."""
        symbols = self._get_elf_symbols()

        expected_funcs = [
            "capability_derive",
            "capability_duplicate",
            "capability_revoke_descendants_locked",
            "capability_close_locked",
            "transfer_move_locked",
            "transfer_delegate_locked",
            "run_stage3h_verification",
        ]

        for func_name in expected_funcs:
            sym = next((s for s in symbols if func_name in s["name"]), None)
            self.assertIsNotNone(sym, f"{func_name} function symbol missing from ELF")
            self.assertEqual(sym["type"], "FUNC", f"{func_name} must be a function")

        # Verify CAPABILITY_NODE_TABLE in .bss
        node_table = next((s for s in symbols if "CAPABILITY_NODE_TABLE" in s["name"]), None)
        self.assertIsNotNone(node_table, "CAPABILITY_NODE_TABLE symbol missing from ELF")
        self.assertEqual(node_table["size"], 12288, "CAPABILITY_NODE_TABLE must be exactly 12 KiB (512 * 24 B)")

    def test_qemu_stage3h_telemetry(self):
        """Execute QEMU and verify Stage 3H Capability System & Kernel Authority Model telemetry."""
        markers = [
            "[Stage 3H: Capability System & Kernel Authority Model Verification]",
            "Test 3H-A: Channel creation allocates root capabilities [VERIFIED]",
            "Test 3H-B: Valid capability lookup returns object & rights [VERIFIED]",
            "Test 3H-C: Invalid handle index rejection [VERIFIED]",
            "Test 3H-D: Forged generation rejection [VERIFIED]",
            "Test 3H-E: Stale handle rejection on closed capability [VERIFIED]",
            "Test 3H-F: Operation allowed with required right [VERIFIED]",
            "Test 3H-G: Operation denied when lacking required right [VERIFIED]",
            "Test 3H-H: Rights attenuation preserves monotonic reduction [VERIFIED]",
            "Test 3H-I: Rights amplification rejected (I-CAP-X) [VERIFIED]",
            "Test 3H-J: Duplicate capability rights invariance [VERIFIED]",
            "Test 3H-K: Derivation tree linkage integrity [VERIFIED]",
            "Test 3H-L: Maximum derivation depth bound (I-CAP-4) [VERIFIED]",
            "Test 3H-M: Sibling list ordering and traversal [VERIFIED]",
            "Test 3H-N: TRANSFER_MOVE ownership transfer [VERIFIED]",
            "Test 3H-O: TRANSFER_DELEGATE authority derivation [VERIFIED]",
            "Test 3H-P: Transfer rejected lacking TRANSFER right [VERIFIED]",
            "Test 3H-Q: Receiver table full atomicity rollback [VERIFIED]",
            "Test 3H-R: Descendant revocation leaves root valid [VERIFIED]",
            "Test 3H-S: Scoped revocation isolation (I-CAP-Y) [VERIFIED]",
            "Test 3H-T: Revocation rejected lacking REVOKE right [VERIFIED]",
            "Test 3H-U: Revocation of already revoked capability [VERIFIED]",
            "Test 3H-V: Cascading multi-level revocation [VERIFIED]",
            "Test 3H-W: Process exit cleans own capabilities [VERIFIED]",
            "Test 3H-X: Process exit preserves delegated peer capabilities [VERIFIED]",
            "Test 3H-Y: Process slot reuse isolation (I-CAP-10) [VERIFIED]",
            "Test 3H-Z: SHM read-only mapping with SHM_MAP_READ [VERIFIED]",
            "Test 3H-AA: SHM write denial without SHM_MAP_WRITE [VERIFIED]",
            "Test 3H-AB: SHM revocation unmapping [VERIFIED]",
            "Test 3H-AC: Cross-process handle isolation [VERIFIED]",
            "Test 3H-AD: Object reclamation after final capability close [VERIFIED]",
            "Test 3H-AE: Monotonic capability ID atomic advance (I-CAP-ID-1) [VERIFIED]",
            "Test 3H-AF: Monotonic lock hierarchy compliance (I-CAP-11) [VERIFIED]",
            "Test 3H-AG: PMM frame neutrality preserved [VERIFIED]",
            "Test 3H-AH: MOVE audit trail and sender slot cleanup [VERIFIED]",
            "Test 3H-AI: Capability ID terminal exhaustion at u64::MAX (I-CAP-ID-1) [VERIFIED]",
            "Test 3H-AJ: Bounded-scan capability lookup by ID (I-CAP-ID-2) [VERIFIED]",
            "Test 3H-AK: MOVE direct parent inheritance (I-CAP-MOVE-1) [VERIFIED]",
            "Test 3H-AL: MOVE followed by parent revocation [VERIFIED]",
            "Test 3H-AM: Parent close with live descendants reparenting [VERIFIED]",
            "Test 3H-AN: TRANSFER_MOVE exhaustion rollback atomicity [VERIFIED]",
            "Test 3H-AO: Derived/MOVE handle allocation never creates root (I-CAP-HANDLE-1) [VERIFIED]",
            "Test 3H-AP: Capability close reparents descendants under Option B (I-CAP-CLOSE-1) [VERIFIED]",
            "Test 3H-AQ: TRANSFER_MOVE preserves descendant subtree (I-CAP-MOVE-2) [VERIFIED]",
            "Test 3H-AR: Process exit removes own capabilities lacking REVOKE [VERIFIED]",
            "Test 3H-AS: Process exit preserves delegated peer capabilities (I-CAP-TEARDOWN-2, I-CAP-TEARDOWN-3) [VERIFIED]",
            "Test 3H-AT: Process capability and handle tables completely empty after teardown (I-CAP-TEARDOWN-1, I-CAP-HANDLE-2) [VERIFIED]",
            "Test 3H-AU: MOVE commit consumes reserved ID without allocating second ID (I-CAP-MOVE-3) [VERIFIED]",
            "Stage 3H verified: frame neutrality preserved",
            "[x] Stage 3H Capability System & Kernel Authority Model verification complete (47/47 tests).",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3H QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


if __name__ == "__main__":
    unittest.main()
