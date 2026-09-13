# Project Zero — Stage 1 Technical Audit Report

## 1. Executive Summary

Prior to beginning Stage 2, a complete technical audit of the Stage 1 implementation was conducted. The audit verified:
1. Ground-truth binary layout of the compiled kernel via `readelf` and `objdump`.
2. Hardware register invariants (CR0, CR3, CR4, EFER, CS, RFLAGS).
3. Physical memory layout, stack safety, and collision risks.
4. Multiboot 1 bootloader handoff and register retention.
5. QEMU test harness assertions and negative test behavior.
6. Rust `no_std` runtime dependencies.

---

## 2. CPU & Boot Protocol Verification

### 2.1 Multiboot 1 Header Analysis
Inspection of `build/kernel32.elf` confirms:
* **Magic Number**: `0x1BADB002` located at physical address `0x00100000` (file offset `0x1000`).
* **Header Flags**: `0x00000003` (Bit 0: 4 KiB align modules; Bit 1: Provide memory map).
* **Checksum**: `-(0x1BADB002 + 0x00000003) = 0xE4524FFB` ($ \text{Sum} = 0 $).
* **Placement Invariant**: Located within the first 4,096 bytes of the file, satisfying the Multiboot specification ($< 8192$ bytes).
* **Bootloader Handoff State**:
  * QEMU loads the ELF segments at `0x100000` physical.
  * Entry point: `_start` at `0x00101000`.
  * CPU state on entry: 32-bit Protected Mode, Paging Disabled, A20 Enabled, `EAX = 0x2BADB002`, `EBX = Multiboot Info Physical Address`.

> [!NOTE]
> **Audit Finding (EBX Preservation)**: In Stage 1, `boot.asm` did not preserve the `EBX` register containing the pointer to the Multiboot Information structure across the 64-bit far jump. For Stage 2 (Physical Memory Manager), `boot.asm` must save `ebx` (e.g. move to `edi`/`rdi`) so that `kernel_main(multiboot_info: u64)` can parse the BIOS memory map to discover usable RAM frames.

### 2.2 CPU Execution Modes & EFER State
The audit inspected the distinct phases of 64-bit initialization:
1. **PAE (Physical Address Extension)**: Activated via `CR4.PAE` (Bit 5). Verified: `CR4 = 0x0000000000000020`.
2. **Long Mode Enabled (LME)**: Activated via `IA32_EFER.LME` (Bit 8 of MSR `0xC0000080`).
3. **Paging Enabled (PG)**: Activated via `CR0.PG` (Bit 31) and `CR0.PE` (Bit 0). Verified: `CR0 = 0x0000000080000011`.
4. **Long Mode Active (LMA)**: Set automatically by hardware when paging is enabled with LME active (`IA32_EFER.LMA`, Bit 10).
5. **64-Bit Sub-mode Execution**: Dictated by `CS.L = 1` and `CS.D = 0` in the GDT descriptor. Selector `0x08` points to descriptor `0x00AF9A000000FFFF` where Bit 53 ($L$) is 1 and Bit 54 ($D$) is 0. Verified: `CS = 0x0008`, executing true 64-bit instructions in Ring 0.

---

## 3. Memory Mapping & Layout Audit

### 3.1 Ground-Truth Memory Map
From `readelf -S` and `readelf -s`:

| Memory Range | Size | Section | Permissions | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| `0x00000000` – `0x000FFFFF` | 1 MiB | Real-Mode Area | R/W | IVT, BDA, VGA Buffer (`0xB8000`), SeaBIOS ROM |
| `0x00100000` – `0x0010000C` | 12 B | `.multiboot_header` | Read-Only | Multiboot header magic & flags |
| `0x00101000` – `0x00124FA2` | 143.9 KiB | `.text` | Read / Execute | Compiled kernel machine code (`_start`, `kernel_main`) |
| `0x00125000` – `0x0012E8A0` | 38.2 KiB | `.rodata` | Read-Only | Static strings, GDT descriptor tables |
| `0x0012E8C8` – `0x0012EC70` | 936 B | `.got` | Read / Write | Global Offset Table |
| `0x0012F000` – `0x001303C0` | 5.0 KiB | `.data` | Read / Write | Initialized global state |
| `0x00131000` – `0x00141000` | 64.0 KiB | `.bss` (Stack) | Read / Write | 16-byte aligned kernel execution stack |
| `0x00141000` – `0x00142000` | 4.0 KiB | `.bss` (PML4) | Read / Write | Early Page Map Level 4 table |
| `0x00142000` – `0x00143000` | 4.0 KiB | `.bss` (PDPT) | Read / Write | Early Page Directory Pointer Table |
| `0x00143000` – `0x00144000` | 4.0 KiB | `.bss` (PD) | Read / Write | Early Page Directory table (512 $\times$ 2 MiB entries) |

* **Total Kernel Footprint**: `0x00100000` to `0x00144000` ($272\text{ KiB}$).
* **Active Identity Mapping Coverage**: `0x00000000` to `0x3FFFFFFF` (0 to 1 GiB).
* **Stack Collision Assessment**: The stack base is at `0x00141000` and grows downward toward `0x00131000`. The page tables are positioned immediately above `0x00141000` (`0x00141000` to `0x00144000`). Because stack pushes decrement `%rsp`, stack usage moves away from the page tables. However, to eliminate any potential collision during stack overflows, Stage 2 will introduce an unmapped guard page below `stack_bottom`.

---

## 4. Bootloader & ELF Audit

* **Binary Format**: `kernel32.elf` is an ELF32 executable container wrapping 64-bit code segments. This allows QEMU's `-kernel` Multiboot loader to parse the program headers while permitting the 32-to-64 bit bootstrap trampoline to transfer control to 64-bit Long Mode.
* **Linker Behavior**: `rust-lld -flavor gnu` correctly links the static library `libkernel.a` with `boot.o` adhering to `linker.ld` alignments.
* **Conversion Tool**: GNU `objcopy -O elf32-i386` transforms the 64-bit ELF container into a 32-bit container without altering the underlying machine instruction bytes.

---

## 5. QEMU Test Harness Verification

* **Signal Capture**: Verified that `run_qemu.py` validates `BPLK` hardware telemetry markers, the full banner text, Ring 0 privilege level, CR0/CR4 bits, and disabled interrupts.
* **Exit Encoding**: Verified that `outb(0xF4, 0x10)` to `isa-debug-exit` generates QEMU process exit code:
  $$\text{Exit Code} = (0x10 \ll 1) \mid 1 = 33$$
* **Negative Test Implementation**: A negative test harness was developed to assert that if the kernel outputs an invalid banner, fails during bootstrap, or panics, the test runner detects the anomaly, rejects the run, and terminates with exit code 1.

---

## 6. Dependency & Compiler Audit

* **`no_std` Compliance**: Verified zero references to standard library symbols (`std::*`).
* **Runtime Zero-Cost**: No dynamic memory allocator or garbage collector is linked.
* **Panic Invariant**: `panic = "abort"` in `Cargo.toml`; panic handler outputs diagnostic file/line and error messages directly to COM1 serial without memory allocation.
