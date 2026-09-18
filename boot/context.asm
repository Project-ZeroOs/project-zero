; Project Zero - Thread Cooperative Context Switching & Bootstrap Trampoline
; Architecture: x86-64
; Assembler: NASM (elf64)

bits 64

global switch_context:function
global thread_bootstrap_entry:function
extern exit_current_thread

section .text

; switch_context(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64)
; System V AMD64 ABI:
;   rdi = prev_rsp (*mut u64) -> Pointer to current thread's RSP storage
;   rsi = next_rsp (u64)       -> Next thread's saved RSP value
;   rdx = orig_rflags (u64)    -> Outgoing thread's original RFLAGS (IF bit source)
switch_context:
    ; 1. Save callee-preserved registers onto current thread stack
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    pushfq

    ; 2. Merge original IF bit from rdx (bit 9 = 0x200) into saved RFLAGS on stack.
    ; This only affects the outgoing thread's saved frame.
    test edx, 0x200
    jz .clear_if
    or qword [rsp], 0x200
    jmp .if_done
.clear_if:
    and qword [rsp], ~0x200
.if_done:

    ; 3. Store current thread's updated stack pointer into *prev_rsp
    mov [rdi], rsp

    ; 4. Switch CPU stack pointer to next thread's stack
    mov rsp, rsi

    ; 5. Restore next thread's callee-preserved registers and its own saved RFLAGS
    popfq
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    ; 6. Return into next thread's execution flow
    ret

; Bootstrap trampoline executed upon first cooperative activation.
; System V AMD64 ABI:
;   - At entry via 'ret' from switch_context: RSP = stack_top (16-byte aligned: RSP % 16 == 0)
;   - Callee registers restored from forged frame:
;       r12 = thread argument
;       rbx = entry function pointer
thread_bootstrap_entry:
    mov rdi, r12        ; Pass initial argument in RDI (AMD64 1st param)
    call rbx            ; Invoke entry_fn(arg) -> pushes 8-byte RIP -> callee sees (RSP + 8) % 16 == 0

    ; When entry_fn returns, execute non-returning thread termination
    call exit_current_thread

    ; Unreachable fallback loop
.halt:
    hlt
    jmp .halt

; ====================================================================
; Stage 3C Increment 4: Preemptive Context Assembly Primitives
; ====================================================================

global switch_context_coop_to_preempt:function
global restore_context_cooperative:function
global restore_context_preemptive:function

; switch_context_coop_to_preempt(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64) -> !
; Precondition: Caller enters with IF=0.
; System V AMD64 ABI:
;   rdi = prev_rsp (*mut u64) -> Pointer to current thread's RSP storage
;   rsi = next_rsp (u64)       -> Next thread's saved RSP value (160-byte frame)
;   rdx = orig_rflags (u64)    -> Outgoing thread's original RFLAGS (IF bit source)
switch_context_coop_to_preempt:
    ; 1. Save 64-byte cooperative frame on outgoing thread stack
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    pushfq

    ; 2. Merge original IF bit from rdx (bit 9 = 0x200) into saved RFLAGS on stack.
    test edx, 0x200
    jz .c2p_clear_if
    or qword [rsp], 0x200
    jmp .c2p_if_done
.c2p_clear_if:
    and qword [rsp], ~0x200
.c2p_if_done:

    ; 3. Store current thread's updated stack pointer into *prev_rsp
    mov [rdi], rsp

    ; 4. Switch CPU stack pointer to incoming preemptive thread's stack
    mov rsp, rsi

    ; 5. Restore 15 general-purpose registers from 160-byte frame
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

    ; 6. Return into interrupted context via iretq (non-returning)
    iretq

; restore_context_cooperative(next_rsp: u64) -> !
; Precondition: Caller enters with IF=0.
; System V AMD64 ABI:
;   rdi = next_rsp (u64) -> Incoming cooperative thread's saved RSP (64-byte frame)
restore_context_cooperative:
    ; 1. Switch CPU stack pointer to incoming cooperative thread's stack
    mov rsp, rdi

    ; 2. Restore 64-byte cooperative frame
    popfq
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    ; 3. Resume cooperative thread execution flow via ret (non-returning to caller)
    ret

