# Stage 4E Architecture Specification: Agent Runtime Subsystem (Rev 2)

## 1. Scope

This document establishes the revised architecture for the **ZeroOS Agent Runtime Subsystem (Stage 4E)**. 

The objective of Stage 4E is to define a first-class, OS-managed **Agent** control entity that operates strictly above the frozen Stage 3A–3N kernel nucleus, Stage 4A system service baseline, Stage 4B Resource Graph (`resourced`), Stage 4C Workload Orchestration (`workloadd`), and Stage 4D Workspace subsystem (`workspaced`).

### Core Design Goal
In ZeroOS, an Agent is **not** a generic AI framework, a chatbot loop, an LLM wrapper SDK, or a second operating-system process scheduler. 

An Agent is a **persistent, capability-bounded, event-driven control entity** that formulates high-level goals into deterministic Workload DAGs executed by `workloadd`, operating strictly within a single `Workspace` persistent context and organizational boundary managed by `workspaced`.

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

## 2. Frozen Dependencies & Subsystem Audit

Stage 4E builds upon the frozen specifications and implementations of prior stages. No frozen Stage 3 or Stage 4A–4D interface, struct, syscall, or daemon binary is modified during Stage 4E.

| Subsystem / Dependency | Repository Reference | Status / Classification | Reused Primitive / Interface |
|---|---|---|---|
| **Stage 3A–3N Kernel** | `kernel/src/` | `EXISTS` | Kernel capability table, `sys_cap_call`, `sys_cap_derive`, `sys_cap_revoke`, `sys_cap_validate`, Ring 3 thread scheduling. |
| **Stage 4A System Services** | `brokerd`, `init`, `libzero` | `EXISTS` | IPC message passing, service registration, ring3 daemon initialization. |
| **Stage 4B Resource Graph** | `resourced`, `ADR-0025` | `EXISTS` | `DistributedIdAllocator` (for persistent IDs), `ResourceLease` allocation, Local Node Accounting. |
| **Stage 4C Workload Orchestration**| `workloadd`, `ADR-0026` | `EXISTS` | `OP_WORKLOAD_CREATE`, `OP_WORKLOAD_CANCEL`, `OP_WORKLOAD_QUERY`, Task DAG execution, Recovery Classes RC-1 to RC-5. |
| **Stage 4D Workspace & Context** | `workspaced`, `ADR-0027` | `EXISTS` | `WorkspaceId`, `workspace.meta`, `context.graph` persistence, Context Graph queries/mutations, $C_{ws}$ capability handle root. |
| **Agent Runtime Daemon (`agentd`)**| `docs/design/STAGE4E...` | `MISSING — 4E MUST DEFINE` | Agent Control Blocks, Agent lifecycle state machine, event subscription router, trigger engine, capability envelope derivation requests. |
| **`libzero` Agent IPC Protocol**| `libzero/src/agent.rs` | `MISSING — 4E MUST DEFINE` | Low-level C/Rust IPC binary protocol headers and client handle wrappers for `agentd`. |
| **Intent Resolver & Compute Fabric**| `docs/design/STAGE4F...` | `MISSING — FUTURE 4F` | High-level natural language intent translation, model planner integration, cross-node fabric placement. |

---

## 3. Agent Semantic Definition & Authority Matrix

An Agent in ZeroOS is formally defined as a tuple:

$$\text{Agent} = \langle \text{AgentId}, \text{WorkspaceId}, \text{PrincipalId}, \text{LifecycleState}, \text{RuntimeState}, \mathbf{C}_{\text{agent}}, \text{GoalPolicy}, \text{SubSet}, \text{TrigTable}, \text{WorkloadRefs}, \text{Lineage} \rangle$$

### Authoritative Attribute Breakdown

