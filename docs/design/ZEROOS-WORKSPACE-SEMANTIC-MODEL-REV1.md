# ZeroOS Workspace Semantic Model (REV1)

**Status:** 🟢 ARCHITECTURE READY  
**Layer:** Architectural Specification (Ring 3 / System Daemon Layer)  
**Dependencies:**  
- `docs/design/ZEROOS-OBJECT-AND-MEMBERSHIP-MODEL-REV8.md` (FROZEN)  
- `docs/design/ZEROOS-FILESYSTEM-MUTATION-AND-DIRECTORY-OPERATIONS-ARCHITECTURE-REV3.md` (FROZEN)  
- `docs/design/ZEROOS-OBJECT-AND-MEMBERSHIP-REV8-FREEZE.md` (FROZEN)  
- `docs/design/ZEROOS-OBJECT-AND-MEMBERSHIP-REV8-FINAL-VERIFICATION.md` (FROZEN)  

---

## 1. Executive Summary & Purpose

The **ZeroOS Workspace Semantic Model (REV1)** defines the OS-level semantic identity, boundaries, lifecycle, security context, execution environment, and human intent binding for a **ZeroOS Workspace**.

Positioned directly above the frozen Object & Membership REV8 substrate and the frozen ZeroFS REV3 filesystem mutation layer, the Workspace model answers the core architectural question:

> **What exactly is a ZeroOS Workspace, and how does it organize persistent state, workloads, agents, capabilities, and human intent into a unified OS abstraction?**

This specification establishes that a ZeroOS Workspace is **not** a mere directory, process group, desktop window, container, chat thread, or project folder. Instead, it is a first-class OS-level logical context defined by the formal relation:

$$\text{Workspace} = \text{Persistent State} + \text{Membership Boundary} + \text{Execution Context} + \text{Human Intent Graph} + \text{Capability Envelope}$$

This document provides complete formal definitions, state machines, identity matrices, recovery semantics, and adversarial proofs for the Workspace layer without introducing code, kernel changes, syscall additions, or modifications to frozen substrates.

---

## 2. Relationship to Frozen REV8 Substrate

The ZeroOS system architecture adheres strictly to a multi-layer stack:

```text
+-----------------------------------------------------------------------+
|                    HUMAN INTENT & APPLICATION LAYER                    |
+-----------------------------------------------------------------------+
|                 ZEROOS WORKSPACE SEMANTIC MODEL (REV1)                |
|  [WorkspaceId | Membership Graph | Intent DAG | Resource Policy]      |
+-----------------------------------------------------------------------+
|                 OBJECT & MEMBERSHIP MODEL (REV8) - FROZEN             |
|  [ObjectId | Persistent Registry | Reconciliation | System Membership]|
+-----------------------------------------------------------------------+
|              ZEROFS MUTATION SUBSTRATE (REV3) - FROZEN               |
|  [Journal V2 | Two-Phase Commit | Directory Inodes | Mutate Bit 15]   |
+-----------------------------------------------------------------------+
|                          ZEROFS PHYSICAL STORAGE                      |
|  [Device Block Storage | Inode Blocks | Data Extents]                 |
+-----------------------------------------------------------------------+
```

### Architectural Guarantees & Constraints:
1. **REV8 Substrate Immutability:** REV8 defines physical object discovery, `ObjectId` persistence in `/storage/system/object_id.registry`, inode binding `(device_id, inode_num)`, and journaled two-phase mutations. REV1 consumes REV8 without altering any REV8 data structure, registry format, or reconciliation algorithm.
2. **Identity Decoupling:** `ObjectId` (128-bit persistent logical identity of a ZeroFS object) and `WorkspaceId` (128-bit persistent identity of a Workspace) exist in separate identity domains.
3. **Zero Kernel Mutation:** The kernel, syscall numbers, capability bit definitions (including Bit 15 `MUTATE = 0x8000`), and process management APIs remain 100% frozen. The Workspace model operates entirely in Ring 3 within the system daemon space (`workspaced`).

---

## 3. Core Workspace Semantic Identity

### 3.1 What a Workspace Is NOT
A ZeroOS Workspace must never be defined or constrained as:
- **A POSIX Directory:** Directory trees reflect physical storage organization; Workspace membership reflects logical semantic utility.
- **A Process Group:** Process groups are transient kernel execution handles; Workspaces survive process termination, daemon restarts, and system reboots.
- **A Container / VM:** Containers isolate namespaces and OS resources; Workspaces establish semantic boundaries for intent, membership, capabilities, and agent state.
- **A Desktop Window / Workspace:** Window managers track visual layout pixels; ZeroOS Workspaces track persistent state and workload governance.
- **A Chat Session:** Chat logs are linear dialogue streams; Workspace intent is a structured directed acyclic graph (DAG) of goals, constraints, and state transitions.

### 3.2 Formal OS Definition
A **ZeroOS Workspace** is a persistent, first-class OS entity representing a **bounded domain of human intent, execution context, resource policy, capability authorization, and object membership**.

Mathematically, a Workspace $W_i$ is defined by the 6-tuple:

$$W_i = \langle \text{WSID}_i, M_i, W\!L_i, A_i, I_i, C_i \rangle$$

