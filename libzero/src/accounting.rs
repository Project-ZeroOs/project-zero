//! ZeroOS - libzero Multi-Dimensional Accounting Engine
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Strictly enforces I-ACCOUNTING-NONNEGATIVE, I-ACCOUNTING-CONSERVATION,
//! I-ACCOUNTING-FEASIBILITY, I-ACCOUNTING-ATOMIC-ADMISSION, and I-QUOTA-BOUNDED.

use crate::error::ZeroError;
use crate::lease::CouplingConstraintMatrix;
use crate::resource::{DimensionCapacityVector, DistributedId, MAX_ACCOUNTING_DIMENSIONS};

pub const MAX_QUOTA_ENTITIES: usize = 32;

/// Entity quota record.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EntityQuota {
    pub entity_id: u64,
    pub max_capacity: DimensionCapacityVector,
    pub used_capacity: DimensionCapacityVector,
    pub active: bool,
}

/// Accounting ledger record for quarantined capacity.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct QuarantineRecord {
    pub resource_id: DistributedId,
    pub lease_id: DistributedId,
    pub entity_id: u64,
    pub quarantined_capacity: DimensionCapacityVector,
    pub reason: u8, // 1: TimeAuthorityLost, 2: ProviderLost, 3: PeerClosed
    pub active: bool,
}

pub const MAX_QUARANTINE_RECORDS: usize = 64;

/// Accounting Domain representing local capacity conservation for a single resource.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AccountingDomain {
    pub resource_id: DistributedId,
    pub phys_capacity: DimensionCapacityVector,
    pub avail_capacity: DimensionCapacityVector,
    pub resv_capacity: DimensionCapacityVector,
    pub alloc_capacity: DimensionCapacityVector,
    pub unavail_capacity: DimensionCapacityVector,
    pub writer_locked: bool,
}

impl AccountingDomain {
    pub fn new(resource_id: DistributedId, phys: DimensionCapacityVector) -> Result<Self, ZeroError> {
        if phys.dimension_count as usize > MAX_ACCOUNTING_DIMENSIONS {
            return Err(ZeroError::UnsupportedResourceShape);
        }
        let domain = Self {
            resource_id,
            phys_capacity: phys,
            avail_capacity: phys,
            resv_capacity: DimensionCapacityVector::empty(),
            alloc_capacity: DimensionCapacityVector::empty(),
            unavail_capacity: DimensionCapacityVector::empty(),
            writer_locked: false,
        };
        domain.assert_conservation()?;
        Ok(domain)
    }

    /// Strictly verifies C_avail + C_resv + C_alloc + C_unavail == C_phys across all active dimensions.
    pub fn assert_conservation(&self) -> Result<(), ZeroError> {
        let count = self.phys_capacity.dimension_count as usize;
        for i in 0..count {
            let sum = self.avail_capacity.dimensions[i]
                .checked_add(self.resv_capacity.dimensions[i])
                .and_then(|v| v.checked_add(self.alloc_capacity.dimensions[i]))
                .and_then(|v| v.checked_add(self.unavail_capacity.dimensions[i]))
                .ok_or(ZeroError::VectorConservationViolated)?;

            if sum != self.phys_capacity.dimensions[i] {
                return Err(ZeroError::VectorConservationViolated);
            }
        }
        Ok(())
    }

    /// Atomic admission of a capacity allocation.
    pub fn admit_allocation(
        &mut self,
        requested: &DimensionCapacityVector,
        coupling: Option<&CouplingConstraintMatrix>,
    ) -> Result<(), ZeroError> {
        if self.writer_locked {
            return Err(ZeroError::WouldBlock);
        }
        self.writer_locked = true;

        let res = self.admit_allocation_inner(requested, coupling);
        self.writer_locked = false;
        res
    }

