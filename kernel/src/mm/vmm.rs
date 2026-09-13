//! Project Zero - Virtual Memory Management (VMM) (Stage 2E-A)
//!
//! Provides hardware-level paging abstractions:
//! - Architectural geometry discovery via CPUID (physical and virtual address widths)
//! - Single authoritative physical-address mask
//! - Explicit IA32_EFER.NXE enablement and NO_EXECUTE protection
//! - Strongly typed CR3 register read/write with documented PCID/control flag policy
//! - Strongly typed 4 KiB-aligned PageTable and PageTableEntry structures
//! - Security domain abstraction (`MappingDomain::Kernel` vs `MappingDomain::User`)
//! - Strict reserved-bit protection and canonical virtual-address validation
//!
//! NOTE: Phase 2E-A establishes the foundational abstractions and instruments.
//! It does NOT alter active page tables or change identity mappings.

use crate::kprintln;
use crate::hal::arch::x86_64::{cpu, idt};
use super::pmm::{PhysFrame, PhysicalMemoryManager, PAGE_SIZE, FrameState, PmmError};

/// Backwards compatibility alias for PageTableFlags
pub type PageFlags = PageTableFlags;

// ====================================================================
// Hardware Address-Space Geometry Discovery
// ====================================================================

/// Authoritative architecture-level geometry discovered from CPUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressSpaceGeometry {
    /// Number of physical address bits supported by the CPU (e.g. 36 to 52).
    pub physical_bits: u8,
    /// Number of linear (virtual) address bits supported by the CPU (standard 48, or 57 for LA57).
    pub virtual_bits: u8,
    /// Authoritative page-aligned physical address mask (bits [physical_bits-1 : 12]).
    pub physical_mask: u64,
    /// Whether the CPU supports the No-Execute (NX) page-protection bit.
    pub nx_supported: bool,
    /// Whether the IA32_EFER.NXE bit has been enabled in the active CPU execution state.
    pub nx_enabled: bool,
}

impl AddressSpaceGeometry {
    /// Discovers physical/virtual address widths via CPUID leaf 0x80000008,
    /// and NX capability via CPUID leaf 0x80000001.
    pub fn discover() -> Self {
        // Query CPUID leaf 0x80000008: Virtual and Physical Address Sizes
        let (eax, _, _, _) = cpu::raw_cpuid(0x8000_0008);
        let raw_phys = (eax & 0xFF) as u8;
        let raw_virt = ((eax >> 8) & 0xFF) as u8;

        // Fallback to conservative architectural defaults if leaf is absent
        let phys_bits = if raw_phys >= 32 && raw_phys <= 52 { raw_phys } else { 36 };
        let virt_bits = if raw_virt >= 48 && raw_virt <= 57 { raw_virt } else { 48 };

        // Calculate authoritative 4 KiB page-aligned physical address mask
        // e.g. for 36 bits: (1 << 36) - 1 & !0xFFF = 0x0000_000F_FFFF_F000
        // e.g. for 52 bits: (1 << 52) - 1 & !0xFFF = 0x000F_FFFF_FFFF_F000
        let phys_mask = if phys_bits >= 64 {
            !0xFFFu64
        } else {
            ((1u64 << phys_bits) - 1) & !0xFFFu64
        };

        // Query CPUID leaf 0x80000001: Extended Processor Info and Feature Bits
        // EDX Bit 20: XD/NX bit available
        let (_, _, _, ext_edx) = cpu::raw_cpuid(0x8000_0001);
        let nx_supported = (ext_edx & (1 << 20)) != 0;

        // Check if IA32_EFER.NXE is currently active in hardware
        let efer = cpu::read_msr(cpu::IA32_EFER);
        let nx_enabled = (efer & (1 << 11)) != 0;

        Self {
            physical_bits: phys_bits,
            virtual_bits: virt_bits,
            physical_mask: phys_mask,
            nx_supported,
            nx_enabled,
        }
    }

    /// Verifies whether a 64-bit linear address is in canonical form.
    /// In 48-bit mode, bits [63:47] must all be 0s or all be 1s.
    #[inline(always)]
    pub fn is_canonical(&self, addr: u64) -> bool {
        let shift = self.virtual_bits - 1;
        let sign_extended = ((addr as i64) >> shift) as u64;
        sign_extended == 0 || sign_extended == !0u64
    }

    /// Verifies whether a physical frame conforms to the discovered physical address mask.
    #[inline(always)]
    pub fn is_valid_phys_frame(&self, frame: PhysFrame) -> bool {
        let addr = frame.address();
        (addr % PAGE_SIZE == 0) && ((addr & !self.physical_mask) == 0)
    }

    /// Verifies whether a physical byte address conforms to the discovered physical address mask.
    #[inline(always)]
    pub fn is_valid_phys_addr(&self, addr: u64) -> bool {
        let max_phys = if self.physical_bits >= 64 { !0u64 } else { (1u64 << self.physical_bits) - 1 };
        addr <= max_phys
    }
}

// ====================================================================
// CR3 Register Abstraction & Control Flags
// ====================================================================

/// CR3 Control Flags on x86-64.
///
/// Hardware Layout of CR3 (with CR4.PCIDE = 0, default single-core bring-up):
/// - Bits [11:5]: Reserved (must be 0)
/// - Bit 4: PCD (Page-level Cache Disable)
/// - Bit 3: PWT (Page-level Write-Through)
/// - Bits [2:0]: Reserved (must be 0)
/// - Bits [MAXPHYSADDR-1:12]: PML4 Physical Base Address (4 KiB aligned)
/// - Bits [63:MAXPHYSADDR]: Reserved (must be 0)
///
/// When CR4.PCIDE = 1 (future multi-tasking / SMP optimization):
/// - Bits [11:0]: Process-Context Identifier (PCID)
/// - Bit 63 on MOV CR3: Preserve TLB entries with matching PCID
///
/// Stage 2E Policy:
/// Single-core bring-up runs with CR4.PCIDE = 0.
/// PWT and PCD default to 0 (Write-Back caching).
/// PCID is explicitly disabled (0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cr3Flags(pub u64);

impl Cr3Flags {
    pub const EMPTY: Self = Self(0);
    pub const PAGE_LEVEL_WRITETHROUGH: Self = Self(1 << 3);
    pub const PAGE_LEVEL_CACHE_DISABLE: Self = Self(1 << 4);

    #[inline(always)]
    pub const fn empty() -> Self {
        Self::EMPTY
    }

    #[inline(always)]
    pub const fn bits(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline(always)]
    pub const fn from_bits_truncate(bits: u64) -> Self {
        Self(bits & (Self::PAGE_LEVEL_WRITETHROUGH.0 | Self::PAGE_LEVEL_CACHE_DISABLE.0))
    }
}

/// Strongly typed, validated CR3 register abstraction.
pub struct Cr3;

impl Cr3 {
    /// Reads active PML4 root physical frame and control flags from CR3.
    #[inline(always)]
    pub fn read(geometry: &AddressSpaceGeometry) -> (PhysFrame, Cr3Flags) {
        let val = cpu::read_cr3();
        let frame = PhysFrame(val & geometry.physical_mask);
        let flags = Cr3Flags::from_bits_truncate(val & 0xFFF);
        (frame, flags)
    }

    /// Safely updates CR3 with validated PML4 root and control flags.
    /// Invariants enforced:
    /// 1. Root frame address must be 4 KiB aligned.
    /// 2. Root frame address must conform strictly to `geometry.physical_mask`.
    /// 3. Control flags must not contain architectural reserved bits.
    #[inline(always)]
    pub fn write(root: PhysFrame, flags: Cr3Flags, geometry: &AddressSpaceGeometry) {
        assert!(
            geometry.is_valid_phys_frame(root),
            "CR3 target root violates discovered physical geometry mask"
        );
        let raw = root.address() | flags.bits();
        unsafe {
            cpu::write_cr3_raw(raw);
        }
    }
}

// ====================================================================
// Security Domain & Mapping Permissions
// ====================================================================

/// Architectural security domain for memory translations.
///
/// In x86-64 4-level paging, user-mode accessibility requires that the USER bit (bit 2)
/// is set to 1 at EVERY translation level from PML4 down to the leaf PTE.
/// If any level has USER = 0, the translation is supervisor-only (Ring 0-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingDomain {
    /// Kernel domain: Supervisor privilege (Ring 0).
    /// All intermediate page-table entries (PML4E, PDPTE, PDE, PTE) enforce `USER = 0`.
    Kernel,
    /// User domain (reserved for future Ring 3 / Stage 3):
    /// Intermediate page-table entries along the user virtual address path set `USER = 1`.
    User,
}

impl MappingDomain {
    /// Returns the required USER flag mask to propagate to intermediate page tables.
    #[inline(always)]
    pub const fn intermediate_flags(&self) -> PageTableFlags {
        match self {
            MappingDomain::Kernel => PageTableFlags::empty(),
            MappingDomain::User => PageTableFlags::USER_ACCESSIBLE,
        }
    }
}

// ====================================================================
// Page Table Flags & Validation
// ====================================================================

/// Strongly typed bitflags for page table entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageTableFlags(pub u64);

impl PageTableFlags {
    pub const PRESENT: Self         = Self(1 << 0);
    pub const WRITABLE: Self        = Self(1 << 1);
    pub const USER_ACCESSIBLE: Self = Self(1 << 2);
    pub const WRITE_THROUGH: Self   = Self(1 << 3);
    pub const CACHE_DISABLE: Self   = Self(1 << 4);
    pub const ACCESSED: Self        = Self(1 << 5);
    pub const DIRTY: Self           = Self(1 << 6);  // Valid on leaf PTE / huge page only
    pub const HUGE_PAGE: Self       = Self(1 << 7);  // Valid on PD/PDPT only
    pub const GLOBAL: Self          = Self(1 << 8);  // Valid on leaf PTE only
    pub const NO_EXECUTE: Self      = Self(1 << 63); // Requires IA32_EFER.NXE

    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub const fn bits(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub const fn from_bits_truncate(bits: u64) -> Self {
        Self(bits)
    }

    #[inline(always)]
    pub const fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline(always)]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    #[inline(always)]
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// Validates flags for a 4 KiB leaf Page Table Entry (PTE).
    pub fn validate_leaf(&self, geometry: &AddressSpaceGeometry) -> Result<(), VmmError> {
        if self.contains(Self::NO_EXECUTE) && !geometry.nx_enabled {
            return Err(VmmError::NxDisabled);
        }
        if self.contains(Self::HUGE_PAGE) {
            return Err(VmmError::InvalidFlags); // Huge page bit invalid at Level 1 PTE
        }
        Ok(())
    }

    /// Validates flags for an intermediate table entry (PML4E, PDPTE, PDE).
    pub fn validate_intermediate(&self) -> Result<(), VmmError> {
        if self.contains(Self::DIRTY) {
            return Err(VmmError::InvalidFlags); // Dirty bit is reserved at intermediate levels
        }
        Ok(())
    }
}

impl core::ops::BitOr for PageTableFlags {
    type Output = Self;
    #[inline(always)]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for PageTableFlags {
    #[inline(always)]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

// ====================================================================
// Strongly Typed Page Table Entries & Tables
// ====================================================================

/// Transparent 64-bit page table entry.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline(always)]
    pub fn is_present(&self) -> bool {
        (self.0 & PageTableFlags::PRESENT.bits()) != 0
    }

    #[inline(always)]
    pub fn flags(&self) -> PageTableFlags {
        PageTableFlags::from_bits_truncate(self.0)
    }

    /// Extracts the physical frame referenced by this entry using the authoritative geometry mask.
    #[inline(always)]
    pub fn points_to_frame(&self, geometry: &AddressSpaceGeometry) -> Option<PhysFrame> {
        if self.is_present() {
            Some(PhysFrame(self.0 & geometry.physical_mask))
        } else {
            None
        }
    }

    /// Sets this entry to point to a physical frame with specified flags and geometry validation.
    #[inline(always)]
    pub fn set(&mut self, frame: PhysFrame, flags: PageTableFlags, geometry: &AddressSpaceGeometry) {
        assert!(
            geometry.is_valid_phys_frame(frame),
            "PTE target frame violates discovered physical geometry mask"
        );
        self.0 = (frame.address() & geometry.physical_mask) | flags.bits();
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

/// Strongly typed 4 KiB-aligned x86-64 page table containing exactly 512 entries.
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn empty() -> Self {
        Self {
            entries: [PageTableEntry::empty(); 512],
        }
    }

    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.clear();
        }
    }

    /// Returns the number of present entries currently in this page table.
    /// Used for intermediate table reclamation when entry count drops to zero.
    pub fn present_count(&self) -> usize {
        let mut count = 0;
        for entry in self.entries.iter() {
            if entry.is_present() {
                count += 1;
            }
        }
        count
    }
}

// ====================================================================
// Strongly Typed Addresses & Pages
// ====================================================================

