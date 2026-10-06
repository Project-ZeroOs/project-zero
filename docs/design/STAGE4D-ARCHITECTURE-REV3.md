# Stage 4D Architecture Specification — Rev3

**Subsystem**: Workspace & Persistent Context Subsystem (`workspaced`)  
**Status**: DRAFT — REQUIRES REVIEW (FREEZE CANDIDATE)  
**Authoritative Contracts**: Stage 3A–3N Architecture (Frozen), Stage 4A Architecture (Frozen), Stage 4B Architecture Rev12 (Frozen), Stage 4C Architecture Rev4 (Frozen), ADR-0024, ADR-0025, ADR-0026, ADR-0027.

---

## 1. Scope

This document specifies the authoritative architecture for **Stage 4D: Workspace & Persistent Context Subsystem (`workspaced`)** in ZeroOS (Revision 3).

Stage 4D establishes **Workspace** as a first-class, capability-secured, persistent context boundary within ZeroOS. It defines how human intent, workloads, data, applications, agent sessions, and system capabilities are organized around coherent user objectives without violating frozen microkernel, IPC, resource accounting, or workload orchestration substrates.

### Strict Non-Goals for Phase 4D
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
|    Workspace Control Blocks, Logical Capability Envelopes, Context Graph|
+-------------------------------------------------------------------------+
```

---

## 3. Workspace Semantic Definition

In ZeroOS, a **Workspace** is formally defined as:

> **A persistent, capability-secured context boundary that organizes human intent, workloads, data, applications, agents, state, and resources around a coherent objective.**

To preserve architectural precision, a Workspace is explicitly distinguished from all lower-level OS primitives:

| Concept | Primary Responsibility | Subsystem Owner |
| :--- | :--- | :--- |
| **Workspace** | Persistent context boundary, logical capability envelope, & semantic graph | Stage 4D (`workspaced`) |
| **Workload** | Execution orchestration DAG of tasks, process lifecycle, & recovery | Stage 4C (`workloadd`) |
| **Task** | Single execution unit node within a Workload DAG | Stage 4C (`workloadd`) |
| **Process** | Isolated address space & hardware thread container | Stage 3F / Stage 4A |
| **Agent** | Autonomous intelligence session executing intent | Stage 4E (`agentd`) |
| **Resource** | Multi-dimensional physical/virtual capacity vector & lease | Stage 4B (`resourced`) |
| **Filesystem** | Capability-native extent/direct-pointer journaled storage (ZeroFS) | Stage 3K (ZeroFS) |
| **Application** | Executable binary image running inside a Process container | User Domain |

### What a Workspace Uniquely Owns vs Associates
- **Workspace Owns**: `WorkspaceControlBlock` metadata header, Logical Capability Envelope configuration, in-memory/persistent Context Graph, and Workspace Root Directory namespace (`/workspaces/<workspace_id_hex>/`).
- **Workspace Associates**: Stage 4C Workloads (`workloadd` owns execution lifecycle; Workload references parent `WorkspaceId`).

### Clarifying Questions & Core Semantic Answers
- **Can multiple Workloads belong to one Workspace?**  
  **Yes.** A Workspace is a context boundary that logically associates zero, one, or multiple concurrent or sequential Stage 4C Workload DAGs.
- **Can one Workload participate in multiple Workspaces?**  
  **No.** A Workload DAG references exactly one parent Workspace ($1:N$ Workspace-to-Workload mapping) to prevent cross-workspace capability leakage and state ambiguity.
- **Can a Workspace exist without active Workloads?**  
  **Yes.** A Workspace can remain passive or idle in `Suspended` or `Active` state, maintaining files, notes, and Context Graph nodes without any active running processes or workloads.
- **Can a Workload outlive the Workspace that created it?**  
  **No.** Closing or deleting a Workspace requests cascade cancellation (`OP_WORKLOAD_CANCEL`) of all associated Workloads via Stage 4C 4-phase cancellation.
- **Can Workspaces be nested?**  
  **No.** Workspaces are top-level, flat security context boundaries. Hierarchical sub-contexts are expressed cleanly within a Workspace's internal Context Graph.
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
    pub capability_envelope_handle: u32,    // 4 bytes: Stage 3H Capability handle held by workspaced
    pub active_workload_count: u8,          // 1 byte: Count of attached active workloads
    pub _pad1: [u8; 3],                     // 3 bytes alignment
    pub associated_workloads: [DistributedId; 32], // 512 bytes: Up to 32 persistent Workload references
    pub root_dir_handle: u32,               // 4 bytes: ZeroFS root directory handle
    pub resident_node_count: u32,           // 4 bytes: In-memory resident context node count
    pub resident_edge_count: u32,           // 4 bytes: In-memory resident context edge count
    pub _padding: [u8; 444],                // Padding to 1024 bytes
}

const _: () = assert!(core::mem::size_of::<WorkspaceControlBlock>() == 1024);
```