; restore_context_preemptive(next_rsp: u64) -> !
; Precondition: Caller enters with IF=0.
; System V AMD64 ABI:
;   rdi = next_rsp (u64) -> Incoming preemptive thread's saved RSP (160-byte frame)
restore_context_preemptive:
    ; 1. Switch CPU stack pointer to incoming preemptive thread's stack
    mov rsp, rdi

    ; 2. Restore 15 general-purpose registers from 160-byte frame
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

    ; 3. Return into interrupted context via iretq (non-returning)
    iretq

; ====================================================================
; Stage 3C Increment 4 Verification Continuation Chain Trampolines
; ====================================================================

global run_inc4_coop_start:function
global inc4_coop_continuation:function
global stage3c_inc4_target1_entry:function
global stage3c_inc4_target2_entry:function

extern stage3c_inc4_verify_target1
extern stage3c_inc4_verify_target2
extern stage3c_inc4_complete_verification

; run_inc4_coop_start(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64)
; Invoked by Rust verification harness with IF=0.
run_inc4_coop_start:
    ; 1. Preserve caller's callee-saved registers
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    ; 2. Pre-load callee-preserved registers with known test sentinels
    mov rbx, 0x4444444444444444
    mov rbp, 0x5555555555555555
    mov r12, 0xCCCCCCCCCCCCCCCC
    mov r13, 0xDDDDDDDDDDDDDDDD
    mov r14, 0xEEEEEEEEEEEEEEEE
    mov r15, 0xFFFFFFFFFFFFFFFF

    ; 3. Call switch_context_coop_to_preempt
    ; Pushes the address of inc4_coop_continuation as return RIP onto stack!
    call switch_context_coop_to_preempt

; inc4_coop_continuation:
; Resumed via ret from restore_context_cooperative!
inc4_coop_continuation:
    ; 1. Capture restored registers directly into memory before modifying anything
    mov [rel inc4_resumed_rbx], rbx
    mov [rel inc4_resumed_rbp], rbp
    mov [rel inc4_resumed_r12], r12
    mov [rel inc4_resumed_r13], r13
    mov [rel inc4_resumed_r14], r14
    mov [rel inc4_resumed_r15], r15
    mov [rel inc4_resumed_rsp], rsp
    pushfq
    pop rax
    mov [rel inc4_resumed_rflags], rax

    ; 2. Invoke Rust verification completion
    call stage3c_inc4_complete_verification

    ; 3. Restore caller's original callee-saved registers
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp

    ; 4. Return to caller of run_inc4_coop_start
    ret

; stage3c_inc4_target1_entry:
; Resumed via iretq from switch_context_coop_to_preempt!
; Entered with RSP = target1_stack_top (RSP % 16 == 0)
stage3c_inc4_target1_entry:
    ; Immediately capture all 15 GPRs, RSP, and RFLAGS
    mov [rel inc4_target1_rsp], rsp
    mov [rel inc4_target1_rax], rax
    mov [rel inc4_target1_rcx], rcx
    mov [rel inc4_target1_rdx], rdx
    mov [rel inc4_target1_rbx], rbx
    mov [rel inc4_target1_rbp], rbp
    mov [rel inc4_target1_rsi], rsi
    mov [rel inc4_target1_rdi], rdi
    mov [rel inc4_target1_r8],  r8
    mov [rel inc4_target1_r9],  r9
    mov [rel inc4_target1_r10], r10
    mov [rel inc4_target1_r11], r11
    mov [rel inc4_target1_r12], r12
    mov [rel inc4_target1_r13], r13
    mov [rel inc4_target1_r14], r14
    mov [rel inc4_target1_r15], r15
    pushfq
    pop rax
    mov [rel inc4_target1_rflags], rax

    ; Call Rust to verify target 1 state
    call stage3c_inc4_verify_target1

    ; Enforce IF=0 precondition before calling next primitive
    cli

    ; Load Target 2 frame RSP and invoke restore_context_preemptive
    mov rdi, [rel inc4_target2_frame_rsp]
    call restore_context_preemptive

.t1_halt:
    hlt
    jmp .t1_halt

; stage3c_inc4_target2_entry:
; Resumed via iretq from restore_context_preemptive!
; Entered with RSP = target2_stack_top (RSP % 16 == 0)
stage3c_inc4_target2_entry:
    ; Immediately capture all 15 GPRs, RSP, and RFLAGS
    mov [rel inc4_target2_rsp], rsp
    mov [rel inc4_target2_rax], rax
    mov [rel inc4_target2_rcx], rcx
    mov [rel inc4_target2_rdx], rdx
    mov [rel inc4_target2_rbx], rbx
    mov [rel inc4_target2_rbp], rbp
    mov [rel inc4_target2_rsi], rsi
    mov [rel inc4_target2_rdi], rdi
    mov [rel inc4_target2_r8],  r8
    mov [rel inc4_target2_r9],  r9
    mov [rel inc4_target2_r10], r10
    mov [rel inc4_target2_r11], r11
    mov [rel inc4_target2_r12], r12
    mov [rel inc4_target2_r13], r13
    mov [rel inc4_target2_r14], r14
    mov [rel inc4_target2_r15], r15
    pushfq
    pop rax
    mov [rel inc4_target2_rflags], rax

    ; Call Rust to verify target 2 state
    call stage3c_inc4_verify_target2

    ; Enforce IF=0 precondition before calling next primitive
    cli

    ; Load saved cooperative RSP and invoke restore_context_cooperative
    mov rdi, [rel inc4_saved_coop_rsp]
    call restore_context_cooperative

.t2_halt:
    hlt
    jmp .t2_halt

section .bss
align 8
global inc4_saved_coop_rsp
global inc4_target2_frame_rsp

inc4_saved_coop_rsp:        resq 1
inc4_target2_frame_rsp:     resq 1

global inc4_target1_rax
global inc4_target1_rcx
global inc4_target1_rdx
global inc4_target1_rbx
global inc4_target1_rbp
global inc4_target1_rsi
global inc4_target1_rdi
global inc4_target1_r8
global inc4_target1_r9
global inc4_target1_r10
global inc4_target1_r11
global inc4_target1_r12
global inc4_target1_r13
global inc4_target1_r14
global inc4_target1_r15
global inc4_target1_rsp
global inc4_target1_rflags

inc4_target1_rax:    resq 1
inc4_target1_rcx:    resq 1
inc4_target1_rdx:    resq 1
inc4_target1_rbx:    resq 1
inc4_target1_rbp:    resq 1
inc4_target1_rsi:    resq 1
inc4_target1_rdi:    resq 1
inc4_target1_r8:     resq 1
inc4_target1_r9:     resq 1
inc4_target1_r10:    resq 1
inc4_target1_r11:    resq 1
inc4_target1_r12:    resq 1
inc4_target1_r13:    resq 1
inc4_target1_r14:    resq 1
inc4_target1_r15:    resq 1
inc4_target1_rsp:    resq 1
inc4_target1_rflags: resq 1

global inc4_target2_rax
global inc4_target2_rcx
global inc4_target2_rdx
global inc4_target2_rbx
global inc4_target2_rbp
global inc4_target2_rsi
global inc4_target2_rdi
global inc4_target2_r8
global inc4_target2_r9
global inc4_target2_r10
global inc4_target2_r11
global inc4_target2_r12
global inc4_target2_r13
global inc4_target2_r14
global inc4_target2_r15
global inc4_target2_rsp
global inc4_target2_rflags

inc4_target2_rax:    resq 1
inc4_target2_rcx:    resq 1
inc4_target2_rdx:    resq 1
inc4_target2_rbx:    resq 1
inc4_target2_rbp:    resq 1
inc4_target2_rsi:    resq 1
inc4_target2_rdi:    resq 1
inc4_target2_r8:     resq 1
inc4_target2_r9:     resq 1
inc4_target2_r10:    resq 1
inc4_target2_r11:    resq 1
inc4_target2_r12:    resq 1
inc4_target2_r13:    resq 1
inc4_target2_r14:    resq 1
inc4_target2_r15:    resq 1
inc4_target2_rsp:    resq 1
inc4_target2_rflags: resq 1

global inc4_resumed_rbx
global inc4_resumed_rbp
global inc4_resumed_r12
global inc4_resumed_r13
global inc4_resumed_r14
global inc4_resumed_r15
global inc4_resumed_rsp
global inc4_resumed_rflags

inc4_resumed_rbx:    resq 1
inc4_resumed_rbp:    resq 1
inc4_resumed_r12:    resq 1
inc4_resumed_r13:    resq 1
inc4_resumed_r14:    resq 1
inc4_resumed_r15:    resq 1
inc4_resumed_rsp:    resq 1
inc4_resumed_rflags: resq 1

