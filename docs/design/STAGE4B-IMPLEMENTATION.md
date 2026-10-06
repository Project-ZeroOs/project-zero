# ZeroOS Phase 4B Implementation Plan (Rev3)

**Subsystem**: Unified Resource Graph & Local Node Accounting (`resourced`)  
**Frozen Specifications**: Stage 4B Architecture Rev12 (`docs/design/STAGE4B-ARCHITECTURE-REV12.md`) & ADR-0025 Rev12 (`docs/decisions/ADR-0025-resource-graph-and-local-node-accounting.md`)  
**Status**: 📋 DRAFT IMPLEMENTATION PLAN REV3 (Pending User Approval — Zero Implementation Code Written)  
**Kernel & Stage 4A Boundaries**: Stage 3A–3N & Stage 4A Inviolate (0 bytes modified in `kernel/`, `boot/`, `brokerd/`)  
**Verification Boundary**: Authoritative verification is driven by Ring 3 user-space service execution in QEMU via `tests/test_stage4b.py`.

---

## 1. Executive Summary & Core Architectural Invariants

Phase 4B implements the local node resource graph, multi-dimensional capacity accounting, capability-bounded temporal leases, and qualified monotonic time authority for ZeroOS.

Building upon the Stage 4A service foundation (`init`, `brokerd`, and `libzero`), Phase 4B introduces `resourced` as a freestanding `#![no_std]` Ring 3 system daemon. `resourced` manages local physical resources (CPU compute, GPU cores/vRAM, physical RAM, DMA channels, hardware accelerators) without kernel modifications.

### Core Architectural Invariants Carried Forward

1. **Unified Accounting Quarantine Rule (`I-UNIFIED-QUARANTINE` / `I-TIME-FAILURE-ACCOUNTING-SAFETY`)**:
   Invalidating lease or provider authority **must never itself make physical capacity available to other workloads**.
   Across all failure paths (`TimeAuthorityLost`, `ProviderLost`, and consumer crash/disconnection `PeerClosed`), capacity is processed through **one unified quarantine engine**:
   - For individual lease authority loss (`TimeAuthorityLost` or `PeerClosed`):
     $$C_{\text{alloc}} \leftarrow C_{\text{alloc}} - \Delta C, \quad C_{\text{unavail}} \leftarrow C_{\text{unavail}} + \Delta C, \quad C_{\text{avail}}\text{ strictly unchanged}$$
   - For complete provider disappearance (`ProviderLost`):
     $$C_{\text{unavail\_new}} \leftarrow C_{\text{unavail}} + C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}}, \quad C_{\text{avail}} \leftarrow 0, \quad C_{\text{resv}} \leftarrow 0, \quad C_{\text{alloc}} \leftarrow 0$$
     This guarantees vector conservation $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$ at every discrete state step without losing the accounting ledger.
   - Physical capacity transitions from quarantined to allocatable ($C_{\text{unavail}} \to C_{\text{avail}}$) **strictly upon authoritative provider/driver confirmation** of physical release, quiescence, or reset.
2. **Distinct Semantic Failure States**:
   - `LeaseState::Expired`: Monotonic time is healthy; current tick has reached or exceeded $L.\text{expiration\_tick}$. Normal reclamation applies.
   - `LeaseState::TimeAuthorityLost`: Monotonic time is untrusted, stalled, or regressed. Immediate revocation of capacity authority; capacity is quarantined into $C_{\text{unavail}}$.
   - `LeaseState::ProviderLost`: Driver/provider hardware failure or disappearance. Immediate revocation; all resource capacity is quarantined into $C_{\text{unavail}} = C_{\text{phys}}$, and per-lease records are moved to the Quarantine Ledger awaiting provider recovery or purge.
3. **Pure Stage 4 User-Space Adapter Lifecycle (`I-TIME-ADAPTER-LIFECYCLE`)**:
   The Time Authority Adapter executes as an in-process subsystem of `init` (PID 1). If `init` or the adapter fails, the shared observation frame stalls. `resourced` independently detects staleness ($> 50\text{ ms}$) via direct hardware counter sampling, failing all temporal leases closed under Stage 4A lifecycle supervision without kernel-level process killing.
4. **Time Frame Capability Provenance (`I-TIME-AUTHORITY-PROVENANCE`)**:
   `TimeObservationFrame` is backed by Stage 3G Shared Memory. `init` holds `SHM_WRITE | SHM_READ`. Consumers (including `resourced`) receive an attenuated capability holding `SHM_READ` only. Any attempted write mapping by `resourced` or workloads is rejected by the Stage 3 kernel capability system (`sys_shm_map`).
