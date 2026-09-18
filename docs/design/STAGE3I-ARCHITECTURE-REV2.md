# Stage 3I Architecture Specification — Rev2

## Status: 🟡 PROPOSED FOR REVIEW & ARCHITECTURAL FREEZE (Reconciles Rev1 Blockers 1–12)

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

### I-SYSCALL-2 (and I-SYSCALL-FRAME-1): Syscall Frame Layout vs PreemptiveFrame
```text
I-SYSCALL-FRAME-1:
SyscallFrame is a distinct ABI structure saved on the thread's kernel stack
upon syscall entry. Stage 3C Cooperative (64 bytes) and Preemptive (160 bytes)
frame layouts MUST NOT be modified or conflated with SyscallFrame.
```

The `SyscallFrame` layout is defined as follows (168 bytes total, aligned to 16 bytes by assembly push sequence):
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallFrame {
    // Callee-saved registers (saved to guarantee kernel C-ABI freedom)
    pub r15: u64,       // +0
    pub r14: u64,       // +8
    pub r13: u64,       // +16
    pub r12: u64,       // +24
    pub rbx: u64,       // +32
    pub rbp: u64,       // +40

    // Argument registers
    pub r9:  u64,       // +48 (arg5)
    pub r8:  u64,       // +56 (arg4)
    pub r10: u64,       // +64 (arg3)
    pub rdx: u64,       // +72 (arg2)
    pub rsi: u64,       // +80 (arg1)
    pub rdi: u64,       // +88 (arg0)

    // Syscall number & return slot
    pub rax: u64,       // +96 (syscall nr on entry, return code on exit)

    // Hardware-saved user state
    pub user_rip:    u64, // +104 (from RCX)
    pub user_cs:     u64, // +112 (0x23)
    pub user_rflags: u64, // +120 (from R11)
    pub user_rsp:    u64, // +128 (from PerCpu scratch slot)
    pub user_ss:     u64, // +136 (0x1B)
}
```

### I-SYSCALL-3 (and I-SYSCALL-STACK-1, I-SYSCALL-GS-1): Stack & GS Transition
```text
I-SYSCALL-STACK-1:
Syscall entry switches from user RSP to the current thread's dedicated kernel stack.
The kernel stack pointer is obtained exclusively via PerCpu:
  swapgs
  mov [gs:scratch_user_rsp], rsp
  mov rsp, [gs:current_thread]
  mov rsp, [rsp + KERNEL_STACK_TOP_OFFSET]
Kernel stack pointer must be 16-byte aligned. If gs:current_thread is null,
the CPU triggers an unrecoverable kernel panic.

I-SYSCALL-GS-1:
Ring 0 GS_BASE holds &BSP_PERCPU at all times during kernel execution.
Ring 3 GS_BASE holds user application TLS/GS state (or 0).
On syscall entry, `swapgs` switches user GS to kernel PerCpu GS.
On sysret exit, `swapgs` restores user GS.
Hardware interrupts from Ring 3 check CS RPL; if RPL=3, execute swapgs on entry and exit.
If RPL=0, swapgs is NOT executed.
```

### I-SYSCALL-4 (and I-SYSCALL-RETURN-1): Syscall Return Validation & Fail-Closed Termination
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

FAIL-CLOSED TERMINATION:
If ANY condition in I-SYSCALL-RETURN-1 fails, the kernel SHALL NOT execute sysretq.
Instead, the kernel logs a security violation, marks the calling process as Zombie,
reclaims its resources, and invokes scheduler::schedule() to switch to the next ready thread.
```

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

### I-SYSCALL-7: Unified User Pointer Range Validation
Before kernel dereference or copy, every user buffer must be validated via `validate_user_range(ptr, len, access, vmm)`:
- `len == 0`: Trivial success (no-op).
- `ptr.checked_add(len)` must not overflow `u64`.
- `ptr >= USER_VA_MIN` and `ptr + len <= USER_VA_MAX_EXCLUSIVE`.
- For every 4 KiB page spanned: page must be mapped in current CR3, flags must contain `USER_ACCESSIBLE`, and flags must satisfy access mode:
  - `Read`: Present and User.
  - `Write`: Must contain `WRITABLE`.
  - `Execute`: Must NOT contain `NO_EXECUTE`.

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

