# STAGE 4B ARCHITECTURE SPECIFICATION (REV 6)

## Unified Resource Graph & Local Node Accounting (`resourced`)

**Status:** 🟢 FINAL ARCHITECTURAL SPECIFICATION — APPROVED FREEZE CANDIDATE  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-09-22  
**Target Milestone:** Stage 4B (Unified Resource Graph & Local Node Accounting)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024 Rev6  
**Kernel Baseline:** Stage 3A–3N Hard Frozen & Inviolate  
**System Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Implemented, Verified, Frozen  

---

## 1. Executive Summary of Revision 6 Finalization

Following the fifth round of adversarial architecture review, **Revision 6** delivers the definitive, machine-verifiable architectural specification for Stage 4B, closing the final five precision contracts:

1. **Rust Memory-Model Soundness for `TimeObservationFrame` (`I-TIME-AUTHORITY-MONOTONIC`)**:
   Defines all observation fields (`sequence`, `boot_epoch`, `monotonic_ticks`, `frequency_hz`) as explicit `AtomicU64` / `AtomicU32` structures. Eliminates undefined behavior from concurrent non-atomic shared-memory reads while preserving a zero-overhead Acquire/Release seqlock synchronization protocol.
2. **Permanent Kernel Frame Lifetime & Pinning (`I-TIME-FRAME-LIFETIME`)**:
   Establishes that the physical frame backing `TimeObservationFrame` is allocated in Kernel Physical Memory during Stage 3A bootstrap as a `FrameState::Reserved` reservation and registered in `KERNEL_OBJECT_TABLE` under PID 0. It is permanently pinned for the entire kernel boot epoch. User-space handle closure cannot destroy or reclaim the kernel producer's backing store.
3. **Write-Ahead BootEpoch Durability (`I-BOOT-EPOCH-DURABILITY`)**:
   Formalizes monotonic boot epoch advancement: candidate epoch $E_{N+1} > E_N$ must be durably committed to non-volatile storage with a hardware flush barrier *before* user-space execution begins. If durability cannot be guaranteed, the kernel halts immediately with `KERNEL_PANIC: BootEpochIdUniquenessFailed`.
4. **Non-Wrapping Seqlock Sequence Counter (`I-TIME-SEQUENCE-NONWRAP`)**:
   Mandates checked addition on the observation sequence counter. If $s \ge \text{u64::MAX} - 2$, the kernel producer permanently freezes the sequence in an odd (mutation) state, failing all subsequent user-space reads fail-closed (`TimeAuthorityUnavailable`) without wrapping to 0.
5. **Hard Dimension Bound (`I-ACCOUNTING-DIMENSION-BOUNDED`)**:
   Establishes `MAX_ACCOUNTING_DIMENSIONS = 8` as a strict, frozen architectural limit. Any resource registration requesting $> 8$ dimensions is rejected fail-closed with `ZeroError::UnsupportedResourceShape`. Complex hardware must decompose into sub-resources linked via `Contains` or `DependsOn` graph edges.

---

## 2. Scope

Phase 4B defines the architecture, data structures, invariants, failure contracts, and interfaces for **Local Node Resource Management** in ZeroOS:
1. The **Canonical Multi-Dimensional Resource Model** representing all physical and logical execution resources (CPU, GPU, NPU, RAM, Storage, Network, Peripherals, Sensors, Energy).
2. The **Unified Resource Graph** capturing physical topology, provider relationships, containment hierarchies, and lease attachments on a single node.
3. The **Local Node Accounting Engine** maintaining hard conservation and joint feasibility across all resource dimensions.
4. The **Resource Lease Contract** providing time-bounded, capability-authorized, auto-reclaiming capacity allocations.
5. The **`resourced` Daemon** running as an unprivileged, capability-constrained Ring 3 system service supervised by `init` and discovered via `brokerd`.

Phase 4B is strictly **local node infrastructure**. It establishes the ground-truth resource substrate upon which higher-level workload dispatch (4C), workspace context (4D), and autonomous agents (4E) will execute.

---

## 3. Goals & Non-Goals

