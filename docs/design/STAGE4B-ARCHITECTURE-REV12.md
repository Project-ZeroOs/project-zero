# STAGE 4B ARCHITECTURE SPECIFICATION (REV 12)

## Unified Resource Graph & Local Node Accounting (`resourced`)

**Status:** 🟢 FINAL ARCHITECTURAL SPECIFICATION — APPROVED FREEZE CANDIDATE (REVISION 12)  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-09-22  
**Target Milestone:** Stage 4B (Unified Resource Graph & Local Node Accounting)  
**Parent Specifications:** Stage 4 Architecture Rev6 (`STAGE4-ARCHITECTURE-REV6.md`), ADR-0024 Rev6  
**Kernel Baseline:** Stage 3A–3N Hard Frozen & Inviolate (0 Bytes Modified)  
**System Service Baseline:** Stage 4A (`init`, `brokerd`, `libzero`) Implemented, Verified, Frozen  

---

## 1. Executive Summary of Revision 12 Finalization

Following the eleventh round of adversarial architecture review, **Revision 12** definitively closes the final four precision contracts for Stage 4B:

1. **Physical Resource Safety during Time Authority Failure (`I-TIME-FAILURE-ACCOUNTING-SAFETY`)**:
   Eliminates the dangerous overcommit vulnerability of immediate capacity reclamation. Invalidating a lease authority does NOT instantly free physical capacity. When time authority fails, active leases enter `LeaseState::TimeAuthorityLost`. To prevent over-admission while workloads may still be physically executing, committed capacity is transitioned to $C_{\text{unavail}}$ ($C_{\text{alloc}} \leftarrow C_{\text{alloc}} - \Delta C, C_{\text{unavail}} \leftarrow C_{\text{unavail}} + \Delta C$). Available capacity ($C_{\text{avail}}$) remains strictly unchanged. Physical capacity only returns to $C_{\text{avail}}$ after the provider confirms that physical hardware occupancy has been released, quiesced, or forcibly reset.
2. **Pure Stage 4 User-Space Adapter Lifecycle (`I-TIME-ADAPTER-LIFECYCLE`)**:
   Eliminates all claims of kernel-level process supervision ("kill all user space on PID1 exit"). The Time Authority Adapter shares `init`'s user-space lifecycle. If `init` crashes, its write authority disappears, the frame exceeds `MAX_TIME_OBSERVATION_AGE`, `resourced` detects staleness, and temporal leases fail closed. Stage 3 remains 100% frozen and unmodified.
3. **Consumer Source Equivalence Contract (`I-TIME-CONSUMER-SOURCE-EQUIVALENCE`)**:
   Mandates that `resourced`'s direct hardware TSC evaluations reuse the exact same qualified source, conversion factor, serialization discipline, active `boot_epoch`, and platform prerequisite as the Time Authority Adapter. Freshness evaluations against unqualified counters are strictly rejected.
4. **Explicit Boundary between `Expired` and `TimeAuthorityLost`**:
   Formally codifies the semantic distinction:
   - `Expired`: Authoritative time is healthy and valid; the lease reached its expiration tick ($T \ge L.\text{expiration\_tick}$).
   - `TimeAuthorityLost`: Authoritative time cannot be verified; ZeroOS does not know what time it is. The lease confers zero authority, cannot be renewed, and its physical capacity remains non-allocatable until confirmed released.

---

## 2. Scope

Phase 4B defines the architecture, data structures, invariants, failure contracts, and interfaces for **Local Node Resource Management** in ZeroOS:
1. The **Canonical Multi-Dimensional Resource Model** representing all physical and logical execution resources (CPU, GPU, NPU, RAM, Storage, Network, Peripherals, Sensors, Energy).
2. The **Unified Resource Graph** capturing physical topology, provider relationships, containment hierarchies, and lease attachments on a single node.
3. The **Local Node Accounting Engine** maintaining hard conservation and joint feasibility across all resource dimensions.
4. The **Resource Lease Contract** providing time-bounded, capability-authorized, auto-reclaiming capacity allocations.
5. The **`resourced` Daemon** running as an unprivileged, capability-constrained Ring 3 system service supervised by `init` and discovered via `brokerd`.

Phase 4B is strictly **local node infrastructure**. It establishes the ground-truth resource substrate upon which higher-level workload dispatch (4C), workspace context (4D), and autonomous agents (4E) will execute.

---

## 3. Goals & Non-Goals

### 3.1 Goals
1. **Generic Resource Abstraction**: Uniform, typed, mathematically bounded abstraction for heterogeneous hardware without erasing hardware-specific constraints.
2. **Strict Authority Decoupling**: Absolute separation of Capability Authority, Accounting Authority, and Lease Authority.
3. **Local Node Self-Sovereignty**: A node is the sole authoritative source of truth for its own physical capacity, telemetry, and leases (`I-GRAPH-AUTHORITY-LOCAL`).
4. **Multi-Dimensional Conservation & Joint Feasibility**: Strict conservation and coupled constraint satisfaction across discrete physical dimensions with zero capacity leaks.
5. **Deterministic Bounded Reclamation**: Consumer crash, provider failure, or TTL expiration automatically reclaims capacity fail-closed (`I-LEASE-CLEANUP`).
6. **Zero Kernel Mutation**: Consume frozen Stage 3 syscalls, handle tables, and capability models without modifying a single byte of the kernel nucleus.
7. **Zero Dynamic Memory Allocation**: Enforce static tables, bounded arrays, and deterministic memory limits in `#![no_std]` userspace.

