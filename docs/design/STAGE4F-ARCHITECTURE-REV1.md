# STAGE 4F ARCHITECTURE SPECIFICATION (REV 1)

## Personal Compute Fabric Foundation & Intent Resolution Subsystem

**Status:** 🟡 DRAFT — ARCHITECTURE REVIEW REQUIRED  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-10-06  
**Target Milestone:** Stage 4F (Intent Resolution & Compute Fabric Foundation)  

---

## 1. Executive Summary & Scope

**Phase 4F** defines the **Intent Resolution and Compute Fabric Foundation** for ZeroOS. It establishes how human intent is securely transformed into deterministic, resource-aware, capability-bounded execution across a personal multi-device computing fabric.

### Core Architectural Mandates
1. **Model-Neutral Intelligence Boundary**: ZeroOS does not architecturally depend on any specific LLM, inference provider, framework, or model architecture. Model output is treated strictly as **untrusted data proposal**, extending the Stage 4E security principle (`Model Output ≠ Authority`).
2. **Intent ≠ Execution Authority**: Intent resolution converts human input into a structured `Plan` proposal. Intent resolution can **never** manufacture capability handles, amplify authority, allocate physical leases, bypass workspace security policy, or directly spawn kernel processes.
3. **Decoupled Orchestration Substrate**: `workloadd` remains the sole authoritative manager of Workload DAG execution; `resourced` remains the sole authority over physical resource leases and local node accounting; `brokerd` remains the capability directory authority; `agentd` remains the goal/event lifecycle daemon. Phase 4F introduces `intentd` (Intent Resolution & Plan Production) and `fabricd` (Distributed Mesh, Discovery, CSDT Transport, & Placement Planning).
4. **Hardware-Grounding & Fabric Security**: Distributed compute nodes are authenticated via asymmetric hardware-rooted keypairs (Ed25519/X25519) and certified Identity Assurance Levels (IAL-1..3). Cross-node capability transfers use Capability-Scoped Delegation Tokens (CSDT) with local monotonic LAPIC timer expiration (`I-CSDT-EXPIRATION-MONOTONIC`).

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

### Classification of Subsystem Dependencies

| Subsystem | Status | Usage in Phase 4F |
|---|---|---|
| **Stage 3 Kernel Nucleus** | `EXISTS` | Thread isolation (`CR3`), IPC rendezvous (`sys_ipc_call`), 10 core syscalls, Ring 3 privilege boundary. |
| **Stage 4A `brokerd`** | `EXISTS` | IPC service directory, service registration, capability handle delegation & attenuation. |
| **Stage 4B `resourced`** | `EXISTS` | Authoritative local Resource Graph, multi-dimensional quota limits, resource leases. |
| **Stage 4C `workloadd`** | `EXISTS` | Workload DAG execution, task dependency resolution, process lifecycle tracking. |
| **Stage 4D `workspaced`** | `EXISTS` | Workspace identity containment (`WorkspaceId`), ZeroFS context graph persistence, capability security envelope. |
| **Stage 4E `agentd`** | `EXISTS` | Agent lifecycle management, event trigger ring buffers, human authorization ticket integration (`AuthorizationTicket`). |
| **`intentd`** | `MISSING — 4F MUST DEFINE` | User-space Intent Resolution daemon; parses natural language into structured `Plan` proposals. |
| **`fabricd`** | `MISSING — 4F MUST DEFINE` | Personal compute fabric peer mesh, CSDT capability transport, two-stage placement planner, P2P discovery. |

---

## 3. The 4F Architecture Boundary & System Flow

ZeroOS converts human intent into personal compute fabric execution through a strictly bounded, multi-tier pipeline:

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
                        +---------------------+---------------------+
                        |                                           |
           +------------v------------+                 +------------v------------+
           |       workspaced        |                 |         agentd          |
           |   (Workspace Envelope)  |                 |     (Goal Loop)         |
           +------------+------------+                 +------------+------------+
                        |                                           |
                        +---------------------+---------------------+
                                              |
                               +--------------v--------------+
                               |           fabricd           |
                               | (2-Stage Fabric Planner)    |
                               |  Phase 1: Hard Constraints  |
                               |  Phase 2: Soft Optimization |
                               +--------------+--------------+
                                              |
                        +---------------------+---------------------+
                        |                                           |
           +------------v------------+                 +------------v------------+
           |    LOCAL NODE EXECUTION |                 |    REMOTE FABRIC NODE   |
           |  (workloadd / resourced)|                 | (CSDT Noise Transport)  |
           +-------------------------+                 +-------------------------+
