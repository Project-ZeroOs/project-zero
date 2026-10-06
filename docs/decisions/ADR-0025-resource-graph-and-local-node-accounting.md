# ADR-0025: Unified Resource Graph & Local Node Accounting (`resourced`) (Rev12)

## Status
🟢 **APPROVED ARCHITECTURE — FINAL FREEZE CANDIDATE (REVISION 12)** (2026-09-22)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024 Rev6  
**Kernel Baseline:** Stage 3A–3N Inviolate (0 Bytes Modified)  
**System Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Frozen  

---

## Context

Stages 3A through 3N established an authoritative, bare-metal capability microkernel operating in 64-bit Long Mode across up to 4 SMP cores. Stage 4A implemented and verified the freestanding user-space system runtime (`libzero`), the process supervisor (`init`), and the capability-mediated service directory (`brokerd`).

Following eleven rounds of adversarial architecture review, **Revision 12** definitively closes all remaining resource safety, lifecycle, and qualification semantics:
1. **Physical Resource Safety during Time Authority Failure (`I-TIME-FAILURE-ACCOUNTING-SAFETY`)**:
   Eliminates immediate capacity overcommit. When time authority is lost, active leases transition to `TimeAuthorityLost`. Their committed capacity is transferred to $C_{\text{unavail}}$ ($C_{\text{alloc}} \leftarrow C_{\text{alloc}} - \Delta C, C_{\text{unavail}} \leftarrow C_{\text{unavail}} + \Delta C$), keeping $C_{\text{avail}}$ strictly unchanged. Capacity only returns to $C_{\text{avail}}$ after the provider confirms that physical hardware occupancy has been released, quiesced, or reset.
2. **Pure Stage 4 User-Space Adapter Lifecycle (`I-TIME-ADAPTER-LIFECYCLE`)**:
   Eliminates all claims of kernel-level process supervision ("kill all user space on PID1 exit"). The Time Authority Adapter is an in-process subsystem of `init` (PID 1) sharing its user-space lifecycle. If `init` terminates or crashes, its write capability disappears, the observation frame becomes stale ($> 50\text{ ms}$), `resourced` detects staleness, and temporal leases fail closed. Stage 3 remains 100% frozen and unmodified.
3. **Consumer Source Equivalence Contract (`I-TIME-CONSUMER-SOURCE-EQUIVALENCE`)**:
   Mandates that `resourced`'s direct hardware TSC evaluations reuse the exact same qualified source, conversion factor, serialization discipline, active `boot_epoch`, and platform prerequisite as the Time Authority Adapter.
4. **Explicit Differentiation: `Expired` vs. `TimeAuthorityLost`**:
   Codifies the semantic distinction: `Expired` indicates healthy time reached the expiration tick; `TimeAuthorityLost` indicates authoritative time cannot be established. Leases in `TimeAuthorityLost` confer zero authority, cannot be renewed, and their physical capacity remains non-allocatable until released.
5. **Strict Stage 3 Preservation (0 Bytes Modified)**:
   Stage 3 remains 100% frozen and inviolate. The `TimeObservationFrame` is backed by standard Stage 3G Shared Memory (`SYS_SHM_CREATE`, `SYS_SHM_MAP`) where `init` retains `handle_refs >= 1`, ensuring `ref_count >= 1` permanently holds without modifying Stage 3G's object table (`I-TIME-FRAME-LIFETIME`).

---

## Architectural Decisions

### ADR-0025-0: Formal Identity Hierarchy
$$\begin{aligned}
\mathbf{NodeId} &\quad (64\text{ bits}) \quad\implies\quad \text{Persistent cryptographic identity of a physical host (Hash of } K_{\text{node}}^{\text{pub}}\text{).} \\
\mathbf{BootEpochId} &\quad (64\text{ bits}) \quad\implies\quad \text{Monotonic boot incarnation counter within a specific } \text{NodeId}\text{ namespace.} \\
\mathbf{DistributedId} &\quad (128\text{ bits}) \quad\implies\quad \text{Composite tuple } (\text{NodeId}: 64\text{ bits},\; \text{LocalSeq}: 64\text{ bits})\text{ for resources and leases.}
\end{aligned}$$

