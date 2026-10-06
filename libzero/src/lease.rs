//! ZeroOS - libzero Lease Authority & Coupling Constraints
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & ADR-0025 Rev12.

use crate::error::ZeroError;
use crate::resource::{DimensionCapacityVector, DistributedId, MAX_ACCOUNTING_DIMENSIONS};

pub const MAX_LEASES_PER_NODE: usize = 256;
pub const MAX_QUOTA_ENTITIES: usize = 32;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LeaseState {
    Active = 0,
    Expired = 1,
    TimeAuthorityLost = 2,
    ProviderLost = 3,
    Released = 4,
}

impl Default for LeaseState {
    fn default() -> Self {
        Self::Active
    }
}

/// Bounded Resource Lease Token.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceLease {
    pub lease_id: DistributedId,                // 16 bytes: offset 0..16
    pub resource_id: DistributedId,             // 16 bytes: offset 16..32
    pub generation: u32,                        // 4 bytes: offset 32..36
    pub state: LeaseState,                      // 1 byte: offset 36..37
    pub _pad1: [u8; 3],                         // 3 bytes: offset 37..40
    pub allocated_capacity: DimensionCapacityVector, // 72 bytes: offset 40..112
    pub granted_tick: u64,                      // 8 bytes: offset 112..120
    pub expiration_tick: u64,                   // 8 bytes: offset 120..128
    pub holder_entity_id: u64,                  // 8 bytes: offset 128..136
    pub client_channel_handle: u32,             // 4 bytes: offset 136..140
    pub _padding: [u8; 4],                      // 4 bytes: offset 140..144
}

const _: () = assert!(core::mem::size_of::<ResourceLease>() == 144);

/// Linear joint coupling constraint matrix: A * C <= b
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CouplingConstraintMatrix {
    pub coefficients: [[i32; MAX_ACCOUNTING_DIMENSIONS]; MAX_ACCOUNTING_DIMENSIONS],
    pub limits: [i64; MAX_ACCOUNTING_DIMENSIONS],
    pub constraint_count: u8,
}

impl Default for CouplingConstraintMatrix {
    fn default() -> Self {
        Self::empty()
    }
}

impl CouplingConstraintMatrix {
    pub const fn empty() -> Self {
        Self {
            coefficients: [[0; MAX_ACCOUNTING_DIMENSIONS]; MAX_ACCOUNTING_DIMENSIONS],
            limits: [0; MAX_ACCOUNTING_DIMENSIONS],
            constraint_count: 0,
        }
    }

    pub fn add_constraint(&mut self, row: [i32; MAX_ACCOUNTING_DIMENSIONS], limit: i64) -> Result<(), ZeroError> {
        if self.constraint_count as usize >= MAX_ACCOUNTING_DIMENSIONS {
            return Err(ZeroError::DimensionLimitExceeded);
        }
        let idx = self.constraint_count as usize;
        self.coefficients[idx] = row;
        self.limits[idx] = limit;
        self.constraint_count += 1;
        Ok(())
    }

    /// Evaluates linear feasibility across all defined constraints.
    pub fn is_feasible(&self, cap: &DimensionCapacityVector) -> bool {
        let count = self.constraint_count as usize;
        let dim_count = cap.dimension_count as usize;

        for r in 0..count {
            let mut sum: i64 = 0;
            for c in 0..dim_count {
                let coeff = self.coefficients[r][c] as i64;
                let val = cap.dimensions[c] as i64;
                sum = sum.saturating_add(coeff.saturating_mul(val));
            }
            if sum > self.limits[r] {
                return false;
            }
        }
        true
    }
}
