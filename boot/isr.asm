; Project Zero - Low-Level Interrupt Service Routine (ISR) Stubs
; Architecture: x86-64
; Assembler: NASM (elf64)

bits 64

global load_idt
global load_gdt_and_tss
extern exception_dispatch
extern timer_interrupt_handler
extern ipi_stop_handler
extern ipi_reschedule_handler
extern ipi_tlb_handler

; Macro for exceptions that do NOT push an error code onto the stack
%macro ISR_NOERR 1
global isr_%1
isr_%1:
    push qword 0               ; Push dummy error code
    push qword %1              ; Push interrupt vector number
    jmp common_isr_stub
%endmacro

; Macro for exceptions that DO push an error code onto the stack
%macro ISR_ERR 1
global isr_%1
isr_%1:
    push qword %1              ; Push interrupt vector number
    jmp common_isr_stub
%endmacro

section .text

; CPU Standard Exceptions (0-31)
ISR_NOERR 0                    ; 0: #DE Divide Error
ISR_NOERR 1                    ; 1: #DB Debug Exception
ISR_NOERR 2                    ; 2: NMI Non-Maskable Interrupt
ISR_NOERR 3                    ; 3: #BP Breakpoint
ISR_NOERR 4                    ; 4: #OF Overflow
ISR_NOERR 5                    ; 5: #BR Bound Range Exceeded
ISR_NOERR 6                    ; 6: #UD Invalid Opcode
ISR_NOERR 7                    ; 7: #NM Device Not Available
ISR_ERR   8                    ; 8: #DF Double Fault
ISR_NOERR 9                    ; 9: Coprocessor Segment Overrun
ISR_ERR   10                   ; 10: #TS Invalid TSS
ISR_ERR   11                   ; 11: #NP Segment Not Present
ISR_ERR   12                   ; 12: #SS Stack Fault
ISR_ERR   13                   ; 13: #GP General Protection Fault
ISR_ERR   14                   ; 14: #PF Page Fault
ISR_NOERR 15                   ; 15: Reserved
ISR_NOERR 16                   ; 16: #MF x87 FPU Floating-Point Error
ISR_ERR   17                   ; 17: #AC Alignment Check
ISR_NOERR 18                   ; 18: #MC Machine Check
ISR_NOERR 19                   ; 19: #XM SIMD Floating-Point Exception
ISR_NOERR 20                   ; 20: #VE Virtualization Exception
ISR_ERR   21                   ; 21: #CP Control Protection Exception
ISR_NOERR 22                   ; 22: Reserved
ISR_NOERR 23                   ; 23: Reserved
ISR_NOERR 24                   ; 24: Reserved
ISR_NOERR 25                   ; 25: Reserved
ISR_NOERR 26                   ; 26: Reserved
ISR_NOERR 27                   ; 27: Reserved
ISR_NOERR 28                   ; 28: #HV Hypervisor Injection Exception
ISR_ERR   29                   ; 29: #VC VMM Communication Exception
ISR_ERR   30                   ; 30: #SX Security Exception
ISR_NOERR 31                   ; 31: Reserved

; Hardware Timer IRQ mapped to Vector 32 (Exact 160-byte Interrupt Frame)
global isr_32:function
isr_32:
    ; Hardware has pushed: SS, RSP, RFLAGS, CS, RIP (40 bytes)
    ; Save all 15 general-purpose registers (120 bytes)
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    ; System V AMD64 ABI: pass pointer to 160-byte InterruptFrame in RDI
    mov rdi, rsp

    ; Call timer interrupt handler in Rust
    call timer_interrupt_handler

    ; Restore all 15 general-purpose registers
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

    ; Return to interrupted context via iretq
    iretq

; Vector 251: IPI_VECTOR_STOP
global isr_251:function
isr_251:
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    call ipi_stop_handler
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

; Vector 252: IPI_VECTOR_RESCHEDULE
global isr_252:function
isr_252:
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    call ipi_reschedule_handler
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

; Vector 253: IPI_VECTOR_TLB_SHOOTDOWN
global isr_253:function
isr_253:
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    call ipi_tlb_handler
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

; Vector 255: LAPIC_SPURIOUS_VECTOR (No EOI required)
global isr_255:function
isr_255:
    iretq

