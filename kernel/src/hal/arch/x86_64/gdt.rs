//! Project Zero - Global Descriptor Table (GDT) & Task State Segment (TSS)
//!
//! Provides the x86-64 hardware segmentation and privilege boundary foundation.
//! Configures Ring 0 (Kernel) and Ring 3 (User) code/data descriptors, Task State
//! Segment (TSS) with RSP0 kernel stack, and an independent Interrupt Stack Table (IST1)
//! for Double Fault (#DF) handling.

use core::mem::size_of;
use crate::kprintln;
use super::cpu::{read_cs, read_ss, read_ds, read_es, read_fs, read_gs, read_tr};

// ====================================================================
// Formally Defined Segment Selectors
// ====================================================================

/// Index 1, DPL 0, RPL 0: 64-bit Kernel Code Segment (0x08)
pub const KERNEL_CODE_SELECTOR: u16 = 0x08;

/// Index 2, DPL 0, RPL 0: 64-bit Kernel Data Segment (0x10)
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;

/// Index 3, DPL 3, RPL 3: 64-bit User Data Segment (0x18 | 3 = 0x1B)
pub const USER_DATA_SELECTOR: u16 = 0x18 | 3;

/// Index 4, DPL 3, RPL 3: 64-bit User Code Segment (0x20 | 3 = 0x23)
pub const USER_CODE_SELECTOR: u16 = 0x20 | 3;

/// Index 5, DPL 0, RPL 0: 16-byte Task State Segment (TSS) Descriptor (0x28)
pub const TSS_SELECTOR: u16 = 0x28;

// ====================================================================
// Task State Segment (64-bit Architectural Structure)
// ====================================================================

/// 64-bit Task State Segment (TSS).
/// In Long Mode, hardware task switching via TSS is deprecated and unsupported.
/// Instead, it provides:
/// 1. `rsp0`: Kernel stack pointer loaded by hardware during privilege-changing
///    interrupt or exception entry into Ring 0.
/// 2. `ist1..7`: Interrupt Stack Table pointers for dedicated exception stacks (e.g. #DF).
/// 3. `iomap_base`: Offset to the I/O Permission Bit Map (set beyond TSS size if absent).
#[repr(C, packed)]
pub struct TaskStateSegment {
    reserved_0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    reserved_1: u64,
    pub ist1: u64, // IST1: Double Fault (#DF) stack
    pub ist2: u64,
    pub ist3: u64,
    pub ist4: u64,
    pub ist5: u64,
    pub ist6: u64,
    pub ist7: u64,
    reserved_2: u64,
    reserved_3: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn zeroed() -> Self {
        Self {
            reserved_0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            reserved_1: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            ist4: 0,
            ist5: 0,
            ist6: 0,
            ist7: 0,
            reserved_2: 0,
            reserved_3: 0,
            iomap_base: 0,
        }
    }
}


// ====================================================================
// Half-Open Critical Memory Range Model [start, end)
// ====================================================================

/// Represents a half-open memory interval `[start, end)`.
#[derive(Debug, Clone, Copy)]
pub struct MemoryRange {
    pub name: &'static str,
    pub start: u64,
    pub end: u64,
}

impl MemoryRange {
    pub const fn new(name: &'static str, start: u64, end: u64) -> Self {
        Self { name, start, end }
    }

    pub fn size(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Determines if two half-open intervals `[s1, e1)` and `[s2, e2)` intersect.
    pub fn overlaps(&self, other: &MemoryRange) -> bool {
        let max_start = if self.start > other.start { self.start } else { other.start };
        let min_end = if self.end < other.end { self.end } else { other.end };
        max_start < min_end
    }
}

// ====================================================================
// Stack Definitions & Allocations
// ====================================================================

/// 16-byte aligned 16 KiB dedicated stack for Double Fault (#DF) exception handling.
/// Provides an independent stack for double-fault handling and reduces the probability
/// that stack corruption prevents the #DF handler from executing.
#[repr(align(16))]
struct AlignedStack([u8; 16384]);

static mut DOUBLE_FAULT_STACK: AlignedStack = AlignedStack([0; 16384]);

pub const KERNEL_VIRT_BASE: u64 = 0xFFFF_FFFF_8000_0000;

#[inline(always)]
pub fn virt_to_phys(vaddr: u64) -> u64 {
    if vaddr >= KERNEL_VIRT_BASE {
        vaddr - KERNEL_VIRT_BASE
    } else {
        vaddr
    }
}

extern "C" {
    static __kernel_start: u8;
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
    fn load_gdt_and_tss(gdt_ptr: *const GdtDescriptor, tss_selector: u16);
}

// Global TSS instance (zeroed so it is placed in .bss alongside runtime tables)
static mut TSS: TaskStateSegment = TaskStateSegment::zeroed();

// Global GDT Table (7 entries of 64-bit slots = 56 bytes total)
static mut GDT: [u64; 7] = [0; 7];


/// GDT pointer passed to `lgdt` instruction.
#[repr(C, packed)]
pub struct GdtDescriptor {
    limit: u16,
    base: u64,
}

// ====================================================================
// Critical Memory Range Queries
// ====================================================================

pub fn get_kernel_image_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const __kernel_start as u64);
        let end = virt_to_phys(&raw const __bss_start as u64);
        MemoryRange::new("Kernel Image (Text/Data)", start, end)
    }
}

pub fn get_kernel_bss_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const __bss_start as u64);
        let end = virt_to_phys(&raw const __bss_end as u64);
        MemoryRange::new("Kernel BSS Storage", start, end)
    }
}

pub fn get_pmm_meta_section_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const __pmm_meta_start as u64);
        let end = virt_to_phys(&raw const __pmm_meta_end as u64);
        MemoryRange::new("PMM Bitmap Metadata Section", start, end)
    }
}

pub fn get_page_tables_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const __page_tables_start as u64);
        let end = virt_to_phys(&raw const __page_tables_end as u64);
        MemoryRange::new("Early Page Tables", start, end)
    }
}

pub fn get_stack_guard_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const __stack_guard_start as u64);
        let end = virt_to_phys(&raw const __stack_guard_end as u64);
        MemoryRange::new("Stack Guard Page", start, end)
    }
}

pub fn get_kernel_stack_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const __stack_start as u64);
        let end = virt_to_phys(&raw const __stack_end as u64);
        MemoryRange::new("Normal Kernel Stack", start, end)
    }
}

pub fn get_ist1_stack_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const DOUBLE_FAULT_STACK as u64);
        let end = start + 16384;
        MemoryRange::new("IST1 Double-Fault Stack", start, end)
    }
}

pub fn get_gdt_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const GDT as u64);
        let end = start + size_of::<[u64; 7]>() as u64;
        MemoryRange::new("GDT Table", start, end)
    }
}

pub fn get_tss_range() -> MemoryRange {
    unsafe {
        let start = virt_to_phys(&raw const TSS as u64);
        let end = start + size_of::<TaskStateSegment>() as u64;
        MemoryRange::new("TSS Structure", start, end)
    }
}

pub fn get_idt_range() -> MemoryRange {
    let (start, end) = super::idt::get_idt_bounds();
    MemoryRange::new("IDT Table (256 Descriptors)", virt_to_phys(start), virt_to_phys(end))
}

/// Collects all 7 critical boot-time memory ranges.
pub fn get_all_critical_ranges() -> [MemoryRange; 7] {
    [
        get_kernel_image_range(),
        get_kernel_stack_range(),
        get_page_tables_range(),
        get_ist1_stack_range(),
        get_gdt_range(),
        get_tss_range(),
        get_idt_range(),
    ]
}