### ZeroFS Atomic Persistence Protocol
- Workspace metadata persists on ZeroFS at `/workspaces/<workspace_id_hex>/workspace.meta`.
- **ZeroFS Extent/Journal Integration**: All metadata writes utilize ZeroFS's native atomic file updates and journaled block commits (`I-WS-CRASH-CONSISTENCY`). 
- **No Duplicate WAL**: Stage 4D relies directly on ZeroFS journaled durability rather than inventing a redundant second WAL layer above ZeroFS.

---

## 5. Workspace Context Model

The Workspace Context Model categorizes all state into five strict architectural classes:

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
  - Logical Cap Env
```

1. **Workspace-Owned**: Owned directly and exclusively by the Workspace (`WorkspaceControlBlock`, Context Graph, Root Directory namespace `/workspaces/<id>/`).
2. **Workspace-Referenced**: External artifacts linked into the Workspace via immutable ZeroFS handles or URIs.
3. **Workspace-Derived**: Reconstructible indexes (vector embeddings, search caches, rendered UI state).
4. **External**: Global node services (`resourced`, `brokerd`, `init`, physical hardware).
5. **Non-Workspace Concern**: Microkernel supervisor state, physical memory allocation frames, CPU thread dispatch queues.

---

## 6. Workspace ↔ Workload Capability Derivation & Transfer Path

Stage 4D defines a precise, enforceable capability derivation path using existing Stage 3H primitives (`SYS_CAP_DERIVE`):

```text
User / Parent Principal Authority
         │
         ▼
workspaced Process C-List (Holds C_ws Handle)
         │
         ▼ SYS_CAP_DERIVE (Stage 3H Syscall)
workspaced derives Workload-scoped capability handle (C_workload ⊆ C_ws)
         │
         ▼ IPC Transfer (OP_WORKLOAD_CREATE payload)
workloadd receives C_workload handle
         │
         ▼ SYS_CAP_DERIVE (Stage 3H Syscall)
workloadd derives Task-scoped capability handle (C_task ⊆ C_workload)
         │
         ▼ IPC Transfer (OP_PROCESS_SPAWN payload)
init supervisor passes C_task handle to spawned Task Process
```

### Enforceable Capability Rules
- **`I-WS-WORKLOAD-CONTAINMENT`**: A Workload associated with Workspace $W$ may receive only capabilities derived from the capability handle $C_{\text{ws}}$ delegated by $W$. `workloadd` MUST NOT attach or derive any capability outside that authority chain.
- **`I-WS-ISOLATION-BOUNDARY`**: A process running inside Workspace $A$ cannot derive or access handles from Workspace $B$ ($C_{\text{ws\_B}}$) unless explicit cross-workspace handle delegation occurs.
- **No Ambient Authority**: Workspace membership itself grants zero ambient kernel authority.

---

## 7. Workspace ↔ ZeroFS Namespace Storage Mapping

Workspace persistent state maps onto Stage 3K ZeroFS as a structured directory namespace convention:

```text
/workspaces/<workspace_id_hex>/
├── workspace.meta              # ZeroFS journaled WorkspaceControlBlock (1024B)
├── context.graph               # ZeroFS journaled Context Graph (Nodes & Edges)
├── data/                       # Workspace-owned user documents & assets
├── checkpoints/                # Class 2 stateful workload checkpoints
└── logs/                       # Task execution logs
```

- **Directory Namespace**: The path `/workspaces/<id>/` is a ZeroFS directory namespace convention, not an independent VFS filesystem mount.

---

## 8. Workspace Lifecycle State Machine & Deterministic Crash Recovery

The Workspace lifecycle consists of 8 deterministic states governed by `workspaced`:

```text
               OP_WORKSPACE_CREATE
                        │
                        ▼
                   [ Creating ]
                        │
                        │ init complete & ZeroFS committed
                        ▼
                    [ Active ] ◄────────────────────┐
                   ╱    │    ╲                      │
     OP_WS_SUSPEND╱     │     ╲ OP_WS_CLOSE         │ OP_WS_RESUME
                 v      │      v                    │
     [ Suspending ]     │   [ Closing ]             │
            │           │        │                  │
   flush state  │        │ cancel workloads │
            v           │        v                  │
      [ Suspended ]─────┼───► [ Reclaiming ] ───────┘
                        │        │
                        │        │ mark deleted & reclaim ID
                        ▼        v
                   [ Reclaimed / Deleted ]
```

### Deterministic Crash Reconciliation Rules
When `workspaced` restarts following an un-scheduled system reboot or daemon failure, it executes deterministic crash recovery:

1. **Crash During `Creating` State**:
   - If `WorkspaceId` was allocated but `workspace.meta` commit was incomplete: `WorkspaceId` burn-on-crash invariant (`I-RES-ID-UNIQUE`) ensures the ID is burned. The incomplete directory is tombstoned and reclaimed on restart. The ID is never resurrected.
2. **Crash During `Closing` State**:
   - On restart, `workspaced` reads state `Closing` from ZeroFS. It does **not** revert to `Active`. It re-issues `OP_WORKLOAD_CANCEL` to `workloadd`, awaits 4C cancellation and 4B lease quarantine, and proceeds to `Reclaiming`.
3. **Crash After Workload Cancellation but Before Metadata Update**:
   - `workspaced` queries `workloadd` (`OP_WORKLOAD_QUERY`), confirms zero active tasks remain, advances state to `Reclaiming`, revokes its derived handles via `SYS_CAP_REVOKE`, and updates ZeroFS metadata.

---

## 9. Workspace ↔ Application Relationship

ZeroOS maintains seamless compatibility with traditional application binaries via a strict 4-tier execution hierarchy:

```text
Workspace (Stage 4D Context)
   │
   ▼
Workload (Stage 4C Execution Intent)
   │
   ▼
Task (Stage 4C Execution Unit)
   │
   ▼
Process (Stage 3F Execution Container)
   ▲
   │
