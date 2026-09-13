# Project Zero — Architecture Roadmap

> [!NOTE]
> Future milestones in this roadmap are architectural intentions and research directions, not promises of delivery or completed implementations. Each stage must undergo formal architecture design, ADR authoring, review, implementation, and deterministic verification before subsequent stages begin.

---

## Completed Foundations

### Stage 1: Multiboot & 64-bit Long Mode Bring-up
- Multiboot 1 compliant bootstrap in NASM.
- Protected mode to Long Mode (64-bit) transition.
- 16550 UART serial driver for early diagnostic telemetry.
- Automated headless QEMU test harness with `isa-debug-exit`.

### Stage 2: Kernel Memory & Protection Foundation
- **Stage 2A**: Core CPU registers, control registers (`CR0`, `CR3`, `CR4`), MSRs, and CPU diagnostics.
- **Stage 2B**: 64-bit Global Descriptor Table (GDT), Ring 0 and Ring 3 segment selectors, and Task State Segment (TSS) with dedicated 16-byte aligned IST1 double-fault stack.
- **Stage 2C**: Interrupt Descriptor Table (IDT) with 256 vector gates, assembly ISR stubs, and controlled exception recovery.
- **Stage 2D**: Physical Memory Manager (PMM) with Multiboot memory map discovery, reserved memory protection, and bitmap-tracked 4 KiB frame allocation.
- **Stage 2E**: 4-level x86-64 Virtual Memory Manager (VMM), dynamic intermediate page table allocation, rollback on allocation failure, and empty-table reclamation.
- **Stage 2F**: Higher-Half Kernel Architecture & Memory Protection:
  - Canonical higher-half execution switch (`0xFFFFFFFF8010xxxx`).
  - Higher-Half Direct Map (HHDM at `0xFFFF800000000000`) over `[0, 4 GiB)` physical RAM.
  - Permanent low identity mapping removal (`PML4[0]` absent).
  - Fine-grained 4 KiB permission splitting ($W \oplus X$) on Kernel VMA.
  - Hardware protections: `CR0.WP = 1` and `IA32_EFER.NXE = 1`.
  - Controlled hardware `#PF` exception verification for `.rodata` write, `.text` write, `.data` execution, and stack guard access.

---

## Next Milestone

### Stage 3: Preemptive Scheduling, Timers & System Services
- Formal Architecture Definition & ADR authoring.
- Local APIC (LAPIC) and I/O APIC discovery and configuration via ACPI MADT.
- High-precision timer configuration (APIC timer / HPET).
- Preemptive round-robin and priority scheduling models.
- Ring 0 interrupt-driven context switching.
- System call boundary (`syscall`/`sysret`) architecture.

---

## Long-Term Vision & Research Horizons

The following higher-level capabilities represent long-term architectural goals for Project Zero:

1. **Capability-Based Object Security**: Fine-grained, unforgeable capability tokens replacing traditional hierarchical permissions (ACLs/Unix user groups).
2. **Personal Compute Fabric**: A decentralized runtime where personal devices (workstations, laptops, mobile nodes) coordinate compute and memory over low-latency cryptographic meshes.
3. **AI-Native Operating System Services**: Deeply integrated, capability-isolated local inference pipelines assisting memory, attention management, and intent routing.
4. **Device-Independent Workspaces**: State migration and execution continuity across heterogeneous hardware targets.
5. **Zero-Copy Compositor & Visual Engine**: Direct hardware buffer sharing and modern presentation pipelines.
