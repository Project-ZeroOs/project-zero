# Stage 3N — SMP / Multi-Core Architecture (Rev3)

**Status:** 🟡 PROPOSED — AWAITING REVIEW  
**Author:** Project Zero Systems Architecture Team  
**Date:** September 2026  
**Scope:** Lowest-Level Native SMP / Multi-Core Substrate for Project Zero / ZeroOS  
**Dependencies:** Stages 3A–3M (Frozen, Non-Reopenable)  

---

## 1. Executive Summary & Design Principles

Stage 3N transforms Project Zero from a uniprocessor kernel nucleus into a fully synchronized, deterministic, multi-CPU (Symmetric Multi-Processing) operating system.

Rev3 addresses the final two core architectural blockers identified in the Rev2 review:
1. **Formal AddressSpace Activation vs Mutation Mutual Exclusion (`I-SMP-TLB-1`):** Completely closes the race between CR3 loading/membership and page-table mutation via a strict atomic protocol where both operations are serialized by `AddressSpace.lock` without holding scheduler locks.
2. **Deterministic Remote-CPU Process Exit Rendezvous State Machine (`I-SMP-ASPACE-1`):** Establishes an explicit, bounded rendezvous protocol ensuring that remote cores executing threads of a dying process cooperatively exit, yield their scheduler context, transition to `MASTER_KERNEL_PML4`, clear `active_cpu_mask`, and acknowledge departure with an exit generation before page-table frames can be reclaimed.
3. **Precise SMP Priority Semantics (`I-SMP-SCHED-1`):** Explicitly decouples local strict priority ordering (`Critical > High > Normal > Idle`) from concurrent multi-core execution, defining preferential wakeup routing and priority-respecting idle work stealing without asserting an invalid global total order.
4. **Architectural Timeout Units:** Replaces raw CPU cycle counts with frequency-independent, bounded polling iteration constants and timer-tick deadlines.
5. **Monotonic Generation Allocation & Exact ACK Matching:** Formulates strict non-wrapping $u64$ generation rules for both TLB shootdowns and exit rendezvous.
6. **Extended Machine Verification Suite (41 Tests):** Adds targeted boundary tests `3N-AI` through `3N-AO`.

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

Every per-CPU field has a single authoritative owner:

| State Field | Authoritative Owner | Location & Storage | Access Method | Rationale |
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
| **Active Process ID (CR3)**| `CpuState.active_pid` | `PER_CPU_STATE[id]` | Under `aspace.lock` | TLB shootdown & teardown audit |
| **Per-Core RunQueues** | `CpuState.run_queues` | `PER_CPU_STATE[id]` | Under `CpuState.lock` | Priority queue partition |
| **Per-Core Scheduler Lock**| `CpuState.lock` | `PER_CPU_STATE[id]` | Level 10 lock | Migration & runqueue guard |
| **Kernel Stack Bounds** | `CpuState.kernel_stack_top`| `PER_CPU_STATE[id]` | AP bootstrap & TSS | Stack overflow & IST auditing |
| **Double-Fault IST1 Stack** | `CpuState.ist1_stack_top`| `PER_CPU_STATE[id]` | AP bootstrap & TSS | Hardware exception stack |
| **Task State Segment (TSS)**| `CpuState.tss` | `PER_CPU_STATE[id]` | `ltr` at boot | Dedicated per-core TSS |
| **IPI Pending Bitmask** | `CpuState.ipi_pending_mask`| `PER_CPU_STATE[id]` | Atomic fetch-or | Coalesced IPI dispatch |
| **TLB Ack Generation** | `CpuState.tlb_ack_gen` | `PER_CPU_STATE[id]` | Atomic write in ISR 253 | Generation-tagged TLB barrier |
| **Rendezvous Ack Gen** | `CpuState.rendezvous_ack_gen`| `PER_CPU_STATE[id]` | Atomic write on exit | Exit rendezvous barrier |
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
1. **`CpuId` Stability:** Assigned during discovery and never changes for the runtime lifetime of the system.
2. **`CpuSlot` Generation Safety:** Slots in `CPU_SLOT_TABLE: [CpuSlot; MAX_CPUS]` are protected by a monotonic generation counter. Stale references to offline CPUs are rejected with generation mismatch.
3. **`LAPIC ID -> CpuId` Mapping:** Authoritative table `LAPIC_TO_CPUID: [Option<CpuId>; 256]` maps hardware APIC IDs directly to logical `CpuId`s.