### 3.1 Goals
1. **Generic Resource Abstraction**: Uniform, typed, mathematically bounded abstraction for heterogeneous hardware without erasing hardware-specific constraints.
2. **Strict Authority Decoupling**: Absolute separation of Capability Authority, Accounting Authority, and Lease Authority.
3. **Local Node Self-Sovereignty**: A node is the sole authoritative source of truth for its own physical capacity, telemetry, and leases (`I-GRAPH-AUTHORITY-LOCAL`).
4. **Multi-Dimensional Conservation & Joint Feasibility**: Strict conservation and coupled constraint satisfaction across discrete physical dimensions with zero capacity leaks.
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
| **`MonotonicTick`** | 64-bit unsigned tick count (`u64`) originating from kernel hardware timer interrupts at 100 Hz resolution. |
| **`BootEpochId`** | 64-bit monotonic identifier established at kernel boot; guaranteed non-colliding across the operational lifetime of a `NodeId`. |
| **Joint Feasibility** | The property that an allocation vector simultaneously satisfies all individual dimensional limits and coupled hardware constraints. |
| **Reconciliation Window** | A bounded temporal grace period ($\Delta T_{\text{reconcile}}(L)$) following `resourced` restart during which a surviving lease may be re-attested. |
| **First Authoritative Observation** | The discrete point in software execution ($t_{\text{observed}}$) where a hardware event or driver failure is first authoritatively ingested. |

---

## 5. Tripartite Authority Model

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
|  * Joint feasibility evaluation across coupled constraints (I-FEASIBILITY)  |
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

---

## 6. Monotonic Time Observation Interface & Boot Epoch

### 6.1 The Ring 0 / Ring 3 Boundary
`resourced` executes in **Ring 3 User Space**. It **never executes privileged instructions and never accesses Ring 0 LAPIC MSRs directly**.

$$\mathbf{Invariant\ I-TIME-AUTHORITY-MONOTONIC:}$$
$$\text{The Stage 3 kernel nucleus is the sole owner of hardware monotonic time.}$$
$$\text{\texttt{resourced} strictly consumes an authenticated observation via an SMP memory-ordered seqlock.}$$
$$\text{Wall-clock synchronization (NTP/PTP) MUST NOT be required or permitted to influence lease validity.}$$

### 6.2 The Rust-Sound Memory-Ordered Observation Contract
The `TimeObservationFrame` is allocated in standard, cache-coherent physical memory mapped into both the kernel high-half and `resourced` via Stage 3G `SYS_SHM_MAP` (`PAGE_NX | SHM_MAP_READ`). Hardware cache coherency (MESI protocol across up to 4 SMP cores from Stage 3N) guarantees store visibility.

To ensure total soundness under the Rust memory model without data races:

```rust
#[repr(C)]
#[derive(Debug)]
pub struct TimeObservationFrame {
    pub sequence: core::sync::atomic::AtomicU64,        // Even = quiescent; Odd = write in progress
    pub boot_epoch: core::sync::atomic::AtomicU64,      // Monotonically non-colliding boot ID
    pub monotonic_ticks: core::sync::atomic::AtomicU64, // Calibrated 100 Hz counter (10 ms)
    pub frequency_hz: core::sync::atomic::AtomicU32,    // Fixed 100 Hz
    pub _pad: u32,                                      // Alignment padding
    pub _reserved: [u64; 4],                            // Pad to exactly 64 bytes
}

const _: () = assert!(core::mem::size_of::<TimeObservationFrame>() == 64);
const _: () = assert!(core::mem::align_of::<TimeObservationFrame>() == 8);
```

#### Writer Protocol (Stage 3 Kernel Timer Interrupt on BSP):
```rust
pub unsafe fn update_kernel_time_frame(frame: &TimeObservationFrame, current_epoch: u64, next_tick: u64) {
    let s = frame.sequence.load(core::sync::atomic::Ordering::Relaxed);
    if s >= u64::MAX - 2 {
        // Freeze sequence odd on terminal exhaustion (I-TIME-SEQUENCE-NONWRAP)
        frame.sequence.store(u64::MAX, core::sync::atomic::Ordering::Release);
        return;
    }
    // 1. Publish odd sequence with Release indicating mutation start
    frame.sequence.store(s + 1, core::sync::atomic::Ordering::Release);
    
    // 2. Atomic relaxed payload updates
    frame.boot_epoch.store(current_epoch, core::sync::atomic::Ordering::Relaxed);
    frame.monotonic_ticks.store(next_tick, core::sync::atomic::Ordering::Relaxed);
    frame.frequency_hz.store(100, core::sync::atomic::Ordering::Relaxed);
    
    // 3. Publish even sequence with Release indicating stable snapshot
    frame.sequence.store(s + 2, core::sync::atomic::Ordering::Release);
}
```

