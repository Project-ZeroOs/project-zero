# STAGE 4F ARCHITECTURE SPECIFICATION (REV 2)

## Personal Compute Fabric Foundation & Intent Resolution Subsystem

**Status:** 🟢 FROZEN ARCHITECTURAL SPECIFICATION — REVIEW CANDIDATE  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-10-06  
**Target Milestone:** Stage 4F (Intent Resolution & Compute Fabric Foundation)  

---

## 1. Executive Summary & Rev2 Architectural Closures

Following architectural review of Revision 1, **Revision 2** establishes the definitive, closed-form specification for Stage 4F. Rev2 resolves all 11 blockers without modifying the frozen Stage 3A–3N kernel or Stage 4A–4E user-space contracts:

1. **Universal Execution Boundary (`I-INTENT-AGENT-BOUND`)**: Formally establishes that **every executable intent must be bound to a Workspace and executed through an Agent (`AgentId`)**. For one-shot human requests, `intentd` routes the validated plan to a workspace-default or ephemeral Agent managed by `agentd`. Intent resolution cannot bypass `agentd` or `workloadd`.
2. **CSDT vs Capability Separation (`I-FABRIC-CSDT-NOT-CAPABILITY`)**: Clarifies that a CSDT is a **distributed transport container**, not a Stage 3H capability. Remote nodes validate CSDTs to derive local shadow Stage 3H capabilities inside `brokerd`. Actual kernel authorization remains 100% Stage 3H capability enforcement.
3. **Monotonic Time Authority Grounding**: CSDT expiration is bound to Stage 4B's **Qualified Monotonic Time Authority** on the receiving node (`I-CSDT-EXPIRATION-MONOTONIC`). Wall-clock time (NTP) is not used for security expiration.
4. **Authoritative Provider Lease Control**: Physical capacity and resource leases remain 100% authoritative at the hosting node's `resourced`. Remote observations are advisory (`I-FABRIC-OBSERVED-CAPACITY-ADVISORY`).
5. **Decoupled Orchestration**: `workloadd` owns Workload DAG execution and lease requests; `fabricd` acts strictly as a P2P mesh transport and two-stage placement planner. `fabricd` does not schedule workloads.
6. **Distributed Class 3 Side-Effect Identity (`I-FABRIC-SIDE-EFFECT-DEDUP`)**: Prevents duplicate execution of Class 3 irreversible operations across network partitions via deterministic `OperationId` deduplication latches on ZeroFS.

---

## 2. Frozen Substrate Dependencies & Preservation Constraints

Phase 4F builds strictly on top of the frozen Stage 3 microkernel and Stage 4A–4E user-space services:

```text
Stage 3A–3N Kernel Nucleus   🟢 FROZEN / UNTOUCHED (0 bytes modified)
Stage 4A System Services     🟢 FROZEN / UNTOUCHED (brokerd, init)
Stage 4B Resource Graph      🟢 FROZEN / UNTOUCHED (resourced, Local Accounting)
Stage 4C Workload Engine     🟢 FROZEN / UNTOUCHED (workloadd, Task DAG)
Stage 4D Workspace Model     🟢 FROZEN / UNTOUCHED (workspaced, ZeroFS context)
Stage 4E Agent Runtime       🟢 FROZEN / UNTOUCHED (agentd, Two-Man Rule)
```

---

## 3. Universal Execution Boundary & Core System Architecture

```text
                               +-----------------------------+
                               |         HUMAN INTENT        |
                               | (Natural Language / Input)  |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |           intentd           |
                               |  (Intent Resolution Engine) |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |     STRUCTURED PLAN PROPOSAL|
                               | (Goals, DAG, Constraints)   |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |   DETERMINISTIC VALIDATOR   |
                               | (Cap / Policy / Envelope)   |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |      agentd (AgentId)       |  ◄── Universal Execution
                               | (Goal / Trigger Container)  |      Boundary (I-INTENT-AGENT-BOUND)
                               +--------------+--------------+
                                              |
                        +---------------------+---------------------+
                        |                                           |
           +------------v------------+                 +------------v------------+
           |        workloadd        |                 |         fabricd         |
           |  (Workload Task DAGs)   | ◄── Placement ──| (2-Stage Placement Plan)|
           +------------+------------+     Proposal    +------------+------------+
                        |                                           |
                        +---------------------+---------------------+
                                              |
                        +---------------------+---------------------+
                        |                                           |
           +------------v------------+                 +------------v------------+
           |    LOCAL NODE EXECUTION |                 |    REMOTE FABRIC NODE   |
           |   (resourced Leases)    |                 |  (CSDT Transport Mesh)  |
           +-------------------------+                 +-------------------------+
```

