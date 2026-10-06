# Stage 4D Architecture Specification — Rev1

**Subsystem**: Workspace & Persistent Context Subsystem (`workspaced`)  
**Status**: DRAFT — REQUIRES REVIEW  
**Authoritative Contracts**: Stage 3A–3N Architecture (Frozen), Stage 4A Architecture (Frozen), Stage 4B Architecture Rev12 (Frozen), Stage 4C Architecture Rev4 (Frozen), ADR-0024, ADR-0025, ADR-0026, ADR-0027.

---

## 1. Scope

This document specifies the authoritative architecture for **Stage 4D: Workspace & Persistent Context Subsystem (`workspaced`)** in ZeroOS.

Stage 4D establishes **Workspace** as a first-class, capability-secured, persistent context boundary within ZeroOS. It defines how human intent, workloads, data, applications, agent sessions, and system capabilities are organized around coherent user objectives without violating frozen microkernel, IPC, resource accounting, or workload orchestration substrates.

### Strict Non-Goals for 4D Architecture
- **No Implementation**: This specification is architecture-only. No code is written or committed in Phase 4D.
- **Zero Kernel/Substrate Modifications**: Stage 3A–3N (Kernel), Stage 4A (Init/Broker/Supervisor), Stage 4B (`resourced`), and Stage 4C (`workloadd`) remain 100% frozen and unmodified.
- **No Agent Runtime Design**: Stage 4E (Agent Runtime) semantics are deferred to Phase 4E. Phase 4D defines only the capability and context boundary exposed to Agents.
- **No Spatial UI Implementation**: Stage 5 spatial UI layout composition is deferred to Phase 5. Phase 4D defines state and context boundaries, not viewport pixels or window rendering.

---

## 2. Frozen Substrate Dependencies

Stage 4D consumes existing, frozen primitives exclusively. It introduces zero Ring 0 system calls and zero modifications to frozen Ring 3 daemons:

```text
+-------------------------------------------------------------------------+
|                         Stage 3 Kernel Nucleus                          |
|   SYS_CAP_DERIVE, SYS_CAP_REVOKE, SYS_CHANNEL_*, SYS_EXIT, SYS_YIELD    |
+-------------------------------------------------------------------------+
                                    │
                                    ▼
+-------------------------------------------------------------------------+
|                  Stage 4A System Services Substrate                     |
|         brokerd (Name Service) & init (Supervisor Process Spawner)      |
+-------------------------------------------------------------------------+
                                    │
                                    ▼
+-------------------------------------------------------------------------+
|              Stage 4B Unified Resource Graph Substrate                  |
|     resourced (Accounting, Leases, Quotas, DistributedIdAllocator)      |
+-------------------------------------------------------------------------+
                                    │
                                    ▼
+-------------------------------------------------------------------------+
|               Stage 4C Workload Orchestration Subsystem                 |
|     workloadd (Task DAGs, Process Recovery Classes, 4-Phase Cancel)     |
+-------------------------------------------------------------------------+
                                    │
                                    ▼
+-------------------------------------------------------------------------+
| 🟢 Stage 4D Workspace & Persistent Context Subsystem (workspaced)      |
|    Workspace Control Blocks, Capability Envelopes, Context Graph        |
+-------------------------------------------------------------------------+
```

---

## 3. Workspace Semantic Definition

In ZeroOS, a **Workspace** is formally defined as:

> **A persistent, capability-secured context boundary that organizes human intent, workloads, data, applications, agents, state, and resources around a coherent objective.**

To preserve architectural precision, a Workspace is explicitly distinguished from all lower-level OS primitives:

| Concept | Primary Responsibility | Subsystem Owner |
| :--- | :--- | :--- |
| **Workspace** | Persistent human/agent context boundary, capability envelope, & semantic graph | Stage 4D (`workspaced`) |
| **Workload** | Execution orchestration DAG of tasks, process lifecycle, & recovery | Stage 4C (`workloadd`) |
| **Task** | Single execution unit node within a Workload DAG | Stage 4C (`workloadd`) |
| **Process** | Isolated address space & hardware thread container | Stage 3F / Stage 4A |
| **Agent** | Autonomous intelligence session executing intent | Stage 4E (`agentd`) |
| **Resource** | Multi-dimensional physical/virtual capacity vector & lease | Stage 4B (`resourced`) |
| **Filesystem** | Content-addressed, journaled hierarchical object storage | Stage 3K (ZeroFS) |
| **Application** | Executable binary image providing domain tools | User Domain |

### What a Workspace Uniquely Owns
1. **Workspace Control Block (`WorkspaceControlBlock`)**: Authoritative header binding metadata, lifecycle state, owner, and capability envelope.
2. **Workspace Capability Envelope ($C_{\text{ws}}$)**: The master unforgeable Stage 3H capability set bounding all operations within the Workspace.
3. **Context Graph**: A directed typed property graph capturing semantic relationships between files, notes, workloads, tools, and agent sessions.
4. **Workspace Root Directory**: Dedicated ZeroFS persistent mount point (`/workspaces/<workspace_id_hex>/`).

### Clarifying Questions & Core Semantic Answers
- **Can multiple Workloads belong to one Workspace?**  
  **Yes.** A Workspace is a context boundary that orchestrates zero, one, or multiple concurrent or sequential Stage 4C Workload DAGs.
- **Can one Workload participate in multiple Workspaces?**  
  **No.** A Workload DAG is strictly owned by exactly one parent Workspace ($1:N$ Workspace-to-Workload mapping) to prevent cross-workspace capability leakage and state ambiguity.
- **Can a Workspace exist without active Workloads?**  
  **Yes.** A Workspace can remain passive or idle in `Suspended` or `Active` state, maintaining files, notes, and Context Graph nodes without any active running processes or workloads.
- **Can a Workload outlive the Workspace that created it?**  
  **No.** Deleting or closing a Workspace triggers cascade cancellation (`OP_WORKLOAD_CANCEL`) of all associated Workloads via Stage 4C 4-phase cancellation.
- **Can Workspaces be nested?**  
  **No.** Workspaces are top-level, flat security context boundaries. Hierarchical sub-contexts are expressed cleanly within a Workspace's internal Context Graph, avoiding recursive privilege-escalation loops.
- **Can Workspaces reference external resources?**  
  **Yes.** A Workspace can hold typed reference nodes pointing to external storage URIs, shared datasets, or remote compute nodes in the Personal Compute Fabric, subject to Capability Envelope authorization.

---

## 4. Workspace Identity & Persistence

Workspace identity is unified through the existing Stage 4B persistence-backed identity authority. ZeroOS does **not** create a second identity generator.

### WorkspaceId Specification
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceId {
    pub node_id: u64,
    pub local_seq: u64,
}
```
- `WorkspaceId` wraps `libzero::resource::DistributedId`.
- Allocated via Stage 4B `DistributedIdAllocator<MemoryPersistenceAuthority>` with crash burn-on-crash non-reuse guarantees (`I-RES-ID-UNIQUE`).

### Persistent State Header (`WorkspaceControlBlock`)
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WorkspaceControlBlock {
    pub workspace_id: WorkspaceId,          // 16 bytes: Unified DistributedId
    pub owner_pid: u64,                     // 8 bytes: Owner process PID / User principal
    pub generation: u32,                    // 4 bytes: Monotonic mutation counter
    pub state: WorkspaceState,              // 1 byte: u8 enum
    pub _pad0: [u8; 3],                     // 3 bytes alignment
    pub capability_envelope_handle: u32,    // 4 bytes: Stage 3H Capability handle
    pub active_workload_count: u8,          // 1 byte: Count of attached active workloads
    pub _pad1: [u8; 3],                     // 3 bytes alignment
    pub workloads: [DistributedId; 8],      // 128 bytes: Attached Workload IDs
    pub root_dir_handle: u32,               // 4 bytes: ZeroFS root directory handle
    pub context_node_count: u32,            // 4 bytes: In-memory context graph node count
    pub context_edge_count: u32,            // 4 bytes: In-memory context graph edge count
    pub _padding: [u8; 336],                // Padding to 512 bytes
}

const _: () = assert!(core::mem::size_of::<WorkspaceControlBlock>() == 512);
```

