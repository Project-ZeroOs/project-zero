//! ZeroOS - libzero Durable DistributedId Allocator
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Strictly enforces I-RES-ID-UNIQUE, I-ID-DURABLE-ALLOCATOR-STATE, and burn-on-crash non-reuse.

use crate::error::ZeroError;
use crate::persistence::{PersistenceAuthority, DISTRIBUTED_ID_CEILING_SLOT};
use crate::resource::DistributedId;

pub const DEFAULT_ID_BATCH_SIZE: u64 = 64;

pub struct DistributedIdAllocator<P: PersistenceAuthority> {
    node_id: u64,
    current_seq: u64,
    persisted_ceiling: u64,
    batch_size: u64,
    persistence: P,
}

impl<P: PersistenceAuthority> DistributedIdAllocator<P> {
    /// Recovers the persisted ceiling from persistent storage, burns any unissued IDs
    /// from the previous reservation window, commits a new batch ceiling, and initializes.
    pub fn recover_or_init(node_id: u64, batch_size: u64, mut persistence: P) -> Result<Self, ZeroError> {
        let persisted_ceiling = persistence.read_slot(DISTRIBUTED_ID_CEILING_SLOT)?;

        // Burn-on-crash: start current_seq strictly at persisted_ceiling
        let current_seq = persisted_ceiling;

        let target_ceiling = persisted_ceiling
            .checked_add(batch_size)
            .ok_or(ZeroError::IdentifierExhausted)?;

        persistence.write_and_commit_slot(DISTRIBUTED_ID_CEILING_SLOT, target_ceiling)?;

        Ok(Self {
            node_id,
            current_seq,
            persisted_ceiling: target_ceiling,
            batch_size,
            persistence,
        })
    }

    /// Allocates the next globally unique DistributedId under write-ahead reservation.
    pub fn allocate_id(&mut self) -> Result<DistributedId, ZeroError> {
        if self.current_seq >= self.persisted_ceiling {
            let next_ceiling = self.persisted_ceiling
                .checked_add(self.batch_size)
                .ok_or(ZeroError::IdentifierExhausted)?;

            // Commit new ceiling to durable media before issuing any ID in this window
            self.persistence.write_and_commit_slot(DISTRIBUTED_ID_CEILING_SLOT, next_ceiling)?;
            self.persisted_ceiling = next_ceiling;
        }

        self.current_seq = self.current_seq
            .checked_add(1)
            .ok_or(ZeroError::IdentifierExhausted)?;

        Ok(DistributedId::new(self.node_id, self.current_seq))
    }

    pub fn current_seq(&self) -> u64 {
        self.current_seq
    }

    pub fn persisted_ceiling(&self) -> u64 {
        self.persisted_ceiling
    }

    pub fn persistence(&self) -> &P {
        &self.persistence
    }

    pub fn persistence_mut(&mut self) -> &mut P {
        &mut self.persistence
    }

    /// Advances the allocation sequence floor (e.g. after loading durable state).
    pub fn advance_floor(&mut self, floor_seq: u64) -> Result<(), ZeroError> {
        if floor_seq > self.current_seq {
            self.current_seq = floor_seq;
            if self.current_seq >= self.persisted_ceiling {
                let next_ceiling = self.current_seq
                    .checked_add(self.batch_size)
                    .ok_or(ZeroError::IdentifierExhausted)?;
                self.persistence.write_and_commit_slot(DISTRIBUTED_ID_CEILING_SLOT, next_ceiling)?;
                self.persisted_ceiling = next_ceiling;
            }
        }
        Ok(())
    }
}
