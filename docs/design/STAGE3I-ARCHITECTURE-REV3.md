# Stage 3I Architecture Specification — Rev3

## Status: 🟡 PROPOSED FOR REVIEW & ARCHITECTURAL FREEZE (Reconciles Rev2 Issues 1–3)

---

## 1. Mission & Architectural Identity

Stage 3I introduces the initial **Ring 3 User-Space Execution Environment** and a strictly **Capability-Mediated System Call Boundary** for Project Zero on bare-metal x86-64.

### 1.1 Non-Negotiable Boundaries & ZeroOS Identity
1. **Not a POSIX / Unix Compatibility Layer**: Project Zero rejects ambient authority. There are no global file descriptors, no implicit process hierarchies conferring power, no `fork()`/`exec()`, no POSIX signals, and no pseudo-filesystems (`/proc`, `/sys`).
2. **Strict Authority Derivation Chain**:
   ```text
   USER INTENT
       ↓
   WORKSPACE
       ↓
   WORKLOAD / PROCESS (Isolated Resource Container, frozen 128-B ABI)
       ↓
   CAPABILITIES (Process-Local Handles -> CapabilityNode -> Kernel Object)
       ↓
   SYSCALL BOUNDARY (Hardware Privilege Gate: Ring 3 -> Ring 0)
       ↓
   KERNEL SUBSYSTEMS (Channel, ShmObject, Task, PMM/VMM)
   ```
3. **No Syscall Number Alone Confers Authority**: Every privileged syscall requires a valid user-visible `Handle`. The kernel validates the handle against `PROCESS_HANDLE_TABLES[proc_slot]` and `CAPABILITY_NODE_TABLE[proc_slot]`, verifying unforgeable generation tags, non-revocation state, and required bitmask rights before dispatching the underlying operation.
4. **Frozen 3A–3H Invariants Preserved**:
   - `PerCpu` struct ABI remains frozen at **exactly 48 bytes** (`I-PERCPU-1`).
   - `KernelThread` struct ABI remains frozen at **exactly 176 bytes** (`I-THREAD-1`).
   - `Process` struct ABI remains frozen at **exactly 128 bytes** (`I-PROC-1`).
   - `HandleTable` ABI remains frozen at **520 bytes** (`I-PROC-2`).
   - Zero kernel heap allocations (`.bss` static tables and stack-local state only).
   - Invariant `I-CAP-1` (1:1 HandleEntry ↔ CapabilityNode pairing).
   - Invariant `I-CAP-CLOSE-1` (Option B grandparent reparenting).
   - Invariant `I-CAP-TEARDOWN-1`, `2`, `3` (ownership-driven teardown, peer capability survival).
   - Monotonic lock hierarchy: $\mathbf{KERNEL\_OBJECT\_TABLE\_LOCK} \prec \mathbf{SCHEDULER.lock} \prec \mathbf{CPU\ (IF=0)}$.

---

## 2. Invariant Contracts (I-SYSCALL-1 through I-SYSCALL-12)

### I-SYSCALL-1: Syscall Register ABI Calling Convention
Syscall parameters and return values are transferred across the hardware boundary via fixed general-purpose registers:
- `RAX`: Syscall number on entry (`1..10`); 64-bit return code on exit ($\ge 0$ for success, negative for `-SyscallError`).
- `RDI`: Argument 0 (`Handle` or primary pointer/integer).
- `RSI`: Argument 1 (second parameter or length).
- `RDX`: Argument 2 (flags or secondary length).
- `R10`: Argument 3 (replaces `RCX`, which is clobbered by CPU `syscall`).
- `R8`:  Argument 4.
- `R9`:  Argument 5.
- `RCX`: Clobbered by CPU hardware; holds user return `RIP`.
- `R11`: Clobbered by CPU hardware; holds user return `RFLAGS`.
- Callee-saved registers (`RBX`, `RBP`, `R12`, `R13`, `R14`, `R15`) are strictly preserved across the syscall.

---

