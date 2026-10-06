# STAGE 4B ARCHITECTURE SPECIFICATION (REV 4)

## Unified Resource Graph & Local Node Accounting (`resourced`)

**Status:** 🟡 ARCHITECTURAL SPECIFICATION — REVISION 4 (FINAL SURGICAL PASS)  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-09-22  
**Target Milestone:** Stage 4B (Unified Resource Graph & Local Node Accounting)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024 Rev6  
**Kernel Baseline:** Stage 3A–3N Hard Frozen & Inviolate  
**System Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Implemented, Verified, Frozen  

---

## 1. Executive Summary of Revision 4 Refinements

Following the third round of adversarial architecture review, **Revision 4** surgically resolves the final five technical specifications:

1. **Concrete Monotonic Time Observation Contract (`I-TIME-AUTHORITY-MONOTONIC`)**:
   Eliminates hypothetical timer devices. Formally defines the Stage 4 user-space time observation interface: a 64-byte, read-only shared-memory observation frame (`TimeObservationFrame`) established via Stage 3G `SYS_SHM_MAP` (`PAGE_NX | SHM_MAP_READ`). Defines snapshot semantics, sequence loop atomicity, bounded staleness ($\le 10\text{ ms}$), and fail-closed behavior without modifying frozen Stage 3 syscalls.
2. **Non-Wrapping Monotonic Deadline Arithmetic (`I-TIME-DEADLINE-NONWRAP`)**:
   Mandates checked arithmetic for all lease deadlines. If $T_{\text{current}} + \Delta T_{\text{duration}} > \text{u64::MAX} - 1$, lease admission fails closed with `ZeroError::DeadlineExhaustion`. Explicitly reserves `0` as `DEADLINE_INVALID` and `u64::MAX` as `DEADLINE_EXHAUSTED`.
3. **Mathematically Consistent Crash Reconciliation Bounds (`I-LEASE-CRASH-RECONCILIATION`)**:
   Resolves the contradiction between $\Delta T_{\text{min\_reconcile}}$ and short-lived leases. Re-attestation is permitted strictly while a surviving lease's remaining lifetime $T_{\text{remaining}}(L) > 0$. Surviving leases with $T_{\text{remaining}} < \Delta T_{\text{policy}}$ must re-attest before their individual expiration ticks elapse. Guarantees that reconciliation can never resurrect an expired lease or allow consumer self-extension.
4. **Rigorous Writer-Serialized `AccountingDomain` (`I-ACCOUNTING-ATOMIC-ADMISSION`)**:
   Clarifies `AccountingDomain` structure: writer serialization spinlock, sequence/version counter, and multi-dimensional capacity state. Enforces that exactly one writer mutates an accounting domain at a time, forming indivisible transitions for admission, reservation, allocation, and reclamation.
5. **Decoupled Boot-Epoch Invalidation (`I-LEASE-REBOOT-INVALIDATION`)**:
   Decouples kernel boot epoch establishment from Stage 3K ZeroFS persistence. Kernel establishes an in-memory `BootEpochId: u64` at Stage 3A initialization before timer start. Guarantees that timestamps from a previous kernel boot can never validate a lease in a new epoch.

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
| **`BootEpochId`** | 64-bit identifier established at kernel boot; changes across every system reboot. |
| **Joint Feasibility** | The property that an allocation vector simultaneously satisfies all individual dimensional limits and coupled hardware constraints. |
| **Reconciliation Window** | A policy-bounded temporal grace period ($\Delta T_{\text{reconcile}}$) following `resourced` restart during which live consumers may re-attest surviving leases. |
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

## 6. Authoritative Monotonic Time Interface & Boot Epoch

### 6.1 The Ring 0 / Ring 3 Boundary
`resourced` is an unprivileged **Ring 3 user-space service**. The LAPIC and CPU MSRs are privileged hardware registers configured in Stage 3B/3N. `resourced` **never executes privileged instructions and never accesses Ring 0 LAPIC MSRs directly**.

$$\mathbf{Invariant\ I-TIME-AUTHORITY-MONOTONIC:}$$
$$\text{The Stage 3 kernel nucleus is the sole owner of hardware monotonic time.}$$
$$\text{\texttt{resourced} strictly consumes an authenticated, kernel-mediated monotonic-time observation.}$$
$$\text{Wall-clock synchronization (NTP/PTP) MUST NOT be required or permitted to influence lease validity.}$$

### 6.2 The Monotonic Time Observation Contract
To avoid modifying the frozen Stage 3 syscall table while providing non-blocking sub-microsecond time queries, the Stage 4 runtime establishes the **Monotonic Time Observation Interface** backed by Stage 3G Shared Memory:

1. **Producer**: Stage 3 Kernel nucleus timer interrupt handler (`TICKS` at 100 Hz).
2. **Mechanism**: A 64-byte shared memory object (`TimeObservationFrame`) created via Stage 3G `SYS_SHM_CREATE`, mapped into `resourced` by `init` with rights `SHM_MAP_READ | PAGE_NX`.
3. **Consumer**: `resourced` (and `libzero::time` client).
4. **Authorization**: Presenting a valid Stage 3 capability handle with `cap_rights::SHM_MAP_READ`.
5. **Snapshot Layout (64 Bytes)**:

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TimeObservationFrame {
    pub sequence: u64,        // Sequence counter (even = quiescent; odd = kernel updating)
    pub boot_epoch: u64,      // Unique 64-bit identifier for current kernel boot
    pub monotonic_ticks: u64, // Monotonic tick counter at 100 Hz (10 ms resolution)
    pub frequency_hz: u32,    // Fixed 100 Hz
    pub _pad: u32,            // Explicit padding
    pub _reserved: [u64; 4],  // Pad to exactly 64 bytes
}

const _: () = assert!(core::mem::size_of::<TimeObservationFrame>() == 64);
const _: () = assert!(core::mem::align_of::<TimeObservationFrame>() == 8);
```

6. **Observation Semantics**:
   - `resourced` reads the snapshot using a lockless sequence loop:
     ```rust
     pub fn read_monotonic_time(frame: &TimeObservationFrame) -> Result<(u64, u64), ZeroError> {
         let mut spins = 0;
         loop {
             let seq1 = core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Acquire);
             let s1 = frame.sequence;
             if s1 & 1 != 0 {
                 spins += 1;
                 if spins > 16 { return Err(ZeroError::TimeAuthorityUnavailable); }
                 core::hint::spin_loop();
                 continue;
             }
             let epoch = frame.boot_epoch;
             let ticks = frame.monotonic_ticks;
             let s2 = frame.sequence;
             if s1 == s2 {
                 return Ok((epoch, ticks));
             }
             spins += 1;
             if spins > 16 { return Err(ZeroError::TimeAuthorityUnavailable); }
             core::hint::spin_loop();
         }
     }
     ```
7. **Maximum Staleness**: Bounded strictly by 1 timer tick ($10\text{ ms}$).
8. **Failure Behavior**: If the frame is unmapped, unreadable, or spins exceed 16, `resourced` halts lease admission and fails closed with `ZeroError::TimeAuthorityUnavailable`.

### 6.3 Non-Wrapping Monotonic Deadline Arithmetic
$$\mathbf{Invariant\ I-TIME-DEADLINE-NONWRAP:}$$
$$\text{Deadline arithmetic MUST use checked addition. If } T_{\text{current}} + \Delta T_{\text{duration}} > \text{u64::MAX} - 1,$$
$$\text{lease admission fails closed immediately with \texttt{ZeroError::DeadlineExhaustion}.}$$
$$\text{Deadline arithmetic NEVER wraps. Deadlines NEVER roll over to 0.}$$

- `0` is explicitly reserved as `DEADLINE_INVALID` (unallocated / uninitialized lease slot).
- `u64::MAX` is explicitly reserved as `DEADLINE_EXHAUSTED`.
- Valid operational deadlines reside strictly in the range `1 ..= u64::MAX - 1`.
- Expiration evaluation ($T_{\text{current}} \ge T_{\text{deadline}}$) is completely monotonic and overflow-free.

### 6.4 Reboot Epoch Invalidation
$$\mathbf{Invariant\ I-LEASE-REBOOT-INVALIDATION:}$$
$$\text{A monotonic timestamp or lease token from a previous kernel boot can never validate a lease}$$
$$\text{in the new boot epoch. Any lease whose recorded \texttt{boot\_epoch} does not match the active}$$
$$\text{kernel \texttt{BootEpochId} is rejected immediately as invalid.}$$

- At Stage 1/3A kernel bootstrap, before any timer interrupt fires, the kernel establishes an in-memory `BootEpochId: u64` (derived from hardware seed / boot instance counter).
- ZeroFS (Stage 3K) is NOT required for timer initialization or early boot.
- All pre-reboot leases are automatically dead because volatile physical frames and capability handle tables are vaporized upon reboot.

---

## 7. Multi-Dimensional Vector Model & Joint Feasibility

### 7.1 Multi-Dimensional Vector Conservation
For every managed resource $R$ and for every accounting dimension $d \in \text{Dimensions}(R)$, `resourced` enforces:

$$\begin{aligned}
C_{\text{phys}}(R, d) &\ge 0 \quad (\text{Authoritative physical hardware envelope}) \\
C_{\text{unavail}}(R, d) &\ge 0 \quad (\text{Temporarily unusable: thermal, faulted, or firmware reserved}) \\
C_{\text{allocatable}}(R, d) &= C_{\text{phys}}(R, d) - C_{\text{unavail}}(R, d) \\
C_{\text{resv}}(R, d) &\ge 0 \quad (\text{Soft reservation headroom}) \\
C_{\text{alloc}}(R, d) &\ge 0 \quad (\text{Hard contractual commitment to active leases}) \\
C_{\text{avail}}(R, d) &= C_{\text{allocatable}}(R, d) - (C_{\text{resv}}(R, d) + C_{\text{alloc}}(R, d)) \\
C_{\text{consumed}}(R, d) &\ge 0 \quad (\text{Dynamically observed physical utilization; advisory telemetry})
\end{aligned}$$

$$\mathbf{Invariant\ I-ACCOUNTING-CONSERVATION:}$$
$$\forall R, \forall d \in \text{Dimensions}(R): \quad C_{\text{avail}}(R, d) + C_{\text{resv}}(R, d) + C_{\text{alloc}}(R, d) + C_{\text{unavail}}(R, d) = C_{\text{phys}}(R, d)$$

### 7.2 Joint Multidimensional Feasibility Invariant
Individual dimensional conservation is necessary but insufficient when physical hardware imposes coupling constraints across dimensions (e.g. GPU compute units requiring minimum memory bandwidth or command queue memory pinning).

$$\mathbf{Invariant\ I-ACCOUNTING-FEASIBILITY:}$$
$$\text{Every admitted lease MUST satisfy the complete resource-capacity constraint set of its provider}$$
$$\text{across all dimensions simultaneously. No dimension may be individually valid while the combined}$$
$$\text{allocation violates a provider-defined coupling constraint: } \mathbf{A}_{\text{coupling}} \cdot (\mathbf{C}_{\text{alloc}} + \Delta \mathbf{C}) \le \mathbf{b}_{\text{coupling}}.$$

The provider registers a `ConstraintProfile` in `ResourceDescriptor`. `resourced` verifies independent vector headroom, joint linear/discrete coupling constraints, and process quotas simultaneously.

### 7.3 Atomic Accounting Domain Transitions
$$\mathbf{Invariant\ I-ACCOUNTING-ATOMIC-ADMISSION:}$$
$$\text{Exactly one writer may mutate an \texttt{AccountingDomain} at a time.}$$
$$\text{Admission, reservation, allocation, release, and reclamation form indivisible state transitions}$$
$$\text{with respect to competing admissions and observers. Observers never observe intermediate states.}$$

```rust
#[repr(C)]
pub struct AccountingDomain {
    /// 1. Writer serialization spinlock ensuring single-writer mutual exclusion
    pub writer_lock: crate::sync::Spinlock,
    /// 2. Sequence/version counter (odd = write in progress; even = quiescent)
    pub version: core::sync::atomic::AtomicU64,
    /// 3. Multi-dimensional capacity accounting state
    pub phys_capacity_d1: u64,
    pub unavail_capacity_d1: u64,
    pub alloc_capacity_d1: u64,
    pub resv_capacity_d1: u64,
    pub phys_capacity_d2: u64,
    pub unavail_capacity_d2: u64,
    pub alloc_capacity_d2: u64,
    pub resv_capacity_d2: u64,
}
```
All transitions (`Available -> Reserved`, `Reserved -> Allocated`, `Allocated -> Reclaimed`) are executed under `AccountingDomain.writer_lock`. Competing requests on the same resource cannot interleave or double-allocate capacity. Requires zero kernel locks.

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
    pub _pad0: u8,                      // 1 byte: Alignment padding

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

## 9. Resource Graph Model

### 9.1 The Separation Invariant
$$\mathbf{Invariant\ I-GRAPH-SEPARATION:}\quad \text{Resource Graph} \neq \text{Workload Task DAG}$$
- The **Resource Graph** captures *what hardware and logical capacity exists, how it is physically interconnected, and which leases bind to it*. It is a **general directed typed graph** that naturally contains cycles (e.g., Node $\to$ Device $\to$ Storage $\to$ Swap $\to$ RAM $\to$ Node).
- The **Workload Task DAG** captures *what computation needs to be accomplished and the flow of data dependencies between tasks*. It is **strictly acyclic** (`I-TASK-DAG-ACYCLIC`).

### 9.2 Graph Edge Semantics
The Resource Graph permits exactly 6 explicit, typed edges:
1. `Contains`: Hierarchical structural containment (Parent contains Child: Node $\to$ CPU Socket $\to$ Core).
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

## 11. Crash Reconciliation Semantics

### 11.1 The Crash Invalidation Rule
$$\mathbf{Invariant\ I-LEASE-CRASH-RECONCILIATION:}$$
$$\text{After \texttt{resourced} restart, no pre-crash lease is considered valid merely because the consumer}$$
$$\text{possesses an old token. Validity requires successful re-attestation within } \Delta T_{\text{reconcile}}.$$
$$\text{Reconciliation CAN NEVER resurrect an expired lease. Consumers CANNOT self-extend.}$$

### 11.2 Individual Lease Reconciliation Bounds
For each surviving lease $L$, its remaining lifetime is:
$$T_{\text{remaining}}(L) = L.\text{expiration\_tick} - T_{\text{current}}$$
- If $T_{\text{remaining}}(L) \le 0$: The lease expired during downtime. Re-attestation fails immediately; the lease is purged fail-closed.
- If $T_{\text{remaining}}(L) > 0$: The consumer is permitted to re-attest *only before* its specific expiration tick elapses.
- The global reconciliation window $\Delta T_{\text{reconcile}}$ is bounded by:
  $$\Delta T_{\text{reconcile}} = \min(\text{PolicyWindow},\, \Delta T_{\text{global\_max}})$$
  where $\Delta T_{\text{global\_max}} = 500\text{ ticks}$ ($5.0\text{ s}$).
  If surviving leases have $T_{\text{remaining}} \ge \Delta T_{\text{policy\_min}}$ ($100\text{ ticks} = 1.0\text{ s}$), consumers are granted at least $\Delta T_{\text{policy\_min}}$ to re-attest without CPU starvation. If an individual lease has $T_{\text{remaining}} < \Delta T_{\text{policy\_min}}$, its reconciliation window is strictly $T_{\text{remaining}}$.
- **Zero Self-Extension**: The reinstated lease retains strictly $L.\text{expiration\_tick}$.
- **Reconciliation Close**: Once $\Delta T_{\text{reconcile}}$ expires, all un-reconciled capacity is declared Free, and normal admission resumes.

---

## 12. Authoritative Hotplug Observation Boundary

### 12.1 Physical Disappearance vs. Software Observation
Software cannot observe physical detachment with zero latency. There is an inevitable physical detection interval:

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

## 15. Authoritative Phase 4B Invariant Catalog (19 Invariants)

1. `I-RES-ID-UNIQUE`: Every `ResourceId` and `LeaseId` is globally unique via composite `(NodeId, LocalSeq)` tuples.
2. `I-ID-DURABLE-ALLOCATOR-STATE`: Identifier allocator sequence ceilings must be durably committed before any identifier becomes externally observable.
3. `I-RES-AUTHORITY-LOCAL`: A node is authoritative solely over its locally hosted physical resources. Remote observations are advisory.
4. `I-RES-CAPABILITY-NO-AMPLIFICATION`: `resourced` cannot grant rights exceeding the authorizing capability token presented by the caller.
5. `I-LEASE-AUTH-BOUNDED`: A lease grants physical capacity, never capability authority: $\text{Lease Authority} \subseteq \text{Capability Authority}$.
6. `I-TIME-AUTHORITY-MONOTONIC`: The Stage 3 kernel nucleus owns hardware monotonic time. `resourced` strictly consumes an authenticated monotonic-time observation. Wall clock can never affect lease validity.
7. `I-TIME-DEADLINE-NONWRAP`: Deadline arithmetic must use checked addition. If $T_{\text{current}} + \Delta T_{\text{duration}} > \text{u64::MAX} - 1$, lease admission fails closed. Arithmetic never wraps.
8. `I-LEASE-REBOOT-INVALIDATION`: A monotonic timestamp or lease token from a previous kernel boot can never validate a lease in a new boot epoch.
9. `I-LEASE-CLEANUP`: When a consumer process terminates or disconnects, all associated leases are reclaimed without leaks.
10. `I-LEASE-STALE-REJECTION`: Any operation presenting a stale lease generation counter is rejected fail-closed with `GenerationMismatch`.
11. `I-ACCOUNTING-NONNEGATIVE`: All capacity terms ($C_{\text{phys}}, C_{\text{unavail}}, C_{\text{allocatable}}, C_{\text{resv}}, C_{\text{alloc}}, C_{\text{avail}}$) are strictly non-negative across all dimensions.
12. `I-ACCOUNTING-CONSERVATION`: For every resource and dimension, $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$ holds invariant across all operations.
13. `I-ACCOUNTING-FEASIBILITY`: Every admitted lease must satisfy the complete resource-capacity constraint set of its provider across all dimensions simultaneously. No dimension may violate provider-defined coupling constraints.
14. `I-ACCOUNTING-ATOMIC-ADMISSION`: Exactly one writer may mutate an `AccountingDomain` at a time. Admission, reservation, allocation, release, and reclamation are indivisible state transitions.
15. `I-LEASE-CRASH-RECONCILIATION`: After `resourced` restart, no lease is valid merely by holding an old token. Reinstatement requires successful re-attestation before expiration. No consumer can self-extend.
16. `I-RES-DISAPPEARANCE-CASCADE`: `ProviderLost` becomes authoritative at the first provider-authoritative observation of disappearance ($t_{\text{observed}}$), setting $C_{\text{unavail}} \leftarrow C_{\text{phys}}$ and cascading leases fail-closed.
17. `I-RES-ENERGY-LIMITS`: Hard resource limits and safety triggers cannot depend on untrusted `ESTIMATED` or `DECLARED` energy telemetry.
18. `I-QUOTA-BOUNDED`: Total capacity leased by an entity cannot exceed its provisioned quota ceiling.
19. `I-GRAPH-SEPARATION`: The Resource Graph models physical and logical topology and may contain cycles; the Workload Task DAG is strictly acyclic.

---

## 16. Verification Strategy (Machine Verification Tests 4B-A through 4B-Q)

1. **4B-A: Multi-Dimensional Registration**: Register heterogeneous resources (CPU, GPU, RAM); verify multi-dimensional $C_{\text{phys}}$ initialization.
2. **4B-B: Vector Conservation**: Perform allocations, reservations, and thermal throttling; assert $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$ across all dimensions.
3. **4B-C: Joint Coupling Feasibility**: Submit requests that satisfy individual dimensions but violate coupled constraints; verify `CouplingViolation` rejection.
4. **4B-D: Atomic Admission Serialization**: Issue concurrent competing lease requests under SMP load; verify zero double-allocations or intermediate state leaks.
5. **4B-E: Monotonic Time Observation**: Verify `read_monotonic_time` lockless snapshot loop; advance kernel ticks past lease deadline; verify fail-closed expiration.
6. **4B-F: Deadline Arithmetic Non-Wrap**: Request lease with `duration = u64::MAX`; verify `DeadlineExhaustion` rejection without wrapping to 0.
7. **4B-G: Reboot Epoch Invalidation**: Present lease with mismatched `boot_epoch`; verify immediate rejection under `I-LEASE-REBOOT-INVALIDATION`.
8. **4B-H: Capability-Mediated Admission**: Verify lease request succeeds with covering capability; fails with `PermissionDenied` when rights are attenuated.
9. **4B-I: Authority Amplification Rejection**: Verify client cannot obtain rights exceeding its presented capability.
10. **4B-J: Stale Generation Rejection**: Attempt renewal using pre-reclamation generation token; verify `GenerationMismatch`.
11. **4B-K: Consumer Crash Cleanup**: Terminate consumer process abruptly; verify `PeerClosed` signal triggers deterministic reclamation.
12. **4B-L: Hotplug Disappearance Cascade**: Simulate driver failure; verify at $t_{\text{observed}}$ resource transitions to `Unavailable`, $C_{\text{unavail}} \leftarrow C_{\text{phys}}$, and leases transition to `ProviderLost`.
13. **4B-M: Short-Lived Crash Reconciliation**: Restart `resourced` with active short-lived lease (50 ticks remaining); verify consumer can re-attest before tick 50, but cannot re-attest at tick 51.
14. **4B-N: Local Quota Enforcement**: Request capacity exceeding quota limit; verify `QuotaExceeded` rejection while node has free capacity.
15. **4B-O: Energy Classification Integrity**: Submit declared vs. measured telemetry; verify hard safety thresholds ignore declared telemetry.
16. **4B-P: Topological Cycle Traversal**: Construct cyclic Resource Graph; verify cycle handling and depth bounding.
17. **4B-Q: PMM Neutrality & Non-Reuse**: Verify zero physical memory frame leakage and verify sequence allocator resumes above ZeroFS-persisted ceiling.

---

## 17. Implementation Boundary Notice

**Implementation is strictly prohibited during this discovery phase.**  
No code, table definitions, or test runners shall be added until this Revision 4 architecture specification is formally reviewed and approved.
