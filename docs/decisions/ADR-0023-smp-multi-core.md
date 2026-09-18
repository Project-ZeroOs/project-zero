# ADR-0023: SMP / Multi-Core Architecture (Stage 3N Rev4)

## Context
Project Zero / ZeroOS requires a native, deterministic, capability-governed, and bounded **Symmetric Multi-Processing (SMP) / Multi-Core Model**. Stages 3A through 3M established threads, cooperative scheduling, preemptive timer ticks, synchronization primitives, thread lifecycles, processes, IPC, capabilities, syscalls, ELF execution, persistent storage (ZeroFS), the device/hardware abstraction layer, and native networking.

Stage 3N Architecture Rev3 was reviewed and refined into Rev4 to resolve one material lifetime race, establish hardware-safe quiescence guarantees, and finalize exact scheduler preemption semantics:
1. **Process Activation Prohibition (`I-SMP-ASPACE-2`)**: Explicitly forbids activating an AddressSpace whose owning Process is in `Terminating`, `Zombie`, `Reclaiming`, or `Free` state. `switch_address_space_locked` checks the authoritative Process state under `aspace.lock` and rejects activation.
2. **Hardware-Safe Quiescence Guarantee (`I-SMP-ASPACE-3`)**: Eliminates physical memory use-after-free risk. An unquiesced CPU at exit rendezvous triggers an immediate fail-closed kernel panic (`CRITICAL: Process exit rendezvous timeout — unquiesced CPU`), preventing premature page-table frame destruction.
3. **Unified Monotonic Generation Namespace**: Monotonic $u64$ generation counter per `AddressSpace` unifies page-table mutations and exit rendezvous. Exact ACK matching ($== gen$) is required; generation wraparound is forbidden (`u64::MAX` panics fail-closed).
4. **Deterministic All-Cores-Busy Wakeup Preemption Policy**: Closed the undefined scheduler state when all cores are busy by defining exact deterministic preemption rules for `Critical` and `High` wakeups.
5. **Extended Machine Verification**: Expanded suite to 46 bare-metal tests (`3N-A` through `3N-AT`).

---

## Architectural Decisions

### ADR-3N-001: CPU Topology & Bounded Core Limit (`MAX_CPUS = 4`)
- **Core Count Limit:** Bounded strictly at `MAX_CPUS = 4` (CPU 0 = BSP; CPUs 1..3 = APs).
- **Justification:**
  1. Matches standard QEMU multi-core testing topology (`-smp 4`).
  2. Guarantees that all per-CPU data, TSS structures, and runqueues fit within the **< 2 KiB** `.bss` headroom remaining below the 2 MiB bootstrap window (`__kernel_end <= 0xFFFFFFFF80200000`).
  3. Sufficient concurrency depth to expose all true multi-core race conditions without combinatorial testing overhead.
- **Topology Discovery:** Primary discovery via ACPI MADT table parsing (Local APIC Type 0 entries); CPUID / QEMU default enumeration acts as a deterministic fallback.

### ADR-3N-002: Per-CPU State Bifurcation (Preserving 48-Byte `PerCpu`)
- **Strict ABI Preservation:** The architectural `PerCpu` structure is **frozen at exactly 48 bytes**. It is NOT enlarged, repacked, or mutated.
- **Bifurcated Structure:**
  - `PER_CPU_INSTANCES: [PerCpu; MAX_CPUS]`: Houses the minimal architectural hardware fields accessed via `%gs:[offset]` (self-pointer, `cpu_id`, `lapic_id`, `current_thread`, `idle_thread`, `preempt_count`, `nested_irq_count`, `need_resched`).
  - `PER_CPU_STATE: [CpuState; MAX_CPUS]`: External bounded table housing extended multi-core state (per-core runqueues, per-core `SchedLock`, `active_pid`, dedicated kernel/IST1 stack pointers, per-core TSS, and IPI pending bitmasks).