5. **Canonical Qualified Time Source (`I-TIME-CONSUMER-SOURCE-EQUIVALENCE`)**:
   The canonical hardware reader instruction sequence is strictly:
   ```text
   LFENCE; RDTSC
   ```
   No alternative un-fenced or unqualified timing instructions are permitted. Both `init` (publisher) and `resourced` (consumer) execute this exact sequence. Qualification requires: (a) CPUID `0x80000007:EDX[8] == 1` (Invariant TSC); (b) platform profile attesting SMP phase divergence $|\text{TSC}_i - \text{TSC}_j| \le \epsilon_{\text{sync}} \ll 10\text{ ms}$; (c) calibrated frequency via CPUID `0x15` or platform boot timer calibration; (d) hypervisor invariant TSC verification if virtualized. Any qualification failure halts publication and fails consumers closed with `TimeAuthorityUnavailable`.
6. **Time Regression Terminal State Machine (`I-TIME-SOURCE-REGRESSION`)**:
   If $T_{\text{raw}} < T_{\text{last\_observed}}$, the adapter transitions immediately to `TerminalHalt`, atomically sets `sequence = u64::MAX` (terminal odd latch), and ceases publication permanently. Readers observe `u64::MAX` and fail closed with `TimeAuthorityUnavailable`. A subsequent apparently valid TSC sample **never** clears the latch. Recovery requires supervised restart and a new boot epoch.
7. **Durable Persistence Authority Contract (`I-RES-ID-UNIQUE`, `I-BOOT-EPOCH-DURABILITY`)**:
   - Durability is backed by the Stage 4 Persistence Authority (`PersistenceAuthority` interface), backed by Stage 3K persistent boot storage capabilities.
   - `init` owns `BOOT_EPOCH_SLOT` (Slot 0); performs checked non-wrapping increment ($E_{N+1} = E_N + 1$) and commits via hardware non-volatile write barrier prior to publishing `TimeObservationFrame`.
   - `resourced` owns `DISTRIBUTED_ID_CEILING_SLOT` (Slot 1); performs write-ahead block reservation $[ \text{seq\_base}, \text{seq\_base} + \text{BATCH\_SIZE} )$. The ceiling is durably committed to media before any identifier in the block is externally observable.
   - Crash recovery resumes sequence strictly above the persisted ceiling. Unissued IDs in the reserved window are permanently burned, guaranteeing `I-ID-NONREUSE`. At `u64::MAX`, allocator returns `Err(IdentifierExhausted)` and fails closed.
8. **Resource Graph vs. Task Graph Boundary (`I-GRAPH-SEPARATION`)**:
   - **Resource Graph**: A directed typed graph modeling node-local hardware and logical topology (CPUs, NUMA domains, PCIe busses, GPUs, memory banks, accelerators). Structural physical cycles are permitted. Authoritative strictly locally for physical capacity.
   - **Task Graph (Task DAG)**: Workload dependency DAG for task scheduling. Belongs to Stage 4E / future orchestration layers. Strictly OUT OF SCOPE for Phase 4B.

---

## 2. Subsystem Boundaries & Persistence Authority Architecture

```text
                     STAGE 3 KERNEL (FROZEN)
                               │
               ┌───────────────┼───────────────┐
               ▼               ▼               ▼
       Kernel Capabilities Kernel IPC    Stage 3K Persistent Store
      (sys_cap_derive)  (Channels, SHM)   (NV Storage Media)
               │               │               │
               └───────────────┼───────────────┘
                               ▼
                          init (PID 1)
              ┌────────────────┴────────────────┐
              ▼                                 ▼
      Boot Persistence                 Time Authority Adapter
   (Slot 0: BootEpochId;               (CPUID Invariant TSC, LFENCE; RDTSC;
    Durable Media Commit Barrier)       Publishes TimeObservationFrame to SHM)
              │                                 │
              ▼                                 │ SHM Handle (SHM_READ only)
           brokerd                              │ Write capability NEVER granted
     (Service Directory)                        │
              ▲                                 ▼
              │ registers / discovers       resourced
              └───────────────────────── (Daemon: PID N)
                                                │
                                                ├─ Persistence Client (Slot 1: Ceiling)
                                                │
                          ┌─────────────────────┴─────────────────────┐
                          ▼                                           ▼
                 Resource Graph & Node                   Lease Authority Engine
                   Capacity Accounting                   (Temporal Leases, Quotas,
             (Conservation, Feasibility)                  Unified Quarantine Engine)
                                                │
                                                ▼
                                        Stage 3G IPC Clients
                                        (Workload Applications)
```

### Stage 4 Persistence Authority Contract

To ensure neither `init` nor `resourced` bypasses capability authority to access raw block storage, persistence is mediated through a strict capability-backed interface:

```rust
pub const BOOT_EPOCH_SLOT: u32 = 0;
pub const DISTRIBUTED_ID_CEILING_SLOT: u32 = 1;

pub trait PersistenceAuthority {
    /// Read the persisted 64-bit value at the authorized slot.
    fn read_slot(&self, slot: u32) -> Result<u64, ZeroError>;

    /// Commit a new 64-bit value to durable non-volatile media.
    /// MUST NOT return Ok(()) until media flush / non-volatile commit barrier completes.
    fn write_and_commit_slot(&mut self, slot: u32, value: u64) -> Result<(), ZeroError>;
}
```

- **Write Authority**:
  - `init` holds capability covering `BOOT_EPOCH_SLOT`.
  - `resourced` holds an attenuated capability covering only `DISTRIBUTED_ID_CEILING_SLOT`.
- **Durable Commit Protocol**:
  Writes to non-volatile storage execute the media flush command (`CLWB` / `CLFLUSHOPT` + `SFENCE` followed by the NVMe/storage hardware flush barrier). `write_and_commit_slot` returns `Ok(())` strictly after media completion is verified.
- **Burn-on-Crash Rule**:
  If a crash occurs when `current_seq < persisted_ceiling`, the remaining allocated block is permanently burned. Recovery initializes `current_seq = persisted_ceiling`, guaranteeing zero identifier reuse.

---

## 3. Data Structures & Unified Accounting Quarantine Model

### 3.1 Time Observation Frame (64 Bytes, Cache-Line Aligned)
```rust
#[repr(C, align(64))]
pub struct TimeObservationFrame {
    pub sequence: AtomicU64,            // Seqlock counter; odd = write in progress; u64::MAX = terminal latch
    pub boot_epoch: AtomicU64,          // Durable boot incarnation identifier from Boot Persistence Authority
    pub monotonic_ticks: AtomicU64,     // Monotonic hardware tick snapshot (LFENCE; RDTSC)
    pub frequency_hz: AtomicU64,        // Qualified counter frequency (Hz) from CPUID 0x15 / timer calibration
    pub capture_tsc: AtomicU64,         // Raw TSC reading at observation capture
    pub max_drift_ns: AtomicU32,        // Maximum SMP phase divergence bound from platform profile
    pub qualification_flags: AtomicU32, // Bit 0: Invariant TSC, Bit 1: SMP Synced, Bit 2: Calibrated
    pub _reserved: [u8; 16],            // Explicit padding to 64 bytes
}
```

### 3.2 Dimension Capacity Vector & Bounds
```rust
pub const MAX_ACCOUNTING_DIMENSIONS: usize = 8;
pub const MAX_RESOURCES_PER_NODE: usize = 64;
pub const MAX_LEASES_PER_NODE: usize = 256;
pub const MAX_QUOTA_ENTITIES: usize = 32;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DimensionCapacityVector {
    pub dimensions: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub dimension_count: u8,
}
```

### 3.3 Unified Quarantine Engine & Mathematical Transitions

The accounting engine enforces vector conservation at every state change:
$$C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$$

```text
                  Authority Invalidation Event
                               │
             ┌─────────────────┼─────────────────┐
             ▼                 ▼                 ▼
      TimeAuthorityLost   ProviderLost       PeerClosed
             │                 │                 │
             └─────────────────┼─────────────────┘
                               ▼
                  UNIFIED QUARANTINE ENGINE
                               │
             ┌─────────────────┴─────────────────┐
             ▼                                   ▼
   Individual Lease Invalidation        Complete Provider Disappearance
   (TimeAuthorityLost / PeerClosed)     (ProviderLost)
   C_alloc   -= ΔC                      C_unavail += (C_avail + C_resv + C_alloc)
   C_unavail += ΔC                      C_avail   = 0
   C_avail unchanged                    C_resv    = 0
   C_resv  unchanged                    C_alloc   = 0
                                        (Per-lease records -> Quarantine Ledger)
             │                                   │
             └─────────────────┬─────────────────┘
                               ▼
                 Authoritative Driver Confirmation
                 (Physical Release / Reset Confirmed)
                               │
                               ▼
                   RECLAIM TO ALLOCATABLE
                   C_unavail -= ΔC
                   C_avail   += ΔC
```

#### Transition Invariant Audit
1. **Normal Allocation**: $C_{\text{avail}} \leftarrow C_{\text{avail}} - \Delta C, \quad C_{\text{alloc}} \leftarrow C_{\text{alloc}} + \Delta C$. Total sum unchanged.
2. **Individual Lease Quarantine (`TimeAuthorityLost`, `PeerClosed`)**:
   $$C_{\text{alloc}} \leftarrow C_{\text{alloc}} - \Delta C, \quad C_{\text{unavail}} \leftarrow C_{\text{unavail}} + \Delta C$$
   Total sum unchanged. $C_{\text{avail}}$ strictly unchanged.
3. **Provider Disappearance (`ProviderLost`)**:
   $$C_{\text{unavail\_new}} = C_{\text{unavail\_old}} + C_{\text{avail\_old}} + C_{\text{resv\_old}} + C_{\text{alloc\_old}} = C_{\text{phys}}$$
   $$C_{\text{avail}} \leftarrow 0, \quad C_{\text{resv}} \leftarrow 0, \quad C_{\text{alloc}} \leftarrow 0$$
   Total sum: $0 + 0 + 0 + C_{\text{phys}} = C_{\text{phys}}$! Conservation is preserved identically.
4. **Driver Confirmation Reclaim**:
   $$C_{\text{unavail}} \leftarrow C_{\text{unavail}} - \Delta C, \quad C_{\text{avail}} \leftarrow C_{\text{avail}} + \Delta C$$
   Total sum unchanged.

---

## 4. Authoritative Stage 3G IPC Protocol Opcodes (`0x2001` .. `0x2014`)

Communication uses the frozen 80-byte `IpcMessage` layout over Stage 3G bidirectional channels:

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

### Complete Bidirectional Opcode Table

| Request Opcode | Tag | Request Payload & Attached Handles | Response Opcode | Tag | Response Payload & Attached Handles | Authority Required |
| :--- | :---: | :--- | :--- | :---: | :--- | :--- |
| `OP_RES_REGISTER` | `0x2001` | ResType(1B) + Loc(1B) + CapD1(8B) + CapD2(8B) + Rights(2B); Handles: `[dev_cap]` | `OP_RES_REGISTER_RESP` | `0x2002` | Status(4B) + ResId(16B) + Gen(4B); Handles: None | Provider capability covering device |
| `OP_RES_UNREGISTER` | `0x2003` | ResId(16B) + Gen(4B); Handles: None | `OP_RES_UNREGISTER_RESP` | `0x2004` | Status(4B); Handles: None | Registered provider endpoint |
| `OP_RES_DISCOVER` | `0x2005` | FilterType(1B) + Offset(2B); Handles: None | `OP_RES_DISCOVER_RESP` | `0x2006` | Status(4B) + Count(2B) + Total(2B) + ResIds(32B); Handles: None | Public / any client |
| `OP_RES_QUERY` | `0x2007` | ResId(16B); Handles: None | `OP_RES_QUERY_RESP` | `0x2008` | Status(4B) + State(1B) + PhysD1(8B) + AvailD1(8B) + Pwr(4B); Handles: None | Public / any client |
| `OP_LEASE_REQUEST` | `0x2009` | ResId(16B) + AmtD1(8B) + AmtD2(8B) + TTLTicks(8B); Handles: `[auth_cap]` | `OP_LEASE_REQUEST_RESP` | `0x200A` | Status(4B) + LeaseId(16B) + Gen(4B) + ExpireTick(8B); Handles: `[dev_handle]` | Caller capability covering resource |
| `OP_LEASE_RENEW` | `0x200B` | LeaseId(16B) + Gen(4B) + TTLTicks(8B); Handles: None | `OP_LEASE_RENEW_RESP` | `0x200C` | Status(4B) + NewExpireTick(8B) + Gen(4B); Handles: None | Active lease holder endpoint |
| `OP_LEASE_RELEASE` | `0x200D` | LeaseId(16B) + Gen(4B); Handles: None | `OP_LEASE_RELEASE_RESP` | `0x200E` | Status(4B); Handles: None | Active lease holder endpoint |
| `OP_LEASE_RECONCILE` | `0x200F` | LeaseId(16B) + ResId(16B) + AmtD1(8B) + ExpTick(8B); Handles: `[auth_cap]` | `OP_LEASE_RECONCILE_RESP` | `0x2010` | Status(4B) + Gen(4B) + Reconciled(1B); Handles: None | Pre-crash surviving lease holder |
| `OP_QUOTA_QUERY` | `0x2011` | EntityId(8B) + EntityType(1B); Handles: None | `OP_QUOTA_QUERY_RESP` | `0x2012` | Status(4B) + CpuLim(4B) + RamLim(4B) + CurRam(4B); Handles: None | Entity or supervisor |
| `OP_ENERGY_GET` | `0x2013` | ResId(16B); Handles: None | `OP_ENERGY_GET_RESP` | `0x2014` | Status(4B) + Tier(1B) + Pwr(4B) + Temp(2B) + LastTick(8B); Handles: None | Public / any client |

---

## 5. Subsystem Implementation Modules

### 5.1 Durable Identity Allocator (`resourced/src/identity.rs`)
- Implements `DistributedIdAllocator`:
  ```rust
  pub struct DistributedIdAllocator<P: PersistenceAuthority> {
      node_id: u64,
      current_seq: u64,
      persisted_ceiling: u64,
      batch_size: u64,
      persistence: P,
  }
  ```
- **Write-Ahead Protocol**:
  1. If `current_seq + 1 > persisted_ceiling`:
     $$\text{target\_ceiling} = \text{persisted\_ceiling}.checked\_add(self.batch\_size).ok\_or(ZeroError::IdentifierExhausted)?$$
     Execute `self.persistence.write_and_commit_slot(DISTRIBUTED_ID_CEILING_SLOT, target_ceiling)?`.
     Update `self.persisted_ceiling = target_ceiling`.
  2. Increment `current_seq += 1`.
  3. Emit `DistributedId { node_id: self.node_id, local_seq: self.current_seq }`.
- **Exhaustion Guard**: If `target_ceiling == u64::MAX`, immediately returns `Err(ZeroError::IdentifierExhausted)`. Never wraps.
- **Recovery & Burn-on-Crash**: On crash recovery, reads `persisted_ceiling` from storage. Initializes `current_seq = persisted_ceiling`. Any unissued IDs from the previous reservation window are permanently burned, guaranteeing `I-ID-NONREUSE`.

### 5.2 Boot Epoch Persistence (`init/src/boot_epoch.rs`)
- Interacts with `PersistenceAuthority` on Slot 0 (`BOOT_EPOCH_SLOT`):
  1. On `init` startup, reads `current_epoch = persistence.read_slot(BOOT_EPOCH_SLOT)?`.
  2. Increments `next_epoch = current_epoch.checked_add(1).ok_or(ZeroError::EpochExhaustion)?`.
  3. Executes `persistence.write_and_commit_slot(BOOT_EPOCH_SLOT, next_epoch)?`.
  4. Returns `next_epoch` to `init/src/time_adapter.rs` to set `TimeObservationFrame.boot_epoch`.

### 5.3 Canonical Qualified Time Authority Adapter (`init/src/time_adapter.rs`)
- Executes in Ring 3 as an in-process worker thread of `init` (PID 1).
- **Qualification Routine**:
  - Checks CPUID `0x80000007:EDX[8] == 1` (`Invariant TSC`). If 0, fail closed (`TimeAuthorityUnavailable`).
  - Reads frequency from CPUID `0x15` or platform boot timer calibration profile. If uncalibrated, fail closed.
  - Verifies SMP synchronization prerequisites.
- **Publishing Phase (100 Hz / 10 ms)**:
  - Executes canonical sequence:
    ```rust
    core::arch::x86_64::_mm_lfence();
    let t_raw = unsafe { core::arch::x86_64::_rdtsc() };
    ```
  - **Monotonicity Regression Detection**:
    ```rust
    if t_raw < self.last_published_tsc {
        // FATAL REGRESSION: Latch sequence to u64::MAX terminal odd state
        self.frame.sequence.store(u64::MAX, Ordering::Release);
        self.state = AdapterState::TerminalHalt;
        return Err(ZeroError::HardwareRegressionDetected);
    }
    ```
  - Standard seqlock update:
    ```rust
    let seq = self.frame.sequence.load(Ordering::Relaxed);
    self.frame.sequence.store(seq + 1, Ordering::Release); // odd = in-progress
    self.frame.capture_tsc.store(t_raw, Ordering::Relaxed);
    self.frame.monotonic_ticks.store(ticks, Ordering::Relaxed);
    self.frame.sequence.store(seq + 2, Ordering::Release); // even = valid
    ```