Where:
- $\text{WSID}_i \in \mathbb{U}_{128}$: Globally unique 128-bit persistent Workspace Identifier.
- $M_i = \{ \text{ObjID}_k \mid \text{MEMBER\_OF}(W_i, \text{ObjID}_k) \}$: Set of logical Object membership edges.
- $W\!L_i = \{ \text{WorkloadID}_m \}$: Set of associated active and declared execution workloads.
- $A_i = \{ \text{AgentID}_n \}$: Set of autonomous persistent control entities assigned to the workspace.
- $I_i$: Structured Human Intent Context Graph (Task DAG, goals, constraints, context state).
- $C_i$: Workspace Capability Envelope governing access delegation.

### 3.3 Workspace Entity Identity Choice
**Architectural Decision:** Is a Workspace itself a ZeroFS Object, or a higher-level logical entity?

**Chosen Architecture:** A Workspace is a **higher-level logical entity** (a system context graph node managed by `workspaced`) backed by persistent system manifests stored at `/storage/system/workspaces/<ws_id>/manifest.json`.

**Trade-Off & Consequence Analysis:**
- *Option A (Workspace is a ZeroFS Object):* If Workspace were a ZeroFS object assigned an `ObjectId`, it would require a parent workspace to define its membership, causing an infinite regression loop or requiring circular self-membership exceptions in REV8.
- *Option B (Workspace is a Logical Context Node - CHOSEN):* Workspace identity `WorkspaceId` is distinct from `ObjectId`. The workspace manifest is persisted in system storage as a standard ZeroFS file, but its logical identity `WorkspaceId` is recognized by `workspaced` as a context root. This cleanly decouples physical object lifecycle from workspace governance.

---

## 4. Entity Identity Matrix

The following table establishes the explicit architectural boundaries between all primary entities in ZeroOS:

| Entity | Identity Type & Representation | Lifetime | Persistent? | Primary Owner | Can Move Across Host/Nodes? | Can Contain Other Entities? |
|---|---|---|---|---|---|---|
| **Object** | `ObjectId` (128-bit UUID/Hash bound to inode `(dev, inode)`) | Permanent (until unlinked & reclaimed) | **Yes** (persisted in `/storage/system/object_id.registry`) | System / User | Yes (file rename/move preserves identity) | No (Directories contain path links, not object identity) |
| **Workspace** | `WorkspaceId` (128-bit UUID) | Persistent (until explicitly destroyed) | **Yes** (persisted in `/storage/system/workspaces/`) | User / Tenant | Yes (manifest & graph can export across fabric) | **Yes** (Contains Workloads, Agents, Intent Graph, Capabilities, Membership edges) |
| **Workload** | `WorkloadId` (128-bit UUID) | Runtime Execution (Process/Task lifetime) | **No** (Execution instance is transient; definition persists) | Workspace | Yes (can migrate across fabric compute nodes) | Yes (Contains Threads, Memory allocations, Open file handles) |
| **Agent** | `AgentId` (128-bit UUID) | Autonomous Lifecycle | **Yes** (Agent memory, goal graph, and identity persist) | Workspace / User | Yes (executes across nodes via fabric daemon) | Yes (Contains Tasks, Memory store, Sub-agent handles) |
| **Process** | `ProcessId` (32-bit OS PID) | Kernel Process Lifetime | **No** (Dies on exit, crash, or reboot) | Workload | No (Tightly bound to local kernel instance) | Yes (Contains Threads, File Descriptor tables) |
| **Capability** | `CapHandle` (64-bit token / bitfield + target path) | Session / Granted Scope | **Optional** (Ephemeral handle or persistent grant token) | Process / Agent / Workload | Yes (Delegatable across IPC/RPC boundaries) | Yes (Contains rights: Read, Write, Execute, Mutate Bit 15) |

---

## 5. Membership Model

### 5.1 Object Membership Semantics
REV8 establishes persistent `ObjectId` identity and ZeroFS storage reconciliation. REV1 Workspace Semantic Model defines how Objects relate to Workspaces.

**Evaluated Models:**
- *Model A (Strict Single Workspace):* Every Object belongs to exactly one Workspace. (Rejected: Prevents cross-workspace collaboration, code library sharing, and multi-project file referencing).
- *Model B (Multi-Workspace Contextual Graph - CHOSEN):* An `ObjectId` can belong to multiple Workspaces simultaneously via explicit logical context edges $\text{MEMBER\_OF}(W_i, \text{ObjID}_k)$.

```text
                   +-------------------+
                   |   Object X        |
                   | (ObjectId 0x8F4A) |
                   +---------+---------+
                             |
             +---------------+---------------+
             |                               |
             v                               v
   +-------------------+           +-------------------+
   |    Workspace A    |           |    Workspace B    |
   | (WorkspaceId W1)  |           | (WorkspaceId W2)  |
   +-------------------+           +-------------------+
```

### 5.2 Decoupling Filesystem Containment from Workspace Membership
- **Filesystem Containment:** Represents physical directory hierarchy in ZeroFS (e.g., `/storage/projects/alpha/src/main.rs`).
- **Workspace Membership:** Represents a logical edge between a Workspace and an `ObjectId`.
- **Architectural Rule:** Moving or renaming a file in ZeroFS (`sys_file_rename`) updates physical path references but **does not alter Workspace membership edges**. Workspace membership is indexed by `ObjectId`, not by physical path string.