```text
+-----------------------------------------------------------------------------------+
|                                  AGENT ENTITY                                     |
+-----------------------------------------------------------------------------------+
|  AgentId         : DistributedId (u64, durable, allocated via 4B allocator)     |
|  WorkspaceId     : DistributedId (Bound strictly to 1 Workspace)                  |
|  PrincipalId     : DistributedId (Owner / human user identity)                    |
|  LifecycleState  : Creating, Active, Stopping, Terminated, Reclaimed              |
|  RuntimeState    : Idle, Waiting, Executing, Suspended, Recovering                |
|  C_agent         : Kernel-derived capability handle (C_agent ⊆ C_ws)             |
|  GoalPolicy      : Goal definition, decision policy, and tool constraints          |
|  SubSet          : Event subscription mask & filter criteria                       |
|  TrigTable       : Registered active trigger conditions                           |
|  WorkloadRefs    : Active & historical WorkloadId references                      |
|  Lineage         : Parent AgentId (0 if root Agent) & delegation depth            |
+-----------------------------------------------------------------------------------+
```

### Attribute Authority Matrix

| Attribute | Authoritative Owner | Non-Authoritative Observers |
|---|---|---|
| `AgentId`, `LifecycleState`, `RuntimeState`, `GoalPolicy`, `Lineage` | `agentd` | `workspaced`, `workloadd` |
| `WorkspaceId`, Context Graph Node | `workspaced` | `agentd` |
| Workload Execution Lifecycle & Task DAGs | `workloadd` | `agentd` |
| Hardware Allocations / Resource Leases | `resourced` | `workloadd`, `agentd` |
| Capability Authority, Derivation & Revocation | **Stage 3 Kernel** | `agentd`, `workloadd`, `workspaced` |
| Model Inference Computation | User-space Model Service | `agentd` |

---

## 4. Kernel Capability Authority Boundary ($C_{\text{agent}}$)

`agentd` does **NOT** constitute a capability authority. `agentd` cannot manufacture, widen, or independently revoke capability authority.

### Capability Derivation Architecture
Capability authority is enforced strictly by the **Stage 3H Kernel Capability System**:

```text
               +-----------------------------------+
               | Workspace Root Capability (C_ws)  |
               +-----------------+-----------------+
                                 |
                                 | kernel sys_cap_derive()
                                 v
               +-----------------------------------+
               |  Agent Capability Envelope        |
               |       (C_agent ⊆ C_ws)            |
               +-----------------+-----------------+
                                 |
                                 | kernel sys_cap_derive()
                                 v
               +-----------------------------------+
               | Workload Capability (C_workload)  |
               +-----------------+-----------------+
                                 |
                                 | kernel sys_cap_derive()
                                 v
               +-----------------------------------+
               |    Task Capability (C_task)       |
               +-----------------------------------+
```

### Process-Local Capability Handles & Crash Behavior
- `agentd` holds process-local capability handle indices (`u32`) pointing to entries in `agentd`'s process capability table managed by the kernel.
- **`u32` capability handles are NEVER persisted to ZeroFS**.
- When `agentd` crashes or restarts, its process capability table is destroyed by the kernel.
- Upon recovery, `agentd` re-obtains fresh process-local capability handle indices by requesting the kernel (or `workspaced`) to derive $C_{agent}$ afresh from the persistent Workspace capability root handle ($C_{ws}$).

```text
I-AGENT-KERNEL-AUTHORITY:
All Agent capability attenuation, derivation, validation, and revocation are
ultimately enforced by the Stage 3H kernel capability system. agentd merely holds
process-local capability handles pointing to kernel capability table entries.
```

---

## 5. Untrusted Model Cognition Boundary

AI inference engines, LLM runtimes, and neural network models exist exclusively in user space as resource-backed services.

### Fundamental Security Rule
$$\text{Model Output} \neq \text{Authority}$$

A model may output structured data or proposed actions (e.g., `"delete file X"`, `"spawn workload Y"`), but model output has **zero ambient authority** and cannot directly execute syscalls, acquire resource leases, or bypass security checks.

```text
                    +------------------------------------+
                    |        USER-SPACE MODEL            |
                    | (Inference Service / Model Output) |
                    +-----------------+------------------+
                                      |
                                      | Untrusted Proposal Data
                                      v
                    +------------------------------------+
                    |       AGENT POLICY ENGINE          |
                    |     (agentd Deterministic Policy)  |
                    +-----------------+------------------+
                                      |
                                      | Validated Request + C_agent
                                      v
                    +------------------------------------+
                    |   STAGE 3 KERNEL CAPABILITY CHECK  |
                    |        (sys_cap_validate)          |
                    +-----------------+------------------+
                                      |
                                      | Authorized Operation
                                      v
                    +------------------------------------+
                    |  ZEROOS SUBSYSTEM EXECUTION        |
                    |  (workloadd / workspaced / ZeroFS) |
                    +------------------------------------+
```

### Security Invariants
- `I-AGENT-MODEL-NO-AUTHORITY`: Model output is untrusted data, not security authority.
- `I-AGENT-MODEL-NO-CAPABILITY-AMPLIFICATION`: Model output cannot increase capability rights or widen $C_{agent}$.
- `I-AGENT-MODEL-NO-HUMAN-AUTHORIZATION`: Model output cannot satisfy, grant, or bypass human authorization requirements.

---

## 6. Generic Human Authorization Boundary

Certain high-impact operations (e.g., persistent credential deletion, bulk workspace purge, system-wide configuration mutation) require mandatory out-of-band human authorization before execution.

```text
[ Agent Decision Engine Proposes Action ]
                   |
                   v
[ agentd Evaluates Action Policy ]
                   |
     +-------------+-------------+
     | High-Impact             | Standard Policy
     v                         v
[ RuntimeState -> Suspended ] [ Pass to sys_cap_validate ]
     |                         |
     v                         v
[ Dispatch Auth Request ]    [ Execute via workloadd ]
     |
     v
[ Trusted Out-of-Band Auth Service ]
     |
     v (Human Grants Explicit Action Ticket / Cap)
[ Agent RuntimeState -> Executing ]
     |
     v
[ Execute Operation ]
```

### Authorization Architecture
1. **No Magic Capability**: Stage 4E does not introduce an ad-hoc `$C_{human_auth}$` handle primitive.
2. **Trusted Authorization Path**: Authorization requests are emitted over IPC to a registered system Trusted Authorization Service (e.g., system admin console service).
3. **Explicit Action Clearance**: The authorization response returns an explicit time-bound clearance ticket or specific single-use derived capability handle.
4. **Self-Escalation Prohibition**: An Agent **cannot** approve its own escalation, delegate authorization to itself, or synthesize authorization tokens.

---

## 7. Agent vs. Workspace Boundary

Following frozen Stage 4D architecture (`STAGE4D-ARCHITECTURE-REV3.md`):

$$\text{Workspace} = \text{Persistent context and organizational boundary with an associated capability envelope enforced by Stage 3H.}$$

```text
Workspace ≠ Capability Domain
Workspace ≠ Security Kernel
Workspace ≠ Filesystem Mount
```

### Single-Workspace Membership Rationale
Stage 4E strictly enforces **Single-Workspace Membership**: an Agent belongs to exactly **one** `WorkspaceId` throughout its lifecycle.

- **Rationale**: Permitting an Agent to belong to multiple Workspaces simultaneously creates complex capability cross-contamination risks, ambient authority ambient leakage, and ambiguous Context Graph ownership.
- **Cross-Workspace Interaction**: If two Agents in different Workspaces need to collaborate, they interact exclusively via explicit inter-Workspace IPC capability channels passed by an authorized administrator, or via 4F Intent mediation.

---

## 8. Agent vs. Workload Ownership & Lifetime

ZeroOS strictly enforces the execution hierarchy:

$$\text{Workspace} \longrightarrow \text{Agent} \longrightarrow \text{Workload(s)} \longrightarrow \text{Task(s)} \longrightarrow \text{Process(es)}$$

