; Project Zero - Stage 2F-A Bootstrap Trampoline
; Assembler: NASM (format: elf64)
; Target: x86-64 bare-metal kernel loaded by QEMU Multiboot 1

bits 32

KERNEL_VIRT_BASE equ 0xFFFFFFFF80000000
%define PHYS(sym) ((sym) - KERNEL_VIRT_BASE)

; Multiboot 1 Header Constants
MB_MAGIC    equ 0x1BADB002
MB_FLAGS    equ 0x00000003          ; Align modules on 4KB; provide memory map
MB_CHECKSUM equ -(MB_MAGIC + MB_FLAGS)

section .multiboot_header alloc
align 4
    dd MB_MAGIC
    dd MB_FLAGS
    dd MB_CHECKSUM

section .text
global _start
extern kernel_main

_start:
    ; 1. Disable maskable interrupts immediately
    cli

    ; Multiboot 1 Boot Contract:
    ; EAX = Multiboot Magic (0x2BADB002)
    ; EBX = Physical address of multiboot_info structure
    mov [PHYS(multiboot_magic_raw)], eax
    mov [PHYS(multiboot_info_raw)], ebx

    ; 2. Diagnostic marker 'B' (Bootloader reached in 32-bit mode)
    mov dx, 0x3F8
    mov al, 'B'
    out dx, al

    ; 3. Set up early page tables (Dual mapping 1 GiB via 2 MiB huge pages)
    call setup_page_tables

    ; 4. Diagnostic marker 'P' (Paging structures initialized)
    mov dx, 0x3F8
    mov al, 'P'
    out dx, al

    ; 5. Load PML4 base into CR3
    mov eax, PHYS(pml4_table)
    mov cr3, eax

    ; 6. Enable PAE in CR4 (bit 5)
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    ; 7. Enable Long Mode (LME) in IA32_EFER MSR (0xC0000080, bit 8)
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    ; 8. Enable Paging (PG, bit 31) and Protection (PE, bit 0) in CR0
    mov eax, cr0
    or eax, (1 << 31) | (1 << 0)
    mov cr0, eax

    ; 9. Load 64-bit Global Descriptor Table
    lgdt [PHYS(gdt64_ptr)]

    ; 10. Far jump to 64-bit code segment (CS = 0x08)
    jmp 0x08:PHYS(long_mode_entry)

setup_page_tables:
    ; Link PML4[0] -> PDPT (Identity mapping [0, 512 GiB))
    mov eax, PHYS(pdpt_table)
    or eax, 0x03
    mov [PHYS(pml4_table)], eax
    mov dword [PHYS(pml4_table) + 4], 0

    ; Link PML4[256] -> hhdm_pdpt (HHDM base 0xFFFF_8000_0000_0000, covers 512 GiB)
    mov eax, PHYS(hhdm_pdpt)
    or eax, 0x03
    mov [PHYS(pml4_table) + 256 * 8], eax
    mov dword [PHYS(pml4_table) + 256 * 8 + 4], 0

    ; Link PML4[511] -> PDPT (Higher-half mapping top 512 GiB)
    mov eax, PHYS(pdpt_table)
    or eax, 0x03
    mov [PHYS(pml4_table) + 511 * 8], eax
    mov dword [PHYS(pml4_table) + 511 * 8 + 4], 0

    ; Link PDPT[0] -> PD (Identity mapping [0, 1 GiB))
    mov eax, PHYS(pd_table)
    or eax, 0x03
    mov [PHYS(pdpt_table)], eax
    mov dword [PHYS(pdpt_table) + 4], 0

    ; Link hhdm_pdpt[0] -> PD (HHDM direct maps physical 0..1 GiB via 2 MiB huge pages)
    mov eax, PHYS(pd_table)
    or eax, 0x03
    mov [PHYS(hhdm_pdpt)], eax
    mov dword [PHYS(hhdm_pdpt) + 4], 0

    ; Link PDPT[510] -> PD (Higher-half 1 GiB window beginning at KERNEL_VIRT_BASE [0xFFFF_FFFF_8000_0000, 0xFFFF_FFFF_C000_0000))
    mov eax, PHYS(pd_table)
    or eax, 0x03
    mov [PHYS(pdpt_table) + 510 * 8], eax
    mov dword [PHYS(pdpt_table) + 510 * 8 + 4], 0

    ; Map each of the 512 PD entries to a 2 MiB huge page
    ; entry i: (i * 2 MiB) | 0x83 (Present | Writable | Huge)
    xor ecx, ecx
