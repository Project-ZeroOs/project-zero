# Stage 3N — SMP / Multi-Core Architecture (Rev2)

**Status:** 🟡 PROPOSED — AWAITING REVIEW  
**Author:** Project Zero Systems Architecture Team  
**Date:** September 2026  
**Scope:** Lowest-Level Native SMP / Multi-Core Substrate for Project Zero / ZeroOS  
**Dependencies:** Stages 3A–3M (Frozen, Non-Reopenable)  

---

## 1. Executive Summary & Design Principles

Stage 3N transforms Project Zero from a uniprocessor kernel into a strictly synchronized, deterministic, multi-CPU (Symmetric Multi-Processing) operating system.

Rev2 addresses the four material concurrency blockers identified in the Rev1 review without altering the frozen Stage 3A–3M contracts:
1. **AddressSpace Activation vs Mutation Race (`I-SMP-TLB-1`):** Resolves the `active_cpu_mask` snapshot race via an explicit AddressSpace mutation lock, per-address-space shootdown generation sequence, and an atomic activation barrier.
2. **Coherent Remote-CPU Process Exit Rendezvous (`I-SMP-ASPACE-1`):** Completely eliminates unsafe CR3 clobbering by implementing a multi-core scheduler handoff protocol where remote threads terminate and exit their address space cooperatively before page tables are dismantled.
3. **Partitioned Runqueue Priority & Stealing Model (`I-SMP-SCHED-1`):** Unambiguously defines the coexistence of local priority monotonicity (`Critical > High > Normal > Idle`), affinity-based wakeup routing, cross-core preemption, and priority-respecting idle work stealing.
4. **IPI Vector & LAPIC Spurious Vector Isolation:** Reassigns `IPI_VECTOR_STOP` away from `0xFF` to `0xFB`, strictly isolating it from `LAPIC_SPURIOUS_VECTOR = 0xFF`.
5. **Authoritative Field Ownership:** Establishes an exhaustive, single-source-of-truth mapping between 48-byte `PerCpu` and external `CpuState`.
6. **Generation-Tagged TLB Acknowledgments:** Replaces bare bitmasks with monotonic request/acknowledgment generation counters to reject stale acks.
7. **Bounded IPI Timeouts & Deterministic Failure Modes:** Replaces unbounded spin-waits with explicit cycle bounds and differentiated recovery for reschedule vs TLB shootdown.
8. **Degraded Boot & AP Startup Failure:** Formalizes deterministic fallback to a degraded $N$-core topology if an AP fails the INIT-SIPI-SIPI sequence.
9. **Extended Machine Verification Suite:** Adds machine tests `3N-AA` through `3N-AH` (34 total machine tests).

```text
                                ZEROOS KERNEL
                                      │
                 ┌────────────────────┴────────────────────┐
                 │                                         │
               CPU 0 (BSP)                               CPU 1..3 (APs)
                 │                                         │
              PerCpu (48 B, %gs)                        PerCpu (48 B, %gs)
              CpuState (Extended)                       CpuState (Extended)
                 │                                         │
           Scheduler (Per-CPU)                       Scheduler (Per-CPU)
                 │                                         │
           KernelThreads                             KernelThreads
                 └────────────────────┬────────────────────┘
                                      │
                   Shared Kernel State (Levels 1..12 Locks)
           [ZeroFS | Devices/DMA | Networking | IPC | Capabilities]
```

---

## 2. Frozen ABI & Structure Layout Boundary

Stage 3N strictly preserves all frozen ABI structure sizes and alignments. In particular, **`PerCpu` remains frozen at exactly 48 bytes**. All multi-core extensions are housed in an external table `PER_CPU_STATE: [CpuState; MAX_CPUS]` indexed by CPU ID.

```text
Process               128 B  (Frozen Stage 3F)
KernelThread          176 B  (Frozen Stage 3A)
PerCpu                 48 B  (Frozen Stage 3A/3C - DO NOT ENLARGE OR REPACK)
HandleTable           520 B  (Frozen Stage 3G)
CapabilityNode         24 B  (Frozen Stage 3H)
KernelObjectSlot       40 B  (Frozen Stage 3G/3M)
SocketSlot             64 B  (Frozen Stage 3M)
PacketBufferSlot       48 B  (Frozen Stage 3M)
NetworkDeviceBinding   32 B  (Frozen Stage 3M)
NetworkInterfaceSlot   64 B  (Frozen Stage 3M)
RouteEntry             32 B  (Frozen Stage 3M)
NeighborEntry          48 B  (Frozen Stage 3M)
PortBindingSlot        24 B  (Frozen Stage 3M)
NetworkTimerSlot       16 B  (Frozen Stage 3M)
```

Compile-time static assertions enforce:
```rust
const _: () = assert!(core::mem::size_of::<PerCpu>() == 48);
const _: () = assert!(core::mem::align_of::<PerCpu>() == 8);
```

---

## 3. Authoritative State Ownership Matrix (`PerCpu` vs `CpuState`)

To prevent ambiguous or conflicting sources of truth, every per-CPU field has a single authoritative owner:

| Subsystem State Field | Authoritative Owner | Location & Storage | Access Method | Rationale |
| :--- | :--- | :--- | :--- | :--- |
| **Self Pointer** | `PerCpu.self_ptr` | `%gs:0x00` (48 B struct) | Atomic read | Architectural base reference |
| **Logical CPU ID** | `PerCpu.cpu_id` | `%gs:0x08` (48 B struct) | Read-only after boot | Hardware identification |
| **Hardware LAPIC ID** | `PerCpu.lapic_id` | `%gs:0x0C` (48 B struct) | Read-only after boot | ICR target routing |
| **Current Thread Pointer** | `PerCpu.current_thread` | `%gs:0x10` (48 B struct) | `current_thread_from_gs()` | Fast context-switch read/write |
| **Idle Thread Pointer** | `PerCpu.idle_thread` | `%gs:0x18` (48 B struct) | Read-only after init | Core-local fallback dispatch |
| **Preemption Disable Depth**| `PerCpu.preempt_count` | `%gs:0x20` (48 B struct) | Atomic increment/decrement | Core-local preemption guard |
| **Nested IRQ Depth** | `PerCpu.nested_irq_count` | `%gs:0x24` (48 B struct) | Interrupt entry/exit | Hardware interrupt nesting |
| **Need Reschedule Flag** | `PerCpu.need_resched` | `%gs:0x28` (48 B struct) | Preempt enable / IPI 252 | Fast local preemption check |
| **CPU Lifecycle State** | `CpuState.state` | `PER_CPU_STATE[id]` | Atomic write / SeqCst | Cross-CPU lifecycle audit |
| **Active Process ID (CR3)**| `CpuState.active_pid` | `PER_CPU_STATE[id]` | Scheduler dispatch | TLB shootdown & teardown audit |
| **Per-Core RunQueues** | `CpuState.run_queues` | `PER_CPU_STATE[id]` | Under `CpuState.lock` | Priority queue partition |
| **Per-Core Scheduler Lock**| `CpuState.lock` | `PER_CPU_STATE[id]` | Level 10 lock | Migration & runqueue guard |
| **Kernel Stack Bounds** | `CpuState.kernel_stack_top`| `PER_CPU_STATE[id]` | AP bootstrap & TSS | Stack overflow & IST auditing |
| **Double-Fault IST1 Stack** | `CpuState.ist1_stack_top`| `PER_CPU_STATE[id]` | AP bootstrap & TSS | Hardware exception stack |
| **Task State Segment (TSS)**| `CpuState.tss` | `PER_CPU_STATE[id]` | `ltr` at boot | Dedicated per-core TSS |
| **IPI Pending Bitmask** | `CpuState.ipi_pending_mask`| `PER_CPU_STATE[id]` | Atomic fetch-or | Coalesced IPI dispatch |
| **TLB Ack Generation** | `CpuState.tlb_ack_gen` | `PER_CPU_STATE[id]` | Atomic write in ISR 253 | Generation-tagged TLB barrier |
| **Serviced IRQ Count** | `CpuState.interrupt_count` | `PER_CPU_STATE[id]` | Atomic increment | Telemetry & diagnostics |

---

## 4. CPU Identity, Discovery & Degraded Boot

### 4.1 Core Limit: `MAX_CPUS = 4`
Stage 3N bounds the maximum processor topology to **4 cores**:
- **CPU 0:** Bootstrap Processor (BSP).
- **CPUs 1..3:** Application Processors (APs).

