# ADR-0011: Interrupt/Timer-Driven Preemptive Scheduling Architecture (Stage 3C — Revision 3)

## Status
Proposed (Stage 3C Design Milestone — Increment 1 Final Revision)

## Context
Stage 3A established the `KernelThread` descriptor table, guarded stack arena, and BSP `PerCpu` infrastructure. Stage 3B established the cooperative scheduler core (`switch_context`, intrusive `RunQueue`s, priority selection, dedicated idle thread, and `exit_current_thread`). Both Stage 3A and Stage 3B are **frozen**.

Stage 3C introduces hardware-driven preemptive execution:
1. A running thread can be involuntarily preempted by a hardware LAPIC timer interrupt without calling `yield_now()`.
2. The Local APIC (LAPIC) timer serves as the authoritative hardware tick source.
3. Preemptive and cooperative contexts must interoperate seamlessly without generic or ambiguous context switch routines.
4. The scheduler lock release-before-switch invariant (`sched_lock == FREE`, `IF == 0`, `GS+16 == next`) must be strictly maintained under preemption.
5. `PerCpu` must be explicitly extended with `need_resched` while preserving all existing field offsets.
6. Scope is strictly **single-core BSP** with **zero dynamic heap allocation**.

---

## Decision

### 1. Stage 3C `PerCpu` Extension (`need_resched`)

To support deterministic deferred preemption when timer ticks fire while preemption is disabled, `PerCpu` is explicitly extended from 40 bytes to 48 bytes (8-byte aligned).

#### 1.1 Field Layout & Compile-Time Assertions
```text
Byte Offset  Field                Type    Description
+0x00 ( 0):  self_ptr             *mut    Pointer to self
+0x08 ( 8):  cpu_id               u32     Logical CPU ID
+0x0C (12):  lapic_id             u32     Hardware LAPIC ID
+0x10 (16):  current_thread       *mut    Active KernelThread (GS + 16)
+0x18 (24):  idle_thread          *mut    Dedicated Idle KernelThread
+0x20 (32):  preempt_count        u32     Preemption disable nesting counter (GS + 32)
+0x24 (36):  nested_irq_count     u32     Interrupt nesting level (GS + 36)
+0x28 (40):  need_resched         u32     Deferred preemption request flag (GS + 40)
+0x2C (44):  _pad                 u32     Explicit padding for 8-byte alignment
Total Size: 48 bytes, Alignment: 8 bytes
```

#### 1.2 Preservation of Existing Machine Offsets
All Stage 3A machine-level offsets are preserved exactly:
- `GS + 16`: `current_thread`
- `GS + 32`: `preempt_count`
- `GS + 36`: `nested_irq_count`
- `GS + 40`: `need_resched` (new Stage 3C extension)

Compile-time assertions enforce all offsets, `size == 48`, and `align == 8`.

---

### 2. Separate Cooperative and Preemptive Frame ABIs

The frozen Stage 3B cooperative frame ABI remains 100% untouched. Stage 3C defines a distinct 160-byte preemptive frame ABI.

#### 2.1 Cooperative Frame ABI (64 bytes — Frozen Stage 3B)
Located at `saved_rsp` on the thread's stack:
```text
Byte Offset  Register / Content
[saved_rsp + 0x00]: RFLAGS (orig_rflags.IF merged into bit 9)
[saved_rsp + 0x08]: R15 (callee-saved)
[saved_rsp + 0x10]: R14 (callee-saved)
[saved_rsp + 0x18]: R13 (callee-saved)
[saved_rsp + 0x20]: R12 (callee-saved / initial argument)
[saved_rsp + 0x28]: RBP (callee-saved)
[saved_rsp + 0x30]: RBX (callee-saved / entry function pointer)
[saved_rsp + 0x38]: Return RIP (thread_bootstrap_entry or resumption RIP)
Total Size: 64 bytes (8 quadwords)
```
**Restoration sequence**:
```asm
popfq
pop r15
pop r14
pop r13
pop r12
pop rbp
pop rbx
ret
```

