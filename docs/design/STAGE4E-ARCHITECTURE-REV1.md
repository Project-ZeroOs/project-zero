# Stage 4E Architecture Specification: Agent Runtime Subsystem (Rev 1)

## 1. Scope

This document establishes the architecture for the **ZeroOS Agent Runtime Subsystem (Stage 4E)**. 

The objective of Stage 4E is to define a first-class, OS-managed **Agent** abstraction that operates strictly above the frozen Stage 3A–3N kernel nucleus, Stage 4A system service baseline, Stage 4B Resource Graph (`resourced`), Stage 4C Workload Orchestration (`workloadd`), and Stage 4D Workspace/Context subsystem (`workspaced`).

### Core Design Goal
In ZeroOS, an Agent is **not** a generic AI framework, a chatbot loop, an LLM wrapper SDK, or a second operating-system process scheduler. 

An Agent is a **persistent, capability-bounded, event-driven computational entity** that formulates high-level goals into deterministic Workload DAGs executed by `workloadd`, operating strictly within a single `Workspace` security and context boundary managed by `workspaced`.

```text
+-----------------------------------------------------------------------+
|                       Stage 4F: Intent & Compute Fabric               |
+-----------------------------------------------------------------------+
|                       Stage 4E: Agent Runtime (agentd)               |
+-----------------------------------------------------------------------+
|                    Stage 4D: Workspace & Persistent Context           |
+-----------------------------------------------------------------------+
|                    Stage 4C: Workload Orchestration (workloadd)       |
+-----------------------------------------------------------------------+
|                    Stage 4B: Resource Graph & Accounting (resourced)  |
+-----------------------------------------------------------------------+
|                    Stage 4A: System Services (brokerd, init, libzero) |
+-----------------------------------------------------------------------+
|                    Stage 3A–3N: Kernel Nucleus & Capabilities         |
+-----------------------------------------------------------------------+
```

---

## 2. Frozen Dependencies & Audit

Stage 4E builds upon the frozen specifications and implementations of prior stages. No frozen Stage 3 or 4A–4D interface, struct, syscall, or daemon binary will be modified during Stage 4E.

| Subsystem / Dependency | Repository Reference | Status / Classification | Reused Primitive / Interface |
|---|---|---|---|
| **Stage 3A–3N Kernel** | `kernel/src/` | `EXISTS` | Capability invocation (`sys_cap_call`), capability derivation (`sys_cap_derive`), revocation (`sys_cap_revoke`), Ring 3 isolation, thread scheduling. |
| **Stage 4A System Services** | `brokerd`, `init`, `libzero` | `EXISTS` | IPC message passing, service registration, ring3 daemon initialization. |
| **Stage 4B Resource Graph** | `resourced`, `ADR-0025` | `EXISTS` | `DistributedIdAllocator` (for persistent IDs), `ResourceLease` allocation, Local Node Accounting. |
| **Stage 4C Workload Orchestration**| `workloadd`, `ADR-0026` | `EXISTS` | `OP_WORKLOAD_CREATE`, `OP_WORKLOAD_CANCEL`, `OP_WORKLOAD_QUERY`, Task DAG execution, Recovery Classes RC-1 to RC-5. |
| **Stage 4D Workspace & Context** | `workspaced`, `ADR-0027` | `EXISTS` | `WorkspaceId`, `workspace.meta`, `context.graph` persistence, Context Graph queries/mutations, $C_{ws}$ handle. |
| **Agent Runtime Daemon (`agentd`)**| `docs/design/STAGE4E...` | `MISSING — 4E MUST DEFINE` | Agent Control Blocks, Agent lifecycle state machine, event subscription router, trigger engine, capability envelope attenuation. |
| **Agent Client Adapters (`libzero`)**| `libzero/src/agent.rs` | `MISSING — FUTURE 4F` | User-space client library wrapper for `agentd` IPC protocol. |
| **Intent Resolver & Compute Fabric**| `docs/design/STAGE4F...` | `MISSING — FUTURE 4F` | Natural language intent translation, cross-node fabric placement. |

### Contradiction Audit
- **Zero Process Scheduler Duplication**: `agentd` does not spawn OS processes or threads directly. It requests `workloadd` to execute `Workloads`.
- **Zero Resource Accounting Duplication**: `agentd` does not track CPU/RAM/NPU bytes. Resource demands pass through `workloadd` to `resourced`.
- **Zero Capability Authority Duplication**: `agentd` holds an attenuated Workspace handle ($C_{agent} \subseteq C_{ws}$). Capability enforcement remains in the Stage 3 kernel.

---

## 3. Agent Semantic Definition

An Agent in ZeroOS is formally defined as a tuple:

$$\text{Agent} = \langle \text{AgentId}, \text{WorkspaceId}, \text{PrincipalId}, \text{AgentState}, \mathbf{C}_{\text{agent}}, \text{GoalPolicy}, \text{SubSet}, \text{TrigTable}, \text{WorkloadSet}, \text{Lineage} \rangle$$

### Authoritative Attribute Breakdown