### 4.3 AP Startup Protocol & Degraded Boot
```text
BSP                                                AP
 │                                                  │
 ├─ Prepare Trampoline at 0x8000                    │
 ├─ Assign AP startup stack & percpu pointer        │
 ├─ Write CpuSlot[ap_id].state = Starting           │
 ├─ Dispatch INIT IPI via ICR                       │
 ├─ Delay 10 ms via LAPIC Timer                    │
 ├─ Dispatch SIPI (Vector 0x08)                     ├─ Resets in 16-bit Real Mode
 ├─ Poll CpuSlot[ap_id].state (10 ticks / 100 ms)   ├─ 16b Real -> 32b Protected -> 64b Long Mode
 │                                                  ├─ Set CR3 = MASTER_KERNEL_PML4
 │                                                  ├─ Set IA32_GS_BASE = &PER_CPU_INSTANCES[ap_id]
 │                                                  ├─ Load dedicated GDT, TSS, IDT
 │                                                  ├─ Initialize LAPIC (SVR = 0x1FF)
 │                                                  ├─ Atomic write CpuSlot[ap_id].state = Online
 │◄─────────────────────────────────────────────────┤ Handshake complete
 ├─ Set CpuSlot[ap_id].state = Active               ├─ Enter Scheduler Idle Loop
```

**Degraded Boot Contract:**
If an AP fails to signal `Online` within `MAX_AP_STARTUP_TICKS = 10` (100 ms):
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

### 5.2 Precise SMP Priority Semantics (`I-SMP-SCHED-1`)
> **Invariant `I-SMP-SCHED-1`:**
> 1. Each CPU maintains strict local priority ordering:
>    $$\text{Critical} > \text{High} > \text{Normal} > \text{Idle}$$
> 2. Wakeups and work stealing preferentially place higher-priority work on available CPUs, and a higher-priority wakeup may trigger cross-CPU rescheduling.
> 3. Concurrent execution on different CPUs does not establish a global total ordering between runnable threads (e.g., CPU 0 executing `Normal` while CPU 1 executes `Critical` is valid and expected under SMP).
> 4. **Wakeup CPU Selection:** Awakened threads preferentially route to their home core if idle or executing lower priority; otherwise routed to the lowest-numbered idle core.
> 5. **Cross-Core Preemption:** If an awakened thread has priority $P > \text{current\_thread.priority}$ on target CPU $C$, an `IPI_VECTOR_RESCHEDULE` is immediately dispatched to CPU $C$.
> 6. **Priority-Respecting Work Stealing:** When CPU $C$ becomes idle:
>    - Scans busy cores in circular ascending order.
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

**Architectural Timeout Constants:**
```rust
pub const MAX_IPI_POLL_ITERATIONS: u32 = 100_000;
pub const MAX_TLB_POLL_ITERATIONS: u32 = 1_000_000;
pub const MAX_RENDEZVOUS_POLL_TICKS: u32 = 10; // 100 ms at 100 Hz
pub const MAX_AP_STARTUP_TICKS: u32 = 10;      // 100 ms at 100 Hz
```

**Differentiated Failure Actions:**
1. **`IPI_VECTOR_RESCHEDULE` Timeout:** Logs warning. Sets `PerCpu.need_resched = 1` directly in the target's memory. Target evaluates preemption on its next timer tick or syscall exit. Kernel execution proceeds.
2. **`IPI_VECTOR_TLB_SHOOTDOWN` Timeout:** Fatal error. Releasing physical memory while a remote core may cache a stale translation violates memory safety. Kernel panics fail-closed: `CRITICAL: TLB shootdown IPI delivery timeout`.
3. **`IPI_VECTOR_STOP` Timeout:** Marks target CPU `Failed` and proceeds with local halt.

---

## 7. Synchronous TLB Shootdown Architecture (`I-SMP-TLB-1`)

### 7.1 AddressSpace Data Structure
```rust
pub struct AddressSpace {
    pub pml4_root: PhysFrame,
    pub active_cpu_mask: AtomicU32,        // Bit i = 1 if CPU i currently has PML4 in CR3
    pub lock: SchedLock,                   // Level 11 lock: serializes activation and mutation
    pub shootdown_generation: AtomicU64,   // Monotonic generation counter (starts at 1)
}
```

### 7.2 Invariant `I-SMP-TLB-1` (Activation & Mutation Mutual Serialization)
> **Invariant `I-SMP-TLB-1`:** Address-space activation and page-table mutation are mutually serialized by `aspace.lock`. No CPU can register itself as an active CPU or load the mutated AddressSpace's CR3 between mutation serialization and completion of the shootdown without participating in that shootdown generation.