.map_loop:
    mov eax, ecx
    shl eax, 21                 ; eax = ecx * 2 MiB
    or eax, 0x83                ; Present | Writable | Huge (2MB)
    mov [PHYS(pd_table) + ecx * 8], eax
    mov dword [PHYS(pd_table) + ecx * 8 + 4], 0
    inc ecx
    cmp ecx, 512
    jne .map_loop
    ret

bits 64
global long_mode_entry
global _start_higher_half

long_mode_entry:
    ; 11. Diagnostic marker 'L' (64-bit Long Mode entered in low identity memory)
    mov dx, 0x3F8
    mov al, 'L'
    out dx, al

    ; 12. Reload data segment registers with 64-bit data selector (0x10)
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; 13. Stage 2F-C: Execute absolute 64-bit jump to Higher-Half Virtual Address
    mov rax, _start_higher_half
    jmp rax

_start_higher_half:
    ; NOW executing with RIP in canonical higher-half space (0xFFFFFFFF8010xxxx)!
    ; 14. Establish 16-byte aligned 64-bit higher-half stack
    mov rsp, stack_top

    ; 15. Diagnostic marker 'K' (Higher-half execution confirmed, calling kernel_main)
    mov dx, 0x3F8
    mov al, 'K'
    out dx, al
    mov al, 10                  ; Newline '\n'
    out dx, al

    ; 16. Pass Multiboot 1 parameters according to Project Zero internal Rust ABI:
    ; RDI (1st arg) = multiboot_info physical address (from EBX)
    ; RSI (2nd arg) = multiboot_magic value (from EAX)
    mov edi, [rel multiboot_info_raw]
    mov esi, [rel multiboot_magic_raw]

    ; Call Project Zero Rust no_std kernel entry in higher-half address space
    call kernel_main

    ; 17. Safe halt loop
.halt:
    cli
    hlt
    jmp .halt

section .rodata
align 8
gdt64:
    dq 0x0000000000000000       ; 0x00: Null descriptor
    dq 0x00AF9A000000FFFF       ; 0x08: 64-bit Kernel Code (DPL 0, Exec/Read, L=1, D=0)
    dq 0x00CF92000000FFFF       ; 0x10: 64-bit Kernel Data (DPL 0, Writable)
gdt64_end:

gdt64_ptr:
    dw gdt64_end - gdt64 - 1
    dq PHYS(gdt64)

section .data
align 8
global multiboot_magic_raw
global multiboot_info_raw
multiboot_magic_raw: dd 0
multiboot_info_raw:  dd 0

section .page_tables alloc noexec write
align 4096
global pml4_table
global pdpt_table
global pd_table
global hhdm_pdpt
global page_tables_end
pml4_table:
    resb 4096
pdpt_table:
    resb 4096
pd_table:
    resb 4096
hhdm_pdpt:
    resb 4096
page_tables_end:


; Dedicated section for stack guard page (outside .bss)
section .stack_guard alloc noexec write
align 4096
global stack_guard_page
global stack_guard_page_end
stack_guard_page:
    resb 4096                   ; 4 KiB unmapped guard page directly below stack_bottom
stack_guard_page_end:

; Dedicated section for normal kernel stack (outside .bss)
section .stack alloc noexec write
align 4096
global stack_bottom
global stack_top
stack_bottom:
    resb 65536                  ; 64 KiB kernel stack (16 x 4 KiB pages)
stack_top:
