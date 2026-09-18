# Project Zero — Stage 3L: Device / Hardware Model
## Architecture Specification Rev3 — Comprehensive Design & Adversarial Review

**Document Version:** 3.0.0  
**Status:** PROPOSED / READY FOR FREEZE (SURGICAL REVISION REV3)  
**Author:** Google DeepMind Advanced Agentic Coding Team  
**Date:** September 2026  
**Target Platform:** x86-64 (QEMU / Bare-Metal PC-AT, LAPIC, IOAPIC, 8259 PIC, ATA PIO, PCI/MMIO)

---

## Executive Summary & System Context

Stage 3L establishes the authoritative **Device and Hardware Model** for Project Zero (ZeroOS). Stages 3A through 3K are complete, machine-verified, and frozen:

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

Rev3 surgically resolves the three material blockers and three critical clarifications identified during Rev2 review:
1. **Dedicated MMIO Virtual Window & Collision Proof (`I-DEV-MMIO-1`):** Establishes the authoritative `DEVICE_MMIO_REGION` at `0x0000_6000_0000_0000 .. 0x0000_7000_0000_0000` (PML4 entries 192..223), completely isolated from Stage 3J code/data (`0x0020_0000`), SHM (`0x2000_0000`), user stack (`0x7F7F_...`), and guard pages (`0x7F80_...`).
2. **Shared IRQ Bounded Multi-Binding Model (`I-DEV-IRQ-OWNERSHIP-1`, `I-DEV-IRQ-DELIVERY-1`):** Formulates `INTERRUPT_BINDINGS: [[InterruptBinding; MAX_SHARED_IRQ_BINDINGS]; 256]` (`MAX_SHARED_IRQ_BINDINGS = 4`) supporting multiple concurrent device bindings per vector with broadcast top-half signaling and bottom-half status filtering.
3. **Two-Tier DMA Architecture: Logical Buffer ↔ Physical Frame Model (`I-DEV-DMA-2`):** Establishes `DMA_BUFFER_TABLE: [DmaBufferDescriptor; 32]` for logical multi-frame allocations (up to 16 frames / 64 KiB per buffer) bound to `PHYSICAL_FRAME_PIN_TABLE: [PhysicalFramePinRecord; 128]` with transactional partial-allocation rollback.
4. **Capability Rights Bit-Subset Semantics (`I-DEV-CAP-SUBSET-1`):** Strictly enforces $(\text{child\_rights} \ \& \ \neg\text{parent\_rights}) == 0$ rather than numeric comparison.
5. **Shared-Line IRQ Storm Mitigation & Fault Isolation:** Distinguishes line-level hardware storm masking from per-device fault attribution.
6. **Scoped Device Reset Authority Under Shared Ownership:** Restricts `SYS_DEV_RESET` to the registered `driver_pid` or sole owner (`handle_refs == 1`), rejecting non-driver client resets with `PermissionDenied`.
7. **Extended Machine Verification Suite:** Expands the test suite to **26 machine tests (`3L-A` through `3L-Z`)** incorporating collision rejection, shared IRQ bindings, multi-frame DMA, partial allocation rollback, shared storm isolation, and scoped reset tests.

---

## 1. Frozen ABI Constraints Preserved

The following foundational data structure sizes remain **strictly unmodified**:
```text
Process          = 128 bytes
KernelThread     = 176 bytes
PerCpu           = 48 bytes
HandleTable      = 520 bytes
CapabilityNode   = 24 bytes
KernelObjectSlot = 40 bytes
```
Zero dynamic kernel heap is required. All device tables, resource descriptors, interrupt dispatch bindings, and DMA trackers reside in static `.bss` arrays, fitting comfortably within the Stage 2F 2 MiB bootstrap mapping window (`__kernel_end <= 0xFFFFFFFF80200000`).

---

## 2. Dedicated Device MMIO Virtual Region & Collision Proof

### 2.1 Authoritative Virtual Address Space Partition
Stage 3I and Stage 3J establish the user virtual address layout (`kernel/src/syscall/pointer.rs`, `kernel/src/elf/types.rs`). Stage 3L partitions the canonical user space ($0 .. 2^{47}-1$) into mutually disjoint regions:

```text
+------------------------------------+ 0x0000_0000_0000_0000
|  USER_NULL_GUARD (2 MiB)           | (Unmapped)
+------------------------------------+ 0x0000_0000_0020_0000
|  USER_CODE_REGION (RX)             | ELF Text segment
+------------------------------------+ 0x0000_0000_0040_0000
|  USER_DATA_REGION (RW + NX)        | ELF Data / BSS / Heap
+------------------------------------+ 0x0000_0000_2000_0000
|  USER_SHM_REGION (RW/RO + NX)      | Stage 3G Shared Memory Objects
+------------------------------------+ 0x0000_0000_4000_0000
|  [RESERVED / FUTURE EXPANSION]     | Unallocated user space
+------------------------------------+ 0x0000_6000_0000_0000  <-- PML4[192]
|                                    |
|  DEVICE_MMIO_REGION (1 TiB)        | Dedicated Uncacheable MMIO Aperture
|  (PAGE_USER | PAGE_NX | PCD | PWT) | (Syscall 18 SYS_DEV_MAP_MMIO)
|                                    |
+------------------------------------+ 0x0000_7000_0000_0000  <-- PML4[224]
|  [RESERVED USER GAP]               | (Unmapped)
+------------------------------------+ 0x0000_7F7F_FFFB_F000
|  USER_STACK_GUARD (4 KiB)          | (Unmapped guard page)
+------------------------------------+ 0x0000_7F7F_FFFC_0000
|  USER_STACK_REGION (16 KiB)        | User execution stack (RW + NX)
+------------------------------------+ 0x0000_7F7F_FFFF_0000
|  [STACK TOP / ENV PADDING]         | Initial RSP
+------------------------------------+ 0x0000_7F80_0000_0000  <-- PML4[255]
|  USER_UPPER_GUARD (2 GiB)          | Unmapped canonical boundary guard
+------------------------------------+ 0x0000_8000_0000_0000
|  NON-CANONICAL ADDRESS GAP         | CPU Hardware Fault Window
+------------------------------------+ 0xFFFF_8000_0000_0000  <-- PML4[256]
|  KERNEL HIGHER-HALF HHDM & TEXT    | Stage 2F / 3A-3K Kernel Space
+------------------------------------+ 0xFFFF_FFFF_FFFF_FFFF
```

### 2.2 MMIO Collision Invariant (`I-DEV-MMIO-1`)
> **Invariant `I-DEV-MMIO-1`:** No ordinary user mapping, ELF segment, stack mapping, or future anonymous memory mapping may intersect `DEVICE_MMIO_REGION` (`[0x0000_6000_0000_0000, 0x0000_7000_0000_0000)`). Conversely, `SYS_DEV_MAP_MMIO` may only map physical MMIO ranges within `DEVICE_MMIO_REGION`.

### 2.3 Mathematical Disjointness Proof
1. $\text{USER\_CODE\_REGION} \cup \text{USER\_DATA\_REGION} \subseteq [0x0020\_0000, 0x2000\_0000)$.
2. $\text{USER\_SHM\_REGION} \subseteq [0x2000\_0000, 0x4000\_0000)$.
3. $\text{DEVICE\_MMIO\_REGION} = [0x6000\_0000\_0000, 0x7000\_0000\_0000)$.
4. $\text{USER\_STACK\_REGION} = [0x7F7F\_FFFC\_0000, 0x7F7F\_FFFF\_0000)$.
5. $\text{USER\_UPPER\_GUARD} = [0x7F80\_0000\_0000, 0x8000\_0000\_0000)$.

Since:
$$0x4000\_0000 < 0x6000\_0000\_0000 < 0x7000\_0000\_0000 < 0x7F7F\_FFFC\_0000$$
The intersection of `DEVICE_MMIO_REGION` with all other user regions is strictly the empty set $\emptyset$.

### 2.4 MMIO VMM Contract & Co-Lifetime Pinning (`I-DEV-MMIO-PIN-1`)
- **Page Alignment:** Base $P_{base}$ and size $P_{size}$ must be multiples of 4096 bytes.
- **Maximum Size:** `MAX_MMIO_MAP_SIZE = 16 MiB` (4,096 pages).
- **PTE Attributes:** `PAGE_PRESENT | PAGE_USER | PAGE_NX | PAGE_PWT | PAGE_PCD` (Strong Uncacheable `UC`, No-Execute). If capability has `DEV_WRITE`, `PAGE_WRITABLE` is set; otherwise read-only.
- **Pinning Invariant `I-DEV-MMIO-PIN-1`:** While an active MMIO mapping exists in any `AddressSpace` (`KernelObjectHeader.mapping_refs > 0`), the underlying `DeviceSlot` remains pinned and cannot be released or reused.
- **Teardown:** Process exit, capability revocation, or unmap zeroes PTEs, flushes TLB via `invlpg` (and IPI broadcast in SMP), and decrements `mapping_refs`.

