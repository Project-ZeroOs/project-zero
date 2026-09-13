//! Project Zero - Physical Frame Manager (PMM) (Stage 2D)
//!
//! Provides authoritative physical frame ownership and allocation.
//! Strictly consumes the Stage 2C page-aligned frame-candidate inventory.
//!
//! Architectural Principles:
//! - Physical frames represent exactly 4096 bytes.
//! - Strict 64-bit frame identity via `FrameNumber(u64)` and `PhysFrame(u64)` (zero 32-bit truncation).
//! - Explicit 4-state bitmap model: `Free (0b00)`, `Allocated (0b01)`, `Reserved (0b10)`, `Unusable (0b11)`.
//! - Statically reserved metadata bootstrap: PMM metadata is allocated in `.bss` and registered
//!   as a critical reservation before candidate pools are derived, eliminating circular dependencies.
//! - Fail-closed initialization: Every frame defaults to Reserved or Unusable; ONLY Stage 2C
//!   candidate frames are promoted to Free.
//! - Strict allocation API: No allocation of reserved memory, rejection of double free, rejection
//!   of freeing reserved/unusable frames, deterministic exhaustion handling.

use crate::kprintln;
use crate::mm::inventory::PhysicalMemoryInventory;

/// Size of a physical frame in bytes (4 KiB).
pub const PAGE_SIZE: u64 = 4096;

/// Documented maximum physical frames tracked by this bring-up implementation.
/// 65,536 frames * 4096 bytes = 268,435,456 bytes (256 MiB physical address space).
/// If firmware memory map requires tracking more than this limit, initialization
/// fails explicitly and safely with `PmmInitError::CapacityExceeded`.
pub const PMM_MAX_TRACKED_FRAMES: usize = 65536;

/// Number of state bits per frame (2 bits per frame).
pub const BITS_PER_FRAME: usize = 2;

/// Number of frames represented in a single 64-bit machine word (64 / 2 = 32 frames).
pub const FRAMES_PER_U64: usize = 64 / BITS_PER_FRAME;

/// Total number of 64-bit words required to store state for `PMM_MAX_TRACKED_FRAMES`.
/// 65536 / 32 = 2048 words * 8 bytes = 16,384 bytes (exactly 4 pages / 16 KiB).
pub const BITMAP_WORDS: usize = PMM_MAX_TRACKED_FRAMES / FRAMES_PER_U64;

// ====================================================================
// Explicit Frame States (2-bit Authoritative Model)
// ====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameState {
    /// 0b00: Frame belongs to Stage 2C candidate pool and is available for allocation.
    Free = 0b00,
    /// 0b01: Frame has been granted by `alloc_frame()` and is currently owned by caller.
    Allocated = 0b01,
    /// 0b10: Frame is occupied by kernel code/data, stack, tables, or Multiboot structures.
    Reserved = 0b10,
    /// 0b11: Frame corresponds to firmware non-RAM (MMIO, ACPI, BIOS ROM, or out of pool).
    Unusable = 0b11,
}

impl FrameState {
    #[inline(always)]
    pub const fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0b00 => FrameState::Free,
            0b01 => FrameState::Allocated,
            0b10 => FrameState::Reserved,
            0b11 => FrameState::Unusable,
            _ => unreachable!(),
        }
    }

    #[inline(always)]
    pub const fn to_bits(self) -> u8 {
        self as u8
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            FrameState::Free => "FREE",
            FrameState::Allocated => "ALLOCATED",
            FrameState::Reserved => "RESERVED",
            FrameState::Unusable => "UNUSABLE",
        }
    }
}

// ====================================================================
// Checked Frame Representations
// ====================================================================

/// Strongly typed 64-bit physical frame number (index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameNumber(pub u64);

impl FrameNumber {
    #[inline(always)]
    pub const fn from_raw(num: u64) -> Self {
        Self(num)
    }

    #[inline(always)]
    pub fn from_address(addr: u64) -> Result<Self, PmmError> {
        if addr % PAGE_SIZE != 0 {
            Err(PmmError::FrameMisaligned)
        } else {
            Ok(Self(addr / PAGE_SIZE))
        }
    }

