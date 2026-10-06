# STAGE 4B ARCHITECTURE SPECIFICATION (REV 2)

## Unified Resource Graph & Local Node Accounting (`resourced`)

**Status:** 🟡 ARCHITECTURAL SPECIFICATION — REVISION 2 (POST-REVIEW ADVERSARIAL PASS)  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-09-22  
**Target Milestone:** Stage 4B (Unified Resource Graph & Local Node Accounting)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024 Rev6  
**Kernel Baseline:** Stage 3A–3N Hard Frozen & Inviolate  
**System Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Implemented, Verified, Frozen  

---

## 1. Executive Summary of Revision 2 Refinements

Following the first architectural review of Phase 4B, **Revision 2** resolves five critical systems-engineering contracts:

1. **Heterogeneous Multi-Dimensional Accounting (`I-ACCOUNTING-CONSERVATION`)**:
   Replaces the scalar equation with an explicit multi-dimensional vector conservation model distinguishing `PhysicalCapacity` ($C_{\text{phys}}$), `UnavailableCapacity` ($C_{\text{unavail}}$), `AllocatableCapacity` ($C_{\text{allocatable}}$), `ReservedCapacity` ($C_{\text{resv}}$), `AllocatedCapacity` ($C_{\text{alloc}}$), `AvailableCapacity` ($C_{\text{avail}}$), and dynamic `ConsumedCapacity` ($C_{\text{consumed}}$) per resource class and dimension (e.g. GPU compute units vs. VRAM vs. command queues).
2. **Authoritative Monotonic Time Boundary (`I-TIME-AUTHORITY-MONOTONIC`)**:
   Formalizes the time authority boundary between Ring 0 and Ring 3. `resourced` does NOT casually access Ring 0 LAPIC hardware directly; it evaluates lease expiration strictly against an authoritative, monotonic, non-wall-clock time source whose authority originates below `resourced` (kernel periodic hardware timer ticks).
3. **Tripartite Authority Model**:
   Explicitly decouples **Resource Fact Authority** (Hardware / Device Driver / Kernel), **Resource Accounting Authority** (`resourced`), and **Lease Authority** (`resourced`).
4. **Crash Consistency & Lease Reconciliation (`I-LEASE-CRASH-RECONCILIATION`)**:
   Establishes that after `resourced` restart, no pre-crash lease is considered valid unless reconstructed during a bounded reconciliation window ($\Delta T_{\text{reconcile}}$) against current provider facts and valid kernel capabilities.
5. **Authoritative Hotplug & Disappearance Semantics (`I-RES-DISAPPEARANCE-CASCADE`)**:
   Defines the deterministic state machine for hardware detach, surprise removal, and hot-add, cascading into `ProviderLost` lease transitions and fail-closed consumer notifications without claiming unverified hardware state.

---

## 2. Scope

Phase 4B defines the architecture, data structures, invariants, failure contracts, and interfaces for **Local Node Resource Management** in ZeroOS:
1. The **Canonical Multi-Dimensional Resource Model** representing all physical and logical execution resources (CPU, GPU, NPU, RAM, Storage, Network, Peripherals, Sensors, Energy).
2. The **Unified Resource Graph** capturing physical topology, provider relationships, containment hierarchies, and lease attachments on a single node.
3. The **Local Node Accounting Engine** maintaining hard conservation across all resource dimensions.
4. The **Resource Lease Contract** providing time-bounded, capability-authorized, auto-reclaiming capacity allocations.
5. The **`resourced` Daemon** running as an unprivileged, capability-constrained Ring 3 system service supervised by `init` and discovered via `brokerd`.

Phase 4B is strictly **local node infrastructure**. It establishes the ground-truth resource substrate upon which higher-level workload dispatch (4C), workspace context (4D), and autonomous agents (4E) will execute.

---

## 3. Goals & Non-Goals

### 3.1 Goals
1. **Generic Resource Abstraction**: Uniform, typed, mathematically bounded abstraction for heterogeneous hardware and synthetic resources without erasing hardware-specific constraints.
2. **Strict Authority Decoupling**: Absolute separation of Capability Authority, Accounting Authority, and Lease Authority.
3. **Local Node Self-Sovereignty**: A node is the sole authoritative source of truth for its own physical capacity, telemetry, and leases (`I-GRAPH-AUTHORITY-LOCAL`).
4. **Multi-Dimensional Conservation**: Strict conservation of resources across discrete physical dimensions with zero capacity leaks.
5. **Deterministic Bounded Reclamation**: Consumer crash, provider failure, or TTL expiration automatically reclaims capacity fail-closed (`I-LEASE-CLEANUP`).
6. **Zero Kernel Mutation**: Consume frozen Stage 3 syscalls, handle tables, and capability models without modifying a single byte of the kernel nucleus.
7. **Zero Dynamic Memory Allocation**: Enforce static tables, bounded arrays, and deterministic memory limits in `#![no_std]` userspace.

### 3.2 Non-Goals
1. **NOT Workload Orchestration (Deferred to Phase 4C)**: `resourced` does NOT parse workload manifests, construct task execution plans, or schedule computational jobs.
2. **NOT Task DAG Scheduling (Deferred to Phase 4C)**: The Resource Graph describes what hardware exists and how it is connected; it is NOT a directed acyclic graph (DAG) of task dependencies.
3. **NOT Distributed Mesh Offloading (Deferred to Phase 4C)**: Phase 4B does not implement peer-to-peer Noise tunnels, remote capability delegation (CSDTs), or multi-node consensus.
4. **NOT Workspace Container Management (Deferred to Phase 4D)**: `resourced` does not manage user workspaces, persistent contexts, or visual surfaces.
5. **NOT Agent Perception or Intent Resolution (Deferred to Phase 4E/4F)**: `resourced` does not interpret human intent or optimize multi-variable utility functions.
6. **NOT Live Process Migration**: Precluded by Stage 4 Rev6 invariant `I-NO-LIVE-PROCESS-MIGRATION`.

---

## 4. Terminology

| Term | Definition |
| :--- | :--- |
| **`resourced`** | The Ring 3 system service daemon responsible for local resource discovery, topology, accounting, and leases. |
| **Resource Fact Authority** | The hardware provider, device driver, or kernel authoritative for the physical existence, raw capacity, and health of a resource. |
| **Resource Accounting Authority** | `resourced`, which is authoritative for multi-dimensional tracking of allocatable, reserved, allocated, and available capacity. |
| **Lease Authority** | `resourced`, which is authoritative for issuing, renewing, expiring, revoking, and reclaiming contractual capacity grants. |
| **`ResourceId`** | A globally unique 128-bit composite identifier `(NodeId, LocalSeq)` identifying a specific resource. |
| **`LeaseId`** | A globally unique 128-bit composite identifier `(NodeId, LocalSeq)` identifying an active capacity grant. |
| **Monotonic Expiration Tick** | A tick count derived from an authoritative monotonic non-wall-clock time source originating below `resourced`. |
| **Reconciliation Window** | A bounded temporal grace period ($\Delta T_{\text{reconcile}}$) following `resourced` restart during which live consumers may re-attest existing leases. |

---

## 5. Tripartite Authority Model

To eliminate architectural ambiguity, ZeroOS separates resource management into three distinct, non-overlapping authority tiers:

```text
+-----------------------------------------------------------------------------+
|                          RESOURCE FACT AUTHORITY                            |
|             (Hardware Device Controllers, Device Drivers, Kernel)           |
|                                                                             |
|  * Physical existence & discovery of hardware peripherals                   |
|  * Physical upper bound capacity: C_phys(R, d)                              |
|  * Hardware operational state (Healthy, Degraded, Faulted, Resetting)       |
|  * Physical hotplug events (Arrival, Surprise Removal, Power Rail Off)      |
|  * Low-level MMIO aperture mapping & DMA buffer pinning                     |
+--------------------------------------┬--------------------------------------+
                                       │ Hardware Facts & State Changes
+--------------------------------------v--------------------------------------+
|                        RESOURCE ACCOUNTING AUTHORITY                        |
|                                 (resourced)                                 |
|                                                                             |
|  * Multi-dimensional capacity vector accounting (C_unavail, C_allocatable)  |
|  * Reservation headroom accounting (C_resv)                                 |
|  * Committed capacity tracking (C_alloc)                                    |
|  * Available admission headroom: C_avail = C_allocatable - (C_resv+C_alloc) |
|  * Local per-process and per-service quota enforcement                      |
+--------------------------------------┬--------------------------------------+
                                       │ Capacity Verification & Commitment
+--------------------------------------v--------------------------------------+
|                               LEASE AUTHORITY                               |
|                                 (resourced)                                 |
|                                                                             |
|  * Verification that Lease Authority ⊆ Capability Authority                 |
|  * Issuance of unique, monotonic LeaseId tokens with generation counters    |
|  * Bounded temporal lifespan tracking against monotonic kernel ticks        |
|  * Renewal, revocation, and deterministic reclamation (I-LEASE-CLEANUP)      |
|  * Crash reconciliation protocol & orphan evacuation                        |
+-----------------------------------------------------------------------------+
```

### 5.1 Authority Matrix

| Entity | Role in System | Authoritative Domain | Strictly NOT Authoritative For |
| :--- | :--- | :--- | :--- |
| **Hardware / Driver** | Resource Fact Authority | Physical capacity ($C_{\text{phys}}$), device health, hotplug interrupts, MMIO. | Issuing leases, multi-consumer quotas, global node accounting. |
| **Kernel Nucleus** | Security & Hardware Primitive | Capability derivation (CDT), physical frames (PMM), thread scheduling, IPC. | Resource topology graphs, soft reservations, lease tokens. |
| **`resourced`** | Accounting & Lease Authority | Allocatable/Reserved/Allocated/Available math, lease lifecycle, quotas. | Fabricating physical facts, overriding kernel capability rights. |
| **`brokerd`** | Service Directory | Service registration and endpoint transfer. | Resource capacities, lease tracking. |
| **`workloadd`** (4C) | Workload Orchestration | Task DAG dependencies, job dispatch, checkpoint recovery. | Allocating physical hardware without a verified lease from `resourced`. |

---

## 6. Authoritative Monotonic Time Boundary

### 6.1 The Ring 3 Time Dilemma
`resourced` is an unprivileged **Ring 3 user-space service**. The LAPIC and CPU MSRs are privileged hardware registers configured in Stage 3B/3N. `resourced` **must not** execute privileged instructions or casually assume direct hardware timer access.

### 6.2 The Monotonic Time Invariant
$$\mathbf{Invariant\ I-TIME-AUTHORITY-MONOTONIC:}$$
$$\text{Lease expiration is evaluated strictly against a monotonic, non-wall-clock time source}$$
$$\text{whose authority originates below \texttt{resourced} (kernel hardware timer ticks).}$$
$$\text{Wall-clock synchronization (NTP/PTP) MUST NOT be required or permitted to influence lease validity.}$$

```text
+-----------------------------------------------------------------------------+
|                             STAGE 3 KERNEL                                  |
|                                                                             |
|  [Hardware LAPIC / PIT] ──(100 Hz Periodic IRQ)──> [Atomic Kernel TICKS]     |
|                                                               │             |
|                                                               │ Exposes tick|
|                                                               │ primitive   |
+---------------------------------------------------------------|-------------+
                                                                │
+---------------------------------------------------------------v-------------+
|                           STAGE 4B resourced (Ring 3)                       |
|                                                                             |
|  [Sample Monotonic Tick: T_now]                                             |
|              │                                                              |
|              ├─> Check Lease Expiration: (T_now >= lease.expiration_tick)   |
|              └─> Compute New Expiration:  T_expire = T_now + Delta_T_TTL     |
+-----------------------------------------------------------------------------+
```

### 6.3 Interface for Monotonic Time Acquisition
`resourced` samples monotonic time using one of two non-bypassable, kernel-grounded mechanisms established in Stage 3:
1. **Channel / Event Timer Tick Notification**: The kernel dispatches periodic tick pulses over a dedicated timer notification channel to `resourced`.
2. **Read-Only Telemetry Query**: Monotonic ticks are read via a kernel-mediated inquiry (`SYS_DEV_QUERY` on the Platform/Timer device, or sampling the thread's accumulated tick baseline).
Under both mechanisms, the tick count increments monotonically, never rolls backward, and is completely immune to network wall-clock skew.

---

## 7. Canonical Multi-Dimensional Resource Model

### 7.1 Separation of Accounting Dimensions
Physical resources cannot be reduced to a single scalar integer. For example, a GPU cannot be represented solely by VRAM bytes: if its compute units or command queues are saturated, new work cannot be admitted regardless of free memory.

ZeroOS defines multi-dimensional capacity vectors per canonical resource class:

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    ComputeCpu     = 1, // Physical cores, logical threads, SIMD/ISA extensions
    ComputeGpu     = 2, // Graphics & compute units, VRAM, command ring buffers
    ComputeNpu     = 3, // Neural processing tensor engines, weight cache, TOPS
    MemoryRam      = 4, // Physical memory frames, NUMA nodes, DMA pool
    StorageVolume  = 5, // Block devices, partitions, ZeroFS volumes, IOPS
    NetworkLink    = 6, // Network interfaces, MAC ports, bandwidth, queue pairs
    HardwareDevice = 7, // Raw PCIe/MMIO peripherals, accelerators, sensors
    SensorStream   = 8, // Hardware telemetry (fuel gauge, temperature, IMU)
    EnergyBudget   = 9, // Battery capacity, power draw ceilings, thermal envelope
}
```

### 7.2 Multi-Dimensional Taxonomy

| Resource Class | Primary Dimension ($d_1$) | Secondary Dimension ($d_2$) | Tertiary Dimension ($d_3$) | Quaternary Dimension ($d_4$) |
| :--- | :--- | :--- | :--- | :--- |
| `ComputeCpu` | Compute Time (mCPU) | Logical Threads (count) | Affinity Mask (core bitmask) | — |
| `ComputeGpu` | Compute Units (CUs) | VRAM (4 KiB frames) | Command Queues (slots) | Bus Bandwidth (MB/s) |
| `ComputeNpu` | Tensor Throughput (MilliTOPS) | Weight SRAM (KiB) | Batch Contexts (slots) | — |
| `MemoryRam` | Physical Frames (4 KiB) | DMA Pinned Frames (count) | NUMA Domain (mask) | — |
| `StorageVolume`| Capacity (4 KiB blocks) | Sustained Bandwidth (KB/s) | Peak IOPS (ops/s) | — |
| `NetworkLink` | Tx Bandwidth (Kbps) | Rx Bandwidth (Kbps) | Queue Descriptors (count) | — |
| `HardwareDevice`| Device Instances (exclusive/shared)| MMIO Window (bytes) | Interrupt Bindings (count) | — |
| `SensorStream` | Sampling Frequency (Hz) | Listener Slots (count) | — | — |
| `EnergyBudget` | Continuous Power (mW) | Energy Reserve ($\mu\text{J}$) | Thermal Headroom ($^\circ\text{C}$) | — |

### 7.3 Multi-Dimensional Conservation Laws
For every managed resource $R$ and for every accounting dimension $d \in \text{Dimensions}(R)$, `resourced` enforces the following multi-state conservation invariants:

$$\begin{aligned}
C_{\text{phys}}(R, d) &\ge 0 \quad (\text{Authoritative physical hardware envelope}) \\
C_{\text{unavail}}(R, d) &\ge 0 \quad (\text{Temporarily unusable: thermal, faulted, or reserved by firmware}) \\
C_{\text{allocatable}}(R, d) &= C_{\text{phys}}(R, d) - C_{\text{unavail}}(R, d) \\
C_{\text{resv}}(R, d) &\ge 0 \quad (\text{Soft reservation headroom}) \\
C_{\text{alloc}}(R, d) &\ge 0 \quad (\text{Hard contractual commitment to active leases}) \\
C_{\text{avail}}(R, d) &= C_{\text{allocatable}}(R, d) - (C_{\text{resv}}(R, d) + C_{\text{alloc}}(R, d)) \\
C_{\text{consumed}}(R, d) &\ge 0 \quad (\text{Dynamically observed physical utilization; advisory telemetry})
\end{aligned}$$

$$\mathbf{Invariant\ I-ACCOUNTING-CONSERVATION:}$$
$$\forall R, \forall d \in \text{Dimensions}(R): \quad C_{\text{avail}}(R, d) + C_{\text{resv}}(R, d) + C_{\text{alloc}}(R, d) + C_{\text{unavail}}(R, d) = C_{\text{phys}}(R, d)$$

Every term is strictly non-negative:
$$C_{\text{avail}} \ge 0, \quad C_{\text{resv}} \ge 0, \quad C_{\text{alloc}} \ge 0, \quad C_{\text{unavail}} \ge 0$$
Under this formulation:
- If a GPU core overheats or throttles, the driver increases $C_{\text{unavail}}(\text{GPU}, \text{ComputeUnits})$.
- $C_{\text{allocatable}}$ decreases immediately.
- If $C_{\text{avail}}$ becomes negative, admission halts immediately, and lower-priority leases are shed fail-closed.
- Physical capacity is never inflated or leaked.

### 7.4 Canonical Resource Descriptor Layout (Static 128 Bytes)
To guarantee bounded memory in `#![no_std]` userspace, each slot is fixed at exactly 128 bytes (8-byte aligned):

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceDescriptor {
    // Identity & Provenance (32 Bytes)
    pub resource_id: DistributedId,     // 16 bytes: (NodeId, LocalSeq)
    pub provider_pid: u64,              // 8 bytes: Process ID of authoritative driver
    pub generation: u32,                // 4 bytes: Slot reuse generation
    pub res_type: ResourceType,         // 1 byte: Canonical type enum
    pub locality_node: u8,              // 1 byte: NUMA domain / CPU affinity socket
    pub _pad0: [u8; 2],                 // 2 bytes: Explicit alignment padding

    // Operational State & Health (16 Bytes)
    pub state: ResourceLifecycleState,  // 1 byte: Available, Degraded, Detaching, etc.
    pub health: ResourceHealth,         // 1 byte: Healthy, Degraded, Critical, Faulted
    pub energy_tier: EnergyTier,        // 1 byte: Measured, Estimated, Declared
    pub thermal_state: ThermalState,    // 1 byte: Nominal, Throttling, Critical
    pub required_rights: u16,           // 2 bytes: Stage 3 capability rights mask
    pub active_leases_count: u16,       // 2 bytes: Number of active concurrent leases
    pub bound_kernel_handle: u32,       // 4 bytes: Process-local handle to kernel object
    pub _pad1: [u8; 4],                 // 4 bytes: Alignment padding

    // Multi-Dimensional Capacity Vector: Primary Dimension (32 Bytes)
    pub phys_capacity_d1: u64,          // 8 bytes: C_phys for Primary Dimension
    pub unavail_capacity_d1: u64,       // 8 bytes: C_unavail (throttled/offline)
    pub alloc_capacity_d1: u64,         // 8 bytes: C_alloc (committed leases)
    pub resv_capacity_d1: u64,          // 8 bytes: C_resv (soft headroom)

    // Multi-Dimensional Capacity Vector: Secondary Dimension (32 Bytes)
    pub phys_capacity_d2: u64,          // 8 bytes: C_phys for Secondary Dimension
    pub unavail_capacity_d2: u64,       // 8 bytes: C_unavail (throttled/offline)
    pub alloc_capacity_d2: u64,         // 8 bytes: C_alloc (committed leases)
    pub resv_capacity_d2: u64,          // 8 bytes: C_resv (soft headroom)

    // Telemetry & Dynamic Counters (16 Bytes)
    pub consumed_capacity_d1: u32,      // 4 bytes: Dynamic usage telemetry (Dimension 1)
    pub consumed_capacity_d2: u32,      // 4 bytes: Dynamic usage telemetry (Dimension 2)
    pub last_telemetry_tick: u64,       // 8 bytes: Monotonic tick of last update
}

const _: () = assert!(core::mem::size_of::<ResourceDescriptor>() == 128);
const _: () = assert!(core::mem::align_of::<ResourceDescriptor>() == 8);
```

---

## 8. Resource Graph Model

### 8.1 The Separation Invariant
$$\mathbf{Invariant\ I-GRAPH-SEPARATION:}\quad \text{Resource Graph} \neq \text{Workload Task DAG}$$
- The **Resource Graph** captures *what hardware and logical capacity exists, how it is physically interconnected, and which leases bind to it*. It is a **general directed typed graph** that naturally contains cycles (e.g., Node $\to$ Device $\to$ Storage $\to$ Swap $\to$ RAM $\to$ Node).
- The **Workload Task DAG** captures *what computation needs to be accomplished and the flow of data dependencies between tasks*. It is **strictly acyclic** (`I-TASK-DAG-ACYCLIC`).

### 8.2 Graph Edge Semantics
The Resource Graph permits exactly 6 explicit, typed edges:
1. `Contains`: Hierarchical structural containment (Parent contains Child: Node $\to$ CPU Socket $\to$ Core).
2. `Provides`: Inception binding (Driver process provides Resource).
3. `DependsOn`: Hardware or functional dependency (NPU Engine depends on PCIe Bus).
4. `AttachedTo`: Interconnect physical binding (NVMe SSD attached to PCIe Controller).
5. `Backs`: Substrate backing relationship (Physical DRAM backs Virtual SHM Pool).
6. `LeasedBy`: Capacity commitment (Resource capacity leased to Consumer Process).

### 8.3 Static Edge Table (32 Bytes)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GraphEdge {
    pub occupied: bool,          // 1 byte: Occupied slot flag
    pub edge_type: GraphEdgeType,// 1 byte: Typed relationship enum
    pub generation: u16,         // 2 bytes: Edge generation
    pub _pad0: [u8; 4],          // 4 bytes: Alignment padding
    pub source_id: DistributedId,// 16 bytes: Source node/resource
    pub target_id: DistributedId,// 16 bytes: Target node/resource/lease
}
```

---

## 9. Resource Leases & Decoupled Authority

### 9.1 Authority vs. Capacity Invariant
$$\mathbf{Capability} \neq \mathbf{Lease}$$
$$\mathbf{Invariant\ I-LEASE-AUTH-BOUNDED:}\quad \text{Lease Authority} \subseteq \text{Capability Authority}$$
- A **Capability** grants unforgeable mathematical authorization to interact with an object.
- A **Lease** grants time-bounded, metered physical capacity of that object.
- Presenting a lease without a valid kernel capability confers zero authority.
- `resourced` rejects any lease request whose required rights exceed the caller's presented capability (`I-RES-CAP-NO-AMPLIFICATION`).

### 9.2 Canonical Lease Record (Static 96 Bytes)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceLease {
    // Identity & Binding (32 Bytes)
    pub lease_id: DistributedId,         // 16 bytes: Unique (NodeId, LocalSeq)
    pub resource_id: DistributedId,      // 16 bytes: Target Resource (NodeId, LocalSeq)

    // Ownership & Security (24 Bytes)
    pub consumer_pid: u64,               // 8 bytes: Process ID holding this lease
    pub consumer_handle: u32,            // 4 bytes: Transferred handle to kernel object
    pub generation: u32,                 // 4 bytes: Monotonic lease generation counter
    pub authorized_rights: u16,          // 2 bytes: Mask of verified capability rights
    pub state: LeaseState,               // 1 byte: Active, Expired, Revoked, etc.
    pub _pad0: [u8; 5],                  // 5 bytes: Alignment padding

    // Multi-Dimensional Committed Capacity (24 Bytes)
    pub allocated_amount_d1: u64,        // 8 bytes: Committed capacity in Dimension 1
    pub allocated_amount_d2: u64,        // 8 bytes: Committed capacity in Dimension 2
    pub quota_bucket_id: u32,            // 4 bytes: Owning quota bucket
    pub renewal_count: u32,              // 4 bytes: Renewal counter

    // Monotonic Time Bounds (16 Bytes)
    pub start_tick: u64,                 // 8 bytes: Monotonic tick at lease grant
    pub expiration_tick: u64,            // 8 bytes: Monotonic tick deadline (T_start + TTL)
}

const _: () = assert!(core::mem::size_of::<ResourceLease>() == 96);
const _: () = assert!(core::mem::align_of::<ResourceLease>() == 8);
```

### 9.3 Canonical Lease Lifecycle

```text
                 +-------------------+
                 |     Requested     |
                 +---------+---------+
                           | Verified & Committed
                           v
                 +-------------------+
                 |      Active       |<-------+
                 +----+----+----+----+        | Renewed
                      |    |    |             |
         +------------+    |    +-------------+
         |                 |
         v                 v
+------------------+ +------------------+ +------------------+ +------------------+
|     Released     | |     Expired      | |     Revoked      | |   ProviderLost   |
| (Clean exit)     | |  (TTL missed)    | |  (Cap revoked)   | | (Driver/HW died) |
+--------+---------+ +--------+---------+ +--------+---------+ +--------+---------+
         |                    |                    |                    |
         +--------------------+--------------------+--------------------+
                              |
                              v
                  +-----------------------+
                  |       Reclaimed       |
                  | (Capacity C_avail +=) |
                  +-----------------------+
```

---

## 10. Crash Consistency & Lease Reconciliation

### 10.1 The Crash Invalidation Rule
When `resourced` restarts following a crash, power loss, or panic, in-memory lease records are vaporized. The architecture rejects assuming pre-crash leases remain valid:

$$\mathbf{Invariant\ I-LEASE-CRASH-RECONCILIATION:}$$
$$\text{After \texttt{resourced} restart, no lease may be considered valid unless its authoritative state}$$
$$\text{can be reconstructed according to the persisted lease contract and current resource/provider state.}$$

### 10.2 The Reconciliation Protocol
```text
resourced Supervised Restart by init
   │
   ├─> 1. Recover Durable Sequence Allocator:
   │      Resume sequence counter strictly > S_ceil persisted on ZeroFS (I-ID-DURABLE-ALLOCATOR-STATE).
   │
   ├─> 2. Re-discover Hardware Facts:
   │      Query drivers & kernel via SYS_DEV_QUERY to establish current C_phys and health.
   │
   ├─> 3. Enter Reconciliation Window:
   │      Open bounded temporal window Delta_T_reconcile (e.g. 50 ticks = 500 ms).
   │      During this window, resourced accepts OP_LEASE_RECONCILE from alive consumers.
   │
   ├─> 4. Consumer Re-Attestation:
   │      Consumer presents pre-crash LeaseToken + valid Stage 3 capability handle.
   │      resourced verifies:
   │        a) Target resource currently exists and is Healthy.
   │        b) Capability handle is valid in kernel CDT.
   │        c) Expiration tick has not passed.
   │        d) Reconciled capacity fits within C_allocatable.
   │      If valid: lease is re-inserted into active table; C_alloc incremented.
   │      If invalid: lease is rejected fail-closed; consumer must terminate or re-acquire.
   │
   └─> 5. Close Reconciliation Window:
          Once Delta_T_reconcile expires:
          All un-reconciled pre-crash capacity is declared Free; C_avail = C_allocatable - C_alloc.
          Normal admission operations resume.
```

---

## 11. Authoritative Hotplug & Disappearance Semantics

### 11.1 Dynamic Hardware Lifecycle
Hardware resources are not static. `resourced` enforces deterministic state transitions:

```text
Discovered ──> Available <──> Reserved <──> Allocated ──> Degraded
                   │                                         │
                   └──> Detaching ──> Detached / Unavailable <┘
                                            │
                                            v
                                         Released
```

### 11.2 Disappearance Cascading Rule
$$\mathbf{Invariant\ I-RES-DISAPPEARANCE-CASCADE:}$$
$$\text{When an authoritative provider event indicates resource disappearance or hardware fault,}$$
$$\text{\texttt{resourced} MUST transition the resource to \texttt{Unavailable}, set } C_{\text{unavail}} \leftarrow C_{\text{phys}},$$
$$\text{and immediately transition all associated active leases to \texttt{ProviderLost} fail-closed.}$$

### 11.3 Sequence of Disappearance Events
1. **Detection**: Hardware surprise removal, PCIe link-down interrupt, or driver process termination (kernel asserts `PeerClosed` signal on provider channel).
2. **State Transition**: `resourced` marks `ResourceDescriptor.state = Unavailable`, `health = Faulted`.
3. **Capacity Adjustment**:
   $$C_{\text{unavail}}(R, d) \leftarrow C_{\text{phys}}(R, d)$$
   $$C_{\text{allocatable}}(R, d) \leftarrow 0$$
   $$C_{\text{avail}}(R, d) \leftarrow 0$$
4. **Lease Invalidation**: Every lease binding to $R$ immediately transitions to `LeaseState::ProviderLost`.
5. **Consumer Notification**: `resourced` dispatches asynchronous notification events or closes consumer lease channels.
6. **Reclamation**: When consumers disconnect or timeout expires, the lease slot is reclaimed and generation is advanced.

### 11.4 Hot-Add Semantics
1. Hardware insertion / power-rail enable detected by kernel/driver.
2. Driver initializes hardware and presents device capability to `resourced` via `OP_RES_REGISTER`.
3. `resourced` verifies capability, issues a new unique `ResourceId` using the durable allocator, and registers initial $C_{\text{phys}}$ with $C_{\text{unavail}} = 0$.
4. Containment and interconnect edges (`Contains`, `AttachedTo`) are added to the Resource Graph.
5. Resource transitions to `Available` for lease admission.

---

## 12. Quota Model

In Phase 4B, quotas are strictly scoped to **Process Identity (`ProcessId`)** and **Service Identity (`ServiceId`)**, with reserved fields for future Stage 4C `WorkloadId` and Stage 4D `WorkspaceId`.

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct QuotaEntry {
    pub occupied: bool,
    pub entity_type: QuotaEntityType,    // Process, Service, Workload, Workspace
    pub _pad0: [u8; 2],
    pub entity_id: u64,                  // PID or ServiceId
    pub max_cpu_millicores: u32,         // Max CPU capacity allocatable
    pub max_ram_frames: u32,             // Max 4 KiB frames allocatable
    pub max_storage_sectors: u64,        // Max persistent disk sectors
    pub max_network_bps: u64,            // Max bandwidth ceiling
    pub cur_cpu_millicores: u32,         // Currently allocated CPU
    pub cur_ram_frames: u32,             // Currently allocated RAM
    pub cur_storage_sectors: u64,        // Currently allocated Storage
    pub cur_network_bps: u64,            // Currently allocated Network
}
```

### 12.1 Quota Invariants
1. `I-QUOTA-BOUNDED`: Total capacity allocated across all active leases held by an entity cannot exceed its provisioned quota ceiling.
2. `I-QUOTA-FAIL-CLOSED`: When an entity reaches its quota ceiling, subsequent lease requests fail immediately with `ZeroError::QuotaExceeded`.
3. `I-QUOTA-CLEANUP`: When an entity terminates or a lease is reclaimed, quota usage counters are decremented monotonically.

---

## 13. Energy Model

### 13.1 Authoritative Telemetry Classification
In accordance with Stage 4 Rev6 invariant `I-ENERGY-MEASUREMENT-CLASSIFIED`:
1. **`MEASURED`**: Grounded in physical hardware counters (Intel/AMD RAPL MSRs, battery fuel gauge, PMIC I2C registers). High assurance.
2. **`ESTIMATED`**: Derived from analytical physics models ($E = \text{OPS} \times \alpha + \text{StaticPower}$). Moderate assurance.
3. **`DECLARED`**: Stated by software intent manifests. Zero hardware assurance.

### 13.2 Enforcement Invariant
$$\mathbf{Invariant\ I-RES-ENERGY-LIMITS:}$$
$$\text{Security boundaries, capability validation, and hard capacity admission decisions}$$
$$\text{MUST NOT depend on untrusted \texttt{DECLARED} or unverified \texttt{ESTIMATED} telemetry.}$$
$$\text{Hard thermal/power shedding triggers strictly on \texttt{MEASURED} PMIC telemetry.}$$

---

## 14. IPC / API Boundary (Stage 3G Protocol)

All communication with `resourced` uses the frozen 80-byte `IpcMessage` layout over Stage 3G bidirectional channels:

```rust
#[repr(C)]
pub struct IpcMessage {
    pub tag: u64,             // Protocol Opcode (0x2001 .. 0x2014)
    pub payload_len: u16,     // Length of valid payload (0..48)
    pub handles_count: u16,   // Attached handles (0..4)
    pub _reserved1: u32,      // Explicit padding
    pub payload: [u8; 48],    // Inline message data
    pub handles: [u32; 4],    // Transferred capability handles
}
```

### Protocol Opcodes & Message Contracts

| Opcode Name | Tag | Request Payload | Handles In | Response Payload | Handles Out |
| :--- | :---: | :--- | :---: | :--- | :---: |
| `OP_RES_REGISTER` | `0x2001` | ResType(1B) + Loc(1B) + CapD1(8B) + CapD2(8B) + Rights(2B) | `[dev_cap]` | Status(4B) + ResId(16B) + Gen(4B) | None |
| `OP_RES_UNREGISTER` | `0x2003` | ResId(16B) + Gen(4B) | None | Status(4B) | None |
| `OP_RES_DISCOVER` | `0x2005` | FilterType(1B) + Offset(2B) | None | Status(4B) + Count(2B) + Total(2B) + ResIds(32B) | None |
| `OP_RES_QUERY` | `0x2007` | ResId(16B) | None | Status(4B) + State(1B) + PhysD1(8B) + AvailD1(8B) + Pwr(4B) | None |
| `OP_LEASE_REQUEST` | `0x2009` | ResId(16B) + AmtD1(8B) + AmtD2(8B) + TTLTicks(8B) | `[auth_cap]` | Status(4B) + LeaseId(16B) + Gen(4B) + ExpireTick(8B) | `[dev_handle]` |
| `OP_LEASE_RENEW` | `0x200B` | LeaseId(16B) + Gen(4B) + TTLTicks(8B) | None | Status(4B) + NewExpireTick(8B) + Gen(4B) | None |
| `OP_LEASE_RELEASE` | `0x200D` | LeaseId(16B) + Gen(4B) | None | Status(4B) | None |
| `OP_LEASE_RECONCILE`| `0x200E` | LeaseId(16B) + ResId(16B) + AmtD1(8B) + ExpTick(8B) | `[auth_cap]` | Status(4B) + Gen(4B) + Reconciled(1B) | None |
| `OP_QUOTA_QUERY` | `0x200F` | EntityId(8B) + EntityType(1B) | None | Status(4B) + CpuLim(4B) + RamLim(4B) + CurRam(4B) | None |
| `OP_ENERGY_GET` | `0x2011` | ResId(16B) | None | Status(4B) + Tier(1B) + Pwr(4B) + Temp(2B) + LastTick(8B) | None |

---

## 15. Security Threat Analysis & Mitigations

| Threat ID | Threat Vector | Boundary | Architectural Mitigation | Testable Invariant |
| :--- | :--- | :--- | :--- | :--- |
| **T-4B-01** | Forged `ResourceId` | Client $\to$ `resourced` | Monotonic issuance by `resourced`; table lookups validate slot occupancy and generation. | `I-RES-ID-UNIQUE` |
| **T-4B-02** | Identifier Reuse After Crash | `resourced` Restart | Sequence ceiling durably committed to ZeroFS prior to external observation. | `I-ID-DURABLE-ALLOCATOR-STATE` |
| **T-4B-03** | Capability Amplification | Client $\to$ `resourced` | Kernel authoritatively enforces rights subset via `sys_cap_derive`; `resourced` verifies bitmask. | `I-RES-CAP-NO-AMPLIFICATION` |
| **T-4B-04** | Stale Lease Reuse | Client $\to$ `resourced` | Monotonic 32-bit `generation` counter per lease slot; mismatched generation rejected. | `I-LEASE-STALE-REJECTION` |
| **T-4B-05** | Multi-Dimensional Leakage | Internal Accounting | Vector conservation checked on every commit: $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$. | `I-ACCOUNTING-CONSERVATION` |
| **T-4B-06** | Consumer Hoarding / Starvation | Client $\to$ System | Hard per-process/service quotas enforced on admission; fail-closed rejection. | `I-QUOTA-BOUNDED` |
| **T-4B-07** | Clock Manipulation / Skew | Lease Expiration | Expiration evaluated strictly against kernel monotonic hardware ticks; NTP/wall-clock ignored. | `I-TIME-AUTHORITY-MONOTONIC` |
| **T-4B-08** | Untrusted Energy Injection | Workload $\to$ System | Energy telemetry strictly classified (`MEASURED`, `ESTIMATED`, `DECLARED`); hard limits use `MEASURED` only. | `I-RES-ENERGY-LIMITS` |
| **T-4B-09** | Orphan Capacity on Crash | Consumer Exit | Kernel IPC `PeerClosed` signal triggers deterministic lease sweep and capacity reclamation. | `I-LEASE-CLEANUP` |
| **T-4B-10** | Provider Impersonation | Driver $\to$ `resourced` | Provider must present kernel-verified device capability handle (`DEV_ATTACH`). | `I-RES-AUTHORITY-LOCAL` |
| **T-4B-11** | Stale Post-Crash Lease Use | Client $\to$ `resourced` | Pre-crash leases rejected unless successfully re-attested during $\Delta T_{\text{reconcile}}$. | `I-LEASE-CRASH-RECONCILIATION` |
| **T-4B-12** | Ghost Resource After Hotplug | Hardware Detach | Driver event forces immediate $C_{\text{unavail}} \leftarrow C_{\text{phys}}$ and cascades leases to `ProviderLost`. | `I-RES-DISAPPEARANCE-CASCADE` |

---

## 16. Authoritative Phase 4B Invariant Catalog (16 Invariants)

1. `I-RES-ID-UNIQUE`: Every `ResourceId` and `LeaseId` is globally unique via composite `(NodeId, LocalSeq)` tuples.
2. `I-ID-DURABLE-ALLOCATOR-STATE`: Identifier allocator sequence ceilings must be durably committed before any identifier becomes externally observable.
3. `I-RES-AUTHORITY-LOCAL`: A node is authoritative solely over its locally hosted physical resources. Remote observations are advisory.
4. `I-RES-CAPABILITY-NO-AMPLIFICATION`: `resourced` cannot grant rights exceeding the authorizing capability token presented by the caller.
5. `I-LEASE-AUTH-BOUNDED`: A lease grants physical capacity, never capability authority: $\text{Lease Authority} \subseteq \text{Capability Authority}$.
6. `I-TIME-AUTHORITY-MONOTONIC`: Lease expiration is evaluated strictly against a monotonic, non-wall-clock time source whose authority originates below `resourced` (kernel hardware timer ticks).
7. `I-LEASE-CLEANUP`: When a consumer process terminates or disconnects, all associated leases are reclaimed without leaks.
8. `I-LEASE-STALE-REJECTION`: Any operation presenting a stale lease generation counter is rejected fail-closed with `GenerationMismatch`.
9. `I-ACCOUNTING-NONNEGATIVE`: All capacity terms ($C_{\text{phys}}, C_{\text{unavail}}, C_{\text{allocatable}}, C_{\text{resv}}, C_{\text{alloc}}, C_{\text{avail}}$) are strictly non-negative across all dimensions.
10. `I-ACCOUNTING-CONSERVATION`: For every resource and dimension, $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$ holds invariant across all operations.
11. `I-LEASE-CRASH-RECONCILIATION`: After `resourced` restart, no lease may be considered valid unless reconstructed during $\Delta T_{\text{reconcile}}$ against current provider facts and valid kernel capabilities.
12. `I-RES-DISAPPEARANCE-CASCADE`: When a resource detaches or faults, `resourced` transitions it to `Unavailable`, sets $C_{\text{unavail}} \leftarrow C_{\text{phys}}$, and cascades all active leases to `ProviderLost` fail-closed.
13. `I-RES-ENERGY-LIMITS`: Hard resource limits and safety triggers cannot depend on untrusted `ESTIMATED` or `DECLARED` energy telemetry.
14. `I-QUOTA-BOUNDED`: Total capacity leased by an entity cannot exceed its provisioned quota ceiling.
15. `I-GRAPH-SEPARATION`: The Resource Graph models physical and logical topology and may contain cycles; the Workload Task DAG is strictly acyclic.
16. `I-STATIC-BOUNDS`: All internal tables in `resourced` have statically fixed maximum capacities with zero unbounded dynamic heap allocation.

---

## 17. Static / Bounded Implementation Constraints

1. `MAX_RESOURCES = 64`: Up to 64 distinct physical and synthetic resources tracked per node.
2. `MAX_LEASES = 128`: Up to 128 concurrent active leases.
3. `MAX_GRAPH_EDGES = 256`: Up to 256 directed topological edges.
4. `MAX_QUOTAS = 32`: Up to 32 concurrent process/service quota profiles.
5. Static Memory Footprint:
   - Resource Table: $64 \times 128\text{ B} = 8\text{ KiB}$.
   - Lease Table: $128 \times 96\text{ B} = 12\text{ KiB}$.
   - Edge Table: $256 \times 32\text{ B} = 8\text{ KiB}$.
   - Quota Table: $32 \times 64\text{ B} = 2\text{ KiB}$.
   - Total Core Tables: $< 32\text{ KiB}$ static BSS memory footprint.

---

## 18. Verification Strategy (Machine Verification Tests 4B-A through 4B-O)

1. **4B-A: Multi-Dimensional Registration**: Register heterogeneous resources (CPU, GPU, RAM); verify multi-dimensional $C_{\text{phys}}$ initialization.
2. **4B-B: Vector Conservation**: Perform allocations, reservations, and thermal throttling simulations; assert $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$ across all dimensions.
3. **4B-C: Monotonic Time Expiration**: Advance kernel monotonic ticks past lease deadline; verify lease expires fail-closed without wall-clock dependency.
4. **4B-D: Capability-Mediated Admission**: Verify lease request succeeds with covering capability; fails with `PermissionDenied` when rights are attenuated.
5. **4B-E: Authority Amplification Rejection**: Verify client cannot obtain rights exceeding its presented capability.
6. **4B-F: Stale Generation Rejection**: Attempt renewal using pre-reclamation generation token; verify `GenerationMismatch`.
7. **4B-G: Consumer Crash Cleanup**: Terminate consumer process abruptly; verify `PeerClosed` signal triggers deterministic reclamation.
8. **4B-H: Provider Disappearance Cascade**: Detach simulated device driver; verify resource transitions to `Unavailable`, $C_{\text{unavail}} \leftarrow C_{\text{phys}}$, and leases transition to `ProviderLost`.
9. **4B-I: Crash Reconciliation Window**: Restart `resourced`; verify alive consumer re-attests lease within $\Delta T_{\text{reconcile}}$, and un-reconciled leases are purged.
10. **4B-J: Local Quota Enforcement**: Request capacity exceeding quota limit; verify `QuotaExceeded` rejection while node has free capacity.
11. **4B-K: Energy Classification Integrity**: Submit declared vs. measured telemetry; verify hard safety thresholds ignore declared telemetry.
12. **4B-L: Topological Cycle Traversal**: Construct cyclic Resource Graph (Node $\to$ Device $\to$ Storage $\to$ RAM $\to$ Node); verify cycle handling and depth bounding.
13. **4B-M: SMP Concurrency Stress**: Issue concurrent lease requests from multiple simulated cores; verify atomic counters prevent double-allocation.
14. **4B-N: PMM Neutrality**: Verify zero physical memory frame leakage before and after test suite (`baseline_free == current_free`).
15. **4B-O: Durable Allocator Non-Reuse**: Verify sequence numbers monotonically resume above ZeroFS-persisted ceiling across simulated reboots.

---

## 19. Implementation Boundary Notice

**Implementation is strictly prohibited during this discovery phase.**  
No code, table definitions, or test runners shall be added until this Revision 2 architecture specification is formally reviewed, adversarial-tested, and approved.
