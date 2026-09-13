# Changelog

All notable architectural milestones of Project Zero are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased]

### Planned
- Stage 3: Preemptive Scheduling, APIC/x2APIC, and Core Subsystems Architecture.

---

## [Stage 2F] - 2026-09-13

### Completed: Higher-Half Kernel Architecture & Memory Protection
- **Stage 2F-A**: Higher-half ELF linker transformation (`VMA = LMA + 0xFFFFFFFF80000000`), disjoint section layout, and dedicated `.stack` / `.stack_guard` sections outside `.bss`.
- **Stage 2F-B**: Dual bootstrap page tables (`PML4[0]` identity and `PML4[511]` higher-half VMA) loaded into CR3.
- **Stage 2F-C**: Higher-half execution switch (`_start_higher_half`), 64 KiB higher-half stack transition, and GDT/TSS/IDT descriptor updates to higher-half VMA pointers.
- **Stage 2F-D**: Higher-Half Direct Map (HHDM) established at base `0xFFFF_8000_0000_0000` mapping `[0, 4 GiB)` physical memory via 2 MiB huge pages (`PML4[256]`). Strongly typed `PhysicalAddress`, `VirtualAddress`, and `HhdmAddress` abstractions.
- **Stage 2F-E**: Identity mapping (`PML4[0]`) permanently removed from active address space. Verified via full TLB shootdown (CR3 reload) and deliberate access to physical `0x00100000` trapping `#PF` vector 14 with exact CR2 match.
- **Stage 2F-F**: Fine-grained 4 KiB Kernel VMA permission splitting and $W \oplus X$ enforcement:
  - `.multiboot_header`: R + NX
  - `.text`: RX
  - `.rodata`: R + NX
  - `.data`, `.bss`, `.pmm_metadata`, `.page_tables`, `.stack`: RW + NX
  - `.stack_guard`: NOT PRESENT (unmapped)
  - Hardware protections: `CR0.WP = 1` (Ring 0 write protect) and `IA32_EFER.NXE = 1` (No-Execute) verified active.
  - Four controlled hardware exception traps verified against exact CPU frames: `.rodata` write (#PF error 0x03), `.text` write (#PF error 0x03), `.data` execute (#PF error 0x11 NX violation), and stack guard access (#PF error 0x00).
  - HHDM security model verified as privileged supervisor physical aperture (write to `.text` physical frame succeeds via HHDM and restored).

---

## [Stage 2E] - 2026-09-13

### Completed: Virtual Memory Management Architecture
- **Stage 2E-A**: 4-level paging architecture foundations, hardware geometry discovery, canonical 48-bit addressing, and active CR3 inspection.
- **Stage 2E-B**: 4 KiB virtual memory mapping engine (`map_page`, `unmap_page`), dynamic intermediate table allocation, atomic allocation-failure rollback, and empty-table reclamation.
- **Stage 2E-C**: Integrated end-to-end VMM lifecycle testing and deterministic verification under QEMU.

---

## [Stage 2] - 2026-09-13

### Completed: Kernel Nucleus & Hardware Abstraction
- 64-bit GDT with Ring 0/Ring 3 descriptors and TSS with dedicated 16-byte aligned IST1 double-fault stack.
- IDT with 256 vector gates and low-level assembly exception dispatchers (`isr.asm`).
- 8259 PIC remapping and 8254 PIT timer at 100 Hz.
- Physical Memory Manager (PMM) with Multiboot memory map discovery and bitmap-tracked 4 KiB frame allocator.
- Kernel Heap Allocator with block coalescing implementing Rust's `GlobalAlloc`.
- Cooperative thread execution and callee-preserved context switching (`context.asm`).
- Synchronous direct rendezvous IPC messaging.
- Hardware cycle microbenchmarking via x86 RDTSC.

---

## [Stage 1] - 2026-09-13

### Completed: Multiboot & 64-bit Long Mode Bring-up
- Multiboot 1 header specification and verification.
- Protected mode (32-bit) to Long Mode (64-bit) bootstrap transition in assembly (`boot.asm`).
- Early 2 MiB identity paging and PAE activation.
- Serial console 16550 UART driver output (`kprint!`, `kprintln!`).
- Headless QEMU build and test automation (`tools/run_qemu.py`).