### Hierarchy of Concepts & Strict Separation of Concerns

1. **Human Intent**: Raw human request or trigger originating from a verified principal.
2. **Intent Resolver (`intentd`)**: Bounded user-space daemon that transforms raw human input into a structured, candidate `Plan`. Has zero capability authority (`Model Output ≠ Authority`).
3. **Agent (`agentd`)**: Universal OS-level execution actor bound 1:1 to a `WorkspaceId`. Every intent is assigned an `AgentId` (persistent or workspace-default ephemeral).
4. **Workload (`workloadd`)**: Authoritative milestone-driven task dependency graph (DAG) executing across local/remote processes.
5. **Compute Fabric (`fabricd`)**: Network mesh transport and placement planner evaluating hard/soft node eligibility.
6. **Resource Provider (`resourced`)**: Authoritative physical capacity, lease issuance, and local node accounting.

---

## 4. Intent Representation, Lifecycle, & Persistence Chain

### 4.1 Immutable Intent Audit Chain
ZeroOS preserves full auditability by keeping human input separate from model interpretations:

```text
IntentDescriptor (Raw Human Request)
        │
        ▼
IntentResolutionRecord (Parsed Interpretation & Ambiguity State)
        │
        ▼
ExecutionPlanRecord (Validated Structural Plan & Constraints)
        │
        ▼
WorkloadId / TaskID (Executed Task DAGs in workloadd)
```

### 4.2 Data Structure Definitions

```rust
#[repr(C)]
pub struct IntentDescriptor {
    pub intent_id: DistributedId,        // Composite 128-bit Intent ID
    pub principal_id: DistributedId,     // Identity of requesting user
    pub workspace_id: DistributedId,     // Bound Workspace container
    pub state: IntentState,              // Current lifecycle state
    pub ambiguity_flag: u8,              // 1 if clarification required, 0 if resolved
    pub _pad0: [u8; 6],
    pub submission_tsc: u64,             // Monotonic submission timestamp
    pub deadline_tsc: u64,               // Monotonic deadline timestamp
    pub min_ial_requirement: u8,        // Minimum IAL level (1..3)
    pub privacy_class: u8,               // Privacy classification (0=LocalOnly, 1=FabricPrivate, 2=Public)
    pub energy_limit_mwh: u32,           // Maximum allowable energy consumption (0=Unlimited)
    pub raw_intent_len: u32,             // Length of raw input string
    pub _pad1: [u8; 4],
    pub raw_intent_payload: [u8; 512],   // Raw input UTF-8 text buffer
}
```

### 4.3 Intent Lifecycle State Machine

| Lifecycle State | Description |
|---|---|
| **`Unresolved`** | Raw human request received by `intentd`; initial parsing in progress. |
| **`Clarifying`** | Request contains missing parameters; awaiting visual user response via `workspaced`/`spatiald`. |
| **`Resolved`** | Request is unambiguous and parsed into candidate constraints. |
| **`Rejected`** | Request violates workspace capability envelope, security policy, or user approval. |
| **`Expired`** | Deadline reached before clarification or placement commitment (`INTENT_CLARIFICATION_TTL`). |
| **`Cancelled`** | User explicitly cancelled the intent before or during execution. |
| **`Executing`** | Plan validated, bound to `AgentId`, dispatches Workload DAG to `workloadd`. |
| **`Executed`** | Workload DAG completed successfully; result logged to workspace audit chain. |

---

## 5. Model-Neutral Intelligence & Reasoning Boundary

ZeroOS decouples reasoning providers from system authority via an abstract interface:

```rust
pub trait InferenceProvider {
    fn propose_intent_resolution(&self, raw_input: &[u8], context_snippet: &[u8]) -> IntentResolutionProposal;
    fn propose_plan_dag(&self, resolved_intent: &IntentDescriptor) -> PlanDagProposal;
}
```

### Model Security Principles
```text
Model Output ≠ Intent Authorization
Model Output ≠ Capability
Model Output ≠ Resource Lease
Model Output ≠ Human Approval
Model Output ≠ Fabric Trust
```

1. **Candidate Status**: Output from any AI model (LLM, neural net, heuristic parser) is classified strictly as **untrusted candidate proposal data**.
2. **Deterministic Verification**: Every model-generated `Plan` must be validated by `intentd` against the caller's capability handle table and workspace security envelope before any action occurs.
3. **Zero Direct Privilege**: Models cannot invoke syscalls, manufacture capability tokens, allocate memory frames, or access network sockets directly.

---

## 6. Execution Plan Model & Task Mapping

An execution plan produced by `intentd` contains high-level intent goals (`PlanNode`) mapping to executable Workload task DAGs (`TaskNode` in Stage 4C):

```rust
#[repr(C)]
pub struct ExecutionPlan {
    pub plan_id: DistributedId,
    pub intent_id: DistributedId,
    pub agent_id: DistributedId,         // Assigned Agent execution context
    pub workspace_id: DistributedId,
    pub plan_node_count: u32,
    pub estimated_total_mflops: u64,
    pub required_capabilities_mask: u64,
    pub human_auth_required_flag: u8,
    pub _pad0: [u8; 7],
    pub plan_nodes: [PlanNode; MAX_PLAN_NODES],
}
```

### Distinction: `PlanNode` vs `TaskNode`
- **`PlanNode`**: High-level intent goal, constraint milestone, or human approval checkpoint.
- **`TaskNode`**: Executable Stage 4C task unit within a Workload DAG. A `PlanNode` maps 1:1 or 1:N to Workload DAGs, where each Workload DAG strictly respects Stage 4C's bound `MAX_TASKS_PER_WORKLOAD = 16`.

---

## 7. Compute Fabric Architecture & Security

```text
                  +-----------------------------------+
                  |      PERSONAL COMPUTE FABRIC      |
                  +-----------------+-----------------+
                                    |
        +---------------------------+---------------------------+
        |                                                       |
+-------v-------+                                       +-------v-------+
|  LOCAL NODE   | ◄─── Authenticated P2P Transport Mesh ─► |  REMOTE PEER  |
|  (Smartphone) |        (Encrypted Session Mesh)       |   (Workstation)|
+---------------+                                       +---------------+
| CPU Cores     |                                       | CPU Cores     |
| Local RAM     |                                       | Discrete GPU  |
| Camera/Sensor |                                       | Neural NPU    |
| Battery PMIC  |                                       | High-Cap NVMe |
+---------------+                                       +---------------+
```

### 7.1 Fabric Node Identity & Trust Hierarchy
1. **User Root Identity Key ($K_{\text{user}}$)**: Offline master Ed25519 keypair owning the personal fabric.
2. **Node Identity Key ($K_{\text{node}}$)**: Long-term Ed25519 signing keypair and X25519 key-exchange keypair bound to a physical node. NodeId = $\text{BLAKE2s}(K_{\text{node}}^{\text{pub}})$.
3. **Device Trust Certificate**: Signed by $K_{\text{user}}$, binding $K_{\text{node}}^{\text{pub}}$ to a `NodeId`, certified Identity Assurance Level (IAL-1..3), and capability scope.

### 7.2 Identity Assurance Levels (IAL)

| Level | Name | Cryptographic Provenance | Permitted Workloads |
|---|---|---|---|
| **IAL-1** | Software-Backed | Keypair stored in encrypted ZeroFS volume. | Batch non-sensitive compute, public compilation. |
| **IAL-2** | Hardware-Backed | Keypair sealed in hardware security module (TPM 2.0 / Apple SE). | Workspace-private compute, user data handling. |
| **IAL-3** | Hardware-Attested | Keypair backed by measured boot attestation proof. | Sovereign vault tasks, biometric cryptographic ops. |

---

## 8. Capability-Scoped Delegation Tokens (CSDT) & Time Authority

Capabilities are transported across network node boundaries using signed **CSDT** containers:

```rust
#[repr(C)]
pub struct CsdtToken {
    pub csdt_id: DistributedId,          // Unique CSDT Identifier
    pub issuer_node_id: u64,             // Source NodeId
    pub target_node_id: u64,             // Destination NodeId
    pub workspace_id: DistributedId,     // Workspace Context
    pub capability_rights_mask: u64,     // Attenuated rights bitmask
    pub valid_from_monotonic_tick: u64,  // Monotonic start tick on receiver
    pub expire_monotonic_tick: u64,      // Monotonic expiration tick on receiver
    pub signature: [u8; 64],             // Ed25519 signature by issuer K_node
}
```