```text
+-----------------------------------------------------------------------------------+
|                                  AGENT ENTITY                                     |
+-----------------------------------------------------------------------------------+
|  AgentId         : DistributedId (u64, durable, allocated via 4B allocator)     |
|  WorkspaceId     : DistributedId (Bound strictly to 1 Workspace)                  |
|  PrincipalId     : DistributedId (Owner / human user identity)                    |
|  AgentState      : State Enum (Creating, Active, Waiting, Running, Suspended, etc)|
|  C_agent         : Capability Handle (Attenuated subset of C_ws)                  |
|  GoalPolicy      : Goal definition, decision policy, and tool constraints          |
|  SubSet          : Event subscription mask & filter criteria                       |
|  TrigTable       : Registered active trigger conditions                           |
|  WorkloadSet     : Active & historical WorkloadId references                      |
|  Lineage         : Parent AgentId (0 if root Agent) & delegation depth            |
+-----------------------------------------------------------------------------------+
```

### Attribute Authority Matrix

| Attribute | Authoritative Owner | Non-Authoritative Observers |
|---|---|---|
| `AgentId`, `AgentState`, `GoalPolicy`, `Lineage` | `agentd` | `workspaced`, `workloadd` |
| `WorkspaceId`, Context Graph Node | `workspaced` | `agentd` |
| `WorkloadSet` execution status | `workloadd` | `agentd` |
| Hardware allocations / Leases | `resourced` | `workloadd`, `agentd` |
| Capability Validity / Revocation | Stage 3 Kernel | `agentd`, `workloadd` |
| Model Inference Context | User-space Model Service | `agentd` |

---

## 4. Agent vs. Workload

ZeroOS strictly enforces the operational hierarchy:

$$\text{Workspace} \longrightarrow \text{Agent} \longrightarrow \text{Workload(s)} \longrightarrow \text{Task(s)} \longrightarrow \text{Process(es)}$$

```text
               +-------------------+
               |     Workspace     |
               +---------+---------+
                         |
                         v
               +-------------------+
               |       Agent       |
               +----+---------+----+
                    |         |
          +---------+         +---------+
          v                             v
+-------------------+         +-------------------+
|    Workload A     |         |    Workload B     |
+---------+---------+         +---------+---------+
          |                             |
     +----+----+                   +----+----+
     v         v                   v         v
+-----+   +-----+             +-----+   +-----+
|Task1|   |Task2|             |Task3|   |Task4|
+-----+   +-----+             +-----+   +-----+
```

### Precise Relationship Answers

1. **Can one Agent own multiple Workloads?**  
   **Yes.** An Agent formulates high-level goals over time into a sequence or DAG of distinct Workloads executed via `workloadd`.
2. **Can one Workload be created by an Agent?**  
   **Yes.** Agent requests `workloadd` to create Workloads using IPC (`OP_WORKLOAD_CREATE`) passing its attenuated capability $C_{agent}$.
3. **Can a Workload exist without an Agent?**  
   **Yes.** System initialization Workloads, maintenance scripts, and direct user shell tasks exist independently of any Agent.
4. **Can an Agent have no active Workload?**  
   **Yes.** An Agent in `Active` (idle) or `Waiting` (for event/trigger) state has zero active Workloads.
5. **Can an Agent observe a Workload without owning it?**  
   **No**, unless the Agent holds an explicit observation capability granted via Workspace Context authorization.
6. **Can a Workload outlive an Agent?**  
   **No**, by default. If an Agent is deleted or terminates, `agentd` automatically issues `OP_WORKLOAD_CANCEL` for all active owned Workloads. (Explicitly detached background system workloads must be transferred to system ownership prior to Agent deletion).
7. **What happens to running Workloads when an Agent terminates?**  
   `agentd` cascades cancellation through `workloadd`, which cleans up Task DAGs and releases `resourced` leases.
8. **Can an Agent delegate Workload creation?**  
   **Yes.** An Agent can spawn a Child Agent with an attenuated capability envelope, delegating sub-goal execution.
9. **Who actually owns execution lifecycle?**  
   **`workloadd` remains the sole execution manager.** `agentd` does not schedule processes, allocate thread stacks, or manage task queues.

---

## 5. Agent vs. Workspace

Following Stage 4D architecture (`STAGE4D-ARCHITECTURE-REV3.md`), the Workspace provides the persistent containment boundary and Context Graph.

### Precise Architectural Boundaries

```text
+-------------------------------------------------------------------------+
|                              WORKSPACE                                  |
|                                                                         |
|  +-----------------------+               +--------------------------+  |
|  |     Context Graph     |               |    Agent Control Block   |  |
|  | (workspace.meta /      |<------------->|        (agentd)          |  |
|  |  context.graph)       |  Node & Edge  | C_agent ⊆ C_ws           |  |
|  +-----------------------+               +-------------+------------+  |
|                                                        |                |
|                                                        v                |
|                                          +--------------------------+  |
|                                          |    Owned Workload Set    |  |
|                                          +--------------------------+  |
+-------------------------------------------------------------------------+
```

1. **Can an Agent belong to exactly one Workspace?**  
   **Yes.** An Agent is bound to a single `WorkspaceId` at creation.
