# STAGE 4F ARCHITECTURE SPECIFICATION (REV 3)

## Personal Compute Fabric Foundation & Intent Resolution Subsystem

**Status:** 🟢 FROZEN ARCHITECTURAL SPECIFICATION — FINAL FREEZE CANDIDATE  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-10-06  
**Target Milestone:** Stage 4F (Intent Resolution & Compute Fabric Foundation)  

---

## 1. Executive Summary & Rev3 Architectural Closures

Following the third round of architecture review, **Revision 3** delivers the definitive, closed-form architectural specification for Stage 4F. Rev3 resolves the remaining six operational contracts without modifying the frozen Stage 3A–3N kernel or Stage 4A–4E user-space specifications:

1. **Kernel-Authoritative Remote Capability Derivation (`I-FABRIC-REMOTE-CAP-KERNEL-AUTHORITY`)**: A valid CSDT container does not create kernel authority. `fabricd` validates the CSDT signature and distributed policy, then presents it to the local workspace root capability (`C_ws`). The target node executes `sys_cap_derive` via Stage 3H to derive a local attenuated capability. When CSDT expires or is revoked, `sys_cap_revoke` invalidates the derived capability subtree fail-closed. `fabricd` and `brokerd` cannot create authority outside Stage 3H.
2. **Side-Effect Boundary & Internal Deduplication (`I-FABRIC-SIDE-EFFECT-BOUNDARY`)**: Distinguishes internal ZeroOS duplicate suppression from external system idempotency. ZeroOS guarantees duplicate suppression for identical `OperationId = BLAKE2s(IntentId, TaskId, Sequence)` within an authoritative provider boundary using a 2-phase `Prepared` → `Committed` ZeroFS journal latch. External side-effects require the external system to accept `OperationId` as an idempotency key.
3. **Operational Ephemeral Agent Lifecycle**: Defines complete operational lifecycle for one-shot human intent execution: `Creating` → `Active` (Workload execution) → `Stopping` → `Reclaimed` (capability teardown & ACB slot recycling). Ephemeral Agents do not persist context across executions.
4. **UI-Agnostic Trusted Interaction Boundary**: Replaces specific UI references with a generic **Trusted User Interaction Boundary** mediated through `workspaced`. 4F remains strictly UI-agnostic.
5. **Non-Authority of `fabricd`**: Explicitly establishes that `fabricd` is strictly a network transport mesh and placement planner. `fabricd` possesses zero capability authority, zero resource allocation authority, and zero workload execution authority.
6. **Authoritative 22-Invariant Catalog**: Fully reconciled catalog of all 22 system invariants.

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

### Decoupled Service Authority Matrix

| Service | Primary Role | System Authority | What it Cannot Do |
|---|---|---|---|
| **`intentd`** | Intent Parsing & Plan Formulation | None (`Model Output ≠ Authority`) | Cannot issue caps, leases, or spawn processes. |
| **`agentd`** | Agent & Goal Lifecycle | Agent Context & Trigger Loops | Cannot bypass `workloadd` or kernel caps. |
| **`workloadd`** | Task DAG Execution | Workload DAG Execution & Task Retry | Cannot allocate resources or create caps directly. |
| **`resourced`**| Local Resource Authority | Physical Lease Issuance & Accounting | Cannot execute workloads or grant caps without proof. |
| **`fabricd`** | Fabric Mesh & Placement | P2P Transport & 2-Stage Placement | **Zero capability, resource, or workload authority.** |

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

### 4.2 Ephemeral vs Persistent Agent Operational Lifecycle

```text
1. ONE-SHOT INTENT (Ephemeral Agent Lifecycle):
   Intent Received ──► Creating (AgentId allocated under C_ws)
                         │
                         ▼
                     Active (Dispatches Workload DAG to workloadd)
                         │
                         ▼
                     Stopping (Workload DAG completes; sys_cap_revoke)
                         │
                         ▼
                     Reclaimed (ACB slot recycled in static table)

2. PERSISTENT INTENT (Persistent Agent Lifecycle):
   Intent Received ──► Assigned to Existing Agent (AgentId)
                         │
                         ▼
                     Active (Goal loop updated; triggers registered)
```

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

### 7.1 Identity Assurance Levels (IAL)

| Level | Name | Cryptographic Provenance | Permitted Workloads |
|---|---|---|---|
| **IAL-1** | Software-Backed | Keypair stored in encrypted ZeroFS volume. | Batch non-sensitive compute, public compilation. |
| **IAL-2** | Hardware-Backed | Keypair sealed in hardware security module (TPM 2.0 / Apple SE). | Workspace-private compute, user data handling. |
| **IAL-3** | Hardware-Attested | Keypair backed by measured boot attestation proof. | Sovereign vault tasks, biometric cryptographic ops. |

---

## 8. Capability-Scoped Delegation Tokens (CSDT) & Kernel Derivation Authority

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

