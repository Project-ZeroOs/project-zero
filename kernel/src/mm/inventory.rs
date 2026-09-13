//! Project Zero - Multiboot Memory-Map Discovery & Physical Memory Inventory (Stage 2C)
//!
//! Provides robust Multiboot 1 memory map parsing, 64-bit physical address preservation,
//! explicit reservation of Project Zero kernel structures and boot metadata, and establishes
//! an authoritative read-only physical memory inventory.
//!
//! Architectural Invariant: Usable RAM is NOT equivalent to immediately allocatable memory.
//! Firmware available RAM is subtracted by Project Zero reservations to discover clean,
//! usable memory ranges for future allocation (Stage 2D).

use crate::kprintln;
use crate::hal::arch::x86_64::gdt::{
    get_kernel_image_range, get_kernel_stack_range, get_page_tables_range,
    get_ist1_stack_range, get_gdt_range, get_tss_range, get_idt_range,
    get_stack_guard_range,
};

/// Multiboot 1 Bootloader Magic (passed by bootloader in EAX).
pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002;

/// Maximum number of memory regions tracked in Stage 2C inventory tables.
pub const MAX_INVENTORY_REGIONS: usize = 32;
pub const MAX_RESERVATIONS: usize = 32;
pub const MAX_USABLE_RANGES: usize = 32;

// ====================================================================
// Multiboot 1 Architectural Structures
// ====================================================================

/// Multiboot 1 Information Structure.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MultibootInfo {
    pub flags: u32,
    pub mem_lower: u32,
    pub mem_upper: u32,
    pub boot_device: u32,
    pub cmdline: u32,
    pub mods_count: u32,
    pub mods_addr: u32,
    pub syms: [u32; 4],
    pub mmap_length: u32,
    pub mmap_addr: u32,
    pub drives_length: u32,
    pub drives_addr: u32,
    pub config_table: u32,
    pub boot_loader_name: u32,
    pub apm_table: u32,
    pub vbe_control_info: u32,
    pub vbe_mode_info: u32,
    pub vbe_mode: u16,
    pub vbe_interface_seg: u16,
    pub vbe_interface_off: u16,
    pub vbe_interface_len: u16,
    pub framebuffer_addr: u64,
    pub framebuffer_pitch: u32,
    pub framebuffer_width: u32,
    pub framebuffer_height: u32,
    pub framebuffer_bpp: u8,
    pub framebuffer_type: u8,
    pub color_info: [u8; 6],
}

/// Multiboot 1 Memory Map Entry.
/// Note: In Multiboot 1, the `size` field contains the byte size of the fields
/// immediately following `size`. The next entry begins at `current_ptr + size + 4`.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MultibootMmapEntry {
    pub size: u32,
    pub base_addr: u64,
    pub length: u64,
    pub type_: u32,
}

// ====================================================================
// Memory Classification & Region Models
// ====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    AvailableRAM,
    Reserved,
    AcpiReclaimable,
    AcpiNvs,
    BadRam,
    Unknown(u32),
}

impl MemoryRegionType {
    pub fn from_multiboot(type_code: u32) -> Self {
        match type_code {
            1 => MemoryRegionType::AvailableRAM,
            2 => MemoryRegionType::Reserved,
            3 => MemoryRegionType::AcpiReclaimable,
            4 => MemoryRegionType::AcpiNvs,
            5 => MemoryRegionType::BadRam,
            other => MemoryRegionType::Unknown(other),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryRegionType::AvailableRAM => "Available RAM (Type 1)",
            MemoryRegionType::Reserved => "Reserved (Type 2)",
            MemoryRegionType::AcpiReclaimable => "ACPI Reclaimable (Type 3)",
            MemoryRegionType::AcpiNvs => "ACPI NVS (Type 4)",
            MemoryRegionType::BadRam => "Defective BadRAM (Type 5)",
            MemoryRegionType::Unknown(_) => "Unknown Region",
        }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, MemoryRegionType::AvailableRAM)
    }
}

/// A half-open physical memory region `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalMemoryRegion {
    pub start: u64,
    pub end: u64,
    pub region_type: MemoryRegionType,
    pub is_usable: bool,
}

impl PhysicalMemoryRegion {
    pub const fn empty() -> Self {
        Self {
            start: 0,
            end: 0,
            region_type: MemoryRegionType::Reserved,
            is_usable: false,
        }
    }

    pub const fn new(start: u64, end: u64, region_type: MemoryRegionType) -> Self {
        Self {
            start,
            end,
            is_usable: matches!(region_type, MemoryRegionType::AvailableRAM),
            region_type,
        }
    }

    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn overlaps(&self, other: &PhysicalMemoryRegion) -> bool {
        let max_start = if self.start > other.start { self.start } else { other.start };
        let min_end = if self.end < other.end { self.end } else { other.end };
        max_start < min_end
    }
}

/// A reserved region descriptor with ownership identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservationEntry {
    pub name: &'static str,
    pub start: u64,
    pub end: u64,
}

impl ReservationEntry {
    pub const fn empty() -> Self {
        Self { name: "", start: 0, end: 0 }
    }

    pub const fn new(name: &'static str, start: u64, end: u64) -> Self {
        Self { name, start, end }
    }

    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn overlaps_range(&self, start: u64, end: u64) -> bool {
        let max_start = if self.start > start { self.start } else { start };
        let min_end = if self.end < end { self.end } else { end };
        max_start < min_end
    }
}

// ====================================================================
// Physical Memory Inventory
// ====================================================================

// ====================================================================
// Physical Memory Inventory
// ====================================================================

pub struct PhysicalMemoryInventory {
    pub raw_regions: [PhysicalMemoryRegion; MAX_INVENTORY_REGIONS],
    pub raw_count: usize,
    pub reservations: [ReservationEntry; MAX_RESERVATIONS],
    pub reservation_count: usize,
    pub usable_ranges: [PhysicalMemoryRegion; MAX_USABLE_RANGES],
    pub usable_count: usize,

    // Derived 4 KiB frame candidates
    pub frame_candidates: [PhysicalMemoryRegion; MAX_USABLE_RANGES],
    pub frame_candidate_count: usize,

    // Address space breakdown (by Multiboot classification)
    pub total_address_space_bytes: u64,
    pub firmware_type1_available_bytes: u64,
    pub reserved_address_space_bytes: u64,
    pub acpi_reclaimable_bytes: u64,
    pub acpi_nvs_bytes: u64,
    pub bad_ram_bytes: u64,
    pub other_address_space_bytes: u64,

    // Rigorous reservation accounting
    pub unique_reservation_union_bytes: u64,
    pub reservation_intersection_type1_bytes: u64,
    pub discoverable_byte_level_ram: u64,
    pub page_aligned_frame_candidate_bytes: u64,
    pub subpage_remainder_bytes: u64,

    // Stage 2D PMM Metadata Bootstrap & Candidate Accounting
    pub stage2c_candidate_frames_before_pmm: u64,
    pub pmm_metadata_start: u64,
    pub pmm_metadata_end: u64,
    pub pmm_metadata_frames: u64,
    pub stage2d_candidate_frames_after_pmm: u64,
}

impl PhysicalMemoryInventory {
    pub const fn new() -> Self {
        Self {
            raw_regions: [PhysicalMemoryRegion::empty(); MAX_INVENTORY_REGIONS],
            raw_count: 0,
            reservations: [ReservationEntry::empty(); MAX_RESERVATIONS],
            reservation_count: 0,
            usable_ranges: [PhysicalMemoryRegion::empty(); MAX_USABLE_RANGES],
            usable_count: 0,
            frame_candidates: [PhysicalMemoryRegion::empty(); MAX_USABLE_RANGES],
            frame_candidate_count: 0,

            total_address_space_bytes: 0,
            firmware_type1_available_bytes: 0,
            reserved_address_space_bytes: 0,
            acpi_reclaimable_bytes: 0,
            acpi_nvs_bytes: 0,
            bad_ram_bytes: 0,
            other_address_space_bytes: 0,

            unique_reservation_union_bytes: 0,
            reservation_intersection_type1_bytes: 0,
            discoverable_byte_level_ram: 0,
            page_aligned_frame_candidate_bytes: 0,
            subpage_remainder_bytes: 0,

            stage2c_candidate_frames_before_pmm: 0,
            pmm_metadata_start: 0,
            pmm_metadata_end: 0,
            pmm_metadata_frames: 0,
            stage2d_candidate_frames_after_pmm: 0,
        }
    }

    pub fn add_reservation(&mut self, name: &'static str, start: u64, end: u64) {
        if start >= end || self.reservation_count >= MAX_RESERVATIONS {
            return;
        }
        self.reservations[self.reservation_count] = ReservationEntry::new(name, start, end);
        self.reservation_count += 1;
    }

