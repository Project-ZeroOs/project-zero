# Stage 3I Architecture Specification — Rev1

## Status: 🟡 PROPOSED FOR REVIEW & ARCHITECTURAL FREEZE

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

## 2. User Virtual Address Space Layout & Geometry

### 2.1 Hardware Geometry & Canonical Addressing
- **Paging Model**: 4-level x86-64 paging (`CR4.PAE = 1`, `IA32_EFER.LME = 1`, `IA32_EFER.NXE = 1`).
- **Discovered Geometry**: Evaluated dynamically via CPUID `0x80000008` through `AddressSpaceGeometry`.
- **Canonical Address Invariant**:
  - Lower Half (User Space): `0x0000_0000_0000_0000` to `0x0000_7FFF_FFFF_FFFF` (PML4 entries `0..255`).
  - Non-Canonical Hole: `0x0000_8000_0000_0000` to `0xFFFF_7FFF_FFFF_FFFF` (Access triggers immediate hardware `#GP(0)`).
  - Higher Half (Kernel Space): `0xFFFF_8000_0000_0000` to `0xFFFF_FFFF_FFFF_FFFF` (PML4 entries `256..511`).
    - `PML4[256]`: Higher-Half Direct Map (`HHDM_BASE = 0xFFFF_8000_0000_0000`).
    - `PML4[511]`: Kernel Code, BSS, IDT, GDT, Stacks (`KERNEL_VIRT_BASE = 0xFFFF_FFFF_8000_0000`).

### 2.2 User Virtual Memory Map (Lower Half)
The user aperture is strictly isolated in PML4 indices `0..255`. Intermediate page tables (PML4, PDPT, PD, PT) along user-mapped virtual paths have `PageTableFlags::USER_ACCESSIBLE` (bit 2) set. The kernel aperture (`PML4[256..511]`) is cloned into each user process with `USER=0` (supervisor only), rendering kernel memory completely inaccessible to Ring 3 execution.

```text
+------------------------------------+ 0x0000_8000_0000_0000 (Non-canonical boundary)
| Guard Window (Unmapped)            | 2 GiB unmapped zone [0x0000_7F80_0000_0000 .. 0x0000_8000_0000_0000)
+------------------------------------+ 0x0000_7F7F_FFFF_0000 (USER_STACK_TOP, 16-B aligned)
| USER STACK REGION                  | 16 KiB (4 frames) allocated from PMM, RW + NX
| [0x0000_7F7F_FFFC_0000 .. TOP)     | Grows downward
+------------------------------------+ 0x0000_7F7F_FFFB_0000
| Stack Guard Page (Unmapped)        | 4 KiB unmapped page to trap stack overflow
+------------------------------------+ 0x0000_7F7F_FFFA_0000
| ... (Unmapped user virtual space)  |
+------------------------------------+ 0x0000_4000_0000_0000
| USER HEAP REGION (Future Stage 3J) | Reserved for user-level allocator
| [0x0000_4000_0000_0000 .. )        |
+------------------------------------+ 0x0000_2000_0000_0000
| USER SHM REGION                    | Dynamic capability-mapped shared memory
| [0x0000_2000_0000_0000 .. )        | Mapped via sys_shm_map, RO+NX or RW+NX
+------------------------------------+ 0x0000_0040_0000_0000
| USER DATA REGION                   | Global/static user data, RW + NX
| [0x0000_0040_0000_0000 .. )        |
+------------------------------------+ 0x0000_0020_0000_0000
| USER CODE REGION                   | Executable user payload, RX (No-Execute = 0, Writable = 0)
| [0x0000_0020_0000_0000 .. )        |
+------------------------------------+ 0x0000_0000_0020_0000
| Null Guard Window (Unmapped)       | First 2 MiB unmapped to trap null/low pointer dereferences
+------------------------------------+ 0x0000_0000_0000_0000
```

### 2.3 Authoritative Address Space Constants
```rust
pub const USER_VA_MIN: u64        = 0x0000_0000_0020_0000; // 2 MiB
pub const USER_VA_MAX: u64        = 0x0000_7FFF_FFFF_0000; // Boundary of user space
pub const USER_CODE_BASE: u64     = 0x0000_0000_0020_0000; // Base of user text segment
pub const USER_DATA_BASE: u64     = 0x0000_0000_0040_0000; // Base of user data segment
pub const USER_SHM_BASE: u64      = 0x0000_0000_2000_0000; // Base of dynamic SHM mappings
pub const USER_STACK_TOP: u64     = 0x0000_7F7F_FFFF_0000; // Initial RSP for user thread
pub const USER_STACK_PAGES: usize = 4;                      // 16 KiB user stack
```

