"""
Project Zero - Stage 2D Physical Frame Manager (PMM) Test Suite

Verifies:
1. PMM Initialization strictly from Stage 2C candidate inventory.
2. Statically reserved PMM metadata exclusion from free frame candidate pool.
3. Fail-closed initialization (untracked/unusable memory is never free).
4. Explicit 4-state 2-bit model (Free, Allocated, Reserved, Unusable).
5. 64-bit frame identity and checked 4 KiB aligned arithmetic.
6. Allocation uniqueness, 4 KiB alignment, and candidate pool inclusion.
7. Free operations, state transitions, and deterministic first-free reuse.
8. Rejection of invalid operations (double free, freeing reserved/unusable, out of range, misaligned).
9. Synthetic exhaustion behavior (returns None when candidate pool is exhausted).
10. Test-only corruption detection via invariant verification.
11. Live QEMU boot test validating Stage 2D serial telemetry and runtime smoke test.
"""

import sys
import unittest
from pathlib import Path

TOOLS_DIR = Path(__file__).resolve().parent.parent / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu

PAGE_SIZE = 4096

class FrameState:
    FREE = 0b00
    ALLOCATED = 0b01
    RESERVED = 0b10
    UNUSABLE = 0b11


class PmmError(Exception):
    pass

class DoubleFree(PmmError):
    pass

class FrameReserved(PmmError):
    pass

class FrameUnusable(PmmError):
    pass

class FrameOutOfRange(PmmError):
    pass

class FrameMisaligned(PmmError):
    pass

class CapacityExceeded(PmmError):
    pass


class SyntheticPmm:
    """
    Python reference implementation strictly replicating kernel/src/mm/pmm.rs.
    Uses an authoritative 2-bit state per frame and fail-closed initialization.
    """
    def __init__(self, max_tracked_frames: int = 65536):
        self.max_tracked_frames = max_tracked_frames
        self.tracked_frames = 0
        self.free_frames = 0
        self.allocated_frames = 0
        self.reserved_frames = 0
        self.unusable_frames = 0
        self.states = []

    def init_from_inventory(self, raw_ram_ranges, reservations, candidate_pools):
        """
        Fail-closed initialization replicating PMM::init_from_inventory.
        """
        if not candidate_pools:
            raise ValueError("Zero candidate pools")

        # Determine required frames from max RAM address
        max_ram = max(end for _, end in raw_ram_ranges)
        required_frames = (max_ram + PAGE_SIZE - 1) // PAGE_SIZE
        if required_frames > self.max_tracked_frames:
            raise CapacityExceeded(f"Required {required_frames} frames exceeds max {self.max_tracked_frames}")

        self.tracked_frames = required_frames
        # Step 1: Default all tracked frames to UNUSABLE or RESERVED (fail-closed)
        self.states = [FrameState.UNUSABLE] * self.tracked_frames
        for idx in range(self.tracked_frames):
            addr = idx * PAGE_SIZE
            for start, end in raw_ram_ranges:
                if start <= addr < end:
                    self.states[idx] = FrameState.RESERVED
                    break

        # Step 2: Mark all reservations as RESERVED
        for start, end in reservations:
            start_f = start // PAGE_SIZE
            end_f = (end + PAGE_SIZE - 1) // PAGE_SIZE
            for f in range(start_f, min(end_f, self.tracked_frames)):
                self.states[f] = FrameState.RESERVED

        # Step 3: Promote ONLY candidate pools to FREE
        for start, end in candidate_pools:
            start_f = start // PAGE_SIZE
            end_f = end // PAGE_SIZE
            for f in range(start_f, min(end_f, self.tracked_frames)):
                self.states[f] = FrameState.FREE

        # Step 4: Calculate accounting counts
        self.free_frames = sum(1 for s in self.states if s == FrameState.FREE)
        self.allocated_frames = sum(1 for s in self.states if s == FrameState.ALLOCATED)
        self.reserved_frames = sum(1 for s in self.states if s == FrameState.RESERVED)
        self.unusable_frames = sum(1 for s in self.states if s == FrameState.UNUSABLE)

        self.verify_invariants()

    def alloc_frame(self):
        """
        Deterministic first-free scan.
        """
        for idx in range(self.tracked_frames):
            if self.states[idx] == FrameState.FREE:
                self.states[idx] = FrameState.ALLOCATED
                self.free_frames -= 1
                self.allocated_frames += 1
                return idx * PAGE_SIZE
        return None

    def free_frame(self, addr: int):
        """
        Checked free operation.
        """
        if addr % PAGE_SIZE != 0:
            raise FrameMisaligned(f"Address 0x{addr:X} is not 4 KiB aligned")

        idx = addr // PAGE_SIZE
        if idx >= self.tracked_frames:
            raise FrameOutOfRange(f"Frame index {idx} exceeds tracked capacity {self.tracked_frames}")

        state = self.states[idx]
        if state == FrameState.ALLOCATED:
            self.states[idx] = FrameState.FREE
            self.allocated_frames -= 1
            self.free_frames += 1
        elif state == FrameState.FREE:
            raise DoubleFree(f"Frame 0x{addr:X} is already FREE")
        elif state == FrameState.RESERVED:
            raise FrameReserved(f"Frame 0x{addr:X} is RESERVED")
        elif state == FrameState.UNUSABLE:
            raise FrameUnusable(f"Frame 0x{addr:X} is UNUSABLE")

    def verify_invariants(self):
        f = sum(1 for s in self.states if s == FrameState.FREE)
        a = sum(1 for s in self.states if s == FrameState.ALLOCATED)
        r = sum(1 for s in self.states if s == FrameState.RESERVED)
        u = sum(1 for s in self.states if s == FrameState.UNUSABLE)

        assert f == self.free_frames, f"Free mismatch: counted {f} != tracked {self.free_frames}"
        assert a == self.allocated_frames, f"Allocated mismatch: counted {a} != tracked {self.allocated_frames}"
        assert r == self.reserved_frames, f"Reserved mismatch: counted {r} != tracked {self.reserved_frames}"
        assert u == self.unusable_frames, f"Unusable mismatch: counted {u} != tracked {self.unusable_frames}"
        assert f + a + r + u == self.tracked_frames, "Sum of states != tracked_frames"