    #[inline(always)]
    pub fn to_address(&self) -> Result<u64, PmmError> {
        self.0.checked_mul(PAGE_SIZE).ok_or(PmmError::AddressOverflow)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Strongly typed 64-bit physical frame address (guaranteed 4 KiB aligned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysFrame(pub u64);

impl PhysFrame {
    #[inline(always)]
    pub fn from_address(addr: u64) -> Result<Self, PmmError> {
        if addr % PAGE_SIZE != 0 {
            Err(PmmError::FrameMisaligned)
        } else {
            Ok(Self(addr))
        }
    }

    #[inline(always)]
    pub fn from_number(num: FrameNumber) -> Result<Self, PmmError> {
        let addr = num.to_address()?;
        Ok(Self(addr))
    }

    #[inline(always)]
    pub const fn address(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub const fn frame_number(&self) -> FrameNumber {
        FrameNumber(self.0 / PAGE_SIZE)
    }

    #[inline(always)]
    pub const fn frame_index(&self) -> usize {
        (self.0 / PAGE_SIZE) as usize
    }

    /// Legacy helper for compatibility with earlier mock structures.
    #[inline(always)]
    pub const fn from_index(idx: usize) -> Self {
        Self((idx as u64) * PAGE_SIZE)
    }
}

// ====================================================================
// PMM Errors
// ====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmmError {
    /// Attempted to free a frame that is already in `Free` state (double free).
    DoubleFree,
    /// Attempted to free a frame that is marked `Reserved` (kernel or boot structure).
    FrameReserved,
    /// Attempted to free a frame that is marked `Unusable` (firmware non-RAM).
    FrameUnusable,
    /// Physical frame index is out of the tracked capacity.
    FrameOutOfRange,
    /// Physical address is not 4096-byte aligned.
    FrameMisaligned,
    /// Address computation overflowed 64 bits.
    AddressOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PmmInitError {
    /// The physical memory discovered requires tracking more frames than `PMM_MAX_TRACKED_FRAMES`.
    CapacityExceeded { required: usize, max: usize },
    /// No usable candidate pools found in Stage 2C inventory.
    ZeroCandidates,
}

// ====================================================================
// Statically Reserved PMM Metadata Bootstrap Storage
// ====================================================================

/// 4096-byte aligned static storage for the authoritative 2-bit state bitmap.
/// Resides in `.bss` and is registered as a Project Zero critical reservation
/// BEFORE candidate pools are derived.
#[repr(align(4096))]
pub struct PmmMetadataStorage {
    pub words: [u64; BITMAP_WORDS],
}

#[link_section = ".pmm_metadata"]
pub static mut PMM_METADATA: PmmMetadataStorage = PmmMetadataStorage {
    // Dedicated .pmm_metadata section outside .bss.
    // Explicitly filled with 0xFFFF_FFFF_FFFF_FFFF (0b11 = Unusable) in init_from_inventory.
    words: [0u64; BITMAP_WORDS],
};

/// Returns the physical byte range `[start, end)` occupied by PMM metadata storage.
pub fn get_pmm_metadata_range() -> (u64, u64) {
    let start = crate::hal::arch::x86_64::gdt::virt_to_phys(&raw const PMM_METADATA as u64);
    let end = start + (core::mem::size_of::<PmmMetadataStorage>() as u64);
    (start, end)
}

// ====================================================================
// Physical Memory Manager
// ====================================================================

pub struct PhysicalMemoryManager {
    tracked_frames: usize,
    free_frames: usize,
    allocated_frames: usize,
    reserved_frames: usize,
    unusable_frames: usize,
    initialized: bool,
}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            tracked_frames: 0,
            free_frames: 0,
            allocated_frames: 0,
            reserved_frames: 0,
            unusable_frames: 0,
            initialized: false,
        }
    }

    #[inline(always)]
    fn storage_ref(&self) -> &PmmMetadataStorage {
        unsafe { &*(&raw const PMM_METADATA) }
    }

    #[inline(always)]
    fn storage_mut(&mut self) -> &mut PmmMetadataStorage {
        unsafe { &mut *(&raw mut PMM_METADATA) }
    }

    /// Reads the authoritative state of a frame by index.
    #[inline(always)]
    pub fn get_state(&self, idx: usize) -> FrameState {
        if idx >= PMM_MAX_TRACKED_FRAMES {
            return FrameState::Unusable;
        }
        let word_idx = idx / FRAMES_PER_U64;
        let bit_offset = (idx % FRAMES_PER_U64) * BITS_PER_FRAME;
        let word = self.storage_ref().words[word_idx];
        let bits = ((word >> bit_offset) & 0b11) as u8;
        FrameState::from_bits(bits)
    }

    /// Sets the authoritative state of a frame by index.
    #[inline(always)]
    pub fn set_state(&mut self, idx: usize, state: FrameState) {
        if idx >= PMM_MAX_TRACKED_FRAMES {
            return;
        }
        let word_idx = idx / FRAMES_PER_U64;
        let bit_offset = (idx % FRAMES_PER_U64) * BITS_PER_FRAME;
        let mask = !(0b11u64 << bit_offset);
        let val = (state.to_bits() as u64) << bit_offset;
        let storage = self.storage_mut();
        storage.words[word_idx] = (storage.words[word_idx] & mask) | val;
    }

    /// Initializes PMM strictly from the Stage 2C page-aligned frame candidates.
    /// Follows fail-closed semantics:
    /// 1. Determines required frames to track based on firmware Type 1 RAM extent.
    /// 2. Initializes all tracked frames to RESERVED or UNUSABLE.
    /// 3. Registers all Stage 2C reservations as RESERVED.
    /// 4. Promotes ONLY verified Stage 2C page-aligned candidate frames to FREE.
    pub fn init_from_inventory(
        &mut self,
        inv: &PhysicalMemoryInventory,
    ) -> Result<(), PmmInitError> {
        if inv.frame_candidate_count == 0 {
            return Err(PmmInitError::ZeroCandidates);
        }

        // 0. Fail-closed: fill entire bitmap with 0xFFFF_FFFF_FFFF_FFFF (0b11 = Unusable)
        for w in self.storage_mut().words.iter_mut() {
            *w = !0u64;
        }

        // 1. Determine maximum physical RAM address to derive required frame count
        let mut max_ram_addr: u64 = 0;
        for i in 0..inv.raw_count {
            let reg = &inv.raw_regions[i];
            if reg.region_type == crate::mm::inventory::MemoryRegionType::AvailableRAM {
                if reg.end > max_ram_addr {
                    max_ram_addr = reg.end;
                }
            }
        }
        // Also ensure candidate pools are completely encompassed
        for i in 0..inv.frame_candidate_count {
            let cand = &inv.frame_candidates[i];
            if cand.end > max_ram_addr {
                max_ram_addr = cand.end;
            }
        }

        let required_frames = ((max_ram_addr + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
        if required_frames > PMM_MAX_TRACKED_FRAMES {
            return Err(PmmInitError::CapacityExceeded {
                required: required_frames,
                max: PMM_MAX_TRACKED_FRAMES,
            });
        }
        self.tracked_frames = required_frames;

        // 2. Fail-Closed Initialization:
        // Set all tracked frames to Unusable or Reserved (based on Type 1 RAM presence).
        // Frame default is Unusable; if an address falls within Type 1 RAM, it is initially Reserved.
        for idx in 0..self.tracked_frames {
            let frame_addr = (idx as u64) * PAGE_SIZE;
            let mut is_type1 = false;
            for r_idx in 0..inv.raw_count {
                let reg = &inv.raw_regions[r_idx];
                if reg.region_type == crate::mm::inventory::MemoryRegionType::AvailableRAM
                    && frame_addr >= reg.start
                    && frame_addr < reg.end
                {
                    is_type1 = true;
                    break;
                }
            }
            if is_type1 {
                self.set_state(idx, FrameState::Reserved);
            } else {
                self.set_state(idx, FrameState::Unusable);
            }
        }

        // 3. Mark all Project Zero reservations explicitly as RESERVED
        for r_idx in 0..inv.reservation_count {
            let res = &inv.reservations[r_idx];
            let start_f = (res.start / PAGE_SIZE) as usize;
            let end_f = ((res.end + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
            let bounded_end = if end_f > self.tracked_frames { self.tracked_frames } else { end_f };
            for f in start_f..bounded_end {
                self.set_state(f, FrameState::Reserved);
            }
        }

        // 4. Explicit Promotion to FREE:
        // ONLY frames strictly belonging to Stage 2C page-aligned candidates are promoted to Free.
        for c_idx in 0..inv.frame_candidate_count {
            let cand = &inv.frame_candidates[c_idx];
            let start_f = (cand.start / PAGE_SIZE) as usize;
            let end_f = (cand.end / PAGE_SIZE) as usize;
            let bounded_end = if end_f > self.tracked_frames { self.tracked_frames } else { end_f };
            for f in start_f..bounded_end {
                self.set_state(f, FrameState::Free);
            }
        }

        // 5. Compute Exact Frame Accounting Counts
        self.free_frames = 0;
        self.allocated_frames = 0;
        self.reserved_frames = 0;
        self.unusable_frames = 0;

        for f in 0..self.tracked_frames {
            match self.get_state(f) {
                FrameState::Free => self.free_frames += 1,
                FrameState::Allocated => self.allocated_frames += 1,
                FrameState::Reserved => self.reserved_frames += 1,
                FrameState::Unusable => self.unusable_frames += 1,
            }
        }

        self.initialized = true;

        // Verify Invariants immediately upon initialization
        self.verify_invariants().map_err(|_| PmmInitError::ZeroCandidates)?;

        Ok(())
    }

    /// Allocates a single 4 KiB physical frame using a deterministic first-free bitmap scan.
    /// Guarantees:
    /// - Returns only `Free` frames.
    /// - Returned address is 4 KiB aligned.
    /// - Returned address belongs to a verified Stage 2C candidate pool.
    /// - Transitions frame state from `Free` to `Allocated`.
    pub fn alloc_frame(&mut self) -> Option<PhysFrame> {
        if !self.initialized || self.free_frames == 0 {
            return None;
        }

        let words_to_scan = (self.tracked_frames + FRAMES_PER_U64 - 1) / FRAMES_PER_U64;
        for word_idx in 0..words_to_scan {
            let word = self.storage_ref().words[word_idx];

            // Fast rejection: A 2-bit pair is 0b00 (Free) iff (bit1 | bit0) == 0.
            // If every pair has at least one bit set, ((odd >> 1) | even) == 0x5555_5555_5555_5555.
            let odd_bits = word & 0xAAAA_AAAA_AAAA_AAAA;
            let even_bits = word & 0x5555_5555_5555_5555;
            let active_mask = (odd_bits >> 1) | even_bits;
            if active_mask == 0x5555_5555_5555_5555 {
                continue; // No free frames in this word
            }

            // Word has at least one free frame slot
            let base_idx = word_idx * FRAMES_PER_U64;
            for slot in 0..FRAMES_PER_U64 {
                let frame_idx = base_idx + slot;
                if frame_idx >= self.tracked_frames {
                    break;
                }
                if self.get_state(frame_idx) == FrameState::Free {
                    self.set_state(frame_idx, FrameState::Allocated);
                    self.free_frames = self.free_frames.saturating_sub(1);
                    self.allocated_frames += 1;

                    let addr = (frame_idx as u64) * PAGE_SIZE;
                    return Some(PhysFrame(addr));
                }
            }
        }

        None
    }

    /// Legacy alias for compatibility with VMM and benchmarks.
    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        self.alloc_frame()
    }

    /// Frees a previously allocated 4 KiB physical frame.
    /// Guarantees:
    /// - Successfully freed frames transition from `Allocated` to `Free`.
    /// - Double free of already `Free` frame is strictly rejected (`DoubleFree`).
    /// - Freeing a `Reserved` frame is strictly rejected (`FrameReserved`).
    /// - Freeing an `Unusable` frame is strictly rejected (`FrameUnusable`).
    /// - Invalid / out-of-range frame numbers are rejected (`FrameOutOfRange`).
    /// - Misaligned physical addresses are rejected (`FrameMisaligned`).
    pub fn free_frame(&mut self, frame: PhysFrame) -> Result<(), PmmError> {
        if frame.address() % PAGE_SIZE != 0 {
            return Err(PmmError::FrameMisaligned);
        }

        let idx = frame.frame_index();
        if idx >= self.tracked_frames {
            return Err(PmmError::FrameOutOfRange);
        }

        match self.get_state(idx) {
            FrameState::Allocated => {
                self.set_state(idx, FrameState::Free);
                self.allocated_frames = self.allocated_frames.saturating_sub(1);
                self.free_frames += 1;
                Ok(())
            }
            FrameState::Free => Err(PmmError::DoubleFree),
            FrameState::Reserved => Err(PmmError::FrameReserved),
            FrameState::Unusable => Err(PmmError::FrameUnusable),
        }
    }

    /// Returns current runtime statistics for PMM.
    pub fn stats(&self) -> PmmStats {
        PmmStats {
            tracked_frames: self.tracked_frames,
            free_frames: self.free_frames,
            allocated_frames: self.allocated_frames,
            reserved_frames: self.reserved_frames,
            unusable_frames: self.unusable_frames,
        }
    }

    /// Verifies all architectural Stage 2D invariants:
    /// 1. `free + allocated + reserved + unusable == tracked_frames`
    /// 2. All states are mutually disjoint by construction in 2-bit storage.
    /// 3. Bitmap counts match internal tracking counters.
    pub fn verify_invariants(&self) -> Result<(), &'static str> {
        let mut count_free = 0usize;
        let mut count_allocated = 0usize;
        let mut count_reserved = 0usize;
        let mut count_unusable = 0usize;

        for f in 0..self.tracked_frames {
            match self.get_state(f) {
                FrameState::Free => count_free += 1,
                FrameState::Allocated => count_allocated += 1,
                FrameState::Reserved => count_reserved += 1,
                FrameState::Unusable => count_unusable += 1,
            }
        }

        if count_free != self.free_frames {
            return Err("PMM Invariant Violated: Free frames count mismatch!");
        }
        if count_allocated != self.allocated_frames {
            return Err("PMM Invariant Violated: Allocated frames count mismatch!");
        }
        if count_reserved != self.reserved_frames {
            return Err("PMM Invariant Violated: Reserved frames count mismatch!");
        }
        if count_unusable != self.unusable_frames {
            return Err("PMM Invariant Violated: Unusable frames count mismatch!");
        }
        if count_free + count_allocated + count_reserved + count_unusable != self.tracked_frames {
            return Err("PMM Invariant Violated: Total states sum != tracked_frames!");
        }

        Ok(())
    }

    /// Prints runtime diagnostics to serial console.
    pub fn print_diagnostics(&self, _inv: &PhysicalMemoryInventory) {
        let (meta_start, meta_end) = get_pmm_metadata_range();
        let meta_frames = (meta_end - meta_start) / PAGE_SIZE;

        kprintln!("\n[Stage 2D: Physical Frame Manager]");
        kprintln!("  Tracked frames:        {:>7}", self.tracked_frames);
        kprintln!("  Free frames:           {:>7}", self.free_frames);
        kprintln!("  Reserved frames:       {:>7}", self.reserved_frames);
        kprintln!("  Unusable frames:       {:>7}", self.unusable_frames);
        kprintln!("  Allocated frames:      {:>7}", self.allocated_frames);
        kprintln!("  PMM metadata range:    [0x{:016X}, 0x{:016X})", meta_start, meta_end);
        kprintln!("  PMM metadata frames:   {:>7} ({} KiB)", meta_frames, meta_frames * 4);
    }

    /// Executes the mandatory Stage 2D deterministic smoke test:
    /// - Allocate A, Allocate B (verify A != B, 4 KiB alignment, candidate membership)
    /// - Free A
    /// - Allocate C (verify C is valid, deterministic first-free reuse)
    /// - Free B and C
    /// - Test invalid operations (double free, freeing reserved frame, misaligned free)
    /// - Verify invariants
    pub fn run_smoke_test(&mut self, inv: &PhysicalMemoryInventory) {
        kprintln!("\n[Stage 2D: Physical Frame Manager Smoke Test]");

        let initial_free = self.free_frames;
        let initial_alloc = self.allocated_frames;

        // 1. Allocate Frame A
        let frame_a = self.alloc_frame().expect("Smoke test: failed to allocate frame A");
        kprintln!("  [1/6] Allocated Frame A: phys=0x{:016X}, frame_num={}", frame_a.address(), frame_a.frame_number().as_u64());
        assert_eq!(frame_a.address() % PAGE_SIZE, 0, "Frame A is not 4 KiB aligned!");
        assert!(inv.is_candidate_address(frame_a.address()), "Frame A does not belong to candidate pool!");

        // 2. Allocate Frame B
        let frame_b = self.alloc_frame().expect("Smoke test: failed to allocate frame B");
        kprintln!("  [2/6] Allocated Frame B: phys=0x{:016X}, frame_num={}", frame_b.address(), frame_b.frame_number().as_u64());
        assert_eq!(frame_b.address() % PAGE_SIZE, 0, "Frame B is not 4 KiB aligned!");
        assert!(inv.is_candidate_address(frame_b.address()), "Frame B does not belong to candidate pool!");
        assert!(frame_a.address() != frame_b.address(), "Frames A and B must be unique!");

        // 3. Free Frame A
        kprintln!("  [3/6] Freeing Frame A (phys=0x{:016X})...", frame_a.address());
        let free_a_res = self.free_frame(frame_a);
        assert_eq!(free_a_res, Ok(()), "Failed to free valid allocated frame A!");

        // 4. Allocate Frame C (Deterministic First-Free Reuse)
        let frame_c = self.alloc_frame().expect("Smoke test: failed to allocate frame C");
        kprintln!("  [4/6] Allocated Frame C: phys=0x{:016X}, frame_num={}", frame_c.address(), frame_c.frame_number().as_u64());
        assert_eq!(frame_c.address() % PAGE_SIZE, 0, "Frame C is not 4 KiB aligned!");
        assert!(inv.is_candidate_address(frame_c.address()), "Frame C does not belong to candidate pool!");
        // Under deterministic first-free scan, frame C must reuse freed frame A
        assert_eq!(frame_c.address(), frame_a.address(), "First-free policy did not reuse freed frame A!");

        // 5. Free Frames B and C
        kprintln!("  [5/6] Freeing Frames B and C...");
        assert_eq!(self.free_frame(frame_b), Ok(()), "Failed to free frame B!");
        assert_eq!(self.free_frame(frame_c), Ok(()), "Failed to free frame C!");

        // 6. Test Invalid Operations:
        kprintln!("  [6/6] Testing rejection of invalid operations...");
        // Double free of frame C (which is now free)
        let df_res = self.free_frame(frame_c);
        assert_eq!(df_res, Err(PmmError::DoubleFree), "Double free was not rejected!");

        // Freeing reserved low memory (Frame 0: 0x00000000)
        let reserved_frame = PhysFrame(0x0000_0000);
        let res_free = self.free_frame(reserved_frame);
        assert_eq!(res_free, Err(PmmError::FrameReserved), "Freeing reserved frame was not rejected!");

        // Freeing misaligned address
        let misaligned_frame = PhysFrame(0x0015_D001);
        let misaligned_res = self.free_frame(misaligned_frame);
        assert_eq!(misaligned_res, Err(PmmError::FrameMisaligned), "Misaligned frame free was not rejected!");

        // Verify restoration of initial state
        assert_eq!(self.free_frames, initial_free, "Free frames count not restored after smoke test!");
        assert_eq!(self.allocated_frames, initial_alloc, "Allocated frames count not restored after smoke test!");

        // Verify all invariants
        self.verify_invariants().expect("PMM invariants violated after smoke test!");
        kprintln!("  [x] Deterministic PMM smoke test passed. All invariants verified.");
    }
}

pub static mut PMM: PhysicalMemoryManager = PhysicalMemoryManager::new();

#[derive(Debug, Clone, Copy)]
pub struct PmmStats {
    pub tracked_frames: usize,
    pub free_frames: usize,
    pub allocated_frames: usize,
    pub reserved_frames: usize,
    pub unusable_frames: usize,
}
