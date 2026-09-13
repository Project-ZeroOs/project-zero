# ADR-0004: Stage 1 Boot Architecture & Memory Model Review

## Status
**Accepted**

## Date
2026-09-13

## Context & Engineering Review

Following the initial architectural survey, an in-depth engineering review was conducted to evaluate the bootloader strategy for **Stage 1 (Minimal Bootable Prototype)**. 

The primary objective of Stage 1 is:
> **A minimal, reliable, debuggable bootable kernel**, not maximum architectural sophistication.

We must avoid premature complexity, fragile toolchain dependencies, and opaque failure modes. When a kernel fails to boot on bare metal or in a virtual machine, having transparent, observable execution points is critical.

---

## 1. Comparison of Candidate Boot Strategies

We evaluate three practical boot mechanisms for bringing up a 64-bit x86-64 research operating system under QEMU:

| Evaluation Criteria | 1. Multiboot2 + Custom Trampoline + GRUB2 ISO | 2. Modern Rust Bootloader (`bootloader` crate / Limine) | 3. QEMU Direct Multiboot1 ELF + Early Serial Trampoline |
| :--- | :--- | :--- | :--- |
| **Stage 1 Complexity** | **High**: Requires complex 32-bit tag parsing, multiboot2 header compliance, and external ISO creation tools (`grub-mkrescue`, `xorriso`, `mtools`). | **High**: Brittle coupling between compiler versions; version churn (`bootloader` 0.9 vs 0.11); complex disk image packaging on Windows host. | **Minimal**: Native QEMU loader directly maps ELF sections to physical memory; zero ISO/disk image dependencies. |
| **Reliability** | **Moderate**: Brittle ISO generation pipeline on Windows; multi-step handoff through GRUB. | **Moderate**: Bootloader crate internal updates frequently break custom kernel builds. | **Exceptional**: Deterministic; zero moving parts outside QEMU and the ELF binary. |
| **Debugging & Failure Observability** | **Poor**: Silent triple faults if Multiboot2 tags or early paging structures are misaligned; no serial feedback until late kernel. | **Moderate**: Diagnostics depend on bootloader crate panic handlers; early failures can hang silently. | **Unmatched**: Assembly trampoline directly writes diagnostic progress codes (`'B'`, `'P'`, `'L'`, `'K'`) to COM1 UART before every state transition. |
| **Rust Integration** | **Awkward**: Requires compiling 32-bit assembly objects and linking with 64-bit Rust objects or two-stage builds. | **High**: Native cargo dependency, but forces cargo to manage the boot disk layout. | **Direct**: Minimal NASM/GAS assembly trampoline produces an ELF object linked directly with the `no_std` Rust static library into a single ELF kernel. |
| **Control Over Memory Layout** | **High**: Dictated by linker script. | **Low**: Abstracted by bootloader crate; memory map constructed opaquely. | **Complete**: Linker script (`linker.ld`) specifies exact physical and virtual load addresses. |
| **Long-Term Portability (ARM64)** | **Moderate**: Multiboot2 is strictly x86-specific. | **Low**: `bootloader` crate is x86-64 exclusive. | **Exceptional**: QEMU `-kernel` ELF/flat binary boot operates identically on ARM64 (`qemu-system-aarch64 -M virt -kernel`). |

---

## 2. Analysis & Trade-Offs

### 2.1 Why Multiboot2 + GRUB2 ISO is Rejected for Stage 1
While Multiboot2 is a recognized standard, utilizing GRUB2 ISO generation introduces a heavy, fragile host dependency chain (`xorriso`, `grub-mkrescue`, `mtools`) that is notoriously difficult to maintain portably across Windows, Linux, and macOS host environments. Furthermore, Multiboot2 requires parsing variable-length tag arrays before memory layout is established, adding unnecessary code prior to basic serial validation.

### 2.2 Why the `bootloader` Crate is Rejected
The Rust `bootloader` crate abstracts away the boot process, but at the cost of control and stability. In a first-principles research OS, understanding and controlling the CPU control registers (CR0, CR3, CR4), descriptor tables (GDT, IDT), and early paging structures is a foundational learning and architectural requirement. Relying on an opaque third-party crate obscures the very hardware mechanisms we must master.

### 2.3 Why QEMU Direct Multiboot1 ELF is Selected
QEMU contains a built-in, native Multiboot1 loader invoked directly via `qemu-system-x86_64 -kernel <kernel.elf>`.
1. **Zero ISO Packaging**: The build pipeline compiles the assembly trampoline and Rust `no_std` code, links them into `kernel.elf`, and launches QEMU directly.
2. **Deterministic Start**: QEMU loads the ELF headers into physical RAM at `0x100000` (1 MiB), enables the A20 line, sets the CPU into 32-bit protected mode with flat segments, places the Multiboot magic (`0x2BADB002`) into `EAX`, and jumps directly to `_start`.
3. **Step-by-Step Serial Telemetry**: Because we control the entry trampoline, we can write diagnostic bytes directly to `0x3F8` (COM1) before and after each CPU mode transition:
   * `'B'`: Multiboot entry reached in 32-bit mode.
   * `'P'`: Early page tables configured.
   * `'L'`: Long mode enabled and 64-bit far jump executed.
   * `'K'`: Stack established and Rust `kernel_main` invoked.
   If any transition faults, the last printed byte immediately identifies the failing hardware step.

---

## 3. Memory Mapping Strategy for Stage 1

Following the review requirement:
> *"Do NOT implement a higher-half kernel unless there is a compelling technical requirement for it at this stage. Prefer a simple physical/identity mapping initially."*

### Stage 1 Identity Mapping
For Stage 1, we implement a **direct 2 MiB huge-page physical identity mapping**:
* A single Page Map Level 4 (PML4) entry.
* A single Page Directory Pointer Table (PDPT) entry.
* A Page Directory (PD) mapping the first 1 GiB (or 512 $\times$ 2 MiB = 1 GiB) with identity addresses:
  $$\text{Virtual Address } X \equiv \text{Physical Address } X$$
* The kernel ELF is linked at `0x100000` (1 MiB physical).
* This eliminates the complex double-mapping (trampoline + higher-half window) during early bootstrap while providing a reliable 64-bit environment.

### Evolutionary Path to Full Virtual Memory (Stage 2+)
The architecture isolates the early page tables into a dedicated HAL structure (`boot/boot.S` and `hal/arch/x86_64/mmu.rs`). In Stage 2, once the memory allocator and serial logging are proven:
1. The kernel will allocate a dynamic PML4.
2. The physical memory map provided by Multiboot will be traversed.
3. The higher-half mapping (`0xFFFFFFFF80000000` or direct physical map at `0xFFFF800000000000`) will be installed.
4. Process address spaces and capability-based memory objects (`VmoCap`) will be introduced without rewriting the kernel core.

---

## 4. CPU Initialization Scope for Stage 1

In adherence to Section 5:
> *"Implement only what Stage 1 genuinely requires. Do not create fake GDT/IDT functionality."*

Stage 1 establishes the exact minimum CPU state required for safe 64-bit execution:
1. **Privilege Level**: Ring 0 (Supervisor).
2. **Segmentation**: A minimal 64-bit Global Descriptor Table (GDT) containing:
   * Null Descriptor (`0x00`).
   * 64-bit Kernel Code Segment (`CS = 0x08`, executable, readable, Ring 0, Long Mode `L=1`, `D=0`).
   * 64-bit Kernel Data Segment (`DS = 0x10`, writable, Ring 0).
3. **Paging**: 4-level paging enabled (`CR4.PAE = 1`, `IA32_EFER.LME = 1`, `CR0.PG = 1`).
4. **Stack**: A known-good 64 KiB stack allocated in the `.bss` section with 16-byte alignment complying with the System V AMD64 ABI.
5. **Interrupt State**: Hardware interrupts disabled (`cli`). No IDT is loaded during Stage 1; interrupts will be introduced in Stage 2 with proper exception handlers and APIC routing.
