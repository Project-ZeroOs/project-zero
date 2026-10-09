# ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 SPECIFICATION

```text
ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1
FORMAL ARCHITECTURAL SPECIFICATION

STAGE NAME: ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 (DEV-MODEL-REV1)
PRIMARY DAEMONS: deviced, driverd

FROZEN-LAYER CHANGES: 0
KERNEL CHANGES: 0
NEW SYSCALLS: 0
NEW ABI: 0
NEW CAPABILITY RIGHTS: 0
ARCHITECTURAL CYCLES: NONE
CRITICAL BLOCKERS: NONE

STATUS: 🟢 READY FOR IMPLEMENTATION PLANNING
IMPLEMENTATION: NOT STARTED
```

---

## 1. Purpose

This document provides the formal architectural specification for **ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 (DEV-MODEL-REV1)**.

DEV-MODEL-REV1 closes the Machine Boundary gap between the abstract semantic control plane (`resourced`, `fabricd`, `CapabilityEnvelope`, `DeviceId`) and physical host computer hardware (PCIe buses, MMIO ranges, DMA buffers, IRQ vectors). It defines a pure Ring3 user-space driver architecture under kernel capability supervision without modifying the kernel, adding new syscalls, or altering the frozen syscall ABI.

---

## 2. Scope

- **Ring3 Device Discovery Daemon (`deviced`)**: Hardware bus topology scanning, `DeviceNodeId` allocation, and physical device capability registration.
- **Ring3 User-Space Driver Manager (`driverd`)**: Driver process lifecycle supervision, device binding, crash isolation, and restart recovery.
- **Kernel Interface Integration**: Binding Ring3 device operations to frozen Stage 3L kernel primitives (`SYS_DEV_QUERY`, `SYS_DEV_MAP_MMIO`, `SYS_DEV_DMA_ALLOC`, `SYS_DEV_RESET`, `SYS_DEV_BIND_IRQ`) and capability rights (`DEV_MAP_MMIO`, `DEV_DMA_ACQUIRE`, `DEV_INTERRUPT_LISTEN`, `DEV_RESET`).
- **Control Plane Composability**: Linking discovered hardware device nodes into `resourced` resource graphs, `workspaced` capability policies, `observed` diagnostic event streams, and `MigrationEngine` non-transferable handle rules.

---

## 3. Non-Goals

1. **Ring0 Kernel Drivers**: Monolithic kernel drivers are strictly prohibited. Driver code executes strictly in Ring3.
2. **Legacy Driver Compatibility**: No POSIX, Linux kernel module, or Windows WDM/WDF binary driver shims.
3. **Product Shell / GUI**: No desktop shell, display compositor, or user-interface product features.

---

## 4. Terminology

- `DeviceNodeId`: 128-bit GUID representing a physical or logical device node in host bus topology (`bus_address`, `hardware_uuid`, `incarnation_seq`).
- `DeviceId`: 64-bit transparent kernel device table slot reference (`kernel/src/dev/types.rs`).
- `DeviceCapabilityHandle`: 32-bit process handle table entry possessing Stage 3L `cap_rights::DEV_*` permissions.
- `deviced`: Ring3 system daemon owning hardware topology discovery and physical device registry authority.
- `driverd`: Ring3 system daemon owning user-space driver process lifecycle supervision.

---

## 5. Architectural Position

```text
Human Intent / Orchestration Control Plane (intentd)
        ↓
Workspace Policy Authority (workspaced)
        ↓
Workload & Agent Execution Authority (workloadd)
        ↓
Resource & Fabric Authority (resourced / fabricd)
        ↓
deviced (Device Discovery & Physical Device Registry Daemon)
        ↓
driverd (Ring3 User-Space Driver Manager) & Driver Processes
        ↓
Stage 3L Ring0 Kernel Device Subsystem (kernel/src/dev/)
        ↓
Physical Computer Hardware (PCIe / ACPI / MMIO / IRQ / PMM DMA)
```

---

## 6. Component Model