; Common Exception Handler Stub
common_isr_stub:
    ; Save all 15 general-purpose registers
    push rax
    push rcx
    push rdx
    push rbx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    ; First argument (RDI in System V AMD64 ABI) = pointer to ExceptionFrame (current RSP)
    mov rdi, rsp

    ; Call Rust exception dispatcher
    call exception_dispatch

    ; Restore registers
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

    ; Drop error code and vector number
    add rsp, 16
    iretq

; lidt instruction wrapper
; rdi = pointer to IdtrDescriptor (limit: u16, base: u64)
load_idt:
    lidt [rdi]
    ret

; Load GDT, reload CS via far return, load data segments, and load TSS
; rdi = pointer to GdtDescriptor (limit: u16, base: u64)
; rsi = TSS selector (0x28)
load_gdt_and_tss:
    lgdt [rdi]

    ; Far return to reload CS with 0x08
    push qword 0x08
    lea rax, [rel .reload_cs]
    push rax
    retfq

.reload_cs:
    ; Reload data segments with 0x10
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Load Task State Segment (TSS)
    ltr si
    ret

; ====================================================================
; Controlled Exception Test Trampolines (Stage 2A Verification)
; ====================================================================

global test_trigger_ud
global test_resume_ud
test_trigger_ud:
    ud2
test_resume_ud:
    ret

global test_trigger_de
global test_resume_de
test_trigger_de:
    xor ecx, ecx
    mov eax, 42
    div ecx
test_resume_de:
    ret

global test_trigger_pf
global test_resume_pf
; rdi = target address to access (read)
test_trigger_pf:
    mov rax, [rdi]
test_resume_pf:
    ret

global test_trigger_pf_write
global test_resume_pf_write
; rdi = target address to write
test_trigger_pf_write:
    mov byte [rdi], 0xFF
test_resume_pf_write:
    ret

global test_trigger_pf_exec
global test_resume_pf_exec
; rdi = target address to execute
test_trigger_pf_exec:
    jmp rdi
test_resume_pf_exec:
    ret

; ====================================================================
; Stage 3C Increment 3: GPR Sentinel Preservation Test Harness
; ====================================================================

global test_gpr_sentinels_under_irq
global gpr_sentinel_flag

test_gpr_sentinels_under_irq:
    ; System V AMD64 ABI:
    ; RDI = pointer to GprSentinelResults output struct
    ; Preserve callee-saved registers of the caller
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    ; Save destination pointer to static variable
    mov [rel gpr_test_out_ptr], rdi

    ; Initialize test control flags
    mov qword [rel gpr_sentinel_flag], 0
    mov qword [rel gpr_test_loop_count], 0

    ; Record expected baseline RSP before loading sentinels
    mov [rel gpr_test_expected_rsp], rsp

    ; Pre-load all 15 GPRs with distinct 64-bit sentinels
    mov rax, 0x1111111111111111
    mov rcx, 0x2222222222222222
    mov rdx, 0x3333333333333333
    mov rbx, 0x4444444444444444
    mov rbp, 0x5555555555555555
    mov rsi, 0x6666666666666666
    mov rdi, 0x7777777777777777
    mov r8,  0x8888888888888888
    mov r9,  0x9999999999999999
    mov r10, 0xAAAAAAAAAAAAAAAA
    mov r11, 0xBBBBBBBBBBBBBBBB
    mov r12, 0xCCCCCCCCCCCCCCCC
    mov r13, 0xDDDDDDDDDDDDDDDD
    mov r14, 0xEEEEEEEEEEEEEEEE
    mov r15, 0xFFFFFFFFFFFFFFFF

    ; Enable maskable interrupts to allow LAPIC timer to interrupt this thread
    sti

    ; Assembly spin-loop: executes without clobbering ANY GPR
    ; inc [mem], cmp [mem], 0, and je only affect memory and RFLAGS
.gpr_spin_loop:
    pause
    inc qword [rel gpr_test_loop_count]
    cmp qword [rel gpr_sentinel_flag], 0
    je .gpr_spin_loop