### 3.2 Non-Goals
1. **NOT Workload Orchestration (Deferred to Phase 4C)**: `resourced` does NOT parse workload manifests, construct task execution plans, or schedule computational jobs.
2. **NOT Task DAG Scheduling (Deferred to Phase 4C)**: The Resource Graph describes what hardware exists and how it is connected; it is NOT a directed acyclic graph (DAG) of task dependencies.
3. **NOT Distributed Mesh Offloading (Deferred to Phase 4C)**: Phase 4B does not implement peer-to-peer Noise tunnels, remote capability delegation (CSDTs), or multi-node consensus.
4. **NOT Workspace Container Management (Deferred to Phase 4D)**: `resourced` does not manage user workspaces, persistent contexts, or visual surfaces.
5. **NOT Agent Perception or Intent Resolution (Deferred to Phase 4E/4F)**: `resourced` does not interpret human intent or optimize multi-variable utility functions.
6. **NOT Live Process Migration**: Precluded by Stage 4 Rev6 invariant `I-NO-LIVE-PROCESS-MIGRATION`.

---

## 4. Formal Identity Hierarchy

To eliminate identity ambiguity across local and distributed domains, ZeroOS establishes a three-tier identity hierarchy:

$$\begin{aligned}
\mathbf{NodeId} &\quad (64\text{ bits}) \quad\implies\quad \text{Persistent cryptographic identity of a physical host (Hash of } K_{\text{node}}^{\text{pub}}\text{).} \\
\mathbf{BootEpochId} &\quad (64\text{ bits}) \quad\implies\quad \text{Monotonic boot incarnation counter within a specific } \text{NodeId}\text{ namespace.} \\
\mathbf{DistributedId} &\quad (128\text{ bits}) \quad\implies\quad \text{Composite tuple } (\text{NodeId}: 64\text{ bits},\; \text{LocalSeq}: 64\text{ bits})\text{ for resources and leases.}
\end{aligned}$$

$$\mathbf{Invariant\ I-BOOT-INCARNATION-UNIQUE:}\quad \text{The composite tuple } (\text{NodeId}, \text{BootEpochId}) \text{ uniquely identifies}$$
$$\text{exactly one completed kernel boot incarnation across all physical machines and all time.}$$

---

## 5. Tripartite Authority Model

```text
+-----------------------------------------------------------------------------+
|                          RESOURCE FACT AUTHORITY                            |
|             (Hardware Device Controllers, Device Drivers, Kernel)           |
|                                                                             |
|  * Physical existence & discovery of hardware peripherals                   |
|  * Physical upper bound capacity: C_phys(R, d)                              |
|  * Hardware operational state (Healthy, Degraded, Faulted, Resetting)       |
|  * Physical hotplug events (Arrival, Surprise Removal, Power Rail Off)      |
|  * Low-level MMIO aperture mapping & DMA buffer pinning                     |
+--------------------------------------┬--------------------------------------+
                                       │ Hardware Facts & State Changes
+--------------------------------------v--------------------------------------+
|                        RESOURCE ACCOUNTING AUTHORITY                        |
|                                 (resourced)                                 |
|                                                                             |
|  * Multi-dimensional capacity vector accounting (C_unavail, C_allocatable)  |
|  * Joint feasibility evaluation across coupled constraints (I-FEASIBILITY)  |
|  * Reservation headroom accounting (C_resv)                                 |
|  * Committed capacity tracking (C_alloc)                                    |
|  * Available admission headroom: C_avail = C_allocatable - (C_resv+C_alloc) |
|  * Local per-process and per-service quota enforcement                      |
+--------------------------------------┬--------------------------------------+
                                       │ Capacity Verification & Commitment
+--------------------------------------v--------------------------------------+
|                               LEASE AUTHORITY                               |
|                                 (resourced)                                 |
|                                                                             |
|  * Verification that Lease Authority ⊆ Capability Authority                 |
|  * Issuance of unique, monotonic LeaseId tokens with generation counters    |
|  * Bounded temporal lifespan tracking against monotonic kernel ticks        |
|  * Renewal, revocation, and deterministic reclamation (I-LEASE-CLEANUP)      |
|  * Crash reconciliation protocol & orphan evacuation                        |
+-----------------------------------------------------------------------------+
```

---

## 6. Monotonic Time Observation, Freshness & Safety

### 6.1 Architectural Substrate & Trust Boundary

```text
+-------------------------------------------------------------------------+
|                  PLATFORM-QUALIFIED HARDWARE TIME SOURCE                |
|       (Invariant TSC, CR4.TSD = 0, Calibrated Rate, Platform Declared)  |
+------------------------------------┬------------------------------------+
                                     │ Unprivileged RDTSCP / LFENCE; RDTSC
                 ┌───────────────────┴───────────────────┐
                 │                                       │
                 ▼                                       ▼
+---------------------------------+     +---------------------------------+
|   init TIME AUTHORITY SUBSYSTEM |     |    resourced FRESHNESS VERIFIER |
|             (PID 1)             |     |                                 |
|                                 |     | * Samples hardware TSC directly |
| * Holds SHM_WRITE Capability    |     | * Computes independent T_eval   |
| * Publishes frame (~10 ms)      |     | * Verifies age <= 50 ms         |
| * Retains handle (ref_count>=1) |     | * Enforces safe reclamation     |
+----------------┬----------------+     +----------------┬----------------+
                 │                                       ▲
                 │ SYS_CAP_DERIVE (Attenuated SHM_READ)  │
                 └───────────────────────────────────────┘
```