### Persistence Protocol
- Stored on ZeroFS at `/workspaces/<workspace_id_hex>/workspace.meta`.
- Writes use double-buffered write-ahead log (WAL) journal commits matching Stage 4B persistence semantics (`I-ID-DURABLE-ALLOCATOR-STATE`).
- Corrupted or incomplete metadata triggers automatic rollback to the last verified journal commit header; unrecoverable corruptions transition the workspace slot to `Reclaimed`.

---

## 5. Workspace Context Model

The Workspace Context Model categorizes all state into five strict architectural classes to prevent uncontrolled global state bloat:

```text
                          +-------------------------+
                          |   WORKSPACE BOUNDARY    |
                          +------------+------------+
                                       |
     ┌──────────────────┬──────────────┼──────────────┬──────────────────┐
     ▼                  ▼              ▼              ▼                  ▼
[ Workspace-Owned ] [ Referenced ] [ Derived ]   [ External ]     [ Non-Workspace ]
  - Control Block     - Shared Files - Vector Index - Fabric Nodes    - PMM Tables
  - Context Graph     - Devices      - Log Buffers  - Time Authority  - Ring 0 Sched
  - Root Directory    - Ext Storage  - Thumbnails   - Global Quota    - LAPIC Vectors
  - Capability Env
```

1. **Workspace-Owned**: Owned directly and exclusively by the Workspace. Destroyed on Workspace deletion (`WorkspaceControlBlock`, Context Graph, Root Directory `/workspaces/<id>/`, Capability Envelope).
2. **Workspace-Referenced**: External artifacts linked into the Workspace via immutable ZeroFS handles or URIs. Unaffected by Workspace deletion.
3. **Workspace-Derived**: Reconstructible indexes (vector embeddings, search caches, rendered UI state). Rebuilt automatically on recovery.
4. **External**: Global node services (`resourced`, `brokerd`, `init`, physical hardware).
5. **Non-Workspace Concern**: Microkernel supervisor state, physical memory allocation frames, CPU thread dispatch queues.

---

## 6. Workspace ↔ Workload Relationship

Stage 4D strictly preserves Stage 4C (`workloadd`) as the exclusive execution and orchestration authority. Workspace does **not** become a second workload manager.

```text
Workspace (Stage 4D)
    │
    │  OP_WORKLOAD_CREATE / OP_WORKLOAD_CANCEL
    ▼
Workload (Stage 4C workloadd)
    │
    │  Task DAG Topological Engine
    ▼
Task (Stage 4C)
    │
    │  OP_PROCESS_SPAWN
    ▼
Process (Stage 3F / Stage 4A init supervisor)
```

### Authority Rules
1. **Creation**: A Workspace creates Workloads by issuing `OP_WORKLOAD_CREATE` to `workloadd`, passing its attenuated Capability Envelope handle.
2. **Containment**: A Workload cannot escape its parent Workspace's Capability Envelope. All task capabilities derived by `workloadd` (`SYS_CAP_DERIVE`) are strictly bounded by $C_{\text{ws}}$.
3. **Cancellation**: A Workspace can initiate cancellation of any attached Workload by issuing `OP_WORKLOAD_CANCEL`.
4. **Deletion Cascade**: When a Workspace is deleted or closed, `workspaced` automatically executes `OP_WORKLOAD_CANCEL` for all active attached Workloads before unmounting workspace storage.

