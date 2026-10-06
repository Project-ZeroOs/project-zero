# Stage 4D Implementation Plan — Workspace & Persistent Context Subsystem (`workspaced`)

**Subsystem**: Workspace & Persistent Context Subsystem (`workspaced`)  
**Status**: DRAFT — IMPLEMENTATION PLAN REQUIRES REVIEW (REV3 — FREEZE CANDIDATE)  
**Implementation**: NOT AUTHORIZED  
**Authoritative Contracts**: Stage 3A–3N Architecture (Frozen), Stage 4A Architecture (Frozen), Stage 4B Architecture Rev12 (Frozen), Stage 4C Architecture Rev4 (Frozen), Stage 4D Architecture Rev3 (Frozen), ADR-0027.

---

## 1. Frozen Architecture Reference

This implementation plan translates the frozen **Stage 4D Architecture Specification Rev3** and **ADR-0027** into a concrete, repository-grounded engineering plan.

```text
Stage 3 Kernel Nucleus (Syscalls, C-Lists, Threads, Address Spaces) [FROZEN]
   │
   ▼
Stage 4A System Services (brokerd Name Service, init Process Spawner) [FROZEN]
   │
   ▼
Stage 4B Unified Resource Graph (resourced Accounting, Leases, DistributedId) [FROZEN]
   │
   ▼
Stage 4C Workload Orchestration (workloadd Task DAGs, Process Recovery) [FROZEN]
   │
   ▼
🟢 Stage 4D Workspace Subsystem (workspaced Control Blocks, Context Graph) [PLANNING]
   │
   ▼
Stage 4E Agent Runtime Subsystem (agentd Intelligence Sessions) [FUTURE]
   │
   ▼
Stage 4F Intent & Compute Fabric Subsystem (Fabric Offloading) [FUTURE]
```

---

## 2. Repository Audit & Stage 3K ZeroFS Interface Inventory

A comprehensive audit of the Project Zero repository was conducted to inventory all existing interfaces, explicitly audit the Stage 3K ZeroFS storage layer, and identify required Phase 4D additions.

### A. Stage 3K ZeroFS Interface Classification Audit

| ZeroFS Operation | Kernel Subsystem Interface | Substrate Status | Usage in Phase 4D |
| :--- | :--- | :--- | :--- |
| **create / open** | `FileManager::alloc_storage_object`, `InodeManager::create_inode` | **EXISTS** | Allocates storage object slot & inode for `workspace.meta` and `context.graph`. |
| **read** | `FileManager::read` | **EXISTS** | Reads 1024B `WorkspaceControlBlock` and Context Graph records. |
| **write** | `FileManager::write` | **EXISTS** | Writes 1024B `WorkspaceControlBlock` and Context Graph node/edge records. |
| **truncate / replace** | `FileManager::truncate`, `InodeManager::truncate` | **EXISTS** | Truncates persistent context files during state reset or tombstoning. |
| **directory creation** | `DirectoryManager::create_entry` | **EXISTS** | Creates `/workspaces/<workspace_id_hex>/` namespace entries under root inode. |
| **directory enumeration** | `DirectoryManager::list_entries` | **EXISTS** | Scans `/workspaces/` namespace directory on `workspaced` daemon startup. |
| **rename / tombstone** | `DirectoryManager::remove_entry`, `INODE_FLAG_PENDING_DELETE` | **EXISTS** | Marks deleted workspace directories as tombstoned for background reclamation. |
| **sync / commit** | `buf::sync_all_buffers`, `Journal::commit` | **EXISTS** | Commits extent metadata & dirty blocks atomically to block device storage. |
| **error reporting** | `FsError` enum (`NotFound`, `TableFull`, `NoSpace`, etc.) | **EXISTS** | Returns explicit filesystem error status to `workspaced`. |

### B. Reused Existing Substrate Interfaces
1. **Stage 3 Kernel Nucleus**:
   - `SYS_CAP_DERIVE` (`syscall::sys_cap_derive`): Capability attenuation for Workspace, Workload, and Task capability handles.
   - `SYS_CAP_REVOKE` (`syscall::sys_cap_revoke`): Revocation of derived capability handles upon Workspace deletion.
   - `SYS_CHANNEL_*` (`syscall::sys_channel_*`): IPC channel communication between processes and system services.
   - `SYS_EXIT` / `SYS_YIELD`: Thread context yield and exit handling.