### I-SYSCALL-2 (and I-SYSCALL-FRAME-1): Exact SyscallFrame Byte Layout & Offset Assertions
```text
I-SYSCALL-FRAME-1:
SyscallFrame is a distinct, standalone ABI structure saved on the thread's kernel stack
upon syscall entry. Stage 3C Cooperative (64 bytes) and Preemptive (160 bytes)
frame layouts MUST NOT be modified or conflated with SyscallFrame.
```

The `SyscallFrame` layout is defined as follows: exactly **144 bytes** (18 quadwords), naturally 16-byte aligned ($144 = 16 \times 9$):

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallFrame {
    // ------------------------------------------------------------------------
    // [0..48] Callee-Saved Registers (kernel-saved for C-ABI safety)
    // ------------------------------------------------------------------------
    pub r15: u64,         // Offset 0x00 (0)  - Callee-saved R15
    pub r14: u64,         // Offset 0x08 (8)  - Callee-saved R14
    pub r13: u64,         // Offset 0x10 (16) - Callee-saved R13
    pub r12: u64,         // Offset 0x18 (24) - Callee-saved R12
    pub rbx: u64,         // Offset 0x20 (32) - Callee-saved RBX
    pub rbp: u64,         // Offset 0x28 (40) - Callee-saved RBP

    // ------------------------------------------------------------------------
    // [48..96] Dispatcher-Visible Syscall Arguments (passed in registers)
    // ------------------------------------------------------------------------
    pub r9:  u64,         // Offset 0x30 (48) - Argument 5
    pub r8:  u64,         // Offset 0x38 (56) - Argument 4
    pub r10: u64,         // Offset 0x40 (64) - Argument 3 (passed in R10 by user)
    pub rdx: u64,         // Offset 0x48 (72) - Argument 2
    pub rsi: u64,         // Offset 0x50 (80) - Argument 1
    pub rdi: u64,         // Offset 0x58 (88) - Argument 0 (Handle or pointer)

    // ------------------------------------------------------------------------
    // [96..104] Syscall Number & Return Value
    // ------------------------------------------------------------------------
    pub rax: u64,         // Offset 0x60 (96) - In: Syscall number; Out: Return code

    // ------------------------------------------------------------------------
    // [104..144] Hardware-Captured User Execution Context
    // ------------------------------------------------------------------------
    pub user_rip:    u64, // Offset 0x68 (104) - Captured from RCX by hardware syscall
    pub user_cs:     u64, // Offset 0x70 (112) - Ring 3 Code Selector (0x23)
    pub user_rflags: u64, // Offset 0x78 (120) - Captured from R11 by hardware syscall
    pub user_rsp:    u64, // Offset 0x80 (128) - User stack pointer
    pub user_ss:     u64, // Offset 0x88 (136) - Ring 3 Data Selector (0x1B)
}
```

#### Compile-Time Machine Offset & Size Assertions
```rust
const _: () = assert!(core::mem::size_of::<SyscallFrame>() == 144);
const _: () = assert!(core::mem::align_of::<SyscallFrame>() == 8);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r15) == 0);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r14) == 8);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r13) == 16);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r12) == 24);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rbx) == 32);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rbp) == 40);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r9) == 48);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r8) == 56);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, r10) == 64);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rdx) == 72);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rsi) == 80);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rdi) == 88);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, rax) == 96);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_rip) == 104);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_cs) == 112);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_rflags) == 120);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_rsp) == 128);
const _: () = assert!(core::mem::offset_of!(SyscallFrame, user_ss) == 136);
```

#### Structural Categorization
1. **Hardware-Provided State**: `user_rip` (from `RCX`), `user_rflags` (from `R11`), `user_cs` (`0x23`), `user_ss` (`0x1B`).
2. **Kernel-Saved Registers**: `r15`, `r14`, `r13`, `r12`, `rbx`, `rbp`.
3. **Dispatcher-Visible Arguments**: `rdi` (arg0), `rsi` (arg1), `rdx` (arg2), `r10` (arg3), `r8` (arg4), `r9` (arg5).
4. **Return State**: `rax` (syscall number on entry, 64-bit return code on exit).

---

### I-SYSCALL-3 (and I-SYSCALL-STACK-1, I-SYSCALL-GS-1): Stack & GS Transition Without Modifying PerCpu
```text
I-SYSCALL-STACK-1:
The frozen Stage 3C PerCpu layout (48 bytes) MUST NOT be modified.
Temporary user RSP storage during syscall entry uses a dedicated static .bss variable:
  pub static mut SYSCALL_SCRATCH_USER_RSP: u64 = 0;
