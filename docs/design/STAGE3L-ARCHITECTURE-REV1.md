# Project Zero — Stage 3L: Device / Hardware Model
## Architecture Specification Rev1 — Comprehensive Design & Adversarial Review

**Document Version:** 1.0.0  
**Status:** PROPOSED / ARCHITECTURE-FIRST DRAFT (PRE-IMPLEMENTATION)  
**Author:** Google DeepMind Advanced Agentic Coding Team  
**Date:** September 2026  
**Target Platform:** x86-64 (QEMU / Bare-Metal PC-AT, LAPIC, PIC, ATA PIO, PCI/MMIO)

---

## Executive Summary & System Context

Stage 3L establishes the authoritative **Device and Hardware Model** for Project Zero (ZeroOS). ZeroOS has completed and frozen Stages 3A through 3K:

```text
Stage 3A: Threads                          🟢 FROZEN
Stage 3B: Cooperative Scheduler            🟢 FROZEN
Stage 3C: Preemption / Timer               🟢 FROZEN
Stage 3D: Synchronization                  🟢 FROZEN
Stage 3E: Thread Lifecycle                 🟢 FROZEN
Stage 3F: Processes                        🟢 FROZEN
Stage 3G: IPC + SHM                        🟢 FROZEN
Stage 3H: Capabilities                     🟢 FROZEN
Stage 3I: User Space + Syscalls            🟢 FROZEN
Stage 3J: ELF / Program Execution          🟢 FROZEN
Stage 3K: Storage / ZeroFS                 🟢 FROZEN
```

Stage 3L designs the unified, low-level hardware abstraction layer without reopening or mutating any frozen Stage 3A–3K ABI or invariant:
1. **Frozen ABI Sizes Preserved:** `Process` (128 B), `KernelThread` (176 B), `PerCpu` (48 B), `HandleTable` (520 B), `CapabilityNode` (24 B), `KernelObjectSlot` (40 B) remain completely unchanged.
2. **Zero Dynamic Kernel Heap:** All device tables, resource maps, interrupt dispatch tables, and DMA trackers are strictly statically sized and bounded in `.bss`.
3. **Capability-Authorized Hardware Access:** Devices are first-class kernel objects (`KernelObjectType::Device = 6`). Processes and drivers can only inspect, control, map MMIO, allocate DMA, or receive interrupts if they possess a valid `CapabilityNode` granting explicit device rights.
4. **Strict Isolation & Resource Conflict Rejection:** Hardware resources (I/O ports, MMIO ranges, IRQs, DMA buffers) cannot be shared or overlapping unless explicitly marked `Shared`. Overlapping exclusive registrations are rejected deterministically.
5. **Stage 3K Storage Harmonization:** The existing `BlockDevice` trait, `MemDeviceWrapper` (ID 0), and `AtaPioBlockDevice` (ID 1) integrate seamlessly into the Stage 3L device registry without breaking ZeroFS contracts.

---

## 1. Repository-Derived Hardware Baseline

An exhaustive audit of the frozen repository reveals the following hardware primitives and contracts:

### 1.1 Existing Hardware Abstractions
- **CPU & Port I/O (`hal/arch/x86_64/cpu.rs`):** Low-level port I/O primitives: `inb`, `outb`, `inw`, `outw`, `io_wait`, `hlt`, `cli`, `sti`.
- **Interrupt Controllers (`hal/arch/x86_64/timer.rs`, `lapic.rs`, `idt.rs`):**
  - Legacy 8259 PIC remapped to vectors 32..47, but masked during LAPIC operation.
  - Local APIC mapped at physical `0xFEE0_0000` via HHDM / identity virtual `0xFFFF_FFFF_FEE0_0000`. Vector 32 is bound to LAPIC periodic timer (100 Hz, 10 ms quantum).
  - Interrupt vectors 0..31 are reserved for CPU exceptions.
- **Physical Memory Management (`mm/pmm.rs`):** Physical memory frames (4096 bytes) managed via bitmap allocator.
- **Virtual Memory Management (`mm/vmm.rs`):** 4-level x86-64 paging (`PML4 -> PDPT -> PD -> PT`). `HHDM_BASE = 0xFFFF_8000_0000_0000`.
- **Block Device Layer (`fs/dev.rs`):** `BlockDevice` trait with `device_id() -> u8`, `block_count() -> u64`, `state() -> DeviceState`, `read_block()`, `write_block()`, `flush()`.
  - Device 0: `MemDeviceWrapper` (256 frames = 1 MiB mock RAM volume).
  - Device 1: `AtaPioBlockDevice` (Primary ATA controller at I/O ports `0x1F0..0x1F7`, `0x3F6`).
  - Synchronization: `BLOCK_DEVICE_LOCK` spinlock.

### 1.2 Frozen Lock Hierarchy Baseline
The existing lock hierarchy is strictly monotonic:
$$\text{FILESYSTEM\_LOCK (1)} < \text{STORAGE\_OBJECT\_TABLE\_LOCK (2)} < \text{BLOCK\_CACHE\_LOCK (3)} < \text{BLOCK\_DEVICE\_LOCK (4)} < \text{KERNEL\_OBJECT\_TABLE\_LOCK (7)} < \text{SCHEDULER.lock (8)} < \text{CPU (IF=0)}$$

Stage 3L inserts device-level locks into levels 5 and 6 without disturbing levels 1–4 or 7–8.

---

## 2. ZeroOS Device Model

### 2.1 Device Identity (`DeviceId`)
A device's authoritative identity is a 64-bit monotonically allocated integer:
```rust
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u64);
```

**Identity Lifetime Invariants:**
- `DeviceId` is allocated from a global atomic counter `NEXT_DEVICE_ID`, beginning at `1`. `DeviceId(0)` is reserved as `INVALID_DEVICE_ID`.
- `DeviceId` is globally unique across the kernel lifetime.
- `DeviceId` is **completely independent** from:
  - Device-table slot index (`slot_idx: usize`, 0..MAX_DEVICES - 1).
  - PCI/Bus topology address (Bus, Device, Function, Segment).
  - MMIO physical base address or I/O port base.
  - Process capability handle (`Handle(pub u32)`).
- If a device is detached, unplugged, or re-probed, its slot may be reused, but a newly assigned `DeviceId` is strictly greater than all previously assigned IDs.
- Stale `DeviceId` checks prevent any ABA table-slot confusion.

### 2.2 Device Classes (`DeviceClass`)
All hardware devices belong to a strongly typed class:
```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Unknown             = 0,
    Storage             = 1, // ATA, NVMe, VirtIO-Blk, MemBlock
    Network             = 2, // Ethernet NIC, VirtIO-Net
    Display             = 3, // VGA, Linear Framebuffer, VirtIO-GPU
    Input               = 4, // PS/2 Keyboard, Mouse, VirtIO-Input
    BusController       = 5, // PCI Host Bridge, PCIe Root Port, USB Controller
    Accelerator         = 6, // GPU Compute Engine, NPU Inference Engine
    Timer               = 7, // LAPIC Timer, PIT, HPET, RTC
    InterruptController = 8, // PIC, IOAPIC, LAPIC
    Platform            = 9, // Power Management, ACPI, System Reset
}
```

### 2.3 Device Lifecycle State Machine
Every device transitions through a formal deterministic state machine:

```text
                 +-------------+
                 | Discovered  | (0)
                 +------+------+
                        | probe()
                        v
                 +-------------+
                 |   Probed    | (1)
                 +------+------+
                        | attach()
                        v
                 +-------------+
                 |  Attached   | (2)
                 +------+------+
                        | activate()
                        v
                 +-------------+     submit()      +-------------+
                 |    Ready    | <===============> |   Active    | (4)
                 +------+------+     complete()    +------+------+
                        |                                 |
              quiesce() |                                 | fault()
                        v                                 v
                 +-------------+                   +-------------+
                 |  Quiescing  | (5)               |   Faulted   | (8)
                 +------+------+                   +------+------+
                        |                                 |
               detach() |               +-----------------+-----------------+
                        v               | reset()                           | unrecoverable
                 +-------------+        v                                   v
                 |  Detached   | (6) +-------------+                 +-------------+
                 +------+------+     |  Resetting  | (9)             |  Detached   | (6)
                        |            +------+------+                 +-------------+
              release() |                   | success
                        v                   v
                 +-------------+     +-------------+
                 |  Released   | (7) |    Ready    | (3)
                 +-------------+     +-------------+
```

#### State Definitions & Invariants:
1. **`Discovered` (0):** Bus enumeration or static configuration detected physical hardware presence. No resources claimed.
2. **`Probed` (1):** Driver `probe()` confirmed hardware responsiveness and device signature. Resources identified but not locked.
3. **`Attached` (2):** Driver successfully claimed exclusive device ownership; hardware resources registered in `RESOURCE_TABLE`.
4. **`Ready` (3):** Hardware fully initialized and idle. Ready to accept I/O, MMIO accesses, or command submission.
5. **`Active` (4):** Hardware actively processing I/O, DMA transfer, or interrupt sequence.
6. **`Quiescing` (5):** Teardown or reset initiated. New operations rejected; waiting for in-flight DMA/interrupts to drain or timeout.
7. **`Detached` (6):** Driver unbound, interrupt lines masked, MMIO unmapped, DMA buffers unpinned.
8. **`Released` (7):** Table slot scrubbed and available for allocation.
9. **`Faulted` (8):** Unrecoverable hardware error, timeout, or bus fault detected. Hardware operations halted.
10. **`Resetting` (9):** Hardware reset protocol underway.

---

## 3. Static Bounded Device Descriptors

ZeroOS enforces **zero dynamic kernel heap allocations**. All device descriptors reside in static `.bss` arrays:

```rust
pub const MAX_DEVICES: usize = 16;
pub const MAX_DEVICE_RESOURCES: usize = 64;
pub const MAX_INTERRUPT_VECTORS: usize = 256;
pub const MAX_DMA_BUFFERS: usize = 64;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceSlot {
    /// Slot occupancy flag.
    pub occupied: bool,
    /// 16-bit generation counter protecting against stale slot reuse.
    pub generation: u16,
    /// Authoritative 64-bit monotonic device ID.
    pub device_id: DeviceId,
    /// Architectural device class.
    pub class: DeviceClass,
    /// Current lifecycle state.
    pub state: DeviceLifecycleState,
    /// Process ID of driver owning this device (0 = Kernel Nucleus).
    pub driver_pid: u64,
    /// Associated KernelObject ID for capability referencing.
    pub kernel_object_id: u64,
    /// Bitmask of allocated resources in RESOURCE_TABLE.
    pub resource_mask: u64,
    /// Optional bound event object ID for interrupt signaling.
    pub bound_event_id: u64,
    /// Explicit padding for alignment.
    pub _pad: [u8; 6],
}
const _: () = assert!(core::mem::size_of::<DeviceSlot>() == 48);
const _: () = assert!(core::mem::align_of::<DeviceSlot>() == 8);
```

