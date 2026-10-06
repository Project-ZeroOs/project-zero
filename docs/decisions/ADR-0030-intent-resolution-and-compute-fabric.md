# ADR-0030: Intent Resolution, Model-Neutral Reasoning Boundary, and Personal Compute Fabric Architecture

## Status
**Proposed / Architectural Discovery Draft (Rev 3)** (Phase 4F Architecture Discovery)

## Context
Project Zero Stage 3 microkernel and Stage 4A–4E user-space services establish a deterministic capability-based kernel, user-space service directory (`brokerd`), generic Resource Graph & leases (`resourced`), workload task DAG orchestrator (`workloadd`), workspace context container (`workspaced`), and autonomous agent runtime (`agentd`).

Phase 4F defines the boundary for **Intent Resolution and Compute Fabric Foundation**. The challenge is to connect human intent and autonomous agent goals to the underlying personal compute fabric without transforming ZeroOS into a chatbot framework, vendor-locked LLM wrapper, or second operating-system abstraction.

## Decision

### 1. Universal Execution Boundary & Ephemeral Agent Lifecycle (`I-INTENT-AGENT-BOUND`)
Every executable Intent is bound to a Workspace (`WorkspaceId`) and executed through an Agent (`AgentId`). For one-shot human requests, `intentd` routes the validated plan to an Ephemeral Agent managed by `agentd`.
- **Ephemeral Agent Operational Lifecycle**: `Creating` → `Active` (Workload execution) → `Stopping` (sys_cap_revoke) → `Reclaimed` (ACB slot recycled).
- Ephemeral Agents do not persist context across executions.

### 2. Kernel-Authoritative Remote Capability Derivation (`I-FABRIC-REMOTE-CAP-KERNEL-AUTHORITY`)
CSDT containers carry distributed authorization across the network, but do NOT directly grant kernel capabilities.
- Target node `fabricd` validates the CSDT container signature and distributed policy, then presents it to the local workspace root capability (`C_ws`).
- Target node executes `sys_cap_derive` via Stage 3H to derive a local attenuated capability inside `brokerd`.
- When CSDT expires or is revoked, `sys_cap_revoke` invalidates the derived capability subtree fail-closed.
- Neither `brokerd` nor `fabricd` can create authority that did not already exist under Stage 3H.

### 3. Separation of Intent, Planning, and Execution
ZeroOS strictly separates intent parsing, plan formulation, workload execution, and resource allocation into distinct user-space components:
- **`intentd` (Intent Resolution Engine)**: Parses raw natural-language human requests into structured `Plan` proposals. Has zero system authority (`Model Output ≠ Authority`).
- **Deterministic Validation Engine**: Validates candidate `Plan` DAGs against caller's `WorkspaceId` security envelope, capability handle table, and IAL rules before plan admission.
- **`agentd` (Agent Runtime)**: Universal execution container managing goals, event triggers, and human visual approval tickets.
- **`workloadd` (Workload Orchestrator)**: Authoritative executor of admitted Workload task DAGs.
- **`fabricd` (Compute Fabric Daemon)**: Manages P2P device mesh channels, CSDTs, and two-stage fabric placement planning. **`fabricd` possesses zero capability, resource, or workload execution authority.**

### 4. Model-Neutral Intelligence Boundary (`Model Output ≠ Authority`)
ZeroOS extends the Stage 4E model non-authority principle (`Model Output ≠ Authority`) to Stage 4F:
- **Candidate Proposal Status**: Output from any AI model, LLM, or neural net is treated strictly as unverified candidate data.
- **No Direct System Control**: AI models cannot issue capability handles, allocate physical memory/leases, bypass workspace privacy policies, or satisfy human visual approval prompts.

### 5. Time Authority Grounding & Authoritative Provider Control
- **Monotonic Expiration (`I-CSDT-EXPIRATION-MONOTONIC`)**: CSDT expiration is bound to Stage 4B's Qualified Monotonic Time Authority on the receiving node. Wall-clock time (NTP) is not used for security validity.
- **Authoritative Provider Control (`I-FABRIC-OBSERVED-CAPACITY-ADVISORY`)**: Remote node telemetry observations in `fabricd` are strictly advisory. Final capacity commitment occurs authoritatively at the provider node's `resourced`.

### 6. Distributed Class 3 Side-Effect Boundary & Deduplication (`I-FABRIC-SIDE-EFFECT-BOUNDARY`)
Internal ZeroOS duplicate suppression uses a 2-phase `Prepared` → `Committed` ZeroFS journal latch for identical `OperationId = BLAKE2s(IntentId, TaskId, Sequence)`. External third-party system side-effects require the external system to accept `OperationId` as an idempotency transaction key.

---

## Authoritative 22-Invariant Catalog
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

## Status Banner
```text
STATUS: DRAFT — ARCHITECTURE REVIEW REQUIRED
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A–4E MODIFICATIONS: NONE
```
