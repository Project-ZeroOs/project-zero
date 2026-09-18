# ADR-0021: Device and Hardware Model Architecture (Stage 3L Rev3)

## Context
Project Zero / ZeroOS requires a foundational device and hardware model to provide safe, isolated, and capability-authorized access to physical hardware peripherals. Stages 3A through 3K established threads, scheduling, synchronization, thread lifecycle, processes, IPC, capabilities, syscalls, ELF loading, and persistent storage (ZeroFS).

Stage 3L designs the unified lowest-level hardware abstraction layer. ZeroOS rejects monolithic driver models where drivers have ambient physical memory access, unrestricted port I/O, or can deadlock the kernel. Instead, ZeroOS models devices as strongly typed kernel objects with capability-mediated access, strict resource conflict rejection, and deterministic lifecycle states.

## Architectural Decisions

### ADR-3L-001: Device Identity, Bootstrap Reservations, and Monotonic Allocation
- `DeviceId(pub u64)` is a globally unique identifier.
- Bootstrap reservations:
  - `BOOTSTRAP_MEM_DEVICE_ID = DeviceId(0)` (MemDevice mock block volume).
  - `BOOTSTRAP_ATA_DEVICE_ID = DeviceId(1)` (Primary ATA PIO controller).
  - `NEXT_DEVICE_ID = AtomicU64::new(2)`: All dynamic peripherals (PCI, NIC, GPU, NPU) allocate strictly monotonically starting at 2.
- `DeviceId` is decoupled from ephemeral table slot indices, capability handles, and bus topologies.

### ADR-3L-002: Formal Device Lifecycle State Machine & Shared Ownership
- States: `Discovered (0) -> Probed (1) -> Attached (2) -> Ready (3) <-> Active (4) -> Quiescing (5) -> Detached (6) -> Released (7)`.
- Fault paths: `Ready/Active -> Faulted (8) -> Resetting (9) -> Ready (3)` or `Faulted -> Detached (6)`.
- Invariant `I-DEV-LIFETIME-1`: Process termination cannot transition a physical device to `Quiescing` while another live owner holds valid capability authority over that device. Process exit cleans up only process-owned DMA buffers, operations, and MMIO mappings.

### ADR-3L-003: KernelObjectSlot ↔ DeviceSlot Co-Lifetime Pinning
- Invariant `I-DEV-OBJ-PIN-1`: A `DeviceSlot` cannot be released, cleared, or reused while its associated `KernelObjectSlot` has an active reference count (`handle_refs + mapping_refs + in_flight_op_refs > 0`).
- Handle resolution verifies `slot.occupied`, `slot.kernel_object_id == header.object_id`, and `slot.generation == object.generation`. Rejects any stale slot reuse without modifying the frozen 40-byte `KernelObjectSlot` ABI.

### ADR-3L-004: Two-Tier DMA Architecture & Physical Frame Isolation
- Logical DMA buffers: `DMA_BUFFER_TABLE: [DmaBufferDescriptor; 32]` (up to 16 frames / 64 KiB per buffer).
- Physical frame tracking: `PHYSICAL_FRAME_PIN_TABLE: [PhysicalFramePinRecord; 128]` with `pin_count: u16`.
- Invariant `I-DEV-DMA-1`: A physical frame with DMA ownership cannot transition to `Free` in PMM until all DMA owners release it (`pin_count == 0`) and the device is confirmed stopped (`state != Active && state != Resetting`).
- Invariant `I-DEV-DMA-2`: A physical frame cannot become `Free` in PMM while any active `DmaBufferDescriptor` or device operation retains ownership.
- Transactional allocation rollback: If a multi-frame allocation fails halfway through, all allocated frames are freed to PMM and the operation returns `Err(OutOfMemory)` with zero net leaks.

### ADR-3L-005: Dedicated MMIO Virtual Region & Collision Proof
- Dedicated aperture: `DEVICE_MMIO_REGION = 0x0000_6000_0000_0000 .. 0x0000_7000_0000_0000` (PML4 entries 192..223).
- Invariant `I-DEV-MMIO-1`: No ordinary user mapping, ELF segment, stack mapping, or future anonymous mapping may intersect `DEVICE_MMIO_REGION`.
- Invariant `I-DEV-MMIO-PIN-1`: While `mapping_refs > 0`, the underlying `DeviceSlot` remains pinned.
- PTE attributes: `PAGE_PRESENT`, `PAGE_USER`, `PAGE_NX` (No-Execute), `PAGE_PWT | PAGE_PCD` (Strong Uncacheable `UC`), and `PAGE_WRITABLE` (if `DEV_WRITE` granted).

### ADR-3L-006: Shared IRQ Multi-Binding Dispatch
- Bounded table: `INTERRUPT_BINDINGS: [[InterruptBinding; MAX_SHARED_IRQ_BINDINGS]; 256]` (`MAX_SHARED_IRQ_BINDINGS = 4`).
- Invariant `I-DEV-IRQ-OWNERSHIP-1`: Every active interrupt binding belongs to exactly one live device/resource/driver context.
- Invariant `I-DEV-IRQ-DELIVERY-1`: Top-half ISR dispatches to every occupied binding for the vector, signaling each registered Event object. Bottom-half drivers inspect device status to filter spurious or unrelated wakeups.

### ADR-3L-007: Scoped Device Reset Authority Under Shared Ownership
- Invariant `I-DEV-RESET-AUTH-1`: Invoking `SYS_DEV_RESET` requires `DEV_RESET` capability right AND the caller must be the registered `driver_pid` (or the sole client holding `handle_refs == 1`). A non-driver client sharing a device cannot reset the physical device.

### ADR-3L-008: Capability Rights Bit-Subset Semantics
- Invariant `I-DEV-CAP-SUBSET-1`: Capability derivation enforces strict bitwise containment:
  $$(\text{child\_rights} \ \& \ \neg\text{parent\_rights}) == 0 \iff \text{child\_rights} \subseteq \text{parent\_rights}$$

### ADR-3L-009: Deterministic Interrupt Storm Mitigation
- Invariant `I-DEV-IRQ-STORM-1`: Window is 1 scheduler tick (10 ms LAPIC tick). If vector fires $\ge 1000$ times in 1 tick, line is auto-masked, marked `in_storm`, and subjected to a 50-tick (500 ms) cooldown period. Line-level storm masking is decoupled from per-device fault attribution.

### ADR-3L-010: Resource Compatibility Matrix
- Exclusive resources reject any overlap. Shared resources permit concurrent read-only MMIO apertures, level-triggered IRQs, and shared DMA rings.

### ADR-3L-011: Monotonic Lock Hierarchy (Levels 1..9)
$$\text{FILESYSTEM\_LOCK (1)} < \text{STORAGE\_OBJECT\_TABLE\_LOCK (2)} < \text{BLOCK\_CACHE\_LOCK (3)} < \text{BLOCK\_DEVICE\_LOCK (4)} < \text{DEVICE\_REGISTRY\_LOCK (5)} < \text{DEVICE\_RESOURCE\_LOCK (6)} < \text{KERNEL\_OBJECT\_TABLE\_LOCK (7)} < \text{SCHEDULER.lock (8)} < \text{CPU (IF=0)}$$

### ADR-3L-012: Zero Dynamic Kernel Heap & Frozen ABI Preservation
- All tables reside in `.bss`.
- Frozen structures (`Process` 128 B, `KernelThread` 176 B, `PerCpu` 48 B, `HandleTable` 520 B, `CapabilityNode` 24 B, `KernelObjectSlot` 40 B) remain completely unmodified. ZeroFS compatibility preserved.