```

### Hierarchy of Concepts

1. **Human Intent**: Natural-language request or trigger originating from a human principal.
2. **Intent Resolver (`intentd`)**: Bounded user-space daemon that transforms raw human input into a structured, candidate `Plan`.
3. **Structured Plan**: Intermediate, deterministic schema containing proposed goals, constraints, required capabilities, and candidate Workload DAG.
4. **Agent (`agentd`)**: Persistent execution actor bound 1:1 to a `WorkspaceId` that manages long-running goals and event triggers.
5. **Workload (`workloadd`)**: Milestone-driven task dependency graph (DAG) executing across local/remote processes.
6. **Resource Graph / Fabric (`resourced` / `fabricd`)**: Distributed network of physical hardware resources, leases, and encrypted interconnects.

---

## 4. Intent Representation & Lifecycle

### 4.1 Data Structure
An Intent is represented by a fixed-size, deterministic metadata structure:

```rust
#[repr(C)]
pub struct IntentDescriptor {
    pub intent_id: DistributedId,        // Composite 128-bit Intent ID
    pub principal_id: DistributedId,     // Identity of the requesting user
    pub workspace_id: DistributedId,     // Bound Workspace container
    pub state: IntentState,              // Current lifecycle state
    pub ambiguity_flag: u8,              // 1 if clarification required, 0 if resolved
    pub _pad0: [u8; 6],
    pub submission_tsc: u64,             // Monotonic submission timestamp
    pub deadline_tsc: u64,               // Monotonic deadline timestamp
    pub min_ial_requirement: u8,        // Minimum IAL level (1..3)
    pub privacy_class: u8,               // Privacy classification (0=LocalOnly, 1=FabricPrivate, 2=Public)
    pub energy_limit_mwh: u32,           // Maximum allowable energy consumption (0=Unlimited)
    pub raw_intent_len: u32,             // Length of raw input string in payload
    pub _pad1: [u8; 4],
    pub raw_intent_payload: [u8; 512],   // Raw input UTF-8 text buffer
}
```

### 4.2 Intent Lifecycle State Machine

```text
  [Unresolved] ───► (Parse & Ambiguity Check)
       │                      │
       │ (Ambiguous)          ▼ (Clear & Valid)
       ▼                 [Resolved]
  [Clarifying]                │
       │                      ▼
       ├─── (User Responds) ──┤
       │                      ▼
       ▼                [Plan Proposed]
  [Rejected]                  │
  [Expired]                   ▼ (Validation & Lease)
  [Cancelled]            [Executing] ───► [Executed]