2. **Stage 4A System Services**:
   - `brokerd` IPC Protocol (`libzero::broker`): `OP_REGISTER_SERVICE` (`0x4A01`), `OP_LOOKUP_SERVICE` (`0x4A03`) for `"workspace.service"` registration.
   - `init` Supervisor Spawner (`libzero::supervisor`): `OP_PROCESS_SPAWN` (`0x4A05`) process container spawner.
3. **Stage 4B Unified Resource Graph (`resourced`)**:
   - `DistributedIdAllocator` (`libzero::identity`): Durable sequence allocator backed by `MemoryPersistenceAuthority` for `WorkspaceId` generation (`I-RES-ID-UNIQUE`).
   - `resourced` IPC Protocol (`libzero::ipc`): `OP_LEASE_REQUEST` (`0x4B01`), `OP_LEASE_RELEASE` (`0x4B03`), `OP_QUOTA_QUERY` (`0x4B05`) for resource quota verification.
4. **Stage 4C Workload Orchestration (`workloadd`)**:
   - `workloadd` IPC Protocol (`libzero::workload`): `OP_WORKLOAD_CREATE` (`0x4C01`), `OP_WORKLOAD_CANCEL` (`0x4C07`), `OP_WORKLOAD_QUERY` (`0x4C05`) for workload DAG execution management.

### C. Missing Interfaces to Add in Phase 4D
- `libzero/src/workspace.rs`: Protocol OpCodes (`0x4D01`–`0x4D14`), `WorkspaceState` enum, `WorkspaceControlBlock` struct, `ContextNode` / `ContextEdge` structs, in-memory Context Graph LRU cache engine.
- `workspaced/Cargo.toml` & `workspaced/src/main.rs`: Freestanding Ring 3 daemon implementing Workspace state machine, ZeroFS header persistence, Context Graph indexing, and IPC dispatch.
- `kernel/src/stage4/tests.rs`: Extension of Stage 4 test harness adding `run_stage4d_verification` alongside existing `run_stage4a_verification`, `run_stage4b_verification`, and `run_stage4c_verification`.
- `tests/test_stage4d.py`: Automated Python test harness validating compilation, symbol export, and QEMU execution.

### D. Substrate Contradiction Audit
- **Contradiction Analysis**: ZERO contradictions discovered. All Stage 4D concepts map 100% cleanly on top of frozen Stage 3, Stage 4A, Stage 4B, and Stage 4C substrates.

---

## 3. Kernel Test Harness Extension vs. Frozen Kernel Substrate Clarification

To ensure absolute architectural clarity regarding kernel code preservation:

1. **Frozen Stage 3A–3N Kernel Nucleus Substrate**: **0 Bytes Modified**.
   - Ring 0 system call dispatch table, memory manager (PMM/VMM), scheduler, capability engine, and microkernel execution primitives remain 100% frozen and untouched.
2. **Stage 4 Ring 3 Verification Test Harness** (`kernel/src/stage4/tests.rs`): **Extension Only**.
   - `kernel/src/stage4/tests.rs` is the repository's established test harness for driving Ring 3 subsystem verifications in QEMU (e.g. `run_stage4a_verification`, `run_stage4b_verification`, `run_stage4c_verification`).
   - Phase 4D extends this test harness by appending `run_stage4d_verification`. This is an extension of the Stage 4 test harness, not a modification of the Stage 3 kernel nucleus substrate.

---

## 4. Implementation Boundary

Phase 4D will strictly implement the frozen Stage 4D Workspace concepts and nothing else:

```text
                  +-----------------------------------+
                  |   STAGE 4D IMPLEMENTATION SCOPE   |
                  +-----------------+-----------------+
                                    |
     ┌──────────────────┬───────────┴───────┬──────────────────┐
     ▼                  ▼                   ▼                  ▼
Workspace Identity  Workspace State    ZeroFS Persistence  Context Graph
& Lifecycle         Machine & IPC      & Header Reconcil.  LRU & Storage
     │                  │                   │                  │
     ▼                  ▼                   ▼                  ▼
Workspace Capability Workspace-Workload  4-Phase Deletion  Crash Recovery
Envelope Attenuation Association        Teardown Cascade  Reconciliation
```