### 4.2 Distinguishing `CpuId`, `CpuSlot`, and `LAPIC ID`
```rust
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(pub u32); // Stable logical ID (0..3)

#[repr(C)]
pub struct CpuSlot {
    pub cpu_id: CpuId,                // Stable logical CPU ID
    pub lapic_id: u32,                // Hardware Local APIC ID
    pub state: CpuLifecycleState,     // Current lifecycle state
    pub generation: u32,              // Monotonic slot generation counter
    pub is_bsp: bool,                 // True if Bootstrap Processor
    pub _pad: [u8; 15],               // Explicit padding to 32 bytes
}

const _: () = assert!(core::mem::size_of::<CpuSlot>() == 32);
const _: () = assert!(core::mem::align_of::<CpuSlot>() == 8);
```

**Identity Invariants:**
1. **`CpuId` Stability:** A logical `CpuId` is assigned during discovery and never changes for the runtime lifetime of the system.
2. **`CpuSlot` Generation Safety:** Slots in `CPU_SLOT_TABLE: [CpuSlot; MAX_CPUS]` are protected by a monotonic generation counter. Stale references to offline CPUs are rejected with generation mismatch.
3. **`LAPIC ID -> CpuId` Mapping:** Authoritative table `LAPIC_TO_CPUID: [Option<CpuId>; 256]` maps hardware APIC IDs directly to logical `CpuId`s.

### 4.3 AP Startup Protocol & Degraded Boot
```text
BSP                                                AP
 │                                                  │
 ├─ Prepare Trampoline at 0x8000                    │
 ├─ Assign AP startup stack & percpu pointer        │
 ├─ Write CpuSlot[ap_id].state = Starting           │
 ├─ Dispatch INIT IPI (Assert/Deassert) via ICR     │
 ├─ Delay 10 ms via LAPIC Timer                    │
 ├─ Dispatch SIPI (Vector 0x08)                     ├─ Resets in 16-bit Real Mode
 ├─ Poll CpuSlot[ap_id].state (100 ms timeout)      ├─ 16b Real -> 32b Protected -> 64b Long Mode
 │                                                  ├─ Set CR3 = MASTER_KERNEL_PML4
 │                                                  ├─ Set IA32_GS_BASE = &PER_CPU_INSTANCES[ap_id]
 │                                                  ├─ Load dedicated GDT, TSS, IDT
 │                                                  ├─ Initialize LAPIC (SVR = 0x1FF)
 │                                                  ├─ Atomic write CpuSlot[ap_id].state = Online
 │◄─────────────────────────────────────────────────┤ Handshake complete
 ├─ Set CpuSlot[ap_id].state = Active               ├─ Enter Scheduler Idle Loop
```

**Degraded Boot Contract:**
If an AP fails to signal `Online` within 100 ms:
1. BSP marks `CpuSlot[ap_id].state = CpuLifecycleState::Failed`.
2. Temporary startup stack is freed to PMM.
3. BSP logs diagnostic failure marker `[WARN] AP {} failed to boot; continuing in degraded mode`.
4. ZeroOS proceeds with $N-1$ operational cores. The system never deadlocks or panics on AP bringup failure as long as BSP (CPU 0) is operational.

---

## 5. Multi-Core Scheduler Ownership (`I-SMP-SCHED-1`)

### 5.1 Partitioned Runqueues with Hierarchical Monotonicity
Each online core owns an independent `CpuRunQueues` structure containing:
- `critical_queue: RunQueue`
- `high_queue: RunQueue`
- `normal_queue: RunQueue`
- `idle_thread: *mut KernelThread`

**Local Priority Invariant:** On any single CPU, a lower-priority thread will NEVER execute if a higher-priority thread is in `ThreadState::Ready` (`Critical > High > Normal > Idle`). Within each priority class, threads are scheduled strictly FIFO.