2. **Can an Agent move between Workspaces?**  
   **No.** Workspace migration is strictly prohibited to prevent cross-workspace capability leaks and context graph pollution.
3. **Can an Agent participate in multiple Workspaces?**  
   **No.** Cross-Workspace collaboration occurs exclusively via explicit inter-workspace IPC capability exchange or 4F Intent Resolver mediation.
4. **Does Workspace deletion terminate Agents?**  
   **Yes.** Deleting a Workspace triggers `workspaced` to request `agentd` to terminate all contained Agents, cascading to `workloadd` Workload cancellation and `resourced` lease quarantine.
5. **Does Agent deletion affect Workspace state?**  
   **No.** Workspace `workspace.meta` remains intact. The Agent's node in `context.graph` is updated to `Terminated` status or removed.
6. **Does an Agent inherit Workspace capabilities?**  
   **No ambient inheritance.** An Agent receives a strictly derived, attenuated capability envelope $C_{agent} \subseteq C_{ws}$.
7. **Can an Agent escape its Workspace capability envelope?**  
   **No.** System capabilities are enforced by the Stage 3 kernel (`sys_cap_validate`). Any attempt to access resources outside $C_{agent}$ yields `ERR_CAPABILITY_DENIED`.
8. **Can Agents communicate across Workspaces?**  
   **No direct channel.** Inter-Agent communication is restricted to Agents within the same Workspace unless an explicit capability bridge channel is passed by a privileged authority.

---

## 6. Agent Identity

Agent identity reuses the frozen Stage 4B durable `DistributedIdAllocator`.

```text
+-----------------------------------------------------------------------+
|                             AgentId                                   |
|  [ 16-bit NodeId ] [ 16-bit SequenceId ] [ 32-bit MonotonicCounter ] |
|                             (64-bit u64)                              |
+-----------------------------------------------------------------------+
```

### Identity Rules
1. **Strongly Typed Wrapper**: `AgentId(u64)` wraps `DistributedId`.
2. **Allocation**: Issued strictly via `resourced` IPC (`OP_ALLOCATE_DISTRIBUTED_ID`).
3. **Non-Reuse**: `AgentId` values are monotonically increasing and never recycled.
4. **Crash Behavior**: `AgentId` is persisted in `/ws/<ws_id>/agents/<agent_id>/agent.meta` on ZeroFS. On reboot, `agentd` restores `AgentId` without re-allocation.
5. **Deletion**: Deleting an Agent tombstones the `AgentId` in ZeroFS. The ID remains permanently retired.

---

## 7. Agent Lifecycle

The Agent lifecycle is managed exclusively by `agentd`.

```text
                       +--------------+
                       |   Creating   |
                       +------+-------+
                              |
                              v
                       +--------------+
            +--------->|    Active    |<----------+
            |          +------+-------+           |
            |                 |                   |
            |                 v                   |
            |          +--------------+           |
            |          |   Waiting    |           |
            |          +------+-------+           |
            |                 | (Event/Trigger)   |
            |                 v                   |
            |          +--------------+           |
            +----------|   Running    |-----------+
                       +------+-------+ (Task done)
                              |
                              | (High-Impact Action Required)
                              v
                       +--------------+
                       |  Suspended   | (Awaiting Human Auth)
                       +------+-------+
                              | (Approved)
                              v
                       +--------------+
                       |  Recovering  | (On Crash/Fault)
                       +------+-------+
                              |
                              v
                       +--------------+
                       |   Stopping   |
                       +------+-------+
                              |
                              v
                       +--------------+
                       |  Terminated  |
                       +------+-------+
                              |
                              v
                       +--------------+
                       |  Reclaimed   |
                       +--------------+
```

### State Transitions & Ownership

| State | Entry Condition | Valid Transitions | Authoritative Owner | Persistent State | Cleanup Responsibility |
|---|---|---|---|---|---|
| **Creating** | `OP_AGENT_CREATE` invoked | $\rightarrow$ `Active`, `Terminated` | `agentd` | Memory only | `agentd` |
| **Active** | Initialization complete; idle | $\rightarrow$ `Waiting`, `Running`, `Stopping` | `agentd` | `agent.meta` (ZeroFS) | `agentd` |
| **Waiting** | Subscribed to event/trigger | $\rightarrow$ `Running`, `Stopping` | `agentd` | `agent.meta` (ZeroFS) | `agentd` |
| **Running** | Formulating goal / Workload active | $\rightarrow$ `Active`, `Suspended`, `Stopping` | `agentd` | `agent.meta` (ZeroFS) | `agentd` + `workloadd` |
| **Suspended** | Human authorization required | $\rightarrow$ `Running`, `Stopping` | `agentd` | `agent.meta` (ZeroFS) | `agentd` |
| **Recovering** | Daemon / model crash detected | $\rightarrow$ `Active`, `Terminated` | `agentd` | `agent.meta` (ZeroFS) | `agentd` |
| **Stopping** | `OP_AGENT_DESTROY` or WS teardown | $\rightarrow$ `Terminated` | `agentd` | `agent.meta` (ZeroFS) | `agentd` + `workloadd` |
| **Terminated**| Workloads cancelled; caps revoked | $\rightarrow$ `Reclaimed` | `agentd` | `agent.meta` (ZeroFS) | `agentd` |
| **Reclaimed** | ZeroFS storage unlinked | None (Terminal) | `agentd` | Tombstoned | `workspaced` |

