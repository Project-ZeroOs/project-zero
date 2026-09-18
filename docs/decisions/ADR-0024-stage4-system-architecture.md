# ADR-0024: Stage 4 System Architecture, Compute Fabric & Generic Resource Sharing (Rev6)

## Status
🟢 **APPROVED — ARCHITECTURALLY FROZEN FOR STAGE 4 IMPLEMENTATION** (2026-09-18)

---

## Context
Stages 3A through 3N established an authoritative, bare-metal capability microkernel operating in 64-bit Long Mode across up to 4 SMP cores. All kernel ABIs, structures, 10 core syscalls, and the 13-level lock hierarchy are permanently frozen.

Following the sixth round of adversarial architecture review, **Revision 6** finalizes the last two distributed-systems contracts:
1. **Durable DistributedId Allocator State (`I-ID-DURABLE-ALLOCATOR-STATE`)**: Ensures that sequence monotonicity and non-reuse survive process crashes, kernel reboots, and power loss. Allocator state persistence must be committed to durable storage *before* any identifier is externally observable.
2. **Monotonic Local Expiration Authority (`I-CSDT-EXPIRATION-MONOTONIC`)**: Eliminates reliance on distributed wall-clock synchronization (NTP/PTP) by evaluating CSDT expiration strictly against the receiving node's local monotonic hardware timer.
3. **Formal NodeId Rotation Rules (`I-NODE-ID-ROTATION-FORMAL`)**: Clarifies that changing a `NodeId` establishes an entirely new cryptographic namespace and trust certificate; rotation cannot be used to escape unpersisted crash state.

---

## Architectural Decisions

### ADR-0024-1: Hard Kernel Freeze & Stage 4 User-Space Scope
1. **Stage 3 Hard Freeze**: No kernel sub-stages (`3O`, `3P`) are introduced. Stage 3 ABIs (`Process` 128 B, `KernelThread` 176 B, `PerCpu` 48 B, `HandleTable` 520 B, `CapabilityNode` 24 B, `KernelObjectSlot` 40 B, `SyscallFrame` 144 B), the 4 MiB bootstrap window, and the zero-dynamic-kernel-heap invariant remain permanently inviolate.
2. **Zero Stage 3 Dependency Conflicts**: No kernel modifications are required. Stage 3 primitives fully support Stage 4.
3. **Stage 4 Execution Domain**: Stage 4 executes entirely in **Ring 3 User Space** as system services (`brokerd`, `resourced`, `fabricd`, `workloadd`, `workspaced`, `intentd`) and unprivileged sandboxed runtimes.

---

### ADR-0024-2: Distributed Identifier Architecture & Crash-Safe Durability
Identifiers are composite 128-bit tuples: `DistributedId = (NodeId: 64 bits, LocalSeq: 64 bits)`.
- `I-ID-UNIQUE`: Identifiers never collide across nodes for all time.
- `I-ID-LOCAL-MONOTONIC`: Identifiers increment monotonically within their local authoritative allocator.
- `I-ID-NO-GLOBAL-ORDER`: No cross-node total ordering is required or assumed.
- `I-ID-EXHAUSTION-FAIL-CLOSED`: When `local_seq` reaches `u64::MAX`, allocation fails permanently for that `NodeId` namespace without wrap or reuse.
- `I-ID-DURABLE-ALLOCATOR-STATE`: Sequence persistence must be committed to durable storage before any issued identifier becomes externally observable.
- `I-NODE-ID-ROTATION-FORMAL`: Changing `NodeId` establishes a new cryptographic identifier namespace and trust certificate; rotation cannot be used to escape unpersisted crash state.

---

### ADR-0024-3: Authoritative vs. Observed Resource Graph State
1. **Authoritative Provider State**: Held exclusively by the local provider's `resourced`. Only the provider can commit capacity and issue Lease Tokens.
2. **Observed State (`I-GRAPH-AUTHORITY-LOCAL`)**: Remote graph entries held by consumer nodes are strictly advisory telemetry. A remote node cannot unilaterally mutate provider capacity without an authorized provider lease token.
3. **Resource Graph vs. Task DAG**: The Resource Graph is a general directed typed graph (cycles permitted for hosting/leases); the Workload Task Graph is strictly acyclic (`I-TASK-DAG-ACYCLIC`).

