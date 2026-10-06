//! ZeroOS - init Boot Epoch Persistence Subsystem
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Strictly enforces I-BOOT-EPOCH-DURABILITY, I-BOOT-EPOCH-NONREUSE, and I-BOOT-EPOCH-NONWRAP.

use libzero::error::ZeroError;
use libzero::persistence::{PersistenceAuthority, BOOT_EPOCH_SLOT};

/// Recovers the previous BootEpochId from persistent storage, increments it via checked
/// addition, commits the new epoch to non-volatile media, and returns the active epoch.
///
/// NOTE: SFENCE orders CPU stores, while persistence.write_and_commit_slot constitutes
/// the authoritative durable media commit barrier.
pub fn advance_boot_epoch<P: PersistenceAuthority>(persistence: &mut P) -> Result<u64, ZeroError> {
    let current_epoch = persistence.read_slot(BOOT_EPOCH_SLOT)?;

    // Non-wrapping checked addition. Must fail closed on u64::MAX (EpochExhaustion).
    let next_epoch = current_epoch
        .checked_add(1)
        .ok_or(ZeroError::EpochExhaustion)?;

    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_mm_sfence();
    }

    // Authoritative persistence barrier
    persistence.write_and_commit_slot(BOOT_EPOCH_SLOT, next_epoch)?;

    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_mm_sfence();
    }

    Ok(next_epoch)
}
