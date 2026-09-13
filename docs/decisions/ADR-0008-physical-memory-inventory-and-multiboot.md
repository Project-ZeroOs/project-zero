# ADR-0008: Multiboot Memory-Map Discovery & Physical Memory Inventory

## Status
Accepted (Stage 2C Baseline)

## Context
Project Zero requires an authoritative, hardware-verified model of physical memory before initializing physical frame allocation, kernel heaps, or paging redesigns. 

Firmware memory discovery is subject to subtle platform bugs and bootloader conventions:
1. **Multiboot 1 Boot Contract**: Bootloaders complying with Multiboot 1 enter 32-bit protected mode with `EAX = 0x2BADB002` (Multiboot magic) and `EBX = physical address of multiboot_info`.
2. **Variable Entry Sizing**: The Multiboot 1 memory map (`mmap_addr`, `mmap_length`) consists of variable-sized records where `entry.size` designates the length of the data structure *following* the 4-byte size field itself (minimum 20 bytes). Assuming fixed-size structs or hardcoding 24 bytes risks parsing errors across diverse bootloaders (GRUB, QEMU `-kernel`, etc.).
3. **64-bit Physical Addresses**: Modern x86-64 platforms and virtualized environments (including QEMU default configs) expose memory regions located above 4 GiB (e.g. PCI MMIO and firmware holes at `0x000000FD00000000` to `0x0000010000000000`). Truncating addresses or lengths to 32 bits corrupts memory accounting and risks catastrophic silent overwrites.
4. **Firmware Available RAM vs. Immediately Allocatable RAM**: A Multiboot memory-map entry of type 1 (`MULTIBOOT_MEMORY_AVAILABLE`) indicates firmware-reported physical RAM. It **does not** imply the operating system can immediately allocate those bytes. The OS image itself, its stack, page tables, IDT, GDT, TSS, and the bootloader's own metadata (`multiboot_info` and the memory-map buffer) reside inside those physical spans.
5. **Low-Memory Preservation**: The bottom 1 MiB (`[0x0, 0x100000)`) contains the real-mode Interrupt Vector Table (IVT), BIOS Data Area (BDA), Extended BIOS Data Area (EBDA), VGA framebuffers, and BIOS/Option ROMs, as well as critical trampolines for future SMP multicore bringup.

## Decision

### 1. Multiboot 1 ABI Contract and Internal Calling Convention
We formally distinguish between the Multiboot 1 specification contract and Project Zero's internal calling convention:
* **Multiboot 1 Contract**:
  - `EAX` = `0x2BADB002` (Magic constant)
  - `EBX` = Physical address of `multiboot_info`
* **Internal Project Zero Entry Convention**:
  - In `boot/boot.asm`, before clobbering 32-bit registers during long-mode page-table setup, `EAX` and `EBX` are saved into assembly storage variables (`multiboot_magic_raw`, `multiboot_info_raw`).
  - Upon transitioning into 64-bit Long Mode, `boot.asm` places the physical address into `RDI` (first argument in System V AMD64 ABI) and the magic into `RSI` (second argument) when invoking `kernel_main(boot_info_addr: u64, multiboot_magic: u64)`.
  - `kernel_main` validates `multiboot_magic == 0x2BADB002` immediately upon initialization before reading any data structures.

### 2. Variable-Sized Entry Parsing & 64-Bit Arithmetic
The parser (`kernel/src/mm/inventory.rs:MultibootMmapParser`) enforces:
* **Strict Stepping**: The buffer pointer advances by `entry.size + 4` bytes.
* **Malformed Rejection**: Any entry declaring `entry.size < 20` or exceeding buffer bounds terminates memory map parsing without corrupting memory or looping indefinitely.
* **64-bit Width**: Base addresses and lengths are parsed as `u64`.
* **Overflow Protection**: `base + length` arithmetic is checked using checked addition (`checked_add`). Entries that wrap around $2^{64}-1$ are discarded.
* **Zero-Length Elimination**: Regions with length 0 are ignored.
* **Region Classification**:
  - Type 1: `MemoryRegionType::AvailableRAM`
  - Type 2: `MemoryRegionType::Reserved`
  - Type 3: `MemoryRegionType::AcpiReclaimable`
  - Type 4: `MemoryRegionType::AcpiNvs`
  - Type 5: `MemoryRegionType::BadRam`
  - Others: `MemoryRegionType::Other(type_code)`