```

| Lifecycle State | Description |
|---|---|
| **`Unresolved`** | Raw human request received by `intentd`; initial parsing in progress. |
| **`Clarifying`** | Request contains missing or ambiguous parameters; awaiting user clarification via trusted visual path. |
| **`Resolved`** | Request is unambiguous and parsed into candidate constraints. |
| **`Rejected`** | Request violates workspace capability envelope, security policy, or user policy. |
| **`Expired`** | Deadline reached before clarification or placement commitment. |
| **`Cancelled`** | User explicitly cancelled the intent before or during execution. |
| **`Executing`** | Plan validated, leases acquired, Workload DAG dispatched to `workloadd`. |
| **`Executed`** | Workload DAG completed successfully; output returned to workspace. |

---

## 5. Model-Neutral Intelligence & Reasoning Boundary

ZeroOS explicitly decouples intent resolution from specific AI model implementations, vendors, or tokenization architectures via an abstract provider interface:

```rust
pub trait InferenceProvider {
    fn propose_intent_resolution(&self, raw_input: &[u8], context_snippet: &[u8]) -> IntentResolutionProposal;
    fn propose_plan_dag(&self, resolved_intent: &IntentDescriptor) -> PlanDagProposal;
}
```

### Security Principles for Intelligence Integration

```text
Model Output ≠ Intent Authorization
Model Output ≠ Capability
Model Output ≠ Resource Lease
Model Output ≠ Human Approval
Model Output ≠ Fabric Trust
```

1. **Candidate Status**: Output from any AI model (LLM, neural net, heuristic parser) is classified strictly as **untrusted candidate proposal data**.
2. **Deterministic Verification**: Every model-generated `Plan` must be validated by the `intentd` Deterministic Engine against the caller's capability handle table and workspace security envelope before any system action is taken.
3. **Zero Direct Privilege**: Models cannot invoke system calls, manufacture capability tokens, allocate memory frames, or access network sockets directly.

---

## 6. Intent Ambiguity & Clarification Subsystem

When an intent lacks critical execution parameters (e.g., target file, destination workspace, high-risk action confirmation), `intentd` enters state `Clarifying` and requests user clarification:

```text
[Ambiguous Intent] ───► [intentd Ambiguity Detector]
                                │
                                ▼
              [Formulate Clarification Question]
                                │
                                ▼
         [Dispatch to workspaced Trusted UI Component]
                                │
                                ▼
               [Visual Display & User Selection]
                                │
                                ▼
            [Return Verified Clarification Token]
                                │
                                ▼
                    [Transition to Resolved]
```

### Ambiguity Invariants
- **`I-INTENT-NO-EXEC-AMBIGUOUS`**: An intent in state `Clarifying` or `Unresolved` can **never** trigger Workload DAG execution or resource lease acquisition.
- **`I-INTENT-TRUSTED-CLARIFICATION`**: Clarification questions and user responses must route through `workspaced` and trusted UI ports (`spatiald`). AI models cannot intercept or synthesize user responses.

---

## 7. Execution Plan Model & Deterministic Validation

An execution plan produced by `intentd` is a candidate structural blueprint for workload execution:

```rust
#[repr(C)]
pub struct ExecutionPlan {
    pub plan_id: DistributedId,
    pub intent_id: DistributedId,
    pub workspace_id: DistributedId,
    pub task_count: u32,
    pub estimated_total_mflops: u64,
    pub required_capabilities_mask: u64,
    pub human_auth_required_flag: u8,
    pub _pad0: [u8; 7],
    pub task_dag_nodes: [PlanTaskNode; MAX_PLAN_NODES],
}
```

### Deterministic Plan Validation Pipeline
Before `intentd` passes an `ExecutionPlan` to `workloadd` or `fabricd`, it must pass 5 deterministic validation checks:

```text
Candidate ExecutionPlan
        │
        ├── 1. Structural Acyclicity Check (Task DAG has zero cycles)
        ├── 2. Capability Envelope Check (Required caps ⊆ Workspace Envelope)
        ├── 3. Minimum IAL Check (Target fabric nodes meet IAL requirement)
        ├── 4. Privacy Boundary Check (Data locality complies with Privacy Class)
        └── 5. Human Escalation Check (High-risk tasks flagged for visual confirmation)
        │
        ▼ (All PASS)
Admitted ExecutionPlan
```

---

## 8. Compute Fabric Substrate Architecture

The **Personal Compute Fabric** pools physical hardware resources across local host nodes and trusted remote peer devices:

```text
                  +-----------------------------------+
                  |      PERSONAL COMPUTE FABRIC      |
                  +-----------------+-----------------+
                                    |
        +---------------------------+---------------------------+
        |                                                       |
