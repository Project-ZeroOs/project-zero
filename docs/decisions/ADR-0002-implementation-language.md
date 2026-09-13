# ADR-0002: Systems Programming Language Selection

## Status
**Accepted**

## Date
2026-09-13

## Context & Evaluation Criteria

The choice of programming language for an operating system kernel and its foundational runtime dictates memory safety, concurrency guarantees, developer velocity, defect density, and long-term maintainability.

We evaluate three language approaches:
1. **C (C11 / C23)**: The historic standard for systems programming (UNIX, Linux, Windows NT).
2. **Rust (`no_std` bare-metal)**: Modern systems programming language with compile-time memory safety, affine type systems, and zero-cost abstractions.
3. **Hybrid Rust / C**: C for the core kernel nucleus, Rust for userspace services, or vice versa.

| Evaluation Metric | C | Rust (`no_std`) | Hybrid Rust / C |
| :--- | :--- | :--- | :--- |
| **Spatial & Temporal Memory Safety** | **Extremely Poor** (Manual malloc/free; UAF, double-free, buffer overflows account for ~70% of CVEs) | **Exceptional** (Enforced at compile-time via ownership, borrowing, lifetimes) | **Partial** (C boundary remains vulnerable to memory corruption) |
| **Data-Race Freedom in Concurrency** | **None** (Requires manual mutex discipline; data races are undefined behavior) | **Guaranteed** (`Send` and `Sync` traits prevent concurrent mutation at compile-time) | **Partial** (C components can introduce subtle multi-core data races) |
| **Zero-Cost Abstractions & Expressiveness** | **Poor** (No generics without macro hacks; void pointers; lack of type safety) | **Exceptional** (Type-safe abstractions, algebraic enums, traits, pattern matching) | **High** (Expressiveness in Rust, restricted in C) |
| **Hardware Control & Inline Assembly** | **Direct** (Trivial pointer arithmetic, volatile writes, inline asm) | **Direct** (`core::arch::asm!`, raw pointers within explicit `unsafe` blocks) | **Direct** |
| **Runtime Overhead & Garbage Collection** | **Zero** (Predictable bare-metal execution) | **Zero** (`no_std` mode uses no garbage collector and no default allocator) | **Zero** |
| **Toolchain & Cross-Compilation** | **Mature** (GCC, Clang across all architectures) | **Exceptional** (`rustc` LLVM backend supports cross-compilation out of the box) | **Complex** (Requires coordinating dual toolchains, headers, and bindgen) |
| **ABI Boundary Stability** | **Universal** (The C ABI is the universal lingua franca) | **Internal ABI Unstable** (Requires `extern "C"` for foreign function interfaces) | **Native** at the boundary |

---

## Detailed Evaluation of Alternatives

### 1. The Pure C Approach
* **Arguments in Favor**: Decades of legacy documentation, established OS textbooks, universal C ABI.
* **Why C is Rejected for Project Zero**:
  * Industry telemetry from Microsoft and Google confirms that approximately **70% of all critical operating system vulnerabilities stem from memory safety defects** (buffer overflows, use-after-free, uninitialized pointers).
  * Project Zero is an advanced, distributed, multi-threaded operating system with complex capability tables, network meshes, and concurrent lockless ring buffers. Writing this in C introduces an unacceptable defect rate and continuous cognitive overhead debugging memory corruption.

### 2. The Hybrid Rust / C Approach
* **Arguments in Favor**: Allows using C for bare-metal assembly/paging while writing higher-level services in Rust.
* **Drawbacks**: Introduces the worst of both worlds:
  * Dual-build system complexity (`make`/`cmake` + `cargo`).
  * Fragile foreign function interface (FFI) bindings via `bindgen` that must be manually synchronized.
  * Undefined behavior in C can silently corrupt Rust memory invariants, invalidating Rust's safety guarantees.

### 3. The Pure Rust (`no_std`) Approach
* **Why Rust is the Optimal Choice**:
  * **Memory Safety by Construction**: In `no_std` Rust, raw pointers and hardware manipulation are encapsulated inside explicit `unsafe` blocks. Over 90% of the kernel code (scheduler logic, IPC routing, capability graphs, protocol parsers) is verifiably safe and incapable of panicking due to buffer overflows.
  * **Type-State Pattern for Capabilities**: Rust's affine type system allows representing capability states (e.g., Unmapped vs. Mapped memory, Attenuated vs. Root tokens) at compile-time, preventing unauthorized state transitions before the code ever compiles.
  * **Modern Ergonomics**: Match expressions, Result/Option error handling, RAII drop semantics, and rich static analysis drastically reduce development time without sacrificing a single microsecond of bare-metal performance.
  * **Minimal Low-Level Assembly**: The few operations that require raw CPU instructions (e.g., loading CR3, setting up GDT/IDT, context switching) are written either via `core::arch::asm!` or standalone assembly files (`.S`) with clean `extern "C"` entry points.

---

## Decision

**We select pure Rust (`no_std`) as the primary systems implementation language for the Project Zero kernel nucleus, drivers, and system services.**

* Target: `x86_64-unknown-none` (bare-metal ELF binary).
* Assembly: Standalone assembly (`.S`) and `core::arch::asm!` strictly isolated to the Hardware Abstraction Layer (`hal/arch/`).
* Interoperability: All kernel system call interfaces and inter-process capability boundaries will adhere to a deterministic, type-safe ABI. External components (if any) can interact with the system via standard C-compatible ABI bindings (`extern "C"`).

---

## Consequences

### Positive
* Elimination of an entire class of memory corruption bugs and data races in the core operating system.
* Extreme expressive power: typed capabilities, zero-cost abstractions, and fearless multi-core concurrency.
* Modern package and dependency management via Cargo with strict offline/vendored build reproducible constraints.

### Negative & Mitigations
* Steep learning curve for bare-metal `no_std` Rust: mitigated by adhering to strict idioms, comprehensive architectural documentation, and isolating `unsafe` blocks.
* Absence of standard library (`std`): mitigated by implementing our own minimal allocation primitives (`core::alloc`) and using `core` library structures.