### 5.3 Membership Lifecycle Operations
1. **Creation (Link):** A workspace daemon or user creates a logical edge $\text{MEMBER\_OF}(W_i, \text{ObjID}_k)$. If `ObjID_k` does not exist in the REV8 registry, it is registered first.
2. **Removal (Unlink):** Removing an object from a workspace deletes the edge $\text{MEMBER\_OF}(W_i, \text{ObjID}_k)$. The underlying ZeroFS object and its `ObjectId` remain intact.
3. **Transfer (Re-parenting):** Modifying membership edges to remove from $W_A$ and attach to $W_B$.
4. **Inheritance & Scope Expansion:** When a directory object `ObjID_dir` is added to $W_i$, child objects discovered within `ObjID_dir` inherit membership in $W_i$ by default scope rule, unless explicitly overridden.
5. **Visibility & Authorization:** Workspace membership acts as an authorization filter. A workload running in $W_A$ queries objects scoped to $W_A$'s membership graph.

---

## 6. Path Projections vs Workspace Containment

### 6.1 Multi-Logical Projections
An Object with `ObjectId X` located at physical path `/storage/repo/core/lib.rs` can be projected into multiple user-visible workspace views:

```text
Physical ZeroFS Path:
/storage/repo/core/lib.rs ----------> ObjectId 0x99A2 (Inode 4092)
                                                |
                   +----------------------------+----------------------------+
                   |                                                         |
                   v                                                         v
Workspace A Virtual Path:                                 Workspace B Virtual Path:
/workspaces/ws_alpha/views/lib.rs                         /workspaces/ws_beta/shared/core_lib.rs
```

### 6.2 Path vs Workspace Operations Matrix

| Operation | Physical File Effect | `ObjectId` Effect | Workspace Membership Effect |
|---|---|---|---|
| `sys_file_rename(old, new)` | Path updated in ZeroFS directory inode | Unchanged | Unchanged (membership tracks `ObjectId`) |
| Move between directories | Inode unlinked/re-linked in target directory | Unchanged | Unchanged |
| Overwrite file content | Blocks updated via REV3 Journal V2 | Unchanged | Unchanged |
| Unlink physical file (`sys_file_delete`) | Inode reference count decremented | Unlinked if refcount == 0 | Membership edge marked `STALE_UNRESOLVED` |
| Remote Fabric Sync | Extents synchronized to fabric node | Unchanged | Edge references Fabric-backed `ObjectId` |

---

## 7. Workspace Lifecycle State Machine

A ZeroOS Workspace transitions through explicit, deterministic lifecycle states managed by `workspaced`:

```text
    +--------------+
    |    CREATE    |
    +-------+------+
            |
            v
    +--------------+
    |  INITIALIZE  |
    +-------+------+
            |
            v
    +--------------+            Suspend Command / Idle Timeout
    |    ACTIVE    +-----------------------------------------------+
    +-------+------+                                               |
            ^                                                      v
            |                   Resume Command               +--------------+
            +------------------------------------------------+  SUSPENDED   |
            |                                                +-------+------+
            |                                                        |
            |                   Daemon Boot / Restore                |
            +--------------------------------------------------------+
            |
            v
    +--------------+
    |   ARCHIVED   |
    +-------+------+
            |
            v
    +--------------+
    |  DESTROYED   |
    +--------------+
```

### 7.1 State Definitions & Transition Matrix

| Lifecycle State | Description | Persistence State | Workloads | Agents | Capabilities |
|---|---|---|---|---|---|
| `CREATE` | Allocates `WorkspaceId`, initializes manifest structure. | Transient memory manifest | None | None | Admin allocation |
| `INITIALIZE` | Loads membership edges, attaches intent DAG, registers system hooks. | Saved to `/storage/system/workspaces/<ws_id>/` | Spawning autostart workloads | Loading agent state | Resolving initial handles |
| `ACTIVE` | Fully operational workspace context. | Periodically flushed journal | Running | Active autonomous execution | Fully active & derived |
| `SUSPENDED` | Quiesced state. Memory footprint minimized. | Flushed & locked manifest | Paused / Terminated | Quiesced (State saved to disk) | Quiesced / Frozen |
| `RESUMED` | Transition from `SUSPENDED` back to `ACTIVE`. | Manifest validated | Re-hydrated / Resumed | Woken with state intact | Re-issued / Verified |
| `ARCHIVED` | Offline read-only storage state. Compute detached. | Compressed manifest + graph | Terminated | Unloaded (Saved archive) | Revoked |
| `DESTROYED` | Metadata purged, workspace entry removed. | Manifest deleted | Terminated & cleaned | Unloaded & unlinked | Permanently Revoked |

### 7.2 Boot & Power Survival Rules
- **What Survives Machine Reboot?**
  1. Workspace identity (`WorkspaceId`) and metadata manifest.
  2. Object membership graph edges $\text{MEMBER\_OF}(W_i, \text{ObjID}_k)$.
  3. Persistent Agent definitions, memory stores, and goal trees.
  4. Human Intent DAG state and historical task log.
  5. Declared resource policies and security cap boundaries.