---

## 7. Workspace ↔ Capability Security Model

ZeroOS Workspace security is strictly built upon Stage 3H Capability List (C-List) primitives. There is **zero ambient authority**.

### Capability Attenuation Hierarchy
```text
Stage 3H Master Capability Table
               │
               ▼
   Workspace Envelope (C_ws)          <-- Bounded by User Principal
               │
               ├────────────────────────────────────────┐
               ▼ SYS_CAP_DERIVE                         ▼ SYS_CAP_DERIVE
   Workload Authority (C_workload ⊆ C_ws)      Agent Session Authority (C_agent ⊆ C_ws)
               │
               ▼ SYS_CAP_DERIVE
     Task Capability (C_task ⊆ C_workload)
               │
               ▼ SYS_CAP_DERIVE
    Process Capability (C_process ⊆ C_task)
```

### Security Invariants
- **`I-WS-NO-AMBIENT-AUTH`**: No process within a Workspace can access memory, channels, or storage outside its capability list.
- **`I-WS-ISOLATION-BOUNDARY`**: Processes in Workspace $A$ cannot inspect, signal, or communicate with processes in Workspace $B$ without explicit cross-workspace capability handle delegation ($C_{A \to B}$).
- **`I-WS-REVOCATION-CASCADE`**: Revoking $C_{\text{ws}}$ immediately invalidates all derived $C_{\text{workload}}$, $C_{\text{agent}}$, $C_{\text{task}}$, and $C_{\text{process}}$ handles across the entire hierarchy.

---

## 8. Workspace ↔ ZeroFS Storage Mapping

Workspace persistent state maps onto Stage 3K ZeroFS as a structured, isolated directory subtree:

```text
/workspaces/<workspace_id_hex>/
├── workspace.meta              # Double-buffered journaled WorkspaceControlBlock (512B)
├── context.graph               # Serialized Context Graph (Nodes & Edges)
├── data/                       # Workspace-owned user documents & assets
├── checkpoints/                # Class 2 stateful workload checkpoints
└── logs/                       # Execution logs & telemetry
```

### Crash Consistency Protocol
All updates to `workspace.meta` and `context.graph` utilize Stage 4B double-buffered write-ahead log (WAL) commits to ZeroFS blocks. Incomplete writes during system power failure are discarded on boot, restoring the workspace to its last valid committed generation ($G_{c}$).

---

## 9. Workspace Lifecycle State Machine

The Workspace lifecycle consists of 8 deterministic states governed by `workspaced`:

```text
               OP_WORKSPACE_CREATE
                        │
                        ▼
                   [ Creating ]
                        │
                        │ init complete & WAL committed
                        ▼
                    [ Active ] ◄────────────────────┐
                   ╱    │    ╲                      │
     OP_WS_SUSPEND╱     │     ╲ OP_WS_CLOSE         │ OP_WS_RESUME
                 v      │      v                    │
     [ Suspending ]     │   [ Closing ]             │
            │           │        │                  │
   flush WAL│           │        │ cancel workloads │
            v           │        v                  │
      [ Suspended ]─────┼───► [ Reclaiming ] ───────┘
                        │        │
                        │        │ unmount & reclaim ID
                        ▼        v
                   [ Reclaimed / Deleted ]
```

| State | Description | Active Workloads Permitted? |
| :--- | :--- | :--- |
| `Creating` | Allocating `WorkspaceId`, initializing root ZeroFS directory & WAL. | No |
| `Active` | Workspace is open; capability envelope active; workloads executing. | Yes |
| `Suspending` | Writing context graph WAL checkpoint & pausing workloads. | Transient |
| `Suspended` | Context state flushed to ZeroFS; processes halted; RAM preserved or paged. | No |
| `Resuming` | Restoring context graph from WAL; re-activating capability envelope. | Transient |
| `Closing` | Issuing `OP_WORKLOAD_CANCEL` to all active workloads. | Teardown only |
| `Reclaiming` | Revoking $C_{\text{ws}}$, unmounting ZeroFS root, freeing control block. | No |
| `Reclaimed` | Terminal state; slot available for reuse. | No |

---

## 10. Workspace ↔ Applications Integration

ZeroOS maintains seamless compatibility with traditional application binaries:

```text
Workspace (Stage 4D)
    └── Workload (Stage 4C)
            └── Application Process Container (Stage 3F / Stage 4A)
                    ├── Standard I/O IPC Channels
                    └── Attenuated Task Capability (C_task)
```

- **Application Launch**: Spawns a single-task Stage 4C Workload attached to the active Workspace.
- **Application State**: Saved inside `/workspaces/<id>/data/` or referenced in the Context Graph.
- **CLI/TUI Transparency**: Unmodified CLI tools run cleanly inside Task process containers without awareness of Stage 4D internals.

---

## 11. Workspace ↔ Agent Boundary (Stage 4E Interface)

Stage 4D defines the capability and context boundary for future Stage 4E Agents (`agentd`):

- **Containment**: Agent sessions run as processes bounded by an Agent Capability Envelope ($C_{\text{agent}} \subseteq C_{\text{ws}}$).
- **Context Access**: Agents query the Workspace Context Graph via `OP_WORKSPACE_CONTEXT_QUERY` to read semantic history and node relations.
- **Action Execution**: Agents create Workloads or modify files solely by presenting authorized handles derived from $C_{\text{agent}}$. An Agent cannot execute operations beyond $C_{\text{ws}}$.

---

## 12. Workspace ↔ Resource Graph Integration (Stage 4B Interface)

Workspace does **not** bypass Stage 4B `resourced`.

- **Resource Quotas**: Each Workspace may declare a Resource Quota Vector ($V_{\text{ws\_quota}}$) representing maximum permitted multi-dimensional capacity (CPU, RAM, GPU, Storage).
- **Lease Verification**: When `workloadd` requests a resource lease from `resourced` (`OP_LEASE_REQUEST`) on behalf of a task inside Workspace $W$, `resourced` verifies that the active sum of leases for $W$ does not exceed $V_{\text{ws\_quota}}$.

---

## 13. Context Graph Specification

The **Context Graph** is a lightweight, persistent typed property graph representing human/agent intent associations:

```text
[ DocumentNode: "DesignSpec.md" ] ──── (References) ───► [ FileNode: "diagram.png" ]
               │                                                    ▲
               │ (AuthoredBy)                                       │ (GeneratedBy)
               ▼                                                    │
    [ UserNode: "Alice" ] ─── (Spawned) ───► [ WorkloadNode: "BuildPipeline" ]
```

### Node Types
- `DocumentNode`: User documents, notes, and text assets.
- `FileNode`: Binary assets, images, compilations.
- `WorkloadNode`: Stage 4C Workload DAG references.
- `AgentSessionNode`: Stage 4E Agent reasoning sessions.
- `ToolNode`: Registered application binaries/capabilities.
- `ExternalReferenceNode`: URIs pointing to external nodes or web resources.

### Edge Types
- `References`, `GeneratedBy`, `DependsOn`, `DerivedFrom`, `AttachedTo`, `AuthoredBy`.

### Graph Integrity & Bounds
- Fixed in-memory index size per Workspace: `MAX_CONTEXT_NODES = 1024`, `MAX_CONTEXT_EDGES = 4096`.
- Graph search and query operations execute in $O(1)$ or $O(E)$ bounded time without blocking real-time UI schedulers.

---

## 14. Workspace Switching & Concurrency