### 8.1 Kernel-Authoritative Remote Capability Derivation Invariant
$$\mathbf{Invariant\ I-FABRIC-REMOTE-CAP-KERNEL-AUTHORITY:}$$
$$\text{A valid CSDT container does NOT directly create kernel authority.}$$
$$\text{Upon receiving a CSDT, target node \texttt{fabricd} validates the signature and distributed policy, then presents it to } C_{\text{ws}}.$$
$$\text{The target workspace derives a local Stage 3H capability via } \text{sys\_cap\_derive}(C_{\text{ws}}, \text{attenuated\_rights}, \&\text{mut } C_{\text{local}}).$$
$$\text{When the CSDT expires or is revoked, } \text{sys\_cap\_revoke}(C_{\text{local}}) \text{ invalidates the derived capability subtree fail-closed.}$$

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

## 11. Distributed Class 3 Side-Effect Boundary & Deduplication

To prevent duplicate execution of Class 3 irreversible operations across network partitions:

$$\mathbf{Invariant\ I-FABRIC-SIDE-EFFECT-BOUNDARY:}$$
$$\text{ZeroOS guarantees internal duplicate suppression for identical } \text{OperationId} = \text{BLAKE2s}(\text{IntentId}, \text{TaskId}, \text{Sequence})$$
$$\text{within an authoritative ZeroOS provider boundary using a 2-phase } \text{Prepared} \to \text{Committed} \text{ ZeroFS journal latch.}$$
$$\text{External side-effects require the external third-party system to accept } \text{OperationId} \text{ as an idempotency transaction key.}$$

---

## 12. Static Memory Bounds Derivation & Derivation Categorization

All Stage 4F structures use fixed, pre-allocated static bounds to guarantee zero dynamic heap allocations:

| Constant Name | Value | Type | Driving Resource | Footprint | Exhaustion Behavior |
|---|---|---|---|---|---|
| `MAX_FABRIC_NODES` | `16` | `HARD BOUND` | Max P2P peers in personal fabric. | 16 × 256 B = 4 KiB | Rejects discovery of 17th peer fail-closed. |
| `MAX_REMOTE_RESOURCES`| `256` | `HARD BOUND` | Remote resource node catalog. | 256 × 128 B = 32 KiB | Rejects new resource publication. |
| `MAX_ACTIVE_CSDT` | `128` | `HARD BOUND` | Concurrent active CSDT tokens. | 128 × 128 B = 16 KiB | Rejects new remote delegation requests. |
| `MAX_REMOTE_LEASES` | `64` | `HARD BOUND` | Active remote contractual leases.| 64 × 128 B = 8 KiB | Rejects remote task placement. |
| `MAX_NODE_SESSIONS` | `16` | `POLICY` | Authenticated P2P mesh channels. | 16 × 512 B = 8 KiB | Drops unauthenticated peer handshakes. |
| `MAX_PENDING_INTENTS` | `32` | `POLICY` | Concurrent intents in `intentd`. | 32 × 1,024 B = 32 KiB| Rejects incoming human intent submissions. |
| `MAX_PLAN_NODES` | `16` | `POLICY` | Max goals per ExecutionPlan. | 16 × 256 B = 4 KiB | Rejects plans exceeding 16 goals. |
| `MAX_DISCOVERY_ENTRIES`| `32` | `POLICY` | Cached local discovery records. | 32 × 128 B = 4 KiB | Evicts oldest unauthenticated beacon (LRU). |

**Total BSS Memory Footprint for `intentd` + `fabricd`**: ~110,592 Bytes (~108 KiB), well within the 1 MiB daemon memory budget.

---

## 13. System Invariants (Reconciled Authoritative 22-Invariant Catalog)

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
12. `I-FABRIC-REMOTE-CAP-KERNEL-AUTHORITY`: CSDTs never directly create kernel authority; local capabilities are derived exclusively via `sys_cap_derive` from `C_ws`.
13. `I-CSDT-EXPIRATION-MONOTONIC`: CSDT expiration is bound to receiving node Stage 4B Qualified Monotonic Time Authority.
14. `I-FABRIC-REMOTE-CAP-ATTENUATION`: Monotonic remote capability attenuation ($\text{ChildRights} \subseteq \text{ParentRights}$).
15. `I-LEASE-AUTH-BOUNDED`: Physical leases cannot confer authority exceeding the authorizing capability token.
16. `I-FABRIC-OBSERVED-CAPACITY-ADVISORY`: Remote telemetry observations are advisory; final capacity allocation occurs at provider `resourced`.
17. `I-FABRIC-PRIVACY-CONSTRAINT`: Workspace privacy policy strictly governs peer node eligibility.
18. `I-FABRIC-IAL-CONSTRAINT`: Node IAL level must meet or exceed task minimum IAL.
19. `I-FABRIC-ENERGY-HARD-LIMIT`: Schedulers enforce hard energy limits.
20. `I-FABRIC-PARTITION-SAFETY`: Under network partition, remote shadow capabilities expire fail-closed upon TTL limit.
21. `I-FABRIC-SIDE-EFFECT-BOUNDARY`: Internal duplicate suppression uses 2-phase ZeroFS `OperationId` latches; external side-effects require external transaction key contracts.
22. `I-FABRIC-DAEMON-NO-AUTHORITY`: `fabricd` is strictly a network transport mesh and placement planner with zero capability or resource authority.

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
