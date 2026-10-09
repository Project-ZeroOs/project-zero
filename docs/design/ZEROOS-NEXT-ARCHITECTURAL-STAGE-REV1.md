# ZEROOS NEXT ARCHITECTURAL STAGE DISCOVERY & CROSS-LAYER GAP ANALYSIS REV1

## Executive Recommendation

| Metric / Item | Recommendation / Status |
|---|---|
| **Authoritative Baseline** | Project ZeroOS Frozen Baseline (Stages 3A–3N, ZeroFS REV3, Object REV8, Workspace REV1, Workload REV1, Resource REV1, Intent REV1, Observation REV1, Migration REV1, Vertical Slice REV1, DEV-MODEL-REV1 Milestone A, Boot Logo REV1) |
| **Highest-Value Missing Primitive** | **Ring3 Physical Driver Substrate & Hardware I/O Execution (`driverd` — DEV-MODEL-REV1 Milestone B)** |
| **Architectural Purpose** | Operationalize Ring3 user-space driver supervision, `DevCap` capability handle binding, MMIO/IRQ validation, physical hardware I/O execution, and crash-restart driver fault recovery over discovered physical PCI devices |
| **Kernel Code Changes** | `0 lines` (`git diff -- kernel/` = 0) |
| **New Syscalls / ABI Changes** | `0` new syscalls, `0` ABI modifications |
| **New Capability Rights** | `0` new capability bits |
| **Final Decision Status** | **`🟢 READY FOR ARCHITECTURE SPECIFICATION`** |

---

## 1. Verified Baseline & Source References

### Frozen Subsystem Baseline

1. **Stage 3A–3N Kernel Nucleus**: Frozen. (PMM, VMM, GDT, TSS, IDT, W^X, fast syscalls, process lifecycle, capability tables).
2. **ZeroFS & Filesystem Mutation**: Frozen (REV3). (Block engine, InodeManager, DirectoryManager, Journal V2, `SYS_DIR_READ` Policy A, crash recovery).
3. **Object Identity & Membership**: Frozen (REV8). (Object UUIDs, capability handles, member authorization, persistence).
4. **Workspace Semantic Model**: Frozen (REV1). (Workspace boundaries, capability containment, isolation enforcement).
5. **Workload & Agent Execution Model**: Frozen (REV1). (Workload DAG, agent proposals, execution boundaries).
6. **Resource & Fabric Execution Model**: Frozen (REV1). (Resource graph, capacity leasing, node identity).
7. **Intent-to-Workload Planning & Orchestration**: Frozen (REV1). (Intent DAG, plan generation, DAG validation).
8. **Execution, Observation & Replanning**: Frozen (REV1). (Observation engine, anomaly detection, replanning triggers).
9. **Execution Migration & Continuity**: Frozen (REV1). (Checkpointing, atomic handoff, migration sessions).
10. **Cross-Layer Architecture Composition**: Frozen. (Seamless composition across all 9 core subsystems).
11. **End-to-End Vertical Slice REV1**: Frozen. (Vertical integration, behavioral test addendum complete).
12. **Device Discovery & Driver Substrate**: REV1 Milestone A FROZEN. (Ring3 `deviced` discovery, `SYS_DEV_QUERY` PCI enumeration, `DeviceNode` validation, idempotency, `resourced` publication, QEMU runtime tested).
13. **Boot Logo Integration**: REV1 FROZEN. (Multiboot 1 framebuffer, 160x160 RGBA static asset, alpha blending, headless non-blocking fallback, QEMU visual screendump verified).

---

## 2. Operational Coverage & Gap Matrix