### ADR-3N-003: AP Bootstrap Protocol & Degraded Boot
- **Trampoline Placement:** 16-bit Real Mode trampoline placed at physical address `0x0000_8000` (Vector `0x08`).
- **Protocol Flow:**
  1. BSP writes trampoline code and sets target AP's startup stack pointer.
  2. BSP dispatches INIT IPI via LAPIC ICR, waits 10 ms.
  3. BSP dispatches Startup IPI (SIPI, Vector `0x08`).
  4. AP boots in Real Mode, transitions to 32-bit Protected Mode, enables PAE/Paging with `CR3 = MASTER_KERNEL_PML4`, and far-jumps to 64-bit Long Mode in canonical higher-half VMA.
  5. AP loads its dedicated GDT, TSS, IDT, sets `IA32_GS_BASE = &PER_CPU_INSTANCES[ap_id]`, enables LAPIC (with `SVR = 0x1FF`), signals `CpuLifecycleState::Online`, and enters the scheduler idle loop.
- **Degraded Boot Fallback:** `MAX_AP_STARTUP_TICKS = 10` (100 ms) timeout. If an AP fails to respond, it is marked `CpuLifecycleState::Failed`, its startup stack is reclaimed, and ZeroOS proceeds with $N-1$ operational cores without panic.

### ADR-3N-004: Partitioned Runqueues with Priority Preservation (`I-SMP-SCHED-1`)
- **Queue Model:** Each CPU owns three intrusive FIFO priority queues: `Critical`, `High`, and `Normal`.
- **Local Priority Invariant:** Strict hierarchical evaluation (`Critical > High > Normal > Idle`).
- **Global Scheduling Invariant (`I-SMP-SCHED-1`):**
  1. Each CPU maintains strict local priority ordering: `Critical > High > Normal > Idle`.
  2. Wakeups and work stealing preferentially place higher-priority work on available CPUs, and a higher-priority wakeup may trigger cross-CPU rescheduling.
  3. Concurrent execution on different CPUs does not establish a global total ordering between runnable threads.
  4. **Wakeup Routing (Idle or Partially Busy):** Awakened threads preferentially route to their home core if idle or executing lower priority; otherwise routed to lowest-numbered idle core.
  5. **Deterministic Preemption Under All-Cores-Busy:**
     - If home core runs $P_{home} < P_{woken}$: enqueue on home core and dispatch `IPI_VECTOR_RESCHEDULE` (252) to home core.
     - If $P_{home} \ge P_{woken}$: find remote core with lowest running priority $P_{min}$. If $P_{min} < P_{woken}$, migrate to $C_{min}$, enqueue, and dispatch `IPI_VECTOR_RESCHEDULE` (252) to $C_{min}$.
     - If all cores run $\ge P_{woken}$: enqueue on home core (FIFO tail for priority $P_{woken}$) without IPI; executes at next scheduling boundary.
  6. **Work Stealing:** Idle core steals from busy cores, checking `Critical` first, then `High`, then `Normal`. It steals from the tail, preserving head FIFO execution on the source core.

### ADR-3N-005: Thread Ownership Invariant (`I-SMP-THREAD-OWNER-1`) & Ascending Lock Migration
- **Invariant `I-SMP-THREAD-OWNER-1`:** At every instant in time, a runnable, running, or transitioning thread descriptor has **exactly one authoritative scheduler owner**.
- **Migration Protocol:**
  1. Threads can only be migrated when in `ThreadState::Ready`. A `Running` thread cannot be migrated until it yields.
  2. SchedLocks are acquired in strictly ascending CPU ID order: `min(cpu_a, cpu_b)` then `max(cpu_a, cpu_b)`.
  3. Thread is dequeued from CPU A, assigned CPU B, and enqueued on CPU B.
  4. Both locks are released in reverse order.
  5. If thread priority > CPU B's active thread priority, an `IPI_RESCHEDULE` is sent to CPU B.