### 6.2 Hardware Time Source Qualification Contract
$$\mathbf{Invariant\ I-TIME-SOURCE-QUALIFIED:}$$
$$\text{Before the Time Authority Adapter becomes authoritative, the underlying hardware time source}$$
$$\text{MUST satisfy five mandatory qualification requirements:}$$
$$1.\ \mathbf{Availability:}\text{ Readable in Ring 3 from every CPU that may execute the adapter without faulting (}\text{CR4.TSD} = 0\text{);}$$
$$2.\ \mathbf{Monotonicity:}\text{ Successive hardware readings MUST NOT regress across any execution path;}$$
$$3.\ \mathbf{Invariance:}\text{ Rate is invariant across CPU P-states, C-states, and thermal throttling (}\text{CPUID.80000007H:EDX}[8]\text{);}$$
$$4.\ \mathbf{Calibration:}\text{ Exact conversion to calibrated 100 Hz (10 ms) monotonic ticks with bounded error } \le \pm 100\text{ ppm;}$$
$$5.\ \mathbf{Virtualization Parity:}\text{ Virtual TSC scaling/offsets preserve monotonicity without backward jumps.}$$
$$\text{If qualification fails, the adapter fails closed and lease issuance is halted (\texttt{ZeroError::TimeAuthorityUnavailable}).}$$

### 6.3 Platform Prerequisite for SMP Invariant TSC
$$\mathbf{Invariant\ I-TIME-SMP-PREREQUISITE:}$$
$$\text{The ZeroOS platform profile MUST formally declare whether invariant, cross-CPU-synchronized TSC is supported:}$$
$$|\text{TSC}(CPU_i) - \text{TSC}(CPU_j)| \le \epsilon_{\text{sync}} \ll 10\text{ ms.}$$
$$\text{A platform lacking this verified hardware prerequisite is unsupported for authoritative time-based leasing.}$$
$$\text{Runtime regression detection (\texttt{I-TIME-SOURCE-REGRESSION}) remains a defensive fault detector,}$$
$$\text{not proof of platform qualification.}$$

### 6.4 Strict Monotonicity & Fatal Regression Rejection
$$\mathbf{Invariant\ I-TIME-SOURCE-REGRESSION:}$$
$$\text{Once qualified, any observed backward movement of the authoritative hardware counter (} T_{\text{raw}} < T_{\text{last\_observed}} \text{)}$$
$$\text{is a fatal, non-recoverable time-source fault.}$$
$$\text{The Time Authority Adapter MUST NOT clamp, compensate, or continue publishing time after detecting regression.}$$
$$\text{The adapter transitions sequence to } \text{SEQUENCE\_TERMINAL\_LATCH} = \text{u64::MAX} \text{ and halts publication.}$$
$$\text{All subsequent lease admissions, renewals, and temporal validations fail closed with \texttt{ZeroError::TimeAuthorityUnavailable}.}$$

### 6.5 Consumer Source Equivalence
$$\mathbf{Invariant\ I-TIME-CONSUMER-SOURCE-EQUIVALENCE:}$$
$$\text{All direct consumer time observations used for lease validity or observation freshness MUST use the}$$
$$\text{exact same qualified monotonic source, conversion factor, active boot epoch, and SMP platform contract}$$
$$\text{as the Time Authority Adapter. An observation from an unqualified time source is invalid fail-closed.}$$

### 6.6 Independent Freshness Evaluation
$$\mathbf{Invariant\ I-TIME-OBSERVATION-FRESHNESS:}$$
$$\text{The consumer (\texttt{resourced}) MUST evaluate observation freshness against an independently current reading}$$
$$\text{sampled directly from the qualified hardware monotonic time source (via unprivileged \texttt{RDTSCP} / \texttt{LFENCE; RDTSC}).}$$
$$\text{A timestamp contained solely within the observation being validated MUST NOT be used as its own freshness authority.}$$
$$\text{An observation is valid ONLY while staleness satisfies: } \Delta T_{\text{staleness}} = T_{\text{hardware\_eval}} - T_{\text{frame\_ticks}} \le \text{MAX\_TIME\_OBSERVATION\_AGE}$$
$$\text{where } \text{MAX\_TIME\_OBSERVATION\_AGE} \le 50\text{ ms (5 ticks). If staleness exceeds 50 ms, the observation is rejected fail-closed.}$$

### 6.7 Deterministic Failure Semantics for Existing Leases
$$\mathbf{Invariant\ I-TIME-FAILURE-LEASE-SEMANTICS:}$$
$$\text{When authoritative monotonic time becomes unavailable (hardware regression, terminal sequence latch, or stall } > 50\text{ ms):}$$
$$1.\ \text{No new time-bounded leases may be admitted (\texttt{ZeroError::TimeAuthorityLost});}$$
$$2.\ \text{No active lease may be renewed or extended;}$$
$$3.\ \text{All existing active time-dependent leases IMMEDIATELY transition to \texttt{LeaseState::TimeAuthorityLost};}$$
$$4.\ \text{Leases in \texttt{TimeAuthorityLost} confer ZERO capacity authority and cannot be evaluated as valid;}$$
$$5.\ \text{Physical capacity is reclaimed strictly under the safety rules of \texttt{I-TIME-FAILURE-ACCOUNTING-SAFETY}.}$$

### 6.8 Resource Accounting Safety during Time Authority Failure
$$\mathbf{Invariant\ I-TIME-FAILURE-ACCOUNTING-SAFETY:}$$
$$\text{Invalidating a lease authority MUST NOT by itself make its physical capacity available for new admission.}$$
$$\text{When a lease transitions to \texttt{TimeAuthorityLost}, its committed capacity is transferred to } C_{\text{unavail}}\text{:}$$
$$C_{\text{alloc}} \leftarrow C_{\text{alloc}} - \Delta C, \quad C_{\text{unavail}} \leftarrow C_{\text{unavail}} + \Delta C$$
$$\text{Available admission capacity } C_{\text{avail}} \text{ remains strictly unchanged.}$$
$$\text{Capacity may decrease from } C_{\text{unavail}} \text{ and become available (} C_{\text{avail}} \leftarrow C_{\text{avail}} + \Delta C \text{) ONLY after the provider}$$
$$\text{or driver confirms that the previously committed capacity has been fully released, quiesced, or reset.}$$
$$\text{No time-authority failure may cause accounting to over-admit physical resources.}$$

### 6.9 `init`-Owned Time Authority Subsystem Lifecycle
$$\mathbf{Invariant\ I-TIME-ADAPTER-LIFECYCLE:}$$
$$\text{The Time Authority Adapter is an integral, in-process subsystem of \texttt{init} (PID 1), sharing its user-space lifecycle.}$$
$$\text{If \texttt{init} terminates or crashes, its write capability disappears, the observation frame becomes stale,}$$
$$\text{\texttt{resourced} enters \texttt{TimeAuthorityLost}, and temporal leases fail closed.}$$
$$\text{Existing Stage 4A service lifecycle supervision governs process restart.}$$
$$\text{No new kernel-wide "kill all user space on PID1 exit" semantic is introduced by Phase 4B.}$$

### 6.10 Capability-Based Provenance & Consistency Metadata
$$\mathbf{Invariant\ I-TIME-AUTHORITY-PROVENANCE:}$$
$$\text{The Stage 3 capability granting write access (}\text{SHM\_WRITE}\text{) to the \texttt{TimeObservationFrame} is the}$$
$$\text{authoritative producer credential. No user-space process lacking this capability can mutate the frame.}$$
$$\text{\texttt{init} derives a strictly attenuated read-only capability (}\text{SHM\_READ}\text{) for \texttt{resourced}.}$$
$$\text{Fields \texttt{producer\_pid} and \texttt{producer\_generation} are consistency metadata used to verify}$$
$$\text{expected producer incarnation alignment across restarts; they are not authentication credentials.}$$

---

### 6.11 The Rust-Sound Memory-Ordered Observation Contract

```rust
#[repr(C)]
#[derive(Debug)]
pub struct TimeObservationFrame {
    pub sequence: core::sync::atomic::AtomicU64,            // 8 bytes: Even = quiescent; Odd = write in progress
    pub boot_epoch: core::sync::atomic::AtomicU64,          // 8 bytes: Monotonically non-colliding boot ID
    pub monotonic_ticks: core::sync::atomic::AtomicU64,     // 8 bytes: Calibrated 100 Hz counter (10 ms)
    pub frequency_hz: core::sync::atomic::AtomicU32,        // 4 bytes: Fixed 100 Hz
    pub producer_generation: core::sync::atomic::AtomicU32, // 4 bytes: Incarnation generation metadata
    pub producer_pid: core::sync::atomic::AtomicU64,        // 8 bytes: Supervisor PID metadata (always 1)
    pub _reserved: [u64; 3],                                // 24 bytes: Pad to exactly 64 bytes
}

const _: () = assert!(core::mem::size_of::<TimeObservationFrame>() == 64);
const _: () = assert!(core::mem::align_of::<TimeObservationFrame>() == 8);

pub const SEQUENCE_MAX_VALID_EVEN: u64 = u64::MAX - 1; // 0xFFFF_FFFF_FFFF_FFFE (Even)
pub const SEQUENCE_TERMINAL_LATCH: u64 = u64::MAX;     // 0xFFFF_FFFF_FFFF_FFFF (Odd)
```

#### Writer Protocol (Executed strictly by `init` holding `SHM_WRITE`):
```rust
pub fn update_time_frame(
    frame: &TimeObservationFrame,
    current_epoch: u64,
    next_tick: u64,
    last_observed_tick: &mut u64,
    generation: u32,
) -> Result<(), ZeroError> {
    // 1. Strict Regression Check (I-TIME-SOURCE-REGRESSION)
    if next_tick < *last_observed_tick {
        frame.sequence.store(SEQUENCE_TERMINAL_LATCH, core::sync::atomic::Ordering::Release);
        return Err(ZeroError::TimeAuthorityUnavailable);
    }
    *last_observed_tick = next_tick;

    let s = frame.sequence.load(core::sync::atomic::Ordering::Relaxed);
    
    // 2. Terminal Sequence Exhaustion Check (I-TIME-SEQUENCE-NONWRAP)
    if s >= SEQUENCE_MAX_VALID_EVEN {
        frame.sequence.store(SEQUENCE_TERMINAL_LATCH, core::sync::atomic::Ordering::Release);
        return Err(ZeroError::TimeAuthorityUnavailable);
    }
    
    // 3. Publish odd sequence with Release indicating mutation start
    frame.sequence.store(s + 1, core::sync::atomic::Ordering::Release);
    
    // 4. Atomic relaxed payload updates
    frame.boot_epoch.store(current_epoch, core::sync::atomic::Ordering::Relaxed);
    frame.monotonic_ticks.store(next_tick, core::sync::atomic::Ordering::Relaxed);
    frame.frequency_hz.store(100, core::sync::atomic::Ordering::Relaxed);
    frame.producer_generation.store(generation, core::sync::atomic::Ordering::Relaxed);
    frame.producer_pid.store(1, core::sync::atomic::Ordering::Relaxed);
    
    // 5. Publish even sequence with Release indicating stable snapshot
    frame.sequence.store(s + 2, core::sync::atomic::Ordering::Release);
    Ok(())
}
```