    /// Checks if a physical byte address falls within any page-aligned candidate pool.
    pub fn is_candidate_address(&self, addr: u64) -> bool {
        for i in 0..self.frame_candidate_count {
            let cand = &self.frame_candidates[i];
            if addr >= cand.start && addr < cand.end {
                return true;
            }
        }
        false
    }
}

pub static mut INVENTORY: PhysicalMemoryInventory = PhysicalMemoryInventory::new();

// ====================================================================
// Multiboot Memory Map Parser
// ====================================================================

/// Validates the Multiboot 1 magic value passed by the bootloader.
pub fn validate_multiboot_magic(magic: u32) -> Result<(), &'static str> {
    if magic == MULTIBOOT_BOOTLOADER_MAGIC {
        Ok(())
    } else {
        Err("Invalid Multiboot 1 bootloader magic! Expected 0x2BADB002.")
    }
}

/// Parses the Multiboot 1 memory map from physical memory and populates the inventory.
pub fn parse_memory_map(
    mbi_phys: u64,
    multiboot_magic: u32,
) -> Result<&'static PhysicalMemoryInventory, &'static str> {
    // 1. Validate Multiboot magic
    validate_multiboot_magic(multiboot_magic)?;

    if mbi_phys == 0 {
        return Err("Multiboot information pointer is null (0)!");
    }

    let mbi = unsafe { &*(mbi_phys as *const MultibootInfo) };

    // Check flag bit 6: memory map valid
    if (mbi.flags & (1 << 6)) == 0 {
        return Err("Multiboot flags indicate memory map is not present (flag bit 6 is clear)!");
    }

    if mbi.mmap_addr == 0 || mbi.mmap_length == 0 {
        return Err("Multiboot memory map address or length is zero!");
    }

    let inv = unsafe { &mut INVENTORY };
    *inv = PhysicalMemoryInventory::new();

    // 2. Parse Raw Firmware Memory Map Entries
    let mut offset: u32 = 0;
    while offset < mbi.mmap_length {
        if offset + 4 > mbi.mmap_length {
            break; // Truncated entry header
        }

        let entry_ptr = (mbi.mmap_addr as u64) + (offset as u64);
        let entry_size = unsafe { *(entry_ptr as *const u32) };

        // Multiboot 1 specification requirement: entry_size must be >= 20
        if entry_size < 20 {
            break;
        }

        let next_offset = match offset.checked_add(entry_size).and_then(|o| o.checked_add(4)) {
            Some(no) if no <= mbi.mmap_length => no,
            _ => break,
        };

        let raw_entry = unsafe { &*(entry_ptr as *const MultibootMmapEntry) };
        let base = raw_entry.base_addr;
        let len = raw_entry.length;
        let type_code = raw_entry.type_;

        // Check for 64-bit integer overflow in region calculation
        if let Some(end) = base.checked_add(len) {
            if len > 0 && inv.raw_count < MAX_INVENTORY_REGIONS {
                let reg_type = MemoryRegionType::from_multiboot(type_code);
                let region = PhysicalMemoryRegion::new(base, end, reg_type);
                inv.raw_regions[inv.raw_count] = region;
                inv.raw_count += 1;

                inv.total_address_space_bytes += len;
                match reg_type {
                    MemoryRegionType::AvailableRAM => inv.firmware_type1_available_bytes += len,
                    MemoryRegionType::Reserved => inv.reserved_address_space_bytes += len,
                    MemoryRegionType::AcpiReclaimable => inv.acpi_reclaimable_bytes += len,
                    MemoryRegionType::AcpiNvs => inv.acpi_nvs_bytes += len,
                    MemoryRegionType::BadRam => inv.bad_ram_bytes += len,
                    MemoryRegionType::Unknown(_) => inv.other_address_space_bytes += len,
                }
            }
        }

        offset = next_offset;
    }

    if inv.raw_count == 0 {
        return Err("No valid Multiboot memory map entries were discovered!");
    }

    // 3. Register Explicit Project Zero Kernel & Boot Reservations
    // Low Physical Memory Preservation [0x00000000, 0x00100000) (1 MiB)
    inv.add_reservation("Low Memory (IVT/BDA/ROM)", 0x0000_0000, 0x0010_0000);

    // Kernel Image Code & Data [.text, .rodata, .data]
    let kimage = get_kernel_image_range();
    inv.add_reservation(kimage.name, kimage.start, kimage.end);

    // Early 4-Level Page Tables
    let ptables = get_page_tables_range();
    inv.add_reservation(ptables.name, ptables.start, ptables.end);

    // Stack Guard Page (4 KiB unmapped page directly below stack)
    let guard = get_stack_guard_range();
    inv.add_reservation(guard.name, guard.start, guard.end);

    // Normal Kernel Stack (64 KiB)
    let kstack = get_kernel_stack_range();
    inv.add_reservation(kstack.name, kstack.start, kstack.end);

    // IDT (256 Descriptors)
    let idt = get_idt_range();
    inv.add_reservation(idt.name, idt.start, idt.end);

    // Dedicated Double-Fault IST1 Stack
    let ist1 = get_ist1_stack_range();
    inv.add_reservation(ist1.name, ist1.start, ist1.end);

    // GDT (7 Descriptors)
    let gdt = get_gdt_range();
    inv.add_reservation(gdt.name, gdt.start, gdt.end);

    // TSS Structure
    let tss = get_tss_range();
    inv.add_reservation(tss.name, tss.start, tss.end);

    // Multiboot Info Structure
    let mbi_start = mbi_phys;
    let mbi_end = mbi_phys + (core::mem::size_of::<MultibootInfo>() as u64);
    inv.add_reservation("Multiboot Info Structure", mbi_start, mbi_end);

    // Multiboot Memory Map Buffer
    let mmap_start = mbi.mmap_addr as u64;
    let mmap_end = (mbi.mmap_addr as u64) + (mbi.mmap_length as u64);
    inv.add_reservation("Multiboot Memory-Map Storage", mmap_start, mmap_end);

    // Multiboot Modules (if present)
    if mbi.mods_count > 0 && mbi.mods_addr != 0 {
        let mods_start = mbi.mods_addr as u64;
        let mods_end = mods_start + ((mbi.mods_count as u64) * 16);
        inv.add_reservation("Multiboot Modules Metadata", mods_start, mods_end);
    }
    // 4. Perform Stage 2C Reservation Accounting & Interval Subtraction (before PMM metadata)
    compute_usable_memory_inventory(inv);
    let stage2c_cands = inv.page_aligned_frame_candidate_bytes / 4096;

    // 5. Stage 2D: Register PMM Metadata Reservation
    // Statically placed in .bss and registered as a Project Zero critical reservation
    // BEFORE Stage 2D candidate pools are finalized.
    let (pmm_meta_start, pmm_meta_end) = crate::mm::pmm::get_pmm_metadata_range();

    // Verify PMM metadata does not overlap any existing critical kernel or boot structure
    for i in 0..inv.reservation_count {
        let res = &inv.reservations[i];
        let max_start = if res.start > pmm_meta_start { res.start } else { pmm_meta_start };
        let min_end = if res.end < pmm_meta_end { res.end } else { pmm_meta_end };
        if max_start < min_end {
            kprintln!("[FATAL] PMM Metadata [0x{:016X}, 0x{:016X}) overlaps reservation '{}' [0x{:016X}, 0x{:016X})!",
                pmm_meta_start, pmm_meta_end, res.name, res.start, res.end);
            panic!("PMM metadata overlaps existing reservation!");
        }
    }

    inv.add_reservation("PMM Bitmap Metadata", pmm_meta_start, pmm_meta_end);

    // 6. Compute Final Stage 2D Candidate Pools (excluding PMM metadata)
    compute_usable_memory_inventory(inv);
    let stage2d_cands = inv.page_aligned_frame_candidate_bytes / 4096;

    inv.stage2c_candidate_frames_before_pmm = stage2c_cands;
    inv.pmm_metadata_start = pmm_meta_start;
    inv.pmm_metadata_end = pmm_meta_end;
    inv.pmm_metadata_frames = (pmm_meta_end - pmm_meta_start) / 4096;
    inv.stage2d_candidate_frames_after_pmm = stage2d_cands;

    Ok(unsafe { &INVENTORY })
}