```text
+-----------------------------------------------------------------------+
| Ring3 User Space                                                      |
|                                                                       |
|  +-------------------+       +--------------------+                   |
|  |     deviced       | <---> |     driverd        |                   |
|  | (Device Registry) |       | (Driver Lifecycle) |                   |
|  +-------------------+       +--------------------+                   |
|            |                           |                              |
|            v                           v                              |
|  +-------------------+       +--------------------+                   |
|  |     resourced     |       | Ring3 Driver Proc  |                   |
|  |  (Resource Graph) |       |  (User-space I/O)  |                   |
|  +-------------------+       +--------------------+                   |
+----------------------------------------|------------------------------+
| Kernel / Ring0 (Stage 3L Subsystem)    | (IPC / Syscalls 17-21)        |
|                                        v                              |
|  +-----------------------------------------------------------------+  |
|  | SYS_DEV_MAP_MMIO | SYS_DEV_BIND_IRQ | SYS_DEV_DMA_ALLOC          |  |
|  | cap_rights::DEV_MAP_MMIO | cap_rights::DEV_INTERRUPT_LISTEN     |  |
|  +-----------------------------------------------------------------+  |
+-----------------------------------------------------------------------+
```

---

## 7. Device Identity

Identity separation invariants across physical hardware and logical control plane abstractions:

```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceNodeId {
    pub bus_address: u64,
    pub hardware_uuid: u64,
    pub incarnation_seq: u64,
}
```

### Invariant Identity Matrix:
- `DeviceNodeId` (128-bit physical bus topology GUID + incarnation)
- `DeviceId` (64-bit kernel device table slot reference)
- `ResourceId` (128-bit `DistributedId` in `resourced` resource graph)
- `NodeId` (128-bit fabric host daemon ID)

$$\text{DeviceNodeId} \neq \text{DeviceId} \neq \text{ResourceId} \neq \text{NodeId}$$

### Device Incarnation Semantics:
When Device A is removed and Device B is inserted into the same physical PCI slot, `DeviceNodeId.incarnation_seq` increments. Any `DeviceCapabilityHandle` derived under incarnation $N$ returns `Err(PermissionDenied)` when presented for incarnation $N+1$, preventing stale handle reuse.

---

## 8. DeviceNode Model

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceNode {
    pub node_id: DeviceNodeId,
    pub kernel_device_id: u64,
    pub class: u8,
    pub lifecycle_state: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub pci_bdf: u16,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub irq_vector: u8,
    pub dma_max_bytes: u32,
    pub assigned_workspace_id: u64,
    pub bound_driver_pid: u64,
}
```

- **Kernel Facts**: `kernel_device_id`, `mmio_base`, `mmio_length`, `irq_vector` are queried directly from Stage 3L kernel `SYS_DEV_QUERY`.
- **Ring3 Registry State**: `node_id`, `lifecycle_state`, `assigned_workspace_id`, `bound_driver_pid` are maintained by `deviced`.

---

## 9. Device Registry

`deviced` maintains an in-memory and persistent `DeviceNodeTable` (up to 32 slots per node). `deviced` is the **single-writer authority** over physical hardware registration and reconciliation.

---

## 10. PCI / ACPI Discovery

1. On system boot or hotplug event, `deviced` queries PCI configuration space and ACPI tables via kernel `SYS_DEV_QUERY` (Syscall 17).
2. `deviced` extracts BAR MMIO base/length, vendor/device IDs, and IRQ vectors.
3. `deviced` creates a new `DeviceNode` record with incremented `incarnation_seq` and registers physical capacity with `resourced`.

---

## 11. Kernel Device Interface

Uses existing Stage 3L kernel syscalls without alteration:
- `SYS_DEV_QUERY` (17): Enumerate physical hardware BARs and kernel device table slots.
- `SYS_DEV_MAP_MMIO` (18): Map physical MMIO pages into caller page table.
- `SYS_DEV_DMA_ALLOC` (19): Allocate PMM-pinned physical memory frames.
- `SYS_DEV_RESET` (20): Issue device hardware reset sequence.
- `SYS_DEV_BIND_IRQ` (21): Bind IRQ vector notification to IPC channel.

---

## 12. Capability Delegation

Device operations require explicit handle capability verification:
- `cap_rights::DEV_MAP_MMIO` (0x0008)
- `cap_rights::DEV_DMA_ACQUIRE` (0x0010)
- `cap_rights::DEV_INTERRUPT_LISTEN` (0x0020)
- `cap_rights::DEV_RESET` (0x0040)

Handles are delegated from `WorkspaceManager` $\to$ `deviced` $\to$ `driverd` $\to$ Driver Process handle table.

---

## 13. MMIO Contract

- **Syscall**: `SYS_DEV_MAP_MMIO` (18)
- **Capability**: `cap_rights::DEV_MAP_MMIO`
- **Validation**: Kernel VMM verifies page-alignment (4 KiB), BAR boundary match, absence of RAM/kernel overlap, and process handle permissions.
- **Revocation**: Upon driver exit or device removal, kernel unmaps VMM pages and invalidates process PTEs.

---

## 14. IRQ Contract

- **Syscall**: `SYS_DEV_BIND_IRQ` (21)
- **Capability**: `cap_rights::DEV_INTERRUPT_LISTEN`
- **Routing**: Hardware interrupts trigger kernel vector dispatchers, sending high-priority notifications over the bound IPC channel.
- **Crash Safety**: On driver process termination, kernel unbinds the IRQ line and flushes channel buffers.

---

## 15. DMA Contract

- **Syscall**: `SYS_DEV_DMA_ALLOC` (19)
- **Capability**: `cap_rights::DEV_DMA_ACQUIRE`
- **Isolation**: Kernel PMM pins physical memory frames up to `MAX_PINNED_DMA_FRAMES` (512 KiB pool max per device). DMA memory frames are strictly isolated from kernel code/data and other workspace memory.

---

## 16. Device Reset Contract

- **Syscall**: `SYS_DEV_RESET` (20)
- **Capability**: `cap_rights::DEV_RESET`
- **Behavior**: Issues hardware bus reset sequence. Quiesces active DMA transfers and flushes pending IRQs before resetting device registers to initial post-boot state.

---

## 17. Driver Model

Drivers execute strictly as isolated user-space processes (`driverd`). They interact with hardware via mapped MMIO pages and IRQ IPC channels. Drivers have zero privileges to execute Ring0 instructions or access unmapped physical memory.

---

## 18. Driver Lifecycle

Device lifecycle states managed by `driverd` / `deviced`:
```text
Discovered (0) → Probed (1) → Attached (2) → Ready (3) → Active (4) → Quiescing (5) → Detached (6) → Released (7) → Faulted (8) → Resetting (9)
```

---

## 19. Driver Crash & Restart

If a driver daemon process crashes:
1. Ring0 kernel reclaims process VMM pages, unmaps MMIO, and unbinds IRQs.
2. `supervisor` receives process exit signal (`OP_PROCESS_EXIT_NOTIFY`).
3. `deviced` marks device state `Faulted` $\to$ `Resetting`.
4. `driverd` spawns a clean driver daemon instance; `deviced` re-delegates capability handles without rebooting the system or corrupting kernel memory.

---

## 20. Device Ownership

Single-writer ownership per physical device node. `deviced` assigns a physical device exclusively to one Ring3 driver process at a time. Multi-process sharing must go through driver IPC proxies.

---

## 21. Device → Resource Mapping

```text
Physical Device Node
        ↓
deviced (Registers DeviceNode)
        ↓
resourced (Publishes ResourceDescriptor)
        ↓
ResourceGraph (Available for Workload Leases)
```

1 Device Node maps to 1 primary `ResourceDescriptor` in `resourced`. Composite devices (e.g. multi-port NICs) expose discrete `ResourceDescriptor` entries with distinct `ResourceId` values.

---

## 22. Workspace Integration

`WorkspaceManager` assigns device nodes to workspaces according to `WorkspacePolicy`. Cross-workspace device capability handle derivation is strictly rejected (`Err(PermissionDenied)`).

---

## 23. Workload Integration

Tasks specify physical device requirements in `TaskResourceDemand`. `workloadd` acquires device resource leases from `resourced` before invoking process spawn.

---

## 24. Execution Integration

Driver daemons run under `WorkloadControlBlock` management. Driver state transitions emit authoritative `ExecutionEvent` records into `ExecutionManager`.

---

## 25. Observation Integration

Interrupt faults, MMIO access errors, and driver status transitions are projected into `ObservationEngine` as `ObservationId` diagnostic evidence.

---

## 26. Replanning Integration

If a driver experience unrecoverable hardware fault (`DeviceLifecycleState::Faulted`), `observed` notifies `intentd`. The orchestrator supersedes Plan v1 and materializes Plan v2 to route tasks around the faulted hardware.

---

## 27. Migration Integration

Physical hardware bindings are strictly classified as `StateClass::NonTransferable`:
- Source node physical device handles are invalidated upon cold migration quiescence.
- Destination node physical devices are assigned independently under target node `deviced` / `resourced` authority.
- Invariant $\text{ActiveExecutions}(\text{WorkloadId}) \le 1$ remains strictly enforced.

---

## 28. Persistence Model

`deviced` persists logical device metadata (bus addresses, assigned workspace IDs, vendor IDs) in `libzero/src/persistence.rs` slots. Physical handles and page mappings are **never** persisted.

---

## 29. Recovery Model

Upon host reboot, `deviced` re-scans physical PCI/ACPI hardware topologies, reconciles physical reality against persisted logical metadata, increments `incarnation_seq`, and re-issues clean capability handles.

---

## 30. Hotplug Model

When a device is physically attached or removed:
1. Kernel interrupt notifies `deviced`.
2. On removal: `deviced` sets state to `Detached`, kernel revokes MMIO page mappings, `resourced` invalidates resource descriptors, and `intentd` triggers closed-loop replanning if active workloads were affected.

---

## 31. Device Removal Model

Device removal invalidates all associated process handles. Any subsequent `SYS_DEV_MAP_MMIO` or `SYS_DEV_BIND_IRQ` calls return `Err(DeviceNotFound)`.

---

## 32. Concurrency & Lock Order

Global lock hierarchy (Acyclic):
$$\text{DEVICE_REGISTRY_LOCK (5)} < \text{DEVICE_RESOURCE_LOCK (6)} < \text{KERNEL_OBJECT_TABLE_LOCK (7)} < \text{SCHEDULER.lock (8)}$$

---

## 33. Bootstrapping DAG

```text
kernel_main (Early IRQ & PMM init)
        ↓
deviced (Scans PCI/ACPI topologies)
        ↓
resourced (Publishes hardware resource capacity)
        ↓
workspaced (Applies workspace policy)
        ↓
driverd (Spawns user-space drivers)
        ↓
