"""
Stage 3M Test Suite: Native Networking Model
Validations:
1. Static Kernel Symbols:
   - `run_stage3m_verification` exists.
   - `alloc_packet_buffer` exists.
   - `bind_socket` exists.
   - `connect_socket` exists.
   - `send_socket` exists.
   - `recv_socket` exists.
   - `PACKET_BUFFER_TABLE` exists in ELF.
   - `SOCKET_TABLE` exists in ELF.
   - `PORT_BINDING_TABLE` exists in ELF.
   - `INTERFACE_TABLE` exists in ELF.
   - `ROUTE_TABLE` exists in ELF.
   - `NEIGHBOR_TABLE` exists in ELF.
2. Live QEMU Telemetry:
   - Tests 3M-A through 3M-Z (26 sequential machine tests) pass.
3. Architecture Invariants:
   - Network identity & monotonicity (I-NET-ID-1).
   - Device binding & lo0 pseudo-device exception (I-NET-DEV-1).
   - Packet-buffer ↔ DMA frame slicing & pin tracking (I-NET-DMA-1, I-NET-DMA-2).
   - Interface queue backpressure & drop accounting (I-NET-QUEUE-1).
   - ARP 4-state engine & LRU eviction (I-NET-ARP-1).
   - LPM route lookup & interface teardown purge (I-NET-ROUTE-TEARDOWN-1).
   - Port binding namespace separation & collision matrix (I-NET-BIND-1).
   - Socket waitqueue blocking & lost-wakeup guard (I-NET-WAIT-1).
   - Multi-tier device failure propagation & socket wakeup (I-NET-DEV-FAIL-1).
   - Process-exit socket & port unbind teardown (I-NET-TEARDOWN-1).
   - Capability revocation cascade (I-NET-REVOKE-1).
4. Transactional PMM Neutrality:
   - Zero net frame leakage across complete networking lifecycle (`baseline_free == post_test_free`).
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


class TestStage3MNetworkingModel(unittest.TestCase):
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

    def test_stage3m_rust_symbols(self):
        """Verify Stage 3M networking functions exist in kernel ELF."""
        symbols = self._get_elf_symbols()

        expected = [
            "run_stage3m_verification",
            "alloc_packet_buffer",
            "bind_socket",
            "connect_socket",
            "send_socket",
            "recv_socket",
        ]
        for name in expected:
            sym = next((s for s in symbols if name in s["name"]), None)
            self.assertIsNotNone(sym, f"Required symbol '{name}' missing from ELF")

    def test_stage3m_static_tables(self):
        """Verify static Stage 3M bounded tables exist in ELF."""
        symbols = self._get_elf_symbols()
        tables = [
            "PACKET_BUFFER_TABLE",
            "SOCKET_TABLE",
            "PORT_BINDING_TABLE",
            "INTERFACE_TABLE",
            "ROUTE_TABLE",
            "NEIGHBOR_TABLE",
        ]
        for tbl in tables:
            table_sym = next((s for s in symbols if tbl in s["name"]), None)
            self.assertIsNotNone(table_sym, f"Static table '{tbl}' missing from ELF")

    def test_stage3m_qemu_execution(self):
        """Execute QEMU and assert that all 26 Stage 3M bare-metal tests pass sequentially."""
        markers = [
            "[3M-A] Testing Network Identity & Monotonicity (I-NET-ID-1)...",
            "[3M-A] PASSED",
            "[3M-B] Testing lo0 Loopback Setup (I-NET-DEV-1 Exception)...",
            "[3M-B] PASSED",
            "[3M-C] Testing Stage 3L Device Binding & Lifetime...",
            "[3M-C] PASSED",
            "[3M-D] Testing Packet Buffer Pool Allocation...",
            "[3M-D] PASSED",
            "[3M-E] Testing Single Packet Ownership & DMA Slicing (I-NET-DMA-2)...",
            "[3M-E] PASSED",
            "[3M-F] Testing Stage 3L DMA Pin Integration (I-NET-DMA-1)...",
            "[3M-F] PASSED",
            "[3M-G] Testing Interface RX Queue FIFO Order...",
            "[3M-G] PASSED",
            "[3M-H] Testing Interface TX Queue Backpressure & Drops (I-NET-QUEUE-1)...",
            "[3M-H] PASSED",
            "[3M-I] Testing Top-Half IRQ Dispatch & Event Signaling...",
            "[3M-I] PASSED",
            "[3M-J] Testing IRQ Storm Threshold Isolation...",
            "[3M-J] PASSED",
            "[3M-K] Testing Ethernet Framing Validation...",
            "[3M-K] PASSED",
            "[3M-L] Testing IPv4 Header Checksum Fidelity (RFC 1071)...",
            "[3M-L] PASSED",
            "[3M-M] Testing ARP Cache 4-State Engine & Eviction (I-NET-ARP-1)...",
            "[3M-M] PASSED",
            "[3M-N] Testing Longest-Prefix Matching Routing...",
            "[3M-N] PASSED",
            "[3M-O] Testing Route Teardown on Interface Detach (I-NET-ROUTE-TEARDOWN-1)...",
            "[3M-O] PASSED",
            "[3M-P] Testing Socket Creation & KernelObjectType::Socket=7...",
            "[3M-P] PASSED",
            "[3M-Q] Testing Port Binding & Collision Matrix (I-NET-BIND-1)...",
            "[3M-Q] PASSED",
            "[3M-R] Testing Privileged Port Authority (1..1023)...",
            "[3M-R] PASSED",
            "[3M-S] Testing UDP Datagram Flow over Loopback...",
            "[3M-S] PASSED",
            "[3M-T] Testing Minimal RFC 793 TCP Handshake & Teardown...",
            "[3M-T] PASSED",
            "[3M-U] Testing Blocking Recv Predicate & Lost-Wakeup Guard (I-NET-WAIT-1)...",
            "[3M-U] PASSED",
            "[3M-V] Testing Non-Blocking Operations (-EAGAIN / WouldBlock)...",
            "[3M-V] PASSED",
            "[3M-W] Testing Device Fault & Socket Wakeup (I-NET-DEV-FAIL-1)...",
            "[3M-W] PASSED",
            "[3M-X] Testing Process Exit Socket Teardown (I-NET-TEARDOWN-1)...",
            "[3M-X] PASSED",
            "[3M-Y] Testing Capability Revocation Cascade (I-NET-REVOKE-1)...",
            "[3M-Y] PASSED",
            "[3M-Z] Testing Monotonic Lock Order & PMM Neutrality...",
            "[3M-Z] PASSED",
            "Stage 3M PMM Neutrality: VERIFIED (zero net frame leakage).",
            "[Stage 3M Verification Complete: 26/26 tests PASSED]",
            "Cumulative Machine Tests: 287 tests",
        ]

        success, msg = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(success, f"Stage 3M QEMU verification failed: {msg}")


if __name__ == "__main__":
    unittest.main()