/// Interval subtraction and union accounting algorithm.
fn compute_usable_memory_inventory(inv: &mut PhysicalMemoryInventory) {
    inv.usable_count = 0;
    inv.discoverable_byte_level_ram = 0;
    inv.frame_candidate_count = 0;
    inv.page_aligned_frame_candidate_bytes = 0;
    inv.subpage_remainder_bytes = 0;

    // --- Part A: Compute Disjoint Union of all Project Zero Reservations ---
    // We maintain a list of disjoint reservation intervals.
    let mut disjoint_res: [(u64, u64); MAX_RESERVATIONS] = [(0, 0); MAX_RESERVATIONS];
    let mut disjoint_res_count = 0;

    for r_idx in 0..inv.reservation_count {
        let mut cur_start = inv.reservations[r_idx].start;
        let mut cur_end = inv.reservations[r_idx].end;
        if cur_start >= cur_end {
            continue;
        }

        // Merge cur with any existing overlapping intervals in disjoint_res
        let mut i = 0;
        while i < disjoint_res_count {
            let (ex_start, ex_end) = disjoint_res[i];
            // If they overlap or touch: max(cur_start, ex_start) <= min(cur_end, ex_end)
            if cur_start <= ex_end && cur_end >= ex_start {
                cur_start = if cur_start < ex_start { cur_start } else { ex_start };
                cur_end = if cur_end > ex_end { cur_end } else { ex_end };
                // Remove disjoint_res[i] by swapping with last
                disjoint_res[i] = disjoint_res[disjoint_res_count - 1];
                disjoint_res_count -= 1;
            } else {
                i += 1;
            }
        }

        if disjoint_res_count < MAX_RESERVATIONS {
            disjoint_res[disjoint_res_count] = (cur_start, cur_end);
            disjoint_res_count += 1;
        }
    }

    // Compute total unique reservation union bytes
    let mut total_union_bytes: u64 = 0;
    for i in 0..disjoint_res_count {
        let (s, e) = disjoint_res[i];
        total_union_bytes += e - s;
    }
    inv.unique_reservation_union_bytes = total_union_bytes;

    // --- Part B: Compute Intersection of Union(Reservations) with Type-1 RAM ---
    let mut intersection_bytes: u64 = 0;
    for r_idx in 0..inv.raw_count {
        let raw = inv.raw_regions[r_idx];
        if !raw.region_type.is_available() {
            continue;
        }

        for i in 0..disjoint_res_count {
            let (res_s, res_e) = disjoint_res[i];
            let max_s = if raw.start > res_s { raw.start } else { res_s };
            let min_e = if raw.end < res_e { raw.end } else { res_e };
            if max_s < min_e {
                intersection_bytes += min_e - max_s;
            }
        }
    }
    inv.reservation_intersection_type1_bytes = intersection_bytes;

    // --- Part C: Subtract Reservations from Type-1 RAM to build Discoverable Usable Ranges ---
    let mut working_ranges: [PhysicalMemoryRegion; MAX_USABLE_RANGES] = [PhysicalMemoryRegion::empty(); MAX_USABLE_RANGES];
    let mut working_count = 0;

    for i in 0..inv.raw_count {
        let raw = inv.raw_regions[i];
        if raw.region_type.is_available() && raw.size() > 0 && working_count < MAX_USABLE_RANGES {
            working_ranges[working_count] = raw;
            working_count += 1;
        }
    }

    // Subtract each disjoint reservation from active working ranges
    for r_idx in 0..disjoint_res_count {
        let (res_start, res_end) = disjoint_res[r_idx];
        let mut next_ranges: [PhysicalMemoryRegion; MAX_USABLE_RANGES] = [PhysicalMemoryRegion::empty(); MAX_USABLE_RANGES];
        let mut next_count = 0;

        for w_idx in 0..working_count {
            let wr = working_ranges[w_idx];

            let max_s = if wr.start > res_start { wr.start } else { res_start };
            let min_e = if wr.end < res_end { wr.end } else { res_end };

            if max_s >= min_e {
                // No overlap
                if next_count < MAX_USABLE_RANGES {
                    next_ranges[next_count] = wr;
                    next_count += 1;
                }
            } else {
                // Overlap exists
                // 1. Left segment: [wr.start, res_start)
                if wr.start < res_start && next_count < MAX_USABLE_RANGES {
                    let left_end = if wr.end < res_start { wr.end } else { res_start };
                    if left_end > wr.start {
                        next_ranges[next_count] = PhysicalMemoryRegion::new(wr.start, left_end, MemoryRegionType::AvailableRAM);
                        next_count += 1;
                    }
                }
                // 2. Right segment: [res_end, wr.end)
                if wr.end > res_end && next_count < MAX_USABLE_RANGES {
                    let right_start = if wr.start > res_end { wr.start } else { res_end };
                    if wr.end > right_start {
                        next_ranges[next_count] = PhysicalMemoryRegion::new(right_start, wr.end, MemoryRegionType::AvailableRAM);
                        next_count += 1;
                    }
                }
            }
        }

        working_ranges = next_ranges;
        working_count = next_count;
    }

    // Sort discoverable usable ranges by start address (bubble sort for small array)
    if working_count > 1 {
        for i in 0..working_count - 1 {
            for j in 0..working_count - i - 1 {
                if working_ranges[j].start > working_ranges[j + 1].start {
                    let tmp = working_ranges[j];
                    working_ranges[j] = working_ranges[j + 1];
                    working_ranges[j + 1] = tmp;
                }
            }
        }
    }

    for i in 0..working_count {
        let range = working_ranges[i];
        inv.usable_ranges[i] = range;
        inv.discoverable_byte_level_ram += range.size();
    }
    inv.usable_count = working_count;

    // --- Part D: Derive 4 KiB Page-Aligned Frame Candidates for Stage 2D ---
    for i in 0..working_count {
        let u = inv.usable_ranges[i];
        // Align start UP to next 4096-byte boundary: (start + 4095) & !4095
        let frame_start = (u.start.saturating_add(4095)) & !4095;
        // Align end DOWN to preceding 4096-byte boundary: end & !4095
        let frame_end = u.end & !4095;

        if frame_start < frame_end {
            let candidate = PhysicalMemoryRegion::new(frame_start, frame_end, MemoryRegionType::AvailableRAM);
            if inv.frame_candidate_count < MAX_USABLE_RANGES {
                inv.frame_candidates[inv.frame_candidate_count] = candidate;
                inv.frame_candidate_count += 1;
                inv.page_aligned_frame_candidate_bytes += candidate.size();
            }
        }
    }

    inv.subpage_remainder_bytes = inv.discoverable_byte_level_ram.saturating_sub(inv.page_aligned_frame_candidate_bytes);
}