### ADR-3N-006: Inter-Processor Interrupt (IPI) Architecture & Spurious Isolation
- **Dedicated Vectors:**
  - `IPI_VECTOR_STOP = 0xFB` (Vector 251): Emergency stop, panic halt, CPU offline evacuation.
  - `IPI_VECTOR_RESCHEDULE = 0xFC` (Vector 252): Triggers immediate `PerCpu.need_resched = 1` assertion and core preemption.
  - `IPI_VECTOR_TLB_SHOOTDOWN = 0xFD` (Vector 253): Triggers synchronous address-space / page TLB invalidation.
  - `LAPIC_SPURIOUS_VECTOR = 0xFF` (Vector 255): Dedicated Local APIC Spurious Interrupt handler (`iretq`, no EOI).
- **Bounded Polling & Iteration Limits:**
  - `MAX_IPI_POLL_ITERATIONS = 100_000` (ICR delivery status spin-wait).
  - `MAX_TLB_POLL_ITERATIONS = 1_000_000` (TLB shootdown ACK wait).
  - `MAX_RENDEZVOUS_POLL_TICKS = 10` (100 ms exit rendezvous deadline).
  - `MAX_AP_STARTUP_TICKS = 10` (100 ms AP boot deadline).

### ADR-3N-007: Synchronous TLB Shootdown (`I-SMP-TLB-1` .. `I-SMP-TLB-4`) & Unified Monotonic Generation
- **Active CPU Tracking:** Each `AddressSpace` contains an `active_cpu_mask: AtomicU64`, updated on CR3 switches (`I-SMP-ASPACE-4`).
- **Invariant `I-SMP-TLB-1`:** Address-space activation and page-table mutation are mutually serialized by `aspace.lock`. No CPU can register itself as an active CPU or load the mutated AddressSpace's CR3 between mutation serialization and completion of the shootdown without participating in that shootdown generation.
- **Invariant `I-SMP-TLB-2` (Tuple-Bound ACKs):** A TLB shootdown acknowledgement is valid if and only if both `ack.aspace_id == req.aspace_id` and `ack.generation == req.generation`. Stale or mismatched ACKs are rejected.
- **Invariant `I-SMP-TLB-3` (Release/Acquire Publication Protocol):** Each target CPU owns a dedicated cache-line aligned (64 B) `TlbTargetSlot` in `TLB_SHOOTDOWN_REQUEST[MAX_CPUS]`. The initiator populates `aspace_id`, `generation`, and `vaddr`, publishing via `active.store(true, Release)`. The target CPU consumes fields only after `active.load(Acquire)` observes true, executes `invlpg`, and writes `tlb_ack_gen.store(gen, Release)`. Initiator verifies via `Acquire` and clears with `active.store(false, Release)`.
- **Invariant `I-SMP-TLB-4` (Monotonic Non-Reused `aspace_id`):** Every live `AddressSpace` is assigned a unique, strictly monotonic, non-wrapping `aspace_id` from a global `AtomicU64`. IDs are never recycled, preventing tuple collisions over the system lifetime.
- **Dedicated Shootdown Lock (`TLB_SHOOTDOWN_LOCK`):** Level 10.5 lock acquired under `aspace.lock` (Level 11) to serialize broadcast sequences across AddressSpaces, preventing request slot clobbering.
- **Unified Monotonic Generation:** Single monotonic $u64$ generation counter per `AddressSpace` for mutations and exit barriers. Wraparound is forbidden (`u64::MAX` panics).