/// Comprehensive verification of Stage 2F-A physical layout invariants:
/// - .bss ∩ stack = ∅
/// - .bss ∩ guard = ∅
/// - .bss ∩ page_tables = ∅
/// - .bss ∩ pmm_metadata = ∅
/// - guard ∩ stack = ∅
/// - guard sits directly below stack (guard.end == stack.start)
/// - Exact 4 KiB alignments and expected sizes
/// - Static globals inside .bss are strictly within [bss.start, bss.end) and pairwise disjoint
pub fn verify_stage2fa_invariants() -> bool {
    let kimage = get_kernel_image_range();
    let bss = get_kernel_bss_range();
    let pmm_meta_sec = get_pmm_meta_section_range();
    let ptables = get_page_tables_range();
    let guard = get_stack_guard_range();
    let stack = get_kernel_stack_range();
    let ist1 = get_ist1_stack_range();
    let gdt = get_gdt_range();
    let tss = get_tss_range();
    let idt = get_idt_range();

    // 1. Invariant: .bss ∩ stack = ∅
    assert!(!bss.overlaps(&stack), "Fatal: .bss overlaps with kernel stack!");

    // 2. Invariant: .bss ∩ guard = ∅
    assert!(!bss.overlaps(&guard), "Fatal: .bss overlaps with stack guard page!");

    // 3. Invariant: .bss ∩ page_tables = ∅
    assert!(!bss.overlaps(&ptables), "Fatal: .bss overlaps with early page tables!");

    // 4. Invariant: .bss ∩ pmm_metadata = ∅
    assert!(!bss.overlaps(&pmm_meta_sec), "Fatal: .bss overlaps with pmm_metadata section!");

    // 5. Invariant: guard ∩ stack = ∅
    assert!(!guard.overlaps(&stack), "Fatal: stack guard overlaps with kernel stack!");

    // 6. Invariant: guard sits directly below stack
    assert_eq!(guard.end, stack.start, "Fatal: stack guard page must sit directly below stack_bottom!");

    // 7. Invariant: alignments and sizes
    assert_eq!(guard.start % 4096, 0, "Guard start not 4 KiB aligned");
    assert_eq!(guard.end % 4096, 0, "Guard end not 4 KiB aligned");
    assert_eq!(guard.size(), 4096, "Guard size must be exactly 4096 bytes");
    assert_eq!(stack.start % 4096, 0, "Stack start not 4 KiB aligned");
    assert_eq!(stack.end % 4096, 0, "Stack end not 4 KiB aligned");
    assert_eq!(stack.size(), 65536, "Stack size must be exactly 64 KiB (65536 bytes)");
    assert_eq!(ptables.size(), 16384, "Page tables size must be exactly 16 KiB (16384 bytes)");
    assert_eq!(pmm_meta_sec.size(), 16384, "PMM metadata section size must be exactly 16 KiB (16384 bytes)");

    // 8. Invariant: all globals inside .bss are strictly bounded and mutually disjoint
    let bss_globals = [idt, ist1, gdt, tss];
    for g in &bss_globals {
        assert!(g.start >= bss.start && g.end <= bss.end, "Global {} outside .bss boundaries!", g.name);
    }
    for i in 0..bss_globals.len() {
        for j in (i + 1)..bss_globals.len() {
            assert!(!bss_globals[i].overlaps(&bss_globals[j]), "Globals {} and {} overlap inside .bss!", bss_globals[i].name, bss_globals[j].name);
        }
    }

    // 9. Invariant: section sequence is strictly disjoint:
    // kernel_image < bss < pmm_meta_sec < ptables < guard < stack
    assert!(!kimage.overlaps(&bss), "kimage overlaps with .bss");
    assert!(!kimage.overlaps(&pmm_meta_sec), "kimage overlaps with pmm_meta_sec");
    assert!(!kimage.overlaps(&ptables), "kimage overlaps with ptables");
    assert!(!kimage.overlaps(&guard), "kimage overlaps with guard");
    assert!(!kimage.overlaps(&stack), "kimage overlaps with stack");
    assert!(!bss.overlaps(&pmm_meta_sec), "bss overlaps with pmm_meta_sec");
    assert!(!pmm_meta_sec.overlaps(&ptables), "pmm_meta_sec overlaps with ptables");
    assert!(!ptables.overlaps(&guard), "ptables overlaps with guard");
    assert!(!ptables.overlaps(&stack), "ptables overlaps with stack");

    kprintln!("  [x] Stage 2F-A physical layout verified: .bss, pmm_meta, guard, stack, and page tables strictly disjoint.");
    true
}