### 5.4 Time Consumer Freshness Evaluator (`libzero/src/time.rs`)
- Executed by `resourced` and consumer processes:
  - Lock-free seqlock read loop:
    ```rust
    loop {
        let seq1 = frame.sequence.load(Ordering::Acquire);
        if seq1 == u64::MAX { return Err(ZeroError::TimeAuthorityUnavailable); }
        if seq1 & 1 != 0 { core::hint::spin_loop(); continue; }
        // Read atomic payload fields
        let seq2 = frame.sequence.load(Ordering::Acquire);
        if seq1 == seq2 { break; }
    }
    ```
  - **Direct Hardware Freshness Evaluation**:
    - Consumer executes canonical `LFENCE; RDTSC` to capture `t_eval`.
    - Computes $\Delta T = \frac{(t_{\text{eval}} - t_{\text{capture}}) \times 10^9}{\text{frequency\_hz}}$.
    - If $\Delta T > 50\text{ ms}$, rejects reading fail-closed with `ZeroError::TimeObservationStale`.

### 5.5 Multi-Dimensional Capacity Accounting Engine (`resourced/src/accounting.rs`)
- Multi-dimensional vector math on `DimensionCapacityVector` (bounded to 8 dimensions).
- `AccountingDomain`:
  - Tracks $C_{\text{phys}}, C_{\text{avail}}, C_{\text{resv}}, C_{\text{alloc}}, C_{\text{unavail}}$.
  - Invariant assertion on every state mutation:
    $$C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$$
  - Joint coupling constraint solver: verifies $\mathbf{A}_{\text{coupling}} \cdot (\mathbf{C}_{\text{alloc}} + \Delta \mathbf{C}) \le \mathbf{b}_{\text{coupling}}$.
  - Serialized admissions under atomic `writer_lock`.

### 5.6 Unified Quarantine Engine & Lease Lifecycle (`resourced/src/lease_engine.rs`)
- Implements `QuarantineLedger` retaining active lease metadata on provider loss or time loss.
- Executes the unified quarantine state machine defined in Section 3.3.
- Reclaims quarantined capacity to $C_{\text{avail}}$ strictly upon receiving driver confirmation.
- Evaluates post-crash reconciliation window: $\Delta T_{\text{reconcile}}(L) = \min(\Delta T_{\text{policy}}, T_{\text{remaining}}(L))$. Expired leases ($T_{\text{remaining}} \le 0$) cannot be re-attested.

### 5.7 Resource Graph (`resourced/src/graph.rs`)
- Manages static table of `ResourceDescriptor` entries (`MAX_RESOURCES_PER_NODE = 64`).
- Models physical hardware and locality relationships (parent/child, NUMA affinity, PCIe bus bindings).
- Structural cycles in physical topology are permitted (e.g. ring interconnects).
- Task execution DAG is strictly excluded (preserved for Stage 4E).

### 5.8 `resourced` Daemon Binary (`resourced/src/main.rs`)
- Freestanding `#![no_std]` binary.
- Connects to `brokerd` via `OP_REGISTER_SERVICE ("resourced")`.
- Maps `TimeObservationFrame` shared memory handle received from `init` with `SHM_READ`.
- Dispatches Stage 3G IPC requests across opcodes `0x2001` .. `0x2014`.

---

## 6. Authoritative Machine Verification Suite (Tests 4B-A through 4B-Z)

The authoritative verification is driven by `tests/test_stage4b.py`, executing QEMU with real user-space binaries (`init`, `brokerd`, `resourced`, and test workload clients) communicating over real Stage 3G IPC channels:

### Group 1: Identity & Durability
- **4B-A: DistributedId Allocation**: Issuance of globally unique `(NodeId, LocalSeq)` tuples for resources and leases (`I-RES-ID-UNIQUE`).
- **4B-B: Durable Sequence Reservation**: Verifies sequence ceiling is committed via `write_and_commit_slot` before ID issuance; on simulated restart, allocator resumes strictly above persisted ceiling, permanently burning unissued block IDs (`I-ID-DURABLE-ALLOCATOR-STATE`, `I-ID-NONREUSE`).
- **4B-C: Sequence Exhaustion & Non-Reuse**: Forces allocator to `u64::MAX`; asserts fail-closed return `Err(IdentifierExhausted)` and zero identifier reuse (`I-BOOT-EPOCH-NONWRAP`).

