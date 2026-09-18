"""
Stage 3C Test Suite: Interrupt/Timer-Driven Preemptive Scheduling
Increment 2 Validations:
1. Static ELF Symbols:
   - `BSP_PERCPU` object symbol exists with exact size 48 bytes and 8-byte alignment.
   - `init_lapic_mmio` function exists in higher-half VMA.
   - `LAPIC_VIRT_BASE` = 0xFFFF_FFFF_FEE0_0000.
2. QEMU Live Telemetry:
   - IA32_APIC_BASE MSR read: 0x00000000FEE00900.
   - APIC Global Enable == true.
   - BSP Processor Flag == true.
   - Discovered Physical Base == 0x00000000FEE00000.
   - Mapped Virtual Base == 0xFFFFFFFFFEE00000.
   - MMIO Page Attributes: PRESENT | WRITABLE | NO_EXECUTE | CACHE_DISABLE | WRITE_THROUGH.
   - Hardware LAPIC ID: 0.
   - Hardware LAPIC Version: 0x14 (Max LVT Entries: 5).
   - PerCpu.lapic_id Populated: 0.
   - Stage 3C Increment 2 verification banner confirmed.
"""

import unittest
import subprocess
import shutil
from pathlib import Path
import sys

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "tools"))
import run_qemu

PERCPU_SIZE = 48  # Stage 3C ABI extension


def get_readelf_path() -> str:
    r = shutil.which("readelf")
    if r:
        return r
    msys_r = Path("C:/msys64/ucrt64/bin/readelf.exe")
    if msys_r.exists():
        return str(msys_r)
    raise FileNotFoundError("readelf not found")


