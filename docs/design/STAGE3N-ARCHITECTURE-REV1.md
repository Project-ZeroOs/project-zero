# Stage 3N — SMP / Multi-Core Architecture (Rev1)

**Status:** 🟡 PROPOSED — AWAITING REVIEW  
**Author:** Project Zero Systems Architecture Team  
**Date:** September 2026  
**Scope:** Lowest-Level Native SMP / Multi-Core Substrate for Project Zero / ZeroOS  
**Dependencies:** Stages 3A–3M (Frozen, Non-Reopenable)  

---

## 1. Executive Summary & Design Principles

Stage 3N transforms the Project Zero single-CPU kernel nucleus into a fully synchronized, deterministic, multi-CPU (SMP) operating system.

The core objective is to scale execution across multiple processor cores while strictly preserving the existing contracts established in Stages 3A–3M:
- **Capability-Governed Execution:** Capabilities remain the sole authority primitive across all cores.
- **Explicit Per-CPU State:** Strict segregation between core-private and globally shared kernel state.
- **Deterministic Ownership:** Every running thread, active interrupt, DMA channel, and network queue has an unambiguous, single scheduler owner at any point in time (`I-SMP-THREAD-OWNER-1`).
- **Bounded Resource Allocations:** Zero dynamic kernel heap allocations. All per-CPU states, IPI queues, and TLB shootdown records reside in statically bounded BSS/Data structures.
- **Bootstrap Window Preservation:** Static SMP footprint strictly fits within the 2 MiB bootstrap window constraint (`__kernel_end <= 0xFFFFFFFF80200000`).
- **Physical Memory Neutrality:** Frame allocations for AP bootstrap stacks and temporary structures are strictly tracked and audited (`baseline_free == post_test_free`).

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

Static assertion compile-time checks guarantee compliance:
```rust
const _: () = assert!(core::mem::size_of::<PerCpu>() == 48);
const _: () = assert!(core::mem::align_of::<PerCpu>() == 8);
```

---

## 3. CPU Identity, Discovery & Topology Model

### 3.1 Core Limit: `MAX_CPUS = 4`
Stage 3N establishes a bounded maximum CPU count of **4 cores**:
- **CPU 0:** Bootstrap Processor (BSP).
- **CPUs 1..3:** Application Processors (APs).

**Justification for `MAX_CPUS = 4`:**
1. **Target Virtual Environment:** Matches standard headless QEMU multi-core verification (`-smp 4` or `-smp 2`).
2. **Bootstrap Window Headroom:** Accommodates per-CPU descriptors, stacks, and TSS tables within the remaining **< 2 KiB** `.bss` headroom below `0xFFFFFFFF80200000`.
3. **Concurrency Depth:** Sufficient to expose and verify true multi-core races (simultaneous execution, lock contention, cross-CPU IPI, TLB shootdowns, and work migration) without artificial scaling bloat.

### 3.2 Types & Lifecycle States
```rust
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(pub u32);

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuLifecycleState {
    Offline   = 0,
    Starting  = 1,
    Online    = 2,
    Active    = 3,
    Stopping  = 4,
    Failed    = 5,
}

#[repr(C)]
pub struct CpuSlot {
    pub cpu_id: u32,                  // 0..4
    pub lapic_id: u32,                // Hardware Local APIC ID
    pub state: CpuLifecycleState,     // Current lifecycle state
    pub generation: u32,              // Generation counter for slot reuse
    pub is_bsp: bool,                 // True if Bootstrap Processor
    pub _pad: [u8; 15],               // Explicit padding to 32 bytes
}

const _: () = assert!(core::mem::size_of::<CpuSlot>() == 32);
const _: () = assert!(core::mem::align_of::<CpuSlot>() == 8);
```