- **What Dies & Is Reconstructed on Reboot?**
  1. Active OS processes (`ProcessId`), threads, and memory pages.
  2. Transient open file descriptors and volatile socket connections.
  3. Active CPU/GPU execution handles. Workloads are re-hydrated by `workspaced` according to the workspace autostart policy.

---

## 8. Workspace Deletion Semantics

### 8.1 Critical Boundary Principle
$$\text{Workspace Deletion} \neq \text{Object Deletion}$$

Deleting a Workspace is an operation on the **logical context envelope**, not on physical ZeroFS storage objects.

```text
BEFORE DELETION:
Workspace A (WSID W1)
 ├── Membership Edge ---> Object X (ObjectId 0x101)
 ├── Membership Edge ---> Object Y (ObjectId 0x102)
 ├── Workload A (PID 405)
 └── Agent Z (AgentId AG9)

ACTION: Destroy Workspace A

AFTER DELETION:
Workspace A: DESTROYED (Manifest purged from system storage)
Object X:    PRESERVED in ZeroFS filesystem (/storage/docs/report.pdf)
Object Y:    PRESERVED in ZeroFS filesystem (/storage/code/app.rs)
Agent Z:     UNBOUND (Archived to system agent pool or unlinked)
Workload A:  TERMINATED (SIGTERM -> SIGKILL issued to PID 405)
Caps:        REVOKED (Capabilities derived from WSID W1 invalidated)
```

### 8.2 Deletion Cascading Rules
1. **Objects:** `MEMBER_OF` edges are purged. Physical files remain in ZeroFS with unchanged `ObjectId`s.
2. **Workloads:** Active processes attached to the workspace receive `SIGTERM` followed by `SIGKILL`.
3. **Agents:** Workspace-exclusive agents transition to `UNBOUND_ARCHIVED` state. Multi-workspace agents drop the deleted workspace from their scope.
4. **Capabilities:** All capability handles associated with `WorkspaceId` are invalidated by the capability broker (`brokerd`).

---

## 9. Workload Relationship

### 9.1 OS Execution Boundary
A **Workload** is a unit of compute execution (a single process, process tree, or containerized execution context) operating within ZeroOS.

```text
+-----------------------------------------------------------------------+
|                             WORKSPACE A                               |
|                                                                       |
|  +-----------------------------------------------------------------+  |
|  |                           WORKLOAD 1                            |  |
|  |  +-------------------+  +-------------------+                   |  |
|  |  | Process (PID 102) |  | Process (PID 103) |                   |  |
|  |  +---------+---------+  +---------+---------+                   |  |
|  |            |                      |                             |  |
|  |            +----------+-----------+                             |  |
|  |                       |                                         |  |
|  |                       v                                         |  |
|  |     [Workspace Inherited Capability Scope / Storage Scope]      |  |
|  +-----------------------------------------------------------------+  |
+-----------------------------------------------------------------------+
```

### 9.2 Workspace Contract with Workloads
Workspace provides to a Workload:
- **Object Visibility Scope:** Workloads query objects via workspace membership index.
- **Resource Priority Tier:** Shares of CPU, memory, NPU, and GPU allocated to the parent workspace.
- **Security Envelope:** Base capabilities inherited by processes within the workload.
- **Intent Context:** Access to current task goals and workspace environment variables.

### 9.3 Standalone Workloads
**Can a Workload exist without a Workspace?**  
**Architectural Answer:** No. Every workload must execute within a workspace context. System processes and background daemons that operate outside user workspaces execute within the implicit **System Core Workspace (`WS_SYSTEM_0`)**, ensuring uniform OS resource and capability governance.

---

## 10. Agent Relationship

### 10.1 OS-Level Agent Integration
In ZeroOS, an **Agent** is an autonomous, persistent control entity with independent goal structures, state memory, and reasoning capabilities.

```text
+-----------------------------------------------------------------------+
|                            WORKSPACE W_1                              |
|                                                                       |
|   +---------------------------------------------------------------+   |
|   |                       AGENT (AgentId A1)                      |   |
|   |                                                               |   |
|   |  Persistent State:                                            |   |
|   |   - Goal Graph: [Task 1 -> Task 2 -> Task 3]                  |   |
|   |   - Memory Store: Vector Store / Local Context                |   |
|   |   - Autonomous Loop: Active                                   |   |
|   |                                                               |   |
|   |  Execution Surface:                                           |   |
|   |   - Spawns Workload 101 (Code execution)                      |   |
|   |   - Invokes Syscalls / Capability Brokers                     |   |
|   +---------------------------------------------------------------+   |
+-----------------------------------------------------------------------+
```

### 10.2 Agent Lifecycle & Constraint Rules
1. **Multi-Workspace Operations:** An Agent can be assigned a primary Workspace $W_1$, and request delegated capability tokens to read/modify objects in secondary Workspace $W_2$.
2. **Workload Spawning Authorization:** Agents spawn workloads via `workspaced`. Spawning is validated against the Workspace Capability Envelope.
3. **Workspace Capability Constraint:** An Agent **cannot** bypass the security envelope of its containing Workspace. Agent actions are strictly bounded by $C_{\text{workspace}}$.
4. **Workspace Suspension Effect:** When $W_1$ enters `SUSPENDED`, active agent execution loops are paused, volatile agent state is serialized to `/storage/system/workspaces/W1/agents/A1_state.bin`, and agent workloads are quiesced.
5. **Workspace Destruction Effect:** When $W_1$ is destroyed, agent definitions are archived or unlinked. Agent transient execution dies immediately.