### Distinction of Lifecycle Levels
- **Agent Lifecycle**: Manages goal formulation, triggers, event loops, and capability envelopes (`agentd`).
- **Workload Lifecycle**: Manages Task DAG execution, state dependencies, and retries (`workloadd`).
- **Process Lifecycle**: Manages Ring 3 address spaces, page tables, and thread context (`Stage 3 Kernel`).

---

## 8. Agent Execution Model

An Agent performs computational work by transforming high-level goals into ZeroOS execution primitives.

```text
[ Agent Decision / Planning Loop ]
               |
               v (IPC: OP_WORKLOAD_CREATE)
      [ workloadd Daemon ]
               |
               v (Spawns Task DAG)
   [ Task DAG Execution Engine ]
               |
               v (Ring 3 Syscalls)
    [ ZeroOS Kernel Processes ]
```

### Absolute Execution Restrictions
An Agent **MUST NOT** directly:
- Access physical memory (CPU/RAM/GPU/NPU addresses).
- Invoke raw hardware IO ports or DMA controllers.
- Modify filesystem inodes directly without ZeroFS IPC handle validation.
- Spawn Ring 3 kernel threads directly.

An Agent **MUST** operate exclusively through OS abstractions:
1. **Capabilities**: Standard Stage 3 kernel capability handles (`sys_cap_call`).
2. **Workloads**: High-level execution units managed by `workloadd`.
3. **Resource Leases**: Hardware accounting leases managed by `resourced`.
4. **Workspace Context**: Semantic context nodes managed by `workspaced`.
5. **IPC Channels**: Service communication routed via `brokerd`.

---

## 9. Agent Cognition is NOT the Kernel

ZeroOS explicitly rejects embedding AI inference engines, model weights, or neural net runtimes inside the operating system kernel or system daemons.

```text
+-----------------------------------------------------------------------+
|                           AGENT RUNTIME                               |
|                            (agentd)                                   |
+----------------------------------+------------------------------------+
                                   |
                                   | Standard IPC Contract
                                   v
+-----------------------------------------------------------------------+
|                       INFERENCE PROVIDER SERVICE                      |
|       (User-space Workload / Service backed by resourced GPU lease)   |
+-----------------------------------------------------------------------+
```

### Intelligence Contract
1. **User-Space Provider**: Model inference (LLM, SLM, custom planner) executes as a standard user-space Workload or external IPC service.
2. **Resource-Backed**: Inference services require a `ResourceLease` issued by `resourced` (covering GPU/NPU compute bounds).
3. **Vendor Agnostic**: `agentd` communicates with inference providers over a standardized stream-oriented IPC channel (`OP_INFERENCE_REQUEST` / `OP_INFERENCE_RESPONSE`). Zero vendor-specific SDKs in `agentd`.

---

## 10. Agent State and Memory Model

To prevent unbounded state growth and context confusion, ZeroOS strictly delineates memory types:

```text
+-----------------------------------------------------------------------------------+
|                               MEMORY CATEGORIES                                   |
+-----------------------------------------------------------------------------------+
| 1. Agent Control State   : Fixed 16 KB AgentControlBlock (PCB-like) in agentd RAM. |
| 2. Workspace Context     : Shared semantic nodes/edges in workspaced context.graph|
| 3. ZeroFS Persistent Log : /ws/<ws_id>/agents/<agent_id>/history.log (bounded)    |
| 4. Workload State        : Ephemeral Task DAG execution status in workloadd RAM.  |
| 5. External Model Context: Ephemeral prompt/KV-cache in Inference Service RAM.    |
| 6. Audit Log             : Immutable, append-only security log on ZeroFS.          |
+-----------------------------------------------------------------------------------+
```

### Memory Allocation Rules
- **No Unbounded Memory**: Ephemeral execution logs are truncated using a fixed ring-buffer policy (`MAX_AGENT_STATE_SIZE = 16 KB`).
- **Context Graph Storage**: Long-term semantic knowledge must be committed to the Workspace Context Graph (`workspaced`) via explicit `C_ws_context` handle calls.

---

## 11. Agent ↔ Workspace Context Graph Integration

Agents interact with the 4D Context Graph using the `C_ws_context` capability handle.

```text
+-----------------------------------------------------------------------+
|                          Workspace Context Graph                      |
|                                                                       |
|  (Node: Agent) ----[OWNS]----> (Node: Workload)                       |
|        |                               |                              |
|   [PRODUCES]                       [MUTATES]                          |
|        v                               v                              |
|  (Node: Artifact) <---[REFERENCES]--- (Node: DataFile)                |
+-----------------------------------------------------------------------+
```