/// Verifies that all critical memory ranges are strictly non-overlapping half-open intervals.
pub fn verify_critical_memory_disjointness() -> bool {
    let ranges = get_all_critical_ranges();

    for i in 0..ranges.len() {
        assert!(ranges[i].start < ranges[i].end, "Invalid memory range: start >= end");
        for j in (i + 1)..ranges.len() {
            if ranges[i].overlaps(&ranges[j]) {
                kprintln!("[FATAL] Critical memory range collision detected!");
                kprintln!("  Region A: {} [0x{:016X}, 0x{:016X})", ranges[i].name, ranges[i].start, ranges[i].end);
                kprintln!("  Region B: {} [0x{:016X}, 0x{:016X})", ranges[j].name, ranges[j].start, ranges[j].end);
                return false;
            }
        }
    }
    true
}

// ====================================================================
// GDT & TSS Initialization
// ====================================================================

/// Formally initializes the 64-bit GDT and TSS, reloads segment registers, and loads TR.
pub fn init_gdt() {
    unsafe {
        let kstack = get_kernel_stack_range();
        let ist1_stack = get_ist1_stack_range();

        // 1. Configure TSS
        // RSP0: Kernel stack loaded by hardware during privilege-changing interrupt/exception entry
        TSS.rsp0 = kstack.end;
        // IST1: Dedicated Interrupt Stack for Double Fault (#DF, Vector 8)
        TSS.ist1 = ist1_stack.end;
        TSS.iomap_base = size_of::<TaskStateSegment>() as u16;

        // 2. Build GDT Entries
        // -------------------------------------------------------------
        // Descriptor 0: Null Descriptor
        GDT[0] = 0x0000_0000_0000_0000;

        // Descriptor 1: Kernel Code Segment (Selector 0x08)
        // - Type: 0xA (Execute/Read, non-conforming)
        // - S: 1 (Code/Data), DPL: 0 (Ring 0), P: 1 (Present)
        // - L: 1 (64-bit Long Mode), D: 0 (Must be 0 when L=1), G: 1 (4 KiB granularity)
        GDT[1] = 0x00AF_9A00_0000_FFFF;

        // Descriptor 2: Kernel Data Segment (Selector 0x10)
        // - Type: 0x2 (Read/Write), S: 1, DPL: 0, P: 1, L: 0, D/B: 1, G: 1
        GDT[2] = 0x00CF_9200_0000_FFFF;

        // Descriptor 3: User Data Segment (Selector 0x18 | 3 = 0x1B)
        // - Type: 0x2 (Read/Write), S: 1, DPL: 3 (Ring 3 User Mode), P: 1, L: 0, D/B: 1, G: 1
        GDT[3] = 0x00CF_F200_0000_FFFF;

        // Descriptor 4: User Code Segment (Selector 0x20 | 3 = 0x23)
        // - Type: 0xA (Execute/Read), S: 1, DPL: 3 (Ring 3 User Mode), P: 1, L: 1, D: 0, G: 1
        GDT[4] = 0x00AF_FA00_0000_FFFF;

        // Descriptors 5 & 6: TSS 16-byte System Descriptor (Selector 0x28)
        // - Type: 0x9 (64-bit Available TSS), S: 0 (System Segment), DPL: 0, P: 1
        let tss_ptr = &raw const TSS as u64;
        let tss_limit = (size_of::<TaskStateSegment>() - 1) as u64;

        let tss_low = (tss_limit & 0xFFFF)
            | ((tss_ptr & 0xFFFF) << 16)
            | (((tss_ptr >> 16) & 0xFF) << 32)
            | (0x89 << 40) // Present, DPL 0, System, Type 0x9
            | (((tss_limit >> 16) & 0x0F) << 48)
            | (((tss_ptr >> 24) & 0xFF) << 56);

        let tss_high = (tss_ptr >> 32) & 0xFFFF_FFFF;

        GDT[5] = tss_low;
        GDT[6] = tss_high;

        // 3. Load GDT and Reload Segment Registers via assembly helper
        let gdt_desc = GdtDescriptor {
            limit: (size_of::<[u64; 7]>() - 1) as u16,
            base: &raw const GDT as u64,
        };

        load_gdt_and_tss(&gdt_desc, TSS_SELECTOR);
    }
}