workloadd / intentd (Materializes workloads & intent plans)
```

---

## 34. Security Model & Threat Boundaries

- **Unprivileged MMIO Escape**: Kernel VMM verifies page boundaries in `dispatch_dev_map_mmio`.
- **IRQ Hijacking**: Kernel checks `cap_rights::DEV_INTERRUPT_LISTEN` on caller handle.
- **DMA Poisoning**: Kernel PMM pins physical frames; DMA memory cannot access kernel pages or other workspace memory.
- **Forged Registrations**: `resourced` accepts device registrations strictly over `deviced` IPC channels.

---

## 35. Failure Model

All failures (driver crash, MMIO fault, IRQ drop, hardware disconnect) map to standard `ZeroError` codes (`NotFound`, `PermissionDenied`, `DeviceFault`, `ResourceConflict`) and propagate to `observed` for automated replanning.

---

## 36. Authority Boundaries

- `deviced`: Physical device discovery & `DeviceNodeId` registry.
- `resourced`: Resource capacity vectors, leasing, & placement.
- `workspaced`: Workspace capability envelopes & policy authorization.
- `kernel`: Hardware memory page mapping, IRQ routing, and physical frame pinning.

---

## 37. IPC Contracts (`OP_DEV_*`)

Opcodes for `deviced` IPC channels:
- `OP_DEV_DISCOVER` (0x4E00): Enumerate physical devices.
- `OP_DEV_REGISTER` (0x4E02): Register device node with `resourced`.
- `OP_DEV_BIND` (0x4E04): Bind driver daemon to device node.
- `OP_DEV_UNBIND` (0x4E06): Unbind driver daemon and release handles.

---

## 38. State Machines

Canonical `DeviceLifecycleState` machine:
$$\text{Discovered} \to \text{Probed} \to \text{Attached} \to \text{Ready} \to \text{Active} \to \text{Quiescing} \to \text{Detached} \to \text{Released}$$
Fault branch: $\text{Active} \to \text{Faulted} \to \text{Resetting} \to \text{Ready}$.

---

## 39. Formal Invariants (DEV-01 to DEV-25)

- `DEV-01`: `DeviceNodeId` identity separation across all 20 existing ZeroOS identities.
- `DEV-02`: Single-writer authority over device registry owned by `deviced`.
- `DEV-03`: `cap_rights::DEV_MAP_MMIO` requirement for physical MMIO page mapping.
- `DEV-04`: Page-aligned MMIO boundary validation in kernel VMM.
- `DEV-05`: `cap_rights::DEV_INTERRUPT_LISTEN` requirement for IRQ vector binding.
- `DEV-06`: Kernel IPC channel delivery for hardware interrupt notifications.
- `DEV-07`: `cap_rights::DEV_DMA_ACQUIRE` requirement for physical DMA buffer pinning.
- `DEV-08`: `MAX_PINNED_DMA_FRAMES` physical DMA pool ceiling enforcement.
- `DEV-09`: Ring3 driver process isolation via `WorkloadControlBlock`.
- `DEV-10`: `supervisor` Ring3 driver crash restart without kernel panic.
- `DEV-11`: Single-writer device assignment per driver process.
- `DEV-12`: 1 Device Node to `ResourceDescriptor` mapping in `resourced`.
- `DEV-13`: `WorkspaceManager` capability envelope delegation for physical devices.
- `DEV-14`: Rejection of cross-workspace device handle access (`Err(PermissionDenied)`).
- `DEV-15`: `StateClass::NonTransferable` classification for physical device handles during migration.
- `DEV-16`: Single active execution invariant ($\text{ActiveExecutions} \le 1$) during driver migration.
- `DEV-17`: Hotplug removal automatic MMIO page unmapping and handle revocation.
- `DEV-18`: `cap_rights::DEV_RESET` requirement for hardware device reset.
- `DEV-19`: Incremental `incarnation_seq` invalidating stale capabilities upon device re-insertion.
- `DEV-20`: Non-persistence of physical page mappings across host reboot.
- `DEV-21`: Hardware interrupt fault emission into `ObservationEngine` pipeline.
- `DEV-22`: Closed-loop replanning trigger upon unrecoverable driver fault.
- `DEV-23`: Monotonic lock hierarchy preventing kernel spinlock deadlocks.
- `DEV-24`: Acyclic bootstrapping DAG from `kernel_main` to `intentd`.
- `DEV-25`: 0 kernel code changes (`git diff -- kernel/` = 0).

---

## 40. Adversarial Scenarios (25 Scenarios A-Y)

Evaluates 25 adversarial attack vectors (fake `DeviceId`, MMIO kernel page mapping attempt, cross-workspace device theft, IRQ hijacking, DMA buffer poisoning, stale handle reuse post-reboot, driver crash during DMA transfer, double-binding attempts, and malicious hardware spoofing). All 25 scenarios fail safely under kernel capability handle checks.

---

## 41. Dependency Graph

```text
Kernel Stage 3L (Hardware interrupts / VMM)
        ↓
deviced (Device Registry Daemon)
        ↓
driverd (User-Space Driver Manager)
        ↓
resourced / fabricd (Resource & Fabric Authority)
        ↓
workloadd / ExecutionManager (Workload Execution)
        ↓
Intent & Orchestration Control Plane
```

---

## 42. Verification Plan

- **Level 1 (Static/Source Verification)**: Freestanding target compilation (`cargo check --target x86_64-unknown-none`).
- **Level 2 (Host Behavioral Verification)**: Unit and integration test suite (`cargo test --lib --target x86_64-pc-windows-gnu`).
- **Level 3 (Freestanding Runtime Execution)**: QEMU / bare-metal hardware execution testing.

---

## 43. Acceptance Criteria

1. 0 kernel changes, 0 new syscalls, 0 ABI modifications.
2. 100% pass rate on host behavioral integration tests.
3. All 25 formal invariants (`DEV-01` to `DEV-25`) verified.