The kernel stack pointer is obtained via PerCpu:
  swapgs
  mov [rel SYSCALL_SCRATCH_USER_RSP], rsp
  mov rsp, [gs:16]                        ; PerCpu.current_thread (offset 16)
  mov rsp, [rsp + 24]                     ; KernelThread.stack_top (offset 24)
Stack top is guaranteed 16-byte aligned. If gs:16 is null, CPU triggers unrecoverable panic.

I-SYSCALL-GS-1:
Ring 0 GS_BASE holds &BSP_PERCPU at all times during kernel execution.
Ring 3 GS_BASE holds user application TLS/GS state (or 0).
IA32_KERNEL_GS_BASE holds the swapped GS base.
```

---

### I-SYSCALL-ENTRY-1 & I-SYSCALL-GS-2: Complete Syscall / Interrupt State Machine
```text
I-SYSCALL-ENTRY-1:
Hardware `syscall` atomically clears `RFLAGS.IF` via `IA32_FMASK` (bit 9 set).
Therefore, `syscall_entry` begins execution with interrupts guaranteed DISABLED (IF=0).
The entire entry sequence:
  swapgs
  mov [rel SYSCALL_SCRATCH_USER_RSP], rsp
  mov rsp, [gs:16]
  mov rsp, [rsp + 24]
  push SyscallFrame (144 bytes)
executes atomically with IF=0. It CANNOT be interrupted or preempted.
Once SyscallFrame is completely saved on the thread's kernel stack, interrupts may be
re-enabled (sti) if preemptible execution is desired.
On exit, `cli` disables interrupts before restoring context, executing swapgs, and sysretq.
```

#### Complete Entry / Exit Privilege & GS State Machine (`I-SYSCALL-GS-2`)
```text
               +-------------------------------------------+
               | User Space Execution (Ring 3)             |
               | GS_BASE = User TLS (or 0)                 |
               | KERNEL_GS_BASE = &BSP_PERCPU              |
               | CPL = 3, IF = 1                           |
               +-------------------------------------------+
                       |                           |
            syscall    |                           | Hardware Interrupt
     (Hardware: IF=0)  |                           | (Hardware pushes 5 words)
                       v                           v
     +-----------------------------------+   +-----------------------------------+
     | syscall_entry (IF=0)              |   | interrupt_entry (IF=0)            |
     | 1. swapgs                         |   | 1. Check saved CS (RPL == 3?)     |
     |    GS_BASE = &BSP_PERCPU          |   |    YES -> swapgs                  |
     |    KERNEL_GS_BASE = User TLS      |   |    NO  -> do not swapgs (from R0) |
     | 2. mov [scratch_user_rsp], rsp    |   | 2. Save registers (PreemptiveFrame)|
     | 3. mov rsp, [gs:16].stack_top     |   +-----------------------------------+
     | 4. Push SyscallFrame (144 B)      |                 |
     +-----------------------------------+                 |
                       |                                   v
                       v                     +-----------------------------------+
     +-----------------------------------+   | Kernel ISR / Preemption           |
     | Kernel Syscall Dispatcher         |   | GS_BASE = &BSP_PERCPU             |
     | GS_BASE = &BSP_PERCPU             |   | Nested IRQ uses normal R0 stack   |
     | (May enable IF=1 for blocking)    |   +-----------------------------------+
     +-----------------------------------+                 |
                       |                                   v
                       v                     +-----------------------------------+
     +-----------------------------------+   | interrupt_exit (IF=0)             |
     | syscall_exit (cli, IF=0)          |   | 1. Restore registers              |
     | 1. Validate return state          |   | 2. Check saved CS (RPL == 3?)     |
     | 2. Pop SyscallFrame               |   |    YES -> swapgs                  |
     | 3. pop rsp (Restores user RSP)    |   |    NO  -> do not swapgs           |
     | 4. swapgs                         |   | 3. iretq                          |
     |    GS_BASE = User TLS             |   +-----------------------------------+
     |    KERNEL_GS_BASE = &BSP_PERCPU   |                 |
     | 5. sysretq                        |                 |
     +-----------------------------------+                 |
                       |                                   |
                       +-----------------+-----------------+
                                         |
                                         v
                       +-------------------------------------------+
                       | User Space Execution Resumed (Ring 3)     |
                       | GS_BASE = User TLS (or 0)                 |
                       | KERNEL_GS_BASE = &BSP_PERCPU              |
                       | CPL = 3, IF = 1                           |
                       +-------------------------------------------+