---

### ADR-0024-4: Capability vs. Lease Decoupling
$$\mathbf{Capability} \neq \mathbf{Lease}$$
- **Capability**: Grants unforgeable mathematical **authority** to access a resource.
- **Lease**: Grants time-bounded, metered physical **capacity** of that resource.
- **Invariant `I-LEASE-AUTH-BOUNDED`**: A lease grants capacity, never authority:
  $$\text{Lease Authority} \subseteq \text{Capability Authority}$$

---

### ADR-0024-5: Network-Realistic CSDT Revocation & Monotonic Expiration
1. CSDTs carry a bounded TTL ($\Delta T_{\text{TTL}}$).
2. **Connected Revocation**: Propagates over authenticated channels; shadow capabilities are invalidated immediately upon verified receipt.
3. **Unreachable / Partitioned Revocation (`I-REMOTE-CAP-REVOCATION-BOUNDED`)**: Under packet delay, loss, or network partition, remote execution continues strictly until TTL expiration ($\Delta t_{\text{stale}} \le \Delta T_{\text{TTL}}$). Upon expiration without a verified renewal token, shadow capabilities and leases expire fail-closed without orphan leaks.
4. **Monotonic Expiration Authority (`I-CSDT-EXPIRATION-MONOTONIC`)**: CSDT expiration is evaluated strictly against the receiving node's local monotonic hardware timer; wall-clock synchronization (NTP/PTP) is not required.

---

### ADR-0024-6: Dual-Path Network Security & Fabric Export Isolation
1. **Path A (Unprivileged Sockets)**: Standard TCP/UDP sockets via Stage 3M syscalls (`NET_SEND`). Transmits unprivileged process byte streams. Cannot access protected files or memory; cannot inject objects into remote ZeroOS capability tables (`I-FABRIC-NAMESPACE-ISOLATION`).
2. **Path B (Fabric-Mediated Export)**: Workspace files and memory buffers require explicit `CAP_NET_EXPORT` authority verified at the Capability Evaluation Layer prior to serialization (`I-DATA-EXPORT-ENFORCED`). Inbound fabric tunnels reject any payload lacking a valid CSDT signed by a trusted node key.

---

### ADR-0024-7: Energy Telemetry Authority Classification
$$\mathbf{Invariant\ I-ENERGY-MEASUREMENT-CLASSIFIED:}$$
Energy telemetry carries its authoritative tier (`MEASURED`, `ESTIMATED`, or `DECLARED`). Soft optimization planners may leverage estimates; security boundaries and hard capacity allocations must never depend on untrusted estimates.

---

### ADR-0024-8: Two-Stage Fabric Planning Model
1. **Phase 1: Hard Constraints Filter**: Prunes candidate nodes by boolean validation of capability authorization (`CAP_NET_EXPORT`), privacy policies, required hardware accelerators, memory/disk headroom, hard latency deadlines ($T_{\text{latency}} < D_{\text{hard}}$), and Identity Assurance Levels (IAL-1..3).
2. **Phase 2: Dimensionless Soft Optimization**: Optimizes normalized placement cost across eligible nodes:
   $$J(\text{Node}_i) = w_{\text{lat}} \cdot N_{\text{lat}}(i) + w_{\text{eng}} \cdot N_{\text{eng}}(i) + w_{\text{cost}} \cdot N_{\text{cost}}(i)$$

---

### ADR-0024-9: Conditional Workload Recovery & Process Migration Prohibition
1. **Conditional Recovery (`I-RECOVERY-CLASS-RESPECTED`)**: Workloads are classified into Class 1 (Pure/Idempotent $\to$ safe re-execution), Class 2 (Checkpointed Stateful $\to$ resume from ZeroFS checkpoint), and Class 3 (Irreversible External $\to$ `FailedAtMilestone` + compensation handlers; never rolled back).
2. **Process Migration Prohibition (`I-NO-LIVE-PROCESS-MIGRATION`)**: Live process/thread memory migration across network links is prohibited. Distributed computing occurs via task dispatch or checkpoint state transfer.

---

## Authoritative Invariant Catalog (22 Invariants)