    fn admit_allocation_inner(
        &mut self,
        requested: &DimensionCapacityVector,
        coupling: Option<&CouplingConstraintMatrix>,
    ) -> Result<(), ZeroError> {
        // 1. Dimension availability check
        if !requested.le(&self.avail_capacity) {
            return Err(ZeroError::InvalidRequest);
        }

        // 2. Linear coupling constraints check (A * (C_alloc + requested) <= b)
        let new_alloc = self.alloc_capacity.checked_add(requested)?;
        if let Some(mat) = coupling {
            if !mat.is_feasible(&new_alloc) {
                return Err(ZeroError::CouplingViolation);
            }
        }

        // 3. Mutate vectors
        self.avail_capacity = self.avail_capacity.checked_sub(requested)?;
        self.alloc_capacity = new_alloc;

        // 4. Invariant assertion
        self.assert_conservation()?;
        Ok(())
    }

    /// Normal lease release (voluntary release by client).
    pub fn release_allocation(&mut self, amount: &DimensionCapacityVector) -> Result<(), ZeroError> {
        self.alloc_capacity = self.alloc_capacity.checked_sub(amount)?;
        self.avail_capacity = self.avail_capacity.checked_add(amount)?;
        self.assert_conservation()
    }

    /// Unified Quarantine: Moves allocated capacity to quarantined capacity upon authority invalidation.
    /// (TimeAuthorityLost or PeerClosed).
    /// C_alloc -> C_unavail. C_avail remains strictly unchanged!
    pub fn quarantine_allocation(&mut self, amount: &DimensionCapacityVector) -> Result<(), ZeroError> {
        self.alloc_capacity = self.alloc_capacity.checked_sub(amount)?;
        self.unavail_capacity = self.unavail_capacity.checked_add(amount)?;
        self.assert_conservation()
    }

    /// Quarantines all capacity on ProviderLost.
    /// C_unavail_new = C_unavail + C_avail + C_resv + C_alloc == C_phys.
    /// C_avail = 0, C_resv = 0, C_alloc = 0.
    pub fn quarantine_on_provider_lost(&mut self) -> Result<(), ZeroError> {
        let count = self.phys_capacity.dimension_count as usize;
        for i in 0..count {
            let total = self.unavail_capacity.dimensions[i]
                .checked_add(self.avail_capacity.dimensions[i])
                .and_then(|v| v.checked_add(self.resv_capacity.dimensions[i]))
                .and_then(|v| v.checked_add(self.alloc_capacity.dimensions[i]))
                .ok_or(ZeroError::VectorConservationViolated)?;

            self.unavail_capacity.dimensions[i] = total;
            self.avail_capacity.dimensions[i] = 0;
            self.resv_capacity.dimensions[i] = 0;
            self.alloc_capacity.dimensions[i] = 0;
        }
        self.unavail_capacity.dimension_count = self.phys_capacity.dimension_count;
        self.avail_capacity.dimension_count = self.phys_capacity.dimension_count;
        self.resv_capacity.dimension_count = self.phys_capacity.dimension_count;
        self.alloc_capacity.dimension_count = self.phys_capacity.dimension_count;

        self.assert_conservation()
    }

    /// Authoritative confirmation of release/reset by driver: C_unavail -> C_avail.
    pub fn confirm_physical_release(&mut self, amount: &DimensionCapacityVector) -> Result<(), ZeroError> {
        self.unavail_capacity = self.unavail_capacity.checked_sub(amount)?;
        self.avail_capacity = self.avail_capacity.checked_add(amount)?;
        self.assert_conservation()
    }

    /// Capacity reservation (e.g. headroom or thermal throttling).
    pub fn reserve_capacity(&mut self, amount: &DimensionCapacityVector) -> Result<(), ZeroError> {
        self.avail_capacity = self.avail_capacity.checked_sub(amount)?;
        self.resv_capacity = self.resv_capacity.checked_add(amount)?;
        self.assert_conservation()
    }
}

/// Node-wide accounting manager tracking multiple resources, quotas, and quarantine ledger.
pub struct AccountingManager {
    pub domains: [AccountingDomain; 64],
    pub domain_count: usize,
    pub quotas: [EntityQuota; MAX_QUOTA_ENTITIES],
    pub quarantine_ledger: [QuarantineRecord; MAX_QUARANTINE_RECORDS],
}

impl AccountingManager {
    pub const fn new() -> Self {
        Self {
            domains: [AccountingDomain {
                resource_id: DistributedId { node_id: 0, local_seq: 0 },
                phys_capacity: DimensionCapacityVector::empty(),
                avail_capacity: DimensionCapacityVector::empty(),
                resv_capacity: DimensionCapacityVector::empty(),
                alloc_capacity: DimensionCapacityVector::empty(),
                unavail_capacity: DimensionCapacityVector::empty(),
                writer_locked: false,
            }; 64],
            domain_count: 0,
            quotas: [EntityQuota {
                entity_id: 0,
                max_capacity: DimensionCapacityVector::empty(),
                used_capacity: DimensionCapacityVector::empty(),
                active: false,
            }; MAX_QUOTA_ENTITIES],
            quarantine_ledger: [QuarantineRecord {
                resource_id: DistributedId { node_id: 0, local_seq: 0 },
                lease_id: DistributedId { node_id: 0, local_seq: 0 },
                entity_id: 0,
                quarantined_capacity: DimensionCapacityVector::empty(),
                reason: 0,
                active: false,
            }; MAX_QUARANTINE_RECORDS],
        }
    }

    pub fn register_domain(&mut self, resource_id: DistributedId, phys: DimensionCapacityVector) -> Result<usize, ZeroError> {
        if self.domain_count >= self.domains.len() {
            return Err(ZeroError::ObjectTableFull);
        }
        let domain = AccountingDomain::new(resource_id, phys)?;
        let idx = self.domain_count;
        self.domains[idx] = domain;
        self.domain_count += 1;
        Ok(idx)
    }

    pub fn find_domain_mut(&mut self, resource_id: &DistributedId) -> Option<&mut AccountingDomain> {
        for i in 0..self.domain_count {
            if self.domains[i].resource_id == *resource_id {
                return Some(&mut self.domains[i]);
            }
        }
        None
    }

    pub fn set_quota(&mut self, entity_id: u64, max_cap: DimensionCapacityVector) -> Result<(), ZeroError> {
        for quota in self.quotas.iter_mut() {
            if quota.active && quota.entity_id == entity_id {
                quota.max_cap_update(max_cap);
                return Ok(());
            }
        }
        for quota in self.quotas.iter_mut() {
            if !quota.active {
                quota.entity_id = entity_id;
                quota.max_capacity = max_cap;
                quota.used_capacity = DimensionCapacityVector::empty();
                quota.used_capacity.dimension_count = max_cap.dimension_count;
                quota.active = true;
                return Ok(());
            }
        }
        Err(ZeroError::ObjectTableFull)
    }

    pub fn check_and_charge_quota(&mut self, entity_id: u64, amount: &DimensionCapacityVector) -> Result<(), ZeroError> {
        for quota in self.quotas.iter_mut() {
            if quota.active && quota.entity_id == entity_id {
                let new_used = quota.used_capacity.checked_add(amount)?;
                if !new_used.le(&quota.max_capacity) {
                    return Err(ZeroError::QuotaExceeded);
                }
                quota.used_capacity = new_used;
                return Ok(());
            }
        }
        // No explicit quota set implies unlimited local headroom
        Ok(())
    }

    pub fn release_quota(&mut self, entity_id: u64, amount: &DimensionCapacityVector) -> Result<(), ZeroError> {
        for quota in self.quotas.iter_mut() {
            if quota.active && quota.entity_id == entity_id {
                quota.used_capacity = quota.used_capacity.checked_sub(amount)?;
                return Ok(());
            }
        }
        Ok(())
    }

    pub fn record_quarantine(
        &mut self,
        res_id: DistributedId,
        lease_id: DistributedId,
        entity_id: u64,
        amount: DimensionCapacityVector,
        reason: u8,
    ) -> Result<(), ZeroError> {
        for rec in self.quarantine_ledger.iter_mut() {
            if !rec.active {
                rec.resource_id = res_id;
                rec.lease_id = lease_id;
                rec.entity_id = entity_id;
                rec.quarantined_capacity = amount;
                rec.reason = reason;
                rec.active = true;
                return Ok(());
            }
        }
        Err(ZeroError::ObjectTableFull)
    }
}

impl EntityQuota {
    fn max_cap_update(&mut self, new_max: DimensionCapacityVector) {
        self.max_capacity = new_max;
    }
}