$$\mathbf{Invariant\ I-BOOT-INCARNATION-UNIQUE:}\quad \text{The composite tuple } (\text{NodeId}, \text{BootEpochId}) \text{ uniquely identifies}$$
$$\text{exactly one completed kernel boot incarnation across all physical machines and all time.}$$

---

### ADR-0025-1: The Unified Resource Graph vs. Workload Task DAGs
1. **Unified Topology Modeling**:
   ZeroOS unifies heterogeneous hardware hierarchies (NUMA domains, multi-socket CPUs, discrete GPUs, NPUs, PCIe switches, storage controllers, network interfaces) into a single, typed, directed **Resource Graph**.
2. **Structural Cycle Allowance**:
   The Resource Graph models physical and logical topology. In hardware and operating system substrates, structural relationships are naturally cyclical (e.g. `Node` contains `Device`, which provides `Storage`, which backs `Swap`, which backs `RAM`, which resides in `Node`). The Resource Graph explicitly permits directed cycles.
3. **Strict Separation from Task DAGs (`I-GRAPH-SEPARATION`)**:
   $$\text{Resource Graph} \neq \text{Workload Task DAG}$$
   The Resource Graph describes hardware and capacity topology. It is **NOT** a computational workflow engine. Workload Task DAGs (managed in Phase 4C by `workloadd`) model computational task dependencies and dataflow; Workload Task DAGs are **strictly acyclic** (`I-TASK-DAG-ACYCLIC`).

---

### ADR-0025-2: Placement in Ring 3 User Space (`resourced`)
1. **Preserving the Minimal Capability Nucleus**:
   In accordance with the microkernel philosophy (ADR-0001, ADR-0024), resource management policy, topological graph modeling, multi-dimensional quota tracking, and dynamic lease heartbeats belong in **Ring 3 User Space**.
2. **Eliminating Kernel Heap Bloat**:
   Placing the Resource Graph in the kernel would require dynamic graph allocations and complex locks, violating the Stage 3 invariant of zero dynamic kernel heap allocations and the 4 MiB bootstrap window.
3. **Fault Containment**:
   If `resourced` crashes, the kernel nucleus remains fully operational. The `init` supervisor restarts `resourced` under bounded restart policies (`I-SUPERVISOR-LIFECYCLE-SPLIT`), and state is safely reconstructed without a kernel panic.

---

### ADR-0025-3: Tripartite Authority Separation
ZeroOS explicitly separates resource management into three non-overlapping authority tiers:
1. **Resource Fact Authority (Hardware Controllers / Drivers / Kernel)**:
   Authoritative for physical existence, physical hardware capacity ($C_{\text{phys}}$), operational health (`Healthy`, `Degraded`, `Faulted`), and hotplug arrival/removal. Cannot issue leases or enforce multi-consumer quotas.
2. **Resource Accounting Authority (`resourced`)**:
   Authoritative for multi-dimensional capacity bookkeeping ($C_{\text{unavail}}$, $C_{\text{allocatable}}$, $C_{\text{resv}}$, $C_{\text{alloc}}$, $C_{\text{avail}}$) and local process/service quota ceilings.
3. **Lease Authority (`resourced`)**:
   Authoritative for issuing contractual `LeaseId` tokens, verifying capability coverage, enforcing monotonic expiration deadlines, and executing automated capacity reclamation.

---

### ADR-0025-4: N-Dimensional Conservation, Coupling & Hard Dimension Bounds
1. **Hard Dimension Bound (`I-ACCOUNTING-DIMENSION-BOUNDED`)**:
   Capacity vectors support up to `MAX_ACCOUNTING_DIMENSIONS = 8`. Requests requesting $> 8$ dimensions are rejected immediately with `ZeroError::UnsupportedResourceShape`.
2. **Vector Conservation (`I-ACCOUNTING-CONSERVATION`)**:
   For every resource $R$ and for every active dimension $d \in 0..\text{dim\_count}$:
   $$C_{\text{avail}}(R, d) + C_{\text{resv}}(R, d) + C_{\text{alloc}}(R, d) + C_{\text{unavail}}(R, d) = C_{\text{phys}}(R, d)$$
   where all terms are strictly non-negative:
   $$C_{\text{avail}} \ge 0, \quad C_{\text{resv}} \ge 0, \quad C_{\text{alloc}} \ge 0, \quad C_{\text{unavail}} \ge 0$$
3. **Joint Feasibility Invariant (`I-ACCOUNTING-FEASIBILITY`)**:
   Every admitted lease must satisfy the complete resource-capacity constraint set across all dimensions simultaneously:
   $$\mathbf{A}_{\text{coupling}} \cdot (\mathbf{C}_{\text{alloc}} + \Delta \mathbf{C}) \le \mathbf{b}_{\text{coupling}}$$
   Requests violating coupled constraints (e.g. GPU queues requiring minimum VRAM) are rejected fail-closed with `ZeroError::ResourceConflict`.

---

### ADR-0025-5: Hardware Time Qualification, Freshness & Safety
$$\mathbf{Invariant\ I-TIME-AUTHORITY-MONOTONIC:}$$
$$\text{Monotonic time observation is derived from the hardware monotonic time substrate.}$$
$$\text{\texttt{resourced} strictly consumes an authenticated observation via an atomic seqlock over cache-coherent shared memory.}$$
$$\text{Wall-clock synchronization (NTP/PTP) MUST NOT be required or permitted to influence lease validity.}$$

$$\mathbf{Invariant\ I-TIME-SOURCE-QUALIFIED:}$$
$$\text{The hardware counter must pass platform qualification across availability, monotonicity, constant rate,}$$
$$\text{calibrated conversion (}\le \pm 100\text{ ppm), and virtualization parity; fails closed on failure.}$$

$$\mathbf{Invariant\ I-TIME-SMP-PREREQUISITE:}$$
$$\text{The platform profile must declare support for synchronized invariant TSC across all online CPUs;}$$
$$\text{lacking this prerequisite is unsupported for time-based leasing.}$$

$$\mathbf{Invariant\ I-TIME-SOURCE-REGRESSION:}$$
$$\text{Any observed backward movement of the hardware counter (} T_{\text{raw}} < T_{\text{last\_observed}} \text{) is a fatal fault;}$$
$$\text{adapter latches sequence odd (}\text{u64::MAX}\text{) and fails closed immediately without clamping.}$$

$$\mathbf{Invariant\ I-TIME-CONSUMER-SOURCE-EQUIVALENCE:}$$
$$\text{All direct consumer observations used for lease validity or freshness must reuse the exact same qualified}$$
$$\text{source, conversion, epoch, and SMP contract as the adapter.}$$

$$\mathbf{Invariant\ I-TIME-OBSERVATION-FRESHNESS:}$$
$$\text{The consumer must evaluate freshness against an independently current reading sampled directly}$$
$$\text{from the qualified hardware counter; staleness } > 50\text{ ms is rejected fail-closed.}$$

$$\mathbf{Invariant\ I-TIME-FAILURE-LEASE-SEMANTICS:}$$
$$\text{When monotonic time authority is lost, all active time-dependent leases immediately transition to}$$
$$\text{\texttt{TimeAuthorityLost}, capacity is held non-allocatable, and renewals/admissions are rejected.}$$

$$\mathbf{Invariant\ I-TIME-FAILURE-ACCOUNTING-SAFETY:}$$
$$\text{Invalidating a lease does NOT make its physical capacity available; capacity transfers to } C_{\text{unavail}}$$
$$\text{and becomes available only after provider confirms release or reset.}$$

$$\mathbf{Invariant\ I-TIME-ADAPTER-LIFECYCLE:}$$
$$\text{The Time Authority Adapter is an in-process subsystem of \texttt{init} (PID 1); failure makes observations stale,}$$
$$\text{causing leases to fail closed under Stage 4A service lifecycle supervision. No new kernel kill primitive is added.}$$

$$\mathbf{Invariant\ I-TIME-AUTHORITY-PROVENANCE:}$$
$$\text{Stage 3 capability write access (}\text{SHM\_WRITE}\text{) is the sole producer credential; \texttt{resourced} receives}$$
$$\text{an attenuated read-only handle; PID/generation are consistency metadata.}$$