### I-SYSCALL-12: Strict Memory Permissions & Isolation
All user-accessible virtual mappings must enforce strict $W \oplus X$ (Write XOR Execute):
- Executable code segments must be `RX` (`WRITABLE=0`, `NO_EXECUTE=0`).
- Data and stack segments must be `RW + NX` (`WRITABLE=1`, `NO_EXECUTE=1`).
- Shared memory mappings mapped read-only must be `RO + NX` (`WRITABLE=0`, `NO_EXECUTE=1`).
- Kernel aperture (`PML4[256..511]`) mapped in the process's page table has `USER_ACCESSIBLE=0` across all page directory levels. Any Ring 3 read or write to kernel memory triggers an immediate hardware Page Fault `#PF(0x05)`.

---

## 3. GDT & Fast Syscall MSR Configuration

### 3.1 Frozen GDT Layout
Project Zero's GDT (`kernel/src/hal/arch/x86_64/gdt.rs`):
- `0x08`: `KERNEL_CODE_SELECTOR` (DPL 0, RX, 64-bit).
- `0x10`: `KERNEL_DATA_SELECTOR` (DPL 0, RW).
- `0x1B`: `USER_DATA_SELECTOR` (`0x18 | 3`, DPL 3, RW).
- `0x23`: `USER_CODE_SELECTOR` (`0x20 | 3`, DPL 3, RX, 64-bit).
- `0x28`: `TSS_SELECTOR` (16-byte TSS descriptor).

### 3.2 MSR Setup Contract
On BSP initialization:
1. `IA32_EFER` (`0xC000_0080`): Set bit 0 (`SCE` = 1).
2. `IA32_STAR` (`0xC000_0081`):
   - Bits [47:32] = `0x0008` (Kernel CS base = `0x08`, Kernel SS base = `0x10`).
   - Bits [63:48] = `0x0010` (On `sysretq`: User CS = `(0x10 + 16) | 3 = 0x23`, User SS = `(0x10 + 8) | 3 = 0x1B`).
3. `IA32_LSTAR` (`0xC000_0082`): Address of `syscall_entry` assembly stub.
4. `IA32_FMASK` (`0xC000_0084`): `0x0000_0200` (`RFLAGS.IF` cleared on syscall entry). Also masks `DF` (bit 10) and `TF` (bit 8).
5. `IA32_KERNEL_GS_BASE` (`0xC000_0102`): Initialized to `&BSP_PERCPU`.

---

## 4. Syscall Entry & Exit Assembly Implementation