#### Reader Protocol (`libzero` / `resourced` in Ring 3):
```rust
pub fn read_monotonic_time_verified(
    frame: &TimeObservationFrame,
    expected_epoch: u64,
    expected_generation: u32,
    hardware_current_tick: u64, // Sampled directly via qualified RDTSCP by caller
) -> Result<(u64, u64), ZeroError> {
    let mut spins = 0;
    loop {
        // 1. Acquire load of initial sequence
        let s1 = frame.sequence.load(core::sync::atomic::Ordering::Acquire);
        if s1 & 1 != 0 {
            spins += 1;
            if spins > 16 { return Err(ZeroError::TimeAuthorityUnavailable); }
            core::hint::spin_loop();
            continue;
        }

        // 2. Atomic relaxed payload loads
        let epoch = frame.boot_epoch.load(core::sync::atomic::Ordering::Relaxed);
        let ticks = frame.monotonic_ticks.load(core::sync::atomic::Ordering::Relaxed);
        let prod_pid = frame.producer_pid.load(core::sync::atomic::Ordering::Relaxed);
        let prod_gen = frame.producer_generation.load(core::sync::atomic::Ordering::Relaxed);

        // 3. Acquire load of terminating sequence
        let s2 = frame.sequence.load(core::sync::atomic::Ordering::Acquire);
        if s1 == s2 {
            // Consistency validation
            if epoch != expected_epoch || prod_pid != 1 || prod_gen != expected_generation {
                return Err(ZeroError::TimeAuthorityUnavailable);
            }
            // Independent Freshness Validation (I-TIME-OBSERVATION-FRESHNESS)
            if hardware_current_tick < ticks || hardware_current_tick - ticks > 5 {
                return Err(ZeroError::TimeObservationStale);
            }
            return Ok((epoch, ticks));
        }

        spins += 1;
        if spins > 16 { return Err(ZeroError::TimeAuthorityUnavailable); }
        core::hint::spin_loop();
    }
}
```

### 6.12 Shared Memory Frame Lifetime & Stage 3G ABI Conformance
$$\mathbf{Invariant\ I-TIME-FRAME-LIFETIME:}$$
$$\text{The \texttt{TimeObservationFrame} is backed by a standard Stage 3G Shared Memory segment.}$$
$$\text{\texttt{init} holds an open handle (}\text{handle\_refs} \ge 1\text{), guaranteeing }\text{ref\_count} \ge 1\text{ under Stage 3G reference accounting.}$$
$$\text{Termination, restart, or handle closure by \texttt{resourced} CANNOT destroy or reclaim the backing frame.}$$
$$\text{Stage 3G's existing }\text{ref\_count} = \text{handle\_refs} + \text{mapping\_refs} + \text{in\_flight\_op\_refs}\text{ ABI remains 100\% unmodified.}$$

### 6.13 Boot Persistence Authority & Epoch Durability Contract
$$\mathbf{Invariant\ I-BOOT-EPOCH-DURABILITY:}$$
$$\text{Before \texttt{resourced} or any Stage 4 service issues resource leases, a previously unused}$$
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

### 6.14 Sequence Non-Wrapping Invariant & Terminal Latch Arithmetic
$$\mathbf{Invariant\ I-TIME-SEQUENCE-NONWRAP:}$$
$$\text{The sequence counter of \texttt{TimeObservationFrame} strictly uses checked addition.}$$
$$\text{The maximum valid published quiescent state is } \text{SEQUENCE\_MAX\_VALID\_EVEN} = \text{u64::MAX} - 1.$$
$$\text{Upon reaching this boundary, the writer transitions the sequence to } \text{SEQUENCE\_TERMINAL\_LATCH} = \text{u64::MAX},$$
$$\text{freezing the frame permanently in an odd (mutation latch) state.}$$
$$\text{All subsequent reader snapshots fail closed with \texttt{ZeroError::TimeAuthorityUnavailable}.}$$
$$\text{The sequence counter NEVER wraps to 0.}$$

### 6.15 Non-Wrapping Monotonic Deadline Arithmetic
$$\mathbf{Invariant\ I-TIME-DEADLINE-NONWRAP:}$$
$$\text{Deadline arithmetic MUST use checked addition. If } T_{\text{current}} + \Delta T_{\text{duration}} > \text{u64::MAX} - 1,$$
$$\text{lease admission fails closed immediately with \texttt{ZeroError::DeadlineExhaustion}.}$$
$$\text{Deadline arithmetic NEVER wraps. Deadlines NEVER roll over to 0.}$$

- `0` is explicitly reserved as `DEADLINE_INVALID` (uninitialized/free lease slot).
- `u64::MAX` is explicitly reserved as `DEADLINE_EXHAUSTED`.
- Valid operational deadlines reside strictly in `1 ..= u64::MAX - 1`.

---

## 7. Multi-Dimensional Vector Model & Joint Feasibility

### 7.1 N-Dimensional Vector Conservation & Hard Dimension Bound
$$\mathbf{Invariant\ I-ACCOUNTING-DIMENSION-BOUNDED:}$$
$$\text{The maximum number of accounting dimensions per resource is strictly bounded at } \text{MAX\_ACCOUNTING\_DIMENSIONS} = 8.$$
$$\text{Any registration requesting } 0 \text{ or } > 8 \text{ dimensions MUST be rejected fail-closed with \texttt{UnsupportedResourceShape}.}$$

```rust
pub const MAX_ACCOUNTING_DIMENSIONS: usize = 8;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DimensionCapacityVector {
    pub dimension_count: u8,
    pub _pad: [u8; 7],
    pub phys: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub unavail: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub alloc: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub resv: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub consumed: [u32; MAX_ACCOUNTING_DIMENSIONS],
}
```

$$\mathbf{Invariant\ I-ACCOUNTING-CONSERVATION:}$$
$$\forall R, \forall d \in 0..\text{dimension\_count}(R): \quad C_{\text{avail}}(R, d) + C_{\text{resv}}(R, d) + C_{\text{alloc}}(R, d) + C_{\text{unavail}}(R, d) = C_{\text{phys}}(R, d)$$
Every term is strictly non-negative:
$$C_{\text{avail}} \ge 0, \quad C_{\text{resv}} \ge 0, \quad C_{\text{alloc}} \ge 0, \quad C_{\text{unavail}} \ge 0$$

### 7.2 Joint Multidimensional Feasibility Invariant
$$\mathbf{Invariant\ I-ACCOUNTING-FEASIBILITY:}$$
$$\text{Every admitted lease MUST satisfy the complete resource-capacity constraint set of its provider}$$
$$\text{across all dimensions simultaneously. No dimension may be individually valid while the combined}$$
$$\text{allocation violates a provider-defined coupling constraint: } \mathbf{A}_{\text{coupling}} \cdot (\mathbf{C}_{\text{alloc}} + \Delta \mathbf{C}) \le \mathbf{b}_{\text{coupling}}.$$

### 7.3 Atomic Accounting Domain Transitions
$$\mathbf{Invariant\ I-ACCOUNTING-ATOMIC-ADMISSION:}$$
$$\text{Exactly one writer may mutate an \texttt{AccountingDomain} at a time.}$$
$$\text{Admission, reservation, allocation, release, and reclamation form indivisible state transitions}$$
$$\text{with respect to competing admissions and observers. Observers never observe intermediate states.}$$

```rust
#[repr(C)]
pub struct AccountingDomain {
    /// 1. Writer spinlock ensuring single-writer mutual exclusion
    pub writer_lock: crate::sync::Spinlock,
    /// 2. Sequence counter for read versioning (odd = write in progress; even = quiescent)
    pub version: core::sync::atomic::AtomicU64,
    /// 3. Resource identity binding
    pub resource_id: DistributedId,
    /// 4. Provider joint constraint coupling profile ID
    pub coupling_profile_id: u8,
    pub _pad: [u8; 7],
    /// 5. Full N-dimensional capacity vector (up to MAX_ACCOUNTING_DIMENSIONS)
    pub capacity: DimensionCapacityVector,
}
```

---

## 8. Resource Descriptor Layout (Static 128 Bytes)

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceDescriptor {
    // Identity & Provenance (32 Bytes)
    pub resource_id: DistributedId,     // 16 bytes: (NodeId, LocalSeq)
    pub provider_pid: u64,              // 8 bytes: Process ID of authoritative driver
    pub generation: u32,                // 4 bytes: Slot reuse generation
    pub res_type: ResourceType,         // 1 byte: Canonical type enum
    pub locality_node: u8,              // 1 byte: NUMA domain / CPU affinity socket
    pub coupling_profile_id: u8,        // 1 byte: Provider-defined joint constraint profile
    pub active_dimensions: u8,          // 1 byte: Number of active dimensions (1..8)

    // Operational State & Health (16 Bytes)
    pub state: ResourceLifecycleState,  // 1 byte: Available, Degraded, Detaching, etc.
    pub health: ResourceHealth,         // 1 byte: Healthy, Degraded, Critical, Faulted
    pub energy_tier: EnergyTier,        // 1 byte: Measured, Estimated, Declared
    pub thermal_state: ThermalState,    // 1 byte: Nominal, Throttling, Critical
    pub required_rights: u16,           // 2 bytes: Stage 3 capability rights mask
    pub active_leases_count: u16,       // 2 bytes: Number of active concurrent leases
    pub bound_kernel_handle: u32,       // 4 bytes: Process-local handle to kernel object
    pub _pad1: [u8; 4],                 // 4 bytes: Alignment padding

    // Multi-Dimensional Primary Dimension (Inline 32 Bytes)
    pub phys_capacity_d1: u64,          // 8 bytes: C_phys for Primary Dimension
    pub unavail_capacity_d1: u64,       // 8 bytes: C_unavail (throttled/offline)
    pub alloc_capacity_d1: u64,         // 8 bytes: C_alloc (committed to active leases)
    pub resv_capacity_d1: u64,          // 8 bytes: C_resv (reserved headroom)

    // Multi-Dimensional Secondary Dimension (Inline 32 Bytes)
    pub phys_capacity_d2: u64,          // 8 bytes: C_phys for Secondary Dimension
    pub unavail_capacity_d2: u64,       // 8 bytes: C_unavail
    pub alloc_capacity_d2: u64,         // 8 bytes: C_alloc
    pub resv_capacity_d2: u64,          // 8 bytes: C_resv

    // Telemetry & Power (16 Bytes)
    pub current_power_mw: u32,          // 4 bytes: Instantaneous power consumption
    pub temperature_mc: u32,            // 4 bytes: Temperature in milli-Celsius
    pub last_telemetry_tick: u64,       // 8 bytes: Monotonic tick of last update
}

const _: () = assert!(core::mem::size_of::<ResourceDescriptor>() == 128);
const _: () = assert!(core::mem::align_of::<ResourceDescriptor>() == 8);
```

---

## 9. Resource Lease Contract Layout (Static 64 Bytes)

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseState {
    Free              = 0,
    Active            = 1,
    Expired           = 2,
    Revoked           = 3,
    ProviderLost      = 4,
    TimeAuthorityLost = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ResourceLease {
    // Lease Identification (32 Bytes)
    pub lease_id: DistributedId,        // 16 bytes: Globally unique (NodeId, LocalSeq)
    pub resource_id: DistributedId,     // 16 bytes: Bound target resource

    // Consumer & Lifecycle (16 Bytes)
    pub consumer_pid: u64,              // 8 bytes: Holder process ID
    pub generation: u32,                // 4 bytes: Slot reuse generation
    pub state: LeaseState,              // 1 byte: Active, Expired, Revoked, ProviderLost, TimeAuthorityLost
    pub _pad0: [u8; 3],                 // 3 bytes: Alignment padding

    // Time-Bounded Durability (16 Bytes)
    pub boot_epoch: u64,                // 8 bytes: Bound kernel boot incarnation
    pub expiration_tick: u64,           // 8 bytes: Monotonic tick deadline
}

const _: () = assert!(core::mem::size_of::<ResourceLease>() == 64);
const _: () = assert!(core::mem::align_of::<ResourceLease>() == 8);
```

### Semantic Differentiation: `Expired` vs. `TimeAuthorityLost`

| Dimension | `LeaseState::Expired` | `LeaseState::TimeAuthorityLost` |
| :--- | :--- | :--- |
| **Time Authority Status** | Healthy, authenticated, and current. | Lost, stale ($> 50\text{ ms}$), regressed, or unavailable. |
| **Expiration Evaluation** | Definitive ($T_{\text{current}} \ge L.\text{expiration\_tick}$). | Undetermined (ZeroOS cannot verify physical time). |
| **Capacity Authority** | Expired (Zero authority). | Revoked immediately (Zero authority). |
| **Accounting Reclamation** | Standard release to $C_{\text{avail}}$. | Capacity held in $C_{\text{unavail}}$ until physical quiescence confirmed (`I-TIME-FAILURE-ACCOUNTING-SAFETY`). |
| **Extension / Renewal** | Permitted via standard re-admission. | Strictly prohibited fail-closed (`ZeroError::TimeAuthorityLost`). |

---

## 10. Capability Authority vs. Lease Decoupling

$$\mathbf{Capability} \neq \mathbf{Lease}$$
$$\mathbf{Invariant\ I-LEASE-AUTH-BOUNDED:}\quad \text{Lease Authority} \subseteq \text{Capability Authority}$$

- **Capability (Stage 3 CDT)**: Grants unforgeable mathematical **authority** to access a resource.
- **Lease (`resourced`)**: Grants time-bounded, metered physical **capacity** of that resource.
- Presenting a lease without a valid covering capability confers zero authority.
- `resourced` rejects any lease request exceeding the caller's presented capability (`I-RES-CAPABILITY-NO-AMPLIFICATION`).

---

## 11. Crash Reconciliation Protocol

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

## 12. Authoritative Hotplug Observation Boundary

$$\mathbf{Invariant\ I-RES-DISAPPEARANCE-CASCADE:}$$
$$\text{\texttt{ProviderLost} becomes authoritative at the FIRST provider-authoritative observation of disappearance}$$
$$\text{or hardware fault (} t_{\text{observed}} \text{). \texttt{resourced} transitions the resource to \texttt{Unavailable}, sets } C_{\text{unavail}} \leftarrow C_{\text{phys}},$$
$$\text{and immediately cascades all associated active leases to \texttt{ProviderLost} fail-closed.}$$

---

## 13. Energy Model & Telemetry Tiers

| Tier | Name | Description | Authoritative for Safety / Quotas? |
| :---: | :--- | :--- | :---: |
| `0` | `Measured` | Direct hardware ADC / power sensor readings. | **YES** |
| `1` | `Estimated` | Driver calculation based on clock, voltage, utilization. | **NO (Advisory Only)** |
| `2` | `Declared` | Static datasheet declaration. | **NO (Advisory Only)** |

$$\mathbf{Invariant\ I-RES-ENERGY-LIMITS:}$$
$$\text{Hard resource limits, thermal mitigation triggers, and budget throttling MUST NOT depend on}$$
$$\text{untrusted \texttt{Estimated} or \texttt{Declared} energy telemetry.}$$

---

## 14. IPC Wire Protocol (Stage 3G Compliant)

All communication uses the frozen 80-byte `IpcMessage` layout over Stage 3G bidirectional channels:

```rust
#[repr(C)]
pub struct IpcMessage {
    pub tag: u64,             // Protocol Opcode (0x2001 .. 0x2014)
    pub payload_len: u16,     // Length of valid payload (0..48)
    pub handles_count: u16,   // Attached handles (0..4)
    pub _reserved1: u32,      // Explicit padding
    pub payload: [u8; 48],    // Inline message data
    pub handles: [u32; 4],    // Transferred capability handles
}
```

### Protocol Opcodes

| Opcode Name | Tag | Request Payload | Handles In | Response Payload | Handles Out |
| :--- | :---: | :--- | :---: | :--- | :---: |
| `OP_RES_REGISTER` | `0x2001` | ResType(1B) + Loc(1B) + CapD1(8B) + CapD2(8B) + Rights(2B) | `[dev_cap]` | Status(4B) + ResId(16B) + Gen(4B) | None |
| `OP_RES_UNREGISTER` | `0x2003` | ResId(16B) + Gen(4B) | None | Status(4B) | None |
| `OP_RES_DISCOVER` | `0x2005` | FilterType(1B) + Offset(2B) | None | Status(4B) + Count(2B) + Total(2B) + ResIds(32B) | None |
| `OP_RES_QUERY` | `0x2007` | ResId(16B) | None | Status(4B) + State(1B) + PhysD1(8B) + AvailD1(8B) + Pwr(4B) | None |
| `OP_LEASE_REQUEST` | `0x2009` | ResId(16B) + AmtD1(8B) + AmtD2(8B) + TTLTicks(8B) | `[auth_cap]` | Status(4B) + LeaseId(16B) + Gen(4B) + ExpireTick(8B) | `[dev_handle]` |
| `OP_LEASE_RENEW` | `0x200B` | LeaseId(16B) + Gen(4B) + TTLTicks(8B) | None | Status(4B) + NewExpireTick(8B) + Gen(4B) | None |
| `OP_LEASE_RELEASE` | `0x200D` | LeaseId(16B) + Gen(4B) | None | Status(4B) | None |
| `OP_LEASE_RECONCILE`| `0x200E` | LeaseId(16B) + ResId(16B) + AmtD1(8B) + ExpTick(8B) | `[auth_cap]` | Status(4B) + Gen(4B) + Reconciled(1B) | None |
| `OP_QUOTA_QUERY` | `0x200F` | EntityId(8B) + EntityType(1B) | None | Status(4B) + CpuLim(4B) + RamLim(4B) + CurRam(4B) | None |
| `OP_ENERGY_GET` | `0x2011` | ResId(16B) | None | Status(4B) + Tier(1B) + Pwr(4B) + Temp(2B) + LastTick(8B) | None |

---

## 15. Authoritative Phase 4B Invariant Catalog (38 Invariants)

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

## 16. Verification Strategy (Machine Verification Tests 4B-A through 4B-U)

1. **4B-A: Multi-Dimensional Registration**: Register heterogeneous resources (CPU, GPU, RAM); verify multi-dimensional $C_{\text{phys}}$ initialization.
2. **4B-B: Dimension Bound Rejection**: Attempt to register a resource with 9 dimensions; verify immediate rejection with `UnsupportedResourceShape`.
3. **4B-C: Vector Conservation**: Perform allocations, reservations, and thermal throttling; assert $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$ across all active dimensions.
4. **4B-D: Joint Coupling Feasibility**: Submit requests that satisfy individual dimensions but violate coupled constraints; verify `CouplingViolation` rejection.
5. **4B-E: Atomic Admission Serialization**: Issue concurrent competing lease requests under SMP load; verify zero double-allocations or intermediate state leaks.
6. **4B-F: Rust-Sound Time Seqlock**: Verify concurrent writer updates and reader loops over atomic fields; assert zero torn reads or data races.
7. **4B-G: Time Source Qualification & Regression Failure**: Test simulated regression $T_{\text{raw}} < T_{\text{observed}}$; verify immediate terminal odd freeze and fail-closed rejection under `I-TIME-SOURCE-REGRESSION`.
8. **4B-H: Consumer Source Equivalence & Freshness**: Verify `resourced` uses identical qualification parameters; assert stale observations ($> 50\text{ ms}$) rejected under `I-TIME-OBSERVATION-FRESHNESS`.
9. **4B-I: Time Authority Loss Accounting Safety**: Trigger time authority failure; verify active leases transition to `TimeAuthorityLost`, capacity transfers to $C_{\text{unavail}}$, and assert zero over-admission under `I-TIME-FAILURE-ACCOUNTING-SAFETY`.
10. **4B-J: Sequence Terminal Latch**: Force sequence counter to `SEQUENCE_MAX_VALID_EVEN`; verify transition to `SEQUENCE_TERMINAL_LATCH` (u64::MAX) and assert readers fail closed with `TimeAuthorityUnavailable`.
11. **4B-K: Deadline Arithmetic Non-Wrap**: Request lease with `duration = u64::MAX`; verify `DeadlineExhaustion` rejection without wrapping to 0.
12. **4B-L: Boot Epoch Durability & Non-Wrap**: Verify Boot Persistence Authority reservation; test $E_{\text{persisted}} = \text{u64::MAX}$ trigger fail-closed halt under `I-BOOT-EPOCH-NONWRAP`.
13. **4B-M: Capability-Mediated Admission**: Verify lease request succeeds with covering capability; fails with `PermissionDenied` when rights are attenuated.
14. **4B-N: Authority Amplification Rejection**: Verify client cannot obtain rights exceeding its presented capability.
15. **4B-O: Stale Generation Rejection**: Attempt renewal using pre-reclamation generation token; verify `GenerationMismatch`.
16. **4B-P: Consumer Crash Cleanup**: Terminate consumer process abruptly; verify `PeerClosed` signal triggers deterministic reclamation.
17. **4B-Q: Hotplug Disappearance Cascade**: Simulate driver failure; verify at $t_{\text{observed}}$ resource transitions to `Unavailable`, $C_{\text{unavail}} \leftarrow C_{\text{phys}}$, and leases transition to `ProviderLost`.
18. **4B-R: Non-Resurrecting Reconciliation**: Attempt to re-attest lease with $T_{\text{remaining}} \le 0$; verify rejection. For lease with 40 ticks remaining, verify reconciliation window is exactly 40 ticks.
19. **4B-S: Local Quota Enforcement**: Request capacity exceeding quota limit; verify `QuotaExceeded` rejection while node has free capacity.
20. **4B-T: Energy Classification Integrity**: Submit declared vs. measured telemetry; verify hard safety thresholds ignore declared telemetry.
21. **4B-U: PMM Neutrality & Non-Reuse**: Verify zero physical memory frame leakage and verify sequence allocator resumes above persisted ceiling.

---

## 17. Implementation Boundary Notice

**Implementation is strictly prohibited during this discovery phase.**  
No code, table definitions, or test runners shall be added until this Revision 12 architecture specification is formally reviewed and approved.