---

## 4. Hardware Resource Model & Conflict Rejection

Hardware resources are strongly typed and managed in a global static `RESOURCE_TABLE`:

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Unused    = 0,
    IoPort    = 1, // x86 I/O Port Range (e.g. 0x1F0..0x1F7)
    Mmio      = 2, // Physical Memory-Mapped I/O Window
    Irq       = 3, // Hardware IRQ line & mapped IDT vector
    DmaBuffer = 4, // Pinned physical memory buffer for DMA
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceSharing {
    Exclusive = 0, // Only one device can hold this resource (default)
    Shared    = 1, // Multiple devices can share (e.g. shared PCI IRQ, read-only MMIO)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceResourceSlot {
    pub occupied: bool,
    pub res_type: ResourceType,
    pub sharing: ResourceSharing,
    pub _pad: u8,
    pub device_id: DeviceId,
    /// Resource range start:
    /// - IoPort: port number (u16 as u64)
    /// - Mmio: physical base address (page aligned)
    /// - Irq: IRQ line number
    /// - DmaBuffer: physical frame start address
    pub base: u64,
    /// Resource range length:
    /// - IoPort: count of ports
    /// - Mmio: size in bytes (page aligned)
    /// - Irq: count of vectors (usually 1)
    /// - DmaBuffer: size in bytes
    pub size: u64,
}
const _: () = assert!(core::mem::size_of::<DeviceResourceSlot>() == 32);
const _: () = assert!(core::mem::align_of::<DeviceResourceSlot>() == 8);
```

### 4.1 Conflict Detection Invariant (`I-DEV-RES-CONFLICT-1`)
Before registering any resource for a device:
1. For `IoPort`: Range `[base, base + size)` must not intersect any existing `Exclusive` `IoPort` range.
2. For `Mmio`: Physical range `[base, base + size)` must not intersect:
   - Kernel code/data/BSS ranges.
   - PMM managed usable RAM bitmap ranges.
   - Any existing `Exclusive` `Mmio` range of another device.
3. For `Irq`: Exclusive IRQ lines cannot be shared with another device.
4. Any conflict causes `register_resource()` to abort immediately returning `DeviceError::ResourceConflict`. No partial allocations are committed.

---

## 5. Driver Boundary & Isolation

ZeroOS establishes an explicit, two-tier driver boundary:

```text
+-------------------------------------------------------------------------+
|                           USER WORKSPACE                                |
|                                                                         |
|   +--------------------------+        +--------------------------+      |
|   | User-Space Driver / App  |        |    ZeroFS File Client    |      |
|   | (Ring 3 Process)         |        |    (Ring 3 Process)      |      |
|   +------------+-------------+        +------------+-------------+      |
+----------------|-----------------------------------|--------------------+
                 | Syscall ABI (17..20)              | Syscall ABI (11..16)
+----------------v-----------------------------------v--------------------+
|                        ZEROOS KERNEL NUCLEUS                            |
|                                                                         |
|   +-----------------------------------------------------------------+   |
|   |                      CAPABILITY SYSTEM                          |   |
|   |     Verifies DEV_READ, DEV_WRITE, DEV_MAP_MMIO, DEV_DMA_ACQUIRE |   |
|   +--------------------------------+--------------------------------+   |
|                                    |                                    |
|   +--------------------------------v--------------------------------+   |
|   |                     DEVICE REGISTRY & HAL                       |   |
|   |   - Device State Machine          - MMIO Mapping Engine         |   |
|   |   - Resource Conflict Checker     - DMA Frame Pin Tracker       |   |
|   |   - Interrupt Dispatch Table      - Device Locks (Levels 5..6)  |   |
|   +----------------+-------------------------------+----------------+   |
|                    |                               |                    |
|   +----------------v---------------+ +-------------v----------------+   |
|   | Kernel Nucleus Drivers (Ring 0)| | ZeroFS Storage Subsystem     |   |
|   | - LAPIC / PIT Timers           | | - BlockDevice Trait          |   |
|   | - 8259 PIC / IOAPIC            | | - AtaPioBlockDevice (Dev 1)  |   |
|   | - Serial Console (0x3F8)       | | - MemDeviceWrapper  (Dev 0)  |   |
|   +----------------+---------------+ +-------------+----------------+   |
+--------------------|-------------------------------|--------------------+
                     | Port I/O / MMIO / IRQ         | Primary ATA Port I/O
+--------------------v-------------------------------v--------------------+
|                         PHYSICAL HARDWARE                               |
|   CPU / LAPIC / Timers / RAM / ATA Storage / Bus Controllers / Devices  |
+-------------------------------------------------------------------------+
```

### 5.1 Driver Execution Domains
1. **Kernel Nucleus Drivers (Ring 0):** Critical infrastructure required for early boot and system survival (timers, interrupt controllers, serial console, early ATA storage). They execute directly within kernel space and access HAL primitives directly.
2. **User-Space Drivers (Ring 3 Workloads):** Peripheral drivers (NICs, GPU command processors, NPU dispatchers, USB drivers) execute as user-space processes. They possess **zero direct hardware authority** by default. They interact with hardware exclusively via capability-mediated syscalls:
   - `SYS_DEV_QUERY` (17): Query device resources and status.
   - `SYS_DEV_MAP_MMIO` (18): Map validated MMIO pages into driver virtual address space with uncacheable attributes.
   - `SYS_DEV_DMA_ALLOC` (19): Allocate pinned physical frames and retrieve device-visible physical addresses.
   - `SYS_DEV_RESET` (20): Trigger a device reset sequence.

---

## 6. Interrupt Model & Dispatch Architecture

### 6.1 Vector Allocation Map
x86-64 IDT vectors (256 total) are partitioned deterministically:
- `0x00 .. 0x1F` (0..31): Architecture-defined CPU Exceptions (Double Fault, Page Fault, GPF, etc.).
- `0x20` (32): LAPIC Periodic Preemption Timer.
- `0x21 .. 0x2F` (33..47): Legacy ISA Hardware IRQs (PIC remapped / IOAPIC), e.g., Vector 46 = IRQ14 (Primary ATA).
- `0x30 .. 0xFE` (48..254): Dynamic Peripheral Device Vectors (PCI MSI / MSI-X / VirtIO).
- `0xFF` (255): APIC Spurious Interrupt Vector.

### 6.2 Interrupt Dispatch Table
```rust
pub struct InterruptBinding {
    pub active: bool,
    pub device_id: DeviceId,
    pub irq: u8,
    pub vector: u8,
    pub masked: bool,
    pub spurious_count: u32,
    pub interrupt_count: u64,
    /// Kernel top-half handler callback (optional).
    pub top_half: Option<fn(vector: u8, device_id: DeviceId)>,
    /// Associated Event object ID to signal on interrupt arrival.
    pub event_id: u64,
}
```

### 6.3 Top-Half / Bottom-Half Execution Model
1. **Top-Half ISR (Interrupt Context, IF=0):**
   - Saves register state (`InterruptFrame`).
   - Dispatches via `INTERRUPT_DISPATCH_TABLE[vector]`.
   - Sends Hardware End-of-Interrupt (EOI) to LAPIC (`lapic_eoi()`) and/or PIC.
   - If `event_id != 0`: Signals the bound `KernelObjectType::Event` object (`event.signal()`).
   - If `top_half` function pointer present: executes minimal top-half acknowledgment.
   - **Storm Mitigation Invariant (`I-DEV-IRQ-STORM-1`):** If a vector fires more than `MAX_IRQS_PER_TICK` (1,000) within a 10 ms timer tick without being serviced/cleared by the driver, the kernel automatically masks the vector at the interrupt controller and marks `masked = true`.
2. **Bottom-Half Driver Worker (Thread Context):**
   - The user or kernel driver thread waits on the bound Event handle via `SYS_EVENT_WAIT` or channel IPC.
   - Thread executes in regular scheduling context with preemption enabled.
   - Never acquires sleeping locks inside top-half interrupt context!

---

## 7. DMA Architecture & Physical Frame Isolation

### 7.1 DMA Memory Model
Without an active IOMMU, physical hardware devices write directly to system physical memory. ZeroOS strictly controls DMA to prevent device writes to arbitrary physical memory:

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DmaFrameDescriptor {
    pub occupied: bool,
    pub pinned: bool,
    pub device_id: DeviceId,
    pub owning_pid: u64,
    pub phys_addr: u64,
    pub frame_count: u32,
    pub ref_count: u32,
}
```

### 7.2 DMA Pinning Invariants
1. **PMM Isolation (`I-DEV-DMA-PIN-1`):** A physical frame allocated for DMA is marked `Allocated` in PMM and registered in `DMA_TRACKER`. While `pinned == true`, PMM will never reallocate or reclaim this frame.
2. **Device State Gating (`I-DEV-DMA-STATE-1`):** DMA frames can only be assigned to devices in `Attached` or `Ready` state.
3. **Driver Termination Cleanup (`I-DEV-DMA-CLEANUP-1`):** If a driver process exits while holding DMA buffers:
   - The device is immediately transitioned to `Quiescing`.
   - The kernel issues a device-level halt/reset or waits for in-flight DMA engines to stop.
   - Once the device reaches `Detached` or `Ready`, DMA frames are unpinned and returned to PMM.
   - A device never accesses a physical frame after that frame has been freed to PMM.

---

## 8. Capability Model Integration

Stage 3L directly integrates with the frozen Stage 3H Capability Subsystem (`KernelObjectType::Device = 6`).

### 8.1 Device-Specific Rights Mask
Bits 0..7 are defined specifically for `KernelObjectType::Device`:

```rust
pub mod dev_rights {
    pub const DEV_READ: u16             = 1 << 0; // 0x0001: Read device registers / status
    pub const DEV_WRITE: u16            = 1 << 1; // 0x0002: Write device registers / commands
    pub const DEV_CONTROL: u16          = 1 << 2; // 0x0004: Issue control & configuration commands
    pub const DEV_MAP_MMIO: u16         = 1 << 3; // 0x0008: Map MMIO windows into address space
    pub const DEV_DMA_ACQUIRE: u16      = 1 << 4; // 0x0010: Allocate / pin physical DMA buffers
    pub const DEV_INTERRUPT_LISTEN: u16 = 1 << 5; // 0x0020: Bind & receive interrupt events
    pub const DEV_RESET: u16            = 1 << 6; // 0x0040: Issue device reset sequence
    pub const DEV_ATTACH: u16           = 1 << 7; // 0x0080: Bind/unbind driver to device slot
}
```