### 5.2 Global Scheduling Invariant (`I-SMP-SCHED-1`)
> **Invariant `I-SMP-SCHED-1`:** Multi-core execution permits concurrent execution of different priority classes across distinct CPUs (e.g., CPU 0 runs `Normal` while CPU 1 runs `Critical`), but enforces global priority preference through deterministic wakeup routing, cross-core preemption, and priority-respecting idle work stealing:
> 1. **Wakeup Routing:** Newly awakened threads are preferentially placed on their affinity core if that core is idle or running a lower priority. If the affinity core is executing an equal or higher priority, the thread is routed to the lowest-numbered idle core.
> 2. **Cross-Core Preemption:** If an awakened thread has priority $P > \text{current\_thread.priority}$ on target CPU $C$, the waking CPU immediately dispatches `IPI_VECTOR_RESCHEDULE` to CPU $C$.
> 3. **Priority-Respecting Work Stealing:** When CPU $C$ becomes idle:
>    - Scans busy CPUs in circular ascending order.
>    - Steals only if target queue depth $> 1$.
>    - Stealing strictly checks `Critical` first, then `High`, then `Normal`. It NEVER steals `Normal` if another core has runnable `Critical` or `High` threads.
>    - Within a stolen priority queue, it steals from the tail, preserving head FIFO execution on the source core.

### 5.3 Single Scheduler Owner Invariant (`I-SMP-THREAD-OWNER-1`)
> **Invariant `I-SMP-THREAD-OWNER-1`:** At every instant in time, a runnable, running, or transitioning thread descriptor has **exactly one authoritative scheduler owner**.
- A thread in `ThreadState::Ready` belongs exclusively to the runqueue of its `assigned_cpu`, protected by `PER_CPU_STATE[assigned_cpu].lock`.
- A thread in `ThreadState::Running` is owned exclusively by `PER_CPU_INSTANCES[cpu_id].current_thread`.
- A thread in `ThreadState::Blocked` is owned exclusively by the synchronization primitive (`WaitQueue`, `Mutex`, `Condvar`) it is enrolled in.
- **Migration Rule:** Only `Ready` threads can be migrated. `Running` threads cannot be stolen or migrated until they yield. Migration acquires `min(cpu_a, cpu_b).lock` followed by `max(cpu_a, cpu_b).lock`.

---

## 6. Inter-Processor Interrupt (IPI) Architecture

### 6.1 Vector Assignments & Spurious Isolation
Stage 3N eliminates the vector `0xFF` collision by establishing dedicated vectors:

| Vector | Symbolic Name | Function & Delivery Action |
| :--- | :--- | :--- |
| **`0xFB` (251)** | `IPI_VECTOR_STOP` | Emergency kernel stop, panic halt, CPU offline evacuation. |
| **`0xFC` (252)** | `IPI_VECTOR_RESCHEDULE`| Asserts `PerCpu.need_resched = 1`, evaluates preemption. |
| **`0xFD` (253)** | `IPI_VECTOR_TLB_SHOOTDOWN`| Synchronous page/address-space translation invalidation. |
| **`0xFF` (255)** | `LAPIC_SPURIOUS_VECTOR`| Dedicated Local APIC Spurious Interrupt handler (`iretq`, no EOI). |

### 6.2 Bounded ICR Delivery & Failure Semantics
IPI transmission programs the LAPIC Interrupt Command Register:
- **Low Register (`0xFEE0_0300`):** Vector | Delivery Mode (`Fixed = 000b`) | Level (`Assert = 1`) | Trigger (`Edge = 0`).
- **High Register (`0xFEE0_0310`):** Destination LAPIC ID (`target_lapic_id << 24`).

**Bounded Delivery Status Poll:**
Instead of an infinite loop, the kernel polls ICR Delivery Status (bit 12) with an explicit cycle bound:
```rust
const IPI_ICR_TIMEOUT_CYCLES: u64 = 100_000;
```

**Differentiated Failure Actions:**
1. **`IPI_VECTOR_RESCHEDULE` Timeout:** Logs warning. Sets `PerCpu.need_resched = 1` directly in the target's memory. Target will evaluate preemption on its next timer tick or syscall exit. Kernel execution proceeds.
2. **`IPI_VECTOR_TLB_SHOOTDOWN` Timeout:** Fatal error. Releasing physical memory while a remote core may cache a stale translation violates memory safety. Kernel panics fail-closed: `CRITICAL: TLB shootdown IPI delivery timeout`.
3. **`IPI_VECTOR_STOP` Timeout:** Marks target CPU `Failed` and proceeds with local halt.

---

## 7. Synchronous TLB Shootdown Architecture (`I-SMP-TLB-1`)

### 7.1 The Race Problem & Rev2 Solution
In Rev1, a CPU could snapshot `active_cpu_mask`, while another CPU concurrently loaded the AddressSpace in CR3, missing the shootdown.

Rev2 solves this by introducing a **per-AddressSpace mutation lock** and a **generation-tagged activation barrier**:
```rust
pub struct AddressSpace {
    pub pml4_root: PhysFrame,
    pub active_cpu_mask: AtomicU32,        // Bit i = 1 if CPU i currently has PML4 in CR3
    pub mutation_lock: SchedLock,          // Level 11 lock: serializes page table mutations
    pub shootdown_generation: AtomicU64,   // Incremented on every unmap/mutation
}
```

### 7.2 Invariant `I-SMP-TLB-1` (Activation vs Mutation Serialization)
> **Invariant `I-SMP-TLB-1`:** No CPU may begin executing with an `AddressSpace` between the point at which a mutation requiring shootdown is serialized and the completion of the corresponding shootdown barrier without participating in that barrier.

### 7.3 Generation-Tagged Shootdown Barrier
```text
CPU A (Mutator)                                    CPU B (Active / Entering)
 │                                                  │
 ├─ Acquire aspace.mutation_lock                    │
 ├─ let gen = aspace.shootdown_generation.inc()    │
 ├─ Snapshot target_mask = aspace.active_cpu_mask   │
 ├─ Unmap page & execute local invlpg [V]           │
 ├─ If target_mask & !(1 << cpu_a) != 0:            │
 │    ├─ Post TLB request: (aspace_id, gen, V)      │
 │    ├─ Send IPI 253 to target_mask ──────────────►│ Intercepts ISR 253
 │    ├─ Spin until:                                │   ├─ Execute invlpg [V] (or CR3 reload)
 │    │    PER_CPU_STATE[B].tlb_ack_gen == gen      │   └─ Atomic write:
 │    │                                             │        PER_CPU_STATE[B].tlb_ack_gen = gen
 │◄───┴─────────────────────────────────────────────┤ Acknowledged with matching generation!
 ├─ Release aspace.mutation_lock                    │
 └─ Return physical frame to PMM                    │
```

**Entering CPU Synchronization (`switch_address_space_locked`):**
When CPU B switches CR3 to `AddressSpace A`:
1. Sets `aspace.active_cpu_mask.fetch_or(1 << cpu_b, SeqCst)`.
2. Reads `aspace.shootdown_generation.load(SeqCst)`.
3. If `aspace.mutation_lock.is_locked()`:
   CPU B executes a full CR3 reload (or waits for the mutation lock to clear) before entering user mode, guaranteeing it never executes with stale translations.

---

## 8. Process & Address-Space SMP Lifecycle (`I-SMP-ASPACE-1`)

### 8.1 Invariant `I-SMP-ASPACE-1` (Coherent Process Exit Rendezvous)
> **Invariant `I-SMP-ASPACE-1`:** An `AddressSpace` cannot be dismantled and its page-table frames returned to PMM until:
> 1. No thread belonging to the process is `Running` on any CPU.
> 2. No CPU has the `AddressSpace` active in CR3 (`active_cpu_mask == 0`).
> 3. All required TLB invalidations are fully acknowledged with generation matching.
> 4. No in-flight operation can reacquire the `AddressSpace`.

### 8.2 Multi-CPU Process Exit Rendezvous Protocol
When process $P$ terminates (`process_exit`):
```text
Initiating Core (CPU 0)                           Remote Active Core (CPU 1)
 │                                                  │
 ├─ Mark process.state = Terminating                │
 ├─ Acquire PROCESS_TABLE_LOCK (Level 9)            │
 ├─ For each thread T in process.thread_group:      │
 │    ├─ If T is on CPU 1 (Running):                │
 │    │    ├─ Send IPI_RESCHEDULE (Vector 252) ────►│ Intercepts preemption
 │    │    │                                        ├─ Thread T yields / exits
 │    │    │                                        ├─ Scheduler switches to idle thread
 │    │    │                                        ├─ switch_address_space switches CR3
 │    │    │                                        │  to MASTER_KERNEL_PML4
 │    │    │                                        ├─ aspace.active_cpu_mask.clear(cpu_1)
 │    │    │                                        └─ CPU 1 completely out of AddressSpace!
 │    │    └─ Wait until T.state == Terminated      │
 ├─ Verify process.thread_count == 0                │
 ├─ Verify aspace.active_cpu_mask.load() == 0       │
 ├─ Release PROCESS_TABLE_LOCK                      │
 ├─ Execute destroy_address_space(aspace)           │
 └─ Reclaim all PML4, PDPT, PD, PT frames to PMM    │
```

This ensures `MASTER_KERNEL_PML4` is loaded via the normal, coherent scheduler context switch rather than an unsafe raw register overwrite from an IPI handler.

---

## 9. Global 12-Level Monotonic Lock Hierarchy

To guarantee mathematical deadlock freedom across all multi-core subsystems:

```text
Level 1:  Block Device Lock (BLOCK_DEVICE_LOCK)
Level 2:  File Manager Lock (FILE_MANAGER_LOCK)
Level 3:  Device Registry Lock (DEVICE_REGISTRY_LOCK)
Level 4:  DMA Tracker Lock (DMA_TRACKER_LOCK)
Level 5:  Interrupt Subsystem Lock (INTERRUPT_LOCK)
Level 6:  Networking Locks (SOCKET_LOCK, IFACE_LOCK, PORT_LOCK, ROUTE_LOCK, ARP_LOCK)
Level 7:  IPC Locks (CHANNEL_LOCK, SHM_LOCK, OBJECT_TABLE_LOCK)
Level 8:  Capability Tree Lock (CAPABILITY_TREE_LOCK)
Level 9:  Process Table Lock (PROCESS_TABLE_LOCK)
Level 10: Scheduler Locks (PER_CPU_STATE[A].lock < PER_CPU_STATE[B].lock for A < B)
Level 11: AddressSpace Mutation / TLB Shootdown Lock (aspace.mutation_lock)
Level 12: CPU Interrupt Flag (IF = 0 / cli)
```

**Locking Rule:** A CPU holding a lock at Level $N$ may only acquire locks at Level $M > N$. Cross-CPU scheduler locks at Level 10 must be acquired in strictly ascending `CpuId` order (`min(A, B) < max(A, B)`).

---

## 10. Subsystem Integration Contracts

### 10.1 Device & Hardware Model (Stage 3L)
- **Interrupt Routing:** Hardware IRQs routed via IO-APIC to designated cores.
- **Top-Half Execution:** Dispatched on the receiving core. If an event wakes a thread on another core, wake logic enqueues the thread and dispatches `IPI_VECTOR_RESCHEDULE`.
- **DMA Authority:** Stage 3L retains exclusive authority over DMA frame pinning (`I-DEV-DMA-1`). Multiple cores can submit concurrent DMA transfers through partitioned channel queues.

### 10.2 Networking Model (Stage 3M)
- **Packet Buffer Slices:** `PACKET_BUFFER_TABLE` reference counts use atomic fetch-add/fetch-sub operations.
- **Concurrent RX/TX:** RX processing on Core 1 and TX processing on Core 2 operate concurrently through partitioned queues.
- **Cross-Core Sockets:** When a packet arriving on Core 1 unblocks a socket waiter on Core 2, Core 1 acquires the socket lock, updates state, and dispatches `IPI_VECTOR_RESCHEDULE` to Core 2.

### 10.3 Storage & ZeroFS (Stage 3K)
- **Block Cache Concurrency:** `BLOCK_CACHE_LOCK` (Level 1) serializes buffer allocation and eviction. Multi-core reads to clean buffers proceed concurrently.

### 10.4 Capability Subsystem (Stage 3H)
- **Invariant `I-SMP-CAP-1`:** All capability tree derivations and revocations acquire `CAPABILITY_TREE_LOCK` (Level 8). Revocations cascade atomically across all cores.

### 10.5 IPC Subsystem (Stage 3G)
- **Invariant `I-SMP-IPC-1`:** Channel ring buffers are protected by `CHANNEL_LOCK` (Level 7). Producer on CPU A and consumer on CPU B synchronize with acquire-release ordering.

---

## 11. Static Memory Budget & Bootstrap Window Accounting

All Stage 3N tables reside in static BSS/Data:

| Table / Structure | Elements | Size per Element | Total Size |
| :--- | :--- | :--- | :--- |
| `PER_CPU_INSTANCES` | 4 | 48 B | 192 B |
| `CPU_SLOT_TABLE` | 4 | 32 B | 128 B |
| `PER_CPU_STATE` | 4 | 128 B | 512 B |
| `LAPIC_TO_CPUID` | 256 | 1 B | 256 B |
| `TLB_SHOOTDOWN_STATE`| 1 | 64 B | 64 B |
| **Total Stage 3N Static Memory** | — | — | **1,152 B (~1.12 KiB)** |