#### Reader Protocol (`libzero` / `resourced` in Ring 3):
```rust
pub fn read_monotonic_time(frame: &TimeObservationFrame) -> Result<(u64, u64), ZeroError> {
    let mut spins = 0;
    loop {
        // 1. Acquire load of initial sequence
        let s1 = frame.sequence.load(core::sync::atomic::Ordering::Acquire);
        if s1 & 1 != 0 {
            spins += 1;
            if spins > 16 { return Err(ZeroError::TimeAuthorityUnavailable); }
            core::hint::spin_loop();
            continue;
        }

        // 2. Atomic relaxed payload loads (guaranteed not to be reordered before s1)
        let epoch = frame.boot_epoch.load(core::sync::atomic::Ordering::Relaxed);
        let ticks = frame.monotonic_ticks.load(core::sync::atomic::Ordering::Relaxed);

        // 3. Acquire load of terminating sequence
        let s2 = frame.sequence.load(core::sync::atomic::Ordering::Acquire);
        if s1 == s2 {
            return Ok((epoch, ticks));
        }

        spins += 1;
        if spins > 16 { return Err(ZeroError::TimeAuthorityUnavailable); }
        core::hint::spin_loop();
    }
}
```

### 6.3 Permanent Kernel Frame Pinning & Lifetime
$$\mathbf{Invariant\ I-TIME-FRAME-LIFETIME:}$$
$$\text{The time observation backing storage is permanently pinned in kernel physical memory for the entire}$$
$$\text{kernel boot epoch. User-space handle creation, transfer, or closure CANNOT destroy, unpin, or reclaim}$$
$$\text{the kernel's producer reference. The frame CANNOT be mapped with writable rights by any process.}$$

- The physical frame backing `TimeObservationFrame` is allocated during Stage 3A bootstrap as a `FrameState::Reserved` frame.
- It is registered in `KERNEL_OBJECT_TABLE` under PID 0 with permanent kernel mapping references.
- User processes only hold read-only capabilities (`cap_rights::SHM_MAP_READ | PAGE_NX`). Closing user handles leaves the kernel pin untouched, guaranteeing zero use-after-free in the timer IRQ.

### 6.4 Sequence Non-Wrapping Invariant
$$\mathbf{Invariant\ I-TIME-SEQUENCE-NONWRAP:}$$
$$\text{The sequence counter of \texttt{TimeObservationFrame} strictly uses checked addition.}$$
$$\text{If } s \ge \text{u64::MAX} - 2, \text{ the kernel timer producer freezes the sequence at an odd value (mutation latch),}$$
$$\text{permanently failing closed all subsequent reader snapshots with \texttt{ZeroError::TimeAuthorityUnavailable}.}$$
$$\text{The sequence counter NEVER wraps to 0.}$$

### 6.5 Publication Cadence vs. Observation Freshness
$$\mathbf{Invariant\ I-TIME-OBSERVATION-FRESHNESS:}$$
$$\text{Kernel publication cadence is fixed at 10 ms (100 Hz). A captured time observation}$$
$$\text{is valid for lease admission or renewal ONLY if its consumer observation age satisfies:}$$
$$\Delta T_{\text{observation\_age}} = T_{\text{evaluate}} - T_{\text{captured}} \le \text{MAX\_TIME\_OBSERVATION\_AGE}$$
$$\text{where } \text{MAX\_TIME\_OBSERVATION\_AGE} \le 5\text{ ticks (50 ms). Stale observations must be re-sampled.}$$

### 6.6 Non-Wrapping Monotonic Deadline Arithmetic
$$\mathbf{Invariant\ I-TIME-DEADLINE-NONWRAP:}$$
$$\text{Deadline arithmetic MUST use checked addition. If } T_{\text{current}} + \Delta T_{\text{duration}} > \text{u64::MAX} - 1,$$
$$\text{lease admission fails closed immediately with \texttt{ZeroError::DeadlineExhaustion}.}$$
$$\text{Deadline arithmetic NEVER wraps. Deadlines NEVER roll over to 0.}$$

- `0` is explicitly reserved as `DEADLINE_INVALID` (uninitialized/free lease slot).
- `u64::MAX` is explicitly reserved as `DEADLINE_EXHAUSTED`.
- Valid operational deadlines reside strictly in `1 ..= u64::MAX - 1`.

### 6.7 Write-Ahead Boot Epoch Durability & Invalidation
$$\mathbf{Invariant\ I-BOOT-EPOCH-DURABILITY:}$$
$$\text{Candidate epoch } E_{N+1} > E_N \text{ MUST be durably committed to non-volatile storage with a hardware flush}$$
$$\text{barrier BEFORE user-space execution begins. If durability cannot be guaranteed, boot fails closed:}$$
$$\texttt{KERNEL\_PANIC: BootEpochIdUniquenessFailed.}$$

$$\mathbf{Invariant\ I-BOOT-EPOCH-NONREUSE:}$$
$$\text{Within the lifetime of a node identity (}\text{NodeId}\text{), no two completed kernel boots may share}$$
$$\text{the same }\text{BootEpochId}\text{.}$$

$$\mathbf{Invariant\ I-LEASE-REBOOT-INVALIDATION:}$$
$$\text{A monotonic timestamp or lease token from a previous kernel boot can never validate a lease}$$
$$\text{in a new boot epoch. Any lease whose recorded \texttt{boot\_epoch} does not match the active}$$
$$\text{kernel \texttt{BootEpochId} is rejected immediately as dead.}$$

---

## 7. Multi-Dimensional Vector Model & Joint Feasibility

### 7.1 N-Dimensional Vector Conservation & Hard Dimension Bound
$$\mathbf{Invariant\ I-ACCOUNTING-DIMENSION-BOUNDED:}$$
$$\text{The maximum number of accounting dimensions per resource is strictly bounded at } \text{MAX\_ACCOUNTING\_DIMENSIONS} = 8.$$
$$\text{Any registration requesting } 0 \text{ or } > 8 \text{ dimensions MUST be rejected fail-closed with \texttt{UnsupportedResourceShape}.}$$

```rust
pub const MAX_ACCOUNTING_DIMENSIONS: usize = 8;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DimensionCapacityVector {
    pub dimension_count: u8,
    pub _pad: [u8; 7],
    pub phys: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub unavail: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub alloc: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub resv: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub consumed: [u32; MAX_ACCOUNTING_DIMENSIONS],
}
```

$$\mathbf{Invariant\ I-ACCOUNTING-CONSERVATION:}$$
$$\forall R, \forall d \in 0..\text{dimension\_count}(R): \quad C_{\text{avail}}(R, d) + C_{\text{resv}}(R, d) + C_{\text{alloc}}(R, d) + C_{\text{unavail}}(R, d) = C_{\text{phys}}(R, d)$$
Every term is strictly non-negative:
$$C_{\text{avail}} \ge 0, \quad C_{\text{resv}} \ge 0, \quad C_{\text{alloc}} \ge 0, \quad C_{\text{unavail}} \ge 0$$

### 7.2 Joint Multidimensional Feasibility Invariant
$$\mathbf{Invariant\ I-ACCOUNTING-FEASIBILITY:}$$
$$\text{Every admitted lease MUST satisfy the complete resource-capacity constraint set of its provider}$$
$$\text{across all dimensions simultaneously. No dimension may be individually valid while the combined}$$
$$\text{allocation violates a provider-defined coupling constraint: } \mathbf{A}_{\text{coupling}} \cdot (\mathbf{C}_{\text{alloc}} + \Delta \mathbf{C}) \le \mathbf{b}_{\text{coupling}}.$$

### 7.3 Atomic Accounting Domain Transitions
$$\mathbf{Invariant\ I-ACCOUNTING-ATOMIC-ADMISSION:}$$
$$\text{Exactly one writer may mutate an \texttt{AccountingDomain} at a time.}$$
$$\text{Admission, reservation, allocation, release, and reclamation form indivisible state transitions}$$
$$\text{with respect to competing admissions and observers. Observers never observe intermediate states.}$$

```rust
#[repr(C)]
pub struct AccountingDomain {
    /// 1. Writer spinlock ensuring single-writer mutual exclusion
    pub writer_lock: crate::sync::Spinlock,
    /// 2. Sequence counter for read versioning (odd = write in progress; even = quiescent)
    pub version: core::sync::atomic::AtomicU64,
    /// 3. Resource identity binding
    pub resource_id: DistributedId,
    /// 4. Provider joint constraint coupling profile ID
    pub coupling_profile_id: u8,
    pub _pad: [u8; 7],
    /// 5. Full N-dimensional capacity vector (up to MAX_ACCOUNTING_DIMENSIONS)
    pub capacity: DimensionCapacityVector,
}
```

---

## 8. Resource Descriptor Layout (Static 128 Bytes)

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
    pub coupling_profile_id: u8,        // 1 byte: Provider-defined joint constraint profile
    pub active_dimensions: u8,          // 1 byte: Number of active dimensions (1..8)

    // Operational State & Health (16 Bytes)
    pub state: ResourceLifecycleState,  // 1 byte: Available, Degraded, Detaching, etc.
    pub health: ResourceHealth,         // 1 byte: Healthy, Degraded, Critical, Faulted
    pub energy_tier: EnergyTier,        // 1 byte: Measured, Estimated, Declared
    pub thermal_state: ThermalState,    // 1 byte: Nominal, Throttling, Critical
    pub required_rights: u16,           // 2 bytes: Stage 3 capability rights mask
    pub active_leases_count: u16,       // 2 bytes: Number of active concurrent leases
    pub bound_kernel_handle: u32,       // 4 bytes: Process-local handle to kernel object
    pub _pad1: [u8; 4],                 // 4 bytes: Alignment padding

    // Multi-Dimensional Primary Dimension (Inline 32 Bytes)
    pub phys_capacity_d1: u64,          // 8 bytes: C_phys for Primary Dimension
    pub unavail_capacity_d1: u64,       // 8 bytes: C_unavail (throttled/offline)
    pub alloc_capacity_d1: u64,         // 8 bytes: C_alloc (committed leases)
    pub resv_capacity_d1: u64,          // 8 bytes: C_resv (soft headroom)

    // Multi-Dimensional Secondary Dimension (Inline 32 Bytes)
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

## 9. Resource Graph Model

### 9.1 The Separation Invariant
$$\mathbf{Invariant\ I-GRAPH-SEPARATION:}\quad \text{Resource Graph} \neq \text{Workload Task DAG}$$
- The **Resource Graph** captures *what hardware and logical capacity exists, how it is physically interconnected, and which leases bind to it*. It is a **general directed typed graph** that naturally contains cycles (e.g., Node $\to$ Device $\to$ Storage $\to$ Swap $\to$ RAM $\to$ Node).
- The **Workload Task DAG** captures *what computation needs to be accomplished and the flow of data dependencies between tasks*. It is **strictly acyclic** (`I-TASK-DAG-ACYCLIC`).

### 9.2 Graph Edge Semantics
The Resource Graph permits exactly 6 explicit, typed edges:
1. `Contains`: Hierarchical structural containment (Node $\to$ CPU Socket $\to$ Core).
2. `Provides`: Inception binding (Driver process provides Resource).
3. `DependsOn`: Hardware or functional dependency (NPU Engine depends on PCIe Bus).
4. `AttachedTo`: Interconnect physical binding (NVMe SSD attached to PCIe Controller).
5. `Backs`: Substrate backing relationship (Physical DRAM backs Virtual SHM Pool).
6. `LeasedBy`: Capacity commitment (Resource capacity leased to Consumer Process).

### 9.3 Static Edge Table (32 Bytes)
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

## 10. Resource Leases & Decoupled Authority

### 10.1 Authority vs. Capacity Invariant
$$\mathbf{Capability} \neq \mathbf{Lease}$$
$$\mathbf{Invariant\ I-LEASE-AUTH-BOUNDED:}\quad \text{Lease Authority} \subseteq \text{Capability Authority}$$
- A **Capability** grants unforgeable mathematical authorization to interact with an object.
- A **Lease** grants time-bounded, metered physical capacity of that object.
- Presenting a lease without a valid kernel capability confers zero authority.
- `resourced` rejects any lease request whose required rights exceed the caller's presented capability (`I-RES-CAP-NO-AMPLIFICATION`).

### 10.2 Canonical Lease Record (Static 96 Bytes)
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
    pub boot_epoch: u64,                 // 8 bytes: Boot epoch at creation
    pub expiration_tick: u64,            // 8 bytes: Absolute monotonic tick deadline
}