---

## 11. Human Intent Integration

### 11.1 Intent Model Architecture
ZeroOS represents **Human Intent** not as unconstrained text or opaque AI state, but as an explicit, OS-managed **Intent Graph (Directed Acyclic Graph)** attached to a Workspace.

```text
USER INTENT PROMPT / DIRECTIVE: "Optimize project build and verify tests"
                                |
                                v
                +-------------------------------+
                |     WORKSPACE INTENT GRAPH    |
                |                               |
                |  [Goal: Build Optimization]   |
                |             |                 |
                |      +------+------+          |
                |      |             |          |
                |      v             v          |
                |  [Task 1:      [Task 2:       |
                |   Compile]      Profile]      |
                |      |             |          |
                |      +------+------+          |
                |             |                 |
                |             v                 |
                |     [Task 3: Run Suite]       |
                +---------------+---------------+
                                |
                                v
                +-------------------------------+
                |   WORKLOAD & AGENT EXECUTION  |
                +-------------------------------+
```

### 11.2 OS Semantic Rules for Intent
1. **Persistence & Mutability:** Workspace Intent is persistent metadata saved in `intent.json` within the workspace system path. It is mutable as goals progress or users revise directives.
2. **Non-AI OS Contract:** The OS kernel and daemons do not rely on probabilistic AI models to interpret intent. Intent is translated into structured task nodes, scheduling parameters, security constraints, and resource priority tiers.
3. **Effect on Resource Allocation:** High-priority intent nodes boost CPU/GPU compute shares for associated workloads.
4. **Effect on Authorization:** Intent nodes define execution boundaries. An agent working on Task 1 (`Compile`) cannot request capabilities for unrelated sensitive objects unless intent explicitly spans those objects.

---

## 12. Resource Context & Fabric Abstraction

### 12.1 OS Resource Policy Scope
A Workspace defines declarative resource governance policies without requiring manual hardware management by the user:

$$\text{ResourcePolicy}(W_i) = \langle \text{PriorityTier}, \text{GPUMode}, \text{MemoryCeiling}, \text{NetworkAccessMode} \rangle$$

### 12.2 Resource Boundary Division

```text
+-----------------------------------------------------------------------+
|                         WORKSPACE LEVEL                               |
|  Declarative Policy: Priority = High | GPU = Allowed | MaxMem = 16GB   |
+----------------------------------+------------------------------------+
                                   |
                                   v
+-----------------------------------------------------------------------+
|                         WORKLOAD LEVEL                                |
|  Enforced Limits: Cgroup / Quota handles (e.g. 4 CPU Cores, 8GB RAM)   |
+----------------------------------+------------------------------------+
                                   |
                                   v
+-----------------------------------------------------------------------+
|                    ZEROOS SCHEDULER & HARDWARE                        |
|  Physical CPU / GPU / NPU Extents & Time Slices                       |
+-----------------------------------------------------------------------+
```

---

## 13. Capability & Security Boundary

### 13.1 Integration with REV3/REV8 Capabilities
ZeroOS filesystem mutations enforce Bit 15 `MUTATE = 0x8000` in capability handles for write operations. Workspace semantics build upon this foundation.

### 13.2 Workspace Capability Envelope Rules
1. **Capability Scoping:** A process executing in Workspace $W_A$ receives capability handles whose target paths are verified against $W_A$'s membership graph.
2. **Cross-Workspace Access:** A workload in $W_A$ requesting access to an Object in $W_B$ must invoke the ZeroOS Capability Broker (`brokerd`). Access requires an explicit delegation token signed by $W_B$'s authority.
3. **Agent Delegation:** Agents inherit capability limits from their primary Workspace. An Agent cannot grant capabilities to its sub-workloads that exceed its own capability envelope.
4. **Workspace Destruction Security:** Upon workspace destruction, `brokerd` immediately invalidates all capability tokens containing `WorkspaceId == W_destroyed`.

---

## 14. Workspace Persistence Model

The state of a ZeroOS Workspace is categorized into four explicit persistence tiers:

```text
+-----------------------------------------------------------------------+
|                             PERSISTENCE TIERS                         |
|                                                                       |
|  1. MUST PERSIST (Saved to /storage/system/workspaces/<ws_id>/)       |
|     - WorkspaceId & System Metadata Manifest                          |
|     - Object Membership Graph Edges (MEMBER_OF)                       |
|     - Persistent Agent Goal Trees & Memory Index                      |
|     - Human Intent Graph (Task DAG)                                   |
|     - Declared Resource Policies & Security Envelopes                 |
|                                                                       |
|  2. MAY PERSIST (Saved as session cache)                              |
|     - Window layout, scroll positions, open document tab states       |
|     - Transient UI state metadata                                     |
|     - Uncommitted agent reasoning scratchpads                         |
|                                                                       |
|  3. MUST NOT PERSIST (Strictly volatile)                              |
|     - OS Process IDs (PIDs), thread handles, socket file descriptors  |
|     - Volatile CPU/GPU register states & RAM pages                    |
|     - Transient IPC shared memory channels                            |
|                                                                       |
|  4. RECONSTRUCTED (Re-created upon workspace startup/boot)            |
|     - Workload runtime processes (spawned per autostart policy)       |
|     - Open file handle tables                                         |
|     - Active capability handle caches in kernel                       |
+-----------------------------------------------------------------------+
```