### 7.3 Exact Atomic Activation Protocol (`switch_address_space_locked`)
Executed under `SCHEDULER.lock` (Level 10) when switching from thread $T_1$ to thread $T_2$ of process $P$:
1. If $T_1.pid == T_2.pid$: return (same process, zero CR3 overhead).
2. Let `old_aspace = get_aspace(T_1.pid)` and `new_aspace = get_aspace(T_2.pid)`.
3. If not null:
   - Acquire `old_aspace.lock` (Level 11).
   - Clear `old_aspace.active_cpu_mask.clear(cpu_id)`.
   - Release `old_aspace.lock`.
4. Acquire `new_aspace.lock` (Level 11).
5. Read `gen = new_aspace.shootdown_generation.load(Relaxed)`.
6. Load hardware register: `CR3 = new_aspace.pml4_root`.
7. Set `new_aspace.active_cpu_mask.set(cpu_id)`.
8. Write `PER_CPU_STATE[cpu_id].tlb_ack_gen = gen`.
9. Write `PER_CPU_STATE[cpu_id].active_pid = T_2.pid`.
10. Release `new_aspace.lock`.

### 7.4 Exact Atomic Mutation Protocol (`unmap_page` / `protect_page`)
Executed when modifying translations in `AddressSpace`:
1. Acquire `aspace.lock` (Level 11).
2. Increment `gen = aspace.shootdown_generation.fetch_add(1, SeqCst) + 1`.
   *Rule:* Wraparound is forbidden. If `gen == u64::MAX`, panic fail-closed (`GenerationExhausted`).
3. Mutate page table entries through HHDM.
4. Perform local CPU invalidation (`invlpg [V]` or full CR3 reload).
5. Snapshot `target_mask = aspace.active_cpu_mask.load(SeqCst) & !(1 << cpu_id)`.
6. If `target_mask != 0`:
   - Post shootdown descriptor to `TLB_SHOOTDOWN_STATE`: `(aspace_id, gen, target_mask, vaddr)`.
   - Dispatch `IPI_VECTOR_TLB_SHOOTDOWN` (Vector 253) to `target_mask`.
   - Bounded wait loop (up to `MAX_TLB_POLL_ITERATIONS`):
     Wait until for all $k \in target\_mask$:
     `PER_CPU_STATE[k].tlb_ack_gen.load(Relaxed) == gen`.
     *Rule:* Acknowledging $G > gen$ does NOT satisfy generation $gen$. Only exact equality $== gen$ satisfies the barrier.
   - If timeout expires: panic fail-closed (`CRITICAL: TLB shootdown ACK timeout`).
7. Release `aspace.lock`.
8. The unmapped physical frame is safely returned to PMM.

### 7.5 Deadlock-Free Proof of `aspace.lock`
`aspace.lock` is held while waiting for remote cores to acknowledge generation $gen$.
*Proof of Deadlock Freedom:*
- The remote cores receive vector 253 in `isr_253_tlb`.
- The ISR reads `TLB_SHOOTDOWN_STATE`, executes `invlpg [V]`, and writes `PER_CPU_STATE[k].tlb_ack_gen = gen` using a volatile atomic write.
- **The ISR acquires ZERO locks.** It does not acquire `aspace.lock`, scheduler locks, or any other lock.
- Since interrupts are enabled on the target core (or unmasked upon next `sti`), the ISR executes immediately and clears the barrier.
- Hence, no circular lock dependency can exist.

---

## 8. Process & Address-Space SMP Lifecycle (`I-SMP-ASPACE-1`)

### 8.1 Invariant `I-SMP-ASPACE-1` (Coherent Process Exit Rendezvous)
> **Invariant `I-SMP-ASPACE-1`:**
> AddressSpace reclamation is forbidden until every CPU that was identified as an active executor has either:
> 1. Completed the rendezvous and acknowledged departure with matching exit generation, or
> 2. Been deterministically declared `Offline` or `Failed` under the CPU lifecycle protocol and proven unable to execute the AddressSpace.

### 8.2 Exact Remote-CPU Process Exit Rendezvous State Machine
When process $P$ terminates (`process_exit`):