---

## 3. User / Kernel Privilege Model & Transitions

### 3.1 GDT Segment Selectors & TSS Integration
Project Zero's frozen GDT (`kernel/src/hal/arch/x86_64/gdt.rs`) establishes:
- **Descriptor 1 (0x08)**: `KERNEL_CODE_SELECTOR` — Ring 0 Code Segment (DPL 0).
- **Descriptor 2 (0x10)**: `KERNEL_DATA_SELECTOR` — Ring 0 Data Segment (DPL 0).
- **Descriptor 3 (0x1B = 0x18 | 3)**: `USER_DATA_SELECTOR` — Ring 3 Data Segment (DPL 3).
- **Descriptor 4 (0x23 = 0x20 | 3)**: `USER_CODE_SELECTOR` — Ring 3 Code Segment (DPL 3).
- **Descriptors 5 & 6 (0x28)**: `TSS_SELECTOR` — 16-byte TSS descriptor containing `RSP0` (Kernel Stack).

### 3.2 Dual Transition Mechanism
Stage 3I specifies a clean, architecturally validated dual transition design:

#### 1. Initial Kernel-to-User Entry via `iretq`
When dispatching a newly created user thread for the first time:
- The kernel sets up an initial **Interrupt Return Frame** on the thread's kernel stack:
  ```text
  [Kernel Stack Top - 40] SS     = 0x1B (USER_DATA_SELECTOR)
  [Kernel Stack Top - 32] RSP    = USER_STACK_TOP (0x0000_7F7F_FFFF_0000)
  [Kernel Stack Top - 24] RFLAGS = 0x0202 (IF=1, Reserved bit 1=1)
  [Kernel Stack Top - 16] CS     = 0x23 (USER_CODE_SELECTOR)
  [Kernel Stack Top - 8]  RIP    = USER_CODE_BASE (0x0000_0000_0020_0000)
  ```
- Executing `iretq` pops these 5 values atomically, dropping hardware privilege from CPL=0 to CPL=3, loading user `CS` (`0x23`), user `SS` (`0x1B`), user `RSP`, and user `RIP`, enabling maskable interrupts (`IF=1`).

#### 2. User-to-Kernel Fast System Call via `syscall` / `sysretq`
Hardware fast system calls are enabled and configured on the BSP during kernel initialization via MSRs:
1. **`IA32_EFER.SCE` (System Call Enable, Bit 0 of MSR `0xC000_0080`)**:
   - Set to `1` using `read_msr` and `write_msr`.
2. **`IA32_STAR` (MSR `0xC000_0081`)**:
   - Bits [47:32] (Kernel CS/SS base): Set to `0x08`. On `syscall`, CPU loads `CS = 0x08` (Kernel Code) and `SS = 0x10` (`0x08 + 8`, Kernel Data).
   - Bits [63:48] (User CS/SS base): Set to `0x10`. On 64-bit `sysretq`, CPU loads `CS = 0x23` (`(0x10 + 16) | 3`, User Code) and `SS = 0x1B` (`(0x10 + 8) | 3`, User Data). This matches GDT entries `[3]` and `[4]` without reordering GDT descriptors.
3. **`IA32_LSTAR` (MSR `0xC000_0082`)**:
   - Set to the address of `syscall_entry` assembly stub.
4. **`IA32_FMASK` (MSR `0xC000_0084`)**:
   - Set to `0x0000_0200` (clears `RFLAGS.IF` atomically on entry, disabling interrupts during stack switch). Also masks `DF` (Direction Flag) and `TF` (Trap Flag).