---

## 15. Recovery Model & Boot Sequence

### 15.1 Boot Recovery Workflow

```text
   +-------------------------------------------------------+
   |  1. ZEROOS KERNEL BOOT & ZEROFS MOUNT                 |
   +---------------------------+---------------------------+
                               |
                               v
   +-------------------------------------------------------+
   |  2. WORKSPACED DAEMON LAUNCH                          |
   +---------------------------+---------------------------+
                               |
                               v
   +-------------------------------------------------------+
   |  3. LOAD REV8 REGISTRY (/storage/system/object_id.registry)
   +---------------------------+---------------------------+
                               |
                               v
   +-------------------------------------------------------+
   |  4. DISCOVER & LOAD WORKSPACE MANIFESTS                |
   |     (/storage/system/workspaces/*/manifest.json)       |
   +---------------------------+---------------------------+
                               |
                               v
   +-------------------------------------------------------+
   |  5. RECONCILE MEMBERSHIP EDGES AGAINST REV8 OBJECTS   |
   +---------------------------+---------------------------+
                               |
                               v
   +-------------------------------------------------------+
   |  6. RESTORE PERSISTENT AGENTS & INTENT DAGS           |
   +---------------------------+---------------------------+
                               |
                               v
   +-------------------------------------------------------+
   |  7. REHYDRATE WORKLOADS PER AUTOSTART POLICY          |
   +-------------------------------------------------------+
```

### 15.2 Recovery Matrix for Degradation Scenarios

| Crash / Failure Condition | Detected State | System Recovery Action |
|---|---|---|
| Reboot during workspace creation | Incomplete manifest file | Daemon discards partial manifest; no orphan objects created |
| Object physically deleted outside workspace | `MEMBER_OF` edge points to missing `ObjectId` | Edge marked `STALE_UNRESOLVED`. Workspace remains functional |
| Dead Workload PID on reboot | Manifest references non-existent PID | PID reference purged. Workload re-spawned per autostart policy |
| Hardware GPU removed | Resource policy requests GPU extents | Scheduler degrades workload to CPU execution fallback |
| Network disconnect during fabric operation | Fabric node unreachable | Workspace enters `DEGRADED_LOCAL_ONLY` state (see Section 16) |

---

## 16. Offline-First & Local-First Semantics

ZeroOS Workspaces are designed around an **Offline-First Architectural Guarantee**:

$$\text{Workspace Operation} \centernot\implies \text{Network Availability}$$

```text
+-----------------------------------------------------------------------+
|                    LOCAL-FIRST WORKSPACE BOUNDARY                     |
|                                                                       |
|   Workspace Core Operations (100% Local):                             |
|   - Workspace Creation, Manifest Updates, Lifecycle State Changes     |
|   - REV8 ObjectId Lookup & REV3 File Mutations                        |
|   - Local Workload Execution & Local Agent Reasoning                  |
|   - Human Intent Graph Management                                     |
|                                                                       |
|   Network Disconnect Transitions:                                     |
|   +--------------------------+-------------------------------------+  |
|   | Fabric Sync Request      | -> Enqueued in Local Deferred Queue   |  |
|   | Remote Agent Task        | -> Degraded to Local Model Fallback |  |
|   | Remote Object Access     | -> Access Cache or Return EHOSTUNREACH|  |
|   +--------------------------+-------------------------------------+  |
+-----------------------------------------------------------------------+
```

Workspaces maintain full operational authority regardless of fabric connection state. Operations that require remote nodes are deferred asynchronously without blocking local workload execution.

---

## 17. Legacy POSIX Application Integration

Legacy applications (e.g., standard C/C++ POSIX binaries, bash scripts, standard tools) interact with ZeroOS Workspaces through an explicit translation boundary managed by Ring 3 wrappers:

```text
+-----------------------------------------------------------------------+
|                     LEGACY POSIX APPLICATION                          |
|             (Opens file: /storage/projects/foo/data.txt)              |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                    WORKLOAD EXECUTION CONTAINER                       |
|           (Bound to Workspace W_1 via Environment / Scope)            |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                      RING 3 DAEMON (workspaced)                       |
|  1. Resolves path -> Inode -> REV8 ObjectId 0x77B1                    |
|  2. Checks: MEMBER_OF(W_1, 0x77B1)? -> Validated                      |
|  3. Validates Capability Bit 15 (MUTATE) for write operations        |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                         ZEROOS KERNEL & ZEROFS                        |
+-----------------------------------------------------------------------+
```

### Legacy Application Rules:
1. **Workspace Binding:** Legacy apps run inside a Workload attached to a parent Workspace.
2. **Implicit Membership Rule:** A legacy app opening a file via POSIX path **does not** automatically add that file to the Workspace's membership graph. Membership mutation requires explicit workspace API invocation.
3. **Path Translation:** POSIX paths are translated to physical ZeroFS inodes, resolved to `ObjectId` via the REV8 registry, and checked against workspace capability scopes.