```text
Initiating Core (CPU 0)                           Remote Active Core (CPU 1)
 │                                                  │
 ├─ 1. State Transition:                            │
 │     Acquire PROCESS_TABLE_LOCK (Level 9)         │
 │     Set process.state = Terminating              │
 │     (Blocks all new thread creation / wakeups)   │
 │                                                  │
 ├─ 2. Identify Active Executors:                   │
 │     Acquire aspace.lock (Level 11)               │
 │     Let exit_gen = aspace.shootdown_gen.inc()    │
 │     Snapshot remote_cpus = aspace.active_cpu_mask│
 │     Release aspace.lock                          │
 │                                                  │
 ├─ 3. Request Scheduler Rendezvous:                │
 │     For each cpu in remote_cpus:                 │
 │       Set thread.state = Terminating             │
 │       Dispatch IPI_VECTOR_RESCHEDULE (252) ─────►│ Intercepts ISR 252
 │                                                  ├─ Observes thread.state == Terminating
 │                                                  ├─ Yields user context cooperatively
 │                                                  ├─ Scheduler unlinks thread (Terminated)
 │                                                  ├─ switch_address_space switches CR3
 │                                                  │  to MASTER_KERNEL_PML4
 │                                                  ├─ Clears aspace.active_cpu_mask(cpu_1)
 │                                                  ├─ Writes CpuState.rendezvous_ack = exit_gen
 │◄─────────────────────────────────────────────────┤ Core 1 completely evacuated!
 │                                                  │
 ├─ 4. Bounded Departure Barrier:                   │
 │     Poll with timeout MAX_RENDEZVOUS_POLL_TICKS: │
 │     Verify aspace.active_cpu_mask == 0 AND       │
 │     CpuState[cpu].rendezvous_ack == exit_gen     │
 │                                                  │
 ├─ 5. Failure Fallback:                            │
 │     If core fails to evacuate within 100 ms:     │
 │       Dispatch IPI_VECTOR_STOP (251) to freeze   │
 │       Declare core Failed / Offline              │
 │       Force clear bit in active_cpu_mask         │
 │                                                  │
 ├─ 6. Reclamation:                                 │
 │     Verify thread_count == 0                     │
 │     Release PROCESS_TABLE_LOCK                   │
 │     Call destroy_address_space(aspace)           │
 └─    Return PML4, PDPT, PD, PT frames to PMM      │
```

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
Level 11: AddressSpace Lock (aspace.lock)
Level 12: CPU Interrupt Flag (IF = 0 / cli)
```

**Locking Rule:** A CPU holding a lock at Level $N$ may only acquire locks at Level $M > N$. Cross-CPU scheduler locks at Level 10 must be acquired in strictly ascending `CpuId` order (`min(A, B) < max(A, B)`).

---

## 10. Static Memory Budget & Bootstrap Window Accounting

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

## 11. Extended Machine Verification Suite (41 Tests)

Stage 3N defines **41 sequential bare-metal machine verification tests**:

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

### Boundary & Failure Tests (`3N-AA` through `3N-AO`)
- `3N-AA`: TLB Activation-vs-Mutation Race (mutator holds `aspace.lock`, verifies entering core blocks)
- `3N-AB`: Multi-CPU Process Exit Rendezvous (Core 0 terminates process while Core 1 runs thread)
- `3N-AC`: Global/CPU-Local Priority Semantics (`I-SMP-SCHED-1` preemption validation)
- `3N-AD`: TLB Generation / Stale ACK Rejection (verifies generation matching)
- `3N-AE`: IPI Delivery Timeout & Bounded Recovery (`MAX_IPI_POLL_ITERATIONS`)
- `3N-AF`: AP Startup Failure & Degraded Boot (`MAX_AP_STARTUP_TICKS` timeout)
- `3N-AG`: Spurious-Vector Isolation (Vector 255 does not trigger Vector 251)
- `3N-AH`: CPUState / PerCpu Authority Consistency (audit zero mutable duplication)
- `3N-AI`: **Activation Blocked During Page-Table Mutation:** Confirms CPU cannot enter AddressSpace while mutation is in flight.
- `3N-AJ`: **CPU Activation After Completed Shootdown:** Confirms entering CPU loads fresh CR3 and clean translation.
- `3N-AK`: **Process Exit While Remote CPU Executes User Thread:** Remote user thread intercepted and evacuated.
- `3N-AL`: **Process Exit While Remote CPU Is in Kernel Mode:** Core yields at syscall exit boundary safely.
- `3N-AM`: **Remote CPU Rendezvous Timeout / Freeze Fallback:** Unresponsive core frozen with IPI 251; address space safely reclaimed.
- `3N-AN`: **Stale TLB Generation ACK Rejection:** Acknowledging $G+1$ or $G-1$ does not satisfy barrier for $G$.
- `3N-AO`: **Concurrent High/Normal Priority Execution Semantics:** Proves concurrent execution of different priorities across cores without violating local priority hierarchy.

---

```text
ARCHITECTURE STATUS:
🟡 PROPOSED — AWAITING REVIEW
```