---

## 3. Shared IRQ Multi-Binding Architecture

### 3.1 Bounded Dispatch Table
ZeroOS supports level-triggered PCI and shared message interrupts via a bounded 2D array:
```rust
pub const MAX_SHARED_IRQ_BINDINGS: usize = 4;
pub const MAX_INTERRUPT_VECTORS: usize = 256;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptBinding {
    pub occupied: bool,
    pub device_id: DeviceId,
    pub driver_pid: u64,
    pub event_object_id: u64,
    pub interrupt_count: u64,
    pub top_half: Option<fn(vector: u8, device_id: DeviceId)>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IrqLineState {
    pub active_bindings: u8,
    pub sharing: ResourceSharing, // Exclusive or Shared
    pub masked: bool,
    pub in_storm: bool,
    pub irq_count_current_tick: u32,
    pub cooldown_ticks_remaining: u32,
    pub total_line_interrupts: u64,
}

pub static mut IRQ_LINE_STATES: [IrqLineState; MAX_INTERRUPT_VECTORS] = ...;
pub static mut INTERRUPT_BINDINGS: [[InterruptBinding; MAX_SHARED_IRQ_BINDINGS]; MAX_INTERRUPT_VECTORS] = ...;
```

### 3.2 Authoritative IRQ Invariants
> **Invariant `I-DEV-IRQ-OWNERSHIP-1`:** Every active `InterruptBinding` slot belongs to exactly one live device, resource, and driver process ownership context (`device_id`, `driver_pid`, `event_object_id`).

> **Invariant `I-DEV-IRQ-DELIVERY-1`:** When a hardware interrupt vector $V$ triggers, the top-half dispatcher acknowledges the hardware interrupt controller (EOI) and dispatches to **every occupied binding slot** in `INTERRUPT_BINDINGS[V]`, signaling each registered Event object.

### 3.3 Acknowledgment & Filtering Protocol
1. **Top-Half ISR (`IF = 0`):**
   - Sends EOI to LAPIC / 8259 PIC.
   - For each slot $i \in 0..\text{MAX\_SHARED\_IRQ\_BINDINGS}$ where `occupied == true`:
     - Calls `event.signal()` on `event_object_id`.
     - Invokes `top_half(V, device_id)` if function pointer is registered.
     - Increments binding `interrupt_count`.
2. **Bottom-Half Driver Filter (Ring 3):**
   - Each driver thread unblocks from `SYS_EVENT_WAIT`.
   - The driver inspects its device's hardware status register via MMIO or port read.
   - If its device did not assert the interrupt: ignores the wakeup.
   - If its device asserted the interrupt: clears the device status bit, services the hardware FIFO/queue, and finishes processing.

### 3.4 Shared Line Storm Mitigation & Fault Isolation
- **Line-Level Masking:** If vector $V$ fires $\ge 1000$ times in a single 10 ms LAPIC timer tick, the line is auto-masked at the hardware controller, marked `in_storm = true`, and given a 50-tick (500 ms) cooldown (`I-DEV-IRQ-STORM-1`).
- **Fault Attribution:** All bound Event objects are signaled with a storm warning. If Driver A services and clears its device while Driver B fails to clear its device, Driver B's binding is marked `faulted` and detached, allowing vector $V$ to be unmasked for healthy devices.

---

## 4. Two-Tier DMA Model: Logical Buffer ↔ Physical Frame Tracking

### 4.1 Data Structure Definitions
```rust
pub const MAX_DMA_BUFFERS: usize = 32;
pub const MAX_FRAMES_PER_BUFFER: usize = 16;   // Max 64 KiB per logical buffer
pub const MAX_PINNED_DMA_FRAMES: usize = 128;  // Max 512 KiB total DMA memory pool

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DmaBufferDescriptor {
    pub occupied: bool,
    pub buffer_id: u32,
    pub device_id: DeviceId,
    pub owning_pid: u64,
    pub frame_count: u16,
    pub sharing: ResourceSharing,
    pub _pad: u8,
    pub phys_base: u64,
    pub user_virt_addr: u64,
    pub frame_indices: [u16; MAX_FRAMES_PER_BUFFER],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicalFramePinRecord {
    pub occupied: bool,
    pub phys_addr: u64,
    pub pin_count: u16,
    pub owning_buffer_mask: u32,
}
```

### 4.2 DMA Invariants
> **Invariant `I-DEV-DMA-1`:** A physical frame with DMA ownership cannot transition to `Free` in PMM until all DMA owners have released it (`pin_count == 0`) and the device is confirmed stopped (`state != Active && state != Resetting`).

> **Invariant `I-DEV-DMA-2`:** A physical frame cannot become `Free` in PMM while any active `DmaBufferDescriptor` or device operation retains ownership of that frame.

### 4.3 Transactional Allocation Rollback
When allocating a multi-frame buffer via `SYS_DEV_DMA_ALLOC(frame_count)`:
1. Validate `frame_count <= MAX_FRAMES_PER_BUFFER`.
2. Allocate a free slot in `DMA_BUFFER_TABLE`.
3. Allocate $N$ physical frames from PMM sequentially.
4. **Failure Case:** If frame $K < N$ fails due to PMM exhaustion:
   - The allocator immediately triggers **transactional rollback**:
   - For all frames $0..K-1$, decrements `pin_count` in `PHYSICAL_FRAME_PIN_TABLE` to 0, unmaps pages, and returns frames to PMM via `pmm.free_frame()`.
   - The `DmaBufferDescriptor` slot is cleared.
   - Syscall returns `Err(SyscallError::OutOfMemory)`. Net frame leak is strictly zero.

---

## 5. Scoped Device Reset Under Shared Ownership

### 5.1 Reset Authority Invariant (`I-DEV-RESET-AUTH-1`)
> **Invariant `I-DEV-RESET-AUTH-1`:** Invoking `SYS_DEV_RESET` requires `DEV_RESET` capability right **AND** the caller must be the registered `driver_pid` of the device (or the sole client holding `handle_refs == 1`). A non-driver client sharing a device cannot reset the physical device.

### 5.2 Deterministic Hardware Reset Sequence
When authorized reset executes:
```text
Ready / Faulted
      ↓
[1. Quiesce Device]          (Reject new I/O submissions with ResourceBusy)
      ↓
[2. Stop DMA Engines]        (Wait for in-flight DMA or issue controller abort)
      ↓
[3. Mask Interrupt Line]     (Prevent spurious IRQs during register re-init)
      ↓
[4. Hardware Reset Pulse]    (Write reset register; clear FIFOs and state)
      ↓
[5. Re-program Resources]    (Restore MMIO base, DMA descriptor rings, unmask IRQ)
      ↓
Ready
```

---

## 6. Capability Rights Bit-Subset Semantics

### 6.1 Bit-Subset Invariant (`I-DEV-CAP-SUBSET-1`)
> **Invariant `I-DEV-CAP-SUBSET-1`:** Capability derivation via `SYS_CAP_DERIVE` enforces strict bitwise subset containment:
> $$(\text{child\_rights} \ \& \ \neg\text{parent\_rights}) == 0 \iff \text{child\_rights} \subseteq \text{parent\_rights}$$

Attempting to derive a child capability with any right bit not present in the parent capability fails immediately with `SyscallError::PermissionDenied`.

---

## 7. User-Space Syscall ABI (Syscalls 17..21)

| Syscall # | Name | RAX | RDI (Arg 1) | RSI (Arg 2) | RDX (Arg 3) | R10 (Arg 4) | Required Capability Right |
|---|---|---|---|---|---|---|---|
| **17** | `SYS_DEV_QUERY` | 17 | `handle: u32` | `info_ptr: *mut DeviceInfo` | `size: u64` | — | `INSPECT (1 << 12)` |
| **18** | `SYS_DEV_MAP_MMIO` | 18 | `handle: u32` | `res_idx: u32` | `out_vaddr_ptr: *mut u64` | `flags: u64` | `DEV_MAP_MMIO (1 << 3)` |
| **19** | `SYS_DEV_DMA_ALLOC`| 19 | `handle: u32` | `frame_count: u32` | `out_phys_ptr: *mut u64` | `out_vaddr_ptr: *mut u64` | `DEV_DMA_ACQUIRE (1 << 4)` |
| **20** | `SYS_DEV_RESET` | 20 | `handle: u32` | `reset_flags: u32` | — | — | `DEV_RESET (1 << 6)` + `driver_pid` check |
| **21** | `SYS_DEV_BIND_IRQ` | 21 | `dev_handle: u32` | `res_idx: u32` | `event_handle: u32` | — | `DEV_INTERRUPT_LISTEN (1 << 5)` |

---

## 8. Monotonic Lock Hierarchy

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

---

## 9. Comprehensive Machine Verification Suite (26 Tests: 3L-A through 3L-Z)

| Test ID | Name | Test Focus & Verification Invariant |
|---|---|---|
| **3L-A** | Device Table & Identity Monotonicity | `DeviceId(2) > DeviceId(1)`, bootstrap IDs 0 and 1 preserved. |
| **3L-B** | Device Discovery & Enumeration | Discovers MemDevice (0), AtaDevice (1), and Platform mock devices. |
| **3L-C** | Device Lifecycle Transitions | Steps through `Discovered -> Probed -> Attached -> Ready -> Active -> Ready`. |
| **3L-D** | Resource Allocation (IoPort & Mmio) | Registers distinct IoPort and MMIO ranges without conflict. |
| **3L-E** | Resource Overlap Rejection | Rejects overlapping exclusive MMIO/Port allocations (`I-DEV-RES-CONFLICT-1`). |
| **3L-F** | Capability Authorization & Rights | Enforces bit-subset rights verification (`I-DEV-CAP-SUBSET-1`). |
| **3L-G** | MMIO Range Mapping & Uncacheable Paging | Validates `PAGE_PCD | PAGE_PWT | PAGE_NX` and user-space read/write fidelity. |
| **3L-H** | Port-I/O Authority & Boundary | Confirms kernel-mediated port I/O isolation. |
| **3L-I** | Interrupt Registration & Binding (`SYS_DEV_BIND_IRQ`) | Binds IRQ to vector and Event object ID. |
| **3L-J** | Interrupt Top-Half Event Signaling | Simulates IRQ; confirms top-half signals Event and wakes thread. |
| **3L-K** | Interrupt Storm Mitigation | Injects 1,000+ interrupts in 10 ms; verifies auto-masking and cooldown (`I-DEV-IRQ-STORM-1`). |
| **3L-L** | DMA Buffer Allocation & PMM Pinning | Allocates DMA buffer; verifies frame marked pinned in `PHYSICAL_FRAME_PIN_TABLE` (`I-DEV-DMA-1`). |
| **3L-M** | DMA Isolation & Bounds Checking | Verifies physical address boundaries and memory isolation. |
| **3L-N** | Device Fault State Transition | Injects hardware fault; verifies transition to `Faulted` state. |
| **3L-O** | Device Reset & Recovery | Resets `Faulted` device; verifies transition to `Resetting -> Ready`. |
| **3L-P** | Driver Detach & Teardown | Unbinds driver; verifies vector masked, MMIO unmapped, DMA unpinned. |
| **3L-Q** | Process Exit Shared Device Teardown | Terminates client process; asserts shared device remains `Ready` (`I-DEV-LIFETIME-1`). |
| **3L-R** | Capability Revocation Cascade | Revokes device capability; verifies descendant handles and MMIO unmapped. |
| **3L-S** | Concurrency & Monotonic Lock Ordering | Stresses concurrent device queries, interrupts, and scheduler preemption. |
| **3L-T** | ZeroFS Storage Backward Compatibility | Mounts ZeroFS volume and performs file read/write across 3L device layer. |
| **3L-U** | MMIO/User-Address Collision Rejection | Asserts MMIO mapping into `USER_CODE_BASE`, `USER_STACK`, or `USER_UPPER_GUARD` is strictly rejected (`I-DEV-MMIO-1`). |
| **3L-V** | Multiple Bindings on Shared IRQ | Binds two distinct devices/events to one shared IRQ vector; triggers IRQ; verifies both receive notification (`I-DEV-IRQ-DELIVERY-1`). |
| **3L-W** | Multi-Frame DMA Ownership | Allocates 4-frame / 16 KiB buffer; asserts `DmaBufferDescriptor` tracks all 4 frames in `PHYSICAL_FRAME_PIN_TABLE` (`I-DEV-DMA-2`). |
| **3L-X** | DMA Partial-Allocation Rollback | Injects PMM exhaustion on frame 3 of 4; asserts frames 0..2 are transactionally freed with zero net leaks. |
| **3L-Y** | Shared IRQ Storm Isolation | Triggers storm on shared vector; asserts line-level storm masking and per-device fault attribution. |
| **3L-Z** | Reset with Shared Device Ownership | Verifies non-driver client rejected with `PermissionDenied` when attempting `SYS_DEV_RESET` on shared device; driver succeeds (`I-DEV-RESET-AUTH-1`). |

---
*End of Stage 3L Architecture Specification Rev3.*