### Out-of-Scope (Prohibited in 4D Implementation)
- ❌ No Ring 0 kernel code changes (Kernel system call count remains strictly 0 added).
- ❌ No modifications to Stage 3A–3N, Stage 4A (`init`/`brokerd`), Stage 4B (`resourced`), or Stage 4C (`workloadd`).
- ❌ No Agent Runtime implementation (Stage 4E `agentd`).
- ❌ No Intent Resolution or Compute Fabric implementation (Stage 4F).
- ❌ No Spatial UI layout composition or rendering (Stage 5).

---

## 5. Workspace Data Structures & On-Disk Representation

All data structures are defined as fixed-size, C-ABI compliant structs (`#[repr(C)]`) to guarantee deterministic memory layouts and zero dynamic allocations.

### A. Authoritative `WorkspaceState` Enum & State Machine
The complete, authoritative state machine across specification and tests consists of 9 unified states:

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceState {
    Unallocated = 0,
    Creating = 1,
    Active = 2,
    Suspending = 3,
    Suspended = 4,
    Resuming = 5,
    Closing = 6,
    Reclaiming = 7,
    Reclaimed = 8,
}
```

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

### B. `WorkspaceControlBlock` (1024 Bytes)
```rust
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WorkspaceControlBlock {
    pub workspace_id: DistributedId,             // 16 bytes: Unified DistributedId (NodeId, LocalSeq)
    pub owner_pid: u64,                         // 8 bytes: Owner process PID / User principal
    pub generation: u32,                        // 4 bytes: Monotonic mutation counter
    pub state: WorkspaceState,                  // 1 byte: u8 enum
    pub _pad0: [u8; 3],                         // 3 bytes alignment
    pub capability_envelope_handle: u32,        // 4 bytes: Stage 3H Capability handle held by workspaced
    pub active_workload_count: u8,              // 1 byte: Count of attached active workloads
    pub _pad1: [u8; 3],                         // 3 bytes alignment
    pub associated_workloads: [DistributedId; 32], // 512 bytes: Up to 32 persistent Workload references
    pub root_dir_handle: u32,                   // 4 bytes: ZeroFS root directory handle
    pub resident_node_count: u32,               // 4 bytes: In-memory resident context node count
    pub resident_edge_count: u32,               // 4 bytes: In-memory resident context edge count
    pub _padding: [u8; 444],                    // Padding to 1024 bytes
}

const _: () = assert!(core::mem::size_of::<WorkspaceControlBlock>() == 1024);
```

### C. Context Graph On-Disk Layout & Lazy Allocation Strategy (`/workspaces/<id>/context.graph`)
The persistent Context Graph is stored as a sparse, lazily allocated record file on ZeroFS:

```text
+-------------------------------------------------------------------------+
| ContextGraphHeader (128 Bytes)                                          |
| magic: u32, version: u32, node_count: u32, edge_count: u32, gen: u32...|
+-------------------------------------------------------------------------+
| Node Record Table: [ContextNodeRecord; N_alloc] (Lazy On-Demand Extent) |
+-------------------------------------------------------------------------+
| Edge Record Table: [ContextEdgeRecord; E_alloc] (Lazy On-Demand Extent) |
+-------------------------------------------------------------------------+
Maximum Logical Capacity Bound: 128 B + 256 KB (4096 nodes) + 512 KB (16384 edges) = ~768.1 KB.
```

- **Lazy Extent Allocation**: On workspace creation, `/workspaces/<id>/context.graph` allocates strictly the 128B header. Node and edge records are appended lazily on-demand as nodes/edges are added. `~768.1 KB` is a **maximum sparse logical capacity bound**, not an eager storage pre-allocation.
- **Slot Management & Tombstoning**: Deleted nodes/edges are tombstoned (validity bit cleared) and added to a free-slot bitmask in the header for fast re-allocation without file re-ordering.
- **Exhaustion Behavior**: Attempting to allocate beyond `MAX_PERSISTENT_NODES = 4096` or `MAX_PERSISTENT_EDGES = 16384` returns `ZeroError::DimensionLimitExceeded`.

---

## 6. ZeroFS Commit Points & Authoritative Crash Reconciliation

### A. Lifecycle ZeroFS Commit Points

```text
[ OP_WORKSPACE_CREATE ]
         │
         ▼
  Allocate WorkspaceId from 4B Allocator
         │
         ▼
  Write workspace.meta (State = Creating) ──► ZeroFS Commit Point 1 (sync_all_buffers)
         │
         ▼
  Initialize context.graph Header
         │
         ▼
  Update workspace.meta (State = Active)  ──► ZeroFS Commit Point 2 (sync_all_buffers)
