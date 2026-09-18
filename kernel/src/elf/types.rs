//! Project Zero - Stage 3J ELF Types & Structures
//!
//! Authoritative Contract: Stage 3J Architecture Specification Rev2 (Approved & Frozen).

use core::mem::size_of;

pub const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
pub const ELFCLASS64: u8 = 2;
pub const ELFDATA2LSB: u8 = 1;
pub const EV_CURRENT: u32 = 1;
pub const ET_EXEC: u16 = 2;
pub const EM_X86_64: u16 = 0x3e;

pub const PT_LOAD: u32 = 1;
pub const PT_NOTE: u32 = 4;
pub const PT_PHDR: u32 = 6;
pub const PT_GNU_STACK: u32 = 0x6474e551;

pub const PF_X: u32 = 0x1;
pub const PF_W: u32 = 0x2;
pub const PF_R: u32 = 0x4;

pub const MAX_PROGRAM_HEADERS: usize = 16;

// Frozen user stack geometry
pub const USER_STACK_SIZE: u64 = 16 * 1024; // 16 KiB = 4 pages
pub const USER_STACK_PAGES: usize = 4;
pub const USER_STACK_TOP: u64 = 0x0000_7F7F_FFFF_0000; // 16-byte aligned
pub const USER_STACK_BASE: u64 = USER_STACK_TOP - USER_STACK_SIZE; // 0x0000_7F7F_FFFC_0000
pub const USER_STACK_GUARD: u64 = USER_STACK_BASE - 4096; // 0x0000_7F7F_FFFB_F000 (unmapped)
pub const USER_INITIAL_RSP: u64 = USER_STACK_TOP;

/// 64-bit ELF file header (64 bytes, 8-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

const _: () = assert!(size_of::<Elf64Ehdr>() == 64);
const _: () = assert!(core::mem::align_of::<Elf64Ehdr>() == 8);

/// 64-bit ELF program header (56 bytes, 8-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

const _: () = assert!(size_of::<Elf64Phdr>() == 56);
const _: () = assert!(core::mem::align_of::<Elf64Phdr>() == 8);

/// Strongly typed errors encountered during ELF parsing, validation, or loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    ImageTooSmall,
    InvalidMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedVersion,
    UnsupportedType,
    UnsupportedMachine,
    InvalidHeader,
    UnsupportedProgramHeader,
    ExecutableStackRejected,
    MalformedSegment,
    FileBoundsExceeded,
    VirtualAddressOverflow,
    OutOfUserBounds,
    InvalidAlignment,
    MisalignedOffsetCongruence,
    OverlappingSegments,
    PagePermissionConflict,
    WwxViolation,
    InvalidPermissions,
    InvalidEntryPoint,
    CapacityExceeded,
    OutOfMemory,
    MappingFailed,
    StackCreationFailed,
    ProcessCreationFailed,
    ThreadCreationFailed,
}