#### 2.2 Preemptive Interrupt Frame ABI (160 bytes — Stage 3C)
Located at `saved_rsp` on the thread's stack. Created by hardware CPU interrupt push (40 bytes) followed by ISR stub push of 15 general-purpose registers (120 bytes):
```text
Byte Offset         Register / Content      Source
[saved_rsp + 0x00]: R15                     Software (ISR stub push)
[saved_rsp + 0x08]: R14                     Software (ISR stub push)
[saved_rsp + 0x10]: R13                     Software (ISR stub push)
[saved_rsp + 0x18]: R12                     Software (ISR stub push)
[saved_rsp + 0x20]: R11                     Software (ISR stub push)
[saved_rsp + 0x28]: R10                     Software (ISR stub push)
[saved_rsp + 0x30]: R9                      Software (ISR stub push)
[saved_rsp + 0x38]: R8                      Software (ISR stub push)
[saved_rsp + 0x40]: RDI                     Software (ISR stub push)
[saved_rsp + 0x48]: RSI                     Software (ISR stub push)
[saved_rsp + 0x50]: RBP                     Software (ISR stub push)
[saved_rsp + 0x58]: RBX                     Software (ISR stub push)
[saved_rsp + 0x60]: RDX                     Software (ISR stub push)
[saved_rsp + 0x68]: RCX                     Software (ISR stub push)
[saved_rsp + 0x70]: RAX                     Software (ISR stub push)
[saved_rsp + 0x78]: RIP                     Hardware CPU push
[saved_rsp + 0x80]: CS                      Hardware CPU push
[saved_rsp + 0x88]: RFLAGS                  Hardware CPU push
[saved_rsp + 0x90]: RSP                     Hardware CPU push
[saved_rsp + 0x98]: SS                      Hardware CPU push
Total Size: 160 bytes (20 quadwords)
```
**Restoration sequence**:
```asm
pop r15
pop r14
pop r13
pop r12
pop r11
pop r10
pop r9
pop r8
pop rdi
pop rsi
pop rbp
pop rbx
pop rdx
pop rcx
pop rax
iretq
```

---

### 3. Four-Way Context Transition Matrix

Every context switch transitions from an outgoing thread context (`Cooperative` or `Preemptive`) to an incoming thread context (`Cooperative` or `Preemptive`). Rather than an ambiguous generic switch routine, each transition is explicitly defined:

| Transition Path | Outgoing Context Representation | Outgoing `saved_rsp` Population | Incoming `saved_rsp` Interpretation | Assembly Primitive Selected | Returns to Caller? | Final Instruction |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **1. Cooperative -> Cooperative** | 64-byte cooperative frame | Populated by `switch_context` via `mov [rdi], rsp` | Points to 64-byte frame | `switch_context(prev_rsp, next_rsp, orig_rflags)` | Yes (when resumed) | `ret` |
| **2. Cooperative -> Preemptive** | 64-byte cooperative frame | Populated by `switch_context_coop_to_preempt` via `mov [rdi], rsp` | Points to 160-byte frame | `switch_context_coop_to_preempt(prev_rsp, next_rsp, orig_rflags)` | No (jumps to interrupted code) | `iretq` |
| **3. Preemptive -> Cooperative** | 160-byte interrupt frame | Populated in ISR handler prior to switch: `outgoing.saved_rsp = RSP` | Points to 64-byte frame | `restore_context_cooperative(next_rsp)` | No (transfers to cooperative flow) | `ret` |
| **4. Preemptive -> Preemptive** | 160-byte interrupt frame | Populated in ISR handler prior to switch: `outgoing.saved_rsp = RSP` | Points to 160-byte frame | `restore_context_preemptive(next_rsp)` | No (jumps to interrupted code) | `iretq` |

---

### 4. Assembly Transfer ABI Specifications

All primitives are implemented in `boot/context.asm` with strictly typed `extern "C"` declarations in Rust.