---

## 18. Distributed Compute Fabric Semantics

In ZeroOS, remote execution across a compute fabric is an **implementation detail**, not a user-visible mental model.

```text
+-----------------------------------------------------------------------+
|                         WORKSPACE W_ALPHA                             |
|              (Logical Workspace Context - Unified View)               |
|                                                                       |
|   +----------------------------+    +-----------------------------+   |
|   |   WORKLOAD A (LOCAL NODE)  |    |   WORKLOAD B (REMOTE NODE)  |   |
|   |   Host: Local x86_64 Core  |    |   Host: Remote NPU Cluster  |   |
|   +--------------+-------------+    +--------------+--------------+   |
|                  |                                 |                  |
|                  +----------------+----------------+                  |
|                                   |                                   |
|                                   v                                   |
|       [Shared Workspace Intent DAG & Membership Object Graph]         |
+-----------------------------------------------------------------------+
```

The Workspace boundary spans fabric nodes transparently. `workspaced` synchronizes manifest states across fabric daemons while preserving `WorkspaceId` identity and security envelopes.

---

## 19. User-Visible Semantic Model & OS Contract Examples

### Concrete User Interactions & OS Semantic Translations:

1. **User Statement:** *"Open my project."*
   - **OS Translation:** User selects a `WorkspaceId`. `workspaced` loads `/storage/system/workspaces/<ws_id>/manifest.json`, transitions state from `SUSPENDED` to `ACTIVE`, mounts member `ObjectId` projections, and hydrates autostart workloads.

2. **User Statement:** *"Continue where I left off."*
   - **OS Translation:** `workspaced` reads the saved Human Intent DAG state ($I_i$) and session cache, restores Agent goal context, re-establishes workspace capability scopes, and launches execution at the uncompleted task node.

3. **User Statement:** *"Work on this document."*
   - **OS Translation:** The document's `ObjectId` is linked to the active Workspace via a `MEMBER_OF` edge. The active Agent adds a task node to the Intent DAG, and workload capabilities are derived for the target `ObjectId`.

---

## 20. Adversarial Scenario Analysis

To ensure complete architectural soundness, the model is tested against 12 adversarial stress scenarios:

### Scenario A — One Object in Two Workspaces
- **Setup:** `ObjectId 0x100` is linked to Workspace A and Workspace B.
- **Expected Result:** Valid. Both workspaces hold logical `MEMBER_OF` edges to `0x100`.
- **Authority:** REV8 persistent registry holds `0x100`. Both workspaces govern execution context independently.
- **Security:** Modifying `0x100` from Workspace A requires capability with Bit 15 (`MUTATE`) for `0x100`. REV3 two-phase journal prevents block corruption.

### Scenario B — Rename Object Referenced by Two Workspaces
- **Setup:** `ObjectId 0x100` at `/storage/foo.txt` is renamed to `/storage/bar.txt`.
- **Expected Result:** REV8 preserves `ObjectId 0x100`. Both Workspace A and Workspace B retain valid membership edges without update because membership tracks `ObjectId`, not path strings.

### Scenario C — Delete Workspace While Agent Is Running
- **Setup:** Workspace A is destroyed while Agent Z is executing an active task loop.
- **Expected Result:** `workspaced` issues `SIGTERM`/`SIGKILL` to Agent Z's workloads. Agent Z state is saved to system archive. Capability tokens derived from Workspace A are immediately revoked by `brokerd`. Objects in Workspace A remain intact in ZeroFS.

### Scenario D — Reboot During Workspace Mutation
- **Setup:** Power failure occurs while `workspaced` is writing an updated workspace manifest.
- **Expected Result:** REV3 Journal V2 atomic write rules apply. The manifest write either completes fully or reverts to the previous valid manifest on boot. No corrupted workspace state is loaded.

### Scenario E — Workspace Contains Workload Using Unavailable GPU
- **Setup:** A workspace specifying a GPU resource policy is booted on a host lacking a GPU.
- **Expected Result:** The scheduler detects missing hardware, degrades the workload resource allocation to CPU execution fallback, logs a degraded resource event, and allows workspace initialization to complete.

### Scenario F — Agent Requests Capability Outside Workspace
- **Setup:** Agent Z in Workspace A requests write access to `ObjectId 0x200` belonging exclusively to Workspace B.
- **Expected Result:** Request denied by `brokerd`. Agent Z cannot bypass Workspace A's security envelope without explicit cross-workspace delegation signed by Workspace B.

### Scenario G — Network Disappears Mid-Execution
- **Setup:** Host loses network link while an agent is executing a multi-step task.
- **Expected Result:** Workspace switches to local-only execution mode. Local workloads continue operating. Fabric synchronization tasks are enqueued in local deferred queues.

### Scenario H — Remote Workload Becomes Unreachable
- **Setup:** Remote fabric node running Workload B crashes or drops off the network.
- **Expected Result:** `workspaced` receives heartbeat timeout, marks Workload B as `UNREACHABLE_FABRIC`, notifies the local Agent, and attempts local workload re-hydration if resource policy permits.