### 3.3 Topology Discovery
Topology is discovered during BSP boot:
1. **ACPI MADT Parser:** Scans RSDP in EBDA (`0x0008_0000..0x000A_0000`) and BIOS ROM (`0x000E_0000..0x0010_0000`) to find `RSD PTR `. Resolves RSDT -> MADT (`APIC` signature). Reads Type 0 (Processor Local APIC) entries.
2. **Fallback Topology:** If ACPI is absent (e.g., minimal test kernels), queries CPUID leaf `0x01` / `0x0B` and initializes up to `MAX_CPUS` slots based on discovered APIC IDs.
3. Every online CPU is registered into `CPU_SLOT_TABLE: [CpuSlot; MAX_CPUS]`.

---

## 4. AP Bootstrap Sequence & Execution Switch

### 4.1 AP Startup Trampoline
Application Processors initialize in 16-bit Real Mode upon reset.
The BSP prepares an AP startup trampoline in low physical memory (below 1 MiB):
- **Trampoline Physical Base:** `0x0000_8000` (Vector `0x08`).
- **Trampoline Components:**
  1. 16-bit Real Mode setup: disables interrupts, loads temporary 16-bit GDT.
  2. 32-bit Protected Mode transition: sets `CR0.PE = 1`, loads 32-bit flat GDT.
  3. Paging enable: loads `CR3 = MASTER_KERNEL_PML4`, sets `CR4.PAE = 1`, enables `IA32_EFER.LME = 1`, sets `CR0.PG = 1`.
  4. 64-bit Long Mode far jump: jumps to `ap_long_mode_entry` in canonical higher-half VMA (`0xFFFF_FFFF_8010_xxxx`).

### 4.2 Inter-Processor Startup Protocol (INIT-SIPI-SIPI)
The BSP brings up each discovered AP sequentially:
```text
BSP                                                AP
 │                                                  │
 ├─ Write Trampoline to 0x8000                     │
 ├─ Set PER_CPU_EXTENDED[ap_id].startup_stack       │
 ├─ Send INIT IPI via LAPIC ICR (Assert, Deassert)  │
 ├─ Delay 10 ms via LAPIC Timer                    │
 ├─ Send Startup IPI (SIPI, Vector 0x08)            ├─ Reset in 16-bit Real Mode
 ├─ Poll AP Online flag (timeout 100 ms)           ├─ Execute Trampoline (16b -> 32b -> 64b)
 │                                                  ├─ Set CR3 = MASTER_KERNEL_PML4
 │                                                  ├─ Load dedicated GDT, TSS, IDT
 │                                                  ├─ Set IA32_GS_BASE = &PER_CPU_INSTANCES[ap_id]
 │                                                  ├─ Initialize LAPIC & LAPIC Timer
 │                                                  ├─ Transition state to Online
 │◄─────────────────────────────────────────────────┤ Signals Online (Atomic SeqCst)
 ├─ Confirm AP Online (State = Active)              ├─ Enter Scheduler Idle Loop
```

### 4.3 Fault Handling & AP Timeouts
If an AP fails to transition to `Online` within a 100 ms timeout window:
- State transitions to `CpuLifecycleState::Failed`.
- Allocated temporary startup stack is reclaimed to PMM.
- System logs warning and continues with remaining operational cores (fail-safe bootstrap).

---

## 5. Per-CPU State Architecture

Because `PerCpu` is frozen at 48 bytes, Stage 3N bifurcates per-CPU state:

### 5.1 Architectural Hardware Per-CPU (`PerCpu`, 48 B, `%gs`)
Directly mapped to `IA32_GS_BASE`, unchanged from Stage 3A/3C:
```rust
#[repr(C)]
pub struct PerCpu {
    pub self_ptr: *mut PerCpu,             // 0x00 (%gs:0)
    pub cpu_id: u32,                       // 0x08 (%gs:8)
    pub lapic_id: u32,                     // 0x0C (%gs:12)
    pub current_thread: *mut KernelThread, // 0x10 (%gs:16)
    pub idle_thread: *mut KernelThread,    // 0x18 (%gs:24)
    pub preempt_count: u32,                // 0x20 (%gs:32)
    pub nested_irq_count: u32,             // 0x24 (%gs:36)
    pub need_resched: u32,                 // 0x28 (%gs:40)
    pub _pad: u32,                         // 0x2C (%gs:44)
}
```