class TestStage3CPreemption(unittest.TestCase):
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
        """Verify Stage 3C symbols exist in ELF with expected size, type, and alignment."""
        symbols = self._get_elf_symbols()

        bsp_percpu = next((s for s in symbols if "BSP_PERCPU" in s["name"]), None)
        self.assertIsNotNone(bsp_percpu, "BSP_PERCPU static symbol missing from ELF")
        self.assertEqual(bsp_percpu["size"], PERCPU_SIZE, f"BSP_PERCPU size must be {PERCPU_SIZE} bytes in Stage 3C")
        self.assertEqual(bsp_percpu["addr"] % 8, 0, "BSP_PERCPU address must be 8-byte aligned")

        isr_32_sym = next((s for s in symbols if s["name"] == "isr_32"), None)
        self.assertIsNotNone(isr_32_sym, "isr_32 symbol missing from ELF")
        self.assertEqual(isr_32_sym["type"], "FUNC", "isr_32 must be a function")

        gpr_harness_sym = next((s for s in symbols if s["name"] == "test_gpr_sentinels_under_irq"), None)
        self.assertIsNotNone(gpr_harness_sym, "test_gpr_sentinels_under_irq symbol missing from ELF")

        # Stage 3C Increment 4 assembly primitives
        switch_coop_sym = next((s for s in symbols if s["name"] == "switch_context"), None)
        self.assertIsNotNone(switch_coop_sym, "switch_context symbol missing from ELF")
        self.assertEqual(switch_coop_sym["type"], "FUNC", "switch_context must be a function")

        c2p_sym = next((s for s in symbols if s["name"] == "switch_context_coop_to_preempt"), None)
        self.assertIsNotNone(c2p_sym, "switch_context_coop_to_preempt symbol missing from ELF")
        self.assertEqual(c2p_sym["type"], "FUNC", "switch_context_coop_to_preempt must be a function")

        p2c_sym = next((s for s in symbols if s["name"] == "restore_context_cooperative"), None)
        self.assertIsNotNone(p2c_sym, "restore_context_cooperative symbol missing from ELF")
        self.assertEqual(p2c_sym["type"], "FUNC", "restore_context_cooperative must be a function")

        p2p_sym = next((s for s in symbols if s["name"] == "restore_context_preemptive"), None)
        self.assertIsNotNone(p2p_sym, "restore_context_preemptive symbol missing from ELF")
        self.assertEqual(p2p_sym["type"], "FUNC", "restore_context_preemptive must be a function")

    def test_qemu_stage3c_increment2_telemetry(self):
        """Execute QEMU and verify Stage 3C Increment 2 LAPIC discovery & MMIO mapping."""
        markers = [
            "[Stage 3C-Increment 2: Local APIC Discovery & MMIO Mapping]",
            "IA32_APIC_BASE MSR:          0x00000000FEE00900",
            "APIC Global Enable:          true (Bit 11 = 1)",
            "BSP Processor Flag:          true (Bit 8 = 1)",
            "Discovered Physical Base:    0x00000000FEE00000",
            "Mapped Virtual Base:         0xFFFFFFFFFEE00000",
            "MMIO Page Attributes:        PRESENT | WRITABLE | NO_EXECUTE | CACHE_DISABLE | WRITE_THROUGH",
            "Hardware LAPIC ID:           0 (0x00)",
            "Hardware LAPIC Version:      0x14 (Max LVT Entry field: 5, LVT Entries: 6)",
            "PerCpu.lapic_id Populated:   0 [VERIFIED]",
            "[x] Stage 3C Increment 2 LAPIC discovery & MMIO mapping verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3C Increment 2 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )

    def test_qemu_stage3c_increment3_telemetry(self):
        """Execute QEMU and verify Stage 3C Increment 3 LAPIC timer programming, PIT2 calibration & GPR sentinels."""
        markers = [
            "[Stage 3C-Increment 3: LAPIC Timer Programming & Non-Preemptive ISR]",
            "Timer Vector:                32 (0x20, IDT[32])",
            "Timer Mode:                  Periodic (Bits [18:17] = 01b)",
            "Divisor Configuration:       Divide by 16 (DCR = 0x03)",
            "PIT2 Calibration Window:     10 ms (11932 PIT cycles)",
            "Calibrated Initial Count:",
            "Observed LAPIC IRQ Ticks:    10",
            "Forward progress across 10 timer interrupts: VERIFIED",
            "Interrupt Return Path:       iretq returning to interrupted execution context [VERIFIED]",
            "EOI Acknowledgment:          lapic_eoi() verified",
            "[TIMER REG TEST] interrupts observed: 10",
            "RAX preserved: PASS (0x1111111111111111)",
            "RCX preserved: PASS (0x2222222222222222)",
            "RDX preserved: PASS (0x3333333333333333)",
            "RBX preserved: PASS (0x4444444444444444)",
            "RBP preserved: PASS (0x5555555555555555)",
            "RSI preserved: PASS (0x6666666666666666)",
            "RDI preserved: PASS (0x7777777777777777)",
            "R8  preserved: PASS (0x8888888888888888)",
            "R9  preserved: PASS (0x9999999999999999)",
            "R10 preserved: PASS (0xAAAAAAAAAAAAAAAA)",
            "R11 preserved: PASS (0xBBBBBBBBBBBBBBBB)",
            "R12 preserved: PASS (0xCCCCCCCCCCCCCCCC)",
            "R13 preserved: PASS (0xDDDDDDDDDDDDDDDD)",
            "R14 preserved: PASS (0xEEEEEEEEEEEEEEEE)",
            "R15 preserved: PASS (0xFFFFFFFFFFFFFFFF)",
            "RSP preserved: PASS",
            "RIP resumed:   PASS",
            "RFLAGS resumed: PASS",
            "[x] Stage 3C Increment 3 LAPIC timer programming & non-preemptive ISR verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3C Increment 3 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )

    def test_qemu_stage3c_increment4_telemetry(self):
        """Execute QEMU and verify Stage 3C Increment 4 Preemptive Context Assembly Primitives."""
        markers = [
            "[Stage 3C-Increment 4: Preemptive Context Assembly Primitives]",
            "[INC4 STEP 1] Coop -> Preempt (switch_context_coop_to_preempt): PASS",
            "Restored RFLAGS: 0x0000000000000202 (IF=1 via iretq) [VERIFIED]",
            "All 15 GPRs Verified: RAX=0x1111.. to R15=0xFFFF.. [MATCH]",
            "[INC4 STEP 2] Preempt -> Preempt (restore_context_preemptive): PASS",
            "Restored RFLAGS: 0x0000000000000202 (IF=1 via iretq) [VERIFIED]",
            "All 15 GPRs Verified: RAX=0xA1A1.. to R15=0x8F8F.. [MATCH]",
            "[INC4 STEP 3] Preempt -> Coop (restore_context_cooperative): PASS",
            "Resumed Callee GPRs Verified: RBX, RBP, R12..R15 [MATCH]",
            "Restored RFLAGS: 0x0000000000000202 (IF=1 preserved across chain) [VERIFIED]",
            "free frames restored [EXACT MATCH]",
            "Four-way transition matrix assembly primitives verified.",
            "[x] Stage 3C Increment 4 Preemptive Context Assembly Primitives verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3C Increment 4 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )

    def test_qemu_stage3c_increment5_telemetry(self):
        """Execute QEMU and verify Stage 3C Increment 5 Scheduler Preemption Integration."""
        markers = [
            "[Stage 3C-Increment 5: Scheduler Preemption Integration]",
            "[STATE MACHINE] Step 1: Coop -> Coop (switch_context): PASS",
            "[STATE MACHINE] Step 2: Preempt -> Coop (restore_context_cooperative): PASS",
            "[STATE MACHINE] Step 3: Preempt -> Preempt (restore_context_preemptive): PASS",
            "[STATE MACHINE] Step 5: Coop -> Preempt (switch_context_coop_to_preempt): PASS",
            "Worker 1 interrupted continuation verified: EntryCount=1, Phase 2->3, Canary=0xDEADBEEFCAFE0001, ForwardProgress: strict > [VERIFIED]",
            "Worker 2 interrupted continuation verified: EntryCount=1, Phase 2->3, Canary=0xDEADBEEFCAFE0002, ForwardProgress: strict > [VERIFIED]",
            "Non-voluntary preemption: Worker 1 interrupted without yield_now() [VERIFIED]",
            "Non-voluntary preemption: Worker 2 interrupted without yield_now() [VERIFIED]",
            "Preempt -> Coop live context switch: PASS",
            "Preempt -> Preempt live context switch: PASS",
            "Coop -> Preempt live context switch: PASS",
            "Pre-switch machine invariants: sched_lock=FREE, IF=0, GS+16=next, nested_irq_count=0 [VERIFIED]",
            "Deferred preemption: need_resched pending under IF=0 [VERIFIED]",
            "Stack Accounting: free frames before=",
            "[EXACT MATCH]",
            "[x] Stage 3C Increment 5 Scheduler Preemption Integration verified.",
        ]
        success, failures = run_qemu.test_qemu(self.kernel32_elf, markers=markers)
        self.assertTrue(
            success,
            f"Stage 3C Increment 5 QEMU verification failed:\n" + "\n".join(f"  - {f}" for f in failures)
        )


if __name__ == "__main__":
    unittest.main()