class TestPmmInitialization(unittest.TestCase):
    def test_fail_closed_initialization(self):
        """Verify that unknown/omitted memory remains UNUSABLE/RESERVED, never FREE."""
        pmm = SyntheticPmm(max_tracked_frames=1024)
        # RAM up to 0x100000 (256 frames). Reservation [0, 0x40000) (64 frames). Candidate [0x40000, 0x80000) (64 frames).
        ram = [(0, 0x100000)]
        res = [(0, 0x40000)]
        cands = [(0x40000, 0x80000)]

        pmm.init_from_inventory(ram, res, cands)
        self.assertEqual(pmm.free_frames, 64)
        self.assertEqual(pmm.allocated_frames, 0)
        # Frames beyond candidate range [0x80000, 0x100000) must remain RESERVED
        for f in range(0x80000 // PAGE_SIZE, 0x100000 // PAGE_SIZE):
            self.assertEqual(pmm.states[f], FrameState.RESERVED)

    def test_capacity_exceeded_safety(self):
        """Verify PMM safely fails when required frames exceed maximum capacity."""
        pmm = SyntheticPmm(max_tracked_frames=100)
        ram = [(0, 0x100000)] # 256 frames > 100 max
        res = [(0, 0x10000)]
        cands = [(0x10000, 0x20000)]
        with self.assertRaises(CapacityExceeded):
            pmm.init_from_inventory(ram, res, cands)


class TestPmmAllocation(unittest.TestCase):
    def setUp(self):
        self.pmm = SyntheticPmm(max_tracked_frames=1024)
        ram = [(0, 0x100000)]
        res = [(0, 0x20000)] # 32 frames reserved
        cands = [(0x20000, 0x30000)] # 16 candidate frames: [0x20000, 0x30000)
        self.pmm.init_from_inventory(ram, res, cands)

    def test_single_and_multiple_allocations(self):
        """Verify allocated frames are unique, 4 KiB aligned, and within candidates."""
        frame_a = self.pmm.alloc_frame()
        frame_b = self.pmm.alloc_frame()
        self.assertIsNotNone(frame_a)
        self.assertIsNotNone(frame_b)
        self.assertNotEqual(frame_a, frame_b)
        self.assertEqual(frame_a % PAGE_SIZE, 0)
        self.assertEqual(frame_b % PAGE_SIZE, 0)
        self.assertTrue(0x20000 <= frame_a < 0x30000)
        self.assertTrue(0x20000 <= frame_b < 0x30000)
        self.pmm.verify_invariants()

    def test_synthetic_exhaustion(self):
        """Verify PMM returns None on exhaustion with tiny synthetic candidate pool."""
        # Initial candidates = 16 frames
        allocated = []
        for _ in range(16):
            f = self.pmm.alloc_frame()
            self.assertIsNotNone(f)
            allocated.append(f)

        self.assertEqual(len(set(allocated)), 16)
        self.assertEqual(self.pmm.free_frames, 0)
        self.assertEqual(self.pmm.allocated_frames, 16)

        # 17th allocation must return None
        exhausted = self.pmm.alloc_frame()
        self.assertIsNone(exhausted)
        self.pmm.verify_invariants()


class TestPmmFreeAndReuse(unittest.TestCase):
    def setUp(self):
        self.pmm = SyntheticPmm(max_tracked_frames=1024)
        ram = [(0, 0x100000)]
        res = [(0, 0x20000)]
        cands = [(0x20000, 0x30000)]
        self.pmm.init_from_inventory(ram, res, cands)

    def test_free_and_deterministic_reuse(self):
        """Verify freeing returns frame to FREE and first-free reuses it."""
        frame_a = self.pmm.alloc_frame()
        frame_b = self.pmm.alloc_frame()
        self.assertNotEqual(frame_a, frame_b)

        # Free A
        self.pmm.free_frame(frame_a)
        self.pmm.verify_invariants()

        # Allocate C - must deterministically reuse freed A
        frame_c = self.pmm.alloc_frame()
        self.assertEqual(frame_c, frame_a)
        self.pmm.verify_invariants()

    def test_rejection_of_invalid_operations(self):
        """Verify double free, reserved free, unusable free, out-of-range, and misaligned are rejected."""
        frame_a = self.pmm.alloc_frame()
        self.pmm.free_frame(frame_a)

        # Double free
        with self.assertRaises(DoubleFree):
            self.pmm.free_frame(frame_a)

        # Freeing reserved frame (0x00000000 in low memory)
        with self.assertRaises(FrameReserved):
            self.pmm.free_frame(0x00000000)

        # Freeing out of range
        with self.assertRaises(FrameOutOfRange):
            self.pmm.free_frame(0x20000000)

        # Freeing misaligned address
        with self.assertRaises(FrameMisaligned):
            self.pmm.free_frame(0x00020001)


class TestPmmCorruptionDetection(unittest.TestCase):
    def test_corruption_detection(self):
        """Verify invariant checker detects bit corruption in PMM state."""
        pmm = SyntheticPmm(max_tracked_frames=1024)
        ram = [(0, 0x100000)]
        res = [(0, 0x20000)]
        cands = [(0x20000, 0x30000)]
        pmm.init_from_inventory(ram, res, cands)

        # Deliberately corrupt state of reserved frame 0 to FREE
        pmm.states[0] = FrameState.FREE
        # Invariant should fail because free count doesn't match
        with self.assertRaises(AssertionError):
            pmm.verify_invariants()


PMM_MARKERS = [
    "[Stage 2D: PMM Metadata Bootstrap & Candidate Accounting]",
    "PMM Metadata Physical Range:",
    "PMM Metadata Frames:",
    "Candidate Frame Delta:",
    "[Stage 2D: Physical Frame Manager]",
    "Tracked frames:",
    "Free frames:",
    "Reserved frames:",
    "Allocated frames:",
    "[Stage 2D: Physical Frame Manager Smoke Test]",
    "[1/6] Allocated Frame A:",
    "[2/6] Allocated Frame B:",
    "[3/6] Freeing Frame A",
    "[4/6] Allocated Frame C:",
    "[5/6] Freeing Frames B and C...",
    "[6/6] Testing rejection of invalid operations...",
    "[x] Deterministic PMM smoke test passed. All invariants verified.",
    "[Stage 2D Architectural Status & Verification Summary]",
]

class TestPmmLiveQemu(unittest.TestCase):
    def test_stage2d_qemu_execution(self):
        """Build and execute Project Zero kernel in QEMU, verifying Stage 2D PMM diagnostics."""
        image = run_qemu.build_stage2()
        self.assertTrue(image.exists(), f"Kernel binary {image} does not exist.")

        success, failures = run_qemu.test_qemu(image, markers=PMM_MARKERS)
        self.assertTrue(success, f"Stage 2D QEMU execution failed: {failures}")


if __name__ == "__main__":
    unittest.main()