- **Multiple Active Workspaces**: Multiple Workspaces can exist concurrently in `Active` or `Suspended` state across the node.
- **Active Focused Workspace**: The primary Workspace associated with active user UI input.
- **Background Workspace Execution**: Non-focused Workspaces with running background Workloads continue executing according to Stage 3 proportional-fair scheduler classes without interference.
- **Focus Transition**: Switching focus updates UI viewport bindings without invalidating background Workload capabilities.

---

## 15. Workspace Deletion & Cleanup Protocol

When a Workspace is deleted (`OP_WORKSPACE_DELETE`), `workspaced` executes a fail-closed 6-stage cleanup protocol:

```text
[ OP_WORKSPACE_DELETE ]
         │
         ▼
 1. Transition state to Closing
         │
         ▼
 2. Issue OP_WORKLOAD_CANCEL to all attached Workloads (Stage 4C)
         │
         ▼
 3. Await 4C process termination & 4B lease surrender to quarantine (C_unavail)
         │
         ▼
 4. Revoke Master Workspace Capability Envelope (C_ws) via SYS_CAP_REVOKE
         │
         ▼
 5. Unmount & remove ZeroFS directory /workspaces/<id>/
         │
         ▼
 6. Mark control block as Reclaimed
```

---

## 16. Threat Model & Security Invariants

| Threat Vector | Mitigation Invariant | Structural Enforcement |
| :--- | :--- | :--- |
| Cross-Workspace Data Leak | `I-WS-ISOLATION-BOUNDARY` | Separate C-Lists; ZeroFS capability path enforcement. |
| Stale `WorkspaceId` Reuse | `I-RES-ID-UNIQUE` | Burn-on-crash sequence reservation via Stage 4B allocator. |
| Ambient Authority Escalation | `I-WS-NO-AMBIENT-AUTH` | Zero ambient handles; mandatory `SYS_CAP_DERIVE` attenuation. |
| Compromised Workload Escape | `I-CAPABILITY-ATTENUATION` | Task capabilities strictly bounded by $C_{\text{ws}}$. |
| Deleted Workspace Resurrection | `I-WS-REVOCATION-CASCADE` | Synchronous `SYS_CAP_REVOKE` of $C_{\text{ws}}$ on deletion. |
| Metadata Tampering / Crash | Double-Buffered WAL Journal | ZeroFS WAL journal recovery discards uncommitted blocks. |

---

## 17. Static System Bounds

| Bound Identifier | Value | Driving Architectural Constraint | Exhaustion Behavior |
| :--- | :--- | :--- | :--- |
| `MAX_WORKSPACES` | `32` | Static control block table size in `workspaced` RAM | Returns `ZeroError::ObjectTableFull` |
| `MAX_WORKLOADS_PER_WORKSPACE` | `8` | Bounded by Stage 4C `MAX_CONCURRENT_WORKLOADS` | Returns `ZeroError::QuotaExceeded` |
| `MAX_CONTEXT_NODES_PER_WORKSPACE` | `1024` | Fixed-size in-memory Context Graph index | Returns `ZeroError::DimensionLimitExceeded` |
| `MAX_CONTEXT_EDGES_PER_WORKSPACE` | `4096` | Fixed-size in-memory Edge index | Returns `ZeroError::DimensionLimitExceeded` |
| `MAX_ACTIVE_WORKSPACES_PER_NODE` | `8` | Node-wide active memory footprint limit | Forces auto-suspend of least-recently-used Workspace |

---

## 18. Service Boundary Specification (`workspaced`)

`workspaced` registers as `"workspace.service"` with `brokerd` (OpCode space `0x4D00`–`0x4DFF`).