```text
Agent Role:
  - Requests/Creates Workloads via workloadd IPC
  - Observes Workload status
  - May request Workload cancellation

workloadd Role:
  - OWNS Workload execution lifecycle, Task DAG scheduling, retries, process management
```

### Agent Termination vs. Running Workload Policy
When an Agent terminates (or its lifecycle moves to `Stopping`/`Terminated`), its running Workloads are evaluated against an explicit per-Workload policy declared at creation:

```text
Agent Termination
       |
       v
Evaluate Workload Policy
       ├── CANCEL (Default): agentd issues OP_WORKLOAD_CANCEL to workloadd for active Workload.
       └── DETACH: Workload ownership transfers to Workspace root context (continues execution under workloadd).
```

---

## 9. Separated Agent State Machines

To prevent runtime execution activity from corrupting Agent lifecycle status, Stage 4E separates the state machine into two distinct layers:

```text
1. LIFECYCLE STATE MACHINE (Administrative Existence)

    +----------+      +--------+      +----------+      +------------+      +-----------+
    | Creating | ---->| Active | ---->| Stopping | ---->| Terminated | ---->| Reclaimed |
    +----------+      +---+----+      +----------+      +------------+      +-----------+
                          |
                          | (Runtime execution occurs ONLY while Lifecycle == Active)
                          v

2. RUNTIME EXECUTION STATE MACHINE (Operational Status)

                          +------+
            +------------>| Idle |<------------+
            |             +--+---+             |
            |                |                 |
            |                v                 |
            |            +---+-----+           |
            |            | Waiting |           |
            |            +---+-----+           |
            |                | (Trigger/Event) |
            |                v                 |
            |            +---+-------+         |
            +------------| Executing |---------+ (Workload Complete)
                         +---+-------+
                             |
                             +---> [ Suspended ]   (Awaiting Human Auth)
                             |
                             +---> [ Recovering ]  (Daemon / Provider Fault)
```

---

## 10. Durable vs. Ephemeral State & Recovery

Stage 4E strictly separates persistent state stored on ZeroFS from ephemeral runtime state in `agentd` RAM:

```text
+-----------------------------------------------------------------------------------+
|                               STATE CLASSIFICATION                                |
+-----------------------------------------------------------------------------------+
| DURABLE STATE (Persisted on ZeroFS under /ws/<ws_id>/agents/<agent_id>/)         |
|   - AgentId & WorkspaceId (64-bit DistributedIds)                                |
|   - LifecycleState (Active, Stopping, Terminated)                                |
|   - GoalPolicy & registered trigger rules (triggers.dat)                         |
|   - Persistent Context Graph node references                                     |
|   - Event subscription criteria (subscriptions.dat)                              |
|   - Audit Log (history.log)                                                      |
|                                                                                   |
| EPHEMERAL STATE (RAM only, destroyed on crash/reboot)                            |
|   - Process-local capability handle indices (u32 indices into kernel table)       |
|   - Active IPC channel handles and message buffers                               |
|   - RuntimeState (Idle, Waiting, Executing, Suspended, Recovering)               |
|   - In-flight event perception ring buffer                                       |
|   - Ephemeral model invocation context                                           |
+-----------------------------------------------------------------------------------+
```

### Crash & Reboot Recovery Protocol
1. On startup, `agentd` scans `/ws/<ws_id>/agents/` on ZeroFS.
2. Reads `agent.meta` to restore `AgentId`, `WorkspaceId`, `GoalPolicy`, and `LifecycleState`.
3. Re-obtains a process-local capability handle for $C_{agent}$ by calling kernel `sys_cap_derive` off the Workspace handle root $C_{ws}$.
4. Re-queries `workloadd` via `OP_WORKLOAD_QUERY` for active `WorkloadId` references to reconcile runtime execution state.

---

## 11. Event Perception & Authenticity Model

Agents perceive system state changes via an authenticated event perception loop.