1. **Rust Soundness**: All fields in `TimeObservationFrame` are atomic (`AtomicU64`, `AtomicU32`), eliminating data races under the Rust memory model.
2. **Standard Stage 3G Ref-Count Conformance (`I-TIME-FRAME-LIFETIME`)**: Backing physical frame is allocated via standard Stage 3G `SYS_SHM_CREATE`. `init` retains an open handle (`handle_refs >= 1`), ensuring `ref_count >= 1` permanently under Stage 3G's $\text{ref\_count} = \text{handle\_refs} + \text{mapping\_refs} + \text{in\_flight\_op\_refs}$ ABI. Closing user handles in `resourced` cannot destroy or reclaim the frame.
3. **Terminal Sequence Latch (`I-TIME-SEQUENCE-NONWRAP`)**: Sequence counter uses checked addition up to $s_{\text{max\_even}} = \text{u64::MAX} - 1$. Upon reaching this boundary, the writer transitions to `u64::MAX` (odd terminal latch) with `Release` ordering, failing subsequent reader snapshots closed with `ZeroError::TimeAuthorityUnavailable`. Counter NEVER wraps.
4. **Non-Wrapping Monotonic Deadlines (`I-TIME-DEADLINE-NONWRAP`)**: Deadlines are calculated via checked addition: $T_{\text{deadline}} = T_{\text{current}} + \Delta T_{\text{duration}}$. Overflow fails closed with `ZeroError::DeadlineExhaustion`. `0` is reserved as invalid; `u64::MAX` as exhausted.

---

### ADR-0025-6: Writer-Serialized Accounting Domains
$$\mathbf{Invariant\ I-ACCOUNTING-ATOMIC-ADMISSION:}$$
$$\text{Exactly one writer may mutate an \texttt{AccountingDomain} at a time.}$$
$$\text{Admission, reservation, allocation, release, and reclamation form indivisible state transitions}$$
$$\text{with respect to competing admissions and observers. Observers never observe intermediate states.}$$
1. Each `AccountingDomain` contains an internal writer spinlock, sequence/version counter, and N-dimensional capacity state ($N \le 8$).
2. All capacity state transitions are serialized under `writer_lock`.
3. Executed purely in user space with zero kernel locks.

---

### ADR-0025-7: Capability vs. Lease Decoupling
$$\mathbf{Capability} \neq \mathbf{Lease}$$
$$\mathbf{Invariant\ I-LEASE-AUTH-BOUNDED:}\quad \text{Lease Authority} \subseteq \text{Capability Authority}$$
- **Capability**: Grants unforgeable mathematical **authority** to interact with an object. Governed exclusively by the Stage 3 kernel CDT.
- **Lease**: Grants time-bounded, metered physical **capacity** of that object. Governed exclusively by `resourced`.
- Presenting a lease without a covering kernel capability confers zero authority.
- `resourced` rejects any lease request exceeding the caller's presented capability (`I-RES-CAPABILITY-NO-AMPLIFICATION`).

---

