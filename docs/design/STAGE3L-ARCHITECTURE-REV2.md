# Project Zero — Stage 3L: Device / Hardware Model
## Architecture Specification Rev2 — Comprehensive Design & Adversarial Review

**Document Version:** 2.0.0  
**Status:** PROPOSED / READY FOR FREEZE REVIEW (SURGICAL REVISION REV2)  
**Author:** Google DeepMind Advanced Agentic Coding Team  
**Date:** September 2026  
**Target Platform:** x86-64 (QEMU / Bare-Metal PC-AT, LAPIC, IOAPIC, 8259 PIC, ATA PIO, PCI/MMIO)

---

## Executive Summary & System Context

Stage 3L establishes the authoritative **Device and Hardware Model** for Project Zero (ZeroOS). Stages 3A through 3K are complete, verified, and frozen:

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

Rev2 surgically resolves the four material blockers and four critical clarifications identified during the Rev1 architecture review:
1. **PMM ↔ DMA Pinning Invariant (`I-DEV-DMA-1`):** Enforces that physical frames allocated for DMA are pinned in PMM (`Allocated` state) and cannot transition to `Free` until all DMA owners release them and the physical device is confirmed stopped.
2. **Shared-Device Process-Exit Cleanup (`I-DEV-LIFETIME-1`):** Delineates process-owned DMA/resources from device ownership; process termination cancels process-owned operations and frees process-owned DMA, but physical devices remain active if other live owners hold valid capabilities.
3. **Complete User-Space Interrupt Delivery ABI (`SYS_DEV_BIND_IRQ`):** Formalizes the unbroken chain from hardware IRQ/vector to device, interrupt binding, kernel Event primitive, and user-space driver thread.
4. **KernelObjectSlot ↔ DeviceSlot Co-Lifetime Invariant (`I-DEV-OBJ-PIN-1`):** Binds the DeviceSlot's existence to the KernelObjectSlot's reference count, eliminating stale slot references and ABA aliasing without modifying the frozen 40-byte `KernelObjectSlot` ABI.
5. **Reserved Bootstrap DeviceIds:** Explicitly defines `DeviceId(0)` for MemDevice and `DeviceId(1)` for ATA Device, with dynamic monotonic allocation starting at `DeviceId(2)`.
6. **Resource-Sharing Compatibility Matrix:** Formally defines deterministic overlap and sharing compatibility across `IoPort`, `Mmio`, `Irq`, and `DmaBuffer`.
7. **Exact MMIO ↔ VMM Mapping Contract:** Formulates physical range validation, kernel-selected virtual addresses, uncacheable (`PWT | PCD`) paging, `PAGE_NX`, and unmapping teardown.
8. **Deterministic Interrupt Storm Mitigation:** Defines exact tick-based windowing (10 ms LAPIC tick), per-vector counting, cooldown semantics, and shared-IRQ handling.

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

## 2. Device Identity & Reserved Bootstrap IDs

### 2.1 Identity Representation
A device's authoritative identity is a 64-bit integer:
```rust
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u64);
```

### 2.2 Bootstrap Reservations & Monotonic Allocation
To maintain backward compatibility with Stage 3K ZeroFS while guaranteeing global monotonicity for all dynamic devices:
- `BOOTSTRAP_MEM_DEVICE_ID = DeviceId(0)`: Reserved for in-memory mock block volume (`MemDeviceWrapper`).
- `BOOTSTRAP_ATA_DEVICE_ID = DeviceId(1)`: Reserved for Primary Master ATA controller (`AtaPioBlockDevice`).
- `NEXT_DEVICE_ID = AtomicU64::new(2)`: All subsequent dynamically discovered or registered devices (PCI devices, NICs, GPUs, NPUs, USB controllers, hotplug devices) are allocated strictly monotonically starting at `DeviceId(2)`.

**Authoritative Invariants:**
- For all dynamically allocated devices $k$, $DeviceId_k \ge 2$ and $DeviceId_k > DeviceId_{k-1}$.
- `DeviceId` is decoupled from ephemeral table slot indices (`0..MAX_DEVICES - 1`), PCI topology addresses, and process capability handles.

---

## 3. Device Lifecycle & Shared Device Ownership

### 3.1 Device Classes (`DeviceClass`)
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

### 3.2 State Machine
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

### 3.3 Ownership Hierarchy & Process-Exit Semantics
ZeroOS distinguishes between five ownership concepts:
1. **Driver-Owned Device:** The primary controlling entity (`driver_pid: u64`, 0 = Kernel Nucleus) holding `DEV_ATTACH` authority.
2. **Shared Device Client:** Multiple processes holding capability handles to the device (e.g. Process A and Process B sharing a NIC or GPU).
3. **Process-Owned Device Resource:** Resources uniquely allocated to a specific process (e.g., Process A's private MMIO mapping).
4. **Process-Owned DMA Buffer:** Pinned DMA physical frames allocated on behalf of a specific PID.
5. **Shared Device:** A hardware peripheral operating concurrently for multiple client processes.

#### Process Termination Invariant (`I-DEV-LIFETIME-1`)
> **Invariant `I-DEV-LIFETIME-1`:** Process termination cannot transition a physical device to `Quiescing` while another live owner holds valid capability authority over that device.

#### Process Exit Teardown Sequence:
When Process $A$ terminates:
1. **Revoke Process Authority:** Close Process $A$'s capability handles to the device; decrement `KernelObjectHeader.handle_refs`.
2. **Cancel Process Operations:** Abort all pending operations and waitqueues tagged with `PID == A`.
3. **Halt Process DMA:** Locate all DMA buffers where `owning_pid == A`. Halt Process $A$'s hardware queues; unpin frames and return them to PMM once DMA stops.
4. **Unmap Process MMIO:** Unmap device MMIO pages from Process $A$'s `AddressSpace` and flush TLB.
5. **Physical Device State Evaluation:**
   - If other live processes still hold open capability handles (`handle_refs > 0`):
     - The physical device **REMAINS IN `Ready` / `Active` STATE**.
     - Process $B$'s operations and DMA transfers continue completely uninterrupted!
   - Only if Process $A$ was the sole driver-owner (`driver_pid == A`) **AND** `handle_refs == 0`:
     - The device transitions `Ready -> Quiescing -> Detached`.

---

## 4. KernelObjectSlot ↔ DeviceSlot Co-Lifetime Model

To bind `KernelObjectType::Device = 6` to `DEVICE_TABLE` without mutating the frozen 40-byte `KernelObjectSlot`:

```rust
pub const MAX_DEVICES: usize = 16;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceSlot {
    pub occupied: bool,
    pub generation: u16,
    pub device_id: DeviceId,
    pub class: DeviceClass,
    pub state: DeviceLifecycleState,
    pub driver_pid: u64,
    pub kernel_object_id: u64,
    pub resource_mask: u64,
    pub bound_event_id: u64,
    pub _pad: [u8; 6],
}
const _: () = assert!(core::mem::size_of::<DeviceSlot>() == 48);
const _: () = assert!(core::mem::align_of::<DeviceSlot>() == 8);
```

### 4.1 Co-Lifetime & Pinning Invariant (`I-DEV-OBJ-PIN-1`)
> **Invariant `I-DEV-OBJ-PIN-1`:** A `DeviceSlot` cannot be released, cleared, or recycled (`state = Released`, `occupied = false`) while an associated `KernelObjectSlot` has an active reference count (`handle_refs + mapping_refs + in_flight_op_refs > 0`).

### 4.2 Stale Slot & Generation Validation Trace
When resolving a device capability handle:
1. Inspect `KernelObjectSlot` at caller's handle reference.
2. Assert `KernelObjectSlot.occupied == true` and `KernelObjectSlot.obj_type == KernelObjectType::Device`.
3. Let `slot_idx = KernelObjectSlot.pool_index as usize`. Assert `slot_idx < MAX_DEVICES`.
4. Inspect `DEVICE_TABLE[slot_idx]`:
   - `DEVICE_TABLE[slot_idx].occupied == true`
   - `DEVICE_TABLE[slot_idx].kernel_object_id == KernelObjectSlot.header.object_id`
   - `DEVICE_TABLE[slot_idx].generation == KernelObjectSlot.generation`
5. If any condition fails, the request is rejected with `SyscallError::BadHandle`. Zero ABA slot reuse is possible.

---

## 5. DMA Architecture & PMM Pinning Invariants

### 5.1 DMA Frame Tracking
```rust
pub const MAX_DMA_BUFFERS: usize = 64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmaBufferState {
    Free       = 0,
    Allocating = 1,
    Bound      = 2,
    Quiescing  = 3,
    Released   = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DmaFrameDescriptor {
    pub occupied: bool,
    pub state: DmaBufferState,
    pub pin_count: u16,
    pub _pad: u32,
    pub device_id: DeviceId,
    pub owning_pid: u64,
    pub phys_addr: u64,
    pub frame_count: u32,
}
const _: () = assert!(core::mem::size_of::<DmaFrameDescriptor>() == 32);
const _: () = assert!(core::mem::align_of::<DmaFrameDescriptor>() == 8);
```

### 5.2 PMM ↔ DMA Pinning Invariant (`I-DEV-DMA-1`)
> **Invariant `I-DEV-DMA-1`:** A physical frame with DMA ownership cannot transition to `Free` in PMM until all DMA owners have released it (`pin_count == 0`) and the device is confirmed stopped (`state != Active && state != Resetting`).

### 5.3 Pinning & Reclamation Rules
1. **Multi-Pin Support:** `pin_count: u16` allows multiple DMA descriptors (e.g. TX and RX scatter-gather rings) to reference the same physical buffer safely.
2. **Transactional Allocation Rollback:**
   - If allocating a multi-frame DMA buffer ($N$ frames) fails halfway through (e.g. frame $K < N$ fails with `OutOfMemory`):
     - The allocator rolls back transactionally: all $K$ already allocated frames are immediately returned to PMM via `pmm.free_frame()`.
     - The DMA descriptor is cleared.
     - Operation returns `Err(SyscallError::OutOfMemory)`. Zero frames are leaked.
3. **Reclamation Precondition:**
   A frame $f$ is freed to PMM if and only if:
   $$\text{pin\_count}(f) == 0 \quad \land \quad \text{DeviceState} \in \{\text{Ready}, \text{Detached}, \text{Released}\}$$

---

## 6. Resource Model & Sharing Compatibility Matrix

Hardware resources reside in a static `RESOURCE_TABLE: [DeviceResourceSlot; MAX_DEVICE_RESOURCES]` (`MAX_DEVICE_RESOURCES = 64`).

### 6.1 Compatibility Matrix
Two resource ranges $[A_{start}, A_{start} + A_{size})$ and $[B_{start}, B_{start} + B_{size})$ intersect if $\max(A_{start}, B_{start}) < \min(A_{end}, B_{end})$.

| Resource Type | New Registration | Existing Registration | Overlap Outcome | Architectural Rationale |
|---|---|---|---|---|
| **`IoPort`** | `Exclusive` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Port collisions corrupt controller command/data registers |
| **`IoPort`** | `Exclusive` | `Shared` | ❌ **REJECT (`ResourceConflict`)** | Exclusive access demands total control of port range |
| **`IoPort`** | `Shared` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Existing exclusive holder cannot be shared |
| **`IoPort`** | `Shared` | `Shared` | ❌ **REJECT (`ResourceConflict`)** | x86 I/O ports have no hardware sharing logic in ZeroOS |
| **`Mmio`** | `Exclusive` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Device control registers (BAR0) must be exclusively owned |
| **`Mmio`** | `Exclusive` | `Shared` | ❌ **REJECT (`ResourceConflict`)** | Exclusive holder conflicts with shared mapping |
| **`Mmio`** | `Shared` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Exclusive holder cannot be shared |
| **`Mmio`** | `Shared` | `Shared` | ✅ **PERMITTED** | Read-only MMIO apertures (e.g. ACPI tables, ROMs, framebuffer) |
| **`Irq`** | `Exclusive` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Dedicated IRQ line cannot be shared |
| **`Irq`** | `Exclusive` | `Shared` | ❌ **REJECT (`ResourceConflict`)** | Cannot mix exclusive and shared IRQs |
| **`Irq`** | `Shared` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Cannot share an already exclusive IRQ |
| **`Irq`** | `Shared` | `Shared` | ✅ **PERMITTED** | PCI Level-triggered IRQ sharing / MSI multi-message routing |
| **`DmaBuffer`**| `Exclusive` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Independent DMA engines cannot share uncoordinated buffers |
| **`DmaBuffer`**| `Exclusive` | `Shared` | ❌ **REJECT (`ResourceConflict`)** | Conflict |
| **`DmaBuffer`**| `Shared` | `Exclusive` | ❌ **REJECT (`ResourceConflict`)** | Conflict |
| **`DmaBuffer`**| `Shared` | `Shared` | ✅ **PERMITTED** | Shared ring buffer (e.g. Producer-Consumer GPU/CPU SHM) |

---

## 7. MMIO ↔ VMM Mapping Contract

When a user-space driver invokes `SYS_DEV_MAP_MMIO` (Syscall 18):

### 7.1 Unbroken Authority Chain
$$\text{Handle} \xrightarrow{\text{DEV\_MAP\_MMIO}} \text{CapabilityNode} \xrightarrow{\text{CDT}} \text{KernelObjectSlot} \xrightarrow{\text{pool\_index}} \text{DeviceSlot} \xrightarrow{\text{resource\_mask}} \text{DeviceResourceSlot} \xrightarrow{\text{phys\_range}} \text{VMM}$$

### 7.2 Exact Paging Specifications
1. **Physical Range Validation:**
   - Physical base address $P_{base}$ must be 4096-byte aligned ($P_{base} \pmod{4096} == 0$).
   - Size $P_{size}$ must be a positive 4096-byte multiple ($P_{size} \pmod{4096} == 0$, $P_{size} > 0$).
   - Maximum mapping size: `MAX_MMIO_MAP_SIZE = 16 MiB` ($4096$ frames).
   - Range $[P_{base}, P_{base} + P_{size})$ must not intersect kernel text, data, BSS, or PMM usable RAM.
2. **Kernel-Selected Virtual Address:**
   - To prevent user address collisions, the kernel assigns addresses from the dedicated user MMIO aperture:
     `USER_MMIO_VIRT_START = 0x0000_7000_0000_0000` to `USER_MMIO_VIRT_END = 0x0000_7FFF_FFFF_FFFF`.
3. **PTE Attributes:**
   - `PAGE_PRESENT` (Bit 0 = 1)
   - `PAGE_USER` (Bit 2 = 1) (Ring 3 accessible)
   - `PAGE_WRITABLE` (Bit 1 = 1 if capability has `DEV_WRITE`, else Bit 1 = 0)
   - `PAGE_NX` (Bit 63 = 1) (No-Execute: executing instructions from MMIO is strictly prohibited)
   - `PAGE_PWT` (Bit 3 = 1) & `PAGE_PCD` (Bit 4 = 1) (Strong Uncacheable `UC` memory type: disables CPU L1/L2/L3 caching of hardware registers)
4. **Teardown & Unmapping:**
   - On handle close, capability revocation, or process termination:
     - All user PTEs in range are zeroed.
     - TLB is invalidated locally via `invlpg` (and broadcast via IPI in SMP).
     - `KernelObjectHeader.mapping_refs` is decremented.

---

## 8. Interrupt Architecture & Storm Mitigation

### 8.1 Vector Map
- `0x00 .. 0x1F` (0..31): Architecture-defined CPU Exceptions.
- `0x20` (32): LAPIC Periodic Preemption Timer.
- `0x21 .. 0x2F` (33..47): Legacy ISA Hardware IRQs (PIC remapped / IOAPIC), e.g. Vector 46 = IRQ14 (Primary ATA).
- `0x30 .. 0xFE` (48..254): Dynamic Peripheral Device Vectors (PCI MSI / MSI-X / VirtIO).
- `0xFF` (255): APIC Spurious Interrupt Vector.

### 8.2 Dispatch Structure
```rust
pub struct InterruptBinding {
    pub active: bool,
    pub device_id: DeviceId,
    pub irq: u8,
    pub vector: u8,
    pub masked: bool,
    pub in_storm: bool,
    pub irq_count_current_tick: u32,
    pub cooldown_ticks_remaining: u32,
    pub total_interrupts: u64,
    pub event_object_id: u64,
    pub top_half: Option<fn(vector: u8, device_id: DeviceId)>,
}
```

### 8.3 Interrupt Storm Mitigation Contract (`I-DEV-IRQ-STORM-1`)
1. **Measurement Window:** Exactly 1 scheduler timer tick (10 ms LAPIC periodic tick).
2. **Threshold:** `MAX_IRQS_PER_TICK = 1000`.
3. **Storm Trigger:**
   If `irq_count_current_tick >= MAX_IRQS_PER_TICK`:
   - Top-half ISR immediately masks the hardware vector at LAPIC/IOAPIC/PIC.
   - Sets `masked = true` and `in_storm = true`.
   - Sets `cooldown_ticks_remaining = STORM_COOLDOWN_TICKS` (50 ticks = 500 ms).
   - Issues hardware EOI so the controller does not hang.
4. **Tick Reset:** At every 10 ms LAPIC timer interrupt, `irq_count_current_tick` is reset to 0 for all vectors. If `cooldown_ticks_remaining > 0`, it is decremented.
5. **Recovery / Unmasking:**
   When `cooldown_ticks_remaining` reaches 0:
   - Kernel or driver invokes `SYS_DEV_RESET` or IRQ acknowledge.
   - Line is unmasked at the controller and `in_storm = false`.
6. **Shared IRQ Semantics:**
   If an IRQ line is shared by multiple devices, all devices bound to that vector have their bound Event objects signaled with a storm alert flag, and the shared line is masked until the offending device is serviced.

---

## 9. User-Space Syscall ABI (Syscalls 17..21)

ZeroOS extends the frozen syscall table (1..16) with 5 device syscalls:

| Syscall # | Name | RAX | RDI (Arg 1) | RSI (Arg 2) | RDX (Arg 3) | R10 (Arg 4) | Required Capability Right |
|---|---|---|---|---|---|---|---|
| **17** | `SYS_DEV_QUERY` | 17 | `handle: u32` | `info_ptr: *mut DeviceInfo` | `size: u64` | — | `INSPECT (1 << 12)` |
| **18** | `SYS_DEV_MAP_MMIO` | 18 | `handle: u32` | `res_idx: u32` | `out_vaddr_ptr: *mut u64` | `flags: u64` | `DEV_MAP_MMIO (1 << 3)` |
| **19** | `SYS_DEV_DMA_ALLOC`| 19 | `handle: u32` | `frame_count: u32` | `out_phys_ptr: *mut u64` | `out_vaddr_ptr: *mut u64` | `DEV_DMA_ACQUIRE (1 << 4)` |
| **20** | `SYS_DEV_RESET` | 20 | `handle: u32` | `reset_flags: u32` | — | — | `DEV_RESET (1 << 6)` |
| **21** | `SYS_DEV_BIND_IRQ` | 21 | `dev_handle: u32` | `res_idx: u32` | `event_handle: u32` | — | `DEV_INTERRUPT_LISTEN (1 << 5)` |

### Complete Interrupt Binding Lifecycle (`SYS_DEV_BIND_IRQ`)
1. User-space driver creates an Event object via existing Stage 3G IPC (`SYS_EVENT_CREATE` or channel event).
2. Driver invokes `SYS_DEV_BIND_IRQ(dev_handle, res_idx, event_handle)`.
3. Kernel validates:
   - Caller possesses `dev_handle` with `DEV_INTERRUPT_LISTEN` right.
   - `res_idx` corresponds to an authorized `Irq` resource of the device.
   - Caller possesses `event_handle` with `EVENT_SIGNAL | EVENT_WAIT` rights.
4. Kernel registers `INTERRUPT_DISPATCH_TABLE[vector].event_object_id = event.object_id`.
5. When hardware interrupt fires:
   - Top-half ISR (`IF = 0`) sends hardware EOI and calls `event.signal()`.
   - Driver thread blocked in `SYS_EVENT_WAIT` unblocks in Ring 3 and processes hardware event.
6. On driver unbind or process exit:
   - Interrupt vector is masked and `event_object_id` is cleared (0).

---

## 10. Monotonic Lock Hierarchy

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

### Operation Lock Traces

#### Trace 1: `SYS_DEV_MAP_MMIO`
1. Validate handle in `Process.handle_table`.
2. Acquire `KERNEL_OBJECT_TABLE_LOCK` (Level 7); verify `DEV_MAP_MMIO` right and generation; increment `mapping_refs`; release lock.
3. Acquire `DEVICE_REGISTRY_LOCK` (Level 5).
4. Acquire `DEVICE_RESOURCE_LOCK` (Level 6).
5. Verify MMIO resource index and parameters.
6. Release `DEVICE_RESOURCE_LOCK` (Level 6).
7. Release `DEVICE_REGISTRY_LOCK` (Level 5).
8. Allocate user virtual range and map uncacheable PTEs into `AddressSpace`.

#### Trace 2: Top-Half IRQ -> Event Signal -> Thread Wakeup
1. Hardware triggers IRQ; CPU enters ISR (`IF = 0`, Level 9).
2. Look up `INTERRUPT_DISPATCH_TABLE[vector]`.
3. Send hardware EOI to LAPIC / PIC.
4. If `event_object_id != 0`:
   - Acquire `KERNEL_OBJECT_TABLE_LOCK` (Level 7).
   - Set event signal state to true.
   - Release `KERNEL_OBJECT_TABLE_LOCK` (Level 7).
   - Acquire `SCHEDULER.lock` (Level 8) to place blocked driver thread on runqueue.
   - Release `SCHEDULER.lock` (Level 8).
5. `iretq` returns from interrupt. Strictly monotonic: Level 9 $\to$ Level 7 $\to$ Level 8.

#### Trace 3: Process Exit Shared Device Teardown
1. Process exit initiates cleanup.
2. For each device handle in `Process.handle_table`:
   - Acquire `DEVICE_REGISTRY_LOCK` (Level 5).
   - Acquire `DEVICE_RESOURCE_LOCK` (Level 6).
   - Cancel operations and unpin DMA frames owned by this PID.
   - Release `DEVICE_RESOURCE_LOCK` (Level 6).
   - Check `handle_refs`: if other processes hold handles, leave device `Ready`. If zero handles and this process is `driver_pid`, transition to `Quiescing -> Detached`.
   - Release `DEVICE_REGISTRY_LOCK` (Level 5).
3. Close handles under `KERNEL_OBJECT_TABLE_LOCK` (Level 7).

---

## 11. ZeroFS Storage Backward Compatibility

Stage 3K introduced `BlockDevice` trait, `MemDeviceWrapper` (ID 0), and `AtaPioBlockDevice` (ID 1):
- `MemDeviceWrapper` is registered as `BOOTSTRAP_MEM_DEVICE_ID = DeviceId(0)`.
- `AtaPioBlockDevice` is registered as `BOOTSTRAP_ATA_DEVICE_ID = DeviceId(1)` with `IoPort` resources `0x1F0..0x1F7`, `0x3F6`.
- ZeroFS filesystem access continues to route through `BLOCK_DEVICE_LOCK` (Level 4) and `dev::get_device(id)` without any change in behaviour or performance.

---

## 12. Adversarial Review (10 Stress Scenarios)

1. **Process A Exits While Process B Uses Shared Device:**
   - *Outcome:* Process A's DMA buffers are unpinned and MMIO unmapped. Process B's capability handles remain valid. Device remains in `Ready` state (`I-DEV-LIFETIME-1`).
2. **Device Removed While DMA Is Active:**
   - *Outcome:* State machine moves device to `Quiescing`. Hardware DMA engine is commanded to stop. Frames are only unpinned after DMA engine halts (`I-DEV-DMA-1`).
3. **DMA Buffer Owner Exits:**
   - *Outcome:* Process teardown locates buffers matching `owning_pid`, stops corresponding descriptor queues, decrements `pin_count`, and reclaims frames to PMM.
4. **Capability Closes While DMA Is Active:**
   - *Outcome:* Closure drops `handle_refs`, but `in_flight_op_refs` and `DmaFrameDescriptor.pin_count` prevent frame freeing until operation completes.
5. **IRQ Arrives After Driver Detach:**
   - *Outcome:* `INTERRUPT_DISPATCH_TABLE[vector].active == false`. Top-half sends EOI and drops interrupt as spurious.
6. **IRQ Arrives During Process Teardown:**
   - *Outcome:* Vector is masked at controller during the first phase of driver detach, before memory is unmapped.
7. **Device Slot Reused After Old Capability Disappears:**
   - *Outcome:* `I-DEV-OBJ-PIN-1` prevents slot reuse while `ref_count > 0`. Slot generation increment rejects any stale handle lookups.
8. **Shared IRQ Enters Storm State:**
   - *Outcome:* Vector masked at controller after 1000 IRQs in 10 ms tick. Cooldown counter (50 ticks) prevents CPU starvation while notifying all sharing devices.
9. **MMIO Capability Revoked While Mapping Exists:**
   - *Outcome:* Revocation traverses CDT, zeroes PTEs in `AddressSpace`, flushes TLB via `invlpg`, and decrements `mapping_refs`.
10. **Partial DMA Allocation Fails:**
    - *Outcome:* Transactional rollback frees all allocated frames back to PMM, clears tracker, and returns `OutOfMemory` with zero leaks.

---

## 13. Verification Matrix (Machine-Level Tests 3L-A through 3L-T)

| Test ID | Test Focus | Key Invariant / Assertion |
|---|---|---|
| **3L-A** | Device Table & Identity Monotonicity | `DeviceId(2) > DeviceId(1)`, bootstrap IDs 0 and 1 preserved. |
| **3L-B** | Device Discovery & Enumeration | Discovers MemDevice (0), AtaDevice (1), and Platform mock devices. |
| **3L-C** | Device Lifecycle Transitions | Steps through `Discovered -> Probed -> Attached -> Ready -> Active -> Ready`. |
| **3L-D** | Resource Allocation (IoPort & Mmio) | Registers distinct IoPort and MMIO ranges without conflict. |
| **3L-E** | Resource Overlap Rejection | Rejects overlapping exclusive MMIO/Port allocations (`I-DEV-RES-CONFLICT-1`). |
| **3L-F** | Capability Authorization & Rights | Enforces `DEV_READ`, `DEV_WRITE`, `DEV_CONTROL` access control. |
| **3L-G** | MMIO Range Mapping & Uncacheable Paging | Validates `PAGE_PCD | PAGE_PWT | PAGE_NX` and user-space read/write fidelity. |
| **3L-H** | Port-I/O Authority & Boundary | Confirms kernel-mediated port I/O isolation. |
| **3L-I** | Interrupt Registration & Binding (`SYS_DEV_BIND_IRQ`) | Binds IRQ to vector and Event object ID. |
| **3L-J** | Interrupt Top-Half Event Signaling | Simulates IRQ; confirms top-half signals Event and wakes thread. |
| **3L-K** | Interrupt Storm Mitigation | Injects 1,000+ interrupts in 10 ms; verifies auto-masking and cooldown (`I-DEV-IRQ-STORM-1`). |
| **3L-L** | DMA Buffer Allocation & PMM Pinning | Allocates DMA buffer; verifies frame marked pinned in `DMA_TRACKER` (`I-DEV-DMA-1`). |
| **3L-M** | DMA Isolation & Transactional Rollback | Injects allocation failure; asserts zero frame leaks and transactional rollback. |
| **3L-N** | Device Fault State Transition | Injects hardware fault; verifies transition to `Faulted` state. |
| **3L-O** | Device Reset & Recovery | Resets `Faulted` device; verifies transition to `Resetting -> Ready`. |
| **3L-P** | Driver Detach & Teardown | Unbinds driver; verifies vector masked, MMIO unmapped, DMA unpinned. |
| **3L-Q** | Process Exit Shared Device Teardown | Terminates client process; asserts shared device remains `Ready` (`I-DEV-LIFETIME-1`). |
| **3L-R** | Capability Revocation Cascade | Revokes device capability; verifies descendant handles and MMIO unmapped. |
| **3L-S** | Concurrency & Monotonic Lock Ordering | Stresses concurrent device queries, interrupts, and scheduler preemption. |
| **3L-T** | ZeroFS Storage Backward Compatibility | Mounts ZeroFS volume and performs file read/write across 3L device layer. |

---
*End of Stage 3L Architecture Specification Rev2.*
