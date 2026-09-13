# ADR-0003: Initial Target Platform & Smallest Bootable Milestone

## Status
**Accepted**

## Date
2026-09-13

## Context & Hardware Strategy

Project Zero's long-term vision encompasses smartphones, laptops, workstations, and future edge devices. These hardware targets utilize heterogeneous architectures:
* **Smartphones**: ARM64 (e.g., Qualcomm Snapdragon, Apple Silicon, MediaTek).
* **Laptops & Desktops**: x86-64 (Intel, AMD) and ARM64 (Apple Silicon, Snapdragon X Elite).
* **Future Systems**: RISC-V (RV64GC).

However, attempting to bring up a bare-metal kernel on physical mobile hardware or raw motherboard silicon at day one introduces immense friction:
* Closed bootloaders, proprietary firmware blobs, lack of serial debugging pins.
* Risk of bricking hardware during early MMU and interrupt development.
* Inability to run fast, automated unit and integration tests in continuous integration environments.

---

## 1. Initial Target Platform Selection: x86-64 + QEMU

We select **x86-64 running inside QEMU (`qemu-system-x86_64`)** as the initial research and execution platform.

### Justification
1. **Unrivaled Debuggability**:
   * QEMU provides built-in GDB remote debugging (`-s -S`), instruction tracing (`-d int,cpu_reset`), memory dumps, and register inspection without physical JTAG hardware.
2. **Serial Console Diagnostics**:
   * The standard 16550 UART serial controller (`COM1` at I/O port `0x3F8`) allows immediate, zero-dependency text output before video drivers, framebuffers, or PCI buses are initialized.
3. **Reproducibility & Automation**:
   * Builds and boots can be executed headlessly inside automated CI pipelines with deterministic test exit codes via QEMU's `isa-debug-exit` device.
4. **Architectural Parity**:
   * Modern x86-64 4-level paging (PML4) and APIC interrupt architecture share direct conceptual equivalence with ARM64 Stage 1 MMU (TTBR0/1) and GICv3. Writing clean HAL traits on x86-64 establishes the exact interfaces needed for the subsequent ARM64 port.

---

## 2. Boot Protocol Specification: Multiboot2 / Limine Bare-Metal ELF

We select **Multiboot2 / Limine compliant 64-bit ELF loading**:
* **Mechanism**: The bootloader initializes protected mode/long mode, sets up early identity paging, provides a structured memory map, and jumps to the 64-bit kernel entry point.
* **Benefits**: Avoids wasting weeks writing fragile 16-bit real-mode BIOS assembly and A20 gate manipulation, allowing engineering effort to focus directly on 64-bit kernel primitives, memory paging, capability structures, and IPC.

---

## 3. The Smallest Bootable Milestone (Stage 1)

To validate the entire engineering pipeline (toolchain, compilation, linker scripts, bootloader handoff, CPU state, and serial diagnostics), we define the **Smallest Bootable Milestone**:

```text
+-------------------------------------------------------------------------+
|                  STAGE 1 EXECUTION SEQUENCE IN QEMU                     |
|                                                                         |
|  [QEMU Machine Boot]                                                    |
|         │                                                               |
|         ▼                                                               |
|  [Bootloader loads 64-bit ELF Kernel into physical RAM]                 |
|         │                                                               |
|         ▼                                                               |
|  [Transfer Control to _start Entry Point]                               |
|         │                                                               |
|         ▼                                                               |
|  [Initialize CPU Early State (GDT, IDT Stubs, Stack Pointer)]           |
|         │                                                               |
|         ▼                                                               |
|  [Initialize Early 16550 UART Serial Controller (COM1, 115200 Baud)]    |
|         │                                                               |
|         ▼                                                               |
|  [Query & Parse Physical Memory Map from Boot Info]                     |
|         │                                                               |
|         ▼                                                               |
|  [Emit Milestone Banner to Serial Console]:                             |
|                                                                         |
|  ============================================================           |
|  PROJECT ZERO                                                           |
|  Kernel initialized.                                                    |
|  ============================================================           |
|         │                                                               |
|         ▼                                                               |
|  [Trigger Automated Test Shutdown via QEMU isa-debug-exit]              |
+-------------------------------------------------------------------------+
```

### 3.1 Verification Criteria for Stage 1
1. **Clean Compilation**: Kernel compiles with zero warnings under bare-metal `no_std` toolchain.
2. **Linker Validation**: Linker script produces a valid 64-bit higher-half ELF binary with proper section alignments (`.text`, `.rodata`, `.data`, `.bss`).
3. **QEMU Automated Execution**: Running the automated build/run script launches QEMU, captures serial output, asserts the presence of the milestone banner, and terminates with exit code 0.
4. **Reproducible Test Script**: A one-line command (`cargo test` / test harness) runs the entire pipeline end-to-end.