| Responsibility | Architecture defined | Implemented in source | Behavioral tests executed | QEMU/runtime demonstrated | Remaining Gap |
|---|---|---|---|---|---|
| **Storage & Filesystem** | `FROZEN` (REV3) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (ZeroFS mutation & directory ops complete) |
| **Identity & Membership** | `FROZEN` (REV8) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Object identity & handles complete) |
| **Workspaces** | `FROZEN` (REV1) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Workspace semantic model complete) |
| **Workloads & Agents** | `FROZEN` (REV1) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Workload DAG & agent proposals complete) |
| **Capability Enforcement** | `FROZEN` (Stage 3H) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Monotonic ID & handles complete) |
| **Resource Accounting & Scheduling** | `FROZEN` (REV1) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Resource graph & leasing complete) |
| **Intent & Planning** | `FROZEN` (REV1) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Intent DAG to workload plan complete) |
| **Execution & Observation** | `FROZEN` (REV1) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Telemetry & anomaly observation complete) |
| **Recovery & Continuity** | `FROZEN` (REV1) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Migration & checkpoint handoff complete) |
| **Device Discovery & I/O** | `FROZEN` (Milestone A) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` (Milestone A) | Ring3 physical driver binding & I/O execution (Milestone B) |
| **Driver Supervision** | `FROZEN` (DEV Spec) | `SOURCE-VERIFIED` | `HOST-TESTED` | `UNPROVEN` | `driverd` process lifecycle supervision & MMIO BAR binding |
| **Boot & Initialization** | `FROZEN` (Stage 2 & Logo) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Boot logo & QEMU Multiboot complete) |
| **Network Connectivity** | `UNPROVEN` | `UNPROVEN` | `UNPROVEN` | `UNPROVEN` | Deferred to future fabric networking stage |
| **Human Feedback & System UI** | `FROZEN` (Stage 5/6C) | `SOURCE-VERIFIED` | `HOST-TESTED` | `QEMU-TESTED` | None (Input routing & intent boundary complete) |

---

## 3. Evaluation of Candidate Milestones

### Candidate 1: Ring3 Physical Driver Substrate & Hardware I/O Execution (`driverd` — DEV-MODEL-REV1 Milestone B)
- **Description**: Implement Ring3 driver supervisor daemon (`driverd`) to spawn driver processes, bind hardware `DevCap` handles, validate MMIO/IRQ boundaries, execute physical sector I/O against the QEMU PIIX3 ATA controller, and perform driver crash-restart recovery.
- **Architectural Necessity**: **CRITICAL**. Milestone A demonstrated physical PCI discovery (`deviced`) and resource graph publication (`resourced`). Without Ring3 driver supervision and physical I/O execution, physical hardware remains discovered but un-driven.
- **End-to-End Impact**: Bridges physical hardware with ZeroOS workloads, unblocking real physical storage and peripheral I/O without kernel drivers.
- **Security & Scope**: 0 kernel changes, 0 new syscalls, 0 ABI modifications. Operates strictly in Ring3 via `DevCap` capability handles.
- **QEMU Demonstrability**: High (QEMU PIIX3 IDE controller `0x8086:0x7010`).

### Candidate 2: Distributed Cross-Node Network Fabric (`netd` Subsystem)
- **Description**: Implement Ring3 network daemon (`netd`) for TCP/IP socket proxying and multi-node capability routing.
- **Evaluation**: Premature. Implementing a network stack before physical device drivers can drive physical NIC hardware or storage creates an architectural dependency cycle.

### Candidate 3: Multi-Window GUI & Window Decoration Framework (`compositord` REV2)
- **Description**: Extend the Stage 5 software compositor with advanced window decorations and desktop UI controls.
- **Evaluation**: Application/UI layer feature. Stage 5 and Stage 6F already established software composition and spatial grounding primitives. Does not introduce a missing OS-level primitive.

---

## 4. Selected Milestone Specification: DEV-MODEL-REV1 Milestone B

### Problem Statement

Demonstrate the operational bridge between physical hardware discovery (`deviced`), resource graph leasing (`resourced`), and Ring3 hardware driver process lifecycle (`driverd`).

Specifically:
1. `deviced` discovers hardware and registers `DeviceNode`.
2. `resourced` admits physical capacity and issues a `DeviceCapabilityHandle` (`DevCap`) for the hardware node.
3. `driverd` (Ring3 driver supervisor) spawns the Ring3 hardware driver process (e.g., `ata_driverd`) for the PIIX3 ATA controller.
4. `driverd` binds the hardware node, validates `DevCap` rights (MMIO, IRQ, DMA), and transitions `DeviceLifecycleState` from `Discovered` $\to$ `Binding` $\to$ `Ready`.
5. The Ring3 driver executes physical hardware I/O commands (read sector / write sector) against the QEMU PIIX3 ATA controller.
6. If the Ring3 driver process crashes, `driverd` detects process termination, transitions device state to `Faulted` $\to$ `Resetting` $\to$ `Ready`, and restarts the driver cleanly without kernel instability.

### Authority Boundaries & Data Flow

```text
Physical Hardware (QEMU PIIX3 ATA)
       ▲
       │ Physical Port I/O / MMIO Commands
       ▼
Ring3 Driver Process (ata_driverd)
       ▲
       │ DevCap Handle Validation / Process Lifecycle
       ▼
Ring3 Driver Supervisor (driverd)
       ▲
       │ ResourceDescriptor / Capacity Lease
       ▼