```

#### Nested Interrupt Invariant
When a hardware interrupt occurs while the kernel is executing a syscall (CPL = 0):
- Hardware pushes `[SS, RSP, RFLAGS, CS, RIP]` onto the current kernel stack.
- The interrupt stub inspects saved `CS` at `[rsp + 8]`. Because `(CS & 3) == 0` (interrupted from Ring 0), the stub **does not execute `swapgs`**.
- `GS_BASE` remains `&BSP_PERCPU` uninterrupted throughout the nested interrupt.
- Upon `iretq`, because returning to Ring 0, the stub **does not execute `swapgs`**.

---

### I-SYSCALL-4 (and I-SYSCALL-RETURN-1, I-SYSCALL-RETURN-2): Syscall Return Validation & Fail-Closed Termination
```text
I-SYSCALL-RETURN-1:
sysretq is executed ONLY after the kernel dispatcher has established:
  1. User return RIP is canonical and inside [USER_VA_MIN .. USER_VA_MAX_EXCLUSIVE).
  2. User return RSP is canonical and inside [USER_VA_MIN .. USER_VA_MAX_EXCLUSIVE).
  3. User RFLAGS satisfies:
       - RFLAGS.IF (bit 9) == 1 (maskable interrupts enabled in user space)
       - RFLAGS.IOPL (bits 12-13) == 0 (no I/O port privilege in user space)
       - RFLAGS.NT (bit 14) == 0
       - RFLAGS.TF (bit 8) == 0 (no trap flag)
       - RFLAGS.VM (bit 17) == 0 (no virtual-8086 mode)
       - Reserved bit 1 == 1
  4. RCX contains validated user return RIP; R11 contains validated user RFLAGS.
  5. Current CR3 matches current process PML4 physical address.
  6. GDT selectors correspond to frozen Ring 3 descriptors (CS=0x23, SS=0x1B).

I-SYSCALL-RETURN-2 (FAIL-CLOSED TERMINATION):
If ANY condition in I-SYSCALL-RETURN-1 fails, the kernel SHALL NOT execute sysretq.
Instead, the kernel logs a security violation, marks the calling process as Zombie,
reclaims its resources, and invokes scheduler::schedule() to switch to the next ready thread.
```

---

### I-SYSCALL-5 (and I-SYSCALL-VA-1): User Virtual Address Boundaries
Exact named boundaries are frozen:
```rust
pub const USER_NULL_GUARD_START: u64   = 0x0000_0000_0000_0000;
pub const USER_NULL_GUARD_END: u64     = 0x0000_0000_0020_0000; // 2 MiB (unmapped)

pub const USER_VA_MIN: u64             = 0x0000_0000_0020_0000; // Inclusive lower boundary
pub const USER_CODE_BASE: u64          = 0x0000_0000_0020_0000; // Text segment base (RX)
pub const USER_DATA_BASE: u64          = 0x0000_0000_0040_0000; // Static data segment base (RW+NX)
pub const USER_SHM_BASE: u64           = 0x0000_0000_2000_0000; // Shared memory base (RW+NX or RO+NX)

