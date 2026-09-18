# ADR-0019: Native ELF Program Loading & Execution (Stage 3J)

## Status
Accepted / Frozen Candidate (Rev2)

## Context
Stage 3I introduced fast system-call infrastructure (`syscall`/`sysretq`) and freestanding Ring-3 execution using statically embedded user opcodes.
To establish real program execution in ZeroOS, the kernel requires a native executable program-loading mechanism.
The mechanism must load valid 64-bit ELF executables, map their code and data into isolated per-process address spaces, construct a guarded user stack, enforce strict W^X security, and initialize user thread state, while strictly preserving frozen Stage 3A–3I contracts (Process ABI, KernelThread ABI, Cooperative/Preemptive frames, Capability/Handle system, and PMM frame neutrality).

## Decision
1. **ELF Format**: Strictly support static `ELF64` little-endian `x86-64` executables (`ET_EXEC`, `EM_X86_64`). Reject relocatable, dynamic, or foreign architecture binaries.
2. **Program Header Support**: Recognize only `PT_LOAD`, `PT_NOTE`, `PT_PHDR`, and non-executable `PT_GNU_STACK`. If `PT_GNU_STACK` requests executable stack (`PF_X`), reject with `ExecutableStackRejected`. Reject any dynamic linking, interpreter, or TLS segments.
3. **PT_LOAD Validation & User VA Bounds**: Enforce `USER_VA_MIN <= p_vaddr` and checked `p_vaddr + p_memsz <= USER_VA_MAX_EXCLUSIVE`. Require `p_align` to be 0, 1, or power-of-two >= 4096 with congruence. Enforce non-overlapping segments and page-level permission conflict prevention.
4. **W^X Policy**: Strictly prohibit `PF_W | PF_X`. Map code as RX and data/BSS as RW+NX.
5. **Exact User Stack Contract**: Fixed 16 KiB (4 pages) stack at `[USER_STACK_BASE, USER_STACK_TOP)` (`0x0000_7F7F_FFFC_0000 .. 0x0000_7F7F_FFFF_0000`) with unmapped lower guard page at `USER_STACK_GUARD` (`0x0000_7F7F_FFFB_F000`). Initial `RSP = USER_STACK_TOP` (16-byte aligned).
6. **Leaf Memory Ownership & No-Double-Free**: Track all allocated leaf physical frames in companion static table `PROCESS_MEMORY_MAPS: [ProcessMemoryMap; MAX_PROCESSES]` (`MAX_PROCESS_MAPPED_PAGES = 64`) with formal lifecycle states (`Free -> Allocating -> Active -> Reclaiming -> Free`). `AddressSpace` manages page tables; `PROCESS_MEMORY_MAPS` manages leaf frames. Teardown unmaps leaf pages, frees leaf frames to PMM, and calls `AddressSpace::destroy()`.
7. **Complete Transactional Rollback**: Any load failure unmaps all mapped leaf pages, frees leaf frames to PMM, destroys `AddressSpace`, and reclaims process slot. PMM neutrality is preserved on both failure and success.
8. **Thread Creation**: Use `create_user_thread()` with cooperative bootstrap trampoline (`user_thread_bootstrap_trampoline`) entering Ring 3 via `iretq`.

## Consequences
- **Positive**: Native ELF binaries can execute in ZeroOS user space with memory isolation, hardware privilege boundaries, and zero leaks.
- **Positive**: 100% fail-closed validation guarantees malformed or malicious ELFs cannot corrupt kernel state or leak physical frames.
- **Positive**: Zero modifications to frozen Stage 3A–3I ABIs.
- **Positive**: Exact memory ownership eliminates double-free hazards between VMM and process teardown.
- **Trade-off**: Dynamic linking, PIE, relocations, and demand paging are deferred to later stages.