+-------v-------+                                       +-------v-------+
|  LOCAL NODE   | ◄─── Mutual Noise Transport Mesh ───► |  REMOTE PEER  |
|  (Smartphone) |        (Noise_IKpsk2 / UDP)           |   (Workstation)|
+---------------+                                       +---------------+
| CPU Cores     |                                       | CPU Cores     |
| Local RAM     |                                       | Discrete GPU  |
| Camera/Sensor |                                       | Neural NPU    |
| Battery PMIC  |                                       | High-Cap NVMe |
+---------------+                                       +---------------+
```

### 8.1 Fabric Node Identity & Trust Hierarchy
Node identity across the compute fabric is established through a 3-tier cryptographic hierarchy:

1. **User Root Identity Key ($K_{\text{user}}$)**: Offline master Ed25519 keypair owning the personal fabric.
2. **Node Identity Key ($K_{\text{node}}$)**: Long-term Ed25519 signing keypair and X25519 key-exchange keypair bound to a physical node. NodeId = $\text{BLAKE2s}(K_{\text{node}}^{\text{pub}})$.
3. **Device Trust Certificate**: Signed by $K_{\text{user}}$, binding $K_{\text{node}}^{\text{pub}}$ to a `NodeId`, certified Identity Assurance Level (IAL-1..3), and capability scope.

#### Identity Assurance Levels (IAL)

| Level | Name | Cryptographic Provenance | Permitted Workloads |
|---|---|---|---|
| **IAL-1** | Software-Backed | Keypair stored in encrypted ZeroFS volume. | Batch non-sensitive compute, public compilation. |
| **IAL-2** | Hardware-Backed | Keypair sealed in hardware security module (TPM 2.0 / Apple SE). | Workspace-private compute, user data handling. |
| **IAL-3** | Hardware-Attested | Keypair backed by measured boot attestation proof. | Sovereign vault tasks, biometric cryptographic ops. |

---

## 9. Capability-Scoped Delegation Tokens (CSDT)

Capabilities are delegated across network node boundaries using signed **CSDT** containers:

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

### 9.1 Monotonic Local Timer Authority Invariant
$$\mathbf{Invariant\ I-CSDT-EXPIRATION-MONOTONIC:}$$
$$\text{CSDT expiration MUST be evaluated using the monotonic local hardware clock on the receiving node (LAPIC timer ticks).}$$
$$\text{Wall-clock synchronization (NTP/PTP) MUST NOT be used for security validity.}$$

### 9.2 Network Partition & TTL Revocation Bound
$$\mathbf{Invariant\ I-REMOTE-CAP-REVOCATION-BOUNDED:}$$
$$\text{Under network partition or packet loss, stale authorization on a target node is strictly bounded}$$
$$\text{by the finite CSDT TTL: } \Delta t_{\text{stale}} \le \Delta T_{\text{TTL}}.$$

---

## 10. Two-Stage Fabric Placement Planner (`fabricd`)

When a workload task requires physical execution, `fabricd` evaluates all available fabric nodes using a strict two-stage planner:

```text
Candidate Nodes (Local + Remote Peers)
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
Target Fabric Node Selected
```

### Normalized Cost Function
$$J(\text{Node}_i) = w_{\text{lat}} \cdot N_{\text{lat}}(i) + w_{\text{eng}} \cdot N_{\text{eng}}(i) + w_{\text{cost}} \cdot N_{\text{cost}}(i)$$

Where:
- $N_{\text{lat}}(i) \in [0, 1]$: Normalized network RTT + serialization + compute latency.
- $N_{\text{eng}}(i) \in [0, 1]$: Normalized battery drain penalty on local mobile host.
- $N_{\text{cost}}(i) \in [0, 1]$: Normalized remote queue contention and thermal penalty.
- $w_{\text{lat}} + w_{\text{eng}} + w_{\text{cost}} = 1.0$ (Policy-driven dynamic weights).

---

## 11. Energy & Privacy Placement Invariants

### 11.1 Telemetry Classification Invariant
$$\mathbf{Invariant\ I-ENERGY-MEASUREMENT-CLASSIFIED:}$$
$$\text{All energy telemetry MUST specify its classification: Measured (PMIC), Estimated (Analytical), or Declared.}$$
$$\text{Security limits and hard resource quotas MUST NOT depend on untrusted estimates.}$$

### 11.2 Privacy Data Export Invariant
$$\mathbf{Invariant\ I-DATA-EXPORT-ENFORCED:}$$
$$\text{Workspace payload data CANNOT be serialized or transmitted across the fabric without proof of } \texttt{CAP\_NET\_EXPORT}$$
$$\text{verified at the Capability Evaluation Layer inside \texttt{brokerd}.}$$

---

## 12. Fabric Failure Semantics & Workload Recovery

When a remote fabric node crashes, disconnects, or experiences a CSDT TTL expiry, recovery semantics follow the **Stage 4C 3-Class Recovery Framework**:

| Workload Class | Nature of Task | Recovery Action on Peer Failure |
|---|---|---|
| **Class 1: Pure / Idempotent** | Stateless compute, compilation, neural inference. | **Disowning & Re-execution**: Discard remote task result; re-dispatch task locally or to another eligible peer node. Zero side-effect risk. |
| **Class 2: Checkpointed Stateful** | Large build, model fine-tuning, data processing. | **Resume from Checkpoint**: Roll back state to the latest verified ZeroFS checkpoint generation; re-issue lease on new peer node. |
| **Class 3: Irreversible External** | Network API send, hardware device trigger, sensor action. | **Compensating Action**: Live rollback is physically impossible. Transition to state `FailedAtMilestone` and execute registered compensation handler. |

$$\mathbf{Invariant\ I-NO-LIVE-PROCESS-MIGRATION:}\quad \text{Live process memory pages } (\text{CR3} / \text{VMO}) \text{ CANNOT migrate live across host nodes.}$$
$$\text{Fabric distribution occurs exclusively via task re-dispatch or ZeroFS checkpoint state transfer.}$$

---

## 13. System Services & IPC Boundaries

```text
+-----------------------------------------------------------------------------+
|                               USER SPACE DOMAIN                             |
|                                                                             |
|   +-------------+  +-------------+  +-------------+  +-------------+        |
|   |   intentd   |  |   agentd    |  |  workspaced |  |   fabricd   |        |
|   | (Intent Res)|  | (Goal Loop) |  | (Workspace) |  | (Mesh/Plan) |        |
|   +------+------+  +------+------+  +------+------+  +------+------+        |
|          |                |                |                |               |
|   +------v----------------v----------------v----------------v------+        |
|   |                           brokerd                              |        |
|   |       (Service Discovery, Namespace, Capability Delegation)    |        |
|   +------+--------------------------------------------------+------+        |
|          |                                                  |               |
|   +------v------+                                    +------v------+        |
|   |  workloadd  |                                    |  resourced  |        |
|   | (Task DAGs) |                                    | (Leases/Cap)|        |
|   +-------------+                                    +-------------+        |
+-----------------------------------------------------------------------------+
```

### IPC Protocol OpCodes for `intentd` & `fabricd`

```rust
// intentd IPC Protocol OpCodes (Service Name: "intentd.srv")
pub const OP_INTENT_SUBMIT:              u64 = 0x0601; // Response: 0x0681
pub const OP_INTENT_RESOLVE:             u64 = 0x0602; // Response: 0x0682
pub const OP_INTENT_QUERY_STATE:         u64 = 0x0603; // Response: 0x0683
pub const OP_INTENT_CANCEL:              u64 = 0x0604; // Response: 0x0684

