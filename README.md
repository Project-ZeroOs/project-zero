# Project Zero

> **An Experimental Operating System for Personal Computing Environments**  
> *"Computing organized around the user and their intent rather than individual applications and physical machines."*

Project Zero is an experimental operating system built around the idea that computing should be organized around the user and their intent rather than around individual applications and physical machines. 

The long-term goal of Project Zero is a unified personal computing environment capable of spanning multiple devices—orchestrating compute, storage, and sensory capabilities across a user's personal fabric. *This long-term vision represents an architectural research horizon; current development is strictly focused on low-level systems foundations.*

---

## 🚦 Current Status

* **Status**: Experimental / Active Research & Development
* **Production Readiness**: **Not production-ready.** Intended for systems architecture research and evaluation.
* **Current Focus**: Low-level kernel primitives, memory protection, hardware abstraction, and deterministic verification.
* **Current Milestone**: **Stage 2 — Kernel Memory & Protection Foundation (Stage 2F Complete)**. Stage 3 (Preemption & System Services) has not yet begun.

---

## 🏛️ Implemented & Verified Architecture

Project Zero is implemented in pure `no_std` Rust with minimal low-level assembly for early bootstrap and context switching. Every subsystem is deterministically verified under QEMU:

* **x86-64 Bring-up**: 32-bit Multiboot 1 entry transitioning cleanly into 64-bit Long Mode with PAE and early paging (`boot.asm`).
* **Rust `no_std` Kernel Nucleus**: Staticlib kernel (`kernel/`) executing in Ring 0 without standard library or runtime overhead.
* **CPU & Hardware Abstraction (HAL)**: Control registers (`CR0`, `CR3`, `CR4`), MSR access, 16550 UART serial driver (`COM1` at 115200 8N1), 8259 PIC remapping, and 8254 PIT timer.
* **Privilege & Exception Architecture**:
  * 64-bit Global Descriptor Table (GDT) with Ring 0 and Ring 3 segment selectors.
  * Task State Segment (TSS) with an independent, 16-byte aligned IST1 stack for Double-Fault (`#DF`) handling.
  * Interrupt Descriptor Table (IDT) with 256 vector gates and low-level assembly ISR dispatchers (`isr.asm`).
* **Physical Memory Management (PMM)**:
  * Multiboot memory map discovery and parsing (`inventory.rs`).
  * Explicit protection of low memory, kernel binary sections, page tables, and metadata.
  * Bitmap-tracked 4 KiB physical frame allocator (`pmm.rs`).
* **Virtual Memory Management (VMM)**:
  * 4-level x86-64 page table management (`vmm.rs`) supporting canonical 48-bit linear addressing.
  * Dynamic demand-allocation of intermediate page tables with zeroed content.
  * Atomic allocation-failure rollback (zero intermediate table leaks).
  * Automatic empty-table reclamation during unmapping.
* **Higher-Half Kernel Architecture (Stage 2F)**:
  * Canonical higher-half kernel VMA (`0xFFFF_FFFF_8000_0000..0xFFFF_FFFF_8020_0000`).
  * 64 KiB higher-half stack and unmapped 4 KiB stack guard page.
  * **Higher-Half Direct Map (HHDM)**: Base `0xFFFF_8000_0000_0000` mapping `[0, 4 GiB)` physical memory via 2 MiB huge pages (`PML4[256]`). HHDM acts as a privileged supervisor aperture, decoupled from kernel VMA immutability.
  * **Identity Mapping Removal**: Low bootstrap identity mapping (`PML4[0]`) permanently cleared post-transition with full TLB shootdown (`CR3` reload).
  * **4 KiB Kernel VMA Permission Splitting & $W \oplus X$**:
    * `.multiboot_header`: Read-Only + No-Execute (R + NX)
    * `.text`: Read + Execute (RX)
    * `.rodata`: Read-Only + No-Execute (R + NX)
    * `.data`, `.bss`, `.pmm_metadata`, `.page_tables`, `.stack`: Read/Write + No-Execute (RW + NX)
    * `.stack_guard`: NOT PRESENT (unmapped page)
  * **Hardware Protections**:
    * `CR0.WP = 1`: Supervisor Write-Protect actively enforced.
    * `IA32_EFER.NXE = 1`: Execute-Disable bit actively enforced across non-code segments.