resourced (Resource Graph) <---> deviced (Device Registry)
```

- **`deviced` Authority**: Single-writer authority over `DeviceRegistry` and physical `DeviceNodeId` allocation.
- **`resourced` Authority**: Single-writer authority over `ResourceGraph` and capacity leasing (`DevCap`).
- **`driverd` Authority**: Single-writer authority over Ring3 driver process lifecycle, driver binding, and fault recovery.
- **Kernel Authority**: Enforces memory isolation and capability checks without holding driver code (0 kernel modifications).

---

## 5. Adversarial Architecture Review (15 Concrete Failure Scenarios)

1. **Unauthorized Driver Spawning Attempt**: Unprivileged Ring3 process attempts to spawn a driver daemon without `DevCap` authorization. (*Enforcement*: `driverd` rejects request with `Err(ZeroError::PermissionDenied)`).
2. **Cross-Workspace Hardware Theft**: Process in Workspace B attempts to bind a device assigned to Workspace A. (*Enforcement*: `WorkspaceManager` capability check rejects binding with `Err(ZeroError::WorkspaceViolation)`).
3. **Duplicate Driver Binding Race**: Two processes attempt to bind the same `DeviceNodeId` simultaneously. (*Enforcement*: Single-writer `driverd` rejects second binding with `Err(ZeroError::AlreadyExists)`).
4. **Ring3 Driver Daemon Crash & Teardown**: Driver process panics or crashes during active I/O. (*Enforcement*: `driverd` traps process exit, transitions state to `Faulted`, revokes handles, resets device, and restarts cleanly).
5. **Unmapped MMIO BAR Access**: Driver attempts to access MMIO address outside its assigned `mmio_base` and `mmio_length`. (*Enforcement*: Rejection / segmentation fault isolated strictly within Ring3 driver process space; kernel remains intact).
6. **Stale Capability Handle Post-Unplug**: Driver attempts hardware I/O using a `DeviceCapabilityHandle` after hot-unplug. (*Enforcement*: Incarnation sequence check fails with `Err(ZeroError::StaleHandle)`).
7. **Driver Privileges Elevation Attempt**: Driver attempts to execute kernel-mode instructions (e.g. `cli`, `lgdt`). (*Enforcement*: Hardware GP fault `#GP(0)` trapped by kernel, terminating driver process without kernel compromise).
8. **DMA Pool Allocation Exhaustion**: Driver requests DMA allocation exceeding `dma_max_bytes`. (*Enforcement*: `driverd` rejects allocation with `Err(ZeroError::QuotaExceeded)`).
9. **Stale Observation Post-Crash**: Anomaly observation engine receives telemetry from a dead driver PID. (*Enforcement*: Stale PID validation discards telemetry).
10. **Daemon Bootstrapping Out-of-Order**: `driverd` attempts to run before `deviced` and `resourced`. (*Enforcement*: Acyclic startup DAG check enforces $\text{kernel} \to \text{deviced} \to \text{resourced} \to \text{driverd}$).
11. **Host-Only Mock Impersonation**: Test runner presents dummy software struct as hardware result. (*Enforcement*: Enforces physical PCI config space reading via `SYS_DEV_QUERY` on QEMU PIIX3 controller).
12. **Kernel Layer Modification Attempt**: Developer attempts to add a new `sys_driver_bind` syscall to kernel. (*Enforcement*: Strict 0 kernel mutation invariant fails build check).
13. **Cross-Node Driver Migration Theft**: Workload migration attempts to transfer physical hardware handle across nodes. (*Enforcement*: Physical device handles classified `NonTransferable`; migration requires re-discovery on destination).
14. **IRQ Storm Denial of Service**: Hardware device generates rapid IRQ interrupts. (*Enforcement*: Interrupt handler rate-limits notifications to Ring3 driver; excess interrupts masked fail-closed).
15. **Partial Recovery Handshake Failure**: Driver crashes during state transition `Resetting` $\to$ `Ready`. (*Enforcement*: `driverd` aborts binding, marks device `Faulted`, and notifies `resourced` to trigger replanning).

---

## 6. Test & Runtime Demonstration Plan

- **Freestanding Check**: `cargo check --target x86_64-unknown-none`
- **Host Unit Tests**: `cargo test --lib --target x86_64-pc-windows-gnu`
- **QEMU Runtime Test**: Execute `python tools/run_qemu.py` and verify serial markers:
  - `DRIVERD_STARTED`
  - `DRIVER_BIND_BEGIN`
  - `DRIVER_CAPABILITY_VALIDATED`
  - `HARDWARE_IO_EXECUTE`
  - `HARDWARE_IO_RESULT`
  - `DRIVER_FAULT_RECOVERY_TEST`
  - `DRIVER_RESTART_COMPLETE`
  - `MILESTONE_B_DEMONSTRATION_COMPLETE`

---

## 7. Frozen-Layer Impact Analysis

- **Kernel Changes**: `0 lines` (`git diff -- kernel/` = 0)
- **Syscall Numbers**: Frozen at Stage 3L (17 syscalls total)
- **Syscall ABI**: 64-byte `SyscallFrame` unchanged
- **Capability Rights**: Bitmask unchanged
- **Bootloader & Page Tables**: Unchanged (`16384 bytes` `.page_tables`)

---

## 8. Final Decision Status

**Final Decision Status: `🟢 READY FOR ARCHITECTURE SPECIFICATION`**