### Allowed Context Operations
1. `OP_CONTEXT_NODE_CREATE`: Agent creates a new semantic entity (e.g., generated artifact reference).
2. `OP_CONTEXT_EDGE_CREATE`: Agent links two entities in the graph.
3. `OP_CONTEXT_QUERY`: Agent queries Workspace context using semantic filters.
4. `OP_CONTEXT_SUBSCRIBE`: Agent registers interest in context node mutations.

### Security Rules
- All Context Graph operations require valid `C_ws_context` handle.
- Agents cannot read or modify context nodes belonging to other Workspaces.
- Private Agent state (internal scratchpad) is stored in `/ws/<ws_id>/agents/<agent_id>/private/` and is isolated from shared Workspace context.

---

## 12. Agent Capabilities & Attenuation

Agent authority strictly adheres to the principle of least privilege and zero ambient authority.

$$\mathbf{C}_{\text{agent}} \subseteq \mathbf{C}_{\text{ws}}$$

```text
              +-----------------------------------+
              | Workspace Capability Root (C_ws)  |
              +-----------------+-----------------+
                                |
                                v (sys_cap_derive)
              +-----------------------------------+
              |  Agent Capability Envelope        |
              |            (C_agent)              |
              +-----------------+-----------------+
                                |
                                v (sys_cap_derive)
              +-----------------------------------+
              | Workload Capability (C_workload)  |
              +-----------------+-----------------+
                                |
                                v (sys_cap_derive)
              +-----------------------------------+
              |    Task Capability (C_task)       |
              +-----------------------------------+
```

### Capability Rules
1. **Derivation**: `agentd` derives $C_{agent}$ from $C_{ws}$ during `OP_AGENT_CREATE` using kernel `sys_cap_derive`.
2. **No Amplification**: $C_{agent}$ can never contain rights absent in $C_{ws}$.
3. **Revocation**: Revoking $C_{ws}$ automatically invalidates all derived $C_{agent}$, $C_{workload}$, and $C_{task}$ handles across the kernel capability tree.
4. **Cleanup**: Agent deletion invokes `sys_cap_revoke(C_agent)`.

---

## 13. Agent Delegation

An Agent may delegate sub-tasks by spawning Child Agents.

```text
+-----------------------------------------------------------------------+
|                         Parent Agent (Depth 0)                        |
|                               C_parent                                |
+-----------------------------------+-----------------------------------+
                                    |
                                    | sys_cap_derive (Attenuated)
                                    v
+-----------------------------------------------------------------------+
|                         Child Agent (Depth 1)                         |
|                         C_child ⊆ C_parent                            |
+-----------------------------------------------------------------------+
```

### Delegation Rules
1. **Attenuated Authority**: $C_{\text{child}} \subseteq C_{\text{parent}} \subseteq C_{\text{ws}}$.
2. **Lineage Tracking**: Each Child Agent records `ParentAgentId` and `DelegationDepth`.
3. **Depth Limit**: Maximum delegation depth is strictly bounded (`MAX_DELEGATION_DEPTH = 4`).
4. **Cascading Termination**: Terminating a Parent Agent automatically terminates all descendant Child Agents recursively.

---

## 14. Agent Event / Perception Model

Agents perceive system state changes via an asynchronous event perception loop.

```text
[ System Event Source ] (ZeroFS / workloadd / workspaced / Timers)
          |
          v
[ agentd Event Dispatcher ] ---> [ Filter & Capability Check ]
                                             |
                                             v (Matches Trigger)
                                  [ Wake Agent Event Loop ]
```

### Event Perception Rules
1. **No Global Subscriptions**: Agents cannot register wildcard global subscriptions. All subscriptions must specify explicit `WorkspaceId` and event type filters.
2. **Event Delivery**: Events are queued in a ring buffer (`MAX_EVENT_SUBSCRIPTIONS = 32`).
3. **Loss Semantics**: If an Agent event buffer overflows, older non-critical events are dropped, and an `EVENT_OVERFLOW` marker is inserted.
4. **Ordering**: Events within a single Workspace are delivered strictly in monotonically increasing sequence order.

---

## 15. Agent Triggers

A Trigger defines an automated wake condition for an Agent:

$$\text{Trigger} = \langle \text{TriggerId}, \text{EventFilter}, \text{ConditionExpression}, \text{TargetAgentId}, \mathbf{C}_{\text{trigger}} \rangle$$

```text
Event Received ---> Match EventFilter ---> Evaluate Condition ---> Wake Target Agent
```

### Trigger Properties
- **Persistence**: Triggers are persisted in `/ws/<ws_id>/agents/<agent_id>/triggers.dat`.
- **Capability Protected**: Creating a trigger requires `C_agent` with event subscription rights.
- **Deterministic Evaluation**: Trigger conditions are evaluated using a deterministic byte-code match engine (no non-deterministic LLM logic in the event loop).

---

## 16. Agent ↔ Workload Orchestration Protocol

`agentd` communicates with `workloadd` using standard IPC opcodes:

```text
+-------------------+                          +-------------------+
|      agentd       |                          |     workloadd     |
+---------+---------+                          +---------+---------+
          |                                              |
          |--- OP_WORKLOAD_CREATE (TaskDAG, C_agent) --->|
          |<-- OK (WorkloadId) --------------------------|
          |                                              |
          |--- OP_WORKLOAD_QUERY (WorkloadId) ---------->|
          |<-- OK (Status: Running/Completed) -----------|
          |                                              |
          |--- OP_WORKLOAD_CANCEL (WorkloadId) --------->|
          |<-- OK (Cancelled) ---------------------------|
```

### Protocol Guarantees
- `workloadd` executes Workloads under the attenuated $C_{agent}$ passed in `OP_WORKLOAD_CREATE`.
- `workloadd` enforces all Stage 4C Recovery Classes (RC-1 to RC-5). `agentd` observes completion or failure status without overriding `workloadd` recovery handlers.

---

## 17. Agent ↔ Resource Graph Boundary

Agents access hardware resources strictly through Workload demand specifications.

$$\text{Agent} \longrightarrow \text{agentd} \longrightarrow \text{workloadd} \longrightarrow \text{resourced} \longrightarrow \text{Hardware Lease}$$

```text
+-------------------+     Workload Request     +-------------------+
|      agentd       |------------------------->|     workloadd     |
+-------------------+                          +---------+---------+
                                                         |
                                                         | Lease Request
                                                         v
                                               +-------------------+
                                               |     resourced     |
                                               +-------------------+
```

### Constraints
- Direct IPC between `agentd` and `resourced` for lease acquisition is prohibited.
- Resource demands (CPU cores, RAM MB, GPU compute) are embedded inside the `WorkloadSpec` sent to `workloadd`.

---

## 18. Agent Sandbox & Security Boundary

The Agent security boundary protects ZeroOS from untrusted, buggy, or compromised Agent logic.

```text
+-----------------------------------------------------------------------+
|                            SECURITY THREATS                           |
|                                                                       |
| 1. Prompt Injection          : Malicious external data in model context|
| 2. Compromised Inference     : Rogue output from inference provider    |
| 3. Capability Escalation     : Agent attempting to access root C_ws   |
| 4. Resource Exhaustion       : Agent spawning infinite Workloads       |
| 5. Cross-Workspace Leakage   : Agent accessing another Workspace       |
+-----------------------------------------------------------------------+
```

### Security Enforcement Controls
1. **Ring 3 Execution**: `agentd` and all Agent user-space helper code run in Ring 3 user space under standard process isolation.
2. **Kernel Capability Verification**: Every IPC request to `workloadd`, `workspaced`, or `brokerd` requires valid kernel handle verification via `sys_cap_validate`.
3. **No Ambient Privileges**: An Agent process possesses zero root or kernel-mode privileges.
4. **Sandboxed Tool Execution**: Any tool or script invoked by an Agent executes inside a isolated Ring 3 Task spawned by `workloadd`.

---

## 19. Human Authorization & High-Impact Boundary

Certain high-impact operations require mandatory out-of-band human authorization (Two-Man Rule).

```text
[ Agent Requests High-Impact Action ] (e.g., Key Deletion, External Mutation)
                  |
                  v
       [ agentd Policy Check ]
                  |
                  v (Action Marked High-Impact)
  [ Agent State -> Suspended (Awaiting Auth) ]
                  |
                  v (Emits Auth Request via IPC)
    [ Human / Admin Control Path ]
                  |
                  v (Approved via C_human_auth Handle)
  [ Agent State -> Running (Action Dispatched) ]
```

### High-Impact Actions List
- Workspace destruction or bulk context purge.
- Modification of persistent security credentials or capability delegates.
- Allocation of hardware resource leases exceeding default node thresholds.
- Unrestricted external network mutations (where network cap is restricted).

### Security Invariants
- An Agent **CANNOT** self-approve high-impact actions.
- Human authorization must be delivered over a kernel-protected IPC channel holding the `C_human_auth` capability.

---

## 20. Agent Failure & Recovery

Stage 4E defines deterministic failure recovery patterns:

```text
+-----------------------+-------------------------------------------------------------+
| Failure Scenario      | Recovery Procedure                                          |
+-----------------------+-------------------------------------------------------------+
| agentd Daemon Crash   | init restarts agentd. agentd reads /ws/*/agents/*.meta from |
|                       | ZeroFS, reconciles active Workload status with workloadd.   |
|                       |                                                             |
| Inference Provider    | agentd detects IPC broken pipe. Agent state moves to        |
| Crash                 | Recovering. agentd requests workloadd to restart service.   |
|                       |                                                             |
| Workspace Deletion    | workspaced notifies agentd. agentd issues OP_WORKLOAD_CANCEL|
|                       | for all owned workloads, revokes C_agent, unlinks storage.  |
|                       |                                                             |
| workloadd Restart     | agentd re-queries active WorkloadIds via OP_WORKLOAD_QUERY  |
|                       | upon workloadd reconnection.                                |
+-----------------------+-------------------------------------------------------------+
```

---

## 21. Agent Persistence

Agent state persistence is anchored to ZeroFS within the owning Workspace folder structure:

```text
/ws/<ws_id>/
  └── agents/
      └── <agent_id>/
          ├── agent.meta      (Authoritative Agent Control Block metadata)
          ├── triggers.dat    (Registered trigger tables)
          ├── subscriptions.dat (Active event subscription masks)
          ├── history.log     (Bounded ring-buffer action audit log)
          └── private/        (Agent-private persistent state)
```

### Persistence Transaction Protocol
1. `agentd` writes state updates to `agent.meta.tmp`.
2. `agentd` issues ZeroFS `OP_FSYNC`.
3. `agentd` atomically renames `agent.meta.tmp` $\rightarrow$ `agent.meta`.

---

## 22. Agent Runtime Service Boundary (`agentd`)

`agentd` is implemented as a Stage 4 system daemon.

### Explicit Responsibilities
- Managing `AgentControlBlock` records and lifecycle transitions.
- Allocating `AgentId` via `resourced` IPC.
- Deriving $C_{agent}$ from $C_{ws}$.
- Parsing goals and issuing `OP_WORKLOAD_CREATE` to `workloadd`.
- Routing event subscriptions and evaluating trigger conditions.
- Enforcing human authorization suspension states.

### Explicit Exclusions (What `agentd` Does NOT Do)
- **NO** process scheduling or thread creation (owned by Kernel / `workloadd`).
- **NO** hardware resource accounting or GPU leasing (owned by `resourced`).
- **NO** Context Graph storage or indexing (owned by `workspaced`).
- **NO** raw filesystem inode management (owned by ZeroFS).
- **NO** natural language intent parsing (owned by Stage 4F Intent Resolver).

---

## 23. Agent ↔ Intent Boundary (Future 4F Interface)

Stage 4E defines the contract boundary for the future Stage 4F Intent Resolver:

```text
[ Human / External System ]
            |
            v (Natural Language / High-Level Command)
[ Stage 4F Intent Resolver ]
            |
            v (Formal GoalSpec & Capability Handle)
[ Stage 4E agentd (OP_AGENT_DISPATCH_GOAL) ]
```

- `agentd` accepts pre-structured `GoalSpec` objects over IPC.
- `agentd` contains no natural language parsers or LLM prompt tokenizers.

---

## 24. Agent ↔ Compute Fabric Boundary (Future 4F Interface)

Stage 4E exposes execution demands to the future Stage 4F Compute Fabric:

```text
[ agentd ] ---> WorkloadSpec (demands: { locality, compute, memory }) ---> [ workloadd ]
                                                                               |
                                                                               v
                                                                   [ Stage 4F Compute Fabric ]
```

- Local vs. remote fabric placement is determined transparently by `workloadd` and Stage 4F. `agentd` only specifies capability requirements and execution constraints.

---

## 25. Static Bounds

To guarantee deterministic system execution and prevent RAM exhaustion, Stage 4E establishes strict static architectural limits:

```text
+-------------------------------+-------------------+---------------------------------------+
| Constant                      | Value             | Justification / Driving Resource      |
+-------------------------------+-------------------+---------------------------------------+
| MAX_AGENTS                    | 64 per Node       | Control block memory (64 * 16KB = 1MB)|
| MAX_AGENT_WORKLOADS           | 16 per Agent      | Bounded active Workload tracking table|
| MAX_EVENT_SUBSCRIPTIONS       | 32 per Agent      | Event dispatcher ring buffer size     |
| MAX_TRIGGERS                  | 16 per Agent      | Trigger table matching array          |
| MAX_DELEGATION_DEPTH          | 4 Levels          | Prevention of recursive agent recursion|
| MAX_AGENT_STATE_SIZE          | 16 KB             | In-memory AgentControlBlock size       |
| MAX_IPC_AGENT_MSG_SIZE        | 2048 Bytes        | Fixed IPC buffer alignment            |
+-------------------------------+-------------------+---------------------------------------+
```

### Static Memory Footprint Calculation
$$\text{Total Memory} = 64 \times 16\text{ KB} = 1024\text{ KB } (1\text{ MB RAM total for } agentd \text{ control blocks})$$

---

## 26. IPC / API Boundary Protocol

`agentd` registers service handle `agentd.srv` with `brokerd`.

### Message Format (2048 Bytes Fixed)

```text
+-------------------+-------------------+-------------------+-----------------------+
| Opcode (u32)      | RequestId (u64)   | AgentId (u64)     | WorkspaceId (u64)     |
+-------------------+-------------------+-------------------+-----------------------+
| Payload (1968 Bytes ...)                                                          |
+-----------------------------------------------------------------------------------+
```

### Opcode Summary