### 5.2 Extended Per-CPU State (`CpuState`)
Stored in static BSS array `PER_CPU_STATE: [CpuState; MAX_CPUS]`:
```rust
#[repr(C)]
pub struct CpuState {
    pub cpu_id: u32,
    pub lapic_id: u32,
    pub state: CpuLifecycleState,
    pub active_pid: u64,                   // Process ID currently loaded in CR3
    pub lock: SchedLock,                   // Per-CPU runqueue spinlock
    pub run_queues: CpuRunQueues,          // Critical, High, Normal FIFO queues
    pub kernel_stack_top: u64,             // Dedicated higher-half kernel stack
    pub ist1_stack_top: u64,               // Dedicated #DF IST1 stack
    pub tss: TaskStateSegment,             // Per-CPU TSS (104 bytes)
    pub ipi_pending_mask: AtomicU32,       // Bitmask of pending IPIs
    pub tlb_shootdown_ack: AtomicBool,     // Acknowledgment flag for TLB flush
    pub interrupt_count: u64,              // Serviced IRQ counter
    pub _pad: [u8; 16],
}
```

---

## 6. Multi-Core Scheduler Architecture

### 6.1 Per-CPU Runqueues with Priority Preservation
Stage 3N selects **Partitioned Per-CPU RunQueues with Work Stealing (Option C)**:
- Each CPU owns three intrusive FIFO priority queues: `Critical`, `High`, and `Normal`.
- Strict priority monotonicity: `Critical > High > Normal > Idle`.
- Dedicated per-core idle thread (`Priority::Idle`), executed only when all worker queues are empty.

### 6.2 Invariant `I-SMP-THREAD-OWNER-1` (Single Scheduler Owner)
> **Invariant `I-SMP-THREAD-OWNER-1`:** At every instant in time, a runnable, running, or transitioning thread descriptor has **exactly one authoritative scheduler owner**.

Rules:
1. When in `ThreadState::Ready`, the thread belongs to exactly one CPU's runqueue, protected by that CPU's `SchedLock`.
2. When in `ThreadState::Running`, the thread is owned exclusively by the executing CPU's `PerCpu.current_thread`. No other core may pick or switch to it.
3. When in `ThreadState::Blocked`, the thread is removed from all runqueues and linked into a `WaitQueue`, `Mutex`, or `Condvar`.

### 6.3 Thread Migration Protocol
Migration transfers ownership from CPU A to CPU B:
1. Thread must be in `ThreadState::Ready`. A `Running` thread cannot be migrated until it yields.
2. Locks are acquired in **strictly ascending CPU ID order**:
   `min(cpu_a, cpu_b)` acquired first, then `max(cpu_a, cpu_b)`.
3. Thread is unlinked from CPU A's runqueue, its `assigned_cpu` is updated to B, and it is enqueued onto CPU B's runqueue.
4. Both locks are released in reverse order.
5. If thread priority > CPU B's currently running thread: BSP/initiator sends `IPI_RESCHEDULE` to CPU B.

---

## 7. Inter-Processor Interrupt (IPI) Subsystem

### 7.1 Fixed IPI Vector Assignment
Stage 3N defines three dedicated, bounded, high-priority interrupt vectors:
- **`IPI_VECTOR_RESCHEDULE` = 0xFD (Vector 253):** Signals target CPU to set `need_resched = 1`.
- **`IPI_VECTOR_TLB_SHOOTDOWN` = 0xFE (Vector 254):** Triggers synchronous TLB page/address-space invalidation.
- **`IPI_VECTOR_STOP` = 0xFF (Vector 255):** Halts target CPU for emergency stop, kernel panic, or offline transition.

