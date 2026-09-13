/*
 * Project Zero - Stage 1 Minimal Bootstrap Trampoline
 * Architecture: x86 (32-bit Protected Mode) -> x86-64 (64-bit Long Mode)
 * Boot Protocol: Multiboot 1
 *
 * This assembly trampoline is responsible for:
 * 1. Advertising Multiboot 1 compliance to the bootloader (QEMU).
 * 2. Emitting early hardware diagnostic byte 'B' to COM1 (0x3F8).
 * 3. Identity mapping the first 1 GiB of RAM via 2 MiB huge pages (PML4 -> PDPT -> PD).
 * 4. Enabling PAE (CR4.PAE = 1) and Long Mode (IA32_EFER.LME = 1).
 * 5. Enabling Paging (CR0.PG = 1).
 * 6. Loading a clean 64-bit Global Descriptor Table (GDT).
 * 7. Executing a far jump into 64-bit Long Mode.
 * 8. Setting up the 64-bit stack and jumping to Rust kernel_main.
 */

.code32

/* Multiboot 1 Header Constants */
.set MB_MAGIC,    0x1BADB002
.set MB_FLAGS,    0x00000003          /* Align modules on 4KB; provide memory map */
.set MB_CHECKSUM, -(MB_MAGIC + MB_FLAGS)

.section .multiboot_header, "a"
.align 4
.long MB_MAGIC
.long MB_FLAGS
.long MB_CHECKSUM

.section .text
.global _start
.type _start, @function

_start:
    /* Disable hardware interrupts immediately */
    cli

    /* Early diagnostic marker 'B' (Bootloader reached in 32-bit mode) */
    mov $0x3F8, %dx
    mov $'B', %al
    out %al, %dx

    /* Set up early identity page tables (1 GiB via 2 MiB huge pages) */
    call setup_page_tables

    /* Early diagnostic marker 'P' (Paging structures initialized) */
    mov $0x3F8, %dx
    mov $'P', %al
    out %al, %dx

    /* Load PML4 physical base address into CR3 */
    mov $pml4_table, %eax
    mov %eax, %cr3

    /* Enable Physical Address Extension (PAE) in CR4 */
    mov %cr4, %eax
    or $(1 << 5), %eax                 /* CR4.PAE = bit 5 */
    mov %eax, %cr4

    /* Enable Long Mode (LME) in IA32_EFER Model-Specific Register (0xC0000080) */
    mov $0xC0000080, %ecx
    rdmsr
    or $(1 << 8), %eax                 /* EFER.LME = bit 8 */
    wrmsr

    /* Enable Paging (PG) and Protection (PE) in CR0 */
    mov %cr0, %eax
    or $(1 << 31) | (1 << 0), %eax     /* CR0.PG = bit 31, CR0.PE = bit 0 */
    mov %eax, %cr0

    /* Load 64-bit Global Descriptor Table (GDT) */
    lgdt gdt64_ptr

    /* Far jump to 64-bit code segment (CS = 0x08) */
    ljmp $0x08, $long_mode_entry

/*
 * setup_page_tables:
 * Identity maps the first 1 GiB of physical memory using 2 MiB huge pages.
 * PML4[0] -> PDPT
 * PDPT[0] -> PD
 * PD[0..511] -> 0 MiB, 2 MiB, 4 MiB, ..., 1022 MiB
 */
setup_page_tables:
    /* Link PML4[0] to PDPT (Present | Writable = 0x03) */
    mov $pdpt_table, %eax
    or $0x03, %eax
    mov %eax, pml4_table

    /* Link PDPT[0] to PD (Present | Writable = 0x03) */
    mov $pd_table, %eax
    or $0x03, %eax
    mov %eax, pdpt_table

    /* Map each PD entry to a 2 MiB huge page (Present | Writable | Huge = 0x83) */
    xor %ecx, %ecx                     /* ecx = entry counter (0 to 511) */
.map_pd_loop:
    mov %ecx, %eax
    shl $21, %eax                      /* eax = ecx * 2 MiB */
    or $0x83, %eax                     /* Present | Writable | Huge Page (2MB) */
    mov %eax, pd_table(,%ecx,8)        /* Low 32 bits of entry */
    movl $0x0, pd_table+4(,%ecx,8)     /* High 32 bits of entry = 0 */
    inc %ecx
    cmp $512, %ecx
    jne .map_pd_loop
    ret

/*
 * 64-Bit Long Mode Entry Point
 */
.code64
long_mode_entry:
    /* Diagnostic marker 'L' (64-bit Long Mode successfully entered) */
    mov $0x3F8, %dx
    mov $'L', %al
    out %al, %dx

    /* Reload data segment registers with 64-bit data selector (0x10) */
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov %ax, %fs
    mov %ax, %gs

    /* Establish 16-byte aligned 64-bit stack */
    mov $stack_top, %rsp

    /* Diagnostic marker 'K' (Stack established, invoking Rust kernel_main) */
    mov $0x3F8, %dx
    mov $'K', %al
    out %al, %dx

    /* Newline marker */
    mov $'\n', %al
    out %al, %dx

    /* Call Project Zero Rust no_std kernel entry */
    call kernel_main

    /* If kernel_main returns, safely halt the CPU in an idle loop */
.halt_loop:
    cli
    hlt
    jmp .halt_loop

/*
 * Global Descriptor Table (64-bit)
 */
.section .rodata
.align 8
gdt64:
    .quad 0x0000000000000000           /* 0x00: Null descriptor */
    .quad 0x00AF9A000000FFFF           /* 0x08: 64-bit Kernel Code (DPL 0, Exec/Read, L=1, D=0) */
    .quad 0x00CF92000000FFFF           /* 0x10: 64-bit Kernel Data (DPL 0, Writable) */
gdt64_end:

gdt64_ptr:
    .word gdt64_end - gdt64 - 1        /* GDT Limit */
    .quad gdt64                        /* GDT Base Address */

/*
 * Uninitialized Data: Stack and Early Page Tables
 */
.section .bss
.align 16
stack_bottom:
    .skip 65536                        /* 64 KiB kernel stack */
stack_top:

.align 4096
pml4_table:
    .skip 4096                         /* Page Map Level 4 (PML4) */
pdpt_table:
    .skip 4096                         /* Page Directory Pointer Table (PDPT) */
pd_table:
    .skip 4096                         /* Page Directory (PD) */