### ADR-3N-008: AddressSpace Destruction Rendezvous (`I-SMP-ASPACE-1` .. `I-SMP-ASPACE-4`)
- **Invariant `I-SMP-ASPACE-1`:** AddressSpace reclamation is forbidden until every CPU identified as an active executor has completed the rendezvous and acknowledged departure with matching exit generation.
- **Invariant `I-SMP-ASPACE-2`:** A Process in `Terminating`, `Zombie`, `Reclaiming`, or `Free` state cannot be newly activated on any CPU. Activation checks `process.state` under `aspace.lock`.
- **Invariant `I-SMP-ASPACE-3`:** An unresponsive CPU does not permit AddressSpace reclamation merely because its lifecycle state is marked `Failed`. If an active CPU fails to acknowledge departure within `MAX_RENDEZVOUS_POLL_TICKS`, the kernel triggers a fail-closed panic (`CRITICAL: Process exit rendezvous timeout — unquiesced CPU`), guaranteeing physical memory safety.
- **Invariant `I-SMP-ASPACE-4` (Atomic Mask & Departure Ordering):**
  1. During rendezvous, `process.state` is authoritative under `aspace.lock`; no `ProcessLock` is acquired, preserving lock hierarchy.
  2. If the initiating CPU has the process active, it switches `CR3 = MASTER_KERNEL_PML4` and clears its own bit: `active_cpu_mask.fetch_and(!(1 << self), Release)`.
  3. Remote CPUs intercept ISR 252, switch `CR3 = MASTER_KERNEL_PML4`, and clear their bit: `active_cpu_mask.fetch_and(!(1 << remote), Release)`.
  4. Initiator polls `active_cpu_mask.load(Acquire) == 0`. Full quiescence guarantees no CPU holds references to the page table hierarchy before frame deallocation.

### ADR-3N-009: Global 13-Level Monotonic Lock Ordering
To guarantee mathematical deadlock freedom across all multi-core subsystems:
```text
Level 1:    Block Device Lock (BLOCK_DEVICE_LOCK)
Level 2:    File Manager Lock (FILE_MANAGER_LOCK)
Level 3:    Device Registry Lock (DEVICE_REGISTRY_LOCK)
Level 4:    DMA Tracker Lock (DMA_TRACKER_LOCK)
Level 5:    Interrupt Subsystem Lock (INTERRUPT_LOCK)
Level 6:    Networking Locks (SOCKET_LOCK, IFACE_LOCK, PORT_LOCK, ROUTE_LOCK, ARP_LOCK)
Level 7:    IPC Locks (CHANNEL_LOCK, SHM_LOCK, OBJECT_TABLE_LOCK)
Level 8:    Capability Tree Lock (CAPABILITY_TREE_LOCK)
Level 9:    Process Table Lock (PROCESS_TABLE_LOCK)
Level 10:   Scheduler Locks (PER_CPU_STATE[A].lock < PER_CPU_STATE[B].lock for A < B)
Level 10.5: Global TLB Shootdown Lock (TLB_SHOOTDOWN_LOCK)
Level 11:   AddressSpace Lock (aspace.lock)
Level 12:   Process Lock (Process.lock)
Level 13:   CPU Interrupt Flag (IF = 0 / cli)
```

### ADR-3N-010: Static Memory Footprint & 4 MiB Bootstrap Window Amendment
- **Architectural Amendment (2 MiB → 4 MiB Bootstrap Window):**
  - **Previous Bound:** 2 MiB (`0xFFFFFFFF80200000`, single 512-entry `kernel_pt` under `kernel_pd[0]`).
  - **Amended Bound:** 4 MiB (`0xFFFFFFFF80400000`, dual 1024-entry page tables `kernel_pt` + `kernel_pt2` under `kernel_pd[0]` and `kernel_pd[1]`).
  - **Justification:** Cumulative growth across Stages 2A through 3N (14 complete subsystems including network, socket, device, filesystem, SMP runqueues, and stack guard) placed the kernel image end at `0xFFFFFFFF80205000` (5 pages beyond 2 MiB).
  - **Mathematical Disjointness Proof:**
    - Top Kernel Window VMA: `[0xFFFFFFFF80000000, 0xFFFFFFFF80400000)` (4 MiB).
    - HHDM Base: `0xFFFF800000000000` (PML4[256]).
    - Identity Map: `0x0000000000000000..0x0000000040000000` (PML4[0]).
    - Top 2 GiB: PML4[511], PDPT[510]. `kernel_pd` entries 0 and 1 map physical `0x000000..0x400000`.
    - Regions are strictly non-overlapping and disjoint by construction.

---

## Status
🟢 **APPROVED / FROZEN (Stage 3N Rev4 + Rev3 Protocol Amendments)**