### 7.2 Hardware Transmission (LAPIC ICR)
IPI dispatch is mediated by the Local APIC Interrupt Command Register:
- **Low Register (`0xFEE0_0300`):** Vector | Delivery Mode (`Fixed = 000b`) | Level (`Assert = 1`) | Trigger (`Edge = 0`).
- **High Register (`0xFEE0_0310`):** Destination LAPIC ID (`target_lapic_id << 24`).
- **Delivery Poll:** Spins on ICR Delivery Status bit (bit 12) with a bounded 10,000-cycle timeout.

### 7.3 Cross-CPU Wakeups & Lost-Wakeup Guard
When thread $T$ (assigned to CPU B) is unblocked by an event on CPU A:
```text
CPU A (Signaler)                                   CPU B (Target)
 │                                                  │
 ├─ Acquire CPU B SchedLock                         │
 ├─ Transition T: Blocked -> Ready                 │
 ├─ Push T to CPU B RunQueue                        │
 ├─ Check: T.priority > CPU B current priority?     │
 ├─ Release CPU B SchedLock                         │
 ├─ Send IPI_RESCHEDULE (Vector 253) to CPU B ─────►│ Intercepts ISR 253
 │                                                  ├─ Set PerCpu.need_resched = 1
 │                                                  ├─ If in hlt, awaken
 │                                                  └─ Evaluate preemption at boundary
```

---

## 8. Global TLB Shootdown Architecture

### 8.1 Invariant `I-SMP-TLB-1` (Global Translation Invalidation)
> **Invariant `I-SMP-TLB-1`:** A page-table mutation (unmap, permission change) is **not considered globally visible**, and the underlying physical frame **cannot be freed to PMM**, until every CPU caching that translation has acknowledged invalidation.

### 8.2 Active CPU Tracking (`active_cpu_mask`)
Each process `AddressSpace` tracks an atomic bitmask:
```rust
pub active_cpu_mask: AtomicU32, // Bit i = 1 if CPU i currently has PML4 loaded in CR3
```
- On CR3 switch to process $P$: `active_cpu_mask.fetch_or(1 << cpu_id, Ordering::SeqCst)`.
- On CR3 switch away from process $P$: `active_cpu_mask.fetch_and(!(1 << cpu_id), Ordering::SeqCst)`.

### 8.3 Shootdown Protocol
When CPU A unmaps virtual address $V$ in process $P$:
1. CPU A invalidates its local TLB entry (`invlpg [V]`).
2. Computes `remote_mask = P.active_cpu_mask.load(SeqCst) & !(1 << cpu_a)`.
3. If `remote_mask == 0`: No remote cores have this address space loaded. Invalidation complete immediately!
4. If `remote_mask != 0`:
   - Acquires `TLB_SHOOTDOWN_LOCK`.
   - Stores shootdown target virtual address $V$ and target PML4 root.
   - Sets `pending_ack_mask = remote_mask`.
   - Dispatches `IPI_VECTOR_TLB_SHOOTDOWN` to all cores in `remote_mask`.
   - Target cores in ISR 254 execute `invlpg [V]` (or reload CR3 if full flush requested) and clear their bit in `pending_ack_mask`.
   - CPU A spins with timeout until `pending_ack_mask.load() == 0`.
   - Releases `TLB_SHOOTDOWN_LOCK`.
5. Frame is safely reclaimed to PMM.

---

## 9. Process & Address-Space SMP Lifecycle

### 9.1 Invariant `I-SMP-ASPACE-1` (AddressSpace Destruction Safety)
> **Invariant `I-SMP-ASPACE-1`:** An `AddressSpace` cannot be dismantled or destroyed while any core currently executes with its PML4 root loaded in CR3 or retains a cached translation.