* **Hardware-Backed Fault Verification**:
  * Controlled `#PF` vector 14 traps verified against actual CPU-generated interrupt frames for `.rodata` write, `.text` write, `.data` execution (NX violation), and stack-guard access.
* **Microkernel Services & Primitives**:
  * Kernel Heap Allocator with coalescing implementing Rust's `GlobalAlloc`.
  * Cooperative threading descriptors and callee-preserved context switching (`context.asm`).
  * Synchronous endpoint rendezvous IPC messaging.
  * Cycle-accurate microbenchmarking using x86 `RDTSC`.

---

## 🎯 Design Principles

1. **Human-Intent-First Computing**: Systems should organize resources and contexts around human tasks and continuity, rather than isolated applications or single physical boxes.
2. **Explicit Ownership & Invariants**: Memory, frames, and capabilities have unambiguous owners. Invariants are asserted statically and dynamically.
3. **Capability-Oriented Security**: Security modeled on unforgeable object capabilities rather than coarse ambient authority.
4. **Strong Isolation**: Enforce $W \oplus X$, supervisor write-protection (`CR0.WP`), non-executable stacks, and distinct address domains.
5. **Deterministic Verification**: Every subsystem must have automated, reproducible verification under headless QEMU.
6. **Incremental Architecture**: Subsystems are built layer-by-layer; no skipping foundations.
7. **No Fake Implementations**: We do not commit empty stubs or mocks disguised as working functionality.
8. **Measured Claims**: Engineering claims must match verified hardware telemetry.
9. **Architecture Before Implementation**: Every major decision is reasoned, debated, and documented in an ADR before writing code.

---

## 🔭 Long-Term Vision (Future Research Goals)

The following capabilities represent long-term architectural intentions and research horizons:

* **AI-Native Operating System Capabilities**: Deep local inference integration for contextual reasoning, attention management, and automated system orchestration.
* **Device-Independent Workspaces**: Fluid migration of active execution contexts between phones, tablets, laptops, and workstations.
* **Personal Compute Fabric**: Distributed resource pooling allowing nearby personal nodes to transparently share CPU, GPU, and NPU compute.
* **Cross-Device Continuity**: Peer-to-peer state synchronization and shared capability spaces over cryptographic mesh networks.
* **Privacy-Preserving Computation**: Local-first processing with cryptographic isolation guarantees.
* **Attention Management**: Operating system primitives designed to respect user focus rather than optimize for engagement metrics.
* **Transactional System Updates**: Atomic, roll-forward/rollback system state management.

---

## 🛠️ Development Philosophy

Every major architectural subsystem in Project Zero follows a rigorous lifecycle:

$$\text{Inspect} \longrightarrow \text{Design} \longrightarrow \text{ADR} \longrightarrow \text{Implement} \longrightarrow \text{Compile} \longrightarrow \text{Test} \longrightarrow \text{Runtime Verification} \longrightarrow \text{Review} \longrightarrow \text{Merge}$$

No architectural layer begins until the underlying foundations are fully verified, regression-tested, and audited.

---

## 📁 Repository Structure

