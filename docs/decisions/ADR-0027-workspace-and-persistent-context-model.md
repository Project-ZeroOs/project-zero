# ADR-0027: Workspace Architecture & Persistent Context Subsystem Model

- **Status**: DRAFT — REQUIRES REVIEW (FREEZE CANDIDATE)
- **Owner**: System Architecture Team
- **Deciders**: Kernel Nucleus, Stage 4 System Services, Workload Orchestration, Security & Storage Teams
- **Date**: 2026-10-04
- **Subsystem**: Stage 4D Workspace & Persistent Context Subsystem (`workspaced`)
- **Authoritative Contracts**: Stage 3A–3N Architecture (Frozen), Stage 4A Architecture (Frozen), Stage 4B Architecture Rev12 (Frozen), Stage 4C Architecture Rev4 (Frozen), ADR-0024, ADR-0025, ADR-0026.

---

## 1. Context and Problem Statement

Traditional operating systems treat the user workspace as an unmanaged desktop directory or a collection of window manager viewports without OS-level capability boundaries. Applications act as ambient-authority dispatchers, forcing users to manually manage files, windows, credentials, and compute sessions.

In ZeroOS, the core paradigm is **"One personal computing environment, multiple physical devices."** To realize this vision, the operating system requires an explicit OS-level abstraction that organizes human intent, workloads, data, application tools, agent sessions, and capabilities around a coherent objective.

The challenge is to define a **Workspace Subsystem (`workspaced`)** that acts as a persistent, capability-secured context boundary without:
1. Creating a second process container or secondary workload scheduler (violating frozen Stage 4C `workloadd`);
2. Introducing ambient authority or a parallel kernel capability domain (violating frozen Stage 3H Capability model);
3. Inventing a redundant identity generator or duplicating filesystem durability layers (violating frozen Stage 4B `DistributedIdAllocator` and Stage 3K ZeroFS);
4. Modifying Ring 0 kernel system calls or frozen Stage 3/4A/4B/4C substrates.

---

## 2. Decision Drivers

1. **Human Intent Over Applications**: The OS must organize state around human objectives rather than forcing the user to dispatch standalone application binaries.
2. **Logical Capability Security & Least Privilege**: Workspace Authority is a logical policy envelope over Stage 3H capabilities held by `workspaced`. `workspaced` can only delegate capabilities it actually holds via `SYS_CAP_DERIVE`. Zero ambient authority.
3. **Substrate Preservation**: Stage 3A–3N (Kernel), Stage 4A (Init/Broker/Supervisor), Stage 4B (`resourced`), and Stage 4C (`workloadd`) are frozen and must remain unmodified.
4. **ZeroFS Atomic Persistence**: Workspace durability uses ZeroFS capability-native extent/direct-pointer journaled storage directly without introducing redundant WAL layers above ZeroFS.
5. **Clear Architectural Separation**: Workspace (`workspaced`) owns context boundary and logical capabilities; Workload (`workloadd`) owns Workload DAG execution lifecycle; Task Process owns memory isolation; Resource (`resourced`) owns capacity accounting.

---

## 3. Considered Options

- **Option 1: Workspace as a Filesystem Directory (`/workspaces/dir/`)**  
  Treat a workspace merely as a ZeroFS folder containing user files.  
  *Rejected*: Fails to provide capability security envelopes, context relationship graphing, or workload process lifecycle containment.

- **Option 2: Workspace as an Agent Process Container**  
  Make Workspace an active agent process managing subordinate child processes directly.  
  *Rejected*: Violates Stage 4C workload orchestration authority (`workloadd`) and creates duplicate process management layers.

- **Option 3: Workspace as a Persistent, Capability-Secured Context Boundary (`workspaced`)**  
  Establish Workspace as a Stage 4D system service (`workspaced`) that manages `WorkspaceControlBlock` metadata headers, enforces logical capability envelopes over Stage 3H handles, maintains an in-memory (resident cache) & ZeroFS journaled Context Graph, and requests execution from Stage 4C `workloadd`.  
  *Selected*: Fully satisfies all architectural drivers while preserving 100% of frozen Stage 3 and Stage 4 substrates.

---

## 4. Decision Outcome

**Chosen Option**: **Option 3: Workspace as a Persistent, Capability-Secured Context Boundary (`workspaced`)**.

### Summary of Architectural Blueprint

```text
                                 Workspace (Stage 4D Context)
                      [ WorkspaceControlBlock, Context Graph ]
                                         │
                                         ▼ OP_WORKLOAD_CREATE Request
                               workloadd (Stage 4C Orchestration)
                                  [ Task DAG Engine ]
                                         │
                                         ▼ SYS_CAP_DERIVE
                            Task Process (Stage 3F / Stage 4A Container)
                                         ▲
                                         │ Application Code Runs Inside
```

1. **Identity**: `WorkspaceId` is a 16-byte `DistributedId` allocated exclusively via Stage 4B `DistributedIdAllocator`.
2. **Logical Capability Envelope**: Workspace ownership grants a logical policy envelope over Stage 3H capability handles held by `workspaced`. Derived workloads, agent sessions, tasks, and processes receive strictly attenuated subsets ($C_{\text{process}} \subseteq C_{\text{task}} \subseteq C_{\text{workload}} \subseteq C_{\text{ws}}$). Workspace membership grants zero ambient authority.
3. **Workload Delegation**: `workspaced` requests workload creation from `workloadd` via IPC (`OP_WORKLOAD_CREATE`). A Workspace logically associates $0..N$ Workloads; a Workload DAG references exactly $1$ parent Workspace.
4. **Context Graph**: Semantic associations (documents, notes, assets, agent sessions) are stored in an in-memory resident LRU cache (`MAX_RESIDENT_NODES = 256`, `MAX_RESIDENT_EDGES = 1024`) and journaled on ZeroFS (`MAX_PERSISTENT_NODES = 4096`, `MAX_PERSISTENT_EDGES = 16384`).
5. **Storage Mapping**: Workspace state persists on ZeroFS at `/workspaces/<workspace_id_hex>/workspace.meta` via ZeroFS atomic file updates and journaled block commits.

---

## 5. Consequences & Pros/Cons

### Positive Consequences
- **Zero Kernel Modifications**: Zero Ring 0 system calls added; kernel remains 100% frozen.
- **Strict Capability Containment**: Prevents cross-workspace data leakage and ambient authority escalation (`I-WS-NO-CAP-AMPLIFICATION`, `I-WS-ISOLATION-BOUNDARY`).
- **Unified Identity & Accounting**: Reuses Stage 4B `DistributedId` and `resourced` quota verification without duplicating accounting.
- **Fail-Closed Deletion Cascade**: Workspace deletion routes through Stage 4C workload cancellation (`OP_WORKLOAD_CANCEL`) and Stage 4B lease quarantine (`C_unavail`) before storage directory tombstoning.

### Negative / Trade-offs
- **Resident vs Persistent Graph Paging**: Requires LRU cache eviction between in-memory resident nodes and ZeroFS persistent graph storage.
- **No Direct Process Spawning**: `workspaced` cannot spawn processes directly; it routes through `workloadd` IPC, preserving Stage 4C authority.

---

## 6. Architectural Invariants

- **`I-WS-ID-UNIQUE`**: Every Workspace is assigned a globally unique `WorkspaceId` from Stage 4B `DistributedIdAllocator`.
- **`I-WS-NO-CAP-AMPLIFICATION`**: `workspaced` can only delegate capabilities that it actually possesses.
- **`I-WS-ISOLATION-BOUNDARY`**: Cross-workspace access requires explicit Stage 3H handle delegation; workspace membership provides zero ambient privilege.
- **`I-WS-NO-RESURRECTION`**: Deleted Workspace cannot resurrect; ID is burned.
- **`I-WS-WORKLOAD-CONTAINMENT`**: Workloads attached to a Workspace cannot exceed the Workspace Capability Envelope.
- **`I-WS-DELETION-CLEANUP-CASCADE`**: Workspace deletion routes through Stage 4C workload cancellation and Stage 4B lease quarantine before storage unmounting.
- **`I-WS-CRASH-CONSISTENCY`**: Persistent metadata and identity remain consistent after crash via ZeroFS atomic journal updates.

---

## 7. Status Sign-off

```text
STATUS: DRAFT — REQUIRES REVIEW (FREEZE CANDIDATE)
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
```
