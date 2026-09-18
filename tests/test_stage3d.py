"""
Stage 3D Test Suite: Thread Blocking & Synchronization
Increment 1 Validations:
1. Static ELF Symbols:
   - `block_current` exists.
   - `run_stage3d_inc1_verification` exists.
2. QEMU Live Telemetry:
   - Priority FIFO ordering: Critical > High > Normal, FIFO equal [VERIFIED]
   - Waiter detachment: next_waiter == NULL after pop/remove [VERIFIED]
   - Single wait-channel invariant: exclusive next_waiter linkage [VERIFIED]
   - Live block_current() -> Blocked state transition [VERIFIED]
   - Live wake_one() -> Ready state transition & RunQueue re-enqueue [VERIFIED]
   - Resumption verification: state == Running, GS+16 == current [VERIFIED]
   - Stack Accounting: [EXACT MATCH]
   - [x] Stage 3D Increment 1 Core Block/Wake verified.
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


class TestStage3DBlocking(unittest.TestCase):
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
                        "ndx": parts[6],
                        "name": parts[7]
                    })
                except (ValueError, IndexError):
                    continue
        return symbols

    def test_stage3d_inc1_symbols(self):
        """Verify symbols for Stage 3D Increment 1 blocking foundation."""
        symbols = self._get_elf_symbols()

        block_sym = next((s for s in symbols if "block_current" in s["name"]), None)
        self.assertIsNotNone(block_sym, "block_current symbol missing from ELF")
        self.assertEqual(block_sym["type"], "FUNC", "block_current must be a function")

        verify_sym = next((s for s in symbols if "run_stage3d_inc1_verification" in s["name"]), None)
        self.assertIsNotNone(verify_sym, "run_stage3d_inc1_verification symbol missing from ELF")
        self.assertEqual(verify_sym["type"], "FUNC", "run_stage3d_inc1_verification must be a function")

    def test_qemu_stage3d_increment1_telemetry(self):
        """Execute QEMU and verify Stage 3D Increment 1 Core Block/Wake Foundation telemetry."""
        markers = [
            "[Stage 3D-Increment 1: Core Block/Wake Foundation]",
            "Priority FIFO ordering: Critical > High > Normal, FIFO equal [VERIFIED]",
            "Waiter detachment: next_waiter == NULL after pop/remove [VERIFIED]",
            "Single wait-channel invariant: exclusive next_waiter linkage [VERIFIED]",
            "Live block_current() -> Blocked state transition [VERIFIED]",
            "Live wake_one() -> Ready state transition & RunQueue re-enqueue [VERIFIED]",
            "Resumption verification: state == Running, GS+16 == current [VERIFIED]",
            "Stack Accounting: free frames before=",
            "[EXACT MATCH]",
            "[x] Stage 3D Increment 1 Core Block/Wake verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3D Increment 1 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )

    def test_stage3d_inc2_symbols(self):
        """Verify symbols for Stage 3D Increment 2 Kernel Mutex primitive."""
        symbols = self._get_elf_symbols()

        # Check for Mutex::lock
        lock_sym = next((s for s in symbols if "Mutex" in s["name"] and "lock" in s["name"] and "unlock" not in s["name"]), None)
        self.assertIsNotNone(lock_sym, "Mutex::lock symbol missing from ELF")
        self.assertEqual(lock_sym["type"], "FUNC", "Mutex::lock must be a function")

        # Check for Mutex::unlock
        unlock_sym = next((s for s in symbols if "Mutex" in s["name"] and "unlock" in s["name"]), None)
        self.assertIsNotNone(unlock_sym, "Mutex::unlock symbol missing from ELF")
        self.assertEqual(unlock_sym["type"], "FUNC", "Mutex::unlock must be a function")

        # Check for run_stage3d_inc2_verification
        verify_sym = next((s for s in symbols if "run_stage3d_inc2_verification" in s["name"]), None)
        self.assertIsNotNone(verify_sym, "run_stage3d_inc2_verification symbol missing from ELF")
        self.assertEqual(verify_sym["type"], "FUNC", "run_stage3d_inc2_verification must be a function")

    def test_qemu_stage3d_increment2_telemetry(self):
        """Execute QEMU and verify Stage 3D Increment 2 Kernel Mutex telemetry."""
        markers = [
            "[Stage 3D-Increment 2: Kernel Mutex Primitive]",
            "Test D: Priority FIFO waiter ordering: Critical > High > Normal [VERIFIED]",
            "Test A: Fast path lock and unlock [VERIFIED]",
            "Test E: Ownership validation & non-recursive rejection [VERIFIED]",
            "Test F: No waiter unlock transitions owner to 0 [VERIFIED]",
            "Test B: Contention blocking on held mutex [VERIFIED]",
            "Test C: Direct ownership handoff [VERIFIED]",
            "Stack Accounting: free frames before=",
            "[EXACT MATCH]",
            "[x] Stage 3D Increment 2 Kernel Mutex verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3D Increment 2 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )

    def test_stage3d_inc3_symbols(self):
        """Verify symbols for Stage 3D Increment 3 Condition Variable primitive."""
        symbols = self._get_elf_symbols()

        # Check for Condvar::wait
        wait_sym = next((s for s in symbols if "Condvar" in s["name"] and "wait" in s["name"] and "waiter" not in s["name"]), None)
        self.assertIsNotNone(wait_sym, "Condvar::wait symbol missing from ELF")
        self.assertEqual(wait_sym["type"], "FUNC", "Condvar::wait must be a function")

        # Check for Condvar::signal
        signal_sym = next((s for s in symbols if "Condvar" in s["name"] and "signal" in s["name"]), None)
        self.assertIsNotNone(signal_sym, "Condvar::signal symbol missing from ELF")
        self.assertEqual(signal_sym["type"], "FUNC", "Condvar::signal must be a function")

        # Check for Condvar::broadcast
        bcast_sym = next((s for s in symbols if "Condvar" in s["name"] and "broadcast" in s["name"]), None)
        self.assertIsNotNone(bcast_sym, "Condvar::broadcast symbol missing from ELF")
        self.assertEqual(bcast_sym["type"], "FUNC", "Condvar::broadcast must be a function")

        # Check for Mutex::unlock_locked
        unlock_locked_sym = next((s for s in symbols if "Mutex" in s["name"] and "unlock_locked" in s["name"]), None)
        self.assertIsNotNone(unlock_locked_sym, "Mutex::unlock_locked symbol missing from ELF")
        self.assertEqual(unlock_locked_sym["type"], "FUNC", "Mutex::unlock_locked must be a function")

        # Check for run_stage3d_inc3_verification
        verify_sym = next((s for s in symbols if "run_stage3d_inc3_verification" in s["name"]), None)
        self.assertIsNotNone(verify_sym, "run_stage3d_inc3_verification symbol missing from ELF")
        self.assertEqual(verify_sym["type"], "FUNC", "run_stage3d_inc3_verification must be a function")

    def test_qemu_stage3d_increment3_telemetry(self):
        """Execute QEMU and verify Stage 3D Increment 3 Condition Variable telemetry."""
        markers = [
            "[Stage 3D-Increment 3: Condition Variable Primitive]",
            "Test E: Ownership validation for condvar wait [VERIFIED]",
            "Test C: Priority FIFO waiter ordering: Critical > High > Normal [VERIFIED]",
            "Test A: Basic predicate wait & signal [VERIFIED]",
            "Test D: Atomic registration & no lost wakeup proof [VERIFIED]",
            "Test B: Broadcast & serialized mutex contention [VERIFIED]",
            "Stack Accounting: free frames before=",
            "[EXACT MATCH]",
            "[x] Stage 3D Increment 3 Condition Variables verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3D Increment 3 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )

    def test_stage3d_inc4_symbols(self):
        """Verify symbols for Stage 3D Increment 4 Timer Sleep primitive."""
        symbols = self._get_elf_symbols()

        # Check for sleep_ms
        sleep_sym = next((s for s in symbols if "sleep_ms" in s["name"]), None)
        self.assertIsNotNone(sleep_sym, "sleep_ms symbol missing from ELF")
        self.assertEqual(sleep_sym["type"], "FUNC", "sleep_ms must be a function")

        # Check for expire_sleepers_locked
        expire_sym = next((s for s in symbols if "expire_sleepers_locked" in s["name"]), None)
        self.assertIsNotNone(expire_sym, "expire_sleepers_locked symbol missing from ELF")
        self.assertEqual(expire_sym["type"], "FUNC", "expire_sleepers_locked must be a function")

        # Check for run_stage3d_inc4_verification
        verify_sym = next((s for s in symbols if "run_stage3d_inc4_verification" in s["name"]), None)
        self.assertIsNotNone(verify_sym, "run_stage3d_inc4_verification symbol missing from ELF")
        self.assertEqual(verify_sym["type"], "FUNC", "run_stage3d_inc4_verification must be a function")

    def test_qemu_stage3d_increment4_telemetry(self):
        """Execute QEMU and verify Stage 3D Increment 4 Timer Sleep telemetry."""
        markers = [
            "[Stage 3D-Increment 4: Timer Sleep (sleep_ms & SleepTable)]",
            "Test A: Zero sleep yields without registration [VERIFIED]",
            "Test G: SleepTable exhaustion rejected deterministically [VERIFIED]",
            "Test H: Duplicate registration rejected [VERIFIED]",
            "Test I: Saturating deadline arithmetic & overflow safety [VERIFIED]",
            "Test B: Sub-tick sleep (1 ms -> >=1 tick) [VERIFIED]",
            "Test C: Exact tick boundary (20 ms -> >=2 ticks) [VERIFIED]",
            "Test D: Staggered concurrent sleepers wake in monotonic order [VERIFIED]",
            "Test E: Same-deadline concurrent sleepers all wake without loss [VERIFIED]",
            "Test F: RunQueue priority ordering on same-deadline wake: Critical > High > Normal [VERIFIED]",
            "Test K: Sleep while higher-priority runnable thread exists dispatches immediately [VERIFIED]",
            "Stack Accounting: free frames before=",
            "[EXACT MATCH]",
            "[x] Stage 3D Increment 4 Timer Sleep verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3D Increment 4 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


    def test_stage3d_inc5_symbols(self):
        """Verify symbols for Stage 3D Increment 5 Kernel Event primitive."""
        symbols = self._get_elf_symbols()

        # Check for Event::wait
        wait_sym = next((s for s in symbols if "Event" in s["name"] and "wait" in s["name"] and "try_wait" not in s["name"] and "waiter" not in s["name"]), None)
        self.assertIsNotNone(wait_sym, "Event::wait symbol missing from ELF")
        self.assertEqual(wait_sym["type"], "FUNC", "Event::wait must be a function")

        # Check for Event::signal
        signal_sym = next((s for s in symbols if "Event" in s["name"] and "signal" in s["name"]), None)
        self.assertIsNotNone(signal_sym, "Event::signal symbol missing from ELF")
        self.assertEqual(signal_sym["type"], "FUNC", "Event::signal must be a function")

        # Check for Event::reset
        reset_sym = next((s for s in symbols if "Event" in s["name"] and "reset" in s["name"]), None)
        self.assertIsNotNone(reset_sym, "Event::reset symbol missing from ELF")
        self.assertEqual(reset_sym["type"], "FUNC", "Event::reset must be a function")

        # Check for Event::try_wait
        try_wait_sym = next((s for s in symbols if "Event" in s["name"] and "try_wait" in s["name"]), None)
        self.assertIsNotNone(try_wait_sym, "Event::try_wait symbol missing from ELF")
        self.assertEqual(try_wait_sym["type"], "FUNC", "Event::try_wait must be a function")

        # Check for run_stage3d_inc5_verification
        verify_sym = next((s for s in symbols if "run_stage3d_inc5_verification" in s["name"]), None)
        self.assertIsNotNone(verify_sym, "run_stage3d_inc5_verification symbol missing from ELF")
        self.assertEqual(verify_sym["type"], "FUNC", "run_stage3d_inc5_verification must be a function")

    def test_qemu_stage3d_increment5_telemetry(self):
        """Execute QEMU and verify Stage 3D Increment 5 Kernel Event primitive telemetry."""
        markers = [
            "[Stage 3D-Increment 5: Kernel Event Primitive]",
            "Test A: Auto-Reset pre-signaled fast path [VERIFIED]",
            "Test B: Auto-Reset single waiter wakeup [VERIFIED]",
            "Test C: Auto-Reset exactly one waiter woken [VERIFIED]",
            "Test D: Auto-Reset priority wakeup ordering: Critical > High > Normal [VERIFIED]",
            "Test E: Manual-Reset pre-signaled pass-through [VERIFIED]",
            "Test F: Manual-Reset broadcast multicast wakeup [VERIFIED]",
            "Test G: Manual-Reset post-signal pass-through [VERIFIED]",
            "Test H: Manual-Reset explicit reset [VERIFIED]",
            "Test I: ISR context signaling via LAPIC timer interrupt [VERIFIED]",
            "Test J: Higher-priority wakeup / rescheduling at permitted scheduling point [VERIFIED]",
            "Test K: Thread completion notification simulation [VERIFIED]",
            "Stack Accounting: free frames before=",
            "[EXACT MATCH]",
            "[x] Stage 3D Increment 5 Kernel Event primitive verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3D Increment 5 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


if __name__ == "__main__":
    unittest.main()