#### 4.1 `switch_context` (Frozen Stage 3B Primitive — Coop -> Coop)
```rust
extern "C" {
    pub fn switch_context(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64);
}
```
- **Inputs**: `RDI = prev_rsp (*mut u64)`, `RSI = next_rsp (u64)`, `RDX = orig_rflags (u64)`.
- **Outgoing Action**: Pushes 64-byte cooperative frame (`rbx`, `rbp`, `r12..r15`, `pushfq`), merges `orig_rflags.IF` into `[rsp]`, stores `mov [rdi], rsp`.
- **Incoming Action**: Loads `mov rsp, rsi`, restores 64-byte cooperative frame (`popfq`, `pop r15..rbx`), executes `ret`.
- **Termination**: Terminates with `ret`. Returns to caller when outgoing thread is later resumed.

#### 4.2 `switch_context_coop_to_preempt` (New Primitive — Coop -> Preempt)
```rust
extern "C" {
    pub fn switch_context_coop_to_preempt(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64) -> !;
}
```
- **Inputs**: `RDI = prev_rsp (*mut u64)`, `RSI = next_rsp (u64)`, `RDX = orig_rflags (u64)`.
- **Outgoing Action**: Pushes 64-byte cooperative frame (`rbx`, `rbp`, `r12..r15`, `pushfq`), merges `orig_rflags.IF` into `[rsp]`, stores `mov [rdi], rsp`.
- **Incoming Action**: Loads `mov rsp, rsi`, restores 160-byte preemptive frame (`pop r15..r8`, `pop rdi..rax`), executes `iretq`.
- **Termination**: Terminates with `iretq`. Non-returning (`-> !`).