```nasm
global syscall_entry
extern syscall_dispatch_rust
extern syscall_fail_closed_terminate

syscall_entry:
    ; Hardware has atomically executed:
    ;   RCX <- User RIP
    ;   R11 <- User RFLAGS
    ;   RFLAGS <- RFLAGS & ~FMASK (IF=0, DF=0, TF=0)
    ;   CS <- 0x08, SS <- 0x10
    ;   RSP remains USER RSP!

    ; 1. Swap user GS base with kernel PerCpu GS base (I-SYSCALL-GS-1)
    swapgs

    ; 2. Save user RSP to scratch space in PerCpu and switch to thread kernel stack (I-SYSCALL-STACK-1)
    mov qword [gs:40], rsp              ; PerCpu.scratch_user_rsp (offset 40)
    mov rsp, qword [gs:16]              ; PerCpu.current_thread (offset 16)
    test rsp, rsp
    jz .fatal_null_thread               ; Catastrophic if null
    mov rsp, qword [rsp + 24]           ; KernelThread.stack_top (offset 24)

    ; 3. Push complete SyscallFrame (168 bytes, 16-B stack aligned before call)
    push qword 0x1B                     ; user_ss
    push qword [gs:40]                  ; user_rsp
    push r11                            ; user_rflags
    push qword 0x23                     ; user_cs
    push rcx                            ; user_rip
    push rax                            ; rax (syscall nr on entry)
    push rdi                            ; rdi (arg0)
    push rsi                            ; rsi (arg1)
    push rdx                            ; rdx (arg2)
    push r10                            ; r10 (arg3)
    push r8                             ; r8  (arg4)
    push r9                             ; r9  (arg5)
    push rbp                            ; callee-saved rbp
    push rbx                            ; callee-saved rbx
    push r12                            ; callee-saved r12
    push r13                            ; callee-saved r13
    push r14                            ; callee-saved r14
    push r15                            ; callee-saved r15

    ; 4. Re-enable interrupts now that we are safely on the thread kernel stack
    sti

    ; 5. Dispatch via Rust router
    mov rdi, rsp                        ; Arg0: &mut SyscallFrame
    call syscall_dispatch_rust

    ; 6. Disable interrupts for return path
    cli

    ; 7. Verify return safety invariants (I-SYSCALL-RETURN-1)
    ; Call Rust validator: returns 0 for valid, non-zero for invalid
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
    pop rax                             ; RAX contains return value written by dispatcher

    pop rcx                             ; User RIP restored for sysretq
    add rsp, 8                          ; Skip User CS
    pop r11                             ; User RFLAGS restored for sysretq
    pop rsp                             ; User RSP restored
    ; Skip User SS (RSP is now user stack)

    ; 9. Restore user GS base
    swapgs

    ; 10. Fast return to Ring 3
    o64 sysret

.fail_closed_exit:
    ; Security violation detected during return validation
    mov rdi, rsp
    call syscall_fail_closed_terminate
    ; Never returns

.fatal_null_thread:
    ; Unrecoverable kernel panic
    ud2
```

---

## 5. Freestanding User Bootstrap Architecture (`user/init`)

### 5.1 Freestanding Wrappers (`user/init/syscall.rs`)
```rust
#![no_std]

#[inline(always)]
pub unsafe fn sys_yield() -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 2, // SYS_YIELD
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}

#[inline(always)]
pub unsafe fn sys_exit(code: i32) -> ! {
    core::arch::asm!(
        "syscall",
        in("rax") 1, // SYS_EXIT
        in("rdi") code as u64,
        options(noreturn)
    );
}

#[inline(always)]
pub unsafe fn sys_channel_create(out_handles: *mut [u32; 2]) -> i64 {
    let ret: i64;
    core::arch::asm!(
        "syscall",
        in("rax") 3, // SYS_CHANNEL_CREATE
        in("rdi") out_handles as u64,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack)
    );
    ret
}
```

### 5.2 User Bootstrap Entry Point (`user/init/main.rs`)
```rust
#![no_std]
#![no_main]

mod syscall;
use syscall::{sys_yield, sys_exit};

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // 1. Cooperative yield 1
    sys_yield();

    // 2. Cooperative yield 2
    sys_yield();

    // 3. Clean exit
    sys_exit(0);
}
```

---

## 6. Verification Accounting Reconciliation

The authoritative baseline is frozen and preserved:
- **Existing Baseline**:
  - 25 host test files
  - 92 pytest test items
  - 181 bare-metal in-kernel machine tests (88 Stage 1..3F + 46 Stage 3G + 47 Stage 3H)
  - PMM neutrality: verified
- **Stage 3I Additions**:
  - 18 sequential bare-metal machine tests (`3I-A` through `3I-R`)
  - 1 host test file (`tests/test_stage3i.py`) containing 6 pytest items
- **Stage 3I Post-Verification Total**:
  - **199 bare-metal machine tests** ($181 + 18$)
  - **26 host test files** ($25 + 1$)
  - **98 pytest test items** ($92 + 6$)
  - PMM neutrality: strictly verified across full user process lifecycle.

### Complete Machine Test Enumeration (`3I-A` .. `3I-R`)
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