### 9.2 Teardown Protocol
When a process exits (`process_exit`):
1. All threads of the dying process on all cores must transition to `Terminated`.
2. Initiator checks `active_cpu_mask`. If any remote CPU still has CR3 pointing to the dying PML4, an IPI forces those cores to switch CR3 to `MASTER_KERNEL_PML4`.
3. Initiator verifies `active_cpu_mask == 0`.
4. Page table frames and user memory mappings are safely returned to PMM.

---

## 10. Monotonic Lock Hierarchy (Levels 1..12)

Stage 3N formalizes the global multi-core lock ordering to guarantee mathematical deadlock freedom:

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
Level 10: Scheduler Locks (CPU_SCHED_LOCK[A] < CPU_SCHED_LOCK[B] where A < B)
Level 11: IPI & TLB Shootdown Lock (TLB_SHOOTDOWN_LOCK)
Level 12: CPU Interrupt Flag (IF = 0 / cli)
```

**Ordering Invariant:** A CPU holding lock at Level $N$ may only acquire locks at Level $M > N$. Cross-CPU scheduler locks at Level 10 must be acquired in strictly ascending `CpuId` order.

---

## 11. Subsystem Integration Audits

### 11.1 Device & Interrupt Model (Stage 3L)
- **Interrupt Routing:** Hardware IRQs routed via IO-APIC to designated cores.
- **Top-Half Execution:** Runs on receiving core. If bound event awakens a thread on another core, wake logic dispatches `IPI_RESCHEDULE`.
- **Shared IRQs:** Multiple drivers on the same vector are safely broadcast without cross-core corruption.

### 11.2 Networking Model (Stage 3M)
- **Packet Buffer Slices:** `PACKET_BUFFER_TABLE` reference counting uses atomic operations (`fetch_add`, `fetch_sub`).
- **Concurrent RX/TX:** RX processing on Core 1 and TX processing on Core 2 operate concurrently through partitioned queues.
- **Socket Waiters:** Multi-core `send`/`recv` wakeups use `SOCKET_WAIT_QUEUES` + `IPI_RESCHEDULE`.

### 11.3 Storage & ZeroFS (Stage 3K)
- **Block Buffer Cache:** Protected by `BLOCK_CACHE_LOCK` (Level 1).
- **Journal & Metadata:** CoW metadata operations and transaction commits remain strictly serialized under device locks.

### 11.4 Capability Subsystem (Stage 3H)
- **Invariant `I-SMP-CAP-1`:** Capability tree traversals, delegations, and revocations acquire `CAPABILITY_TREE_LOCK` (Level 8). Revocations cascade atomically across all cores.

### 11.5 IPC Subsystem (Stage 3G)
- **Invariant `I-SMP-IPC-1`:** Channel ring buffers are protected by `CHANNEL_LOCK` (Level 7). Producer on CPU A and consumer on CPU B synchronize cleanly.

---

## 12. Static Resource Bounds & Footprint Budget

All Stage 3N tables reside in static BSS/Data:

| Table / Structure | Elements | Size per Element | Total Size |
| :--- | :--- | :--- | :--- |
| `PER_CPU_INSTANCES` | 4 | 48 B | 192 B |
| `CPU_SLOT_TABLE` | 4 | 32 B | 128 B |
| `PER_CPU_STATE` | 4 | 128 B | 512 B |
| `TLB_SHOOTDOWN_STATE`| 1 | 64 B | 64 B |
| `IPI_STATE` | 1 | 64 B | 64 B |
| **Total Stage 3N Static Memory** | — | — | **960 B (~0.94 KiB)** |

**Bootstrap Window Guarantee:**
The ~0.94 KiB static footprint fits comfortably within the `< 2 KiB` budget, ensuring `__kernel_end <= 0xFFFFFFFF80200000` remains strictly satisfied.

---

## 13. Machine Verification Plan (Tests 3N-A through 3N-Z)

Stage 3N defines **26 sequential bare-metal machine verification tests**:

| Test ID | Name | Architectural Verification Focus |
| :--- | :--- | :--- |
| `3N-A` | CPU Topology Discovery | ACPI MADT / CPUID enumeration, `CpuSlot` allocation. |
| `3N-B` | AP Real-Mode Bootstrap | INIT-SIPI-SIPI dispatch, trampoline execution, Long Mode entry. |
| `3N-C` | Per-CPU State & GS_BASE | `%gs:[16]` isolation, independent `PerCpu` across 4 cores. |
| `3N-D` | Multi-Core GDT & TSS | Dedicated TSS, independent `#DF` IST1 stacks per core. |
| `3N-E` | Multi-CPU RunQueues | Independent per-core FIFO priority queues (`Critical > High > Normal`). |
| `3N-F` | IPI Delivery & ICR | Low-level LAPIC ICR fixed IPI transmission and EOI acknowledgment. |
| `3N-G` | Cross-CPU Reschedule | Vector 253 dispatch, `need_resched` assertion, core preemption. |
| `3N-H` | Thread Migration | `I-SMP-THREAD-OWNER-1`, ascending lock migration protocol. |
| `3N-I` | Active CPU Mask Tracking | `active_cpu_mask` maintenance across simultaneous process switches. |
| `3N-J` | Cross-Core TLB Shootdown | `I-SMP-TLB-1`, Vector 254 broadcast, synchronous ACK barrier. |
| `3N-K` | AddressSpace Destruction | `I-SMP-ASPACE-1`, teardown barrier, master kernel PML4 fallback. |
| `3N-L` | SMP Mutex Contention | Concurrent threads contending across cores, no lost wakes. |
| `3N-M` | SMP Condvar & WaitQueue | Cross-core signal, lost-wakeup guard, predicate re-check. |
| `3N-N` | SMP Event Broadcast | Single core signals Event, multiple cores unblock concurrently. |
| `3N-O` | Multi-Core Process Spawn | Child process initialization and thread distribution across cores. |
| `3N-P` | Concurrent Syscalls | Simultaneous user syscall execution across multiple cores. |
| `3N-Q` | Cross-Core Capabilities | `I-SMP-CAP-1`, delegation and revocation cascade under lock. |
| `3N-R` | Cross-Core IPC Channel | Producer Core 1 -> Consumer Core 2 rendezvous flow. |
| `3N-S` | Cross-Core Shared Memory | Multi-core read/write coherency on mapped SHM pages. |
| `3N-T` | SMP Device IRQ Handling | Hardware IRQ arrival on Core 0, top-half signal to Core 1. |
| `3N-U` | SMP Network Concurrency | Concurrent TX Core 1, RX Core 2 packet processing. |
| `3N-V` | Cross-Core Socket Wakeup | Socket waiter on Core 2 unblocked by incoming packet on Core 1. |
| `3N-W` | Concurrent ZeroFS Cache | Simultaneous block read/write under `BLOCK_CACHE_LOCK`. |
| `3N-X` | Controlled CPU Offline | AP stopping transition, runqueue evacuation to BSP. |
| `3N-Y` | Monotonic Lock Ordering | Automated cycle detection across Levels 1..12 locks. |
| `3N-Z` | SMP Regression & PMM | Full multi-core regression, `baseline_free == post_test_free`. |

---

## 14. Unresolved Questions & Explicit Decisions

1. **ACPI MADT vs CPUID Discovery:**  
   *Decision:* ACPI MADT is the authoritative primary discovery mechanism; CPUID / QEMU default topology acts as a deterministic fallback.
2. **Global vs Per-CPU Runqueues:**  
   *Decision:* Partitioned Per-CPU runqueues with deterministic core affinity and priority-respecting idle work-stealing.
3. **True Hardware Hotplug:**  
   *Decision:* Hardware physical hotplug (ACPI SCI CPU hot-add) is explicitly deferred to later stages. Controlled logical offline (`Active -> Stopping -> Offline`) is fully implemented in Stage 3N.

---

```text
ARCHITECTURE STATUS:
🟡 PROPOSED — AWAITING REVIEW
```