### ADR-0025-8: Non-Resurrecting Crash Reconciliation
$$\mathbf{Invariant\ I-LEASE-RECONCILIATION-BOUNDED:}$$
$$\forall L \in \text{SurvivingLeases}: \quad \Delta T_{\text{reconcile}}(L) = \min\left(\Delta T_{\text{policy\_max}},\; T_{\text{remaining}}(L)\right)$$
$$\text{A post-crash reconciliation window CAN NEVER exceed the lease's pre-crash remaining lifetime:}$$
$$\Delta T_{\text{reconcile}}(L) \le T_{\text{remaining}}(L)$$
$$\text{Reconciliation CAN NEVER resurrect an already-expired lease, and consumers CANNOT self-extend.}$$
1. **Re-attestation Eligibility**: Permitted strictly while $T_{\text{remaining}} > 0$.
2. **Short-Lived Leases**: If $T_{\text{remaining}} < \Delta T_{\text{policy}}$, the consumer must re-attest before its specific expiration tick elapses.
3. **Zero Self-Extension**: The reinstated lease strictly retains $L.\text{expiration\_tick}$.
4. **Purge**: Un-reconciled capacity is reclaimed as free once the reconciliation window closes.

---

### ADR-0025-9: Authoritative Hotplug Observation Boundary
$$\mathbf{Invariant\ I-RES-DISAPPEARANCE-CASCADE:}$$
$$\text{\texttt{ProviderLost} becomes authoritative at the FIRST provider-authoritative observation of disappearance}$$
$$\text{or hardware fault (} t_{\text{observed}} \text{). \texttt{resourced} transitions the resource to \texttt{Unavailable}, sets } C_{\text{unavail}} \leftarrow C_{\text{phys}},$$
$$\text{and immediately cascades all associated active leases to \texttt{ProviderLost} fail-closed.}$$
1. **Observation Boundary**: Software cannot observe physical disconnection ($t_{\text{phys}}$) with zero latency. The state transition becomes authoritative at $t_{\text{observed}}$ when the driver event or channel `PeerClosed` signal is ingested.
2. **Cascading Semantics**: Sets $C_{\text{unavail}} \leftarrow C_{\text{phys}}$, $C_{\text{allocatable}} \leftarrow 0$, and transitions all active leases on the resource to `ProviderLost`.
3. **Consumer Notification**: Consumers are notified asynchronously; hardware operations fail at the Stage 3 MMIO/bus level.

---

### ADR-0025-10: Write-Ahead Boot Epoch Durability & Non-Wrapping
$$\mathbf{Invariant\ I-BOOT-EPOCH-DURABILITY:}$$
$$\text{Before resource lease authority begins issuing capacity leases, a previously unused}$$
$$\text{BootEpochId must be durably reserved using an authoritative Boot Persistence Authority.}$$
$$\text{The Boot Persistence Authority must satisfy five mandatory criteria:}$$
$$1.\ \text{Available before lease issuance begins;}$$
$$2.\ \text{Durable across power loss and crashes;}$$
$$3.\ \text{Atomic monotonic increment: } E_{N+1} > E_N;$$
$$4.\ \text{Self-validating: detects torn or corrupt persistent state;}$$
$$5.\ \text{Fail-closed: failure to verify durable non-reuse immediately halts boot/initialization.}$$
$$\text{No Stage 3K ZeroFS or unverified user service may be required to establish the initial BootEpochId.}$$

$$\mathbf{Invariant\ I-BOOT-EPOCH-NONREUSE:}$$
$$\text{Within the lifetime of a node identity (}\text{NodeId}\text{), no two completed boot incarnations may share the same }\text{BootEpochId}.$$

$$\mathbf{Invariant\ I-BOOT-EPOCH-NONWRAP:}$$
$$\text{BootEpochId allocation strictly uses checked increment. If } E_{\text{persisted}} == \text{u64::MAX},$$
$$\text{the system fails closed and halts immediately with \texttt{ZeroError::EpochExhaustion}.}$$
$$\text{BootEpochId NEVER wraps, rolls over, or reuses epoch 0.}$$

$$\mathbf{Invariant\ I-LEASE-REBOOT-INVALIDATION:}$$
$$\text{A monotonic timestamp or lease token from a previous boot incarnation can never validate a lease}$$
$$\text{in a new boot epoch. Any lease whose recorded \texttt{boot\_epoch} does not match the active}$$
$$\text{\texttt{BootEpochId} is rejected immediately as dead.}$$

---

## Authoritative Invariant Catalog (38 Invariants)

1. `I-RES-ID-UNIQUE`: Every `ResourceId` and `LeaseId` is globally unique via composite `(NodeId, LocalSeq)` tuples.
2. `I-ID-DURABLE-ALLOCATOR-STATE`: Identifier allocator sequence ceilings must be durably committed before any identifier becomes externally observable.
3. `I-BOOT-INCARNATION-UNIQUE`: The composite tuple `(NodeId, BootEpochId)` globally identifies exactly one completed kernel boot incarnation across all machines and time.
4. `I-BOOT-EPOCH-DURABILITY`: Before resource lease authority begins issuing capacity leases, a durable monotonic `BootEpochId` must be reserved using an authoritative Boot Persistence Authority satisfying the 5 mandatory criteria; fails closed on uncertainty.
5. `I-BOOT-EPOCH-NONREUSE`: Within the lifetime of a node identity (`NodeId`), no two completed boot incarnations may share the same `BootEpochId`.
6. `I-BOOT-EPOCH-NONWRAP`: BootEpochId allocation strictly uses checked increment. If $E_{\text{persisted}} == \text{u64::MAX}$, the system halts fail-closed with `ZeroError::EpochExhaustion` and NEVER wraps.
7. `I-RES-AUTHORITY-LOCAL`: A node is authoritative solely over its locally hosted physical resources. Remote observations are advisory.
8. `I-RES-CAPABILITY-NO-AMPLIFICATION`: `resourced` cannot grant rights exceeding the authorizing capability token presented by the caller.
9. `I-LEASE-AUTH-BOUNDED`: A lease grants physical capacity, never capability authority: $\text{Lease Authority} \subseteq \text{Capability Authority}$.
10. `I-TIME-AUTHORITY-MONOTONIC`: Monotonic time observation is derived from the hardware monotonic substrate without wall-clock dependencies.
11. `I-TIME-SOURCE-QUALIFIED`: The hardware counter must pass platform qualification across availability, monotonicity, constant rate, calibration, and virtualization parity; fails closed on failure.
12. `I-TIME-SMP-PREREQUISITE`: The platform profile must declare support for synchronized invariant TSC across all online CPUs; lacking this prerequisite is unsupported for time-based leasing.
13. `I-TIME-SOURCE-REGRESSION`: Any observed backward movement of the hardware counter ($T_{\text{raw}} < T_{\text{last\_observed}}$) is a fatal fault; adapter latches odd and fails closed immediately without clamping.
14. `I-TIME-CONSUMER-SOURCE-EQUIVALENCE`: All direct consumer observations used for lease validity or freshness must reuse the exact same qualified source, conversion, epoch, and SMP contract as the adapter.
15. `I-TIME-OBSERVATION-FRESHNESS`: The consumer must evaluate freshness against an independently current reading from the qualified hardware counter; staleness $> 50\text{ ms}$ is rejected fail-closed.
16. `I-TIME-FAILURE-LEASE-SEMANTICS`: When monotonic time authority is lost, all active time-dependent leases immediately transition to `TimeAuthorityLost`, capacity is held non-allocatable, and renewals/admissions are rejected.
17. `I-TIME-FAILURE-ACCOUNTING-SAFETY`: Invalidating a lease does NOT make its physical capacity available; capacity transfers to $C_{\text{unavail}}$ and becomes available only after provider confirms release or reset.
18. `I-TIME-ADAPTER-LIFECYCLE`: The Time Authority Adapter is an in-process subsystem of `init` (PID 1); adapter failure makes observations stale, causing leases to fail closed under Stage 4A service lifecycle supervision.
19. `I-TIME-AUTHORITY-PROVENANCE`: Stage 3 capability write access (`SHM_WRITE`) is the sole producer credential; `resourced` receives an attenuated read-only handle; PID/generation are consistency metadata.
20. `I-TIME-AUTHORITY-LIVENESS`: Producer stall or death beyond $\text{MAX\_TIME\_OBSERVATION\_AGE}$ ($\le 50\text{ ms}$) triggers immediate fail-closed freezing of lease admissions and renewals.
21. `I-TIME-FRAME-WRITER-SINGLETON`: At most one execution context may write to `TimeObservationFrame` at a time. The writer executes strictly within `init`'s Time Authority Adapter.
22. `I-TIME-FRAME-LIFETIME`: `TimeObservationFrame` is backed by standard Stage 3G Shared Memory. `init` retains `handle_refs >= 1`, ensuring `ref_count >= 1` permanently.
23. `I-TIME-SEQUENCE-NONWRAP`: Sequence counter uses checked addition up to $s_{\text{max\_even}} = \text{u64::MAX} - 1$; transitions to `u64::MAX` terminal latch, failing readers closed. Counter NEVER wraps.
24. `I-TIME-DEADLINE-NONWRAP`: Deadline arithmetic must use checked addition. If $T_{\text{current}} + \Delta T_{\text{duration}} > \text{u64::MAX} - 1$, lease admission fails closed.
25. `I-LEASE-REBOOT-INVALIDATION`: A monotonic timestamp or lease token from a previous boot incarnation can never validate a lease in a new boot epoch.
26. `I-LEASE-CLEANUP`: When a consumer process terminates or disconnects, all associated leases are reclaimed without leaks.
27. `I-LEASE-STALE-REJECTION`: Any operation presenting a stale lease generation counter is rejected fail-closed with `GenerationMismatch`.
28. `I-ACCOUNTING-NONNEGATIVE`: All capacity terms ($C_{\text{phys}}, C_{\text{unavail}}, C_{\text{allocatable}}, C_{\text{resv}}, C_{\text{alloc}}, C_{\text{avail}}$) are strictly non-negative across all dimensions.
29. `I-ACCOUNTING-CONSERVATION`: For every resource and dimension, $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} = C_{\text{phys}}$ holds invariant across all operations.
30. `I-ACCOUNTING-FEASIBILITY`: Every admitted lease must satisfy the complete resource-capacity constraint set of its provider across all dimensions simultaneously.
31. `I-ACCOUNTING-DIMENSION-BOUNDED`: Maximum accounting dimensions per resource is strictly bounded at $\text{MAX\_ACCOUNTING\_DIMENSIONS} = 8$; larger requests rejected with `UnsupportedResourceShape`.
32. `I-ACCOUNTING-ATOMIC-ADMISSION`: Exactly one writer may mutate an `AccountingDomain` at a time under `writer_lock`. Transitions are indivisible.
33. `I-LEASE-RECONCILIATION-BOUNDED`: For all surviving leases, $\Delta T_{\text{reconcile}}(L) \le T_{\text{remaining}}(L)$. Reconciliation cannot resurrect expired leases or self-extend.
34. `I-RES-DISAPPEARANCE-CASCADE`: `ProviderLost` becomes authoritative at the first provider-authoritative observation of disappearance ($t_{\text{observed}}$), cascading leases fail-closed.
35. `I-RES-ENERGY-LIMITS`: Hard resource limits and safety triggers cannot depend on untrusted `ESTIMATED` or `DECLARED` energy telemetry.
36. `I-QUOTA-BOUNDED`: Total capacity leased by an entity cannot exceed its provisioned quota ceiling.
37. `I-GRAPH-SEPARATION`: The Resource Graph models physical and logical topology and may contain cycles; the Workload Task DAG is strictly acyclic.
38. `I-STATIC-BOUNDS`: All internal tables in `resourced` have statically fixed maximum capacities with zero unbounded dynamic heap allocation.