Together with frozen Generic Management Rights (Bits 8..15):
`DUPLICATE (1 << 8)`, `TRANSFER (1 << 9)`, `REVOKE (1 << 10)`, `CLOSE (1 << 11)`, `INSPECT (1 << 12)`, `AUDIT (1 << 13)`.

### 8.2 Capability Delegation & Monotonic Rights Invariant (`I-DEV-CAP-MONOTONIC-1`)
- When deriving a child device capability via `SYS_CAP_DERIVE`:
  $$\text{child\_rights} \subseteq \text{parent\_rights} \iff (\text{child\_rights} \ \& \ \neg\text{parent\_rights}) == 0$$
- A process cannot grant `DEV_MAP_MMIO` or `DEV_DMA_ACQUIRE` if its own capability lacks those rights.
- Revocation of a parent capability recursively traverses the Capability Derivation Tree (CDT), revoking all descendant handles in all processes, unmapping MMIO windows, and tearing down device bindings.

---

## 9. Comprehensive Lock Hierarchy & Concurrency Design

### 9.1 Authoritative Lock Order
Stage 3L expands the frozen lock hierarchy to integrate device registry and resource management without inversion:

$$\begin{aligned}
\text{Level 1:} & \quad \text{FILESYSTEM\_LOCK} \\
\text{Level 2:} & \quad \text{STORAGE\_OBJECT\_TABLE\_LOCK} \\
\text{Level 3:} & \quad \text{BLOCK\_CACHE\_LOCK} \\
\text{Level 4:} & \quad \text{BLOCK\_DEVICE\_LOCK} \\
\text{Level 5:} & \quad \text{DEVICE\_REGISTRY\_LOCK} \\
\text{Level 6:} & \quad \text{DEVICE\_RESOURCE\_LOCK} \\
\text{Level 7:} & \quad \text{KERNEL\_OBJECT\_TABLE\_LOCK} \\
\text{Level 8:} & \quad \text{SCHEDULER.lock} \\
\text{Level 9:} & \quad \text{CPU (IF=0 / Interrupt Context)}
\end{aligned}$$

### 9.2 Operation Lock Acquisition Traces

#### Trace 1: Device Registration & Probe
1. Acquire `DEVICE_REGISTRY_LOCK` (Level 5).
2. Acquire `DEVICE_RESOURCE_LOCK` (Level 6).
3. Validate resources; register in `RESOURCE_TABLE`.
4. Release `DEVICE_RESOURCE_LOCK` (Level 6).
5. Allocate slot in `DEVICE_TABLE`; assign monotonic `DeviceId`.
6. Release `DEVICE_REGISTRY_LOCK` (Level 5).

#### Trace 2: User Process MMIO Mapping (`SYS_DEV_MAP_MMIO`)
1. Look up handle in `Process.handle_table` (Lock-free per-process or thread private).
2. Acquire `KERNEL_OBJECT_TABLE_LOCK` (Level 7) to validate capability generation and `DEV_MAP_MMIO` right. Release lock.
3. Acquire `DEVICE_REGISTRY_LOCK` (Level 5).
4. Acquire `DEVICE_RESOURCE_LOCK` (Level 6).
5. Verify MMIO physical range matches device descriptor.
6. Release `DEVICE_RESOURCE_LOCK` (Level 6).
7. Release `DEVICE_REGISTRY_LOCK` (Level 5).
8. Map physical pages into `AddressSpace` page tables.

#### Trace 3: Top-Half Device Interrupt Dispatch -> Event Signal -> Thread Wakeup
1. Hardware triggers IRQ; CPU enters ISR with `IF = 0` (Level 9).
2. Look up `INTERRUPT_DISPATCH_TABLE[vector]`.
3. Send hardware EOI to LAPIC / PIC.
4. If `event_id != 0`:
   - Signal Event primitive:
     - Acquire `KERNEL_OBJECT_TABLE_LOCK` (Level 7).
     - Update event signal state.
     - Release `KERNEL_OBJECT_TABLE_LOCK` (Level 7).
     - Acquire `SCHEDULER.lock` (Level 8) to wake blocked driver thread.
     - Release `SCHEDULER.lock` (Level 8).
5. `iretq` returns from interrupt.
*Proof of no lock inversion:* Level 9 $\to$ Level 7 $\to$ Level 8. Strictly monotonic!

#### Trace 4: Process Termination Cleanup / Driver Detach
1. Process exit initiates teardown.
2. Acquire `DEVICE_REGISTRY_LOCK` (Level 5).
3. For all devices owned by `driver_pid`:
   - Set device state to `Quiescing`.
   - Issue hardware stop / reset.
   - Acquire `DEVICE_RESOURCE_LOCK` (Level 6).
   - Unmap MMIO pages; unpin DMA frames.
   - Release `DEVICE_RESOURCE_LOCK` (Level 6).
   - Transition device state to `Detached`.
4. Release `DEVICE_REGISTRY_LOCK` (Level 5).
5. Reclaim process capabilities via `KERNEL_OBJECT_TABLE_LOCK` (Level 7).

---

## 10. User-Space Syscall ABI (Syscalls 17..20)

ZeroOS extends the frozen syscall table (1..16) with 4 device syscalls:

| Syscall # | Name | RAX | RDI (Arg 1) | RSI (Arg 2) | RDX (Arg 3) | R10 (Arg 4) | Required Capability Right |
|---|---|---|---|---|---|---|---|
| **17** | `SYS_DEV_QUERY` | 17 | `handle: u32` | `info_ptr: *mut DeviceInfo` | `size: u64` | — | `INSPECT (1 << 12)` |
| **18** | `SYS_DEV_MAP_MMIO` | 18 | `handle: u32` | `res_idx: u32` | `out_vaddr_ptr: *mut u64` | `flags: u64` | `DEV_MAP_MMIO (1 << 3)` |
| **19** | `SYS_DEV_DMA_ALLOC`| 19 | `handle: u32` | `frame_count: u32` | `out_phys_ptr: *mut u64` | `out_vaddr_ptr: *mut u64` | `DEV_DMA_ACQUIRE (1 << 4)` |
| **20** | `SYS_DEV_RESET` | 20 | `handle: u32` | `reset_flags: u32` | — | — | `DEV_RESET (1 << 6)` |

### Detailed Syscall Semantics
1. **`SYS_DEV_QUERY` (17):**
   - Validates user buffer pointers via `validate_user_slice_mut`.
   - Returns `DeviceInfo`: device class, state, resource counts, vendor/device IDs.
2. **`SYS_DEV_MAP_MMIO` (18):**
   - Validates handle and verifies `DEV_MAP_MMIO` right.
   - Validates resource index against `RESOURCE_TABLE`.
   - Maps physical range into driver's `AddressSpace` with `PAGE_WRITABLE | PAGE_NX | PAGE_PCD | PAGE_PWT` (Uncacheable).
   - Returns allocated virtual address.
3. **`SYS_DEV_DMA_ALLOC` (19):**
   - Validates handle and verifies `DEV_DMA_ACQUIRE` right.
   - Allocates `frame_count` physical frames from PMM.
   - Pins frames in `DMA_TRACKER` bound to `device_id` and `current_pid`.
   - Maps frames into driver's address space.
   - Returns physical address (for hardware programming) and virtual address (for driver CPU access).
4. **`SYS_DEV_RESET` (20):**
   - Validates handle and verifies `DEV_RESET` right.
   - Transitions device `Ready -> Resetting -> Ready`.
   - Halts active DMA, clears pending queues, resets hardware registers.

---

## 11. ZeroFS & Storage Integration

Stage 3K introduced `BlockDevice` trait, `MemDeviceWrapper` (ID 0), and `AtaPioBlockDevice` (ID 1). Stage 3L **completely preserves** this layer:
1. **Device Table Registration:** During Stage 3L initialization:
   - Device 0 (`MemDeviceWrapper`) is registered as `DeviceClass::Storage`, state `Ready`, resources: 256 physical RAM blocks.
   - Device 1 (`AtaPioBlockDevice`) is registered as `DeviceClass::Storage`, state `Ready`, resources: `IoPortRange(0x1F0..0x1F7)`, `IoPortRange(0x3F6)`.
2. **Locking Integration:** ZeroFS continues calling `dev::get_device(id)` under `BLOCK_DEVICE_LOCK` (Level 4). Level 4 is higher priority than `DEVICE_REGISTRY_LOCK` (Level 5). Storage operations never deadlock with general device management.
3. **Full Backward Compatibility:** ZeroFS tests 3K-A through 3K-T continue to run untouched.

---

## 12. Future GPU, NPU, and Network Compatibility

The Stage 3L architecture cleanly supports upcoming high-performance devices without requiring core model changes:

### 12.1 GPU Compatibility
- **Control & VRAM:** GPU BAR0 (MMIO registers) and BAR1 (VRAM aperture) are registered as `Mmio` resources. Driver maps BAR0 for command submissions and BAR1 for framebuffer/textures.
- **Command Queues:** Driver allocates pinned ring buffers via `SYS_DEV_DMA_ALLOC`.
- **Interrupts:** GPU MSI vector signals bound Event object; bottom-half thread handles command completion.

### 12.2 NPU Compatibility
- **Model Weights & Activations:** Large contiguous pinned DMA buffers allocated via `SYS_DEV_DMA_ALLOC`.
- **Inference Submission:** User process submits descriptor pointer into NPU MMIO doorbell.
- **Completion:** Interrupt vector triggers completion Event; zero-copy results read directly from pinned output buffer.

### 12.3 Network Compatibility
- **RX / TX Rings:** Circular descriptor rings allocated in pinned DMA memory.
- **Packet Buffers:** Buffer pool allocated via PMM and pinned in `DMA_TRACKER`.
- **Interrupt Coalescing:** Top-half handles link status and packet arrival; driver worker thread drains RX ring.

---

## 13. SMP Compatibility Contract (Stage 3N Preview)

Although Project Zero is currently single-core (BSP), Stage 3L is designed with the multi-core contract for Stage 3N:
1. **Per-Device Spinlocks:** Device state transitions are guarded by atomic spinlocks rather than global flags.
2. **Interrupt Affinity:** `InterruptBinding` reserves a `target_cpu: u32` field for routing via IOAPIC / MSI Redirection Tables.
3. **Cross-CPU Wakeups:** Signaling an Event on CPU $A$ bound to a thread running on CPU $B$ uses an Inter-Processor Interrupt (IPI) to trigger scheduler preemption on CPU $B$.
4. **MMIO TLB Shootdown:** When an MMIO window is unmapped on CPU $A$, an IPI shootdown ensures all CPU cores flush the corresponding TLB entries.

---

## 14. Security & Invariant Matrix