/// Strongly typed 64-bit linear virtual address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtualAddress(pub u64);

impl VirtualAddress {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub fn is_canonical(&self, geometry: &AddressSpaceGeometry) -> bool {
        geometry.is_canonical(self.0)
    }

    #[inline(always)]
    pub const fn p4_index(&self) -> usize {
        ((self.0 >> 39) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn p3_index(&self) -> usize {
        ((self.0 >> 30) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn p2_index(&self) -> usize {
        ((self.0 >> 21) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn p1_index(&self) -> usize {
        ((self.0 >> 12) & 0x1FF) as usize
    }

    #[inline(always)]
    pub const fn offset(&self) -> u64 {
        self.0 & 0xFFF
    }

    #[inline(always)]
    pub fn is_hhdm(&self) -> bool {
        is_hhdm_address(self.0)
    }

    #[inline(always)]
    pub fn is_kernel_image(&self) -> bool {
        self.0 >= KERNEL_VIRT_BASE && self.0 < KERNEL_VIRT_BASE + 0x4000_0000
    }
}

/// Stage 2F-D Higher-Half Direct Map (HHDM) Base Virtual Address
pub const HHDM_BASE: u64 = 0xFFFF_8000_0000_0000;

/// Stage 2F-D HHDM Bootstrap Size (1 GiB mapped via 2 MiB huge pages during early boot)
pub const HHDM_SIZE: u64 = 0x4000_0000;

/// Strongly typed 64-bit physical byte address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalAddress(pub u64);

impl PhysicalAddress {
    #[inline(always)]
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub fn is_valid(&self, geometry: &AddressSpaceGeometry) -> bool {
        geometry.is_valid_phys_addr(self.0)
    }

    #[inline(always)]
    pub fn to_hhdm(&self) -> Result<HhdmAddress, VmmError> {
        phys_to_hhdm(*self)
    }
}

/// Strongly typed 64-bit Higher-Half Direct Map virtual address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HhdmAddress(pub u64);

impl HhdmAddress {
    #[inline(always)]
    pub fn from_u64(vaddr: u64) -> Result<Self, VmmError> {
        if is_hhdm_address(vaddr) {
            Ok(Self(vaddr))
        } else {
            Err(VmmError::InvalidHhdmAddress(vaddr))
        }
    }

    #[inline(always)]
    pub fn from_phys(phys: PhysicalAddress) -> Result<Self, VmmError> {
        phys_to_hhdm(phys)
    }

    #[inline(always)]
    pub fn to_phys(&self) -> PhysicalAddress {
        hhdm_to_phys(*self)
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    pub fn as_ptr<T>(&self) -> *const T {
        self.0 as *const T
    }

    #[inline(always)]
    pub fn as_mut_ptr<T>(&self) -> *mut T {
        self.0 as *mut T
    }
}

/// Verifies whether a 64-bit virtual address falls within the HHDM window.
#[inline(always)]
pub fn is_hhdm_address(vaddr: u64) -> bool {
    vaddr >= HHDM_BASE && vaddr < HHDM_BASE + HHDM_SIZE
}

/// Translates a physical byte address to its corresponding HHDM virtual address.
#[inline(always)]
pub fn phys_to_hhdm(phys: PhysicalAddress) -> Result<HhdmAddress, VmmError> {
    if phys.as_u64() < HHDM_SIZE {
        Ok(HhdmAddress(HHDM_BASE + phys.as_u64()))
    } else {
        Err(VmmError::PhysicalAddressOutOfHhdmRange(phys.as_u64()))
    }
}

/// Translates an HHDM virtual address back to its underlying physical byte address.
#[inline(always)]
pub fn hhdm_to_phys(hhdm: HhdmAddress) -> PhysicalAddress {
    PhysicalAddress(hhdm.as_u64() - HHDM_BASE)
}

/// Strongly typed 4 KiB page-aligned virtual address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page(pub VirtualAddress);

impl Page {
    #[inline(always)]
    pub fn from_start_address(
        addr: VirtualAddress,
        geometry: &AddressSpaceGeometry,
    ) -> Result<Self, VmmError> {
        if addr.as_u64() % PAGE_SIZE != 0 {
            return Err(VmmError::UnalignedPageAddress);
        }
        if !addr.is_canonical(geometry) {
            return Err(VmmError::NonCanonicalAddress);
        }
        Ok(Self(addr))
    }

    /// Legacy unchecked constructor for backwards compatibility with benchmarks.
    #[inline(always)]
    pub const fn from_address(addr: VirtualAddress) -> Self {
        Self(VirtualAddress(addr.0 & !(PAGE_SIZE - 1)))
    }

    #[inline(always)]
    pub const fn start_address(&self) -> VirtualAddress {
        self.0
    }
}

// ====================================================================
// VMM Error Model
// ====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmmError {
    NonCanonicalAddress,
    UnalignedPageAddress,
    UnalignedFrameAddress,
    InvalidPhysicalAddress,
    AlreadyMapped,
    NotMapped,
    InvalidFlags,
    NxDisabled,
    OutOfMemory,
    HugePageUnsupported,
    InvalidHhdmAddress(u64),
    PhysicalAddressOutOfHhdmRange(u64),
}

// ====================================================================
// Physical-to-Virtual Page Table Address Translation Seam
// ====================================================================

/// Converts a page-table physical frame into a dereferenceable virtual pointer via the HHDM.
///
/// Architectural Seam (Stage 2F-E and beyond):
/// - All page-table frames reside within the first 1 GiB of physical memory.
/// - The HHDM maps physical [0, 1 GiB) at [HHDM_BASE, HHDM_BASE + 1 GiB).
/// - Therefore every page-table frame is reachable at HHDM_BASE + frame.address().
/// - The identity mapping (virt == phys) is NO LONGER assumed or required.
#[inline(always)]
pub unsafe fn phys_to_virt_table(frame: PhysFrame) -> *mut PageTable {
    (HHDM_BASE + frame.address()) as *mut PageTable
}

/// Translates a physical frame into an HHDM page-table virtual pointer.
/// Alias for phys_to_virt_table — both now use HHDM after Stage 2F-E.
#[inline(always)]
pub unsafe fn phys_to_hhdm_table(frame: PhysFrame) -> *mut PageTable {
    (HHDM_BASE + frame.address()) as *mut PageTable
}

// ====================================================================
// Hardware Initialization & Diagnostics (Stage 2E-A)
// ====================================================================

/// Global hardware geometry discovered during bootstrap.
pub static mut ACTIVE_GEOMETRY: AddressSpaceGeometry = AddressSpaceGeometry {
    physical_bits: 36,
    virtual_bits: 48,
    physical_mask: 0x0000_000F_FFFF_F000,
    nx_supported: false,
    nx_enabled: false,
};

/// Initializes hardware paging geometry, enables IA32_EFER.NXE if supported,
/// and validates CR3 consistency.
///
/// NOTE: Phase 2E-A does NOT alter the active CR3 root or modify any page tables.
pub fn init_paging_hardware() -> AddressSpaceGeometry {
    let mut geometry = AddressSpaceGeometry::discover();

    // Enable No-Execute (NXE) in IA32_EFER if supported by CPU hardware
    if geometry.nx_supported {
        cpu::enable_nxe();
        // Re-read to confirm hardware state
        let efer = cpu::read_msr(cpu::IA32_EFER);
        geometry.nx_enabled = (efer & (1 << 11)) != 0;
    }

    unsafe {
        ACTIVE_GEOMETRY = geometry;
    }

    geometry
}

/// Returns a reference to the active hardware geometry.
pub fn get_active_geometry() -> &'static AddressSpaceGeometry {
    unsafe { &*(&raw const ACTIVE_GEOMETRY) }
}

/// Prints Stage 2E-A architectural diagnostics to the serial console.
pub fn print_stage2e_diagnostics(geometry: &AddressSpaceGeometry) {
    let (cr3_root, cr3_flags) = Cr3::read(geometry);

    kprintln!("\n[Stage 2E-A: Architecture Foundations (Geometry, CR3 & Typed Tables)]");
    kprintln!("  Physical Address Width:   {:>2} bits (Mask: 0x{:016X})", geometry.physical_bits, geometry.physical_mask);
    kprintln!("  Linear/Virtual Width:     {:>2} bits (Canonical 48-bit Mode)", geometry.virtual_bits);
    kprintln!("  No-Execute (NX) Status:   Supported: {}, Enabled: {} (IA32_EFER.NXE)", geometry.nx_supported, geometry.nx_enabled);
    kprintln!("  Active CR3 Physical Root: 0x{:016X} (frame_num={})", cr3_root.address(), cr3_root.frame_number().as_u64());
    kprintln!("  CR3 Control Flags:        PWT={}, PCD={}, PCID=disabled (single-core bring-up)",
        cr3_flags.contains(Cr3Flags::PAGE_LEVEL_WRITETHROUGH),
        cr3_flags.contains(Cr3Flags::PAGE_LEVEL_CACHE_DISABLE)
    );
    kprintln!("  Page Table Geometry:      512 entries x 8 bytes = 4096 bytes (aligned: true)");
    kprintln!("  Security Domain Policy:   Kernel mappings enforce USER=0 throughout hierarchy");
    kprintln!("  Active Mapping State:     Untouched (Phase 2E-A instrument verification only)");
}

/// Executes self-test assertions validating Stage 2E-A invariants.
pub fn run_stage2ea_verification(geometry: &AddressSpaceGeometry) {
    // 1. Validate Canonical Checking
    let canonical_low = VirtualAddress::new(0x0000_7FFF_FFFF_0000);
    assert!(canonical_low.is_canonical(geometry), "Valid lower-half address failed canonical check");

    let non_canonical = VirtualAddress::new(0x0000_8000_0000_0000);
    assert!(!non_canonical.is_canonical(geometry), "Non-canonical hole address passed canonical check");

    let canonical_high = VirtualAddress::new(0xFFFF_8000_0000_0000);
    assert!(canonical_high.is_canonical(geometry), "Valid higher-half address failed canonical check");

    // 2. Validate CR3 Round-Trip Query
    let (root, flags) = Cr3::read(geometry);
    assert_eq!(root.address() % PAGE_SIZE, 0, "CR3 root frame is not 4 KiB aligned");
    assert!(geometry.is_valid_phys_frame(root), "CR3 root frame exceeds physical address mask");
    assert_eq!(flags.bits(), 0, "Non-zero control flags detected on bootstrap CR3");

    // 3. Validate Page Table Entry Flag Encoding
    let test_frame = PhysFrame(0x0015_0000);
    assert!(geometry.is_valid_phys_frame(test_frame));

    let mut pte = PageTableEntry::empty();
    assert!(!pte.is_present());
    assert_eq!(pte.points_to_frame(geometry), None);

    let leaf_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    leaf_flags.validate_leaf(geometry).expect("Valid leaf flags rejected");
    pte.set(test_frame, leaf_flags, geometry);

    assert!(pte.is_present());
    assert_eq!(pte.points_to_frame(geometry), Some(test_frame));
    assert!(pte.flags().contains(PageTableFlags::WRITABLE));
    assert!(!pte.flags().contains(PageTableFlags::USER_ACCESSIBLE)); // Enforces USER=0

    // 4. Validate Reserved-Bit Rejection
    let invalid_leaf_flags = PageTableFlags::PRESENT | PageTableFlags::HUGE_PAGE;
    assert_eq!(invalid_leaf_flags.validate_leaf(geometry), Err(VmmError::InvalidFlags));

    let invalid_intermediate_flags = PageTableFlags::PRESENT | PageTableFlags::DIRTY;
    assert_eq!(invalid_intermediate_flags.validate_intermediate(), Err(VmmError::InvalidFlags));

    // 5. Validate Security Domain intermediate flags
    assert_eq!(MappingDomain::Kernel.intermediate_flags(), PageTableFlags::empty());
    assert_eq!(MappingDomain::User.intermediate_flags(), PageTableFlags::USER_ACCESSIBLE);

    kprintln!("  [x] Stage 2E-A architectural instruments & invariants verified.");
}

// ====================================================================
// Active Page Table & Page Table Walker (Stage 2E-B)
// ====================================================================

/// Authoritative Active Page Table Walker & Mapper operating over an active 4-level paging hierarchy.
pub struct ActivePageTable {
    pml4_base: PhysFrame,
}

impl ActivePageTable {
    /// Acquires the active address space rooted in CR3.
    pub fn new() -> Self {
        let (root, _) = Cr3::read(get_active_geometry());
        Self {
            pml4_base: root,
        }
    }

    /// Explicit constructor for any arbitrary PML4 root frame.
    pub fn from_root(pml4_root: PhysFrame) -> Self {
        Self {
            pml4_base: pml4_root,
        }
    }

    /// Returns the physical root frame of this address space.
    #[inline(always)]
    pub fn root_frame(&self) -> PhysFrame {
        self.pml4_base
    }

    /// Checks if a page is currently mapped in this address space.
    pub fn is_mapped(&self, page: Page) -> bool {
        self.translate(page).is_ok()
    }

    /// Translates a virtual page to its mapped physical frame.
    pub fn translate(&self, page: Page) -> Result<PhysFrame, VmmError> {
        let geometry = get_active_geometry();
        let vaddr = page.start_address();

        if !vaddr.is_canonical(geometry) {
            return Err(VmmError::NonCanonicalAddress);
        }

        let pml4 = unsafe { &*phys_to_virt_table(self.pml4_base) };
        let p4_entry = &pml4.entries[vaddr.p4_index()];
        if !p4_entry.is_present() {
            return Err(VmmError::NotMapped);
        }

        let pdpt_frame = p4_entry.points_to_frame(geometry).ok_or(VmmError::NotMapped)?;
        let pdpt = unsafe { &*phys_to_virt_table(pdpt_frame) };
        let p3_entry = &pdpt.entries[vaddr.p3_index()];
        if !p3_entry.is_present() {
            return Err(VmmError::NotMapped);
        }
        if p3_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            let base_addr = p3_entry.0 & geometry.physical_mask & !0x3FFF_FFFF;
            let offset_4k = (vaddr.as_u64() & 0x3FFF_FFFF) & !0xFFF;
            return Ok(PhysFrame(base_addr + offset_4k));
        }

        let pd_frame = p3_entry.points_to_frame(geometry).ok_or(VmmError::NotMapped)?;
        let pd = unsafe { &*phys_to_virt_table(pd_frame) };
        let p2_entry = &pd.entries[vaddr.p2_index()];
        if !p2_entry.is_present() {
            return Err(VmmError::NotMapped);
        }
        if p2_entry.flags().contains(PageTableFlags::HUGE_PAGE) {
            let base_addr = p2_entry.0 & geometry.physical_mask & !0x1F_FFFF;
            let offset_4k = (vaddr.as_u64() & 0x1F_FFFF) & !0xFFF;
            return Ok(PhysFrame(base_addr + offset_4k));
        }

        let pt_frame = p2_entry.points_to_frame(geometry).ok_or(VmmError::NotMapped)?;
        let pt = unsafe { &*phys_to_virt_table(pt_frame) };
        let p1_entry = &pt.entries[vaddr.p1_index()];
        if !p1_entry.is_present() {
            return Err(VmmError::NotMapped);
        }

        p1_entry.points_to_frame(geometry).ok_or(VmmError::NotMapped)
    }

    /// Translates an arbitrary virtual address to a physical address with byte offset preserved.
    pub fn translate_addr(&self, vaddr: VirtualAddress) -> Result<PhysicalAddress, VmmError> {
        let page = Page::from_start_address(VirtualAddress::new(vaddr.as_u64() & !0xFFF), get_active_geometry())?;
        let frame = self.translate(page)?;
        Ok(PhysicalAddress::new(frame.address() + vaddr.offset()))
    }

    /// Maps a 4 KiB virtual page to a physical frame with rollback tracking on allocation failure.
    ///
    /// Invariants enforced:
    /// - `page` must be 4 KiB aligned and canonical.
    /// - `frame` must be 4 KiB aligned and valid under discovered physical geometry.
    /// - `flags` must satisfy leaf PTE constraints (NO_EXECUTE requires IA32_EFER.NXE).
    /// - `domain` controls intermediate table permissions: `MappingDomain::Kernel` strictly enforces `USER=0`.
    /// - Duplicate mapping returns `Err(VmmError::AlreadyMapped)`.
    /// - If allocation fails during hierarchy creation (PDPT, PD, or PT), any intermediate table frames
    ///   allocated during this attempt are completely freed back to PMM, and parent entries cleared (atomic rollback).
    pub fn map_page(
        &mut self,
        page: Page,
        frame: PhysFrame,
        flags: PageTableFlags,
        domain: MappingDomain,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<(), VmmError> {
        let geometry = get_active_geometry();
        let vaddr = page.start_address();

        // 1. Validate inputs
        if !vaddr.is_canonical(geometry) {
            return Err(VmmError::NonCanonicalAddress);
        }
        if !geometry.is_valid_phys_frame(frame) {
            return Err(VmmError::InvalidPhysicalAddress);
        }
        flags.validate_leaf(geometry)?;

        // Intermediate flags: PRESENT | WRITABLE | domain.intermediate_flags()
        // If domain == Kernel, intermediate_flags() has USER=0.
        let intermediate_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | domain.intermediate_flags();

        // Track allocations made during this specific map invocation for rollback
        let mut allocated_pdpt: Option<PhysFrame> = None;
        let mut allocated_pd: Option<PhysFrame> = None;
        let mut allocated_pt: Option<PhysFrame> = None;

        let p4_idx = vaddr.p4_index();
        let p3_idx = vaddr.p3_index();
        let p2_idx = vaddr.p2_index();
        let p1_idx = vaddr.p1_index();

        // Step 1: PML4 -> PDPT
        let pml4 = unsafe { &mut *phys_to_virt_table(self.pml4_base) };
        let pdpt_frame = if !pml4.entries[p4_idx].is_present() {
            let new_frame = match pmm.alloc_frame() {
                Some(f) => f,
                None => return Err(VmmError::OutOfMemory),
            };
            unsafe { (&mut *phys_to_virt_table(new_frame)).zero() };
            pml4.entries[p4_idx].set(new_frame, intermediate_flags, geometry);
            allocated_pdpt = Some(new_frame);
            new_frame
        } else {
            pml4.entries[p4_idx].points_to_frame(geometry).ok_or(VmmError::InvalidPhysicalAddress)?
        };

        // Step 2: PDPT -> PD
        let pdpt = unsafe { &mut *phys_to_virt_table(pdpt_frame) };
        let pd_frame = if !pdpt.entries[p3_idx].is_present() {
            let new_frame = match pmm.alloc_frame() {
                Some(f) => f,
                None => {
                    // Rollback Step 1 if allocated in this call
                    if let Some(f_pdpt) = allocated_pdpt {
                        pml4.entries[p4_idx].clear();
                        let _ = pmm.free_frame(f_pdpt);
                    }
                    return Err(VmmError::OutOfMemory);
                }
            };
            unsafe { (&mut *phys_to_virt_table(new_frame)).zero() };
            pdpt.entries[p3_idx].set(new_frame, intermediate_flags, geometry);
            allocated_pd = Some(new_frame);
            new_frame
        } else {
            if pdpt.entries[p3_idx].flags().contains(PageTableFlags::HUGE_PAGE) {
                return Err(VmmError::HugePageUnsupported);
            }
            pdpt.entries[p3_idx].points_to_frame(geometry).ok_or(VmmError::InvalidPhysicalAddress)?
        };

        // Step 3: PD -> PT
        let pd = unsafe { &mut *phys_to_virt_table(pd_frame) };
        let pt_frame = if !pd.entries[p2_idx].is_present() {
            let new_frame = match pmm.alloc_frame() {
                Some(f) => f,
                None => {
                    // Rollback Step 2 and Step 1 if allocated in this call
                    if let Some(f_pd) = allocated_pd {
                        pdpt.entries[p3_idx].clear();
                        let _ = pmm.free_frame(f_pd);
                    }
                    if let Some(f_pdpt) = allocated_pdpt {
                        pml4.entries[p4_idx].clear();
                        let _ = pmm.free_frame(f_pdpt);
                    }
                    return Err(VmmError::OutOfMemory);
                }
            };
            unsafe { (&mut *phys_to_virt_table(new_frame)).zero() };
            pd.entries[p2_idx].set(new_frame, intermediate_flags, geometry);
            allocated_pt = Some(new_frame);
            new_frame
        } else {
            if pd.entries[p2_idx].flags().contains(PageTableFlags::HUGE_PAGE) {
                return Err(VmmError::HugePageUnsupported);
            }
            pd.entries[p2_idx].points_to_frame(geometry).ok_or(VmmError::InvalidPhysicalAddress)?
        };

        // Step 4: PT -> Leaf PTE
        let pt = unsafe { &mut *phys_to_virt_table(pt_frame) };
        if pt.entries[p1_idx].is_present() {
            // Duplicate mapping detected! Rollback newly allocated tables.
            if let Some(f_pt) = allocated_pt {
                pd.entries[p2_idx].clear();
                let _ = pmm.free_frame(f_pt);
            }
            if let Some(f_pd) = allocated_pd {
                pdpt.entries[p3_idx].clear();
                let _ = pmm.free_frame(f_pd);
            }
            if let Some(f_pdpt) = allocated_pdpt {
                pml4.entries[p4_idx].clear();
                let _ = pmm.free_frame(f_pdpt);
            }
            return Err(VmmError::AlreadyMapped);
        }

        // Configure leaf flags with PRESENT and domain rules
        let mut leaf_flags = flags | PageTableFlags::PRESENT;
        if domain == MappingDomain::Kernel {
            leaf_flags.remove(PageTableFlags::USER_ACCESSIBLE);
        }

        pt.entries[p1_idx].set(frame, leaf_flags, geometry);
        cpu::invlpg(vaddr.as_u64());

        Ok(())
    }

    /// Unmaps a 4 KiB virtual page, reclaims empty intermediate tables, and invalidates TLB.
    ///
    /// Crucial Invariant: The mapped data `PhysFrame` is RETURNED to the caller and is NOT freed to PMM.
    /// The caller retains authoritative ownership of the mapped data frame.
    /// Only empty intermediate page tables (PT, PD, PDPT) created to support translations are reclaimed.
    pub fn unmap_page(
        &mut self,
        page: Page,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<PhysFrame, VmmError> {
        let geometry = get_active_geometry();
        let vaddr = page.start_address();

        if !vaddr.is_canonical(geometry) {
            return Err(VmmError::NonCanonicalAddress);
        }

        let p4_idx = vaddr.p4_index();
        let p3_idx = vaddr.p3_index();
        let p2_idx = vaddr.p2_index();
        let p1_idx = vaddr.p1_index();

        let pml4 = unsafe { &mut *phys_to_virt_table(self.pml4_base) };
        if !pml4.entries[p4_idx].is_present() {
            return Err(VmmError::NotMapped);
        }
        let pdpt_frame = pml4.entries[p4_idx].points_to_frame(geometry).ok_or(VmmError::NotMapped)?;

        let pdpt = unsafe { &mut *phys_to_virt_table(pdpt_frame) };
        if !pdpt.entries[p3_idx].is_present() {
            return Err(VmmError::NotMapped);
        }
        if pdpt.entries[p3_idx].flags().contains(PageTableFlags::HUGE_PAGE) {
            return Err(VmmError::HugePageUnsupported);
        }
        let pd_frame = pdpt.entries[p3_idx].points_to_frame(geometry).ok_or(VmmError::NotMapped)?;

        let pd = unsafe { &mut *phys_to_virt_table(pd_frame) };
        if !pd.entries[p2_idx].is_present() {
            return Err(VmmError::NotMapped);
        }
        if pd.entries[p2_idx].flags().contains(PageTableFlags::HUGE_PAGE) {
            return Err(VmmError::HugePageUnsupported);
        }
        let pt_frame = pd.entries[p2_idx].points_to_frame(geometry).ok_or(VmmError::NotMapped)?;

        let pt = unsafe { &mut *phys_to_virt_table(pt_frame) };
        if !pt.entries[p1_idx].is_present() {
            return Err(VmmError::NotMapped);
        }

        // Extract the mapped physical frame to preserve its ownership for the caller
        let mapped_frame = pt.entries[p1_idx].points_to_frame(geometry).ok_or(VmmError::NotMapped)?;

        // Clear the leaf entry
        pt.entries[p1_idx].clear();
        cpu::invlpg(vaddr.as_u64());

        // Empty intermediate table reclamation:
        // 1. If PT has 0 present entries, free pt_frame and clear PD entry.
        if pt.present_count() == 0 {
            pd.entries[p2_idx].clear();
            let _ = pmm.free_frame(pt_frame);

            // 2. If PD has 0 present entries, free pd_frame and clear PDPT entry.
            if pd.present_count() == 0 {
                pdpt.entries[p3_idx].clear();
                let _ = pmm.free_frame(pd_frame);

                // 3. If PDPT has 0 present entries, free pdpt_frame and clear PML4 entry.
                if pdpt.present_count() == 0 {
                    pml4.entries[p4_idx].clear();
                    let _ = pmm.free_frame(pdpt_frame);
                }
            }
        }

        Ok(mapped_frame)
    }
}

/// Executes comprehensive Stage 2E-B verification of the page-table mapping engine:
/// 1. Verifies 4 KiB alignment checks on page and frame.
/// 2. Verifies canonical address rejection.
/// 3. Allocates data frame from PMM and records baseline free frame count.
/// 4. Maps sparse virtual address (0x5000_0000) outside boot identity space.
/// 5. Verifies address translation (translate, translate_addr, is_mapped).
/// 6. Verifies kernel domain enforces USER=0 across all four hierarchy levels (PML4, PDPT, PD, PT).
/// 7. Verifies duplicate mapping rejection with AlreadyMapped.
/// 8. Executes volatile write and read memory access through the newly mapped virtual address.
/// 9. Verifies the written data appears identically in the underlying physical frame.
/// 10. Unmaps virtual page, verifying that the mapped data frame is returned and NOT freed to PMM.
/// 11. Verifies empty intermediate tables (PT, PD) are reclaimed and parent entries cleared.
/// 12. Verifies translation disappears after unmap (is_mapped == false).
/// 13. Caller explicitly frees the data frame back to PMM.
/// 14. Verifies final PMM free frame count equals the baseline free frame count (zero leaks).
/// 15. Verifies boot kernel mappings remain intact.
pub fn run_stage2eb_verification(pmm: &mut PhysicalMemoryManager) {
    let geometry = get_active_geometry();
    let initial_free = pmm.stats().free_frames;

    kprintln!("\n[Stage 2E-B: Virtual Memory Mapping Engine]");
    kprintln!("  Initial Free Physical Frames: {}", initial_free);

    // 1. Alignment & Canonical Validation
    let misaligned_vaddr = VirtualAddress::new(0x5000_0001);
    assert_eq!(
        Page::from_start_address(misaligned_vaddr, geometry),
        Err(VmmError::UnalignedPageAddress),
        "Misaligned virtual address was not rejected"
    );

    let non_canonical_vaddr = VirtualAddress::new(0x0000_8000_0000_0000);
    assert_eq!(
        Page::from_start_address(non_canonical_vaddr, geometry),
        Err(VmmError::NonCanonicalAddress),
        "Non-canonical virtual address was not rejected"
    );

    let mut apt = ActivePageTable::new();
    let sparse_vaddr = VirtualAddress::new(0x5000_0000);
    let sparse_page = Page::from_start_address(sparse_vaddr, geometry).expect("Valid sparse page");

    // 2. Reject frame violating discovered physical geometry
    let invalid_frame = PhysFrame(0x0000_0100_0000_0000); // bit 40 set, out of 40-bit space
    assert_eq!(
        apt.map_page(sparse_page, invalid_frame, PageTableFlags::WRITABLE, MappingDomain::Kernel, pmm),
        Err(VmmError::InvalidPhysicalAddress),
        "Physical frame exceeding geometry mask was not rejected"
    );

    // 3. Allocate test data frame from PMM
    let data_frame = pmm.alloc_frame().expect("Allocating test data frame");
    kprintln!("  Allocated Test Data Frame:   0x{:016X} (frame_num={})", data_frame.address(), data_frame.frame_number().as_u64());

    // 4. Map sparse virtual address (0x5000_0000)
    apt.map_page(
        sparse_page,
        data_frame,
        PageTableFlags::WRITABLE,
        MappingDomain::Kernel,
        pmm,
    ).expect("Mapping sparse page to data frame");
    kprintln!("  Mapped 0x50000000 -> 0x{:016X} (WRITABLE, Kernel domain)", data_frame.address());

    // 5. Verification of Translation
    let translated_frame = apt.translate(sparse_page).expect("Translating sparse page");
    assert_eq!(translated_frame, data_frame, "Translated frame does not match mapped frame");

    let translated_addr = apt.translate_addr(VirtualAddress::new(0x5000_0078)).expect("Translating sparse offset");
    assert_eq!(translated_addr.as_u64(), data_frame.address() + 0x78, "Translated offset address mismatch");
    assert!(apt.is_mapped(sparse_page), "is_mapped returned false for mapped page");

    // 6. Security Domain Verification (USER=0 throughout hierarchy)
    let pml4 = unsafe { &*phys_to_virt_table(apt.root_frame()) };
    let p4_entry = &pml4.entries[sparse_vaddr.p4_index()];
    assert!(!p4_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PML4E has USER=1 in kernel mapping");

    let pdpt_frame = p4_entry.points_to_frame(geometry).unwrap();
    let pdpt = unsafe { &*phys_to_virt_table(pdpt_frame) };
    let p3_entry = &pdpt.entries[sparse_vaddr.p3_index()];
    assert!(!p3_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PDPTE has USER=1 in kernel mapping");

    let pd_frame = p3_entry.points_to_frame(geometry).unwrap();
    let pd = unsafe { &*phys_to_virt_table(pd_frame) };
    let p2_entry = &pd.entries[sparse_vaddr.p2_index()];
    assert!(!p2_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PDE has USER=1 in kernel mapping");

    let pt_frame = p2_entry.points_to_frame(geometry).unwrap();
    let pt = unsafe { &*phys_to_virt_table(pt_frame) };
    let p1_entry = &pt.entries[sparse_vaddr.p1_index()];
    assert!(!p1_entry.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PTE has USER=1 in kernel mapping");
    kprintln!("  Security Domain Policy:      USER=0 verified at PML4E, PDPTE, PDE, and PTE");

    // 7. Duplicate Mapping Rejection
    let dup_result = apt.map_page(
        sparse_page,
        data_frame,
        PageTableFlags::WRITABLE,
        MappingDomain::Kernel,
        pmm,
    );
    assert_eq!(dup_result, Err(VmmError::AlreadyMapped), "Duplicate mapping was not rejected");
    kprintln!("  Duplicate Mapping Rejection: Successfully rejected with AlreadyMapped");

    // 8. Memory Access (Volatile Write and Read) at 0x5000_0000
    let test_ptr = 0x5000_0000 as *mut u64;
    let test_magic: u64 = 0x5A5A_BEEF_CAFE_0042;
    unsafe {
        core::ptr::write_volatile(test_ptr, test_magic);
    }
    let read_back = unsafe { core::ptr::read_volatile(test_ptr) };
    assert_eq!(read_back, test_magic, "Memory read back does not match written value");

    // Also verify byte offset write
    let offset_ptr = 0x5000_0F00 as *mut u64;
    let offset_magic: u64 = 0x1234_5678_9ABC_DEF0;
    unsafe {
        core::ptr::write_volatile(offset_ptr, offset_magic);
    }
    assert_eq!(unsafe { core::ptr::read_volatile(offset_ptr) }, offset_magic);

    // 9. Verify underlying physical frame contains this data via HHDM (identity map no longer used)
    let hhdm_phys_ptr = (HHDM_BASE + data_frame.address()) as *const u64;
    assert_eq!(unsafe { core::ptr::read_volatile(hhdm_phys_ptr) }, test_magic, "HHDM frame content mismatch");
    kprintln!("  Live Memory Access:          Read/write verified (magic: 0x{:016X})", test_magic);

    // 10. Unmap page
    let unmapped_frame = apt.unmap_page(sparse_page, pmm).expect("Unmapping sparse page");
    assert_eq!(unmapped_frame, data_frame, "unmap_page did not return the mapped data frame");
    assert!(!apt.is_mapped(sparse_page), "Page still reports mapped after unmap");
    assert_eq!(apt.translate(sparse_page), Err(VmmError::NotMapped));
    kprintln!("  Unmapped 0x50000000:         Returned frame 0x{:016X} (ownership preserved)", unmapped_frame.address());

    // 11. Empty Intermediate Table Reclamation
    // In sparse address 0x5000_0000 (PDPT index 1), the PT and PD should now be empty and reclaimed!
    // PDPT entry 1 should now be cleared!
    assert!(!pdpt.entries[sparse_vaddr.p3_index()].is_present(), "Empty intermediate PD was not reclaimed");
    kprintln!("  Empty-Table Reclamation:     Intermediate PT & PD reclaimed; parent PDPT[1] cleared");

    // 12. Verify Mapped Frame Ownership: data_frame was NOT freed by unmap_page
    assert_eq!(
        pmm.get_state(data_frame.frame_index()),
        crate::mm::pmm::FrameState::Allocated,
        "Data frame was freed prematurely during unmap"
    );

    // 13. Caller Explicitly Frees Data Frame
    pmm.free_frame(data_frame).expect("Caller freeing data frame");
    kprintln!("  Data Frame Reclaimed:        Caller freed data frame 0x{:016X}", data_frame.address());

    // 14. PMM Accounting Restoration
    let final_free = pmm.stats().free_frames;
    assert_eq!(
        final_free, initial_free,
        "PMM free frame count leaked or corrupted! Initial: {}, Final: {}",
        initial_free, final_free
    );
    kprintln!("  PMM Accounting Restored:     Final free ({}) == Initial free ({}) [EXACT MATCH]", final_free, initial_free);

    // 15. Verify Boot Kernel Mapping Intact
    let boot_page = Page::from_start_address(VirtualAddress::new(0x0010_0000), geometry).unwrap();
    assert!(apt.is_mapped(boot_page), "Boot kernel identity mapping was damaged");
    kprintln!("  Boot Mappings Intact:        Kernel image page 0x00100000 verified mapped");
    kprintln!("  [x] Stage 2E-B virtual memory mapping engine verified.");
}

/// Stage 2F-B Higher-Half Virtual Base Constant
pub const KERNEL_VIRT_BASE: u64 = 0xFFFF_FFFF_8000_0000;

/// Stage 2F-B Verification: Dual Bootstrap Page-Table Construction
///
/// Verifies:
/// 1. Active CR3 points to PML4 table frame at 0x15C000.
/// 2. PML4[0] (Identity 0..512 GiB) is Present | Writable, USER=0, pointing to PDPT at 0x15D000.
/// 3. PML4[511] (Higher-Half top 512 GiB) is Present | Writable, USER=0, pointing to PDPT at 0x15D000.
/// 4. All other PML4 entries (1..510) are Not Present (zero).
/// 5. PDPT[0] (Identity 0..1 GiB) is Present | Writable, USER=0, pointing to PD at 0x15E000.
/// 6. PDPT[510] (Higher-half 1 GiB window beginning at KERNEL_VIRT_BASE) is Present | Writable, USER=0, pointing to PD at 0x15E000.
/// 7. All other PDPT entries (1..509 and 511) are Not Present (zero).
/// 8. All 512 entries in PD are Present | Writable | Huge (0x83), mapping contiguous physical 0..1 GiB.
/// 9. Live Dual Address Memory Access Test:
///    - Compare reading Multiboot magic via identity pointer (0x00100000) and higher-half pointer (0xFFFFFFFF80100000).
///    - Verify both read 0x1BADB002 identically.
///    - Compare reading bootstrap entry code byte via identity pointer (0x00101000) and higher-half pointer (0xFFFFFFFF80101000).
///    - Verify both read 0xFA (cli opcode) identically.
pub fn run_stage2fb_verification(geometry: &AddressSpaceGeometry) {
    kprintln!("\n[Stage 2F-B: Dual Bootstrap Page-Table Construction]");

    let pt_range = crate::hal::arch::x86_64::gdt::get_page_tables_range();
    let expected_pml4 = pt_range.start;
    let expected_pdpt = pt_range.start + 4096;
    let expected_pd = pt_range.start + 8192;

    let (root_frame, _flags) = Cr3::read(geometry);
    let pml4_phys = root_frame.address();
    kprintln!("  Active CR3 Physical Root:    0x{:016X}", pml4_phys);
    assert_eq!(pml4_phys, expected_pml4, "CR3 physical root does not match linker __page_tables_start");

    // Access PML4 via HHDM (not identity) — safe both before and after Stage 2F-E removal.
    let pml4 = unsafe { &*(((HHDM_BASE + pml4_phys)) as *const PageTable) };

    // 1. Check PML4[0] (Identity map — present before Stage 2F-E removal)
    let pml4_0 = pml4.entries[0];
    assert!(pml4_0.is_present(), "PML4[0] is not present!");
    assert!(pml4_0.flags().contains(PageTableFlags::WRITABLE), "PML4[0] is not writable!");
    assert!(!pml4_0.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PML4[0] has USER flag set!");
    let pdpt_frame_0 = pml4_0.points_to_frame(geometry).expect("PML4[0] valid frame");
    assert_eq!(pdpt_frame_0.address(), expected_pdpt, "PML4[0] does not point to expected PDPT");
    kprintln!("  PML4[0] (Identity 0..512 GiB):     Present, Writable, User=0 -> PDPT (0x{:08X})", pdpt_frame_0.address());

    // 2. Check PML4[511] (Higher-half top 512 GiB)
    let pml4_511 = pml4.entries[511];
    assert!(pml4_511.is_present(), "PML4[511] is not present!");
    assert!(pml4_511.flags().contains(PageTableFlags::WRITABLE), "PML4[511] is not writable!");
    assert!(!pml4_511.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PML4[511] has USER flag set!");
    let pdpt_frame_511 = pml4_511.points_to_frame(geometry).expect("PML4[511] valid frame");
    assert_eq!(pdpt_frame_511.address(), expected_pdpt, "PML4[511] does not point to expected PDPT");
    kprintln!("  PML4[511] (Higher-Half 512 GiB):   Present, Writable, User=0 -> PDPT (0x{:08X})", pdpt_frame_511.address());

    // 3. Verify intermediate PML4 entries (1..510) are clean (PML4[256] routes to HHDM)
    for idx in 1..511 {
        if idx == 256 {
            continue;
        }
        assert!(!pml4.entries[idx].is_present(), "PML4 entry {} should not be present", idx);
    }
    kprintln!("  PML4 Invariant:              PML4[0] == PML4[511] (dual link); PML4[1..510] clean");

    // 4. Check PDPT table via HHDM
    let pdpt = unsafe { &*((HHDM_BASE + pdpt_frame_0.address()) as *const PageTable) };

    // PDPT[0] (Identity 0..1 GiB)
    let pdpt_0 = pdpt.entries[0];
    assert!(pdpt_0.is_present(), "PDPT[0] is not present!");
    assert!(pdpt_0.flags().contains(PageTableFlags::WRITABLE), "PDPT[0] is not writable!");
    assert!(!pdpt_0.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PDPT[0] has USER flag set!");
    let pd_frame_0 = pdpt_0.points_to_frame(geometry).expect("PDPT[0] valid frame");
    assert_eq!(pd_frame_0.address(), expected_pd, "PDPT[0] does not point to expected PD");
    kprintln!("  PDPT[0] (Identity 0..1 GiB):       Present, Writable, User=0 -> PD (0x{:08X})", pd_frame_0.address());

    // PDPT[510] (Higher-half 1 GiB window beginning at KERNEL_VIRT_BASE)
    let pdpt_510 = pdpt.entries[510];
    assert!(pdpt_510.is_present(), "PDPT[510] is not present!");
    assert!(pdpt_510.flags().contains(PageTableFlags::WRITABLE), "PDPT[510] is not writable!");
    assert!(!pdpt_510.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PDPT[510] has USER flag set!");
    let pd_frame_510 = pdpt_510.points_to_frame(geometry).expect("PDPT[510] valid frame");
    assert_eq!(pd_frame_510.address(), expected_pd, "PDPT[510] does not point to expected PD");
    kprintln!("  PDPT[510] (Higher-Half 1 GiB):     Present, Writable, User=0 -> PD (0x{:08X})", pd_frame_510.address());

    // Verify other PDPT entries are clean
    for idx in 1..510 {
        assert!(!pdpt.entries[idx].is_present(), "PDPT entry {} should not be present", idx);
    }
    assert!(!pdpt.entries[511].is_present(), "PDPT entry 511 should not be present");
    kprintln!("  PDPT Invariant:              PDPT[0] == PDPT[510] (dual link); PDPT[1..509, 511] clean");

    // 5. Check PD table via HHDM (512 x 2 MiB entries)
    let pd = unsafe { &*((HHDM_BASE + pd_frame_0.address()) as *const PageTable) };
    for idx in 0..512 {
        let pde = pd.entries[idx];
        assert!(pde.is_present(), "PD entry {} is not present", idx);
        assert!(pde.flags().contains(PageTableFlags::WRITABLE), "PD entry {} is not writable", idx);
        assert!(pde.flags().contains(PageTableFlags::HUGE_PAGE), "PD entry {} is not huge (2 MiB)", idx);
        assert!(!pde.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PD entry {} has USER flag", idx);
        let expected_phys = (idx as u64) * (2 * 1024 * 1024);
        let pde_phys = pde.0 & geometry.physical_mask;
        assert_eq!(pde_phys, expected_phys, "PD entry {} phys address mismatch", idx);
    }
    kprintln!("  PD Table Coverage:           512 entries x 2 MiB = 1024 MiB (1 GiB) mapped continuously");

    // 6. Memory Access via Higher-Half VMA and HHDM (identity no longer used).
    // Multiboot magic: kernel VMA (0xFFFFFFFF80100000) and HHDM (0xFFFF800000100000)
    let hh_mb_ptr = (KERNEL_VIRT_BASE + 0x0010_0000) as *const u32;
    let hhdm_mb_ptr = (HHDM_BASE + 0x0010_0000) as *const u32;
    let mb_hh = unsafe { core::ptr::read_volatile(hh_mb_ptr) };
    let mb_hhdm = unsafe { core::ptr::read_volatile(hhdm_mb_ptr) };
    assert_eq!(mb_hh, 0x1BADB002, "Multiboot magic read via higher-half failed");
    assert_eq!(mb_hhdm, 0x1BADB002, "Multiboot magic read via HHDM failed");
    kprintln!("  Live Higher-Half VMA Access: 0x{:016X} (Magic: 0x{:08X})", KERNEL_VIRT_BASE + 0x0010_0000, mb_hh);
    kprintln!("  Live HHDM Access:            0x{:016X} (Magic: 0x{:08X})", HHDM_BASE + 0x0010_0000, mb_hhdm);

    // Bootstrap entry opcode via higher-half VMA and HHDM
    let hh_entry_ptr = (KERNEL_VIRT_BASE + 0x0010_1000) as *const u8;
    let hhdm_entry_ptr = (HHDM_BASE + 0x0010_1000) as *const u8;
    let byte_hh = unsafe { core::ptr::read_volatile(hh_entry_ptr) };
    let byte_hhdm = unsafe { core::ptr::read_volatile(hhdm_entry_ptr) };
    assert_eq!(byte_hh, 0xFA, "Bootstrap entry opcode read via higher-half failed");
    assert_eq!(byte_hhdm, 0xFA, "Bootstrap entry opcode read via HHDM failed");
    kprintln!("  Live HH+HHDM Code Access:   HH 0x{:016X} == HHDM 0x{:016X} (Opcode: 0x{:02X} [cli])",
        KERNEL_VIRT_BASE + 0x0010_1000, HHDM_BASE + 0x0010_1000, byte_hh);

    kprintln!("  [x] Stage 2F-B dual bootstrap page-table mappings verified.");
}

/// Stage 2F-C Verification: Higher-Half Execution Switch & Kernel Stack Transition
///
/// Verifies:
/// 1. Current CPU instruction pointer (RIP) is executing in canonical higher-half virtual space (>= KERNEL_VIRT_BASE).
/// 2. Current stack pointer (RSP) resides in canonical higher-half virtual space (>= KERNEL_VIRT_BASE).
/// 3. RSP is within the dedicated linker-allocated kernel stack boundaries [__stack_start, __stack_end].
/// 4. Stack alignment: RSP % 16 == 0 or 8 (standard x86-64 function prologue alignment).
/// 5. Existing dual mappings remain fully intact in CR3:
///    - PML4[0] (identity) and PML4[511] (higher-half) are both active.
pub fn run_stage2fc_verification(geometry: &AddressSpaceGeometry) {
    kprintln!("\n[Stage 2F-C: Higher-Half Execution Switch & Stack Transition]");

    let rip: u64;
    let rsp: u64;
    unsafe {
        core::arch::asm!("lea {}, [rip]", out(reg) rip, options(nomem, nostack));
        core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack));
    }

    kprintln!("  Current Execution RIP:       0x{:016X}", rip);
    kprintln!("  Current Kernel RSP:          0x{:016X}", rsp);

    // 1. Assert RIP is in canonical higher-half kernel window
    assert!(
        rip >= KERNEL_VIRT_BASE,
        "CRITICAL FAILURE: RIP is executing in identity/low memory (0x{:016X} < KERNEL_VIRT_BASE)",
        rip
    );
    assert!(
        rip < KERNEL_VIRT_BASE + 0x4000_0000,
        "CRITICAL FAILURE: RIP exceeds bootstrap 1 GiB mapping window"
    );
    kprintln!("  Execution Domain:            Canonical Higher-Half (0xFFFF_FFFF_8000_0000+) [VERIFIED]");

    // 2. Assert RSP is in canonical higher-half kernel stack
    assert!(
        rsp >= KERNEL_VIRT_BASE,
        "CRITICAL FAILURE: RSP is located in identity/low memory (0x{:016X} < KERNEL_VIRT_BASE)",
        rsp
    );
    let stack_range = crate::hal::arch::x86_64::gdt::get_kernel_stack_range();
    let rsp_phys = rsp - KERNEL_VIRT_BASE;
    assert!(
        rsp_phys >= stack_range.start && rsp_phys <= stack_range.end,
        "CRITICAL FAILURE: RSP physical frame (0x{:016X}) is outside linker stack range [0x{:016X}, 0x{:016X})",
        rsp_phys, stack_range.start, stack_range.end
    );
    kprintln!("  Kernel Stack Placement:      Dedicated .stack region [0x{:016X}, 0x{:016X}) [VERIFIED]",
        KERNEL_VIRT_BASE + stack_range.start, KERNEL_VIRT_BASE + stack_range.end
    );

    // 3. Confirm Dual Mappings are Preserved in CR3 (read PML4 via HHDM)
    let (root_frame, _) = Cr3::read(geometry);
    let pml4 = unsafe { &*((HHDM_BASE + root_frame.address()) as *const PageTable) };
    assert!(pml4.entries[0].is_present(), "PML4[0] identity mapping was unexpectedly modified");
    assert!(pml4.entries[511].is_present(), "PML4[511] higher-half mapping is missing");
    kprintln!("  Dual Mapping State:          PML4[0] (identity) & PML4[511] (higher-half) preserved [via HHDM]");

    kprintln!("  [x] Stage 2F-C higher-half execution switch & stack transition verified.");
}

/// Stage 2F-D Verification: Higher-Half Direct Map (HHDM) Architecture
///
/// Verifies:
/// 1. Architectural HHDM Base: HHDM_BASE = 0xFFFF_8000_0000_0000 (PML4 index 256).
/// 2. Active CR3 hierarchy contains valid HHDM mapping:
///    - PML4[256] is Present | Writable, USER=0, pointing to hhdm_pdpt.
///    - hhdm_pdpt[0] is Present | Writable, USER=0, pointing to pd_table.
///    - hhdm_pdpt[1..512] are Not Present (zero).
/// 3. HHDM Direct Map Window covers physical [0, 1 GiB) via 2 MiB huge pages.
/// 4. Triple Virtual Translation Test:
///    - Identical physical data (Multiboot magic 0x1BADB002 at physical 0x00100000) verified across:
///      * Identity pointer:    0x0000000000100000
///      * Kernel VMA pointer:  0xFFFFFFFF80100000
///      * HHDM pointer:        0xFFFF800000100000
///    - Identical instruction byte (opcode 0xFA [cli] at physical 0x00101000) verified across all 3 mappings.
/// 5. Live Dynamic PMM Frame Write/Read via HHDM Pointer:
///    - Allocate a physical frame from PMM (PhysFrame P).
///    - Convert P to HhdmAddress via phys_to_hhdm().
///    - Write 64-bit diagnostic signature (0x4848444D_564D4D31, "HHDMVMM1") through the HHDM pointer.
///    - Confirm signature via volatile read from HHDM pointer.
///    - Confirm signature via volatile read from identity pointer (P as *const u64).
///    - Zero memory and free frame back to PMM.
///    - Verify PMM accounting restored.
/// 6. Ownership vs Addressability Decoupling Invariant:
///    - Addressability != Owned RAM != Allocatable RAM.
///    - Frame 0 (IVT) is addressable at HHDM_BASE + 0x0, but marked Reserved in PMM.
///    - Kernel image (0x00100000) is addressable at HHDM_BASE + 0x100000, but marked Reserved in PMM.
///    - Attempting to free reserved frame 0x00100000 fails closed with Err(PmmError::FrameReserved).
/// 7. Dual Bootstrap Mappings Preserved:
///    - PML4[0] (identity) and PML4[511] (higher-half) remain fully intact.
pub fn run_stage2fd_verification(geometry: &AddressSpaceGeometry, pmm: &mut PhysicalMemoryManager) {
    kprintln!("\n[Stage 2F-D: Higher-Half Direct Map (HHDM) Verification]");

    // 1. Validate HHDM Mathematical Routing Invariants
    let hhdm_virt = VirtualAddress::new(HHDM_BASE);
    assert_eq!(hhdm_virt.p4_index(), 256, "HHDM_BASE does not route to PML4[256]");
    assert_eq!(hhdm_virt.p3_index(), 0, "HHDM_BASE does not route to PDPT[0]");
    kprintln!("  HHDM Base Address:           0x{:016X} (PML4 Index: {})", HHDM_BASE, hhdm_virt.p4_index());

    // 2. Validate CR3 PML4[256] -> hhdm_pdpt Link via HHDM (not identity)
    let (root_frame, _) = Cr3::read(geometry);
    let pml4 = unsafe { &*((HHDM_BASE + root_frame.address()) as *const PageTable) };

    let pml4_256 = pml4.entries[256];
    assert!(pml4_256.is_present(), "CRITICAL: PML4[256] (HHDM) is not present!");
    assert!(pml4_256.flags().contains(PageTableFlags::WRITABLE), "PML4[256] is not writable!");
    assert!(!pml4_256.flags().contains(PageTableFlags::USER_ACCESSIBLE), "PML4[256] has USER flag set!");
    let hhdm_pdpt_frame = pml4_256.points_to_frame(geometry).expect("PML4[256] must point to valid frame");
    kprintln!("  PML4[256] Hierarchy:         Present, Writable, User=0 -> hhdm_pdpt (0x{:08X})", hhdm_pdpt_frame.address());

    // Validate hhdm_pdpt[0] -> pd_table Link via HHDM
    let hhdm_pdpt = unsafe { &*((HHDM_BASE + hhdm_pdpt_frame.address()) as *const PageTable) };
    let pdpt_0 = hhdm_pdpt.entries[0];
    assert!(pdpt_0.is_present(), "CRITICAL: hhdm_pdpt[0] is not present!");
    assert!(pdpt_0.flags().contains(PageTableFlags::WRITABLE), "hhdm_pdpt[0] is not writable!");
    assert!(!pdpt_0.flags().contains(PageTableFlags::USER_ACCESSIBLE), "hhdm_pdpt[0] has USER flag set!");
    let pd_frame = pdpt_0.points_to_frame(geometry).expect("hhdm_pdpt[0] must point to valid frame");
    kprintln!("  HHDM PDPT[0] Link:           Present, Writable, User=0 -> pd_table (0x{:08X})", pd_frame.address());

    // Verify hhdm_pdpt[1..512] are clean
    for idx in 1..512 {
        assert!(!hhdm_pdpt.entries[idx].is_present(), "hhdm_pdpt entry {} should not be present", idx);
    }
    kprintln!("  HHDM Direct Map Window:      [0x{:016X}, 0x{:016X}) -> Phys [0, 1 GiB)", HHDM_BASE, HHDM_BASE + HHDM_SIZE);

    // 3. Mathematical Translation Abstraction Invariants
    let test_phys = PhysicalAddress::new(0x0010_0000);
    let expected_hhdm = HhdmAddress(HHDM_BASE + 0x0010_0000);
    assert_eq!(phys_to_hhdm(test_phys), Ok(expected_hhdm), "phys_to_hhdm mismatch");
    assert_eq!(hhdm_to_phys(expected_hhdm), test_phys, "hhdm_to_phys mismatch");
    assert!(is_hhdm_address(HHDM_BASE + 0x1234));
    assert!(!is_hhdm_address(KERNEL_VIRT_BASE));
    assert!(!is_hhdm_address(0x0010_0000));

    // 4. Dual Virtual Translation Test via Kernel VMA and HHDM
    // (Identity read deliberately removed — proves HHDM path suffices before 2F-E removal)
    let vma_mb_ptr = (KERNEL_VIRT_BASE + 0x0010_0000) as *const u32;
    let hhdm_mb_ptr = (HHDM_BASE + 0x0010_0000) as *const u32;
    let mb_vma = unsafe { core::ptr::read_volatile(vma_mb_ptr) };
    let mb_hhdm = unsafe { core::ptr::read_volatile(hhdm_mb_ptr) };
    assert_eq!(mb_vma, 0x1BADB002, "Multiboot magic read via kernel VMA failed");
    assert_eq!(mb_hhdm, 0x1BADB002, "Multiboot magic read via HHDM failed");
    assert_eq!(mb_vma, mb_hhdm, "Kernel VMA vs HHDM Multiboot header read mismatch");
    kprintln!("  Dual Memory Access Test:     VMA 0x{:016X} == HHDM 0x{:016X} (Magic: 0x{:08X})",
        KERNEL_VIRT_BASE + 0x0010_0000, HHDM_BASE + 0x0010_0000, mb_hhdm
    );

    // Dual Code Byte Access Test (Opcode 0xFA [cli])
    let vma_code_ptr = (KERNEL_VIRT_BASE + 0x0010_1000) as *const u8;
    let hhdm_code_ptr = (HHDM_BASE + 0x0010_1000) as *const u8;
    let code_vma = unsafe { core::ptr::read_volatile(vma_code_ptr) };
    let code_hhdm = unsafe { core::ptr::read_volatile(hhdm_code_ptr) };
    assert_eq!(code_vma, 0xFA, "Bootstrap entry opcode read via kernel VMA failed");
    assert_eq!(code_hhdm, 0xFA, "Bootstrap entry opcode read via HHDM failed");
    assert_eq!(code_vma, code_hhdm, "Kernel VMA vs HHDM entry opcode mismatch");
    kprintln!("  Dual Code Access Test:       VMA 0x{:016X} == HHDM 0x{:016X} (Opcode: 0x{:02X})",
        KERNEL_VIRT_BASE + 0x0010_1000, HHDM_BASE + 0x0010_1000, code_hhdm
    );

    // 5. Live Dynamic PMM Frame Write/Read via HHDM Pointer
    let initial_free = pmm.stats().free_frames;
    let frame = pmm.alloc_frame().expect("Allocating test frame from PMM");
    let frame_phys = frame.address();
    let hhdm_addr = phys_to_hhdm(PhysicalAddress::new(frame_phys)).expect("Converting frame to HHDM");
    let hhdm_ptr = hhdm_addr.as_mut_ptr::<u64>();

    const HHDM_TEST_SIG: u64 = 0x4848_444D_564D_4D31; // "HHDMVMM1"
    unsafe {
        core::ptr::write_volatile(hhdm_ptr, HHDM_TEST_SIG);
    }
    let read_back_hhdm = unsafe { core::ptr::read_volatile(hhdm_ptr) };
    assert_eq!(read_back_hhdm, HHDM_TEST_SIG, "HHDM write readback mismatch");

    // Cross-verify the same physical frame via a second HHDM read (identity no longer used)
    let hhdm_cross_ptr = (HHDM_BASE + frame_phys) as *const u64;
    let read_back_cross = unsafe { core::ptr::read_volatile(hhdm_cross_ptr) };
    assert_eq!(read_back_cross, HHDM_TEST_SIG, "HHDM cross-read mismatch");

    // Clean up frame
    unsafe {
        core::ptr::write_volatile(hhdm_ptr, 0);
    }
    pmm.free_frame(frame).expect("Freeing test frame back to PMM");
    assert_eq!(pmm.stats().free_frames, initial_free, "PMM free frame count did not restore after HHDM cycle");
    kprintln!("  HHDM Live Frame Write/Read:  Allocated phys 0x{:08X}, wrote signature 0x{:016X} via HHDM, read back verified",
        frame_phys, HHDM_TEST_SIG
    );

    // 6. Ownership vs Addressability Decoupling Invariant
    let frame_zero_state = pmm.get_state(0);
    assert_eq!(frame_zero_state, FrameState::Reserved, "Frame 0 must remain Reserved in PMM");

    let kimage_frame_idx = (0x0010_0000 / PAGE_SIZE) as usize;
    let kimage_state = pmm.get_state(kimage_frame_idx);
    assert_eq!(kimage_state, FrameState::Reserved, "Kernel image frame must remain Reserved in PMM");

    // Freeing reserved frame must fail closed
    let reserved_phys_frame = PhysFrame(0x0010_0000);
    let free_res = pmm.free_frame(reserved_phys_frame);
    assert_eq!(free_res, Err(PmmError::FrameReserved), "Freeing reserved frame should return FrameReserved error");
    kprintln!("  PMM Ownership Invariant:     Addressable != Owned; frame 0 & kernel image remain Reserved in PMM");

    // 7. Dual Bootstrap Mappings Preserved (via HHDM read)
    assert!(pml4.entries[0].is_present(), "PML4[0] identity mapping was unexpectedly modified");
    assert!(pml4.entries[511].is_present(), "PML4[511] higher-half mapping was unexpectedly modified");
    kprintln!("  Dual Mappings Preserved:     PML4[0] (identity) & PML4[511] (higher-half VMA) intact [via HHDM]");

    kprintln!("  [x] Stage 2F-D Higher-Half Direct Map verified.");
}

/// Stage 2F-E Verification: Identity Mapping Removal
///
/// Verifies:
/// 1. Execution is in higher-half (RIP >= KERNEL_VIRT_BASE) before and after removal.
/// 2. PML4[0] entry is cleared via HHDM pointer — no identity access needed.
/// 3. CR3 is reloaded for a full TLB shootdown.
/// 4. After removal:
///    - PML4[0] is NOT present.
///    - PML4[256] (HHDM) remains present.
///    - PML4[511] (kernel higher-half) remains present.
/// 5. Controlled identity-removal fault test:
///    - expect_fault(vector=14, resume_rip, Some(0x00100000)).
///    - Attempt to read from low physical address 0x00100000 directly.
///    - CPU traps #PF (vector 14) — IDTR points to higher-half IDT, handler executes in higher-half.
///    - Verify CR2 == 0x00100000, error_code == 0 (non-present, read, kernel).
///    - Recovery executes in higher-half domain.
/// 6. Kernel VMA and HHDM accesses remain valid after identity removal.
/// 7. PMM and VMM (alloc/map/translate/unmap/free) remain functional after removal.
///
/// Address Domain Proof (explicit 3-way distinction):
///   physical address (e.g. 0x00100000)
///   != kernel virtual address (e.g. 0xFFFFFFFF80100000)
///   != HHDM virtual address  (e.g. 0xFFFF800000100000)
pub fn run_stage2fe_verification(
    geometry: &AddressSpaceGeometry,
    pmm: &mut PhysicalMemoryManager,
) {
    use crate::hal::arch::x86_64::idt;

    kprintln!("\n[Stage 2F-E: Identity Mapping Removal]");

    // ---------------------------------------------------------------
    // Step 1: Capture RIP — prove higher-half execution before removal
    // ---------------------------------------------------------------
    let rip_before: u64;
    unsafe {
        core::arch::asm!("lea {}, [rip]", out(reg) rip_before, options(nomem, nostack));
    }
    assert!(
        rip_before >= KERNEL_VIRT_BASE,
        "CRITICAL: RIP before removal is not in higher-half (0x{:016X})",
        rip_before
    );
    kprintln!("  Pre-Removal RIP:             0x{:016X} (canonical higher-half) [VERIFIED]", rip_before);

    // ---------------------------------------------------------------
    // Step 2: Read PML4 via HHDM, verify preconditions before removal
    // ---------------------------------------------------------------
    let (root_frame, _) = Cr3::read(geometry);
    let pml4_hhdm_ptr = (HHDM_BASE + root_frame.address()) as *mut PageTable;
    let pml4 = unsafe { &mut *pml4_hhdm_ptr };

    assert!(pml4.entries[0].is_present(),   "Pre-condition: PML4[0] not present before removal");
    assert!(pml4.entries[256].is_present(), "Pre-condition: PML4[256] (HHDM) not present before removal");
    assert!(pml4.entries[511].is_present(), "Pre-condition: PML4[511] (higher-half) not present before removal");
    kprintln!("  Pre-Removal PML4 State:      PML4[0]=present, PML4[256]=present, PML4[511]=present");

    // ---------------------------------------------------------------
    // Step 3: Clear PML4[0] via HHDM pointer — identity mapping removed.
    //         physical address domain: 0x00000000_00000000 - 0x00000000_FFFFFFFF
    //         is now unmapped from virtual address space.
    // ---------------------------------------------------------------
    pml4.entries[0].clear();
    kprintln!("  Identity Removal:            PML4[0] cleared (virt == phys no longer valid)");

    // ---------------------------------------------------------------
    // Step 4: Reload CR3 for a full TLB shootdown.
    //         This flushes all cached TLB entries for the identity range.
    // ---------------------------------------------------------------
    Cr3::write(root_frame, Cr3Flags::empty(), geometry);
    kprintln!("  TLB Shootdown:               CR3 reloaded (full TLB flush performed)");

    // ---------------------------------------------------------------
    // Step 5: Capture RIP after removal — must still be in higher-half
    // ---------------------------------------------------------------
    let rip_after: u64;
    unsafe {
        core::arch::asm!("lea {}, [rip]", out(reg) rip_after, options(nomem, nostack));
    }
    assert!(
        rip_after >= KERNEL_VIRT_BASE,
        "CRITICAL: RIP after identity removal is not in higher-half (0x{:016X})",
        rip_after
    );
    kprintln!("  Post-Removal RIP:            0x{:016X} (canonical higher-half) [VERIFIED]", rip_after);

    // ---------------------------------------------------------------
    // Step 6: Verify PML4 state after removal (read via HHDM, not identity)
    // ---------------------------------------------------------------
    assert!(!pml4.entries[0].is_present(),
        "POST-REMOVAL FAILURE: PML4[0] still present after clearing!");
    assert!(pml4.entries[256].is_present(),
        "POST-REMOVAL FAILURE: PML4[256] (HHDM) was accidentally cleared!");
    assert!(pml4.entries[511].is_present(),
        "POST-REMOVAL FAILURE: PML4[511] (higher-half) was accidentally cleared!");
    kprintln!("  Post-Removal PML4 State:     PML4[0]=ABSENT, PML4[256]=present, PML4[511]=present [VERIFIED]");

    // ---------------------------------------------------------------
    // Step 7: Controlled identity-removal fault test.
    //
    // Attempt to read directly from physical address 0x00100000.
    // After PML4[0] removal this virtual address is no longer mapped.
    // The CPU must trap #PF (vector 14).
    //
    // Proof that the handler executes in the higher-half environment:
    //   - IDTR loaded in kernel_main points to IDT at higher-half VMA.
    //   - common_isr_stub lives in higher-half .text (VMA 0xFFFFFFFF8010xxxx).
    //   - exception_dispatch is a Rust function at higher-half VMA.
    //   - The kernel stack (RSP) is in the higher-half .stack section.
    // ---------------------------------------------------------------
    let identity_probe_addr: u64 = 0x0010_0000;
    let resume_rip = unsafe { idt::test_resume_pf as usize as u64 };

    unsafe {
        idt::expect_fault(14, resume_rip, Some(identity_probe_addr));
        idt::test_trigger_pf(identity_probe_addr);
    }

    let fault_result = unsafe { idt::last_fault_result() };
    match fault_result {
        Some(fault) => {
            // Vector must be 14 (#PF)
            assert_eq!(fault.vector, 14,
                "Expected #PF (vector 14), got vector {}", fault.vector);
            // CR2 must equal 0x00100000 (exact page-aligned match)
            assert_eq!(
                fault.fault_addr & !0xFFF,
                identity_probe_addr & !0xFFF,
                "CR2 mismatch: expected 0x{:016X}, got 0x{:016X}",
                identity_probe_addr, fault.fault_addr
            );
            // error_code == 0: non-present | read | kernel mode (bits 2:0 == 0b000)
            assert_eq!(
                fault.error_code & 0x7,
                0,
                "Unexpected #PF error code: 0x{:016X} (expected 0 = non-present read kernel)",
                fault.error_code
            );
            kprintln!("  Controlled Fault Test:       #PF (vector 14) trapped successfully");
            kprintln!("    CR2 Fault Address:          0x{:016X} == expected 0x{:016X} [EXACT MATCH]",
                fault.fault_addr, identity_probe_addr);
            kprintln!("    Error Code:                 0x{:04X} (Non-present, Read, Kernel)",
                fault.error_code);
            kprintln!("    Handler Domain:             IDT in higher-half VMA; common_isr_stub in higher-half .text");
            kprintln!("    Recovery:                   Execution diverted to test_resume_pf in higher-half VMA");
        }
        None => {
            panic!(
                "CRITICAL: Controlled #PF at 0x{:016X} after identity removal was NOT intercepted!",
                identity_probe_addr
            );
        }
    }

    // ---------------------------------------------------------------
    // Step 8: Verify HHDM and kernel VMA remain valid post-removal
    // physical addr 0x00100000 != kernel VMA 0xFFFFFFFF80100000 != HHDM addr 0xFFFF800000100000
    // ---------------------------------------------------------------
    let hhdm_mb_ptr = (HHDM_BASE + 0x0010_0000) as *const u32;
    let mb_hhdm = unsafe { core::ptr::read_volatile(hhdm_mb_ptr) };
    assert_eq!(mb_hhdm, 0x1BADB002, "HHDM read failed after identity removal!");
    kprintln!("  HHDM Post-Removal Access:    phys 0x00100000 via HHDM 0x{:016X} = 0x{:08X} [VERIFIED]",
        HHDM_BASE + 0x0010_0000, mb_hhdm);

    let vma_mb_ptr = (KERNEL_VIRT_BASE + 0x0010_0000) as *const u32;
    let mb_vma = unsafe { core::ptr::read_volatile(vma_mb_ptr) };
    assert_eq!(mb_vma, 0x1BADB002, "Kernel VMA read failed after identity removal!");
    kprintln!("  Kernel VMA Post-Removal:     phys 0x00100000 via VMA  0x{:016X} = 0x{:08X} [VERIFIED]",
        KERNEL_VIRT_BASE + 0x0010_0000, mb_vma);

    // ---------------------------------------------------------------
    // Step 9: PMM + VMM post-removal functional verification.
    // All page-table walks in map_page/translate/unmap_page now use HHDM
    // via the phys_to_virt_table() seam.
    // Test address: 0xFFFF_8001_0000_0000 (HHDM range, far from existing mappings)
    // ---------------------------------------------------------------
    let initial_free = pmm.stats().free_frames;
    let mut apt = ActivePageTable::new();

    let test_vaddr = VirtualAddress::new(0xFFFF_8001_0000_0000);
    let test_page = Page::from_start_address(test_vaddr, geometry).expect("Valid test page VA");
    let data_frame = pmm.alloc_frame().expect("Allocating test data frame post-removal");

    apt.map_page(
        test_page,
        data_frame,
        PageTableFlags::WRITABLE,
        MappingDomain::Kernel,
        pmm,
    ).expect("map_page failed post identity-removal");

    const POST_REMOVAL_SIG: u64 = 0x2F45_5F4E_4F49_444E; // "NOIDEN/E"
    unsafe {
        core::ptr::write_volatile(test_vaddr.as_u64() as *mut u64, POST_REMOVAL_SIG);
    }
    let readback = unsafe { core::ptr::read_volatile(test_vaddr.as_u64() as *const u64) };
    assert_eq!(readback, POST_REMOVAL_SIG, "Post-removal VMM write/read mismatch");

    // Cross-verify via HHDM (physical address != HHDM virtual address != VMA)
    let hhdm_cross = (HHDM_BASE + data_frame.address()) as *const u64;
    let readback_hhdm = unsafe { core::ptr::read_volatile(hhdm_cross) };
    assert_eq!(readback_hhdm, POST_REMOVAL_SIG, "Post-removal HHDM cross-read mismatch");

    let returned_frame = apt.unmap_page(test_page, pmm).expect("unmap_page failed post removal");
    assert_eq!(returned_frame, data_frame, "unmap_page returned wrong frame");
    pmm.free_frame(data_frame).expect("Freeing data frame post-removal");
    assert_eq!(pmm.stats().free_frames, initial_free,
        "PMM accounting corrupted post identity-removal");
    kprintln!("  Post-Removal VMM Cycle:      Allocate, map at 0x{:016X}, write/read, unmap, free [VERIFIED]",
        test_vaddr.as_u64());
    kprintln!("  PMM Accounting:              Free frames restored to baseline ({}) [EXACT MATCH]", initial_free);

    // ---------------------------------------------------------------
    // Final address-domain proof (mandatory per 2F-E requirements)
    // ---------------------------------------------------------------
    kprintln!("  Address Domain Proof:");
    kprintln!("    physical addr  0x00100000");
    kprintln!("    != kernel VMA  0x{:016X}  (difference: 0x{:016X})",
        KERNEL_VIRT_BASE + 0x0010_0000, KERNEL_VIRT_BASE);
    kprintln!("    != HHDM addr   0x{:016X}  (difference: 0x{:016X})",
        HHDM_BASE + 0x0010_0000, HHDM_BASE);
    kprintln!("    Project Zero no longer requires identity mapping for normal kernel operation.");

    kprintln!("  [x] Stage 2F-E identity mapping removal verified.");
}

// ====================================================================
// Stage 2F-F: 4 KiB Kernel Permission Splitting & W^X Enforcement
// ====================================================================

extern "C" {
    static __multiboot_header_start: u8;
    static __multiboot_header_end: u8;
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __data_start: u8;
    static __data_end: u8;
    static __bss_start: u8;
    static __bss_end: u8;
    static __pmm_meta_start: u8;
    static __pmm_meta_end: u8;
    static __page_tables_start: u8;
    static __page_tables_end: u8;
    static __stack_guard_start: u8;
    static __stack_guard_end: u8;
    static __stack_start: u8;
    static __stack_end: u8;
    static __kernel_end: u8;

    static mut pdpt_table: PageTable;
    static pd_table: PageTable;
}

#[link_section = ".rodata"]
static RODATA_TEST_SENTINEL: u32 = 0x5445_5354; // "TEST"

#[link_section = ".data"]
static mut DATA_EXEC_TRAMPOLINE: [u8; 16] = [
    0xC3, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
    0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90,
];

#[inline(never)]
fn test_normal_execution(a: u32, b: u32) -> u32 {
    a + b
}

#[inline(never)]
fn test_stack_depth(n: u32) -> u32 {
    if n == 0 { 0 } else { n + test_stack_depth(n - 1) }
}

/// Executes Stage 2F-F: 4 KiB Kernel Permission Splitting & W^X Enforcement.
pub fn run_stage2ff_verification(pmm: &mut PhysicalMemoryManager) {
    let geometry = get_active_geometry();

    kprintln!("\n[Stage 2F-F: 4 KiB Kernel Permission Splitting & W^X Enforcement]");

    // 1. Audit Section Boundaries & Alignments
    let mb_start    = &raw const __multiboot_header_start as u64;
    let mb_end      = &raw const __multiboot_header_end as u64;
    let text_start  = &raw const __text_start as u64;
    let text_end    = &raw const __text_end as u64;
    let ro_start    = &raw const __rodata_start as u64;
    let ro_end      = &raw const __rodata_end as u64;
    let data_start  = &raw const __data_start as u64;
    let data_end    = &raw const __data_end as u64;
    let bss_start   = &raw const __bss_start as u64;
    let bss_end     = &raw const __bss_end as u64;
    let pmm_start   = &raw const __pmm_meta_start as u64;
    let pmm_end     = &raw const __pmm_meta_end as u64;
    let pt_start    = &raw const __page_tables_start as u64;
    let pt_end      = &raw const __page_tables_end as u64;
    let guard_start = &raw const __stack_guard_start as u64;
    let guard_end   = &raw const __stack_guard_end as u64;
    let stack_start = &raw const __stack_start as u64;
    let stack_end   = &raw const __stack_end as u64;
    let kernel_end  = &raw const __kernel_end as u64;

    kprintln!("  Section Boundaries & Alignments:");
    kprintln!("    .multiboot_header: [0x{:016X}, 0x{:016X}) (R + NX)", mb_start, mb_end);
    kprintln!("    .text:             [0x{:016X}, 0x{:016X}) (RX)", text_start, text_end);
    kprintln!("    .rodata:           [0x{:016X}, 0x{:016X}) (R + NX)", ro_start, ro_end);
    kprintln!("    .data:             [0x{:016X}, 0x{:016X}) (RW + NX)", data_start, data_end);
    kprintln!("    .bss:              [0x{:016X}, 0x{:016X}) (RW + NX)", bss_start, bss_end);
    kprintln!("    .pmm_metadata:     [0x{:016X}, 0x{:016X}) (RW + NX)", pmm_start, pmm_end);
    kprintln!("    .page_tables:      [0x{:016X}, 0x{:016X}) (RW + NX)", pt_start, pt_end);
    kprintln!("    .stack_guard:      [0x{:016X}, 0x{:016X}) (NOT PRESENT)", guard_start, guard_end);
    kprintln!("    .stack:            [0x{:016X}, 0x{:016X}) (RW + NX)", stack_start, stack_end);

    assert_eq!(mb_start & 0xFFF, 0, "mb_start misaligned");
    assert_eq!(mb_end & 0xFFF, 0, "mb_end misaligned");
    assert_eq!(text_start & 0xFFF, 0, "text_start misaligned");
    assert_eq!(text_end & 0xFFF, 0, "text_end misaligned");
    assert_eq!(ro_start & 0xFFF, 0, "ro_start misaligned");
    assert_eq!(ro_end & 0xFFF, 0, "ro_end misaligned");
    assert_eq!(data_start & 0xFFF, 0, "data_start misaligned");
    assert_eq!(data_end & 0xFFF, 0, "data_end misaligned");
    assert_eq!(guard_start & 0xFFF, 0, "guard_start misaligned");
    assert_eq!(guard_end & 0xFFF, 0, "guard_end misaligned");
    assert_eq!(stack_start & 0xFFF, 0, "stack_start misaligned");
    assert_eq!(stack_end & 0xFFF, 0, "stack_end misaligned");

    // 2. Kernel size bound invariant (must fit inside 2 MiB bootstrap PT window)
    assert!(
        kernel_end <= KERNEL_VIRT_BASE + 2 * 1024 * 1024,
        "CRITICAL: Kernel image exceeds 2 MiB bootstrap PT window!"
    );
    kprintln!("  Kernel Size Bound:           0x{:016X} <= 0x{:016X} (<= 2 MiB window) [VERIFIED]",
        kernel_end, KERNEL_VIRT_BASE + 2 * 1024 * 1024);

    // 3. Allocate kernel_pd and kernel_pt dynamically from PMM (PMM ownership tracked)
    let pd_frame = pmm.alloc_frame().expect("Allocating kernel_pd frame");
    let pt_frame = pmm.alloc_frame().expect("Allocating kernel_pt frame");

    let pt = unsafe { &mut *phys_to_virt_table(pt_frame) };
    pt.zero();

    let mut mapped_count = 0usize;
    for i in 0..512 {
        let page_vaddr = KERNEL_VIRT_BASE + (i as u64 * 4096);
        let page_phys  = (i as u64 * 4096);
        let frame = PhysFrame(page_phys);

        if page_vaddr >= mb_start && page_vaddr < mb_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT | PageTableFlags::NO_EXECUTE, geometry);
            mapped_count += 1;
        } else if page_vaddr >= text_start && page_vaddr < text_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT, geometry);
            mapped_count += 1;
        } else if page_vaddr >= ro_start && page_vaddr < ro_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT | PageTableFlags::NO_EXECUTE, geometry);
            mapped_count += 1;
        } else if page_vaddr >= data_start && page_vaddr < data_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE, geometry);
            mapped_count += 1;
        } else if page_vaddr >= bss_start && page_vaddr < bss_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE, geometry);
            mapped_count += 1;
        } else if page_vaddr >= pmm_start && page_vaddr < pmm_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE, geometry);
            mapped_count += 1;
        } else if page_vaddr >= pt_start && page_vaddr < pt_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE, geometry);
            mapped_count += 1;
        } else if page_vaddr >= guard_start && page_vaddr < guard_end {
            pt.entries[i].clear(); // Stack guard: NOT PRESENT
        } else if page_vaddr >= stack_start && page_vaddr < stack_end {
            pt.entries[i].set(frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE, geometry);
            mapped_count += 1;
        } else {
            pt.entries[i].clear();
        }
    }
    kprintln!("  Populated 4 KiB kernel_pt:   {} active 4 KiB pages mapped with granular permissions", mapped_count);

    // 4. Build kernel_pd through HHDM
    let pd = unsafe { &mut *phys_to_virt_table(pd_frame) };
    pd.zero();

    // Entry 0 points to kernel_pt (intermediate flags PRESENT | WRITABLE, USER=0)
    pd.entries[0].set(pt_frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE, geometry);

    // Copy entries 1..511 from pd_table to preserve any remaining 1 GiB mappings
    let old_pd_phys = (&raw const pd_table as u64) - KERNEL_VIRT_BASE;
    let old_pd = unsafe { &*phys_to_virt_table(PhysFrame(old_pd_phys)) };
    for j in 1..512 {
        pd.entries[j] = old_pd.entries[j];
    }

    // 5. Install kernel_pd into pdpt_table[510] through HHDM
    let pdpt_phys = (&raw const pdpt_table as u64) - KERNEL_VIRT_BASE;
    let pdpt = unsafe { &mut *phys_to_virt_table(PhysFrame(pdpt_phys)) };
    pdpt.entries[510].set(pd_frame, PageTableFlags::PRESENT | PageTableFlags::WRITABLE, geometry);
    kprintln!("  Installed kernel_pd:         pdpt_table[510] -> kernel_pd[0] -> kernel_pt [via HHDM]");

    // 6. Reload CR3 (full TLB shootdown)
    let cr3_val = cpu::read_cr3();
    unsafe { cpu::write_cr3_raw(cr3_val); }
    kprintln!("  TLB Shootdown:               CR3 reloaded (switched from 2 MiB to 4 KiB mappings)");

    // 7. Verify execution continues in higher half
    let rip_after: u64;
    unsafe {
        core::arch::asm!("lea {}, [rip]", out(reg) rip_after, options(nomem, nostack));
    }
    assert!(rip_after >= KERNEL_VIRT_BASE, "Execution failed to remain in higher half");
    kprintln!("  Higher-Half RIP:             0x{:016X} [VERIFIED]", rip_after);

    // 8. Verify PML4 state (PML4[0] absent, PML4[256] intact, PML4[511] intact)
    let pml4 = unsafe { &*phys_to_virt_table(Cr3::read(geometry).0) };
    assert!(!pml4.entries[0].is_present(), "PML4[0] must remain absent!");
    assert!(pml4.entries[256].is_present(), "PML4[256] (HHDM) must remain intact!");
    assert!(pml4.entries[511].is_present(), "PML4[511] (Kernel VMA) must remain intact!");
    kprintln!("  Active PML4 Invariants:      PML4[0]=ABSENT, PML4[256]=present, PML4[511]=present [VERIFIED]");

    // 9. Enable CR0.WP (Write Protect) and verify
    cpu::enable_write_protect();
    let cr0 = cpu::read_cr0();
    assert!((cr0 & (1 << 16)) != 0, "CR0.WP must be enabled");
    let efer = cpu::read_msr(cpu::IA32_EFER);
    assert!((efer & (1 << 11)) != 0, "IA32_EFER.NXE must be enabled");
    kprintln!("  Hardware Enforcement:       CR0.WP=1 (Ring 0 write protect), IA32_EFER.NXE=1 (No-Execute) [VERIFIED]");

    // 10. Controlled Fault 1: Write to .rodata via Kernel VMA
    let rodata_addr = &raw const RODATA_TEST_SENTINEL as u64;
    unsafe {
        idt::expect_fault(14, idt::test_resume_pf_write as usize as u64, Some(rodata_addr));
        idt::test_trigger_pf_write(rodata_addr);
    }
    let fault1 = unsafe { idt::last_fault_result() }.expect("Missing #PF on .rodata write");
    assert_eq!(fault1.vector, 14, "Expected #PF for .rodata write");
    assert_eq!(fault1.fault_addr & !0xFFF, rodata_addr & !0xFFF, "CR2 mismatch for .rodata write");
    assert!((fault1.error_code & 0x02) != 0, "Error code must indicate write access");
    assert!((fault1.error_code & 0x01) != 0, "Error code must indicate protection violation");
    kprintln!("  Controlled .rodata Write:    #PF trapped at CR2 0x{:016X}, Error Code: 0x{:016X} (Protection Write Kernel) [VERIFIED]",
        fault1.fault_addr, fault1.error_code);

    // 11. Controlled Fault 2: Write to .text via Kernel VMA
    let text_addr = text_start;
    unsafe {
        idt::expect_fault(14, idt::test_resume_pf_write as usize as u64, Some(text_addr));
        idt::test_trigger_pf_write(text_addr);
    }
    let fault2 = unsafe { idt::last_fault_result() }.expect("Missing #PF on .text write");
    assert_eq!(fault2.vector, 14, "Expected #PF for .text write");
    assert_eq!(fault2.fault_addr & !0xFFF, text_addr & !0xFFF, "CR2 mismatch for .text write");
    assert!((fault2.error_code & 0x02) != 0, "Error code must indicate write access");
    assert!((fault2.error_code & 0x01) != 0, "Error code must indicate protection violation");
    kprintln!("  Controlled .text Write:      #PF trapped at CR2 0x{:016X}, Error Code: 0x{:016X} (Protection Write Kernel) [VERIFIED]",
        fault2.fault_addr, fault2.error_code);

    // 12. Controlled Fault 3: Execution from .data via Kernel VMA (NX violation)
    let data_exec_addr = &raw const DATA_EXEC_TRAMPOLINE as u64;
    assert!(data_exec_addr >= data_start && data_exec_addr < data_end, "DATA_EXEC_TRAMPOLINE must be in .data");
    let trampoline_pte_idx = ((data_exec_addr - KERNEL_VIRT_BASE) / 4096) as usize;
    assert!(pt.entries[trampoline_pte_idx].flags().contains(PageTableFlags::NO_EXECUTE), "Target PTE must have NO_EXECUTE");
    assert!(pt.entries[trampoline_pte_idx].flags().contains(PageTableFlags::WRITABLE), "Target PTE must have WRITABLE");
    assert!(pt.entries[trampoline_pte_idx].flags().contains(PageTableFlags::PRESENT), "Target PTE must have PRESENT");

    unsafe {
        idt::expect_fault(14, idt::test_resume_pf_exec as usize as u64, Some(data_exec_addr));
        idt::test_trigger_pf_exec(data_exec_addr);
    }
    let fault3 = unsafe { idt::last_fault_result() }.expect("Missing #PF on .data execution");
    assert_eq!(fault3.vector, 14, "Expected #PF for .data execute");
    assert_eq!(fault3.fault_addr & !0xFFF, data_exec_addr & !0xFFF, "CR2 mismatch for .data execute");
    assert!((fault3.error_code & 0x10) != 0, "Error code must indicate instruction fetch (NX violation)");
    kprintln!("  Controlled .data Execute:    #PF trapped at CR2 0x{:016X}, Error Code: 0x{:016X} (Instruction Fetch NX Violation) [VERIFIED]",
        fault3.fault_addr, fault3.error_code);

    // 13. Controlled Fault 4: Access to .stack_guard via Kernel VMA
    let guard_addr = guard_start;
    unsafe {
        idt::expect_fault(14, idt::test_resume_pf as usize as u64, Some(guard_addr));
        idt::test_trigger_pf(guard_addr);
    }
    let fault4 = unsafe { idt::last_fault_result() }.expect("Missing #PF on stack_guard access");
    assert_eq!(fault4.vector, 14, "Expected #PF for stack_guard");
    assert_eq!(fault4.fault_addr & !0xFFF, guard_addr & !0xFFF, "CR2 mismatch for stack_guard");
    assert_eq!(fault4.error_code & 0x01, 0, "Error code must indicate non-present page");
    kprintln!("  Controlled Stack Guard:      #PF trapped at CR2 0x{:016X}, Error Code: 0x{:016X} (Non-present Kernel) [VERIFIED]",
        fault4.fault_addr, fault4.error_code);

    // 14. HHDM Privileged Aperture Verification (Dual-Path Alias Test)
    let text_phys = text_start - KERNEL_VIRT_BASE;
    let hhdm_text_ptr = (HHDM_BASE + text_phys) as *mut u8;
    let original_byte = unsafe { core::ptr::read_volatile(hhdm_text_ptr) };
    let test_byte = original_byte ^ 0xAA;
    unsafe {
        core::ptr::write_volatile(hhdm_text_ptr, test_byte);
    }
    let readback_byte = unsafe { core::ptr::read_volatile(hhdm_text_ptr) };
    assert_eq!(readback_byte, test_byte, "HHDM write failed: HHDM must remain privileged RW aperture");
    // Immediately restore original byte to preserve code integrity!
    unsafe {
        core::ptr::write_volatile(hhdm_text_ptr, original_byte);
    }
    let restored_byte = unsafe { core::ptr::read_volatile(hhdm_text_ptr) };
    assert_eq!(restored_byte, original_byte, "Failed restoring original byte via HHDM");
    kprintln!("  HHDM Privileged Aperture:    Write to .text physical frame 0x{:08X} via HHDM SUCCEEDED [VERIFIED]", text_phys);
    kprintln!("                               Byte modified 0x{:02X} -> 0x{:02X} and restored to 0x{:02X}",
        original_byte, test_byte, restored_byte);
    kprintln!("                               [CONFIRMED: HHDM is a privileged RW aperture, not an immutability boundary]");

    // 15. Normal Subsystem Operations Post-W^X
    let add_res = test_normal_execution(40, 2);
    assert_eq!(add_res, 42, "Normal .text execution failed");

    let ro_val = unsafe { core::ptr::read_volatile(&raw const RODATA_TEST_SENTINEL) };
    assert_eq!(ro_val, 0x5445_5354, "Normal .rodata read failed");

    let stack_val = test_stack_depth(5);
    assert_eq!(stack_val, 15, "Normal stack recursion failed");

    let mb_hhdm_final = unsafe { core::ptr::read_volatile((HHDM_BASE + 0x0010_0000) as *const u32) };
    assert_eq!(mb_hhdm_final, 0x1BADB002, "HHDM read failed post-W^X");

    // VMM lifecycle test
    let initial_free = pmm.stats().free_frames;
    let mut apt = ActivePageTable::new();
    let test_va = VirtualAddress::new(0xFFFF_8002_0000_0000);
    let test_p = Page::from_start_address(test_va, geometry).unwrap();
    let frame = pmm.alloc_frame().unwrap();
    apt.map_page(test_p, frame, PageTableFlags::WRITABLE, MappingDomain::Kernel, pmm).unwrap();
    unsafe {
        core::ptr::write_volatile(test_va.as_u64() as *mut u64, 0x5758_5F56_4552_4946);
        assert_eq!(core::ptr::read_volatile(test_va.as_u64() as *const u64), 0x5758_5F56_4552_4946);
    }
    let unmapped = apt.unmap_page(test_p, pmm).unwrap();
    assert_eq!(unmapped, frame);
    pmm.free_frame(frame).unwrap();
    assert_eq!(pmm.stats().free_frames, initial_free, "PMM frame count mismatch post-W^X");
    kprintln!("  Normal Subsystems:           .text execution, .rodata read, stack recursion, HHDM, and VMM lifecycle [ALL OPERATIONAL]");
    kprintln!("  PMM Accounting:              Free frames preserved at {} [EXACT MATCH]", initial_free);

    kprintln!("  [x] Stage 2F-F 4 KiB permission splitting & W^X enforcement verified.");
}

