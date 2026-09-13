; Project Zero - Thread Cooperative Context Switching
; Architecture: x86-64
; Assembler: NASM (elf64)

bits 64

global switch_context

section .text

; switch_context(prev_rsp: *mut *mut u8, next_rsp: *const u8)
; AMD64 ABI:
;   rdi = prev_rsp (*mut *mut u8) -> Pointer to current thread's RSP storage
;   rsi = next_rsp (*const u8)     -> Next thread's saved RSP value
switch_context:
    ; 1. Save callee-preserved registers onto current thread stack
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    pushfq

    ; 2. Store current thread's updated stack pointer into *prev_rsp
    mov [rdi], rsp

    ; 3. Switch CPU stack pointer to next thread's stack
    mov rsp, rsi

    ; 4. Restore next thread's callee-preserved registers
    popfq
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    ; 5. Return into next thread's execution flow
    ret