| Invariant ID | Name | Formulation / Description | Enforcement Point |
|---|---|---|---|
| `I-DEV-ID-UNIQUE` | DeviceId Monotonicity | $DeviceId_{k} > DeviceId_{k-1} \ge 1$ across kernel lifetime. | Atomic counter `NEXT_DEVICE_ID` |
| `I-DEV-RES-CONFLICT`| Exclusive Resource Overlap Rejection | For all exclusive $R_a, R_b$, $R_a \cap R_b = \emptyset$. | `register_resource()` validation |
| `I-DEV-MMIO-GUARD` | MMIO Isolation | Driver cannot map any physical memory outside its registered MMIO resource. | `SYS_DEV_MAP_MMIO` validation |
| `I-DEV-DMA-PIN` | DMA Frame Pinning | A DMA frame cannot be returned to PMM while device is `Active` or `Resetting`. | `DMA_TRACKER` pin check |
| `I-DEV-CAP-SUBSET` | Monotonic Capability Delegation | $\text{rights}_{child} \subseteq \text{rights}_{parent}$. | `SYS_CAP_DERIVE` |
| `I-DEV-IRQ-STORM` | Interrupt Storm Mitigation | If vector fires $> 1000$ times / tick without service, auto-mask. | IDT top-half ISR dispatcher |
| `I-DEV-TEARDOWN` | Safe Process Termination | Process exit synchronously halts DMA and unmaps MMIO before reclaiming memory. | Process exit / driver cleanup |
| `I-DEV-LOCK-ORDER` | Strict Lock Hierarchy | Lock acquisition must strictly follow Levels 1 through 9. | Compile-time & trace verification |

---

## 15. Verification Architecture (Machine-Level Tests 3L-A through 3L-T)

Stage 3L defines 20 bare-metal machine verification tests:

| Test ID | Name | Focus & Invariant Verified | Expected Outcome |
|---|---|---|---|
| **3L-A** | Device Table & Identity Monotonicity | Verify monotonic `DeviceId` assignment and slot recycling prevention. | `DeviceId(2) > DeviceId(1)`, `occupied == true` |
| **3L-B** | Device Discovery & Enumeration | Enumerate registered devices (MemDevice 0, AtaDevice 1, Platform 2). | All devices discovered with correct class |
| **3L-C** | Device Lifecycle Transitions | Step device through `Discovered -> Probed -> Attached -> Ready -> Active -> Ready`. | Legal transitions succeed; illegal rejected |
| **3L-D** | Resource Allocation (IoPort & Mmio) | Register distinct IoPort and MMIO ranges for mock devices. | Resources registered with correct base/size |
| **3L-E** | Resource Overlap Rejection | Attempt to register overlapping exclusive MMIO and Port ranges. | Deterministic `Err(ResourceConflict)` |
| **3L-F** | Capability Authorization & Rights | Verify `DEV_READ`, `DEV_WRITE`, `DEV_CONTROL` access gating. | Allowed operations succeed; unauthorized fail with `EACCES` |
| **3L-G** | MMIO Range Mapping & Uncacheable Paging | Map device MMIO window; verify `PAGE_PCD | PAGE_PWT | PAGE_NX`. | Page table bits validated; read/write matches |
| **3L-H** | Port-I/O Authority & Boundary | Validate kernel-mediated port I/O isolation. | Only authorized ports accessible |
| **3L-I** | Interrupt Registration & Vector Binding | Bind hardware IRQ line and vector in `INTERRUPT_DISPATCH_TABLE`. | Vector registered, masked=false |
| **3L-J** | Interrupt Top-Half Event Signaling | Fire mock interrupt; assert top-half signals bound `KernelObjectType::Event`. | Event state signaled; thread unblocks |
| **3L-K** | Interrupt Storm Mitigation | Fire $> 1000$ interrupts in tight loop; assert line auto-masks. | Line masked, system stays responsive |
| **3L-L** | DMA Buffer Allocation & PMM Pinning | Allocate DMA buffer; verify frame marked pinned in `DMA_TRACKER`. | Frame pinned; PMM alloc rejects frame |
| **3L-M** | DMA Isolation & Bounds Checking | Attempt DMA access out of bounds or to non-DMA physical address. | Operation rejected; memory untouched |
| **3L-N** | Device Fault State Transition | Inject hardware failure; assert transition to `Faulted` state. | State == `Faulted`; active operations halted |
| **3L-O** | Device Reset & Recovery | Issue reset on `Faulted` device; assert transition to `Resetting -> Ready`. | Device recovered and accepts operations |
| **3L-P** | Driver Detach & Teardown | Unbind driver; verify interrupt masked, MMIO unmapped, DMA unpinned. | All resources released cleanly |
| **3L-Q** | Process Exit Driver Cleanup | Terminate driver process while holding device resources; verify zero leaks. | Devices quiesced, memory freed, zero leaks |
| **3L-R** | Capability Revocation Cascade | Revoke root device capability; verify child capabilities and mappings revoked. | Descendant handles invalidated |
| **3L-S** | Concurrency & Monotonic Lock Ordering | Stress concurrent device queries, interrupts, and scheduler yields. | Zero deadlocks, zero lock inversion |
| **3L-T** | ZeroFS Storage Backward Compatibility | Execute full ZeroFS volume mount and file operations over 3L device registry. | 3K storage tests pass 100% |

---

## 16. Adversarial Architecture Review

Before submitting Rev1 for freeze approval, the architecture was subjected to 16 targeted adversarial stress tests:

1. **Stale `DeviceId` vs Table Slot Confusion:**
   - *Attack:* Thread stores table slot index $0$. Device is detached, slot $0$ is reallocated to new device. Thread invokes syscall using old slot index.
   - *Resolution:* Kernel APIs and syscalls accept `DeviceId(pub u64)` and capability handles, **never** raw slot indices. Every slot lookup validates `slot.device_id == expected_id` and `slot.generation == expected_generation`.
2. **Driver Teardown During Active Interrupt:**
   - *Attack:* Driver process exits while top-half ISR is executing on CPU.
   - *Resolution:* Driver detach first masks vector at interrupt controller, executes memory barrier, and verifies no ISR is in-flight before reclaiming driver structures.
3. **Device Removal During DMA:**
   - *Attack:* Device unplugged or reset while DMA write to physical RAM is in-flight.
   - *Resolution:* State machine forces `Quiescing` state. Hardware reset halts DMA engine before frames are unpinned.
4. **Process Exit During Device I/O:**
   - *Attack:* Owning process crashes while waiting for device completion.
   - *Resolution:* Process exit handler transitions device to `Quiescing`, aborts waiting threads, flushes device queues, and safely unpins DMA memory.
5. **Capability Close While Operation In Flight:**
   - *Attack:* Process closes device handle while background I/O operation is running.
   - *Resolution:* Handle closure decrements `handle_refs`. `KernelObjectHeader.in_flight_op_refs` holds the object alive until completion.
6. **IRQ Arriving After Detach:**
   - *Attack:* Spurious hardware IRQ arrives after driver detached.
   - *Resolution:* Interrupt dispatcher checks `INTERRUPT_DISPATCH_TABLE[vector].active`. If inactive, issues EOI and drops interrupt as spurious.
7. **DMA Buffer Freed While Device Owns It:**
   - *Attack:* User process attempts to free or unmap DMA buffer while device is `Active`.
   - *Resolution:* `SYS_SHM_UNMAP` or frame reclamation checks `DmaFrameDescriptor.pinned`. If pinned, call fails with `SyscallError::ResourceBusy`.
8. **Overlapping MMIO Resources:**
   - *Attack:* Malicious device driver attempts to claim LAPIC MMIO (`0xFEE0_0000`) or Kernel Code (`0x100000`).
   - *Resolution:* `register_resource()` checks range against reserved system ranges and existing devices. Conflict returns `DeviceError::ResourceConflict`.
9. **Overlapping IRQ Ownership:**
   - *Attack:* Two devices register vector 32 (LAPIC Timer).
   - *Resolution:* Vector 32 is permanently reserved. Exclusive vectors reject duplicate registration.
10. **Driver Accessing Another Device's Resources:**
    - *Attack:* Driver A queries Device B's MMIO address and attempts to map it.
    - *Resolution:* `SYS_DEV_MAP_MMIO` checks that the requested resource index is in the resource mask of the device bound to the caller's capability handle.
11. **Lock Inversion Through Wakeups:**
    - *Attack:* ISR signals Event, which wakes thread, which acquires `FILESYSTEM_LOCK` while holding scheduler lock.
    - *Resolution:* Top-half only moves thread to scheduler runqueue under `SCHEDULER.lock` (Level 8). Woken thread does not execute until after ISR returns.
12. **Device Fault While Holding Kernel Resources:**
    - *Attack:* Hardware hangs during ATA PIO read while holding `BLOCK_DEVICE_LOCK`.
    - *Resolution:* Bounded poll loops (`MAX_ATA_POLL_ITERATIONS`) timeout deterministically, set state to `Faulted`, release locks, and return `FsError::DeviceTimeout`.
13. **Interrupt-Context Acquisition of Sleeping Locks:**
    - *Attack:* ISR attempts to acquire `BLOCK_CACHE_LOCK` or allocate memory.
    - *Resolution:* Top-half handlers are prohibited from acquiring Locks 1–6 or calling PMM. Top-half only sends EOI and signals Event primitives.
14. **Resource Leaks After Failed Initialization:**
    - *Attack:* Device driver probe succeeds, but resource 3 of 4 fails conflict check.
    - *Resolution:* Transactional rollback: any failure during device attachment releases all previously allocated resources for that device before returning error.
15. **Interrupt Storm Denial of Service:**
    - *Attack:* Stuck hardware line fires millions of interrupts per second, starving scheduler.
    - *Resolution:* Hardware rate limiter auto-masks vector after 1000 interrupts in a single 10 ms tick.
16. **Capability Revocation During Active I/O:**
    - *Attack:* Administrator revokes device capability while device is actively DMAing.
    - *Resolution:* Revocation unmaps user MMIO pages and marks handle invalid; in-flight operations complete normally via `in_flight_op_refs`, then device transitions to `Ready`.

---

## 17. Architectural Deliverables Summary

1. `STAGE3L-ARCHITECTURE-REV1.md` (this document).
2. `ADR-0021-device-and-hardware-model.md` (formal architecture decision record).
3. Exact structure layouts: `DeviceSlot` (48 B), `DeviceResourceSlot` (32 B), `InterruptBinding` (32 B), `DmaFrameDescriptor` (32 B).
4. Frozen structures preserved: `Process` (128 B), `KernelThread` (176 B), `PerCpu` (48 B), `HandleTable` (520 B), `CapabilityNode` (24 B), `KernelObjectSlot` (40 B).
5. Exact constants, state machines, capability rights, and lock hierarchy defined.
6. 20 machine verification tests (`3L-A` through `3L-T`) defined.

---
*End of Stage 3L Architecture Specification Rev1.*