// ====================================================================
// Runtime Diagnostics Telemetry
// ====================================================================

pub fn print_memory_inventory_diagnostics(mbi_phys: u64, multiboot_magic: u32) {
    kprintln!("\n[Stage 2C: Multiboot Memory-Map Discovery & Inventory]");
    kprintln!("  Multiboot Magic:    0x{:08X} (Valid: {})", multiboot_magic, multiboot_magic == MULTIBOOT_BOOTLOADER_MAGIC);
    kprintln!("  Multiboot Info:     0x{:016X}", mbi_phys);

    let inv = unsafe { &INVENTORY };

    kprintln!("\n[Firmware Memory-Map Entries Discovered ({} regions)]", inv.raw_count);
    for i in 0..inv.raw_count {
        let r = inv.raw_regions[i];
        kprintln!("  [{:02}] [0x{:016X}, 0x{:016X}) {:>8} KiB  {}",
            i, r.start, r.end, r.size() / 1024, r.region_type.as_str());
    }

    kprintln!("\n[Project Zero Explicit Kernel & Boot Reservations ({} objects registered)]", inv.reservation_count);
    for i in 0..inv.reservation_count {
        let res = inv.reservations[i];
        kprintln!("  * {:<32} [0x{:016X}, 0x{:016X}) {:>6} KiB",
            res.name, res.start, res.end, res.size() / 1024);
    }

    kprintln!("\n[Discoverable Byte-Level Usable Ranges ({} ranges)]", inv.usable_count);
    for i in 0..inv.usable_count {
        let u = inv.usable_ranges[i];
        kprintln!("  * Range {:02}: [0x{:016X}, 0x{:016X}) ({:>7} KiB / {:>3} MiB)",
            i + 1, u.start, u.end, u.size() / 1024, u.size() / (1024 * 1024));
    }

    kprintln!("\n[Derived 4 KiB Page-Aligned Frame Candidates for Stage 2D ({} ranges)]", inv.frame_candidate_count);
    for i in 0..inv.frame_candidate_count {
        let f = inv.frame_candidates[i];
        let frame_count = f.size() / 4096;
        kprintln!("  * Pool {:02}:  [0x{:016X}, 0x{:016X}) ({:>7} KiB / {:>5} frames)",
            i + 1, f.start, f.end, f.size() / 1024, frame_count);
    }

    kprintln!("\n[Physical Address Space Breakdown]");
    kprintln!("  Total Physical Address Space Described: {:>8} KiB ({:>4} MiB)",
        inv.total_address_space_bytes / 1024, inv.total_address_space_bytes / (1024 * 1024));
    kprintln!("  Type-1 Available RAM:                   {:>8} KiB ({:>4} MiB)",
        inv.firmware_type1_available_bytes / 1024, inv.firmware_type1_available_bytes / (1024 * 1024));
    kprintln!("  Reserved Address Space:                 {:>8} KiB ({:>4} MiB)",
        inv.reserved_address_space_bytes / 1024, inv.reserved_address_space_bytes / (1024 * 1024));
    kprintln!("  ACPI Reclaimable:                       {:>8} KiB ({:>4} MiB)",
        inv.acpi_reclaimable_bytes / 1024, inv.acpi_reclaimable_bytes / (1024 * 1024));
    kprintln!("  ACPI NVS:                               {:>8} KiB ({:>4} MiB)",
        inv.acpi_nvs_bytes / 1024, inv.acpi_nvs_bytes / (1024 * 1024));
    kprintln!("  Bad RAM:                                {:>8} KiB ({:>4} MiB)",
        inv.bad_ram_bytes / 1024, inv.bad_ram_bytes / (1024 * 1024));
    kprintln!("  Other:                                  {:>8} KiB ({:>4} MiB)",
        inv.other_address_space_bytes / 1024, inv.other_address_space_bytes / (1024 * 1024));

    kprintln!("\n[Authoritative Stage 2C Memory Accounting]");
    kprintln!("  Firmware Type-1 RAM:                    {:>10} B ({:>8} KiB)",
        inv.firmware_type1_available_bytes, inv.firmware_type1_available_bytes / 1024);
    kprintln!("  Unique Project Zero Reservation Union:  {:>10} B ({:>8} KiB)",
        inv.unique_reservation_union_bytes, inv.unique_reservation_union_bytes / 1024);
    kprintln!("  Reservation Intersection With Type-1:   {:>10} B ({:>8} KiB)",
        inv.reservation_intersection_type1_bytes, inv.reservation_intersection_type1_bytes / 1024);
    kprintln!("  Final Discoverable Byte-Level RAM:      {:>10} B ({:>8} KiB)",
        inv.discoverable_byte_level_ram, inv.discoverable_byte_level_ram / 1024);
    kprintln!("  Page-Aligned Frame Candidate RAM:       {:>10} B ({:>8} KiB)",
        inv.page_aligned_frame_candidate_bytes, inv.page_aligned_frame_candidate_bytes / 1024);
    kprintln!("  Sub-Page Remainder:                     {:>10} B ({:>8} KiB)",
        inv.subpage_remainder_bytes, inv.subpage_remainder_bytes / 1024);

    // Exact byte-level invariant verification:
    // Type-1 RAM - Intersection = Discoverable Byte-Level RAM
    let calculated_usable = inv.firmware_type1_available_bytes - inv.reservation_intersection_type1_bytes;
    if calculated_usable == inv.discoverable_byte_level_ram {
        kprintln!("  Accounting Invariant:                   [x] VERIFIED (Type1 - Intersection == Usable)");
    } else {
        kprintln!("  Accounting Invariant:                   [!] FAILED MISMATCH");
    }

    kprintln!("\n[Stage 2D: PMM Metadata Bootstrap & Candidate Accounting]");
    kprintln!("  Stage 2C Candidate Frames (Before PMM Metadata): {:>7}", inv.stage2c_candidate_frames_before_pmm);
    kprintln!("  PMM Metadata Physical Range:                    [0x{:016X}, 0x{:016X})", inv.pmm_metadata_start, inv.pmm_metadata_end);
    kprintln!("  PMM Metadata Frames:                            {:>7} ({} KiB)", inv.pmm_metadata_frames, inv.pmm_metadata_frames * 4);
    kprintln!("  Stage 2D Candidate Frames (After PMM Metadata):  {:>7}", inv.stage2d_candidate_frames_after_pmm);
    kprintln!("  Candidate Frame Delta:                          {:>7} frames", (inv.stage2d_candidate_frames_after_pmm as i64) - (inv.stage2c_candidate_frames_before_pmm as i64));
}