; ====================================================================
; Stage 3I: Fast Syscall Hardware Entry & Exit (I-SYSCALL-1..11)
; ====================================================================

section .bss
align 16
global SYSCALL_SCRATCH_USER_RSP:data 128
SYSCALL_SCRATCH_USER_RSP:
    resq 16                             ; MAX_CPUS = 16 (128 bytes total)

section .text

global syscall_entry:function
global user_thread_bootstrap_trampoline:function
extern syscall_dispatch_rust
extern syscall_validate_return_rust
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

    ; 2. SMP-safe scratch save: inspect cpu_id from gs:[8] without register clobber (I-SYSCALL-STACK-1)
    cmp dword [gs:8], 0
    je .save_cpu0
    cmp dword [gs:8], 1
    je .save_cpu1
    cmp dword [gs:8], 2
    je .save_cpu2
    cmp dword [gs:8], 3
    je .save_cpu3
    ud2

.save_cpu0:
    mov [rel SYSCALL_SCRATCH_USER_RSP + 0*8], rsp
    jmp .switch_stack
.save_cpu1:
    mov [rel SYSCALL_SCRATCH_USER_RSP + 1*8], rsp
    jmp .switch_stack
.save_cpu2:
    mov [rel SYSCALL_SCRATCH_USER_RSP + 2*8], rsp
    jmp .switch_stack
.save_cpu3:
    mov [rel SYSCALL_SCRATCH_USER_RSP + 3*8], rsp
    jmp .switch_stack

.switch_stack:
    mov rsp, [gs:16]                    ; PerCpu.current_thread (offset 16, frozen 48-B PerCpu)
    test rsp, rsp
    jz .fatal_null_thread               ; Catastrophic if null
    mov rsp, [rsp + 24]                 ; KernelThread.stack_top (offset 24, frozen 176-B KernelThread)

    ; 3. Push complete SyscallFrame (144 bytes = 18 pushes * 8 bytes, 16-B aligned)
    push qword 0x1B                     ; user_ss (+136)
    ; Retrieve saved user RSP from per-CPU slot
    cmp dword [gs:8], 0
    je .push_rsp_cpu0
    cmp dword [gs:8], 1
    je .push_rsp_cpu1
    cmp dword [gs:8], 2
    je .push_rsp_cpu2
    push qword [rel SYSCALL_SCRATCH_USER_RSP + 3*8] ; user_rsp (+128)
    jmp .push_rest
.push_rsp_cpu0:
    push qword [rel SYSCALL_SCRATCH_USER_RSP + 0*8] ; user_rsp (+128)
    jmp .push_rest
.push_rsp_cpu1:
    push qword [rel SYSCALL_SCRATCH_USER_RSP + 1*8] ; user_rsp (+128)
    jmp .push_rest
.push_rsp_cpu2:
    push qword [rel SYSCALL_SCRATCH_USER_RSP + 2*8] ; user_rsp (+128)
    jmp .push_rest

.push_rest:
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

    ; 5. Dispatch via Rust router (RSP is 16-byte aligned, satisfying I-SYSCALL-ABI-STACK-1)
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

    ; 10. Fast return to Ring 3 (I-SYSCALL-RETURN-3: RCX=user_rip, R11=user_rflags, RSP=user_rsp, GS=user_GS)
    o64 sysret

.fail_closed_exit:
    ; Security violation detected during return validation (I-SYSCALL-RETURN-2)
    mov rdi, rsp
    call syscall_fail_closed_terminate
    ; Never returns

.fatal_null_thread:
    ; Unrecoverable kernel panic
    ud2

; Initial trampoline for dropping from cooperative scheduler into Ring 3 (I-SYSCALL-11)
; Input via forged cooperative registers:
;   r12 = Initial user RIP (USER_CODE_BASE)
;   r13 = Initial user RSP (USER_STACK_TOP)
user_thread_bootstrap_trampoline:
    push qword 0x1B                     ; SS = USER_DATA_SELECTOR (0x1B)
    push r13                            ; RSP = user_rsp
    push qword 0x0202                   ; RFLAGS = IF=1, reserved bit 1=1
    push qword 0x23                     ; CS = USER_CODE_SELECTOR (0x23)
    push r12                            ; RIP = user_rip
    iretq

