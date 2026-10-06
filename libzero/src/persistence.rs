//! ZeroOS - libzero Persistence Authority Interface
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Mediates non-volatile persistence via capability-backed slot abstraction.

use crate::error::ZeroError;

pub const BOOT_EPOCH_SLOT: u32 = 0;
pub const DISTRIBUTED_ID_CEILING_SLOT: u32 = 1;

/// Trait representing an authoritative persistence provider.
/// Implementations must guarantee that write_and_commit_slot only returns
/// Ok(()) after durable media commit / flush barriers complete.
pub trait PersistenceAuthority {
    /// Read the persisted 64-bit value at the authorized slot.
    fn read_slot(&self, slot: u32) -> Result<u64, ZeroError>;

    /// Commit a new 64-bit value to durable non-volatile media.
    /// Must not return Ok(()) until media flush / non-volatile commit barrier completes.
    fn write_and_commit_slot(&mut self, slot: u32, value: u64) -> Result<(), ZeroError>;
}

/// In-memory persistence authority for tests and volatile execution environments.
#[derive(Clone, Copy, Debug)]
pub struct MemoryPersistenceAuthority {
    slots: [u64; 16],
    write_barrier_count: u64,
}

impl MemoryPersistenceAuthority {
    pub const fn new() -> Self {
        Self {
            slots: [0u64; 16],
            write_barrier_count: 0,
        }
    }

    pub fn with_initial_values(boot_epoch: u64, id_ceiling: u64) -> Self {
        let mut auth = Self::new();
        auth.slots[BOOT_EPOCH_SLOT as usize] = boot_epoch;
        auth.slots[DISTRIBUTED_ID_CEILING_SLOT as usize] = id_ceiling;
        auth
    }

    pub fn barrier_count(&self) -> u64 {
        self.write_barrier_count
    }
}

impl Default for MemoryPersistenceAuthority {
    fn default() -> Self {
        Self::new()
    }
}

impl PersistenceAuthority for MemoryPersistenceAuthority {
    fn read_slot(&self, slot: u32) -> Result<u64, ZeroError> {
        if (slot as usize) < self.slots.len() {
            Ok(self.slots[slot as usize])
        } else {
            Err(ZeroError::InvalidRequest)
        }
    }

    fn write_and_commit_slot(&mut self, slot: u32, value: u64) -> Result<(), ZeroError> {
        if (slot as usize) < self.slots.len() {
            self.slots[slot as usize] = value;
            self.write_barrier_count += 1;
            Ok(())
        } else {
            Err(ZeroError::InvalidRequest)
        }
    }
}