#### 4.3 `restore_context_cooperative` (New Primitive — Preempt -> Coop)
```rust
extern "C" {
    pub fn restore_context_cooperative(next_rsp: u64) -> !;
}
```
- **Inputs**: `RDI = next_rsp (u64)`.
- **Outgoing Action**: None (outgoing thread's 160-byte frame was already pushed by hardware + ISR stub, and `outgoing.saved_rsp` was stored in Rust prior to lock release).
- **Incoming Action**: Loads `mov rsp, rdi`, restores 64-byte cooperative frame (`popfq`, `pop r15..rbx`), executes `ret`.
- **Termination**: Terminates with `ret`. Non-returning (`-> !`).

#### 4.4 `restore_context_preemptive` (New Primitive — Preempt -> Preempt)
```rust
extern "C" {
    pub fn restore_context_preemptive(next_rsp: u64) -> !;
}
```
- **Inputs**: `RDI = next_rsp (u64)`.
- **Outgoing Action**: None (outgoing thread's 160-byte frame was already pushed by hardware + ISR stub, and `outgoing.saved_rsp` was stored in Rust prior to lock release).
- **Incoming Action**: Loads `mov rsp, rdi`, restores 160-byte preemptive frame (`pop r15..r8`, `pop rdi..rax`), executes `iretq`.
- **Termination**: Terminates with `iretq`. Non-returning (`-> !`).

---

### 5. Deferred Preemption Trigger & Execution Boundary

#### 5.1 Tick Processing Under `preempt_count > 0`
1. Timer ticks are **never discarded**. `total_ticks` is incremented and `current.quantum_remaining` is decremented.
2. If `current.quantum_remaining == 0` (or higher-priority thread becomes ready) while `preempt_count > 0`:
   - Preemption is deferred: `PerCpu.need_resched` is set to `1`.
   - The timer ISR finishes, sends LAPIC EOI, restores registers, and executes `iretq`, allowing the critical section to complete without interruption.

#### 5.2 Consumption Boundary: `preempt_enable()`
`need_resched` is evaluated synchronously at the **`preempt_enable()` execution boundary** when `preempt_count` transitions from `1 -> 0`:
```rust
#[inline(always)]
pub fn preempt_enable() {
    let percpu = unsafe { &mut *current_percpu_ptr() };
    assert!(percpu.preempt_count > 0, "preempt_count underflow");
    percpu.preempt_count -= 1;

    // Deferred preemption trigger condition
    if percpu.preempt_count == 0 && percpu.need_resched != 0 && percpu.nested_irq_count == 0 {
        // Confirm caller's execution state has interrupts enabled
        if cpu_if_bit() == 1 {
            percpu.need_resched = 0;
            // Triggers context switch via existing cooperative path!
            yield_now();
        }
    }
}
```

#### 5.3 Invariant Preservation
- **Execution Context**: `preempt_enable()` executes synchronously in normal thread context (not inside an ISR, so `nested_irq_count == 0`).
- **Trigger Mechanism**: Invoking `yield_now()` naturally executes the cooperative switch path (`SavedFrameType::Cooperative`).
- **Lock Invariant**: `yield_now()` acquires `sched_lock` with `cli`, performs state mutation, sets `GS+16 = next`, releases `sched_lock` leaving `IF=0`, and invokes `switch_context` or `switch_context_coop_to_preempt`.
- `sched_lock` is NEVER held across stack switching.

---

### 6. Nested Interrupt Scope & IDT Gate Semantics

1. **Interrupt Gate Semantics**:
   - CPU vector 32 is installed in the IDT as an **Interrupt Gate** (`type_attr = 0x8E`).
   - By x86-64 hardware architectural definition, entering an Interrupt Gate **automatically clears `RFLAGS.IF` (`IF = 0`)**.
2. **Single-Core BSP Scope**:
   - In Stage 3C, no nested hardware interrupts can occur during timer ISR execution because `IF` is 0 and no other unmasked hardware IRQs are enabled.
3. **Defensive Accounting**:
   - `nested_irq_count` (at `GS + 36`) is incremented on ISR entry and decremented on exit.
   - Preemption eligibility requires `nested_irq_count == 1`.
   - This serves as **defensive validation** rather than a claim of arbitrary nested interrupt scheduling.

---

### 7. Scheduler Decision Order (Timer Tick Handler)

```text
timer tick
    ↓
account elapsed quantum (total_ticks += 1, decrement quantum_remaining saturating at 0)
    ↓
send LAPIC EOI (lapic_eoi())
    ↓
if preempt_count > 0 || nested_irq_count > 1:
    if current.quantum_remaining == 0:
        PerCpu.need_resched = 1
    return to interrupted context (pop r15..rax, iretq)
    ↓
if higher-priority runnable thread exists in SCHEDULER:
    preempt current thread
else if current.quantum_remaining == 0:
    preempt current thread
else:
    return to interrupted context (pop r15..rax, iretq)
```

---

### 8. BSP First-Interrupt Context Rule

- In Stage 3B, the BSP thread was assumed to establish its first `saved_rsp` via cooperative `yield_now()`.
- In Stage 3C, the BSP may acquire its first saved context through a hardware timer interrupt.
- Therefore:
  - `BSP.saved_rsp` may initially represent a **Preemptive** frame (160 bytes).
  - The BSP descriptor is initialized in `init_scheduler` with `saved_rsp = 0` and `frame_type = SavedFrameType::Cooperative`.
  - If interrupted by the LAPIC timer before ever yielding, `BSP.saved_rsp` is populated from the 160-byte frame on the BSP stack, and `BSP.frame_type` is updated to `SavedFrameType::Preemptive`.
  - When later resumed, the scheduler selects `restore_context_preemptive`, correctly returning to the interrupted BSP execution via `iretq`.

---

### 9. Architectural Preservation of Stage 3B

1. **Cooperative ABI Frozen**: The 64-byte cooperative frame layout and `switch_context(prev_rsp, next_rsp, orig_rflags)` implementation remain frozen and unmodified.
2. **Zero Regressions**: All 69 existing tests across Stage 1, Stage 2, Stage 3A, and Stage 3B must pass unchanged.
3. **Zero Dynamic Allocation**: All descriptor tables and runqueues remain static/intrusive.