### Scenario I — Legacy Application Opens an Object
- **Setup:** A legacy C binary opens an object at `/storage/data.bin` via `sys_file_open`.
- **Expected Result:** Kernel resolves inode to `ObjectId`, checks workload capability scope. Open succeeds if authorized. Opening the file **does not** alter workspace membership graphs.

### Scenario J — User Says "Continue Where I Left Off" After Reboot
- **Setup:** System reboots following a forced shutdown. User requests session restoration.
- **Expected Result:** `workspaced` loads the persisted Intent DAG, validates member `ObjectId`s against the restored REV8 registry, re-hydrates autostart workloads, and resumes agent task execution from the last persisted task checkpoint.

### Scenario K — Same Workspace Restored on Different Machine
- **Setup:** Workspace manifest and member objects are transferred to a new host machine.
- **Expected Result:** The new host's `workspaced` daemon imports the workspace manifest, reconciles member objects into its local REV8 registry, resolves host-specific hardware allocations, and initializes the workspace in `SUSPENDED` state.

### Scenario L — Object Physically Deleted While Workspace Still References It
- **Setup:** Object `0x300` is deleted via `sys_file_delete` while Workspace A holds a `MEMBER_OF` edge.
- **Expected Result:** Physical inode and REV8 registry entry are unlinked. On workspace inspection, the `MEMBER_OF` edge fails `ObjectId` existence check and is marked `STALE_UNRESOLVED`. The workspace remains functional.

---

## 21. Formal Architectural Invariants

The Workspace Semantic Model strictly enforces the following 12 invariants:

- **W1 (Persistent Workspace Identity):** Workspace identity (`WorkspaceId`) is a permanent 128-bit logical identifier that persists across daemon restarts, execution terminations, and system reboots.
- **W2 (Object Identity Independence):** `ObjectId` exists independently of Workspace membership. Workspace membership does not alter or re-generate an `ObjectId`.
- **W3 (Substrate Non-Redefinition):** Workspace membership does not redefine ZeroFS inode identity, journal semantics, or physical storage layout.
- **W4 (Non-Destructive Deletion):** Destroying a Workspace removes logical membership edges and workspace metadata; it does **not** delete underlying ZeroFS physical objects.
- **W5 (Explicit Workload Boundary):** Every workload process executes within an explicit Workspace boundary (defaulting to System Workspace `WS_SYSTEM_0` for system daemons).
- **W6 (Explicit Agent Boundary):** Autonomous agents operate within defined Workspace capability envelopes and cannot silently escalate authorization.
- **W7 (Capability Non-Bypass):** Workspace boundaries strictly respect frozen capability rules, including Bit 15 `MUTATE = 0x8000` enforcement for write operations.
- **W8 (Deterministic Recovery):** Workspace boot recovery proceeds in a deterministic sequence without producing orphaned workspace states or corrupted manifests.
- **W9 (Offline Independence):** Workspaces function completely offline; network unavailability never prevents local workspace operations or local object mutations.
- **W10 (Resource Placement Abstraction):** Physical compute placement (CPU/GPU/NPU/Fabric) is managed by OS resource daemons and remains transparent to user intent models.
- **W11 (Deterministic Reconciliation):** Reconciliation of workspace membership graphs against REV8 registries never silently creates duplicate logical identities.
- **W12 (Intent/Identity Decoupling):** Human Intent graphs represent high-level task workflows and remain strictly decoupled from low-level physical file/inode identities.

---

## 22. Non-Goals & Open Questions

### Non-Goals:
- **No Kernel Changes:** This model requires zero modifications to kernel memory managers, schedulers, or system call tables.
- **No REV9 Invention:** REV8 Object & Membership substrate is frozen and fully sufficient.
- **No UI/UX Spec:** Visual window placement, desktop shell rendering, and CSS/HTML layouts are outside the scope of this OS semantic specification.
- **No AI Model Architecture:** The specification defines OS contracts for human intent, not prompt engineering algorithms or LLM weight formats.

### Open Questions:
- *Q1 (Cross-Tenant Workspace Migration):* What formal cryptographic re-keying sequence is required when exporting a Workspace manifest between untrusted tenant nodes? (To be addressed in Stage 7 Security Architecture).
- *Q2 (Intent Graph Merging):* When two users collaborate by merging Workspace A and Workspace B, what conflict-resolution semantics apply to overlapping task DAG nodes? (To be specified in Stage 7 Workspace Collaboration Spec).

---

## 23. Dependency Matrix & Implementation Status

| Component | Status | Architectural Dependency |
|---|---|---|
| ZeroFS Mutation Substrate (REV3) | 🟢 FROZEN | Substrate block journal & directory mutation |
| Object & Membership Model (REV8) | 🟢 FROZEN | Persistent `ObjectId` registry & reconciliation |
| REV8 Registry Persistence | 🟢 VERIFIED | Durable `/storage/system/object_id.registry` |
| Workspace Semantic Model (REV1) | 🟢 ARCHITECTURE READY | Higher-level logical context & intent boundary |

---

## Explicit Implementation Boundaries

```text
IMPLEMENTATION:
NOT STARTED

KERNEL CHANGES:
0

REV8 CHANGES:
0

NEW SYSCALLS:
0

NEW ABI:
0
```

---

## Architectural Verdict

```text
🟢 ARCHITECTURE READY
```