### Protocol OpCodes
```rust
pub const OP_WORKSPACE_CREATE:            u64 = 0x4D01;
pub const OP_WORKSPACE_CREATE_RESP:       u64 = 0x4D02;
pub const OP_WORKSPACE_OPEN:              u64 = 0x4D03;
pub const OP_WORKSPACE_OPEN_RESP:         u64 = 0x4D04;
pub const OP_WORKSPACE_CLOSE:             u64 = 0x4D05;
pub const OP_WORKSPACE_CLOSE_RESP:        u64 = 0x4D06;
pub const OP_WORKSPACE_QUERY:             u64 = 0x4D07;
pub const OP_WORKSPACE_QUERY_RESP:        u64 = 0x4D08;
pub const OP_WORKSPACE_SUSPEND:           u64 = 0x4D09;
pub const OP_WORKSPACE_SUSPEND_RESP:      u64 = 0x4D0A;
pub const OP_WORKSPACE_RESUME:            u64 = 0x4D0B;
pub const OP_WORKSPACE_RESUME_RESP:       u64 = 0x4D0C;
pub const OP_WORKSPACE_DELETE:            u64 = 0x4D0D;
pub const OP_WORKSPACE_DELETE_RESP:       u64 = 0x4D0E;
pub const OP_WORKSPACE_ATTACH_WORKLOAD:   u64 = 0x4D0F;
pub const OP_WORKSPACE_ATTACH_WORKLOAD_RESP: u64 = 0x4D10;
pub const OP_WORKSPACE_CONTEXT_ADD_NODE:  u64 = 0x4D11;
pub const OP_WORKSPACE_CONTEXT_ADD_NODE_RESP: u64 = 0x4D12;
pub const OP_WORKSPACE_CONTEXT_QUERY:     u64 = 0x4D13;
pub const OP_WORKSPACE_CONTEXT_QUERY_RESP:u64 = 0x4D14;
```

---

## 19. Subsystem Failure & Crash Recovery

### `workspaced` Daemon Crash Recovery Protocol
When `workspaced` restarts following an unexpected process crash or system reboot:
1. **Header Inventory**: Scans `/workspaces/*.meta` on ZeroFS.
2. **WAL Reconciliation**: Validates double-buffered WAL headers for each workspace directory, discarding incomplete writes.
3. **Workload Reconciliation**: Queries `workloadd` (`OP_WORKLOAD_QUERY`) for all active workloads listed in workspace headers.
4. **State Reconstitution**: Re-populates `WorkspaceControlBlock` slots in memory and restores active `WorkspaceState`.

---

## 20. Dependency Lineage Graph

```text
Stage 3 Kernel Nucleus (Syscalls, C-Lists, Threads, Address Spaces)
   │
   ▼
Stage 4A System Services (brokerd Name Service, init Process Spawner)
   │
   ▼
Stage 4B Unified Resource Graph (resourced Accounting, Leases, DistributedId)
   │
   ▼
Stage 4C Workload Orchestration (workloadd Task DAGs, Process Recovery)
   │
   ▼
🟢 Stage 4D Workspace Subsystem (workspaced Control Blocks, Context Graph)
   │
   ▼
Stage 4E Agent Runtime Subsystem (agentd Intelligence Sessions) [Future]
   │
   ▼
Stage 4F Intent & Compute Fabric Subsystem (Fabric Offloading) [Future]
```

---

## 21. Subsystem Invariants

1. **`I-WS-ID-UNIQUE`**: Every Workspace is assigned a globally unique `WorkspaceId` from Stage 4B `DistributedIdAllocator`.
2. **`I-WS-NO-AMBIENT-AUTH`**: All Workspace operations require explicit capability handles derived from $C_{\text{ws}}$.
3. **`I-WS-WORKLOAD-CONTAINMENT`**: Workloads attached to a Workspace cannot exceed the Workspace Capability Envelope.
4. **`I-WS-DELETION-CLEANUP`**: Workspace deletion cascades workload cancellation, capability revocation, and lease quarantine before directory unmounting.
5. **`I-WS-PMM-NEUTRALITY`**: Workspace lifecycle operations commit zero un-reclaimed memory or capability leaks.

---

## 22. Implementation Boundary & Sign-off

```text
STATUS: DRAFT — REQUIRES REVIEW
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
```