1. `I-ID-UNIQUE`: Every `DistributedId` is globally non-colliding via composite `(NodeId, LocalSeq)` tuples.
2. `I-ID-LOCAL-MONOTONIC`: Identifiers increment monotonically within their authoritative local allocator.
3. `I-ID-NO-GLOBAL-ORDER`: No cross-node total ordering is required or assumed.
4. `I-ID-EXHAUSTION-FAIL-CLOSED`: When `local_seq` reaches `u64::MAX`, allocation fails permanently for that `NodeId` namespace without wrap or reuse.
5. `I-ID-DURABLE-ALLOCATOR-STATE`: For every authoritative `NodeId` namespace, sequence persistence must be committed to durable storage before any issued identifier becomes externally observable.
6. `I-NODE-ID-ROTATION-FORMAL`: Changing `NodeId` establishes a new cryptographic identifier namespace and trust certificate; rotation cannot be used to escape unpersisted crash state.
7. `I-GRAPH-AUTHORITY-LOCAL`: A node is authoritative solely over its locally hosted resources; remote graph entries are advisory.
8. `I-TASK-DAG-ACYCLIC`: Task dependency graphs within a Workload are strictly acyclic; cycles are rejected at admission.
9. `I-LEASE-AUTH-BOUNDED`: A lease cannot confer authority exceeding the authorizing capability token.
10. `I-LEASE-CLEANUP`: When a consumer process, workload, or node disconnects, all associated leases are reclaimed without leaks.
11. `I-REMOTE-CAP-NO-AMPLIFICATION`: Cross-node capability delegation enforces monotonic attenuation: $\text{ChildRights} \subseteq \text{ParentRights}$.
12. `I-REMOTE-CAP-REVOCATION-BOUNDED`: Revocation propagates immediately on active channels; under packet loss or partition, stale authorization is strictly bounded by CSDT TTL ($\Delta T_{\text{TTL}}$).
13. `I-CSDT-EXPIRATION-MONOTONIC`: CSDT expiration is evaluated strictly against the receiving node's local monotonic clock; wall-clock synchronization is not required.
14. `I-NODE-IDENTITY-ASSURANCE`: Nodes advertise certified Identity Assurance Levels (IAL-1..3); workloads enforce minimum IAL requirements.
15. `I-WORKLOAD-DISTINCT-FROM-PROCESS`: A process is an isolated local execution container; a workload is a goal-oriented computation DAG.
16. `I-NO-LIVE-PROCESS-MIGRATION`: Processes are strictly local to their host kernel instance; workload distribution occurs via task dispatch or checkpoint state migration.
17. `I-WORKSPACE-POLICY-BOUNDS`: Workloads and agents within a workspace cannot exceed the workspace's capability security envelope.
18. `I-ENERGY-POLICY-RESPECTED`: Schedulers and planners must respect declared energy policies; energy optimization must never violate capability authority, privacy, or hard deadlines.
19. `I-ENERGY-MEASUREMENT-CLASSIFIED`: Telemetry must identify whether it is Measured, Estimated, or Declared; hard limits cannot depend on untrusted estimates.
20. `I-DATA-EXPORT-ENFORCED`: The fabric transport cannot transmit data without proof of authorization verified at the capability evaluation layer (`CAP_NET_EXPORT`).
21. `I-FABRIC-NAMESPACE-ISOLATION`: Unprivileged raw socket traffic cannot create, modify, or inject objects into remote ZeroOS capability tables or workspaces.
22. `I-RECOVERY-CLASS-RESPECTED`: Workloads with irreversible external side effects are never rolled back; they transition to `FailedAtMilestone` and trigger compensation handlers.

---

## Consequences

### Positive
- Fully and definitively closes every distributed-systems contract with mathematical rigor and network reality.
- Eliminates sequence loss or reuse across reboots via crash-safe persistence ordering.
- Immunizes remote capability expiration against clock drift and NTP/PTP attacks using local monotonic hardware timers.
- Provides absolute boundary protection between unprivileged socket networking and fabric-mediated capability export.
- Fully respects the Stage 3 hard freeze with zero kernel dependency conflicts.

### Negative / Trade-offs
- Local sequence persistence requires periodic disk sync passes to ZeroFS for sequence ceiling reservations.
- Monotonic local timers require renewal intervals to be negotiated relative to receiver tick rates rather than absolute timestamps.