---

## Consequences

### Positive
- **Guaranteed Physical Resource Safety**: Transferring capacity from $C_{\text{alloc}}$ to $C_{\text{unavail}}$ upon time authority loss prevents physical hardware overcommit while workloads are still releasing resources.
- **Pure Stage 4 Lifecycle**: Conforms strictly to Stage 4A `init` service supervision without inventing non-existent Stage 3 kernel kill primitives.
- **Consumer Source Equivalence**: Guarantees that independent hardware freshness evaluations reuse the exact same qualified time substrate.
- **Independent Freshness Evaluation**: Eliminates circular self-attestation; `resourced` samples the qualified counter directly.
- **Clear Semantic Differentiation**: Explicit boundary between normal expiration (`Expired`) and undetermined time loss (`TimeAuthorityLost`).
- **Zero Kernel Mutation**: Fully preserves frozen Stage 3 ABIs and Stage 4A service runtimes without modifying a single byte of kernel code.

### Negative / Trade-offs
- **Fail-Closed Lease Invalidation**: When monotonic time authority is lost, active leases immediately lose authority and capacity is quarantined in $C_{\text{unavail}}$, prioritizing hardware safety over transient lease availability.
- **Sub-Resource Decomposition**: Peripherals requiring $> 8$ dimensions must be partitioned into linked sub-resources in the Resource Graph.
