# ADR-0005: Interrupt Controller & Timer Architecture

## Status
**Accepted**

## Date
2026-09-13

## Context & Architectural Problem

Project Zero requires an interrupt and timer subsystem capable of supporting:
1. Deterministic kernel execution and diagnostic exception trapping in early boot.
2. Low-jitter periodic and one-shot timer events for thread scheduling.
3. Long-term scalability to multi-core symmetric multiprocessing (SMP) and per-CPU runqueues.
4. Preemptive multitasking (Stage 3).

Under x86-64 and QEMU, multiple timer and interrupt architectures exist. Selecting an interrupt architecture must not be based on familiarity, but on a clear progression from minimal, debuggable single-core bringup to high-throughput multi-core preemption.

---

## Evaluation of Candidate Architectures

### 1. Legacy 8259 PIC + 8254 PIT (Programmable Interval Timer)
* **Mechanism**: 
  * Interrupts: Dual cascaded 8259 controllers on legacy I/O ports `0x20/0x21` (Master) and `0xA0/0xA1` (Slave).
  * Timer: 8254 PIT running at fixed $1.193182\text{ MHz}$ oscillator frequency accessed via I/O ports `0x40..0x43`.
* **Advantages**:
  * Extreme simplicity: Requires zero MMIO page mapping, zero ACPI table parsing, and zero MSR access.
  * Universal baseline in QEMU and PC hardware.
  * Deterministic 100 Hz ($10\text{ ms}$) periodic tick generation with minimal lines of code.
* **Disadvantages & Bottlenecks**:
  * **No Multi-Core / SMP capability**: Only routes interrupts to the Bootstrap Processor (BSP). Cannot send Inter-Processor Interrupts (IPIs).
  * **Low Resolution / Jitter**: Limited to ~$1\text{ kHz}$ maximum rate without excessive port I/O bus overhead ($~1\ \mu\text{s}$ per port access).
  * **Legacy Overhead**: Slow bus access (`outb`/`inb`) on peripheral buses.

### 2. Local APIC + I/O APIC
* **Mechanism**:
  * Interrupts: Each CPU core possesses an integrated Local APIC (LAPIC) mapped into physical MMIO space (default base address `0xFEE00000`). System-wide hardware interrupts route through an I/O APIC (mapped at `0xFEC00000`).
  * Timer: Integrated 32-bit LAPIC timer running at CPU bus clock or core clock frequency, programmable in one-shot, periodic, or TSC-deadline mode.
* **Advantages**:
  * **True Per-CPU Timers**: Each core has an independent timer with its own interrupt vector. Essential for per-CPU scheduling and lockless runqueues.
  * **Inter-Processor Interrupts (IPIs)**: Supports core-to-core signaling required for TLB shootdown, thread migration, and scheduler wakeup.
  * **High Resolution**: High frequency, low latency (MMIO or register access).
* **Disadvantages & Bottlenecks**:
  * Requires identity mapping or virtual memory paging for the LAPIC MMIO page (`0xFEE00000`).
  * Requires parsing ACPI MADT (Multiple APIC Description Table) to discover I/O APIC and core topologies.
  * In early single-core bringup, introduces circular dependencies if memory management is not yet stable.

### 3. x2APIC
* **Mechanism**:
  * Architectural extension to Local APIC introduced in modern x86-64 processors.
  * Replaces MMIO with direct Model-Specific Registers (MSRs `0x800..0x8FF`), accessed via `rdmsr` and `wrmsr`.
* **Advantages**:
  * Eliminates MMIO mapping requirements entirely.
  * Supports 32-bit APIC IDs (scaling to $>256$ cores).
  * Supports hardware TSC-deadline mode for near-zero jitter timer programming.
* **Disadvantages & Bottlenecks**:
  * Requires hardware CPUID feature flag validation (`CPUID.01H:ECX[21]`).
  * Not available on older virtualization baselines without modern CPU model flags.

### 4. HPET (High Precision Event Timer) & ACPI PM Timer
* **Mechanism**: Platform timers mapped in MMIO (HPET at `0xFED00000`).
* **Advantages**: High-resolution monotonically increasing counter (10+ MHz).
* **Disadvantages**: Off-chip peripheral bus latency; does not replace per-core scheduling timers.

---

## Architectural Comparison Matrix

| Dimension | 8259 PIC + PIT | Local APIC + I/O APIC | x2APIC | HPET |
| :--- | :--- | :--- | :--- | :--- |
| **Stage 2 Bringup Complexity** | **Minimal** (Port I/O only) | **Moderate** (Requires MMIO mapping) | **Moderate** (Requires MSRs + CPUID) | **High** (ACPI + MMIO) |
| **Multi-Core (SMP) Scalability** | **None** (Single-core BSP only) | **Native** (IPIs, per-core LAPIC) | **Native** (>256 cores) | **Shared** (Platform-wide) |
| **Per-CPU Scheduling Support** | **No** (Single shared tick) | **Yes** (Dedicated per-core timer) | **Yes** (Dedicated per-core timer) | **No** (Centralized) |
| **Preemption Precision** | Fixed 100 Hz ($10\text{ ms}$) | High-frequency bus clock / One-shot | TSC-Deadline mode | High frequency |
| **MMIO Paging Dependency** | **None** | Yes (`0xFEE00000`) | **None** (MSRs) | Yes (`0xFED00000`) |

---

## Decision & Staged Migration Path

We establish a clear two-phase interrupt and timer strategy that avoids dual competing systems while preserving a clean path to future SMP:

### Phase 1: Stage 2 Nucleus Baseline (8259 PIC Remap + 8254 PIT)
1. **Purpose**: Remap the 8259 PIC vectors from real-mode defaults (`0x08..0x0F`) to protected vectors `32..47`, preventing collisions with CPU hardware exceptions (`0..31`). Configure PIT Channel 0 to 100 Hz periodic rate generator (`1193182 / 11932 \approx 100\text{ Hz}`).
2. **Strict Scope**: In Stage 2, the timer is used strictly for uptime tracking and diagnostic telemetry. Stage 2 establishes the **Cooperative Kernel Scheduling Foundation** (`yield`), and does not implement timer-driven preemption.
3. **Encapsulation**: All interrupt control is isolated inside the Hardware Abstraction Layer (`crate::hal::arch::x86_64::timer` and `send_eoi`). Higher-level scheduler code interacts only with abstract tick counters and never directly references PIC ports.

### Phase 2: Stage 3 Preemption & Multi-Core SMP (Local APIC Migration)
1. In Stage 3, as part of multi-core bringup, the 8259 PIC will be disabled permanently (`outb(0x21, 0xFF)` and `outb(0xA1, 0xFF)`).
2. The Local APIC timer will be initialized in one-shot or TSC-deadline mode on each active CPU core.
3. The HAL `send_eoi()` interface will switch to writing `0` to LAPIC EOI register (`0xFEE000B0`).
4. Per-CPU timers will drive preemptive thread quantum slices and per-core scheduler runqueues.

This guarantees zero rework of kernel scheduling abstractions while keeping Stage 2 completely free of premature ACPI/MMIO dependencies.