#### 3. Syscall Entry / Exit Assembly Protocol (`syscall_entry`)
```nasm
global syscall_entry
extern syscall_dispatch_rust

syscall_entry:
    ; Hardware has atomically:
    ; RCX <- User RIP
    ; R11 <- User RFLAGS
    ; RFLAGS <- RFLAGS & ~FMASK (IF=0)
    ; CS <- 0x08, SS <- 0x10
    ; RSP remains USER RSP!

    ; 1. Swap user GS base with kernel PerCpu GS base
    swapgs

    ; 2. Save user RSP to scratch space in PerCpu and load kernel stack pointer
    mov qword [gs:40], rsp              ; PerCpu.scratch_user_rsp (offset 40)
    mov rsp, qword [gs:16]              ; Load current_thread pointer from gs:[16]
    mov rsp, qword [rsp + 24]           ; Load KernelThread.stack_top from offset 24

    ; 3. Push complete user context frame (SyscallFrame: 160 bytes, 16-B aligned)
    push qword 0x1B                     ; User SS
    push qword [gs:40]                  ; User RSP
    push r11                            ; User RFLAGS
    push qword 0x23                     ; User CS
    push rcx                            ; User RIP
    push rax                            ; Syscall number (RAX)
    push rdi                            ; arg0
    push rsi                            ; arg1
    push rdx                            ; arg2
    push r10                            ; arg3 (user passed in R10)
    push r8                             ; arg4
    push r9                             ; arg5
    push rbp                            ; Callee-saved RBP
    push rbx                            ; Callee-saved RBX
    push r12                            ; Callee-saved R12
    push r13                            ; Callee-saved R13
    push r14                            ; Callee-saved R14
    push r15                            ; Callee-saved R15

    ; 4. Call Rust dispatcher
    mov rdi, rsp                        ; Arg0 = *mut SyscallFrame
    call syscall_dispatch_rust

    ; Return value from syscall_dispatch_rust is in RAX
    ; Store return value into saved RAX slot (+88 from RSP)
    mov qword [rsp + 88], rax

    ; 5. Restore user registers
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
    pop rax                             ; Restores returned value into RAX

    pop rcx                             ; Restore User RIP for sysretq
    add rsp, 8                          ; Skip User CS
    pop r11                             ; Restore User RFLAGS for sysretq
    pop rsp                             ; Restore User RSP
    ; Skip User SS (RSP is now user stack)

    ; 6. Restore user GS base
    swapgs

    ; 7. Return to Ring 3
    o64 sysret
```

---

## 4. Syscall ABI & Calling Convention

### 4.1 Register Layout
The register calling convention is frozen as follows:
- **`RAX`**: Syscall Number on entry; Return Value on exit.
- **`RDI`**: Argument 0 (e.g. `Handle` or primary pointer).
- **`RSI`**: Argument 1 (e.g. second parameter or length).
- **`RDX`**: Argument 2 (e.g. flags or secondary length).
- **`R10`**: Argument 3 (replaces `RCX`, which is clobbered by hardware `syscall`).
- **`R8`**: Argument 4.
- **`R9`**: Argument 5.
- **`RCX`**: Hardware clobbered (captures user return RIP).
- **`R11`**: Hardware clobbered (captures user return RFLAGS).
- **Callee-saved Registers (`RBX`, `RBP`, `R12`–`R15`)**: Preserved across syscall boundaries.

### 4.2 Syscall Return Conventions
- Exactly one 64-bit integer returned in `RAX`:
  - **Success**: $\ge 0$ (e.g. `0` for OK, positive integer for count or byte length).
  - **Error**: Negative integer (`-1` through `-4095`) representing `-SyscallError`.

```rust
#[repr(i64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallError {
    Success             = 0,
    InvalidSyscall      = -1,   // -ENOSYS: Syscall number unrecognized
    InvalidArgument     = -2,   // -EINVAL: Malformed parameters or flags
    BadHandle           = -3,   // -EBADF: Handle not occupied or stale generation
    PermissionDenied    = -4,   // -EACCES: Lacking required capability right
    BadAddress          = -5,   // -EFAULT: Non-user, non-canonical, or unmapped pointer
    OutOfMemory         = -6,   // -ENOMEM: Kernel table or PMM frames exhausted
    ResourceBusy        = -7,   // -EBUSY: Channel ring full or resource pinned
    NotFound            = -8,   // -ENOENT: Process/thread/object not found
    WouldBlock          = -9,   // -EAGAIN: Non-blocking call would block
    PeerClosed          = -10,  // -EPIPE: Channel peer endpoint closed
}
```

---

## 5. Frozen Syscall Namespace (Stage 3I)

Stage 3I freezes exactly **10 minimal system calls** necessary to execute and verify user workloads:

| Number | Name | Arguments | Required Capability Right | Description |
| :---: | :--- | :--- | :--- | :--- |
| `1` | `sys_exit` | `arg0: exit_code (i32)` | None (Unconditional self-exit) | Transitions calling process to Zombie state |
| `2` | `sys_yield` | None | None | Voluntarily yields CPU quantum to scheduler |
| `3` | `sys_channel_create` | `arg0: out_handles (*mut [Handle; 2])` | None (Creates root channel pair) | Allocates channel object & installs 2 handles |
| `4` | `sys_channel_send` | `arg0: handle (Handle), arg1: msg_ptr (*const IpcMessage), arg2: flags (u32)` | `cap_rights::CHANNEL_SEND` | Enqueues 80-byte message into channel ring |
| `5` | `sys_channel_receive` | `arg0: handle (Handle), arg1: msg_ptr (*mut IpcMessage), arg2: flags (u32)` | `cap_rights::CHANNEL_RECEIVE` | Dequeues 80-byte message from channel ring |
| `6` | `sys_channel_close` | `arg0: handle (Handle)` | `cap_rights::CLOSE` | Closes channel handle, Option B reparenting |
| `7` | `sys_shm_create` | `arg0: pages (usize), arg1: out_handle (*mut Handle)` | None (Creates root SHM object) | Allocates PMM frames & root SHM capability |
| `8` | `sys_shm_map` | `arg0: handle (Handle), arg1: vaddr (u64), arg2: writable (bool)` | `cap_rights::SHM_MAP_READ` / `WRITE` | Maps SHM frames into user aperture (`RW+NX` or `RO+NX`) |
| `9` | `sys_shm_unmap` | `arg0: handle (Handle), arg1: vaddr (u64)` | `cap_rights::SHM_UNMAP` | Unmaps SHM frames from user aperture |
| `10` | `sys_cap_derive` | `arg0: parent (Handle), arg1: rights (u32), arg2: out_handle (*mut Handle)` | `cap_rights::DUPLICATE` | Derives child capability with attenuated rights |

---

## 6. Authoritative User Pointer Validation (`pointer.rs`)

All user pointers must be defensively validated prior to kernel memory access. Ad-hoc pointer checks scattered across syscall implementations are strictly forbidden.

### 6.1 Validation Primitive Contract
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAccess {
    Read,
    Write,
    Execute,
}

pub fn validate_user_range(
    ptr: u64,
    len: usize,
    access: MemoryAccess,
    vmm: &mut ActivePageTable,
) -> Result<(), SyscallError> {
    if len == 0 {
        return Ok(());
    }

    // 1. Integer overflow check (ptr + len must not overflow u64)
    let end = ptr.checked_add(len as u64).ok_or(SyscallError::BadAddress)?;

    // 2. Canonical user address boundary check
    if ptr < USER_VA_MIN || end > USER_VA_MAX {
        return Err(SyscallError::BadAddress);
    }

    // 3. Multi-page traversal & permission verification
    let geometry = crate::mm::vmm::get_active_geometry();
    let start_page = ptr & !0xFFF;
    let end_page = (end - 1) & !0xFFF;

    let mut curr_page = start_page;
    loop {
        let vaddr = crate::mm::vmm::VirtualAddress::new(curr_page);
        if !vmm.is_mapped(vaddr) {
            return Err(SyscallError::BadAddress);
        }
        
        let flags = vmm.get_page_flags(vaddr).ok_or(SyscallError::BadAddress)?;
        if !flags.contains(PageTableFlags::USER_ACCESSIBLE) {
            return Err(SyscallError::BadAddress);
        }

        match access {
            MemoryAccess::Read => {
                // Present and User-accessible is sufficient for Read
            },
            MemoryAccess::Write => {
                if !flags.contains(PageTableFlags::WRITABLE) {
                    return Err(SyscallError::BadAddress);
                }
            },
            MemoryAccess::Execute => {
                if flags.contains(PageTableFlags::NO_EXECUTE) {
                    return Err(SyscallError::BadAddress);
                }
            }
        }

        if curr_page == end_page {
            break;
        }
        curr_page += 4096;
    }

    Ok(())
}
```

---

## 7. Initial User-Space Bootstrap & Freestanding Program

Stage 3I avoids complex ELF binary parsing and disk storage by providing an embedded, freestanding user binary payload (`user/init/main.rs`):

```rust
// user/init/main.rs - Freestanding ZeroOS User Entry Payload
#![no_std]
#![no_main]

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // 1. First cooperative yield
    sys_yield();

    // 2. Second cooperative yield
    sys_yield();

    // 3. Clean exit with status 0
    sys_exit(0);
}

#[inline(always)]
unsafe fn sys_yield() -> i64 {
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
unsafe fn sys_exit(code: i32) -> ! {
    core::arch::asm!(
        "syscall",
        in("rax") 1, // SYS_EXIT
        in("rdi") code as u64,
        options(noreturn)
    );
}
```

### 7.1 Bootstrap Sequence
```text
Kernel Boot Sequence (main.rs)
  ↓
Stage 3H verification passes (181 in-kernel tests)
  ↓
Run Stage 3I Verification:
  1. Initialize MSRs (IA32_EFER.SCE, STAR, LSTAR, FMASK).
  2. Create Process via create_process(0, pmm).
  3. AddressSpace allocates PML4, clones kernel aperture (PML4[256..512], USER=0).
  4. PMM allocates 1 frame for user code; copy embedded _start payload.
  5. Map user code at USER_CODE_BASE (0x0020_0000) as RX (USER=1, WRITABLE=0, NX=0).
  6. PMM allocates 4 frames for user stack; map at USER_STACK_TOP as RW+NX (USER=1, WRITABLE=1, NX=1).
  7. Create user thread via create_user_thread(process, USER_CODE_BASE, USER_STACK_TOP).
  8. Enqueue user thread into scheduler RunQueue.
  9. Yield execution; scheduler switches CR3 and dispatches user thread via iretq.
  10. User code executes in Ring 3, issues sys_yield(), sys_yield(), sys_exit(0).
  11. Kernel reaps user process, verifying 100% PMM frame neutrality.
```

---

## 8. Preserved Invariants & Boundaries

1. **Process ABI Invariance**: `Process` remains exactly **128 bytes** (8-byte aligned) (`I-PROC-1`).
2. **HandleTable ABI Invariance**: `HandleTable` remains exactly **520 bytes** (`I-PROC-2`).
3. **Zero Kernel Dynamic Allocation**: Zero heap usage. All tables (`PROCESS_HANDLE_TABLES`, `CAPABILITY_NODE_TABLE`, `CHANNEL_TABLE`, `SHM_MAPPING_TABLE`) reside in `.bss`.
4. **PMM Frame Neutrality**: Physical memory allocated for user code, data, stack, and page tables is tracked and completely reclaimed upon process termination.
5. **Stages 3A–3H Untouched**: All 181 prior bare-metal tests remain active and unmodified.

---

## 9. Stage 3I Bare-Metal Machine Verification Suite

The verification suite in `kernel/src/syscall/tests.rs` will execute in bare-metal QEMU, validating 18 sequential machine tests:

- **3I-A**: Fast syscall MSR initialization (`EFER.SCE`, `STAR`, `LSTAR`, `FMASK`).
- **3I-B**: User process creation and isolated address space aperture (`USER=1` lower half, `USER=0` upper half).
- **3I-C**: Strict W^X memory permissions on user code (`RX`) and user stack (`RW+NX`).
- **3I-D**: Initial kernel-to-user privilege transition via `iretq` (CS=`0x23`, SS=`0x1B`, Ring 3).
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

---

## 10. Deliverables Outline

1. `docs/design/STAGE3I-ARCHITECTURE-REV1.md`: This authoritative architecture specification.
2. `docs/decisions/ADR-0018-user-space-and-syscalls.md`: Architectural Decision Record.
3. `kernel/src/syscall/mod.rs`: Module exports and initialization.
4. `kernel/src/syscall/numbers.rs`: Frozen syscall numbers and error codes.
5. `kernel/src/syscall/abi.rs`: Syscall frame structure and register mappings.
6. `kernel/src/syscall/entry.rs`: Low-level assembly entry/exit stubs (`syscall_entry`).
7. `kernel/src/syscall/dispatch.rs`: Syscall router and capability-authorizing handlers.
8. `kernel/src/syscall/pointer.rs`: Unified user pointer range validator.
9. `kernel/src/syscall/tests.rs`: Bare-metal test runner (Tests `3I-A` through `3I-R`).
10. `user/init/main.rs`: Freestanding user bootstrap payload.
11. `tests/test_stage3i.py`: Host pytest validation suite (symbols, ELF layout, QEMU telemetry).