const _: () = assert!(core::mem::size_of::<ResourceLease>() == 96);
const _: () = assert!(core::mem::align_of::<ResourceLease>() == 8);
```

---

## 11. Bounded Crash Reconciliation Semantics

### 11.1 The Non-Resurrection Reconciliation Invariant
$$\mathbf{Invariant\ I-LEASE-RECONCILIATION-BOUNDED:}$$
$$\forall L \in \text{SurvivingLeases}: \quad \Delta T_{\text{reconcile}}(L) = \min\left(\Delta T_{\text{policy\_max}},\; T_{\text{remaining}}(L)\right)$$
$$\text{A post-crash reconciliation window CAN NEVER exceed the lease's pre-crash remaining lifetime:}$$
$$\Delta T_{\text{reconcile}}(L) \le T_{\text{remaining}}(L)$$
$$\text{Reconciliation CAN NEVER resurrect an already-expired lease, and consumers CANNOT self-extend.}$$

### 11.2 Individual Feasibility Evaluation
For each surviving lease $L$, its remaining lifetime is:
$$T_{\text{remaining}}(L) = L.\text{expiration\_tick} - T_{\text{current}}$$
- If $T_{\text{remaining}}(L) \le 0$: Expired during downtime. Re-attestation is rejected immediately; capacity is freed fail-closed.
- If $T_{\text{remaining}}(L) > 0$: The consumer must submit `OP_LEASE_RECONCILE` *before* $L.\text{expiration\_tick}$ passes.
- **Fairness Guarantee**: When $T_{\text{remaining}}(L) \ge \Delta T_{\text{policy\_target}}$ ($100\text{ ticks} = 1.0\text{ s}$), the consumer is guaranteed at least $\Delta T_{\text{policy\_target}}$ to overcome scheduler delay. If $T_{\text{remaining}}(L) < 100\text{ ticks}$, the reconciliation deadline is strictly $L.\text{expiration\_tick}$.
- **Zero Self-Extension**: The reinstated lease strictly retains $L.\text{expiration\_tick}$.
- **Reconciliation Close**: Once $\Delta T_{\text{reconcile}}$ expires, all un-reconciled capacity is declared Free, and normal admission resumes.

---

## 12. Authoritative Hotplug Observation Boundary

### 12.1 Physical Disappearance vs. Software Observation
Software cannot observe physical detachment with zero latency:

$$t_{\text{phys}} \xrightarrow{\text{Hardware Line Drop}} t_{\text{detect}} \xrightarrow{\text{Kernel/Driver IRQ}} t_{\text{notify}} \xrightarrow{\text{IPC / PeerClosed}} t_{\text{observed}}$$

$$\mathbf{Invariant\ I-RES-DISAPPEARANCE-CASCADE:}$$
$$\text{\texttt{ProviderLost} becomes authoritative at the FIRST provider-authoritative observation of disappearance}$$
$$\text{or hardware fault (} t_{\text{observed}} \text{). \texttt{resourced} transitions the resource to \texttt{Unavailable}, sets } C_{\text{unavail}} \leftarrow C_{\text{phys}},$$
$$\text{and immediately cascades all associated active leases to \texttt{ProviderLost} fail-closed.}$$

Hardware interaction attempts between $t_{\text{phys}}$ and $t_{\text{observed}}$ trap safely at the Stage 3 kernel MMIO/bus level.

---

## 13. Quota & Energy Models

### 13.1 Quota Invariants
1. `I-QUOTA-BOUNDED`: Total capacity allocated across all active leases held by an entity cannot exceed its provisioned quota ceiling.
2. `I-QUOTA-FAIL-CLOSED`: When an entity reaches its quota ceiling, subsequent lease requests fail immediately with `ZeroError::QuotaExceeded`.
3. `I-QUOTA-CLEANUP`: When an entity terminates or a lease is reclaimed, quota usage counters are decremented monotonically.

### 13.2 Energy Invariant
$$\mathbf{Invariant\ I-RES-ENERGY-LIMITS:}$$
$$\text{Security boundaries, capability validation, and hard capacity admission decisions}$$
$$\text{MUST NOT depend on untrusted \texttt{DECLARED} or unverified \texttt{ESTIMATED} telemetry.}$$
$$\text{Hard thermal/power shedding triggers strictly on \texttt{MEASURED} PMIC telemetry.}$$

---

## 14. IPC / API Boundary (Stage 3G Protocol)

All communication uses the frozen 80-byte `IpcMessage` layout over Stage 3G bidirectional channels:

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

### Protocol Opcodes

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

## 15. Authoritative Phase 4B Invariant Catalog (26 Invariants)

1. `I-RES-ID-UNIQUE`: Every `ResourceId` and `LeaseId` is globally unique via composite `(NodeId, LocalSeq)` tuples.
2. `I-ID-DURABLE-ALLOCATOR-STATE`: Identifier allocator sequence ceilings must be durably committed before any identifier becomes externally observable.
3. `I-BOOT-EPOCH-DURABILITY`: Candidate epoch $E_{N+1} > E_N$ must be durably committed to non-volatile storage with a flush barrier before user space begins; fails closed on uncertainty.
4. `I-BOOT-EPOCH-NONREUSE`: Within the lifetime of a node identity (`NodeId`), no two completed kernel boots may share the same `BootEpochId`.
5. `I-RES-AUTHORITY-LOCAL`: A node is authoritative solely over its locally hosted physical resources. Remote observations are advisory.
6. `I-RES-CAPABILITY-NO-AMPLIFICATION`: `resourced` cannot grant rights exceeding the authorizing capability token presented by the caller.
7. `I-LEASE-AUTH-BOUNDED`: A lease grants physical capacity, never capability authority: $\text{Lease Authority} \subseteq \text{Capability Authority}$.
8. `I-TIME-AUTHORITY-MONOTONIC`: The Stage 3 kernel nucleus owns hardware monotonic time. `resourced` strictly consumes an authenticated observation via an SMP-ordered atomic seqlock.
9. `I-TIME-FRAME-LIFETIME`: The time observation backing storage is permanently pinned in kernel physical memory for the entire boot epoch. Handle closure cannot reclaim it.
10. `I-TIME-SEQUENCE-NONWRAP`: The sequence counter of `TimeObservationFrame` uses checked addition; if $s \ge \text{u64::MAX} - 2$, the producer freezes odd, failing readers closed.
11. `I-TIME-OBSERVATION-FRESHNESS`: An observation older than `MAX_TIME_OBSERVATION_AGE` ($\le 50\text{ ms}$) cannot be used for admission or renewal.
12. `I-TIME-DEADLINE-NONWRAP`: Deadline arithmetic must use checked addition. If $T_{\text{current}} + \Delta T_{\text{duration}} > \text{u64::MAX} - 1$, lease admission fails closed.
13. `I-LEASE-REBOOT-INVALIDATION`: A monotonic timestamp or lease token from a previous kernel boot can never validate a lease in a new boot epoch.
14. `I-LEASE-CLEANUP`: When a consumer process terminates or disconnects, all associated leases are reclaimed without leaks.
15. `I-LEASE-STALE-REJECTION`: Any operation presenting a stale lease generation counter is rejected fail-closed with `GenerationMismatch`.
16. `I-ACCOUNTING-NONNEGATIVE`: All capacity terms ($C_{\text{phys}}, C_{\text{unavail}}, C_{\text{allocatable}}, C_{\text{resv}}, C_{\text{alloc}}, C_{\text{avail}}$) are strictly non-negative across all dimensions.
17. `I-ACCOUNTING-CONSERVATION`: For every resource and dimension, $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$ holds invariant across all operations.
18. `I-ACCOUNTING-FEASIBILITY`: Every admitted lease must satisfy the complete resource-capacity constraint set of its provider across all dimensions simultaneously.
19. `I-ACCOUNTING-DIMENSION-BOUNDED`: Maximum accounting dimensions per resource is strictly bounded at $\text{MAX\_ACCOUNTING\_DIMENSIONS} = 8$; larger requests rejected with `UnsupportedResourceShape`.
20. `I-ACCOUNTING-ATOMIC-ADMISSION`: Exactly one writer may mutate an `AccountingDomain` at a time under `writer_lock`. Transitions are indivisible.
21. `I-LEASE-RECONCILIATION-BOUNDED`: For all surviving leases, $\Delta T_{\text{reconcile}}(L) \le T_{\text{remaining}}(L)$. Reconciliation cannot resurrect expired leases or self-extend.
22. `I-RES-DISAPPEARANCE-CASCADE`: `ProviderLost` becomes authoritative at the first provider-authoritative observation of disappearance ($t_{\text{observed}}$), cascading leases fail-closed.
23. `I-RES-ENERGY-LIMITS`: Hard resource limits and safety triggers cannot depend on untrusted `ESTIMATED` or `DECLARED` energy telemetry.
24. `I-QUOTA-BOUNDED`: Total capacity leased by an entity cannot exceed its provisioned quota ceiling.
25. `I-GRAPH-SEPARATION`: The Resource Graph models physical and logical topology and may contain cycles; the Workload Task DAG is strictly acyclic.
26. `I-STATIC-BOUNDS`: All internal tables in `resourced` have statically fixed maximum capacities with zero unbounded dynamic heap allocation.

---

## 16. Verification Strategy (Machine Verification Tests 4B-A through 4B-T)

1. **4B-A: Multi-Dimensional Registration**: Register heterogeneous resources (CPU, GPU, RAM); verify multi-dimensional $C_{\text{phys}}$ initialization.
2. **4B-B: Dimension Bound Rejection**: Attempt to register a resource with 9 dimensions; verify immediate rejection with `UnsupportedResourceShape`.
3. **4B-C: Vector Conservation**: Perform allocations, reservations, and thermal throttling; assert $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$ across all active dimensions.
4. **4B-D: Joint Coupling Feasibility**: Submit requests that satisfy individual dimensions but violate coupled constraints; verify `CouplingViolation` rejection.
5. **4B-E: Atomic Admission Serialization**: Issue concurrent competing lease requests under SMP load; verify zero double-allocations or intermediate state leaks.
6. **4B-F: Rust-Sound Time Seqlock**: Simulate concurrent writer tick updates and reader loops over atomic fields; assert zero torn reads or data races.
7. **4B-G: Time Frame Lifetime Pinning**: Close user-space handle to time observation frame; verify kernel producer continues writing without fault or reclamation.
8. **4B-H: Sequence Non-Wrapping Latch**: Force sequence counter to `u64::MAX - 2`; verify kernel freezes odd and readers fail closed with `TimeAuthorityUnavailable`.
9. **4B-I: Time Observation Freshness**: Inject simulated scheduling pause $> 50\text{ ms}$; verify `TimeObservationStale` rejection until re-sampled.
10. **4B-J: Deadline Arithmetic Non-Wrap**: Request lease with `duration = u64::MAX`; verify `DeadlineExhaustion` rejection without wrapping to 0.
11. **4B-K: Boot Epoch Durability & Non-Reuse**: Verify write-ahead epoch commit; assert reboot epoch $E_{N+1} > E_N$; verify pre-reboot leases rejected immediately under `I-LEASE-REBOOT-INVALIDATION`.
12. **4B-L: Capability-Mediated Admission**: Verify lease request succeeds with covering capability; fails with `PermissionDenied` when rights are attenuated.
13. **4B-M: Authority Amplification Rejection**: Verify client cannot obtain rights exceeding its presented capability.
14. **4B-N: Stale Generation Rejection**: Attempt renewal using pre-reclamation generation token; verify `GenerationMismatch`.
15. **4B-O: Consumer Crash Cleanup**: Terminate consumer process abruptly; verify `PeerClosed` signal triggers deterministic reclamation.
16. **4B-P: Hotplug Disappearance Cascade**: Simulate driver failure; verify at $t_{\text{observed}}$ resource transitions to `Unavailable`, $C_{\text{unavail}} \leftarrow C_{\text{phys}}$, and leases transition to `ProviderLost`.
17. **4B-Q: Non-Resurrecting Reconciliation**: Attempt to re-attest lease with $T_{\text{remaining}} \le 0$; verify rejection. For lease with 40 ticks remaining, verify reconciliation window is exactly 40 ticks.
18. **4B-R: Local Quota Enforcement**: Request capacity exceeding quota limit; verify `QuotaExceeded` rejection while node has free capacity.
19. **4B-S: Energy Classification Integrity**: Submit declared vs. measured telemetry; verify hard safety thresholds ignore declared telemetry.
20. **4B-T: PMM Neutrality & Non-Reuse**: Verify zero physical memory frame leakage and verify sequence allocator resumes above ZeroFS-persisted ceiling.

---

## 17. Implementation Boundary Notice

**Implementation is strictly prohibited during this discovery phase.**  
No code, table definitions, or test runners shall be added until this Revision 6 architecture specification is formally reviewed and approved.
