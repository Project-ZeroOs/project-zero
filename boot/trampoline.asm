; ====================================================================
; Project Zero - Application Processor (AP) Real-Mode Startup Trampoline
; Architecture: x86-64 (16-bit Real -> 32-bit Protected -> 64-bit Long Mode)
; Relocation Target: Physical Address 0x0000_8000 (SIPI Vector 0x08)
; ====================================================================

[bits 16]

section .rodata
global ap_trampoline_start
global ap_trampoline_end

align 16
ap_trampoline_start:
    cli
    cld

    ; Initialize 16-bit Real Mode segment registers (CS is 0x0800, base is 0x8000)
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Load temporary 32-bit GDT descriptor
    lgdt [0x8000 + (ap_trampoline_gdt_desc - ap_trampoline_start)]

    ; Enable Protected Mode (CR0.PE = 1)
    mov eax, cr0
    or eax, 1
    mov cr0, eax

    ; Far jump to 32-bit Protected Mode entry
    jmp dword 0x08:(0x8000 + (ap_pm_entry - ap_trampoline_start))

align 16
; Mailbox area embedded at fixed offsets relative to 0x8000
global ap_mailbox_cr3
global ap_mailbox_stack
global ap_mailbox_entry
global ap_mailbox_cpu_id
global ap_mailbox_online

ap_mailbox_cr3:     dq 0
ap_mailbox_stack:   dq 0
ap_mailbox_entry:   dq 0
ap_mailbox_cpu_id:  dd 0
ap_mailbox_online:  dd 0

align 16
ap_trampoline_gdt:
    dq 0x0000000000000000          ; 0x00: Null Descriptor
    dq 0x00CF9A000000FFFF          ; 0x08: 32-bit Flat Code (DPL 0, RX)
    dq 0x00CF92000000FFFF          ; 0x10: 32-bit Flat Data (DPL 0, RW)
    dq 0x00AF9A000000FFFF          ; 0x18: 64-bit Long Mode Code (DPL 0, RX)
ap_trampoline_gdt_end:

align 4
ap_trampoline_gdt_desc:
    dw (ap_trampoline_gdt_end - ap_trampoline_gdt - 1)
    dd 0x8000 + (ap_trampoline_gdt - ap_trampoline_start)

; --------------------------------------------------------------------
; 32-bit Protected Mode Entry Point
; --------------------------------------------------------------------
[bits 32]
align 16
ap_pm_entry:
    ; Reload data segments with flat 32-bit data selector (0x10)
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Enable Physical Address Extension (CR4.PAE = bit 5)
    mov eax, cr4
    or eax, 0x20
    mov cr4, eax

    ; Load CR3 (PML4 root physical address) from mailbox
    mov eax, [0x8000 + (ap_mailbox_cr3 - ap_trampoline_start)]
    mov cr3, eax

    ; Enable Long Mode in EFER MSR (0xC0000080, IA32_EFER.LME = bit 8)
    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 8)
    wrmsr

    ; Enable Paging (CR0.PG = bit 31)
    mov eax, cr0
    or eax, 0x80000000
    mov cr0, eax

    ; Far jump to 64-bit Long Mode entry in code selector 0x18
    jmp 0x18:(0x8000 + (ap_lm_entry - ap_trampoline_start))

; --------------------------------------------------------------------
; 64-bit Long Mode Entry Point
; --------------------------------------------------------------------
[bits 64]
align 16
ap_lm_entry:
    mov rbx, 0x8000

    ; Load 64-bit stack pointer from mailbox
    mov rsp, [rbx + (ap_mailbox_stack - ap_trampoline_start)]

    ; Signal arrival in 64-bit mode to mailbox
    mov dword [rbx + (ap_mailbox_online - ap_trampoline_start)], 1

    ; Load CPU ID into RDI (System V AMD64 first argument)
    xor rdi, rdi
    mov edi, [rbx + (ap_mailbox_cpu_id - ap_trampoline_start)]

    ; Load 64-bit Rust entry function pointer
    mov rax, [rbx + (ap_mailbox_entry - ap_trampoline_start)]

    ; Call Rust entry function: ap_startup_entry(cpu_id)
    call rax

    ; Fail-safe halt loop if Rust entry returns
.ap_halt:
    hlt
    jmp .ap_halt

align 16
ap_trampoline_end:
