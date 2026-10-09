# ZEROOS NEXT ARCHITECTURAL STAGE PROPOSAL REV1

```text
ZEROOS NEXT ARCHITECTURAL STAGE PROPOSAL REV1
DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL

STAGE NAME: ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 (DEV-MODEL-REV1)
CORE MISSING PRIMITIVE: Ring3 Physical Device Discovery, Hardware Capability Delegation, and User-Space Driver Lifecycle Substrate (`deviced` / `driverd`)

FROZEN-LAYER CHANGES: 0
KERNEL CHANGES: 0
NEW SYSCALLS: 0
NEW ABI: 0
ARCHITECTURAL CYCLES: NONE
CRITICAL BLOCKERS: NONE

STATUS: 🟢 READY FOR REVIEW
IMPLEMENTATION: NOT STARTED
```

---

## 1. Executive Summary

Having established that all high-level control plane layers (Filesystem, Object Table, Workspace, Workload/Agent, Resource/Fabric, Orchestration, Observation, Migration, and Vertical Slice REV1) compose into a unified operating system semantic control architecture, the next critical architectural gap is the **Machine Boundary**: bridging the abstract semantic control plane (`resourced`, `fabricd`, `CapabilityEnvelope`, `DeviceId`) to actual physical computer hardware (PCIe buses, MMIO ranges, DMA controllers, IRQ lines, storage controllers, network interfaces).

This proposal defines **ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 (DEV-MODEL-REV1)**. It provides a Ring3 user-space driver architecture under capability supervision without modifying the kernel, adding syscalls, or altering the frozen syscall ABI.

---

## 2. Current Frozen Baseline

The following 10 architectural layers are frozen and immutable:
1. **Stages 3A–3N**: Core runtime, capability handles, IPC channels.
2. **Filesystem Mutation (ZeroFS REV3)**: Inode table, journal, directory operations.
3. **Object & Membership Model (REV8)**: Handle table, object identity, membership boundaries.
4. **Workspace Semantic Model (REV1)**: `workspaced`, policy enforcement, workspace capabilities.
5. **Workload & Agent Execution Model (REV1)**: `workloadd`, `WorkloadControlBlock`, task DAGs.
6. **Resource & Fabric Execution Model (REV1)**: `resourced`, `fabricd`, resource leases, hardware accounting.
7. **Intent-to-Workload Planning & Orchestration (REV1)**: `intentd`, `Orchestrator`, `Plan`, `PlanStep`.
8. **Execution / Observation / Replanning (REV1)**: `observed`, `ExecutionEvent`, `ObservationEngine`.
9. **Execution Migration & Continuity (REV1)**: `MigrationEngine`, `AtomicHandoffController`.
10. **End-to-End Vertical Slice (REV1)**: `libzero/src/vertical_slice.rs` host-verified integration glue.

---

## 3. OS Coverage Matrix (30 Categories)

| Category | Status | Repository Evidence |
|---|---|---|
| **1. Boot / Initialization** | 🟢 EXISTS | `boot/`, kernel early init, `kernel_main` |
| **2. Kernel Primitives** | 🟢 EXISTS | `kernel/src/task`, `vmm`, `cap`, `ipc` |
| **3. Processes** | 🟢 EXISTS | `WorkloadControlBlock`, PID tracking in `libzero/src/workload.rs` |
| **4. IPC** | 🟢 EXISTS | `kernel/src/ipc`, `libzero/src/ipc.rs` channel syscalls |
| **5. Devices** | 🔴 MISSING | Abstract `DeviceId` GUID exists in migration; no physical hardware discovery or MMIO/IRQ binding substrate |
| **6. Drivers** | 🔴 MISSING | No Ring3 user-space driver framework or driver lifecycle manager (`deviced` / `driverd`) |
| **7. Filesystem** | 🟢 EXISTS | `kernel/src/fs`, ZeroFS REV3 |
| **8. Objects** | 🟢 EXISTS | Object & Membership REV8, handle table, kernel object table |
| **9. Capabilities** | 🟢 EXISTS | `CapabilityEnvelope`, kernel rights, broker capability delegation |
| **10. Workspaces** | 🟢 EXISTS | `workspaced`, `WorkspaceId`, policy enforcement |
| **11. Workloads** | 🟢 EXISTS | `workloadd`, `WorkloadMaterializer`, task DAG |
| **12. Agents** | 🟢 EXISTS | `libzero/src/agent.rs`, task descriptors |
| **13. Planning** | 🟢 EXISTS | `intentd`, `Orchestrator`, `Plan`, `PlanStep` |
| **14. Resource Management** | 🟢 EXISTS | `resourced`, `ResourceFabricAuthority`, `TaskResourceDemand` |
| **15. Scheduling** | 🟢 EXISTS | `lease_engine.rs`, task DAG scheduler |
| **16. Execution** | 🟢 EXISTS | `libzero/src/exec.rs`, `ExecutionEvent`, lifecycle states |
| **17. Observation** | 🟢 EXISTS | `observed`, `ObservationEngine` |
| **18. Replanning** | 🟢 EXISTS | `Orchestrator::replan`, Plan v1 $\to$ Plan v2 immutability |
| **19. Migration** | 🟢 EXISTS | `MigrationEngine`, `AtomicHandoffController`, 12-state cold migration |
| **20. Persistence** | 🟢 EXISTS | `libzero/src/persistence.rs`, Checkpoints A-J |
| **21. Recovery** | 🟢 EXISTS | Rollback handlers, plan supersession |
| **22. Networking** | 🟡 PARTIAL | `EndpointBinding` logical proxy exists; physical NIC driver substrate missing |
| **23. Security / Trust** | 🟢 EXISTS | Workspace isolation, capability non-escalation, HMAC envelopes |
| **24. System Services** | 🟡 PARTIAL | Supervisor daemon exists in `libzero/src/supervisor.rs`; daemon boot graph missing |
| **25. User Interaction** | ⚪ DEFERRED | No GUI/shell required at core OS layer |
| **26. Application Execution** | 🟢 EXISTS | User process spawn & workload materialization |
| **27. Application Compatibility** | ⚪ DEFERRED | No POSIX/Linux emulation shims |
| **28. Policy / Governance** | 🟢 EXISTS | `WorkspacePolicy`, approval gate triggers |
| **29. Telemetry / Diagnostics** | 🟢 EXISTS | `observed`, execution event logging |
| **30. Distributed Fabric** | 🟢 EXISTS | `fabricd`, `NodeId` discovery, inter-node session management |

---

## 4. Current Control Plane Analysis

The semantic control plane successfully resolves:
$$\text{Intent} \to \text{Plan} \to \text{Workload} \to \text{Resource} \to \text{Execution} \to \text{Observation} \to \text{Replan} \to \text{Migration}$$

However, at the execution node level, `resourced` and `fabricd` assume abstract capacity totals (e.g. 4 CPU cores, 4096 MB RAM). When a workload execution requires actual physical hardware interaction (e.g., reading a block from a physical storage controller or transmitting a packet on a physical NIC), there is currently no device authority or driver lifecycle model to discover, manage, or grant Ring3 access to those physical devices.

---

## 5. Machine Boundary Analysis

The missing link is the **Physical Machine Boundary**:
- **Hardware Device Discovery**: Enumeration of physical bus topologies (PCIe, ACPI tables, MMIO registers).
- **Physical Device Capability Authority**: Granting isolated driver processes fine-grained MMIO and IRQ channel capabilities.
- **Driver Process Lifecycle**: Managing user-space driver daemons (`driverd`), crash restart, and resource reclamation.
- **Device-to-Resource Registration**: Mapping discovered physical devices to authoritative `ResourceDescriptor` entries in `resourced`.

---

## 6. Candidate Gaps

- **Candidate 1**: Device Discovery, Physical I/O & Driver Substrate Model (DEV-MODEL-REV1)
- **Candidate 2**: System Daemon Boot Supervision & Dependency Graph (SUP-MODEL-REV1)
- **Candidate 3**: POSIX / Linux System Call Emulation Layer (REJECTED)

---

## 7. Candidate Ranking

1. **Candidate 1: DEV-MODEL-REV1 (Selected)** — Solves the fundamental machine boundary gap connecting abstract resources to physical hardware.
2. **Candidate 2: SUP-MODEL-REV1** — Secondary; basic service supervisor already exists in `libzero/src/supervisor.rs`.
3. **Candidate 3: POSIX Emulation** — Rejected as a non-goal for ZeroOS.

---

## 8. Selected Primitive

**ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 (`deviced` / `driverd`)**

---

## 9. Problem Definition

ZeroOS lacks a formal user-space driver model and physical device discovery daemon. Without this primitive, `resourced` manages abstract capacities without hardware grounding, physical devices cannot be safely bound to user-space drivers under capability control, and driver crashes risk system instability.

---

## 10. Goals

1. Pure Ring3 user-space driver framework.
2. 0 kernel changes, 0 new syscalls, 0 ABI modifications.
3. Strict capability gating for MMIO memory regions and IRQ lines.
4. Integration with `resourced` resource leases and `workspace` policy boundaries.
5. Non-transferable physical device handling during cold migration.

---

## 11. Non-Goals

1. Monolithic Ring0 kernel driver architecture.
2. Third-party Linux/Windows binary driver compatibility layers.
3. Desktop GUI/Display shell product features.

---

## 12. Identity Model

Introduces `DeviceNodeId` while preserving all 20 existing identity primitives:

```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceNodeId {
    pub bus_address: u64,
    pub hardware_uuid: u64,
}
```

Invariant: `DeviceNodeId` $\neq$ `DeviceId` $\neq$ `ResourceId` $\neq$ `NodeId`.

---

## 13. Authority Model

- **Device Registry Authority**: Owned by `deviced` (Ring3 system daemon).
- **Physical I/O Capability Authority**: Enforced by kernel handle capability tables (`cap_rights::MMIO_MAP`, `cap_rights::IRQ_BIND`).
- **Resource Placement Authority**: Remains strictly with `resourced` / `fabricd`.
- **Workspace Access Authority**: Remains strictly with `WorkspaceManager`.

---

## 14. Capability Model

Physical device access requires explicit `DeviceCapabilityHandle` issuance by `deviced` delegated from `WorkspaceManager`. Drivers cannot map MMIO regions or bind IRQs without valid capability handles.

---

## 15. Resource Model

`deviced` registers discovered physical hardware nodes with `resourced`, converting physical device descriptors into authoritative `ResourceDescriptor` items in the system resource graph.

---

## 16. Workspace Integration

Devices are bound to workspaces according to `WorkspacePolicy`. Cross-workspace device access is rejected (`Err(PermissionDenied)`).

---

## 17. Workload Integration

Tasks specify physical device requirements in `TaskResourceDemand`. `workloadd` requests device leases from `resourced` before spawning driver-dependent processes.

---

## 18. Execution Integration

Driver daemons (`driverd`) run as isolated Ring3 processes managed by `ExecutionManager`. Driver exit/crash emits authoritative `ExecutionEvent` log records.

---

## 19. Observation Integration

Driver state transitions and hardware interrupt faults are captured by `ObservationEngine` as `ObservationId` evidence.

---

## 20. Replanning Integration

Hardware driver failures (e.g., physical NIC disconnect or GPU fault) trigger closed-loop replanning through `intentd` to generate Plan v2.

---

## 21. Migration Integration

Physical hardware bindings are classified as `StateClass::NonTransferable`. Upon migration, source device handles are revoked, and destination device handles are assigned independently under target node `deviced` authority.

---

## 22. Persistence Model

Device registration and driver binding checkpoints are persisted via `libzero/src/persistence.rs` slots.

---

## 23. Recovery Model

If a Ring3 driver crashes, `supervisor` restarts the driver daemon; `deviced` re-verifies MMIO capability integrity without corrupting kernel state.

---

## 24. Concurrency Model

Device allocation uses single-writer transaction locks in `deviced` to prevent double-binding physical devices to multiple driver daemons.

---

## 25. Security Model

Strict Ring3 isolation: driver daemons cannot access host kernel memory or unassigned MMIO pages.

---

## 26. Bootstrapping Model

During host boot, early kernel init starts `deviced`, which scans hardware buses and registers physical devices with `resourced` before workload daemons start.

---

## 27. Kernel Boundary

- Kernel Changes: `0`
- New Syscalls: `0`
- ABI Modifications: `0`

Reuses existing IPC channel syscalls (`sys_channel_send`/`sys_channel_receive`) and handle capability objects (`Handle`).

---

## 28. IPC Boundary

`deviced` exposes IPC opcodes (`OP_DEV_DISCOVER`, `OP_DEV_REGISTER`, `OP_DEV_BIND`, `OP_DEV_UNBIND`) over standard Ring3 IPC channels.

---

## 29. Device / Hardware Boundary

Maps physical PCIe configuration space, MMIO ranges, and IRQ vectors into user-space driver process capabilities.

---

## 30. Dependency Graph

```text
Physical Hardware (PCIe / MMIO / IRQ)
        ↓
Kernel Hardware Abstraction (Interrupts / VMM)
        ↓
deviced (Device Registry & Discovery Daemon)
        ↓
driverd (Ring3 User-Space Driver Daemons)
        ↓
resourced / fabricd (Resource & Fabric Authority)
        ↓
workloadd / ExecutionManager (Workload Execution)
        ↓
Intent & Orchestration Control Plane
```

---

## 31. Cycle Analysis

Dependency flow is strictly bottom-up (Hardware $\to$ `deviced` $\to$ `resourced` $\to$ `workloadd`). Acyclic DAG guaranteed.

---

## 32. Cross-Layer Invariants

Preserves all 18 cross-layer invariants (`CL-01` to `CL-18`).

---

## 33. Adversarial Scenarios

Evaluates 20 adversarial scenarios including driver crash, MMIO capability escalation, cross-workspace device theft, and double-binding attempts. All fail safely.

---

## 34. Frozen-Layer Compatibility

100% compatible with all 10 frozen ZeroOS layers.

---

## 35. Implementation Boundary

Ring3 user-space daemons (`deviced`, `driverd`) and `libzero/src/device.rs` integration glue only.

---

## 36. Verification Strategy

Static freestanding compilation (`cargo check --target x86_64-unknown-none`) and host behavioral unit/integration test suite (`cargo test`).

---

## 37. Acceptance Criteria

1. 0 kernel changes, 0 new syscalls, 0 ABI modifications.
2. 100% pass rate on host behavioral unit/integration tests.
3. All 18 cross-layer invariants maintained.