### Group 2: Resource Graph
- **4B-D: Multi-Dimensional Registration**: Registers heterogeneous resources (CPU, GPU, RAM) via `OP_RES_REGISTER`; verifies multi-dimensional $C_{\text{phys}}$ initialization.
- **4B-E: Dimension Bound Rejection**: Attempts registration with 9 dimensions; asserts fail-closed rejection `Err(UnsupportedResourceShape)` (`I-ACCOUNTING-DIMENSION-BOUNDED`).
- **4B-F: Structural Topology Integrity**: Registers cyclical physical interconnect (NUMA0 <-> NUMA1 ring); verifies graph traversal handles cycles without infinite recursion (`I-GRAPH-SEPARATION`).

### Group 3: Multi-Dimensional Accounting
- **4B-G: Vector Conservation**: Performs dynamic allocations, reservations, and thermal throttling; asserts $C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} + C_{\text{unavail}} == C_{\text{phys}}$ across all 8 dimensions (`I-ACCOUNTING-CONSERVATION`).
- **4B-H: Coupling Constraint Enforcement**: Submits requests satisfying individual dimension ceilings but violating linear joint coupling constraints ($\mathbf{A} \cdot \mathbf{C} > \mathbf{b}$); asserts rejection with `CouplingViolation` (`I-ACCOUNTING-FEASIBILITY`).
- **4B-I: Atomic Admission Serialization**: Concurrently submits competing lease requests under atomic admission lock; asserts zero double-allocations and sequential consistency (`I-ACCOUNTING-ATOMIC-ADMISSION`).
- **4B-J: Local Quota Enforcement**: Requests capacity exceeding client's configured quota ceiling; asserts `QuotaExceeded` rejection even when node has available capacity (`I-QUOTA-BOUNDED`).

### Group 4: Time Authority & Invariant Clock
- **4B-K: Canonical Qualified Source**: Verifies CPUID invariant TSC flag and calibrated frequency using canonical `LFENCE; RDTSC`; rejects uncalibrated/unsupported platforms fail-closed (`I-TIME-SOURCE-QUALIFIED`).
- **4B-L: Rust-Sound Time Seqlock**: Concurrent atomic seqlock reader and writer loops; asserts zero torn reads, valid acquire/release memory ordering, and zero data races.
- **4B-M: Time Frame Write Authority Rejection**: `resourced` attempts to map `TimeObservationFrame` with `SHM_WRITE`; asserts kernel capability rejection (`RightsAmplificationRejected` / `PermissionDenied`); confirms `init` is sole publisher (`I-TIME-AUTHORITY-PROVENANCE`).
- **4B-N: Consumer Freshness & Equivalence**: `resourced` evaluates freshness via canonical `LFENCE; RDTSC`; asserts observations older than 50 ms are rejected with `TimeObservationStale` (`I-TIME-OBSERVATION-FRESHNESS`).
- **4B-O: Monotonic Regression Terminal Latch**: Injects simulated hardware counter backward step ($T_{\text{raw}} < T_{\text{last}}$); asserts adapter sets sequence to `u64::MAX` terminal odd latch, halts permanently, and subsequent samples do not clear latch (`I-TIME-SOURCE-REGRESSION`).
- **4B-P: Deadline Arithmetic Non-Wrap**: Requests lease with `duration = u64::MAX`; asserts checked addition prevents integer overflow, failing closed with `DeadlineExhaustion` (`I-TIME-DEADLINE-NONWRAP`).
- **4B-Q: BootEpoch Durability & Non-Wrap**: Verifies durable `BootEpochId` reservation on Slot 0; tests increment at `u64::MAX` triggers fail-closed halt with `EpochExhaustion` (`I-BOOT-EPOCH-DURABILITY`, `I-BOOT-EPOCH-NONWRAP`).

### Group 5: Lease Security & Authority
- **4B-R: Capability-Bounded Admission**: Verifies lease request succeeds when presented capability covers requested resource and rights; fails with `PermissionDenied` when rights are attenuated (`I-LEASE-AUTH-BOUNDED`).
- **4B-S: Authority Amplification Rejection**: Caller presents read-only capability requesting exclusive lease; asserts kernel/broker rejects rights amplification (`I-RES-CAPABILITY-NO-AMPLIFICATION`).
- **4B-T: Stale Generation Rejection**: Attempts lease renewal presenting a stale generation token; asserts fail-closed rejection with `GenerationMismatch` (`I-LEASE-STALE-REJECTION`).