```text
[ System Event Producer ] (workspaced / workloadd / ZeroFS / Kernel Timers)
          |
          v
[ brokerd Verified IPC Message ] (Sender PID & Cap validated by brokerd)
          |
          v
[ agentd Event Dispatcher ]
          |
          +---> Validates Subscription Filter & WorkspaceId Match
          |
          +---> Enqueues in Agent Event Buffer (Ring Buffer: MAX_EVENT_SUBSCRIPTIONS)
          |
          +---> Wakes Agent RuntimeState (Idle/Waiting -> Executing)
```

### Event Authenticity & Security Rules
1. **Producer Authenticity**: Events originate strictly from trusted system daemons (`workspaced`, `workloadd`, `resourced`, `brokerd`) or kernel timer interrupts. `brokerd` validates sender Process IDs on all IPC event messages.
2. **No Direct Inter-Agent Injection**: An Agent cannot inject synthetic events directly into another Agent's event queue. Inter-agent messages must route through `agentd` IPC subject to explicit capability validation.
3. **Suspended State Behavior**: When an Agent is `Suspended` (awaiting human auth), events continue to queue in its ring buffer up to `MAX_EVENT_SUBSCRIPTIONS`. Excess non-critical events drop with an `EVENT_OVERFLOW` flag.

---

## 12. Static Bounds Classification & Derivation

Stage 4E bounds are explicitly classified into **Architectural Hard Bounds** (fixed protocol/layout limits) and **Configuration Policies** (tunable system parameters driven by resource budgets):

```text
+-------------------------------+-------------------+-----------------------+---------------------------------------+
| Constant                      | Value             | Classification        | Justification / Driving Resource      |
+-------------------------------+-------------------+-----------------------+---------------------------------------+
| MAX_IPC_AGENT_MSG_SIZE        | 2048 Bytes        | ARCHITECTURAL HARD    | Fixed IPC ring buffer page alignment  |
| MAX_DELEGATION_DEPTH          | 4 Levels          | ARCHITECTURAL HARD    | Stack depth & cap derivation limit    |
| MAX_AGENT_STATE_SIZE          | 16 KB             | ARCHITECTURAL HARD    | Fixed AgentControlBlock struct layout |
| MAX_AGENTS                    | 64 per Node       | CONFIGURATION POLICY  | System RAM budget (64 * 16KB = 1 MB)  |
| MAX_AGENT_WORKLOADS           | 16 per Agent      | CONFIGURATION POLICY  | Control block array allocation limit  |
| MAX_EVENT_SUBSCRIPTIONS       | 32 per Agent      | CONFIGURATION POLICY  | Ring buffer event queue RAM limit     |
| MAX_TRIGGERS                  | 16 per Agent      | CONFIGURATION POLICY  | Trigger match table array limit       |
+-------------------------------+-------------------+-----------------------+---------------------------------------+
```

---

## 13. Low-Level `libzero` Agent IPC Protocol (`libzero/src/agent.rs`)

Stage 4E defines the binary IPC message protocol for communicating with `agentd` over `brokerd`.

### Fixed Message Layout (2048 Bytes Alignment)

```text
+-------------------+-------------------+-------------------+-----------------------+
| Opcode (u32)      | RequestId (u64)   | AgentId (u64)     | WorkspaceId (u64)     |
+-------------------+-------------------+-------------------+-----------------------+
| Capability Handle (u32) | Reserved (u32)                                           |
+-----------------------------------------------------------------------------------+
| Payload (1952 Bytes ...)                                                          |
+-----------------------------------------------------------------------------------+
```

### IPC Opcodes Defined in 4E