// fabricd IPC Protocol OpCodes (Service Name: "fabricd.srv")
pub const OP_FABRIC_NODE_REGISTER:       u64 = 0x0610; // Response: 0x0690
pub const OP_FABRIC_NODE_HEARTBEAT:      u64 = 0x0611; // Response: 0x0691
pub const OP_FABRIC_CSDT_DELEGATE:       u64 = 0x0612; // Response: 0x0692
pub const OP_FABRIC_CSDT_REVOKE:         u64 = 0x0613; // Response: 0x0693
pub const OP_FABRIC_QUERY_TOPOLOGY:      u64 = 0x0614; // Response: 0x0694
```

---

## 14. Static Memory Bounds & Allocation Limits

All Stage 4F data structures in `intentd` and `fabricd` use fixed, pre-allocated static bounds to guarantee zero dynamic heap growth during execution:

| Constant Name | Value | Description | Static Footprint |
|---|---|---|---|
| `MAX_FABRIC_NODES` | `16` | Maximum peer compute nodes in personal fabric. | 16 × 256 B = 4,096 B |
| `MAX_REMOTE_RESOURCES` | `256` | Maximum remote advertised resource nodes. | 256 × 128 B = 32,768 B |
| `MAX_ACTIVE_CSDT` | `128` | Maximum concurrently active CSDT tokens. | 128 × 128 B = 16,384 B |
| `MAX_REMOTE_LEASES` | `64` | Maximum active remote resource leases. | 64 × 128 B = 8,192 B |
| `MAX_NODE_SESSIONS` | `16` | Maximum authenticated Noise P2P sessions. | 16 × 512 B = 8,192 B |
| `MAX_PENDING_INTENTS` | `32` | Maximum concurrent intents in `intentd`. | 32 × 1,024 B = 32,768 B |
| `MAX_PLAN_NODES` | `16` | Maximum task nodes per ExecutionPlan DAG. | 16 × 256 B = 4,096 B |
| `MAX_DISCOVERY_ENTRIES` | `32` | Maximum cached local discovery records. | 32 × 128 B = 4,096 B |

**Total BSS Memory Footprint for `intentd` + `fabricd`**: ~110,592 Bytes (~108 KiB), well within the 1 MiB user daemon memory limit.

---

## 15. System Invariants (Authoritative Catalog)

1. `I-INTENT-NO-AUTHORITY`: Intent descriptors and Parsed Plans do not grant capabilities or system authority.
2. `I-INTENT-NO-CAPABILITY-AMPLIFICATION`: Plans derived from intent cannot request capabilities exceeding the Workspace Capability Envelope.
3. `I-INTENT-WORKSPACE-CONTAINMENT`: Every Intent is strictly bound to a single `WorkspaceId`.
4. `I-INTENT-DETERMINISTIC-VALIDATION`: Every model-generated plan must be deterministically validated before admission.
5. `I-MODEL-NO-AUTHORITY`: Model output is untrusted data proposal (`Model Output ≠ Authority`).
6. `I-PLAN-HARD-CONSTRAINTS`: Hard security, privacy, latency, and capability constraints can never be overridden by model proposals or soft cost functions.
7. `I-PLAN-DAG-VALID`: Task graphs within an ExecutionPlan must be strictly acyclic.
8. `I-PLAN-NO-BYPASS`: ExecutionPlans execute strictly through `workloadd` task DAG scheduling.
9. `I-FABRIC-NODE-AUTHENTICATED`: All fabric nodes must authenticate via Noise_IKpsk2 and possess a valid Device Trust Certificate.
10. `I-FABRIC-CSDT-VALID`: Remote capability delegation requires a valid, unrevoked CSDT token.
11. `I-FABRIC-CSDT-TTL`: Remote CSDT validity is bounded by receiving node monotonic timer expiration.
12. `I-FABRIC-REMOTE-CAP-ATTENUATION`: Cross-node capability delegation enforces monotonic attenuation ($\text{ChildRights} \subseteq \text{ParentRights}$).
13. `I-FABRIC-REMOTE-LEASE-AUTHORITY`: Remote lease requests must present a valid CSDT capability token.
14. `I-FABRIC-REMOTE-TELEMETRY-NOT-AUTHORITY`: Remote telemetry observations are strictly advisory.
15. `I-FABRIC-PRIVACY-CONSTRAINT`: Workspace privacy policy strictly governs peer node eligibility.
16. `I-FABRIC-IAL-CONSTRAINT`: Node IAL level must meet or exceed task minimum IAL requirements.
17. `I-FABRIC-ENERGY-HARD-LIMIT`: Schedulers must enforce hard energy boundaries.
18. `I-FABRIC-PARTITION-SAFETY`: Under network partition, remote shadow capabilities expire fail-closed upon TTL limit.
19. `I-FABRIC-NO-DUPLICATE-IRREVERSIBLE-EXECUTION`: Class 3 workloads with irreversible side effects are never automatically re-executed on failure.

---

## 16. Non-Goals

Phase 4F explicitly excludes:
- Generic chatbot / conversational AI UI frameworks.
- Proprietary LLM vendor bindings or tokenizers.
- Model training / weights optimization pipelines.
- Replacement of the kernel capability engine or `workloadd` scheduler.
- Live process virtual memory migration across network links (`I-NO-LIVE-PROCESS-MIGRATION`).

---

## 17. Architecture Status Banner

```text
STATUS: DRAFT — REQUIRES REVIEW
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
STAGE 4D MODIFICATIONS: NONE
STAGE 4E MODIFICATIONS: NONE
```