pub const USER_STACK_BASE: u64         = 0x0000_7F7F_FFFC_0000; // Stack bottom (16 KiB / 4 pages)
pub const USER_STACK_TOP: u64          = 0x0000_7F7F_FFFF_0000; // Initial user RSP (16-B aligned)

pub const USER_UPPER_GUARD_START: u64  = 0x0000_7F80_0000_0000; // 2 GiB upper guard window
pub const USER_UPPER_GUARD_END: u64    = 0x0000_8000_0000_0000; // Non-canonical boundary

pub const USER_VA_MAX_EXCLUSIVE: u64   = 0x0000_7F80_0000_0000; // Exclusive upper boundary
```
All user pointers and buffer ranges `[ptr .. ptr + len)` must satisfy:
$$\mathtt{USER\_VA\_MIN} \le \mathtt{ptr} \quad \text{and} \quad \mathtt{ptr} + \mathtt{len} \le \mathtt{USER\_VA\_MAX\_EXCLUSIVE}$$

---

### I-SYSCALL-6 (and I-SYSCALL-BLOCK-1): Syscall Blocking & Scheduler Continuation
```text
I-SYSCALL-BLOCK-1:
When a thread blocks inside a synchronous syscall (e.g. SYS_CHANNEL_RECEIVE on an empty ring):
  1. SyscallFrame remains intact on the thread's dedicated 16 KiB kernel stack.
  2. The blocking subsystem transitions thread state to ThreadState::Blocked(reason).
  3. The scheduler invokes `switch_context()`, pushing the frozen Stage 3C CooperativeFrame (64 bytes)
     on top of the thread's kernel stack.
  4. When an IPC sender wakes the blocked thread, the thread transitions to ThreadState::Ready.
  5. Upon being scheduled, restore_cooperative_context pops the CooperativeFrame and returns into
     the blocking function, which yields the received message and returns to syscall_dispatch_rust.
  6. The return value is written into frame.rax.
  7. Assembly exit restores SyscallFrame, validates return state (I-SYSCALL-RETURN-1), executes
     swapgs, and returns to Ring 3 via sysretq.