| Opcode | Name | Capability Required | Description |
|---|---|---|---|
| `0x0501` | `OP_AGENT_CREATE` | $C_{ws}$ | Creates a new Agent entity in `agentd`. |
| `0x0502` | `OP_AGENT_DESTROY` | $C_{agent}$ | Transitions Agent to `Stopping` and handles Workloads.|
| `0x0503` | `OP_AGENT_GET_STATE` | $C_{agent}$ | Queries lifecycle & runtime execution state. |
| `0x0504` | `OP_AGENT_DISPATCH_GOAL`| $C_{agent}$ | Dispatches a structured GoalSpec to an Active Agent. |
| `0x0505` | `OP_AGENT_REGISTER_TRIGGER`| $C_{agent}$ | Registers an automated trigger condition. |
| `0x0506` | `OP_AGENT_SUBSCRIBE_EVENT`| $C_{agent}$ | Subscribes Agent to specific Workspace events. |
| `0x0507` | `OP_AGENT_DELEGATE` | $C_{agent}$ | Spawns a Child Agent with derived capability. |
| `0x0508` | `OP_AGENT_CLEAR_SUSPENSION`| Trusted Clearance Ticket | Resumes Agent execution following human clearance. |

---

## 14. Formal Security Invariants

Stage 4E enforces 13 primary security invariants:

1. **`I-AGENT-KERNEL-AUTHORITY`**: All capability attenuation, derivation, validation, and revocation are enforced strictly by the Stage 3H kernel. `agentd` holds only process-local handle indices.
2. **`I-AGENT-MODEL-NO-AUTHORITY`**: Model output is untrusted data and carries zero security authority.
3. **`I-AGENT-MODEL-NO-CAPABILITY-AMPLIFICATION`**: Model output cannot increase capability rights or widen $C_{agent}$.
4. **`I-AGENT-MODEL-NO-HUMAN-AUTHORIZATION`**: Model output cannot satisfy, grant, or bypass human authorization policies.
5. **`I-AGENT-ID-UNIQUE`**: Every Agent has a globally unique 64-bit `DistributedId` allocated via `resourced` that is never reused.
6. **`I-AGENT-NO-CAP-AMPLIFICATION`**: An Agent's capability envelope is derived strictly from its owning Workspace: $C_{agent} \subseteq C_{ws}$.
7. **`I-AGENT-WORKSPACE-CONTAINMENT`**: An Agent is strictly bound to a single `WorkspaceId` throughout its lifecycle.
8. **`I-AGENT-WORKLOAD-BOUNDARY`**: Execution requests route strictly through `workloadd` via `OP_WORKLOAD_CREATE`.
9. **`I-AGENT-NO-RESOURCE-BYPASS`**: Resource demands pass inside Workload specs to `resourced`.
10. **`I-AGENT-NO-AMBIENT-AUTHORITY`**: All `agentd` IPC operations require explicit capability handle verification.
11. **`I-AGENT-DELEGATION-ATTENUATION`**: Child Agent capability $C_{child} \subseteq C_{parent} \subseteq C_{ws}$.
12. **`I-AGENT-PERSISTENCE-CONSISTENCY`**: Persistent Agent metadata updates on ZeroFS are atomic via write-fsync-rename file operations. Capability handles are never persisted.
13. **`I-AGENT-EVENT-AUTHENTICITY`**: Event perception routing validates producer signatures via `brokerd`; unauthenticated event injection is rejected.

---

## 15. Non-Goals

The following are explicitly outside the scope of Stage 4E:
- Implementing LLM prompt templates, tokenizers, or neural network inference frameworks.
- Building a chatbot UI, terminal assistant, or web interface.
- Creating a second process scheduler or thread manager outside `workloadd`.
- Modifying any frozen Stage 3 or Stage 4A–4D binary, header, or source file.
- Implementing cross-node distributed compute fabric routing (deferred to Stage 4F).

---

## 16. Open Questions for Future Design Phases

1. **Inference Provider IPC Framing**: Should inference streaming adopt a standard chunked binary buffer framing over ZeroOS IPC channels, or rely on shared memory ring buffers for zero-copy token streaming?
2. **Context Summarization Scheduling**: Should Context Graph summarization be triggered automatically by `workspaced` when node counts exceed threshold policy, or explicitly requested by `agentd` via Workload tasks?

---

```text
STATUS: APPROVED — FROZEN
IMPLEMENTATION: REQUIRES SEPARATE IMPLEMENTATION PLAN
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
STAGE 4D MODIFICATIONS: NONE
```
