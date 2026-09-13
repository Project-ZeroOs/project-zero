# Project Zero: Operating System Architecture

## 1. Architectural Philosophy: The Lean Capability Microkernel

Project Zero adopts a **microkernel-inspired, capability-secured modular architecture**. 

Traditional monolithic kernels (e.g., Linux, Windows NT) place memory managers, virtual filesystem layers, network stacks, storage drivers, and GPU drivers directly inside the privileged supervisor mode (Ring 0 / EL1). While monolithic kernels historically offered high IPC performance, they suffer from severe structural drawbacks:
* **Massive Attack Surface**: A flaw in any third-party Wi-Fi driver or filesystem parser grants total kernel compromise.
* **Lack of Fault Isolation**: A driver crash causes a complete kernel panic and system halt.
* **Rigid Monolithic Boundary**: Distributed offloading of kernel services is extraordinarily complex because kernel components assume direct access to local physical memory.

Project Zero enforces the **Principle of Least Privilege** and the **Separation of Mechanism from Policy**:
* **Mechanism in Kernel (Ring 0)**: Minimal primitives required for hardware control: CPU state switching, low-level virtual memory mapping, hardware interrupt dispatch, capability validation, and synchronous/asynchronous IPC.
* **Policy in Userspace (Ring 3)**: Device drivers, filesystem implementations, network protocol stacks, resource allocation policies, memory pagers, and window compositors run as isolated, unprivileged processes communicating over typed IPC channels.

```text
+-------------------------------------------------------------------------+
|                           USER WORKSPACE DOMAIN                         |
|   +-----------------------+ +--------------------+ +----------------+   |
|   | Intent / Task Manager | | Spatial UI Surface | | AI Context Eng |   |
|   +-----------+-----------+ +---------+----------+ +--------+-------+   |
+---------------|-----------------------|---------------------|-----------+
                | Typed Capability IPC  |                     |
+---------------v-----------------------v---------------------v-----------+
|                          SYSTEM SERVICES DOMAIN                         |
|   +-----------------------+ +--------------------+ +----------------+   |
|   |  Capability Broker    | |  Compute Fabric    | | Memory Pager   |   |
|   +-----------------------+ +--------------------+ +----------------+   |
|   +-----------------------+ +--------------------+ +----------------+   |
|   |  Spatial Compositor   | |  Network Mesh Dmn  | | Storage Engine |   |
|   +-----------------------+ +--------------------+ +----------------+   |
|   +-----------------------------------------------------------------+   |
|   |          ISOLATED DEVICE DRIVERS (GPU, Net, Storage, USB)       |   |
|   +-----------------------------------------------------------------+   |
+-----------------------------------|-------------------------------------+
                                    | Syscalls (Invoke Capability)
+-----------------------------------v-------------------------------------+
|                      PROJECT ZERO NUCLEUS (KERNEL)                      |
|                                                                         |
|  * Capability Verification Unit       * Thread Dispatch & Preemption    |
|  * Zero-Copy IPC Rendezvous Channels  * Page Table Manipulation Prims  |
|  * Interrupt Dispatch & Vectoring     * Hardware Abstraction Layer      |
+-------------------------------------------------------------------------+
                                    |
                           PHYSICAL HARDWARE
               (x86-64 CPU, MMU, APIC, IOMMU, PCIe, UART)
```

---

## 2. Kernel Nucleus Responsibilities

The Project Zero kernel nucleus is deliberately restricted to four essential duties:

### 2.1 Capability Table Management (C-Lists)
Every thread belongs to a protection domain defined by its **Capability List (C-List)**. The kernel holds no ambient lookup tables; a thread can only access memory regions, trigger hardware I/O, or message another process if it holds an authorized index in its C-List.

### 2.2 IPC Rendezvous & Shared Memory Channels
IPC is the nervous system of Project Zero:
1. **Synchronous Fast-Path IPC**: Optimized for single-message request-reply exchanges. Registers ($rax, rdx, rdi, rsi$) are preserved across address space switches to achieve sub-microsecond latency.
2. **Asynchronous Ring Buffers**: Shared memory regions mapped into both client and service spaces for high-throughput streaming (e.g., audio streams, network packet buffers, GPU command buffers).
3. **Capability Transfer**: IPC messages can carry capabilities, allowing processes to delegate and revoke rights dynamically.

### 2.3 Low-Level Address Space Control
The kernel manages raw page tables (CR3 in x86-64, TTBR0/1 in ARM64) but does **not** decide paging policy. When a thread encounters an unmapped address, the kernel generates a page fault message and forwards it via IPC to the userspace **Memory Pager Service**.

### 2.4 Scheduling & Hardware Interrupt Dispatch
* The scheduler manages execution units (threads) across available physical and logical CPU cores.
* Hardware interrupts from the APIC/GIC are transformed by the kernel into lightweight IPC notification signals dispatched to registered userspace driver threads.

---

## 3. Hardware Abstraction Layer (HAL)

To maintain absolute independence from CPU and board architectures, Project Zero implements a strict HAL interface dividing architecture-specific assembly from portable kernel logic:

```text
kernel/
├── hal/
│   ├── arch/
│   │   ├── x86_64/
│   │   │   ├── cpu.rs           // GDT, IDT, CR0/CR3/CR4, RFLAGS
│   │   │   ├── mmu.rs           // PML4, PDPT, PD, PT entry manipulation
│   │   │   ├── apic.rs          // LAPIC, IOAPIC interrupt controllers
│   │   │   ├── serial.rs        // 16550 UART early debugging console
│   │   │   └── context.S        // Assembly thread context save/restore
│   │   ├── arm64/               // Future target (EL0/EL1, GICv3, Stage 1 MMU)
│   │   └── riscv64/             // Future target (Sv39/Sv48, PLIC)
│   └── hal_traits.rs            // Generic CPU, MMU, and Interrupt traits
```

