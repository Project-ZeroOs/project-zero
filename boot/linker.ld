/*
 * Project Zero - Stage 2F-A Linker Script
 * Architecture: x86-64 (bare-metal ELF)
 * Virtual Base:  0xFFFFFFFF80000000 (Top Kernel Window)
 * Physical Base: 0x00100000 (1 MiB physical memory)
 */

KERNEL_VIRT_BASE = 0xFFFFFFFF80000000;
KERNEL_PHYS_BASE = 0x00100000;

_start_phys = _start - KERNEL_VIRT_BASE;
ENTRY(_start_phys)
EXTERN(ap_startup_entry)

SECTIONS {
    /* Stage 2F-A: Virtual addresses in higher half, loaded at physical 1 MiB */
    . = KERNEL_VIRT_BASE + KERNEL_PHYS_BASE;

    __kernel_start = .;

    /* Multiboot 1 header (Read-Only metadata, Non-Executable) */
    . = ALIGN(4K);
    __multiboot_header_start = .;
    .multiboot_header ALIGN(4K) : AT(ADDR(.multiboot_header) - KERNEL_VIRT_BASE) {
        KEEP(*(.multiboot_header))
    }
    . = ALIGN(4K);
    __multiboot_header_end = .;

    /* Executable machine instructions (Read-Only Executable) */
    . = ALIGN(4K);
    __text_start = .;
    .text ALIGN(4K) : AT(ADDR(.text) - KERNEL_VIRT_BASE) {
        *(.text .text.*)
        KEEP(*(.text.ap_startup_entry))
    }
    . = ALIGN(4K);
    __text_end = .;

    /* Read-only constants & descriptors (Read-Only Non-Executable) */
    . = ALIGN(4K);
    __rodata_start = .;
    .rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_VIRT_BASE) {
        *(.rodata .rodata.*)
    }
    . = ALIGN(4K);
    __rodata_end = .;

    /* Initialized mutable data + GOT (Writable Non-Executable) */
    . = ALIGN(4K);
    __data_start = .;
    .data ALIGN(4K) : AT(ADDR(.data) - KERNEL_VIRT_BASE) {
        *(.got .got.*)
        *(.data .data.*)
    }
    . = ALIGN(4K);
    __data_end = .;

    /* Uninitialized data (Rust runtime globals & tables ONLY) */
    . = ALIGN(4K);
    __bss_start = .;
    .bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_VIRT_BASE) {
        *(COMMON)
        *(.bss .bss.*)
    }
    . = ALIGN(4K);
    __bss_end = .;

    /* PMM Bitmap Metadata (Dedicated section, outside .bss) */
    __pmm_meta_start = .;
    __pmm_metadata_start = .;
    .pmm_metadata ALIGN(4K) : AT(ADDR(.pmm_metadata) - KERNEL_VIRT_BASE) {
        KEEP(*(.pmm_metadata))
    }
    . = ALIGN(4K);
    __pmm_meta_end = .;
    __pmm_metadata_end = .;

    /* Early 4-Level Page Tables (Dedicated section, outside .bss) */
    __page_tables_start = .;
    .page_tables ALIGN(4K) : AT(ADDR(.page_tables) - KERNEL_VIRT_BASE) {
        KEEP(*(.page_tables))
    }
    . = ALIGN(4K);
    __page_tables_end = .;

    /* Stack Guard Page (4 KiB, unmapped in Stage 2F-F, outside .bss) */
    __stack_guard_start = .;
    .stack_guard ALIGN(4K) : AT(ADDR(.stack_guard) - KERNEL_VIRT_BASE) {
        KEEP(*(.stack_guard))
    }
    . = ALIGN(4K);
    __stack_guard_end = .;

    /* Normal Kernel Stack (64 KiB, page-aligned, outside .bss) */
    __stack_start = .;
    .stack ALIGN(4K) : AT(ADDR(.stack) - KERNEL_VIRT_BASE) {
        KEEP(*(.stack))
    }
    . = ALIGN(4K);
    __stack_end = .;

    . = ALIGN(4K);
    __kernel_end = .;
}

/* Bounded bootstrap window: kernel must fit within initial 4 MiB PT */
ASSERT(__kernel_end <= KERNEL_VIRT_BASE + 0x400000, "Kernel image exceeds 4 MiB bootstrap PT mapping window!");
