//! Project Zero - Stage 3J ELF Program Execution Subsystem
//!
//! Authoritative Contract: Stage 3J Architecture Specification Rev2 (Approved & Frozen).

pub mod types;
pub mod mmap;
pub mod validate;
pub mod load;
pub mod tests;

pub use types::{ElfError, USER_INITIAL_RSP, USER_STACK_BASE, USER_STACK_GUARD, USER_STACK_SIZE, USER_STACK_TOP};
pub use validate::validate_elf;
pub use load::load_elf;