Application Code (Executes INSIDE Process)
```

- **Application Execution**: Application code executes **inside** a Process container managed by a Stage 4C Task within a Workload associated with a Workspace.
- **CLI/TUI Transparency**: Traditional CLI/TUI applications run unmodified inside Task process containers inheriting standard I/O channels without needing Stage 4D API awareness.

---

## 10. Context Graph Specification

The **Context Graph** is a lightweight, persistent typed property graph representing human/agent intent associations. It is strictly distinguished from Task DAGs and Resource Graphs:

- **Context Graph (Stage 4D)**: Represents semantic human/agent context and asset relationships.
- **Task DAG (Stage 4C)**: Represents operational process execution dependencies (`workloadd`).
- **Resource Graph (Stage 4B)**: Represents physical hardware topology and capacity accounting (`resourced`).

### Resident vs Persistent Capacity Bounds
- `MAX_PERSISTENT_NODES = 4096`: Stage 4D architectural configuration bound selected for bounded metadata management (not a ZeroFS filesystem limit).
- `MAX_PERSISTENT_EDGES = 16384`: Stage 4D architectural configuration bound for relationship storage.
- `MAX_RESIDENT_NODES = 256`: Fixed in-memory LRU Context Graph cache budget in `workspaced` RAM.
- `MAX_RESIDENT_EDGES = 1024`: Fixed in-memory Edge index cache budget in `workspaced` RAM.

---

## 11. Workspace Deletion & Teardown Protocol

Workspace deletion routes strictly through Stage 4C workload cancellation and Stage 4B lease quarantine:

```text
[ OP_WORKSPACE_DELETE ]
         │
         ▼
 1. workspaced transitions workspace state to Closing
         │
         ▼
 2. workspaced sends OP_WORKLOAD_CANCEL to workloadd for all associated Workloads
         │
         ▼
 3. workloadd executes 4C cancellation protocol (terminate tasks -> release leases)
         │
         ▼
 4. resourced surrenders leases to quarantine (C_unavail) until provider confirms
         │
         ▼
 5. workspaced revokes its derived handles via SYS_CAP_REVOKE
         │
         ▼
 6. ZeroFS marks workspace persistent state as tombstoned & reclaims directory
```

---

## 12. Threat Model & Invariants

| Threat Vector | Mitigation Invariant | Structural Enforcement |
| :--- | :--- | :--- |
| Capability Amplification | `I-WS-NO-CAP-AMPLIFICATION` | `workspaced` only delegates Stage 3H capabilities it holds. |
| Cross-Workspace Data Leak | `I-WS-ISOLATION-BOUNDARY` | Separate C-Lists; ZeroFS capability path enforcement. |
| Stale `WorkspaceId` Reuse | `I-RES-ID-UNIQUE` | Burn-on-crash sequence reservation via 4B allocator. |
| Deleted Workspace Resurrection | `I-WS-NO-RESURRECTION` | Burn-on-crash ID non-reuse; synchronous handle revocation. |
| Compromised Workload Escape | `I-WS-WORKLOAD-CONTAINMENT` | Task capabilities strictly derived from $C_{\text{ws}}$. |
| Deletion Teardown Bypass | `I-WS-DELETION-CLEANUP-CASCADE` | Mandatory 4C cancellation & 4B lease quarantine. |
| Crash Metadata Corruption | `I-WS-CRASH-CONSISTENCY` | ZeroFS atomic file write and journaled block recovery. |

---

## 13. Static System Bounds

| Bound Identifier | Value | Driving Architectural Constraint | Exhaustion Behavior |
| :--- | :--- | :--- | :--- |
| `MAX_WORKSPACES` | `32` | Control block table size in `workspaced` RAM | Returns `ZeroError::ObjectTableFull` |
| `MAX_WORKLOADS_PER_WORKSPACE` | `32` | Persistent workload reference array capacity | Returns `ZeroError::QuotaExceeded` |
| `MAX_RESIDENT_NODES_PER_WORKSPACE` | `256` | Fixed in-memory LRU Context Graph cache budget | LRU eviction to ZeroFS |
| `MAX_RESIDENT_EDGES_PER_WORKSPACE` | `1024` | Fixed in-memory Edge index cache budget | LRU eviction to ZeroFS |
| `MAX_PERSISTENT_NODES_PER_WORKSPACE`| `4096` | Stage 4D configuration bound for node storage | Returns `ZeroError::DimensionLimitExceeded` |
| `MAX_PERSISTENT_EDGES_PER_WORKSPACE`| `16384` | Stage 4D configuration bound for edge storage | Returns `ZeroError::DimensionLimitExceeded` |

---

## 14. Service Boundary Specification (`workspaced`)

`workspaced` registers as `"workspace.service"` with `brokerd` (OpCode space `0x4D00`–`0x4DFF`).

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

## 15. Dependency Lineage Graph

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

## 16. Implementation Boundary & Sign-off

```text
STATUS: DRAFT — REQUIRES REVIEW (FREEZE CANDIDATE)
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
```
