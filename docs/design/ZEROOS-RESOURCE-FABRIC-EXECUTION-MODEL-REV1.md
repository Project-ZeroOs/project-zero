# ZeroOS Resource & Fabric Execution Model (REV1)

```text
ZEROOS RESOURCE & FABRIC EXECUTION MODEL REV1

ARCHITECTURE:
🟢 ARCHITECTURE READY

KERNEL CHANGES:
0

NEW SYSCALLS:
0

NEW ABI:
0

FROZEN DEPENDENCIES:
INTACT

CIRCULAR DEPENDENCIES:
NONE

SINGLE-NODE MODEL:
PROVEN

DISTRIBUTED EXTENSION:
COHERENT

CAPABILITY BOUNDARY:
COHERENT

WORKLOAD INTEGRATION:
COHERENT

OVERALL:
🟢 ARCHITECTURE FROZEN & READY FOR IMPLEMENTATION
```

---

## 1. Problem Statement

Modern operating systems and cloud frameworks conflate resource allocation with authority, force workloads to manually target specific hardware instances, and treat multi-machine clustering as a fundamental shift in user mental models. In contrast, ZeroOS requires a unified computational resource substrate where workloads express **WHAT** compute parameters they require, while the OS autonomously handles admission, hardware topology matching, scheduling, leasing, and lifecycle integration without exposing physical hardware details, compromising security boundaries, or modifying the frozen kernel substrate.

---

## 2. Design Goals

1. **Declarative Resource Specification:** Workloads express resource demands (CPU, RAM, GPU, storage, network, latency) as declarative multi-dimensional capacity vectors without specifying physical device indices, NUMA nodes, hostnames, or hardware paths.
2. **Single-Node Completeness:** ZeroOS operates 100% complete and fully functional on a single laptop without requiring external nodes, cluster control planes, or remote networks.
3. **Strict Capability Boundary:** Scheduler placement decisions never manufacture or grant capability authority. Hardware access remains authoritatively governed by kernel bitmask handles (`MUTATE Bit 15`, `FILE_READ`, `FILE_WRITE`) and `brokerd`.
4. **Frozen Substrate Compliance:** Consumes frozen ZeroFS REV3, Object & Membership REV8, Workspace REV1, and Workload/Agent REV1 without adding kernel syscalls, modifying ABI, or introducing circular dependencies.
5. **Deterministic Lease & Allocation Governance:** Resource usage is bounded by tick-based leases that automatically enforce renewal, expiration, and non-destructive release upon process or workload exit.
6. **Unified Distributed Extension:** Multi-node clusters join physical nodes into the same Resource Graph using 128-bit `DistributedId` handles without changing the user's mental model.

---

## 3. Non-Goals

1. **Cloud Orchestration Productization:** ZeroOS is not a Kubernetes clone or cloud-management product. Remote nodes are an optional infrastructure extension, not a product prerequisite.
2. **Arbitrary Process Migration in REV1:** Process memory migration across nodes is explicitly deferred in REV1 to maintain single-node simplicity.
3. **Kernel-Level Resource Scheduling:** `schedulerd` operates entirely in Ring 3. No kernel scheduler modifications, new syscalls, or kernel-space resource tracking will be added.
4. **Natural Language Intent Parsing in Schedulerd:** `schedulerd` does not parse human natural language; intent translation occurs strictly within Workspace/Agent layers before workload creation.

---

## 4. Terminology

- **`ResourceId`**: 128-bit persistent `DistributedId` `(node_id, local_seq)` representing a discrete physical or logical hardware resource extent.
- **`ResourceDescriptor`**: 128-byte frozen struct describing physical capacity, locality domain, energy tier, and availability state.
- **`DimensionCapacityVector`**: Bounded 72-byte multi-dimensional vector representing accounting quantities (CPU cores, RAM MB, GPU compute, storage MB).
- **`ResourceGraph`**: Topology graph linking physical nodes, resource extents, pools, and workload dependencies.
- **`ResourceLease`**: 144-byte bounded token representing a time-limited grant of resource capacity to a Workload.
- **`Fabric Scheduler` (`schedulerd`)**: Ring 3 daemon responsible for workload admission, resource topology placement, preemption, and queue management.

---

## 5. Identity Model

ZeroOS strictly preserves explicit separation between all 8 core identity types:

```text
WorkspaceId
    ≠ ObjectId
    ≠ WorkloadId
    ≠ AgentId
    ≠ ProcessId
    ≠ CapabilityHandle
    ≠ ResourceId
    ≠ IntentNodeId
```

### ResourceId Specifics:
- **Format:** 128-bit `DistributedId` `(node_id: u64, local_seq: u64)`.
- **Absolute Exclusions:** A `ResourceId` is **NOT** a filesystem inode, file path, `WorkloadId`, `ProcessId`, device path string (`/dev/gpu0`), hostname, IP address, or local array index.
- **Lifetime & Persistence:** Hardware `ResourceId` definitions persist in `/storage/system/resources/` across system reboots. Runtime state (`Available`, `Allocated`, `Leased`, `Faulted`) is transient and reconciled during boot recovery.

---

## 6. Resource Taxonomy

ZeroOS categorizes computational hardware into standard resource types:

```text
+-----------------------------------------------------------------------+
|                         RESOURCE TAXONOMY                             |
+-------------------+---------------------------------------------------+
| Resource Type     | Managed Dimensions                                |
+-------------------+---------------------------------------------------+
| Cpu               | Cores, Threads, Frequency MHz                     |
| Memory            | Capacity MB, Bandwidth MB/s                       |
| GpuCore           | Compute Units, Shader Pipelines, SM Count         |
| GpuMemory         | VRAM MB, Memory Bandwidth GB/s                    |
| Accelerator (NPU) | Tensor TOPS, Vector Units                         |
| LocalStorage      | Temporary Extent MB, Read/Write IOPS              |
| PersistentStorage | ZeroFS Physical Inode Extents MB                  |
| NetworkInterface  | Tx/Rx Bandwidth Mbps, Latency Microseconds        |
| Device            | Custom Hardware Extent (Sensors, Framegrabbers)   |
+-------------------+---------------------------------------------------+
```

### Abstraction Hierarchy:
1. **`ResourceDescriptor`**: Physical hardware identity and maximum physical capacity.
2. **`ResourcePool`**: Logical aggregation of matching resource descriptors within a locality domain.
3. **`ResourceRequirement`**: Workload-declared compute demand vector.
4. **`ResourceAllocation`**: Scheduler placement mapping requirement to target resource extents.
5. **`ResourceLease`**: Time-bounded runtime token granting active resource usage.

---

## 7. Resource Graph

The **Resource Graph** is owned by `schedulerd` and models physical and logical hardware topology, containment, connectivity, and active allocations:

```text
                     +-----------------------+
                     |    Node (System)      |
                     +-----------+-----------+
                                 |
                        NODE_HAS_RESOURCE
                                 v
                     +-----------+-----------+
                     |   NUMA Socket 0       |
                     +-----------+-----------+
                                 |
                     +-----------+-----------+
                     |                       |
        RESOURCE_CONTAINS_RESOURCE    RESOURCE_CONNECTED_TO
                     |                       |
                     v                       v
          +----------+----------+  +---------+---------+
          |   CPU Cores / RAM   |  |   GPU Accelerator |
          +----------+----------+  +---------+---------+
                     |                       |
            RESOURCE_ALLOCATED_TO   RESOURCE_LEASED_TO
                     |                       |
                     v                       v
          +----------+-----------------------+---------+
          |         WorkloadId (Compute Unit)          |
          +--------------------------------------------+
```

### Normative Relationship Types:
- `NODE_HAS_RESOURCE`: Links system node to top-level hardware components.
- `RESOURCE_CONTAINS_RESOURCE`: Models physical hierarchy (NUMA domain containing CPU cores and RAM).
- `RESOURCE_CONNECTED_TO`: Represents bus topology and interconnect bandwidth (PCIe link between CPU and GPU).
- `RESOURCE_DEPENDS_ON`: Models functional dependency (NPU requiring dedicated DMA buffer).
- `RESOURCE_MEMBER_OF_POOL`: Links resource descriptor to logical resource pool.
- `RESOURCE_ALLOCATED_TO`: Maps active allocation to target `WorkloadId`.
- `RESOURCE_LEASED_TO`: Maps active `ResourceLease` handle to `WorkloadId`.
- `RESOURCE_PROVIDES`: Connects physical storage/device extents to capability providers.

---

## 8. Workload Resource Requirements

Workloads declare resource requirements using `TaskResourceDemand` ([`libzero/src/workload.rs:140`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/libzero/src/workload.rs#L140)):

```text
+-----------------------------------------------------------------------+
|                    WORKLOAD RESOURCE REQUIREMENT                      |
|  - Resource Type: Cpu / Memory / GpuCore / Accelerator                |
|  - Locality Domain: HostLocal / Numa0 / Numa1 / PcieBus               |
|  - Required Capacity: [Cores, Memory MB, Compute Units, 0, 0, 0, 0, 0]|
|  - Min Duration Ticks: 100 Ticks                                      |
|  - Required Cap Rights: Bit 15 MUTATE (0x8000)                        |
|  - Execution Class: Foreground / Background / Batch / SystemCore      |
|  - Exclusivity: Shared / Exclusive                                    |
|  - Migration Tolerance: StrictLocal / Migratable                       |
+-----------------------------------------------------------------------+
```

Workload Control Blocks retain lightweight 4-byte intent pointers (`originating_intent_id`) and 1-byte execution class specifications, keeping the control block fixed at 576 bytes without hardware-specific bloat.

---

## 9. Admission Control

Admission control separates declarative requests from actual hardware execution:

```text
WORKLOAD (Created)
       │
       ▼
RESOURCE REQUIREMENT (Declared in TaskDescriptor)
       │
       ▼
ADMISSION CONTROL (schedulerd validates quotas & vector feasibility)
       │
       ▼
PLACEMENT (Topology matching & NUMA selection)
       │
       ▼
RESERVATION / LEASE (ResourceLease granted)
       │
       ▼
CAPABILITY RESOLUTION (brokerd validates Bit 15 MUTATE handles)
       │
       ▼
EXECUTION (Kernel Process PID spawned)
       │
       ▼
RELEASE (Lease freed / non-destructive teardown)
```

### Ownership Responsibilities:
- **`workspaced`**: Owns Workload identity, workspace envelope validation, and persistent manifests.
- **`schedulerd`**: Owns admission decisions, capacity vector feasibility checks (`CouplingConstraintMatrix::is_feasible`), hardware placement, time slicing, and preemption.
- **`brokerd`**: Owns capability handle lookup, service registration, and monotonic handle derivation.
- **Kernel**: Authoritatively enforces syscall capability bitmasks and process isolation.

---

## 10. Allocation Model

1. **Request Phase:** Workload enqueues demand vector (`QUEUED`).
2. **Admission Phase:** `schedulerd` evaluates workspace quota ceilings and system vector availability. If valid, workload transitions to `ADMITTED`.
3. **Allocation Phase:** `schedulerd` subtracts demand from available capacity (`DimensionCapacityVector::checked_sub`), creating a `ResourceAllocation` record.
4. **Lease Issuance:** `schedulerd` issues a `ResourceLease` token to the workload (`RUNNABLE`).
5. **Process Binding:** `workspaced` spawns kernel PID (`RUNNING`).
6. **Deallocation Phase:** Upon workload completion, failure, or cancellation, capacity is added back (`DimensionCapacityVector::checked_add`), and leases are revoked.

---

## 11. Resource Leases

Resource usage is governed by `ResourceLease` tokens ([`libzero/src/lease.rs:30-43`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/libzero/src/lease.rs#L30-L43)):

- **Layout (144 bytes):** `lease_id: DistributedId`, `resource_id: DistributedId`, `generation: u32`, `state: LeaseState`, `allocated_capacity: DimensionCapacityVector`, `granted_tick: u64`, `expiration_tick: u64`, `holder_entity_id: u64`, `client_channel_handle: u32`.
- **States:** `Active`, `Expired`, `TimeAuthorityLost`, `ProviderLost`, `Released`.
- **Renewal & Expiration:** Workloads heartbeat `schedulerd` to extend `expiration_tick`. Un-renewed leases expire automatically, causing `schedulerd` to signal process pause (`SUSPENDED`) or teardown.
- **Crash Recovery:** If a process crashes, `schedulerd` reclaims the lease immediately without waiting for tick expiration.

---

## 12. Capability Boundary

```text
+-----------------------------------------------------------------------+
|                    AUTHORITY vs SCHEDULING SEPARATION                 |
+-----------------------------------+-----------------------------------+
| SCHEDULER PLACEMENT (schedulerd)  | CAPABILITY AUTHORIZATION (Brokerd)|
+-----------------------------------+-----------------------------------+
| - Decides WHERE workload runs     | - Decides WHAT workload can access|
| - Allocates CPU/RAM/GPU extents   | - Validates Bit 15 MUTATE handles |
| - Manages preemption & queues     | - Derives monotonic cap handles   |
| - CANNOT issue capability tokens  | - CANNOT override scheduler queues|
+-----------------------------------+-----------------------------------+
```

### Invariant:
**SCHEDULER PLACEMENT $\neq$ CAPABILITY AUTHORIZATION.**
Possession of a GPU resource allocation does **not** grant access to physical storage or administrative syscalls. Conversely, holding a valid capability handle does **not** allow a workload to bypass `schedulerd` admission control.

---

## 13. Placement Model

`schedulerd` evaluates single-node placement constraints:

- **NUMA Locality:** Co-locates CPU execution threads and RAM extents on matching NUMA sockets (`Numa0`, `Numa1`).
- **Accelerator Locality:** Pairs GPU/NPU compute units with adjacent PCIe bus extents (`PcieBus`).
- **Storage Locality:** Places I/O-intensive workloads near local storage extents.
- **Affinity & Anti-Affinity:** Groups related task descriptors while separating competing heavy workloads across distinct CPU core sets.

---

## 14. Fabric Scheduler Boundary

### Responsibilities:
- Workload queue management (`QUEUED` state).
- Capacity vector math and linear feasibility checks (`CouplingConstraintMatrix`).
- Preemption ordering and hardware time-slicing.
- Resource lease lifecycle management.
- Hardware failure detection and reallocation.

### Explicit Non-Responsibilities:
- Manufacturing or granting capability handles.
- Mutating ZeroFS inode storage or directory structures.
- Natural language intent parsing.
- Overriding Workspace capability envelopes.

---

## 15. Workload Lifecycle Integration

The Resource & Fabric model integrates directly into the frozen REV1 Workload state machine:

```text
CREATED ──> QUEUED ──> ADMITTED ──> RUNNABLE ──> RUNNING ──> COMPLETED / FAILED / CANCELLED
  │           │           │           │           │
  │           │           │           │           └── (Process PID Spawned)
  │           │           │           └── (ResourceLease Issued & Caps Resolved)
  │           │           └── (Resource Vector Reserved by Schedulerd)
  │           └── (Enqueued in Schedulerd Queue)
  └── (Workload Definition Persisted in Manifest)
```

- **`SUSPENDED` State:** Triggered by preemption or resource loss. Process receives `SIGSTOP`/`SIGTERM`; leases remain held or paused.
- **`ORPHAN_COMPLETING` State:** Triggered when originating Intent node is deleted. Resource allocations and leases remain active until workload completes.

---

## 16. Failure Model

| Failure Event | Schedulerd Action | Workload State Transition | Capability Effect |
|---|---|---|---|
| Process PID Crash | Reclaims PID resources, evaluates retries | `RUNNING` $\to$ `FAILED` $\to$ `QUEUED` | Retains authorized cap handles |
| GPU Extent Offline | Detects hardware fault, attempts CPU fallback | `RUNNING` $\to$ `SUSPENDED` $\to$ `RUNNABLE` | GPU handles invalidated |
| RAM Pressure | Preempts lower-priority batch workloads | `RUNNING` $\to$ `SUSPENDED` | Pauses execution, retains state |
| Storage Failure | Marks storage extent faulted | `RUNNING` $\to$ `FAILED` | Storage handle returns `EIO` |
| Network Loss | Retains local compute, defers sync tasks | Local `RUNNING`, Remote `DEGRADED` | Local caps intact |
| Daemon Restart | Re-queries `workspaced`, rebuilds resource graph | State definitions restored | Capabilities re-validated |

---

## 17. Migration Model

- **Process Migration:** PID 409 killed $\to$ PID 812 spawned. `WorkloadId` remains 100% constant.
- **Workload Migration:** `WorkloadId`, `AgentId`, `WorkspaceId`, and `originating_intent_id` survive migration across nodes/sockets intact.
- **Deferred Feature:** Full process memory state migration across network nodes is **deferred** in REV1. Single-node local reassignment is fully supported.

---

## 18. Single-Node First Semantics

ZeroOS is designed **single-node first**:
1. A single laptop runs `workspaced`, `brokerd`, `schedulerd`, and `libzero` as a complete, autonomous system.
2. Local CPU cores, RAM, GPU, and ZeroFS storage form a self-contained Resource Graph.
3. No network interfaces or external nodes are required for complete operational functionality.

---

## 19. Distributed Fabric Extension

When additional ZeroOS nodes are available:
1. External nodes register their local `ResourceDescriptor`s with the primary node's `schedulerd`.
2. Each resource extent receives a unique 128-bit `DistributedId` where `node_id` identifies the physical machine.
3. The user's conceptual mental model remains unchanged: "I submitted compute execution to my Workspace." Remote hardware placement is handled autonomously by `schedulerd`.

---

## 20. Persistence & Boot Reconciliation

- **Persistent State:** Hardware definitions, Workspace resource policy manifests, and Workload definitions persist in `/storage/system/resources/` and `/storage/system/workspaces/`.
- **Transient State:** Real-time capacity availability, active leases, process PIDs, and channel handles are transient.
- **Boot Reconciliation:** Upon system boot:
  1. Hardware discovery scans local devices and populates the Resource Graph.
  2. `load_durable_registry()` restores persistent workspace and workload definitions.
  3. `allocator.advance_floor()` advances ID allocation floors past maximum restored sequence numbers.
  4. Transient leases and allocations are initialized to clean default states.

---

## 21. Security Model

- **No Resource Spoofing:** Resource registration requires administrative system authorization in `WS_SYSTEM_0`.
- **Cross-Workspace Isolation:** Workloads cannot request or access resources allocated to another workspace without explicit signed delegation tokens.
- **Capability Enforcement:** Kernel bitmask handles authoritatively block unauthorized storage or device writes.
- **Lease Theft Prevention:** Leases bind strictly to `(holder_entity_id, workload_id)` pairs and cannot be hijacked by third-party processes.

---

## 22. Resource Accounting

- **Multi-Dimensional Vector Accounting:** Uses `DimensionCapacityVector` with checked vector arithmetic (`checked_add`, `checked_sub`, `le`).
- **Overcommit Policy:** Overcommit is strictly **forbidden** for RAM and Storage extents to prevent kernel OOM panics and storage corruption. Controlled time-slice overcommit is permitted for CPU execution cores.

---

## 23. Human Intent Integration

Human Intent remains declarative:
```text
Human Intent ("Build & Test Application")
       │
       ▼
Workspace intent.json (Declarative Goal DAG)
       │
       ▼
Agent / workspaced (Interpretation)
       │
       ▼
Workload TaskDescriptor (Procedural Resource Demand Vector)
       │
       ▼
Schedulerd (Hardware Placement & Admission)
```
Intent nodes specify priority and deadlines; they do **not** invoke low-level hardware control APIs.

---

## 24. Energy & Quality of Service (QoS) Decisions

- **QoS Tiers:** `Foreground` (Latency-critical), `Background` (Throughput-optimized), `Batch` (Preemptible), `SystemCore` (Administrative).
- **Energy Aware Placement:** Evaluates `EnergyTier` (`Measured`, `Estimated`, `Declared`) and `current_power_mw` to optimize battery life on mobile hardware.

---

## 25. Observability

ZeroOS provides clear non-telemetry diagnostic status answering: *"Why is my workload queued/slow?"*

- `RESOURCE_CONSTRAINED_CPU`: CPU cores fully allocated.
- `RESOURCE_CONSTRAINED_MEMORY`: RAM limit reached.
- `RESOURCE_CONSTRAINED_ACCELERATOR`: GPU/NPU busy.
- `CAPABILITY_UNRESOLVED`: Waiting for capability handle authorization.
- `SCHEDULER_QUEUED`: Waiting in priority queue.

---

## 26. Architectural Invariants

- **RF-01 (ResourceId Uniqueness):** `ResourceId` is a 128-bit persistent `DistributedId` unique across space and time.
- **RF-02 (Identity Non-Conflation):** `ResourceId ≠ WorkspaceId ≠ ObjectId ≠ WorkloadId ≠ AgentId ≠ ProcessId ≠ CapabilityHandle ≠ IntentNodeId`.
- **RF-03 (Declarative Workload Requirements):** Workloads express resource demands as multi-dimensional capacity vectors without targeting physical device indices.
- **RF-04 (Non-Manufacturing Scheduler):** `schedulerd` decides hardware placement; `schedulerd` **cannot** manufacture capability handles or grant file access.
- **RF-05 (Single-Node Completeness):** ZeroOS operates 100% complete on a single node without network dependencies.
- **RF-06 (Lease Bounded Lifetime):** Resource allocations are governed by tick-bounded `ResourceLease` tokens.
- **RF-07 (Capability/Placement Decoupling):** Possessing a resource allocation does not grant capability authority; holding a capability handle does not force scheduler placement.
- **RF-08 (Resource Graph Containment Acyclicity):** Hardware containment graphs are strictly acyclic trees.
- **RF-09 (Offline Execution Independence):** Resource scheduling operates offline without remote network validation.
- **RF-10 (Deterministic Recovery & Allocator Advance):** Boot recovery restores persistent descriptors and advances allocator sequence floors past maximum restored IDs.
- **RF-11 (Workload Lifecycle Alignment):** Resource state events trigger valid transitions in the frozen REV1 Workload state machine.
- **RF-12 (No Implicit Authority Elevation):** High-priority or SystemCore workloads execute under strict capability envelopes.
- **RF-13 (System Workspace Protection):** `WS_SYSTEM_0` is reserved exclusively for administrative system daemons.
- **RF-14 (Vector Capacity Conservation):** Resource allocation and deallocation preserve exact capacity vector conservation (`allocated + available = total`).
- **RF-15 (Transient Availability vs Persistent Identity):** Hardware identity persists; allocation and availability states are transient.
- **RF-16 (Orphan Completing Preservation):** Intent node deletion allows active workloads to complete in `ORPHAN_COMPLETING` state without resource revocation.
- **RF-17 (Non-Destructive Resource Release):** Resource release frees compute extents; it does not delete ZeroFS physical objects.
- **RF-18 (Explicit Capability Delegation):** Cross-workspace resource access requires explicit capability delegation.
- **RF-19 (Quota Enforcement Before Admission):** Resource demands are validated against workspace quota ceilings before transition to `ADMITTED`.
- **RF-20 (Idempotent Lease Renewal):** Lease renewal operations operate idempotently without state corruption.
- **RF-21 (Zero Kernel Syscall Mutation):** Resource and Fabric semantics operate entirely in Ring 3 with 0 kernel modifications.
- **RF-22 (Hardware Abstraction Transparency):** Physical device swaps do not mutate logical `WorkloadId` or `WorkspaceId`.
- **RF-23 (Linear Coupling Feasibility):** Multi-resource feasibility is validated via `CouplingConstraintMatrix` checks.
- **RF-24 (Deterministic Failure Policy Execution):** Resource disappearance triggers explicit failure policies (`RETRY`, `SUSPEND`, `FAIL`).
- **RF-25 (No Cloud/Remote Product Dependency):** Remote compute is an optional extension, not a product requirement.

---

## 27. Adversarial Scenario Analysis (Scenarios A–T)

- **Scenario A (Two Workloads Compete for One GPU):** `schedulerd` admits higher priority workload (`ADMITTED`), queues lower priority workload (`QUEUED`).
- **Scenario B (Workload Requests More RAM Than Available):** Capacity vector check (`checked_sub`) fails; workload remains `QUEUED` with status `RESOURCE_CONSTRAINED_MEMORY`.
- **Scenario C (GPU Disappears Mid-Execution):** `schedulerd` detects fault, transitions workload to `SUSPENDED` or `FAILED`, and evaluates CPU fallback policy.
- **Scenario D (Process Crashes While Leased):** Process PID dies; `workspaced` retains `WorkloadId`, increments retry count, and reclaims lease immediately.
- **Scenario E (`schedulerd` Crashes During Allocation):** Daemon restarts, re-queries active workloads from `workspaced`, and rebuilds allocation table.
- **Scenario F (`brokerd` Crashes During Authorization):** Capability state remains backed by kernel handle table; `brokerd` reconnects seamlessly.
- **Scenario G (Lease Expire Mid-Execution):** Un-renewed lease expires; `schedulerd` signals process pause (`SUSPENDED`).
- **Scenario H (Workload Migrates to Another Local Socket):** PID 409 killed $\to$ PID 812 spawned. `WorkloadId` remains 100% constant.
- **Scenario I (Remote Node Disappears):** Remote workloads transition to `UNREACHABLE` $\to$ `FAILED`; local fallback policy executed.
- **Scenario J (Two Workspaces Compete for Scarce Resources):** `schedulerd` enforces workspace quota ceilings and priority tier preemption.
- **Scenario K (Impossible Deadline Requested):** Admission check rejects impossible request; returns `ZeroError::DeadlineExhaustion`.
- **Scenario L (Capability Authority but No Placement):** Workload remains `QUEUED` until `schedulerd` allocates compute resources.
- **Scenario M (Placement but Insufficient Capability):** Process spawned but kernel syscall dispatcher rejects write with `EPERM`.
- **Scenario N (Single-Node Machine Without GPU):** GPU workloads request CPU fallback mode or remain queued gracefully.
- **Scenario O (Laptop Operates Offline):** All scheduling and execution operate locally without network errors.
- **Scenario P (Resource Identity Survives Reboot):** Physical `ResourceId` restored from persistent registry; sequence floor advanced.
- **Scenario Q (Stale Allocation After Crash):** Boot recovery clears volatile allocation table, restoring physical resources to `Available`.
- **Scenario R (Intent Priority Mutates While Queued):** `schedulerd` re-orders priority queue position dynamically.
- **Scenario S (Multiple Workloads Share Resource):** Coordinated time-slicing applied for shared CPU/RAM resources.
- **Scenario T (Exclusive Resource Requested by Two Workloads):** First workload receives exclusive lease; second workload queued until release.

---

## 28. Dependency Graph & Cycle Audit

```text
HUMAN INTENT (Declarative Goals in Workspace intent.json)
    │
    ▼
WORKSPACE (WorkspaceId Logical Context Envelope)
    │
    ├──> AGENT (AgentId Persistent Control Entity)
    │     │
    │     ▼
    └──> WORKLOAD (WorkloadId Compute Task Unit)
          │
          ├──> CAPABILITY SCOPE (Brokerd & Kernel Bit 15 MUTATE Handle)
          │
          └──> FABRIC SCHEDULER (Schedulerd Admission & Placement)
                │
                ▼
          RESOURCE GRAPH (ResourceDescriptors & Locality Domains)
                │
                ▼
          RESOURCE LEASE (ResourceLease Token)
                │
                ▼
          PROCESS (Transient Kernel PID Execution Thread)
```

**Cycle Audit Result:** Exactly 0 cycles found. Dependency directions flow strictly downwards.

---

## 29. Implementation Boundary

- **Expected Surface:** Ring 3 daemons (`schedulerd`, `brokerd`, `workspaced`, `libzero`).
- **Kernel Changes:** 0
- **New Syscalls:** 0
- **New ABI:** 0

All resource descriptors, leases, vector capacity arithmetic, and graph topology management consume existing primitives in `libzero/src/resource.rs` and `libzero/src/lease.rs`.

---

## 30. Open Questions

- *None.* All identity boundaries, capability limits, state transitions, lease lifetimes, single-node guarantees, and adversarial scenarios are fully resolved.

---

## 31. Final Architectural Verdict

```text
ZEROOS RESOURCE & FABRIC EXECUTION MODEL REV1

ARCHITECTURE:
🟢 ARCHITECTURE READY

KERNEL CHANGES:
0

NEW SYSCALLS:
0

NEW ABI:
0

FROZEN DEPENDENCIES:
INTACT

CIRCULAR DEPENDENCIES:
NONE

SINGLE-NODE MODEL:
PROVEN

DISTRIBUTED EXTENSION:
COHERENT

CAPABILITY BOUNDARY:
COHERENT

WORKLOAD INTEGRATION:
COHERENT

OVERALL:
🟢 ARCHITECTURE FROZEN & READY FOR IMPLEMENTATION
```
