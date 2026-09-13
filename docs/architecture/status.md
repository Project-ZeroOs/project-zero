# Project Zero — Architectural Status

**Current Milestone**: Stage 2 — Kernel Memory & Protection Foundation (COMPLETE)  
**Next Milestone**: Stage 3 — Preemptive Multitasking, APIC, & System Services (NOT STARTED)

---

## Milestone Status Matrix

| Milestone | Subsystem / Focus | Status | Verification Summary |
|---|---|---|---|
| **Stage 1** | Multiboot & 64-bit Long Mode Bring-up | **COMPLETE** | ELF loaded at 1 MiB physical, PAE enabled, 64-bit far jump executed, serial console functional. |
| **Stage 2A** | Core CPU & HAL Infrastructure | **COMPLETE** | CR registers, MSRs, serial COM1 115200 8N1 driver, and CPU diagnostics. |
| **Stage 2B** | GDT, TSS & Privilege Model | **COMPLETE** | 64-bit GDT, Ring 0/Ring 3 selectors, TSS loaded with independent 16-byte aligned IST1 double-fault stack. |
| **Stage 2C** | IDT & Exception Handling | **COMPLETE** | 256 vector gates, assembly ISR dispatchers (`isr.asm`), graceful recovery from Breakpoint (#BP) and Invalid Opcode (#UD). |
| **Stage 2D** | PMM & Physical Inventory | **COMPLETE** | Multiboot memory map parsed, reserved regions protected, bitmap-tracked 4 KiB frame allocator operational. |
| **Stage 2E** | Virtual Memory Management | **COMPLETE** | 4-level paging engine (`map_page`/`unmap_page`), dynamic table allocation, atomic rollback, and table reclamation. |
| **Stage 2F** | Higher-Half Kernel & Protection | **COMPLETE** | Higher-half VMA (`PML4[511]`), 4 GiB HHDM (`PML4[256]`), low identity map removed (`PML4[0]` absent), 4 KiB permission splitting ($W \oplus X$), `CR0.WP = 1`, and controlled `#PF` hardware verification. |
| **Stage 3** | Preemption, Timers & Scheduling | **NOT STARTED** | Architecture definition phase. No scheduler changes, APIC setup, or user-mode processes yet implemented. |

---

## Current Subsystem Invariants

* **Address Space Layout**:
  - `0x0000000000000000 .. 0x00007FFFFFFFFFFF`: User Space (Unmapped in Stage 2)
  - `0xFFFF800000000000 .. 0xFFFF8000FFFFFFFF`: Higher-Half Direct Map (HHDM, `[0, 4 GiB)`, `RW + NX`)
  - `0xFFFFFFFF80000000 .. 0xFFFFFFFF80200000`: Kernel VMA (2 MiB bootstrap window, 4 KiB granular permissions)
* **Active Hardware Protections**:
  - `CR0.WP = 1` (Ring 0 write protect enforced on supervisor reads/writes)
  - `IA32_EFER.NXE = 1` (Execute-Disable bit enforced across non-code pages)
* **Kernel Permissions**:
  - `.multiboot_header`: R + NX
  - `.text`: RX
  - `.rodata`: R + NX
  - `.data`, `.bss`, `.pmm_metadata`, `.page_tables`, `.stack`: RW + NX
  - `.stack_guard`: NOT PRESENT (unmapped page)