| Opcode | Name | Capability Required | Description |
|---|---|---|---|
| `0x0501` | `OP_AGENT_CREATE` | $C_{ws}$ | Spawns a new Agent within specified Workspace. |
| `0x0502` | `OP_AGENT_DESTROY` | $C_{agent}$ | Terminates Agent and cascades Workload cancellation.|
| `0x0503` | `OP_AGENT_GET_STATE` | $C_{agent}$ | Returns current Agent lifecycle state and metrics. |
| `0x0504` | `OP_AGENT_DISPATCH_GOAL`| $C_{agent}$ | Issues a new goal to an Active Agent. |
| `0x0505` | `OP_AGENT_REGISTER_TRIGGER`| $C_{agent}$ | Registers a trigger condition for the Agent. |
| `0x0506` | `OP_AGENT_SUBSCRIBE_EVENT`| $C_{agent}$ | Subscribes Agent to specific Workspace events. |
| `0x0507` | `OP_AGENT_DELEGATE` | $C_{agent}$ | Spawns a Child Agent with derived capabilities. |
| `0x0508` | `OP_AGENT_HUMAN_APPROVE`| $C_{human\_auth}$ | Clears `Suspended` state for high-impact action. |

---

## 27. Security Invariants

Stage 4E enforces 11 mandatory architectural invariants:

1. **`I-AGENT-ID-UNIQUE`**: Every Agent has a globally unique 64-bit `DistributedId` allocated via `resourced` that is never reused.
2. **`I-AGENT-NO-CAP-AMPLIFICATION`**: An Agent's capability envelope is strictly derived from its owning Workspace: $C_{agent} \subseteq C_{ws}$.
3. **`I-AGENT-WORKSPACE-CONTAINMENT`**: An Agent is strictly bound to a single Workspace and cannot access resources or context outside its assigned `WorkspaceId`.
4. **`I-AGENT-WORKLOAD-BOUNDARY`**: An Agent does not schedule processes or execute tasks directly; all execution requests pass to `workloadd` via `OP_WORKLOAD_CREATE`.
5. **`I-AGENT-NO-RESOURCE-BYPASS`**: An Agent cannot acquire hardware leases directly from `resourced`; all demands are declared in Workload specs.
6. **`I-AGENT-NO-AMBIENT-AUTHORITY`**: All `agentd` IPC operations require explicit kernel capability handles passed in the invocation header.
7. **`I-AGENT-DELEGATION-ATTENUATION`**: Child Agents receive strictly attenuated capability envelopes derived from the parent: $C_{child} \subseteq C_{parent}$.
8. **`I-AGENT-FAILURE-CONTAINMENT`**: An Agent crash or cancellation cannot corrupt `agentd`, `workloadd`, `workspaced`, or the kernel nucleus.
9. **`I-AGENT-PERSISTENCE-CONSISTENCY`**: Persistent Agent metadata updates on ZeroFS are atomic via write-fsync-rename file operations.
10. **`I-AGENT-EVENT-AUTHENTICITY`**: Event perception routing validates event source signatures; fake or cross-workspace event injection is rejected.
11. **`I-AGENT-HUMAN-AUTHORIZATION`**: High-impact actions transition the Agent to `Suspended` state until cleared by an out-of-band IPC message bearing $C_{human\_auth}$.

---

## 28. Dependency Graph

```text
+-----------------------------------------------------------------------+
|                    Stage 3 Kernel Nucleus                             |
|          (sys_cap_call, sys_cap_derive, Ring 3 isolation)             |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                    Stage 4A System Services                           |
|                      (brokerd IPC router, init)                       |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                  Stage 4B Resource Graph                              |
|           (resourced, DistributedIdAllocator, Node Leases)            |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                Stage 4C Workload Orchestration                        |
|             (workloadd, Task DAG Engine, RC-1..RC-5)                  |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|              Stage 4D Workspace & Persistent Context                  |
|          (workspaced, workspace.meta, context.graph, C_ws)            |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                     Stage 4E Agent Runtime                            |
|        (agentd, AgentControlBlocks, Event Perception Loop)            |
+-----------------------------------+-----------------------------------+
                                    |
                                    v
+-----------------------------------------------------------------------+
|                 Stage 4F Intent & Compute Fabric                      |
|                (Intent Resolver, Fabric Placement)                    |
+-----------------------------------------------------------------------+
```

---

## 29. Non-Goals

The following are explicitly outside the scope of Stage 4E:
- Implementing LLM prompt templates, tokenizers, or neural network inference frameworks.
- Building a chatbot UI, terminal assistant, or web interface.
- Creating a second process scheduler or thread manager outside `workloadd`.
- Modifying any frozen Stage 3 or Stage 4A–4D binary, header, or source file.
- Implementing cross-node distributed compute fabric routing (deferred to Stage 4F).

---

## 30. Open Questions for Future Design Phases

1. **Inference Provider Standard Protocol**: Should `agentd` define a raw byte stream interface for inference providers, or rely on a standardized binary payload schema over ZeroOS IPC channels?
2. **Context Summarization Triggers**: Should context graph summarization be driven automatically by `workspaced` when nodes exceed threshold bounds, or requested explicitly by `agentd` via Workload tasks?
3. **Fine-Grained Event Filtering**: How should complex trigger boolean predicate expressions be compiled to run efficiently inside `agentd`'s fixed 16 KB memory footprint without dynamic memory allocation?

---

```text
STATUS: DRAFT — REQUIRES REVIEW
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
STAGE 4D MODIFICATIONS: NONE
```