```

---

### I-SYSCALL-7: Unified User Pointer Range Validation
Before kernel dereference or copy, every user buffer must be validated via `validate_user_range(ptr, len, access, vmm)`:
- `len == 0`: Trivial success (no-op).
- `ptr.checked_add(len)` must not overflow `u64`.
- `ptr >= USER_VA_MIN` and `ptr + len <= USER_VA_MAX_EXCLUSIVE`.
- For every 4 KiB page spanned: page must be mapped in current CR3, flags must contain `USER_ACCESSIBLE`, and flags must satisfy access mode:
  - `Read`: Present and User.
  - `Write`: Must contain `WRITABLE`.
  - `Execute`: Must NOT contain `NO_EXECUTE`.

---

### I-SYSCALL-8: Syscall Number & Frozen Signature Table
Stage 3I freezes exactly 10 syscalls:

| # | Name | ABI Signature | Regs Used | Max Len | Ptr Dir | Return | Blocking? |
| :-: | :--- | :--- | :--- | :-: | :-: | :--- | :-: |
| `1` | `sys_exit` | `(code: i32) -> !` | `RAX, RDI` | N/A | None | Never returns | No |
| `2` | `sys_yield` | `() -> i64` | `RAX` | N/A | None | `0` (OK) | Yields CPU |
| `3` | `sys_channel_create` | `(out_handles: *mut [Handle; 2]) -> i64` | `RAX, RDI` | 16 B | OUT | `0` or `-Error` | No |
| `4` | `sys_channel_send` | `(h: Handle, msg: *const IpcMessage, flags: u32) -> i64` | `RAX, RDI, RSI, RDX` | 80 B | IN | `0` or `-Error` | No |
| `5` | `sys_channel_receive` | `(h: Handle, msg: *mut IpcMessage, flags: u32) -> i64` | `RAX, RDI, RSI, RDX` | 80 B | OUT | `0` or `-Error` | **Yes (if empty)** |
| `6` | `sys_channel_close` | `(h: Handle) -> i64` | `RAX, RDI` | N/A | None | `0` or `-Error` | No |
| `7` | `sys_shm_create` | `(pages: usize, out_h: *mut Handle) -> i64` | `RAX, RDI, RSI` | 8 B | OUT | `0` or `-Error` | No |
| `8` | `sys_shm_map` | `(h: Handle, vaddr: u64, writable: bool) -> i64` | `RAX, RDI, RSI, RDX` | N/A | None | `0` or `-Error` | No |
| `9` | `sys_shm_unmap` | `(h: Handle, vaddr: u64) -> i64` | `RAX, RDI, RSI` | N/A | None | `0` or `-Error` | No |
| `10`| `sys_cap_derive` | `(h: Handle, rights: u32, out_h: *mut Handle) -> i64` | `RAX, RDI, RSI, RDX` | 8 B | OUT | `0` or `-Error` | No |

---

### I-SYSCALL-9: Capability Rights & Creation Authority Table
Every syscall is explicitly classified by authority source:

| Syscall | Handle Input? | Authority Type | Required Capability Rights |
| :--- | :-: | :--- | :--- |
| `sys_exit` | No | Process Self-Authority | Transitions caller to Zombie; releases owned capabilities |
| `sys_yield` | No | Thread Self-Authority | Relinquishes remaining time quantum to scheduler |
| `sys_channel_create` | No | Process Quota Authority | Permitted if process owned channels < `MAX_CHANNELS_PER_PROC` |
| `sys_channel_send` | **Yes** | Capability Right | `cap_rights::CHANNEL_SEND` |
| `sys_channel_receive` | **Yes** | Capability Right | `cap_rights::CHANNEL_RECEIVE` |
| `sys_channel_close` | **Yes** | Capability Right | `cap_rights::CLOSE` (triggers Option B reparenting) |
| `sys_shm_create` | No | Process Quota Authority | Permitted if process owned SHM frames < `MAX_SHM_PAGES_PER_PROC` |
| `sys_shm_map` | **Yes** | Capability Right | Read-only: `SHM_MAP_READ`; Writable: `SHM_MAP_READ \| SHM_MAP_WRITE` |
| `sys_shm_unmap` | **Yes** | Capability Right | `cap_rights::SHM_UNMAP` |
| `sys_cap_derive` | **Yes** | Capability Right | `cap_rights::DUPLICATE` (requires requested rights $\subseteq$ parent rights) |

*Zero Ambient Authority Reconciliation*: Object creation (`sys_channel_create`, `sys_shm_create`) does NOT grant ambient authority. It allocates isolated, unshared kernel objects bound strictly within the calling process's static resource quota. Access is strictly confined to the newly minted handle returned to the caller.

---

### I-SYSCALL-10: User Bootstrap Binary & Embedding
```text
I-SYSCALL-BOOT-1:
Freestanding user payloads are authored in `user/init/main.rs` and `user/init/syscall.rs`
without standard library dependencies (#![no_std], #![no_main]).
The compiled freestanding payload is statically embedded in the kernel image as:
  pub static USER_INIT_PAYLOAD: &[u8] = &[ ... ];
The kernel loader:
  1. Allocates 1 physical PMM frame for code.
  2. Copies USER_INIT_PAYLOAD into the frame via HHDM.
  3. Maps the frame at USER_CODE_BASE (0x0020_0000) with flags PRESENT | USER_ACCESSIBLE (RX).
  4. Allocates 4 physical PMM frames for stack.
  5. Maps them at [USER_STACK_BASE .. USER_STACK_TOP) as PRESENT | USER_ACCESSIBLE | WRITABLE | NO_EXECUTE (RW+NX).
```

---

### I-SYSCALL-11 (and I-SYSCALL-FRAME-TYPE-1): Initial Ring 3 Entry via Trampoline
```text
I-SYSCALL-FRAME-TYPE-1:
SavedFrameType MUST NOT be modified. It remains strictly:
  SavedFrameType::Cooperative (64 bytes)
  SavedFrameType::Preemptive (160 bytes)
User thread initialization DOES NOT introduce SavedFrameType::User.
Instead, a new user thread is initialized as a standard cooperative thread whose entry
function is `user_thread_bootstrap_trampoline`.
When the scheduler switches to this thread for the first time, it restores CooperativeFrame
and jumps into `user_thread_bootstrap_trampoline`.
The trampoline pushes the initial 40-byte iretq frame:
  push 0x1B                      ; User SS (USER_DATA_SELECTOR)
  push USER_STACK_TOP            ; User RSP (0x0000_7F7F_FFFF_0000)
  push 0x0202                    ; User RFLAGS (IF=1, Reserved bit 1=1)
  push 0x23                      ; User CS (USER_CODE_SELECTOR)
  push USER_CODE_BASE            ; User RIP (0x0000_0000_0020_0000)
  iretq                          ; Drops privilege to Ring 3 atomically
```

---

### I-SYSCALL-12: Strict Memory Permissions & Isolation
All user-accessible virtual mappings must enforce strict $W \oplus X$ (Write XOR Execute):
- Executable code segments must be `RX` (`WRITABLE=0`, `NO_EXECUTE=0`).
- Data and stack segments must be `RW + NX` (`WRITABLE=1`, `NO_EXECUTE=1`).
- Shared memory mappings mapped read-only must be `RO + NX` (`WRITABLE=0`, `NO_EXECUTE=1`).
- Kernel aperture (`PML4[256..511]`) mapped in the process's page table has `USER_ACCESSIBLE=0` across all page directory levels. Any Ring 3 read or write to kernel memory triggers an immediate hardware Page Fault `#PF(0x05)`.

---

## 3. Syscall Entry & Exit Assembly Implementation

```nasm
global syscall_entry
global SYSCALL_SCRATCH_USER_RSP
extern syscall_dispatch_rust
extern syscall_validate_return_rust
extern syscall_fail_closed_terminate

section .bss
align 8
SYSCALL_SCRATCH_USER_RSP:
    resq 1

section .text
syscall_entry:
    ; Hardware has atomically executed:
    ;   RCX <- User RIP
    ;   R11 <- User RFLAGS
    ;   RFLAGS <- RFLAGS & ~FMASK (IF=0, DF=0, TF=0)
    ;   CS <- 0x08, SS <- 0x10
    ;   RSP remains USER RSP!

    ; 1. Swap user GS base with kernel PerCpu GS base (I-SYSCALL-GS-1)
    swapgs

    ; 2. Save user RSP to static scratch space and switch to thread kernel stack (I-SYSCALL-STACK-1)
    mov [rel SYSCALL_SCRATCH_USER_RSP], rsp
    mov rsp, [gs:16]                    ; PerCpu.current_thread (offset 16, frozen 48-B PerCpu)
    test rsp, rsp
    jz .fatal_null_thread               ; Catastrophic if null
    mov rsp, [rsp + 24]                 ; KernelThread.stack_top (offset 24, frozen 176-B KernelThread)

    ; 3. Push complete SyscallFrame (144 bytes = 18 pushes, 16-B aligned)
    push qword 0x1B                     ; user_ss (+136)
    push qword [rel SYSCALL_SCRATCH_USER_RSP] ; user_rsp (+128)
    push r11                            ; user_rflags (+120)
    push qword 0x23                     ; user_cs (+112)
    push rcx                            ; user_rip (+104)
    push rax                            ; rax: syscall nr on entry (+96)
    push rdi                            ; rdi: arg0 (+88)
    push rsi                            ; rsi: arg1 (+80)
    push rdx                            ; rdx: arg2 (+72)
    push r10                            ; r10: arg3 (+64)
    push r8                             ; r8:  arg4 (+56)
    push r9                             ; r9:  arg5 (+48)
    push rbp                            ; callee-saved rbp (+40)
    push rbx                            ; callee-saved rbx (+32)
    push r12                            ; callee-saved r12 (+24)
    push r13                            ; callee-saved r13 (+16)
    push r14                            ; callee-saved r14 (+8)
    push r15                            ; callee-saved r15 (+0)

    ; 4. Re-enable interrupts now that we are safely on the thread kernel stack
    sti

    ; 5. Dispatch via Rust router
    mov rdi, rsp                        ; Arg0: &mut SyscallFrame
    call syscall_dispatch_rust

    ; 6. Disable interrupts for return path
    cli

    ; 7. Verify return safety invariants (I-SYSCALL-RETURN-1)
    mov rdi, rsp
    call syscall_validate_return_rust
    test rax, rax
    jnz .fail_closed_exit

    ; 8. Restore user context
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    pop r9
    pop r8
    pop r10
    pop rdx
    pop rsi
    pop rdi
    pop rax                             ; RAX contains return code written by dispatcher

    pop rcx                             ; User RIP restored for sysretq
    add rsp, 8                          ; Skip User CS
    pop r11                             ; User RFLAGS restored for sysretq
    pop rsp                             ; User RSP restored directly from stack!
    ; Skip User SS (RSP is now pointing to user stack)

    ; 9. Restore user GS base
    swapgs

    ; 10. Fast return to Ring 3
    o64 sysret

.fail_closed_exit:
    ; Security violation detected during return validation (I-SYSCALL-RETURN-2)
    mov rdi, rsp
    call syscall_fail_closed_terminate
    ; Never returns

.fatal_null_thread:
    ; Unrecoverable kernel panic
    ud2
```

---

## 4. Verification Accounting & Planned Targets

### 4.1 Authoritative Verified Baseline (Frozen)
- **Host Test Files**: 25 files
- **Host Pytest Items**: 92 items
- **Bare-Metal In-Kernel Machine Tests**: 181 tests (Stage 1..3F: 88, Stage 3G: 46, Stage 3H: 47)
- **PMM Neutrality**: Verified

### 4.2 Planned Stage 3I Additions (Target Accounting)
- **Planned Machine Tests**: 18 sequential machine tests (`3I-A` through `3I-R`)
- **Planned Host Test File**: 1 new file (`tests/test_stage3i.py`) containing 6 pytest items
- **Target Post-Stage 3I Totals (To be verified upon execution)**:
  - **199 bare-metal machine tests** ($181 + 18$)
  - **26 host test files** ($25 + 1$)
  - **98 pytest items** ($92 + 6$)
  - PMM neutrality strictly verified across user process lifecycle.

### 4.3 Complete Machine Test Matrix (`3I-A` .. `3I-R`)
- **3I-A**: Fast syscall MSR initialization (`EFER.SCE`, `STAR`, `LSTAR`, `FMASK`).
- **3I-B**: User process creation & isolated address space aperture (`USER=1` lower half, `USER=0` upper half).
- **3I-C**: Strict W^X memory permissions on user code (`RX`) and user stack (`RW+NX`).
- **3I-D**: Initial kernel-to-user privilege transition via trampoline `iretq` (CS=`0x23`, SS=`0x1B`, Ring 3).
- **3I-E**: User code execution in Ring 3.
- **3I-F**: Fast `syscall` hardware transition from Ring 3 to Ring 0 with full register capture.
- **3I-G**: `sys_yield` round-robin scheduler invocation from user space.
- **3I-H**: Unknown syscall number rejection (`-ENOSYS`).
- **3I-I**: User pointer validation: null pointer rejection (`-EFAULT`).
- **3I-J**: User pointer validation: kernel-address pointer rejection (`-EFAULT`).
- **3I-K**: User pointer validation: non-canonical address rejection (`-EFAULT`).
- **3I-L**: User pointer validation: integer overflow rejection (`-EFAULT`).
- **3I-M**: User pointer validation: multi-page boundary and write permission enforcement.
- **3I-N**: Capability-authorized IPC: `sys_channel_create`, `send`, `receive`, `close`.
- **3I-O**: Syscall rejection on forged handle or missing capability rights.
- **3I-P**: Shared memory lifecycle: `sys_shm_create`, `sys_shm_map`, `sys_shm_unmap`.
- **3I-Q**: `sys_exit` process lifecycle termination and zombie transition.
- **3I-R**: PMM frame neutrality across complete user process lifecycle.