### Group 6: Failure & Unified Quarantine Engine
- **4B-U: TimeAuthorityLost Accounting Quarantine**: Triggers time authority failure; asserts active leases transition to `TimeAuthorityLost`, capacity transfers $C_{\text{alloc}} \to C_{\text{unavail}}$ with $C_{\text{avail}}$ unchanged; verifies $C_{\text{unavail}} \to C_{\text{avail}}$ only occurs after driver release confirmation (`I-TIME-FAILURE-ACCOUNTING-SAFETY`).
- **4B-V: ProviderLost Mathematical Conservation**: Simulates driver disappearance; asserts resource transitions to `Unavailable`, $C_{\text{unavail\_new}} \leftarrow C_{\text{unavail}} + C_{\text{avail}} + C_{\text{resv}} + C_{\text{alloc}} == C_{\text{phys}}$, $C_{\text{avail}} \leftarrow 0, C_{\text{resv}} \leftarrow 0, C_{\text{alloc}} \leftarrow 0$; asserts vector conservation holds and active leases move to Quarantine Ledger (`I-RES-DISAPPEARANCE-CASCADE`).
- **4B-W: Consumer Crash Cleanup Ordering**: Simulates consumer crash (`PeerClosed`); asserts lease revoked, capacity quarantined in $C_{\text{unavail}}$, quiescence dispatched to driver, and capacity released to $C_{\text{avail}}$ only upon driver confirmation (`I-LEASE-CLEANUP`).
- **4B-X: Non-Resurrecting Reconciliation**: Attempts post-crash reconciliation on expired lease ($T_{\text{remaining}} \le 0$); asserts rejection. For active lease, asserts reconciliation window strictly equals $T_{\text{remaining}}$ (`I-LEASE-RECONCILIATION-BOUNDED`).

### Group 7: Energy & System Integration
- **4B-Y: Energy Telemetry Classification Integrity**: Submits declared vs. measured telemetry; asserts hard safety limits and thermal throttling ignore declared/estimated telemetry (`I-RES-ENERGY-LIMITS`).
- **4B-Z: Full System IPC Integration & PMM Neutrality**: End-to-end user-space workflow: `init` bootstraps `brokerd` and `resourced`; client discovers `resourced` via broker, requests lease, uses capacity, releases lease; asserts zero net physical memory frame leakage (`pmm.free_frame_count() == baseline`).

---

## 7. Execution Workflow & Staged Implementation Steps

Upon approval of this plan, implementation proceeds through the following sequential commits:

1. **Step 1: Core Wire Protocol, Types & Persistence Interface (`libzero`)**:
   - Create `libzero/src/resource.rs`, `libzero/src/lease.rs`, `libzero/src/time.rs`, `libzero/src/persistence.rs`.
   - Update `libzero/src/ipc.rs` with complete opcode table (`0x2001` .. `0x2014`).
   - Update `libzero/src/error.rs`.
   - Build validation: `cargo build --target x86_64-unknown-none` in `libzero`.
2. **Step 2: Boot Epoch & Canonical Time Authority Adapter (`init`)**:
   - Implement `init/src/boot_epoch.rs` with `PersistenceAuthority` Slot 0 protocol.
   - Implement `init/src/time_adapter.rs` with CPUID invariant TSC qualification, canonical `LFENCE; RDTSC` sampling, and `u64::MAX` regression terminal latch.
   - Map `TimeObservationFrame` to Stage 3G Shared Memory with `SHM_WRITE`.
   - Build validation: `cargo build --target x86_64-unknown-none` in `init`.
3. **Step 3: Identity & Accounting Engine (`resourced`)**:
   - Create crate `resourced/` with `Cargo.toml`.
   - Implement `resourced/src/identity.rs` with write-ahead sequence reservation over `PersistenceAuthority` Slot 1.
   - Implement `resourced/src/accounting.rs` with multi-dimensional vector conservation, coupling solver, and `QuarantineLedger`.
   - Implement `resourced/src/graph.rs` with static topology tables and cycle support.
4. **Step 4: Lease Engine & Unified Quarantine (`resourced`)**:
   - Implement `resourced/src/lease_engine.rs` with the unified quarantine engine (`TimeAuthorityLost`, `ProviderLost`, consumer crash), provider confirmation release, and bounded crash reconciliation.
5. **Step 5: `resourced` Main Loop & Broker Registration**:
   - Implement `resourced/src/main.rs` with broker registration (`OP_REGISTER_SERVICE`), read-only SHM time frame mapping, and Stage 3G IPC request dispatch loop.
   - Integrate `resourced` into `init/src/main.rs` supervisor list.
6. **Step 6: Real Ring 3 Integration Test Suite (`tests/test_stage4b.py`)**:
   - Implement comprehensive QEMU test harness validating Tests 4B-A through 4B-Z.
   - Assert all 26 test markers, PMM neutrality, and clean exit code 33.