```

```text
[ OP_WORKSPACE_DELETE ]
         │
         ▼
  Update workspace.meta (State = Closing) ──► ZeroFS Commit Point 1 (sync_all_buffers)
         │
         ▼
  Issue OP_WORKLOAD_CANCEL to workloadd (4C Teardown & 4B Lease Quarantine)
         │
         ▼
  Update workspace.meta (State = Reclaiming) ──► ZeroFS Commit Point 2 (sync_all_buffers)
         │
         ▼
  Revoke C_ws via SYS_CAP_REVOKE & Tombstone Namespace Directory
```

### B. Cross-File Authoritative Recovery Rule
- **Metadata Primacy Invariant**: `workspace.meta` is authoritatively sovereign for Workspace lifecycle and state. `context.graph` is subordinate persistent context state and is reconciled or invalidated according to `workspace.meta`.

### C. Deterministic Crash Recovery Matrix
When `workspaced` restarts following a daemon crash or power failure:
1. **`meta == Creating` (Commit Point 1 passed, Commit Point 2 missing)**:
   - `WorkspaceId` was reserved in Stage 4B allocator (burned via `I-RES-ID-UNIQUE`).
   - `workspaced` detects incomplete initialization, tombstones the directory, and reclaims the slot. The `WorkspaceId` is never resurrected.
2. **`meta == Active`, `context.graph` missing or corrupt**:
   - `workspaced` re-initializes a clean `context.graph` header with generation counter matching `workspace.meta`, preserving workspace identity and metadata while clearing corrupted graph nodes.
3. **`meta == Suspending` or `meta == Resuming`**:
   - On restart, `workspaced` completes state transition to `Suspended` (if suspending) or `Active` (if resuming) after verifying header integrity.
4. **`meta == Closing` (Commit Point 1 passed)**:
   - `workspaced` reads `state == Closing` from ZeroFS. It does **not** revert to `Active`.
   - Re-issues `OP_WORKLOAD_CANCEL` to `workloadd` for all associated workloads, awaits 4C cancellation and 4B lease quarantine (`C_unavail`), and proceeds to `Reclaiming`.
5. **`meta == Reclaiming` (Commit Point 2 passed)**:
   - `workspaced` confirms zero active tasks remain in `workloadd`, revokes derived handles via `SYS_CAP_REVOKE`, tombstones directory, and advances state to `Reclaimed`.

---

## 7. Capability Handle Lifecycle & Scoped Revocation Invariant

To prevent handle invalidation bugs or unintended capability revocation across independent domains:

1. **Handle Classification**:
   - **`C_ws` (Master Workspace Handle)**: Ephemeral handle passed to `workspaced` in RAM at workspace open/create time. Held in `workspaced`'s process C-List.
   - **`C_workload` (Workload Handle)**: Ephemeral handle derived via `SYS_CAP_DERIVE` from `C_ws` when attaching a workload, passed to `workloadd` in `OP_WORKLOAD_CREATE` payload.
   - **`C_task` (Task Handle)**: Ephemeral handle derived via `SYS_CAP_DERIVE` from `C_workload` by `workloadd`, passed to `init` supervisor in `OP_PROCESS_SPAWN` payload.
2. **No Handle Persistence**:
   - Raw numeric handles (e.g. `u32` C-List indices) are process-local and **NEVER stored in persistent ZeroFS files** (`workspace.meta`). Persistent storage holds only policy declarations and `DistributedId` references. Handles are derived fresh via `SYS_CAP_DERIVE` upon workspace opening or state resumption.
3. **Scoped Revocation Invariant (`I-WS-CAP-REVOKE-SCOPED`)**:
   ```text
   C_ws (Workspace Root Handle held by workspaced)
    ├── C_workload_A
    │    └── C_task...
    └── C_workload_B
         └── C_task...
   ```
   - Invoking `sys_cap_revoke(C_ws)` revokes **strictly the capability subtree rooted at `C_ws`**. In full accordance with Stage 3H capability tree revocation contract (`ADR-0017`), it MUST NOT revoke capabilities belonging to unrelated Workspace roots or capabilities independently delegated outside that subtree.

---

## 8. Complete 20-OpCode IPC Protocol Table (10 Requests + 10 Response Tags)

`workspaced` registers as `"workspace.service"` with `brokerd` (OpCode space `0x4D00`–`0x4DFF`).

### Protocol Direction Invariant (`I-WS-PROTOCOL-DIRECTION`)
- Request opcodes (`0x4D01`, `0x4D03`, `0x4D05`, `0x4D07`, `0x4D09`, `0x4D0B`, `0x4D0D`, `0x4D0F`, `0x4D11`, `0x4D13`) are accepted **strictly from client $\to$ `workspaced`**.
- Response opcodes (`0x4D02`, `0x4D04`, `0x4D06`, `0x4D08`, `0x4D0A`, `0x4D0C`, `0x4D0E`, `0x4D10`, `0x4D12`, `0x4D14`) are emitted **strictly by `workspaced` $\to$ client**.
- Receiving a response opcode tag as an incoming client request is rejected immediately with `ZeroError::InvalidRequest`.

| OpCode | Operation Name | Type | Request Payload Struct (Size) | Response Payload Struct (Size) | Required Capability |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `0x4D01` | `OP_WORKSPACE_CREATE` | Request | `WorkspaceCreateReq` (48B) | — | Principal Cap Handle |
| `0x4D02` | `OP_WORKSPACE_CREATE_RESP` | Response | — | `WorkspaceCreateResp` (24B) | Response Tag |
| `0x4D03` | `OP_WORKSPACE_OPEN` | Request | `WorkspaceOpenReq` (24B) | — | Principal Cap Handle |
| `0x4D04` | `OP_WORKSPACE_OPEN_RESP` | Response | — | `WorkspaceOpenResp` (20B) | Response Tag |
| `0x4D05` | `OP_WORKSPACE_CLOSE` | Request | `WorkspaceCloseReq` (20B) | — | `C_ws` Handle |
| `0x4D06` | `OP_WORKSPACE_CLOSE_RESP` | Response | — | `WorkspaceCloseResp` (8B) | Response Tag |
| `0x4D07` | `OP_WORKSPACE_QUERY` | Request | `WorkspaceQueryReq` (20B) | — | `C_ws` Handle |
| `0x4D08` | `OP_WORKSPACE_QUERY_RESP` | Response | — | `WorkspaceQueryResp` (28B) | Response Tag |
| `0x4D09` | `OP_WORKSPACE_SUSPEND` | Request | `WorkspaceSuspendReq` (20B) | — | `C_ws` Handle |
| `0x4D0A` | `OP_WORKSPACE_SUSPEND_RESP` | Response | — | `WorkspaceSuspendResp` (8B) | Response Tag |
| `0x4D0B` | `OP_WORKSPACE_RESUME` | Request | `WorkspaceResumeReq` (20B) | — | `C_ws` Handle |
| `0x4D0C` | `OP_WORKSPACE_RESUME_RESP` | Response | — | `WorkspaceResumeResp` (8B) | Response Tag |
| `0x4D0D` | `OP_WORKSPACE_DELETE` | Request | `WorkspaceDeleteReq` (20B) | — | `C_ws` Handle |
| `0x4D0E` | `OP_WORKSPACE_DELETE_RESP` | Response | — | `WorkspaceDeleteResp` (8B) | Response Tag |
| `0x4D0F` | `OP_WORKSPACE_ATTACH_WORKLOAD`| Request | `WorkspaceAttachReq` (36B) | — | `C_ws` Handle |
| `0x4D10` | `OP_WORKSPACE_ATTACH_WORKLOAD_RESP` | Response | — | `WorkspaceAttachResp` (8B) | Response Tag |
| `0x4D11` | `OP_WORKSPACE_CONTEXT_ADD_NODE`| Request | `ContextAddNodeReq` (88B) | — | `C_ws` Handle |
| `0x4D12` | `OP_WORKSPACE_CONTEXT_ADD_NODE_RESP` | Response | — | `ContextAddNodeResp` (12B) | Response Tag |
| `0x4D13` | `OP_WORKSPACE_CONTEXT_QUERY` | Request | `ContextQueryReq` (24B) | — | `C_ws` Handle |
| `0x4D14` | `OP_WORKSPACE_CONTEXT_QUERY_RESP` | Response | — | `ContextQueryResp` (260B) | Response Tag |

---

## 9. Static Memory Budget & Analysis

Static memory in `workspaced` is completely bounded and pre-allocated in `.bss`:

| Structure | Count | Size per Element | Total Size | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| `WorkspaceControlBlock` Table | 32 | 1,024 bytes | 32,768 bytes (32 KB) | Active & persistent workspace headers |
| Resident Context Node LRU | 256 | 64 bytes | 16,384 bytes (16 KB) | In-memory resident graph nodes |
| Resident Context Edge LRU | 1,024 | 32 bytes | 32,768 bytes (32 KB) | In-memory resident graph edges |
| IPC Message Buffers | 4 | 2,048 bytes | 8,192 bytes (8 KB) | Inbound/outbound IPC message slots |
| Recovery State Scratch | 1 | 8,192 bytes | 8,192 bytes (8 KB) | Header reconciliation scratch buffer |
| **Total Static Memory** | — | — | **98,304 bytes (~96 KB)** | **Completely Bounded** |

---

## 10. Complete Enumeration of 36 QEMU Acceptance Tests (`4D-1` through `4D-36`)

The machine verification suite in `kernel/src/stage4/tests.rs` (`run_stage4d_verification`) will execute all 36 test scenarios:

| Test ID | Test Name | Target Subsystem / Property | Pass Criterion |
| :--- | :--- | :--- | :--- |
| `4D-1` | Workspace Monotonic ID Creation | `DistributedIdAllocator` | Allocated `WorkspaceId` is non-zero & monotonic |
| `4D-2` | Control Block Layout Integrity | C-ABI Layout | `size_of::<WorkspaceControlBlock>() == 1024` |
| `4D-3` | Workspace Creation Transition | State Machine | `Creating` -> `Active` state transition succeeds |
| `4D-4` | Workspace Identity Immutability | Identity | `WorkspaceId` remains unchanged on mutation |
| `4D-5` | Suspend Transition | State Machine | `Active` -> `Suspending` -> `Suspended` succeeds |
| `4D-6` | Resume Transition | State Machine | `Suspended` -> `Resuming` -> `Active` succeeds |
| `4D-7` | Invalid Transition Rejection | State Machine | Double transition returns `ZeroError::InvalidRequest` |
| `4D-8` | ZeroFS Header Persistence Write | Storage | Metadata written to `/workspaces/<id>/workspace.meta` |
| `4D-9` | ZeroFS Header Persistence Read | Storage | Header read matches written control block |
| `4D-10` | ZeroFS Atomic Metadata Update | Storage | Extent sync preserves atomic header state |
| `4D-11` | Resident Node Allocation | Context Graph | In-memory resident node added to LRU cache |
| `4D-12` | Node Label & Type Serialization | Context Graph | `ContextNode` type & UTF-8 label round-trip |
| `4D-13` | Context Edge Allocation | Context Graph | `ContextEdge` relationship binding succeeds |
| `4D-14` | Context Graph Query | Context Graph | Neighborhood search returns expected target nodes |
| `4D-15` | Resident Graph LRU Eviction | Context Cache | Exceeding 256 nodes triggers LRU eviction to disk |
| `4D-16` | Context Graph Reload from Disk | Context Graph | Evicted node reloaded from ZeroFS on query |
| `4D-17` | Persistent Graph Limit Enforcement| Context Graph | Exceeding 4096 persistent nodes returns error |
| `4D-18` | Workload Attachment | Workload Integration | Workload attached to Workspace successfully |
| `4D-19` | Workload Multiplicity Limit | Workload Integration | Attaching 33rd workload returns `ZeroError::QuotaExceeded` |
| `4D-20` | Workload Creation via `workloadd` | 4C IPC | `OP_WORKLOAD_CREATE` dispatched to `workloadd` |
| `4D-21` | Workload Capability Bounding | Security | Derived $C_{\text{workload}} \subseteq C_{\text{ws}}$ enforced |
| `4D-22` | Cross-Workspace Isolation | Security | Accessing Workspace B with $C_{\text{ws\_A}}$ fails |
| `4D-23` | Capability Amplification Rejection | Security | Amplification attempt returns `CapAmplificationRejected` |
| `4D-24` | Resource Quota Verification | 4B IPC | Quota query dispatched to `resourced` |
| `4D-25` | Workspace Deletion Initiation | Lifecycle | `OP_WORKSPACE_DELETE` transitions state to `Closing` |
| `4D-26` | Deletion Cascade 4C Cancel | 4C Teardown | `OP_WORKLOAD_CANCEL` sent to all attached workloads |
| `4D-27` | Deletion Cascade 4B Lease Quarantine| 4B Teardown | Leases surrendered to quarantine ($C_{\text{unavail}}$) |
| `4D-28` | Capability Handle Revocation | Security | `sys_cap_revoke(C_ws)` invalidates handle subtree |
| `4D-29` | ZeroFS Directory Tombstoning | Storage | Namespace directory marked tombstoned |
| `4D-30` | Deletion Final Transition | Lifecycle | State advances to `Reclaimed`; slot freed |
| `4D-31` | Crash Recovery (`Creating`) | Failure / Recovery | Uncommitted creation tombstoned; ID burned |
| `4D-32` | Crash Recovery (`Closing`) | Failure / Recovery | Restart in `Closing` re-initiates 4C cancel |
| `4D-33` | Crash Recovery (Post-Cancel) | Failure / Recovery | Restart queries 4C and advances to `Reclaiming` |
| `4D-34` | Daemon Restart State Reconstitution| Failure / Recovery | Control block table reconstituted from ZeroFS |
| `4D-35` | PMM Frame Leak Neutrality | Memory Safety | `PMM_Baseline == PMM_Final` after 36 tests |
| `4D-36` | Frozen Substrate Preservation | Microkernel Safety | 0 Bytes Stage 3A–3N kernel substrate modified |

---

## 11. Planned Acceptance Criteria & Dual-Verification Strategy

The implementation will be validated against the following explicit acceptance criteria upon completion using a **Dual-Verification Strategy**:

### A. Dual-Verification Breakdown
1. **Behavioral Verification (QEMU Integration)**:
   - `python -m unittest tests/test_stage4d.py` executes cleanly in QEMU.
   - All 36 Stage 4D test cases pass (`4D-1` through `4D-36`).
   - QEMU terminates cleanly with ISA debug exit code 33 (`0x21`).
   - PMM Frame Leak Neutrality is verified (`PMM_Baseline == PMM_Final`).
2. **Repository Source-Level Verification (`git diff`)**:
   - `git diff --stat origin/main` explicitly verifies **0 lines modified** in Stage 3A–3N kernel nucleus files (`kernel/src/hal/`, `kernel/src/mm/`, `kernel/src/task/`, `kernel/src/cap/`, `kernel/src/syscall/`).
   - Kernel source modifications are strictly restricted to the Stage 4 Ring 3 test harness extension in `kernel/src/stage4/tests.rs`.

---

## 12. Status Sign-off Block

```text
STATUS: DRAFT — IMPLEMENTATION PLAN REQUIRES REVIEW (REV3 — FREEZE CANDIDATE)
IMPLEMENTATION: NOT AUTHORIZED
ARCHITECTURE: STAGE 4D REV3 FROZEN
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
```