### 8.1 CSDT Container vs Kernel Capability Invariant
$$\mathbf{Invariant\ I-FABRIC-CSDT-NOT-CAPABILITY:}$$
$$\text{A CSDT is a distributed cryptographic delegation container, NOT a kernel capability handle.}$$
$$\text{Upon receiving a CSDT, the target node's \texttt{fabricd} validates the signature and requests \texttt{brokerd} to instantiate}$$
$$\text{a local Stage 3H shadow capability inside the workspace handle table. Kernel authorization remains 100\% Stage 3H capability enforcement.}$$

### 8.2 Monotonic Time Authority Expiration
$$\mathbf{Invariant\ I-CSDT-EXPIRATION-MONOTONIC:}$$
$$\text{CSDT expiration MUST be evaluated using the receiving node's Stage 4B Qualified Monotonic Time Authority.}$$
$$\text{Wall-clock synchronization (NTP/PTP) MUST NOT be used for security validity.}$$

---

## 9. Decoupled Handoff: `workloadd` vs `fabricd` vs `resourced`

The orchestration handoff enforces strict decoupling between scheduling, placement, and resource authorization:

```text
1. workloadd  ──► Formulates Task Demand & Queries fabricd for Placement Proposal
2. fabricd   ──► Evaluates Hard Constraints & Soft Objective; Returns Candidate NodeId
3. workloadd  ──► Sends Lease Request to resourced on Target Candidate NodeId
4. resourced  ──► Authoritatively Commits Capacity & Issues ResourceLease Token
5. workloadd  ──► Dispatches Task Execution to Process Spawner
```

`fabricd` **never schedules tasks or issues resource leases**.

---

## 10. Two-Stage Fabric Placement Planner (`fabricd`)

```text
Candidate Nodes (Local Host + Remote Peers)
        │
        ▼ Phase 1: Hard Constraints Filter (Boolean Eligibility)
  - Cap Export Permitted? (CAP_NET_EXPORT verified)
  - Privacy Policy Compliant? (Workspace Privacy Class)
  - Hardware Capable? (ISA, GPU RAM, NPU TOPS)
  - Minimum IAL Satisfied? (Node IAL >= Task Min IAL)
  - Deadline Feasible? (Est RTT + Compute <= Deadline)
        │
        ▼ Eligible Nodes
        │
        ▼ Phase 2: Soft Optimization Cost Function
  Evaluate J(Node_i) = w_lat * N_lat(i) + w_eng * N_eng(i) + w_cost * N_cost(i)
        │
        ▼
Placement Proposal Returned to workloadd
```

### 10.1 Authoritative Provider Capacity Invariant
$$\mathbf{Invariant\ I-FABRIC-OBSERVED-CAPACITY-ADVISORY:}$$
$$\text{Remote node telemetry observations in \texttt{fabricd} are strictly advisory. Placement decisions MUST tolerate }$$
$$\text{observed capacity } \neq \text{ actual capacity. Final capacity commitment occurs authoritatively at provider node \texttt{resourced}.}$$

---

## 11. Distributed Class 3 Side-Effect Deduplication

To prevent duplicate execution of Class 3 irreversible operations across network partitions:

$$\mathbf{Invariant\ I-FABRIC-SIDE-EFFECT-DEDUP:}$$
$$\text{Every Class 3 operation carries a deterministic } \text{OperationId} = \text{BLAKE2s}(\text{IntentId}, \text{TaskId}, \text{Sequence}).$$
$$\text{The resource provider node maintains a durable Side-Effect Latch Table on ZeroFS. Before executing a Class 3 task,}$$
$$\text{the provider checks the latch. If already latched or executed, duplicate execution is rejected fail-closed.}$$

---

## 12. Static Memory Bounds Derivation & Derivation Categorization

All Stage 4F structures use fixed, pre-allocated static bounds to guarantee zero dynamic heap allocations:

| Constant Name | Value | Type | Driving Resource | Footprint | Exhaustion Behavior |
|---|---|---|---|---|---|
| `MAX_FABRIC_NODES` | `16` | `HARD BOUND` | Max P2P peers in personal fabric. | 16 × 256 B = 4 KiB | Rejects discovery of 17th peer fail-closed. |
| `MAX_REMOTE_RESOURCES`| `256` | `HARD BOUND` | Remote resource node catalog. | 256 × 128 B = 32 KiB | Rejects new resource publication. |
| `MAX_ACTIVE_CSDT` | `128` | `HARD BOUND` | Concurrent active CSDT tokens. | 128 × 128 B = 16 KiB | Rejects new remote delegation requests. |
| `MAX_REMOTE_LEASES` | `64` | `HARD BOUND` | Active remote contractual leases.| 64 × 128 B = 8 KiB | Rejects remote task placement. |
| `MAX_NODE_SESSIONS` | `16` | `POLICY` | Authenticated Noise mesh channels. | 16 × 512 B = 8 KiB | Drops unauthenticated peer handshakes. |
| `MAX_PENDING_INTENTS` | `32` | `POLICY` | Concurrent intents in `intentd`. | 32 × 1,024 B = 32 KiB| Rejects incoming human intent submissions. |
| `MAX_PLAN_NODES` | `16` | `POLICY` | Max goals per ExecutionPlan. | 16 × 256 B = 4 KiB | Rejects plans exceeding 16 goals. |
| `MAX_DISCOVERY_ENTRIES`| `32` | `POLICY` | Cached local discovery records. | 32 × 128 B = 4 KiB | Evicts oldest unauthenticated beacon (LRU). |

**Total BSS Memory Footprint for `intentd` + `fabricd`**: ~110,592 Bytes (~108 KiB), well within the 1 MiB daemon memory budget.

---

## 13. System Invariants (Authoritative Catalog)

1. `I-INTENT-AGENT-BOUND`: Every executable Intent is bound to a Workspace and executed through an Agent (`AgentId`).
2. `I-INTENT-NO-AUTHORITY`: Intent descriptors do not grant capabilities or system authority.
3. `I-INTENT-NO-CAPABILITY-AMPLIFICATION`: Plans cannot exceed Workspace Capability Envelope.
4. `I-INTENT-WORKSPACE-CONTAINMENT`: Every Intent is strictly bound to a single `WorkspaceId`.
5. `I-INTENT-DETERMINISTIC-VALIDATION`: Every model-generated plan must be deterministically validated before admission.
6. `I-MODEL-NO-AUTHORITY`: Model output is untrusted proposal data (`Model Output ≠ Authority`).
7. `I-PLAN-HARD-CONSTRAINTS`: Hard security, privacy, latency, and capability constraints can never be overridden by model proposals or soft cost functions.
8. `I-PLAN-DAG-VALID`: Task graphs within an ExecutionPlan must be strictly acyclic.
9. `I-PLAN-NO-BYPASS`: Workload DAGs execute strictly through `workloadd` task DAG scheduling.
10. `I-FABRIC-NODE-AUTHENTICATED`: All nodes must authenticate and hold valid Device Trust Certificates.
11. `I-FABRIC-CSDT-NOT-CAPABILITY`: CSDTs are transport containers; actual capabilities are local Stage 3H handles in `brokerd`.
12. `I-CSDT-EXPIRATION-MONOTONIC`: CSDT expiration is bound to the receiving node's Stage 4B Qualified Monotonic Time Authority.
13. `I-FABRIC-REMOTE-CAP-ATTENUATION`: Monotonic remote capability attenuation ($\text{ChildRights} \subseteq \text{ParentRights}$).
14. `I-FABRIC-OBSERVED-CAPACITY-ADVISORY`: Remote telemetry observations are advisory; final capacity allocation occurs at provider `resourced`.
15. `I-FABRIC-PRIVACY-CONSTRAINT`: Workspace privacy policy strictly governs peer node eligibility.
16. `I-FABRIC-IAL-CONSTRAINT`: Node IAL level must meet or exceed task minimum IAL.
17. `I-FABRIC-ENERGY-HARD-LIMIT`: Schedulers enforce hard energy limits.
18. `I-FABRIC-PARTITION-SAFETY`: Under network partition, remote shadow capabilities expire fail-closed upon TTL limit.
19. `I-FABRIC-SIDE-EFFECT-DEDUP`: Class 3 irreversible operations execute deduplication latches via deterministic `OperationId`.

---

## 14. Architecture Status Banner

```text
STATUS: DRAFT — ARCHITECTURE REVIEW REQUIRED
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
STAGE 4D MODIFICATIONS: NONE
STAGE 4E MODIFICATIONS: NONE
```