**Bootstrap Window Guarantee:**
The ~1.12 KiB static footprint fits comfortably within the `< 2 KiB` budget, ensuring `__kernel_end <= 0xFFFFFFFF80200000` remains strictly satisfied.

---

## 12. Extended Machine Verification Suite (34 Tests)

Stage 3N defines **34 sequential bare-metal machine verification tests**:

### Core SMP Tests (`3N-A` through `3N-Z`)
- `3N-A`: CPU Topology Discovery (MADT / CPUID enumeration)
- `3N-B`: AP Real-Mode Bootstrap & Long Mode Transition
- `3N-C`: Per-CPU State & GS_BASE Isolation
- `3N-D`: Multi-Core GDT, IDT & Independent TSS Stacks
- `3N-E`: Multi-CPU Scheduler RunQueue Partitioning
- `3N-F`: IPI Delivery via LAPIC ICR
- `3N-G`: Cross-CPU Reschedule IPI & Preemption
- `3N-H`: Thread Migration Across Cores (`I-SMP-THREAD-OWNER-1`)
- `3N-I`: Address-Space Active CPU Mask Tracking
- `3N-J`: Cross-Core TLB Shootdown Invalidation (`I-SMP-TLB-1`)
- `3N-K`: AddressSpace Destruction Protection (`I-SMP-ASPACE-1`)
- `3N-L`: SMP Mutex Contention & Fairness
- `3N-M`: SMP Condvar & WaitQueue Lost-Wakeup Guard
- `3N-N`: SMP Event Broadcast Across Cores
- `3N-O`: Multi-Core Process Spawn & Lifecycle
- `3N-P`: Concurrent Syscall Invocation Across Cores
- `3N-Q`: Cross-Core Capability Operations (`I-SMP-CAP-1`)
- `3N-R`: Cross-Core IPC Channel Rendezvous (`I-SMP-IPC-1`)
- `3N-S`: Cross-Core Shared Memory (SHM) Coherency
- `3N-T`: SMP Device IRQ Routing & Top-Half Dispatch
- `3N-U`: SMP Network RX/TX Concurrent Throughput
- `3N-V`: Cross-Core Socket Waiter Wakeup
- `3N-W`: Concurrent ZeroFS Block Cache Contention
- `3N-X`: Controlled CPU Offline & Thread Evacuation
- `3N-Y`: Monotonic Lock Ordering & Deadlock Freedom
- `3N-Z`: Full SMP Regression & Physical Memory Neutrality

### Boundary & Failure Tests (`3N-AA` through `3N-AH`)
- `3N-AA`: **TLB Activation-vs-Mutation Race:** CPU 1 activates AddressSpace concurrently with CPU 0 mutation; verifies no stale translation execution.
- `3N-AB`: **Multi-CPU Process Exit Rendezvous:** Core 0 terminates process while Core 1 executes thread; verifies coherent CR3 switch to `MASTER_KERNEL_PML4`.
- `3N-AC`: **Global/CPU-Local Priority Semantics (`I-SMP-SCHED-1`):** Awakened `Critical` thread immediately preempts lower-priority worker on target core.
- `3N-AD`: **TLB Generation / Stale ACK Rejection:** Out-of-order/stale IPI 253 responses rejected; verified via monotonic generation counter.
- `3N-AE`: **IPI Delivery Timeout & Bounded Recovery:** Simulated ICR delivery delay triggers bounded timeout without kernel deadlock.
- `3N-AF`: **AP Startup Failure & Degraded Boot:** Simulated AP timeout triggers `Failed` state transition and graceful degraded multi-core boot.
- `3N-AG`: **Spurious-Vector Isolation:** Hardware spurious interrupt on vector 255 does not collide with or trigger `IPI_STOP` (vector 251).
- `3N-AH`: **CPUState / PerCpu Authority Consistency:** Automated audit asserting zero duplicate mutable state between `%gs` and `PER_CPU_STATE`.

---

```text
ARCHITECTURE STATUS:
🟡 PROPOSED — AWAITING REVIEW
```