```text
project-zero/
├── .github/                          # CI workflows, issue and PR templates
├── boot/                             # Low-level CPU bootstrap & assembly stubs
│   ├── boot.asm                      # Multiboot 1 entry, 32->64 bit transition, paging
│   ├── isr.asm                       # Assembly ISR exception stubs & fault triggers
│   ├── context.asm                   # Callee-preserved context switch routine
│   └── linker.ld                     # Higher-half linker script (VMA = LMA + 0xFFFFFFFF80000000)
├── kernel/                           # Ring 0 Rust no_std Kernel
│   ├── Cargo.toml                    # Kernel staticlib crate definition
│   └── src/
│       ├── lib.rs                    # Kernel initialization & stage orchestrator
│       ├── hal/                      # Hardware Abstraction Layer (CPU, GDT, IDT, PIC, PIT, UART)
│       ├── mm/                       # Memory Management (PMM, VMM, Kernel Heap, Memory Map)
│       ├── task/                     # Concurrency & Scheduling (Threads, Context Switching)
│       ├── ipc/                      # Inter-Process Communication (Synchronous Rendezvous)
│       └── bench/                    # Cycle-accurate hardware microbenchmarking
├── docs/                             # Architectural Specifications & ADRs
│   ├── architecture/status.md        # Current milestone verification status
│   ├── decisions/                    # Architecture Decision Records (ADR-0001 through ADR-0010)
│   ├── audits/                       # Technical inspection & register audit reports
│   └── roadmap.md                    # Milestone roadmap (completed, next, and future)
├── tests/                            # Automated regression & integration test suite (61 tests)
├── tools/                            # Build automation & headless QEMU runner
│   └── run_qemu.py                   # Automated compilation, QEMU boot, and telemetry assertion
├── README.md                         # Project overview and documentation
├── LICENSE                           # Dual MIT / Apache-2.0 license notice
├── CONTRIBUTING.md                   # Contribution guidelines and engineering standards
├── CODE_OF_CONDUCT.md               # Contributor Covenant Code of Conduct
├── SECURITY.md                       # Security policy and vulnerability reporting
└── CHANGELOG.md                      # Milestone changelog
```

---

## ⚙️ Building

### Prerequisites
* **Rust**: Nightly toolchain with `rust-src` and `llvm-tools-preview` components (`rustup target add x86_64-unknown-none`).
* **NASM**: Netwide Assembler (`nasm`).
* **LLD Linker**: `rust-lld` (included with Rust toolchains).
* **Binutils**: `objcopy` (e.g., from MSYS2 on Windows or standard package manager on Linux).
* **Python**: Python 3.10+ (for test automation).
* **QEMU**: `qemu-system-x86_64`.

### Compilation & Build
To build the kernel staticlib and create the Multiboot-compatible kernel image:
```powershell
python tools/run_qemu.py
```
This script automatically compiles the Rust staticlib via Cargo, assembles all NASM bootstrap and ISR stubs, links the 64-bit ELF via `rust-lld`, extracts the Multiboot ELF container using `objcopy`, and boots the kernel headlessly in QEMU.

---

## 🧪 Testing & Verification

Project Zero relies on automated hardware-in-the-loop verification under headless QEMU with serial telemetry assertions.

### Run Automated Build & QEMU Telemetry Verification
```powershell
python tools/run_qemu.py
```

### Run Full Regression Suite (61 Tests)
```powershell
python -m unittest discover -s tests -v
```

The test suite validates:
* ELF section boundaries, 4 KiB alignments, and $\le 2$ MiB window assertion.
* GDT, TSS with IST1, and IDT 256 vector registrations.
* Multiboot memory map parsing and PMM physical frame allocation/freeing.
* VMM 4-level mapping, unmapping, and table reclamation.
* Higher-half VMA transition and permanent identity-mapping removal (`PML4[0]` absent).
* Hardware protections (`CR0.WP = 1`, `IA32_EFER.NXE = 1`).
* Controlled hardware-generated Page Faults (`#PF`) with exact CR2 addresses and error codes.
* HHDM privileged aperture access and restoration.
* Clean shutdown via `isa-debug-exit` (exit code 33).

---

## 🗺️ Roadmap

For details on completed milestones, upcoming Stage 3 plans, and long-term architectural horizons, refer to [docs/roadmap.md](docs/roadmap.md).

---

## 📄 License

Project Zero is dual-licensed under:
* [MIT License](LICENSE-MIT)
* [Apache License, Version 2.0](LICENSE-APACHE)

at your option.