### 3.1 Initial Architecture Selection: x86-64
* **Target Environment**: x86-64 virtualized under QEMU.
* **Firmware/Boot**: Multiboot2 / Limine compliant bootloader initiating 64-bit Long Mode with paging enabled.
* **Debugging**: 16550 UART serial output mapped to `COM1` ($0x3F8$) for reliable, zero-dependency diagnostics.

### 3.2 Portability Guidelines
* All architecture-specific structures (page tables, control registers, TSS) are encapsulated behind uniform Rust traits.
* Higher-level kernel subsystems (schedulers, IPC routers, capability checkers) contain zero architecture `#cfg` flags.

---

## 4. Execution Domains & Isolation

Project Zero splits system execution into distinct security domains:

| Domain | Privilege Level | Paging Rights | Memory Access | Role |
| :--- | :--- | :--- | :--- | :--- |
| **Kernel Nucleus** | Ring 0 (Supervisor) | Kernel High-Half Virtual Address Space | Direct access to physical memory mapping | Primitives, IPC, Scheduler, MMU control |
| **Hardware Drivers** | Ring 3 (User) | Isolated Address Space | MMIO regions mapped strictly via capabilities; no direct physical access | GPU, Network, NVMe, USB drivers |
| **System Services** | Ring 3 (User) | Isolated Address Space | Capability-bounded memory | Memory Pager, Storage Engine, Compositor |
| **Compute Fabric** | Ring 3 (User) | Isolated Address Space | Sandboxed network & telemetry channels | Node discovery, compute planning, offloading |
| **User Workspaces** | Ring 3 (User) | Isolated Address Space | Private heap, capability-granted shared buffers | User tasks, application engines, AI tools |

---

## 5. Threading & Scheduling Model

Project Zero implements a **multi-class latency-deterministic scheduler**:

```text
              +-----------------------------------+
              |      THREAD READY QUEUE           |
              +-----------------+-----------------+
                                |
       +------------------------+------------------------+
       |                                                 |
+------v----------------+                       +--------v---------------+
| Real-Time Class       |                       | Proportional Fair      |
| (UI Compositor, Input,|                       | (Background Tasks,     |
|  Audio, Fast IPC)     |                       |  Compute Offloading,   |
| Guaranteed Deadline   |                       |  Data Indexing)        |
+-----------------------+                       +------------------------+
```

### 5.1 Real-Time UI / Input Class
* Uses an **Earliest Deadline First (EDF)** or static priority scheme with strict execution time budgets.
* Input event ingestion, pointer handling, and frame composition threads run in this class.
* Preempts any background computation to guarantee sub-millisecond input response and zero dropped frames.

### 5.2 Workload / Compute Class
* Employs a **proportional-share fair queuing** algorithm.
* Workloads are dynamic and state-aware: if thermal headroom drops or battery levels fall below a critical threshold, the scheduler throttles background threads or triggers the Compute Fabric to offload work.

---

## 6. Virtual Memory Management Architecture

Memory in Project Zero is treated as a capability:
1. **Physical Frames (`FrameCap`)**: Represents a physical block of 4 KiB, 2 MiB, or 1 GiB memory.
2. **Virtual Memory Objects (`VmoCap`)**: Abstract continuous regions of virtual memory that can be backed by physical frames, memory-mapped files, or shared channels.
3. **Address Spaces (`SpaceCap`)**: Represents a complete page table tree (e.g., PML4). A process can map a `VmoCap` into an `SpaceCap` only if it holds write/execute rights.

### 6.1 Userspace Memory Paging
The kernel holds no complex page swap or allocation logic:
```text
Thread Access Unmapped Memory
         │
         ▼
[Hardware Page Fault (#PF)]
         │
         ▼
[Kernel Nucleus Captures Trap]
         │
         ▼  (Converts fault to IPC message: {addr, IP, fault_flags})
[Kernel Dispatches IPC to Memory Pager]
         │
         ▼
[Userspace Memory Pager Resolves Frame / Reads from Storage]
         │
         ▼  (Invokes MapPage(SpaceCap, VmoCap, VirtualAddr, PhysicalFrame))
[Kernel Updates Page Table Entry]
         │
         ▼
[Kernel Resumes Faulting Thread]
```

This ensures that distributed memory, network-backed memory, or compressed memory caches can be implemented and modified entirely in userspace without risking kernel stability.

---

## 7. First-Class IPC Design

Communication between isolated domains must be blazingly fast. Project Zero implements three IPC mechanisms:

1. **Direct Thread Switch (Hand-off Scheduling)**:
   When Process A calls synchronous IPC to Process B, the kernel does not place Process A on a wait queue and schedule B later. Instead, the CPU yields its remaining time slice directly to Process B's receiving thread, eliminating scheduling latency and context-switch queue overhead.
2. **Zero-Copy Grant & Map**:
   Large payloads (such as network packet buffers or image frames) are never copied across boundaries. Instead, page frames are granted or shared between address spaces using capability transfer.
3. **Asynchronous Notification Ports**:
   Lightweight 64-bit atomic bitmasks for high-frequency signal dispatch without blocking. Used for interrupt handlers and device event notifications.