### 3. Explicit Critical-Memory Reservation Subtraction & Rigorous Accounting
Project Zero maintains a reservation list containing all active kernel and boot structures:
1. `Low Memory (IVT/BDA/ROM)`: `[0x0000000000000000, 0x0000000000100000)` (1 MiB)
2. `Kernel Image (Text/Data)`: `[__executable_start, __bss_end)`
3. `Normal Kernel Stack`: `[stack_bottom, stack_top)`
4. `Early Page Tables`: `[boot_p4, boot_p1_end)`
5. `IDT Table (256 Descriptors)`: `[&IDT_TABLE, &IDT_TABLE + sizeof(IdtTable))`
6. `IST1 Double-Fault Stack`: `[DOUBLE_FAULT_STACK_BOTTOM, DOUBLE_FAULT_STACK_TOP)`
7. `GDT Table`: `[&GDT_TABLE, &GDT_TABLE + sizeof(GdtTable))`
8. `TSS Structure`: `[&TSS_STRUCTURE, &TSS_STRUCTURE + sizeof(TaskStateSegment))`
9. `Multiboot Info Structure`: `[mbi_addr, mbi_addr + sizeof(MultibootInfo))`
10. `Multiboot Memory-Map Storage`: `[mmap_addr, mmap_addr + mmap_length)`
11. `Multiboot Modules`: `[mod_start, mod_end)` (if present)

#### Accounting Invariant & No Double-Counting
Because individual reservation objects may overlap (specifically, the low-memory reservation `[0, 0x100000)` encompasses the Multiboot Info structure and the memory-map storage buffer), Project Zero explicitly avoids simply summing individual reservation sizes.

Instead, the inventory engine computes:
1. $\text{Unique Reservation Union} = \bigcup_{i} \text{Reservation}_i$
2. $\text{Reservation Intersection With Type-1} = \text{Firmware Type-1 RAM} \cap \text{Unique Reservation Union}$
3. $\text{Final Discoverable Byte-Level RAM} = \text{Firmware Type-1 RAM} \setminus \text{Reservation Intersection With Type-1}$

We verify at runtime that:
$$\text{Firmware Type-1 RAM} - \text{Reservation Intersection} = \text{Final Discoverable Byte-Level RAM}$$

#### Derived 4 KiB Page-Aligned Frame Candidates
The discoverable inventory intervals are byte-level intervals. To provide an unambiguous, clean input for Stage 2D PMM without implementing allocation or bitmaps:
* `frame_start = align_up(usable_start, 4096) = (usable_start + 4095) & !4095`
* `frame_end   = align_down(usable_end, 4096) = usable_end & !4095`
* Only intervals where `frame_start < frame_end` produce frame candidates.
* Sub-page fragments and boundary slivers are classified as `subpage_remainder_bytes` rather than silently treated as frames.

### 4. Physical Address Space Terminology
We eliminate imprecise references to non-RAM regions as physical RAM. The inventory explicitly distinguishes:
* `Total Physical Address Space Described`: Complete span of all firmware Multiboot memory-map entries (e.g. ~12.4 GiB in QEMU, including the 12 GiB PCI hole).
* `Type-1 Available RAM`: Actual usable physical system memory reported by firmware (127 MiB).
* `Reserved Address Space`: Memory reserved by firmware/motherboard/MMIO (12.2 GiB).
* `ACPI Reclaimable / ACPI NVS / Bad RAM / Other`: Explicitly tracked category fields.

### 5. Architectural Boundary
Stage 2C is strictly a **discovery, read-only physical memory inventory, and accounting verification** phase. 
* No physical frames are marked as allocated.
* No bitmap or buddy allocator is initialized.
* The inventory publishes immutable statistics and clean page-aligned candidate ranges ready for Stage 2D.

## Consequences

### Positive
* **Zero-Collision Guarantee**: The future Stage 2D physical frame allocator will never touch kernel code, page tables, stacks, exception tables, or bootloader data.
* **Deterministic Telemetry**: Boot logs clearly distinguish firmware-reported available RAM (127 MiB in default QEMU) from Project Zero usable RAM (126 MiB after reserving kernel, stacks, tables, and low memory).
* **Robust Hardware Support**: Safely parses entries > 4 GiB and ignores malformed/corrupted BIOS structures.
* **Tested Regressions**: Automated Python test suite validates both synthetic memory map edge cases (zero length, variable sizes, 64-bit overflow, unsorted intervals) and live QEMU boot execution.

### Negative / Trade-offs
* **Static Bounds Capacity**: Fixed-size arrays (`MAX_REGIONS = 32`, `MAX_RESERVATIONS = 16`, `MAX_USABLE_RANGES = 32`) are used to avoid requiring early heap allocation. 32 entries exceed typical PC memory maps (typically 6 to 12 entries).
* **Fragmentation into Small Slices**: Padding gaps between stacks and page tables generate small sub-page clean usable ranges (e.g. 1 KiB, 3 KiB). The Stage 2D frame allocator will filter or align these to 4 KiB boundaries when creating page frame pools.
