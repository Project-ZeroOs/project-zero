; Project Zero - Low-Level Interrupt Service Routine (ISR) Stubs
; Architecture: x86-64
; Assembler: NASM (elf64)

bits 64

global load_idt
global load_gdt_and_tss
extern exception_dispatch
extern timer_interrupt_handler

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

; Hardware Timer IRQ0 mapped to Vector 32
global isr_32
isr_32:
    push qword 0               ; Dummy error code
    push qword 32              ; Vector 32 (Timer)
    
    ; Save all general-purpose registers
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

    ; Call timer interrupt handler in Rust
    call timer_interrupt_handler

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