.gpr_loop_done:
    ; Immediately disable interrupts upon notification of test completion
    cli

    ; Capture RSP and all 15 GPRs directly into memory BEFORE touching any register!
    mov [rel gpr_test_out_rsp], rsp
    mov [rel gpr_test_out_rax], rax
    mov [rel gpr_test_out_rcx], rcx
    mov [rel gpr_test_out_rdx], rdx
    mov [rel gpr_test_out_rbx], rbx
    mov [rel gpr_test_out_rbp], rbp
    mov [rel gpr_test_out_rsi], rsi
    mov [rel gpr_test_out_rdi], rdi
    mov [rel gpr_test_out_r8],  r8
    mov [rel gpr_test_out_r9],  r9
    mov [rel gpr_test_out_r10], r10
    mov [rel gpr_test_out_r11], r11
    mov [rel gpr_test_out_r12], r12
    mov [rel gpr_test_out_r13], r13
    mov [rel gpr_test_out_r14], r14
    mov [rel gpr_test_out_r15], r15

    ; Now that all 15 GPRs are captured, capture RFLAGS and resumed RIP
    pushfq
    pop rax
    mov [rel gpr_test_out_rflags], rax

    lea rax, [rel .gpr_loop_done]
    mov [rel gpr_test_out_rip], rax

    ; Check if RSP perfectly matched expected baseline
    mov rax, [rel gpr_test_out_rsp]
    cmp rax, [rel gpr_test_expected_rsp]
    sete al
    movzx rax, al
    mov [rel gpr_test_rsp_matched], rax

    ; Copy captured values into the output structure buffer
    mov rdi, [rel gpr_test_out_ptr]
    mov rax, [rel gpr_test_out_rax]
    mov [rdi + 0x00], rax
    mov rax, [rel gpr_test_out_rcx]
    mov [rdi + 0x08], rax
    mov rax, [rel gpr_test_out_rdx]
    mov [rdi + 0x10], rax
    mov rax, [rel gpr_test_out_rbx]
    mov [rdi + 0x18], rax
    mov rax, [rel gpr_test_out_rbp]
    mov [rdi + 0x20], rax
    mov rax, [rel gpr_test_out_rsi]
    mov [rdi + 0x28], rax
    mov rax, [rel gpr_test_out_rdi]
    mov [rdi + 0x30], rax
    mov rax, [rel gpr_test_out_r8]
    mov [rdi + 0x38], rax
    mov rax, [rel gpr_test_out_r9]
    mov [rdi + 0x40], rax
    mov rax, [rel gpr_test_out_r10]
    mov [rdi + 0x48], rax
    mov rax, [rel gpr_test_out_r11]
    mov [rdi + 0x50], rax
    mov rax, [rel gpr_test_out_r12]
    mov [rdi + 0x58], rax
    mov rax, [rel gpr_test_out_r13]
    mov [rdi + 0x60], rax
    mov rax, [rel gpr_test_out_r14]
    mov [rdi + 0x68], rax
    mov rax, [rel gpr_test_out_r15]
    mov [rdi + 0x70], rax
    mov rax, [rel gpr_test_out_rsp]
    mov [rdi + 0x78], rax
    mov rax, [rel gpr_test_out_rip]
    mov [rdi + 0x80], rax
    mov rax, [rel gpr_test_out_rflags]
    mov [rdi + 0x88], rax
    mov rax, [rel gpr_test_loop_count]
    mov [rdi + 0x90], rax
    mov rax, [rel gpr_test_rsp_matched]
    mov [rdi + 0x98], rax

    ; Restore caller's callee-saved registers in exact reverse order
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

section .bss
align 8
gpr_test_out_ptr:      resq 1
gpr_test_out_rax:      resq 1
gpr_test_out_rcx:      resq 1
gpr_test_out_rdx:      resq 1
gpr_test_out_rbx:      resq 1
gpr_test_out_rbp:      resq 1
gpr_test_out_rsi:      resq 1
gpr_test_out_rdi:      resq 1
gpr_test_out_r8:       resq 1
gpr_test_out_r9:       resq 1
gpr_test_out_r10:      resq 1
gpr_test_out_r11:      resq 1
gpr_test_out_r12:      resq 1
gpr_test_out_r13:      resq 1
gpr_test_out_r14:      resq 1
gpr_test_out_r15:      resq 1
gpr_test_out_rsp:      resq 1
gpr_test_out_rip:      resq 1
gpr_test_out_rflags:   resq 1
gpr_test_loop_count:   resq 1
gpr_test_expected_rsp: resq 1
gpr_test_rsp_matched:  resq 1

section .data
align 8
gpr_sentinel_flag:     dq 0