/// Performs structural validation of #DF IST1 and GDT/TSS configuration.
pub fn verify_double_fault_ist_structure() -> bool {
    let ist1_val = unsafe { TSS.ist1 };
    let ist1_range = get_ist1_stack_range();
    let idt_df_ist = super::idt::get_vector_ist(8);

    // 1. Verify TSS.IST1 is non-zero
    assert!(ist1_val != 0, "#DF Structural Failure: TSS.IST1 is null!");

    // 2. Verify IST1 is 16-byte aligned
    assert!((ist1_val % 16) == 0, "#DF Structural Failure: TSS.IST1 is not 16-byte aligned!");

    // 3. Verify IDT[8].ist is configured to 1
    assert_eq!(idt_df_ist, 1, "#DF Structural Failure: IDT Vector 8 IST index != 1!");

    // 4. Verify IST1 range matches allocation
    assert_eq!(ist1_val, ist1_range.end, "#DF Structural Failure: TSS.IST1 does not point to top of IST1 stack!");

    // 5. Verify disjointness against all other critical regions
    let ranges = get_all_critical_ranges();
    for r in &ranges {
        if r.name != ist1_range.name {
            assert!(!ist1_range.overlaps(r), "#DF Structural Failure: IST1 overlaps with {}!", r.name);
        }
    }

    true
}

/// Prints formal GDT, TSS, Segment Register, Stack, and Critical Range Diagnostics.
pub fn print_gdt_diagnostics() {
    let cs = read_cs();
    let ss = read_ss();
    let ds = read_ds();
    let es = read_es();
    let fs = read_fs();
    let gs = read_gs();
    let tr = read_tr();

    let ranges = get_all_critical_ranges();

    kprintln!("\n[Stage 2B: GDT, TSS & Privilege Boundary Diagnostics]");
    kprintln!("  GDT Table Base:     0x{:016X} (7 entries, 56 bytes)", unsafe { &raw const GDT as u64 });
    kprintln!("  TSS Structure Base: 0x{:016X} (size {} bytes)", unsafe { &raw const TSS as u64 }, size_of::<TaskStateSegment>());
    kprintln!("  Task Register (TR): 0x{:04X} (Active TSS Selector: 0x{:04X})", tr, TSS_SELECTOR);
    kprintln!("  Segment Registers:  CS=0x{:04X} SS=0x{:04X} DS=0x{:04X} ES=0x{:04X} FS=0x{:04X} GS=0x{:04X}",
        cs, ss, ds, es, fs, gs);
    kprintln!("  Privilege Level:    Ring {} (DPL 0)", cs & 0x03);

    kprintln!("\n[Explicit Critical-Memory Range Audit (Half-Open Intervals [start, end))]");
    for r in &ranges {
        let align16 = (r.start % 16 == 0) && (r.end % 16 == 0);
        kprintln!("  * {:<28} [0x{:016X}, 0x{:016X}) ({} bytes, 16B aligned: {})",
            r.name, r.start, r.end, r.size(), align16);
    }

    let disjoint_ok = verify_critical_memory_disjointness();
    assert!(disjoint_ok, "Critical memory ranges overlap!");
    kprintln!("  [x] All 7 critical memory regions verified strictly non-overlapping (disjoint).");

    kprintln!("\n[Double-Fault (#DF) Structural Verification]");
    kprintln!("  TSS.RSP0 Stack Top: 0x{:016X} (Kernel stack loaded on Ring 3 interrupt entry)", unsafe { TSS.rsp0 });
    kprintln!("  TSS.IST1 Stack Top: 0x{:016X} (Dedicated stack for #DF abort handling)", unsafe { TSS.ist1 });
    kprintln!("  IDT Vector 8 IST:   IST{} (Configured: {})", super::idt::get_vector_ist(8), super::idt::get_vector_ist(8) == 1);

    let df_struct_ok = verify_double_fault_ist_structure();
    assert!(df_struct_ok, "#DF structural verification failed!");
    kprintln!("  [x] #DF gate structurally bound to independent 16-byte aligned IST1 stack.");
    kprintln!("  [x] IST1 isolation confirmed: zero overlap with normal stack, GDT, TSS, IDT, or page tables.");
}
