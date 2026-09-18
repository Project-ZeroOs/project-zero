# ADR-0018: Ring 3 User Space and System Call Interface Architecture

## Context
Project Zero has completed Stages 1 through 3H, establishing verified physical/virtual memory management (PMM/VMM), preemptive context switching, priority round-robin scheduling, inter-process communication (IPC) channels, shared memory, and capability derivation/revocation with process-isolated handle tables.

Until Stage 3I, all execution occurred exclusively in Ring 0 (kernel mode). Stage 3I introduces the first real Ring 3 User-Space execution environment and a hardware fast system call boundary (`syscall` / `sysretq`) mediated strictly by capability handles with zero ambient authority.

## Decision
1. **Privilege Boundary & Hardware Transition**:
   - Initial entry from kernel to user mode is executed via a cooperative trampoline executing `iretq` with Ring 3 segment selectors (`CS = 0x23`, `SS = 0x1B`).
   - Fast syscall transitions from Ring 3 to Ring 0 are executed via hardware `syscall` / `sysretq` instructions configured through MSRs:
     - `IA32_EFER.SCE` = 1 (System Call Enable)
     - `IA32_STAR` = `(0x0010 << 48) | (0x0008 << 32)`
     - `IA32_LSTAR` = Address of `syscall_entry` assembly stub
     - `IA32_FMASK` = `0x0000_0200` (`RFLAGS.IF` cleared atomically on entry)
     - `IA32_KERNEL_GS_BASE` = `&BSP_PERCPU`
2. **Exact `SyscallFrame` Layout (144 bytes = 18 × 8 bytes)**:
   - `SyscallFrame` is a standalone ABI boundary struct distinct from the frozen Stage 3C `PreemptiveFrame` (160 B) and `CooperativeFrame` (64 B).
   - Contains 6 kernel-saved registers (`r15..rbp`), 6 arguments (`r9..rdi`), 1 syscall/return register (`rax`), and 5 hardware-captured user state fields (`user_rip`, `user_cs`, `user_rflags`, `user_rsp`, `user_ss`).
   - Preserves 16-byte stack alignment at all `call` sites (`I-SYSCALL-ABI-STACK-1`).
3. **SMP-Safe Stack Transition**:
   - `PerCpu` remains frozen at **48 bytes**; `KernelThread` remains frozen at **176 bytes**.
   - Temporary user RSP scratch storage is allocated in `.bss` as `SYSCALL_SCRATCH_USER_RSP[MAX_CPUS]` (16 CPUs).
   - CPU ID is inspected directly from `gs:[8]` without clobbering general-purpose registers.
4. **Authoritative GS & Privilege State Machine (`I-SYSCALL-GS-1..5`)**:
   - `GS_BASE` holds `&BSP_PERCPU` at all times during kernel execution.
   - `swapgs` is executed on syscall entry and exit.
   - Hardware interrupts inspect saved `CS` RPL: if RPL=3, execute `swapgs`; if RPL=0, zero `swapgs` operations.
5. **Strict Return Validation & Fail-Closed Termination (`I-SYSCALL-RETURN-1..3`)**:
   - Return state (`user_rip`, `user_rsp`, `user_rflags`, `user_cs`, `user_ss`) is defensively validated before `sysretq`.
   - Invalid return state is treated as an unrecoverable kernel security fault, terminating the process immediately without executing `sysretq`.
6. **Zero Ambient Authority & 10-Syscall Namespace**:
   - Exactly 10 minimal syscalls: `sys_exit`, `sys_yield`, `sys_channel_create`, `sys_channel_send`, `sys_channel_receive`, `sys_channel_close`, `sys_shm_create`, `sys_shm_map`, `sys_shm_unmap`, `sys_cap_derive`.
   - All privileged operations require a valid `Handle` authorized against process handle and capability node tables.

## Status
Approved and Architecturally Frozen (Rev4).

## Consequences
- Clean separation between kernel supervisor and untrusted user code.
- Strict $W \oplus X$ memory protections on user mappings.
- Exact verification accounting: 181 baseline + 18 Stage 3I = 199 in-kernel machine tests; 25 baseline + 1 Stage 3I = 26 host test files; 92 baseline + 6 Stage 3I = 98 pytest items.
