//! ZeroOS - libzero Lease Authority Lifecycle Engine
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Strictly enforces I-LEASE-AUTH-BOUNDED, I-UNIFIED-QUARANTINE,
//! I-TIME-FAILURE-LEASE-SEMANTICS, I-TIME-DEADLINE-NONWRAP, and I-LEASE-RECONCILIATION-BOUNDED.

use crate::accounting::AccountingManager;
use crate::error::ZeroError;
use crate::lease::{CouplingConstraintMatrix, LeaseState, ResourceLease, MAX_LEASES_PER_NODE};
use crate::resource::{DimensionCapacityVector, DistributedId};

pub struct LeaseEngine {
    pub leases: [Option<ResourceLease>; MAX_LEASES_PER_NODE],
    pub lease_count: usize,
}

impl LeaseEngine {
    pub const fn new() -> Self {
        Self {
            leases: [None; MAX_LEASES_PER_NODE],
            lease_count: 0,
        }
    }

    /// Requests a new capability-mediated temporal capacity lease.
    pub fn request_lease(
        &mut self,
        lease_id: DistributedId,
        resource_id: DistributedId,
        entity_id: u64,
        client_channel_handle: u32,
        requested_amount: DimensionCapacityVector,
        duration_ticks: u64,
        current_tick: u64,
        caller_has_coverage: bool,
        accounting: &mut AccountingManager,
        coupling: Option<&CouplingConstraintMatrix>,
    ) -> Result<ResourceLease, ZeroError> {
        // 1. Capability coverage check (Lease Authority ⊆ Capability Authority)
        if !caller_has_coverage {
            return Err(ZeroError::PermissionDenied);
        }

        // 2. Non-wrapping deadline check
        let expiration_tick = current_tick
            .checked_add(duration_ticks)
            .ok_or(ZeroError::DeadlineExhaustion)?;

        // 3. Quota ceiling check
        accounting.check_and_charge_quota(entity_id, &requested_amount)?;

        // 4. Accounting domain admission
        let domain = accounting
            .find_domain_mut(&resource_id)
            .ok_or(ZeroError::NotFound)?;
        if let Err(e) = domain.admit_allocation(&requested_amount, coupling) {
            let _ = accounting.release_quota(entity_id, &requested_amount);
            return Err(e);
        }

        // 5. Allocate lease slot
        for slot in self.leases.iter_mut() {
            if slot.is_none() {
                let lease = ResourceLease {
                    lease_id,
                    resource_id,
                    generation: 1,
                    state: LeaseState::Active,
                    _pad1: [0; 3],
                    allocated_capacity: requested_amount,
                    granted_tick: current_tick,
                    expiration_tick,
                    holder_entity_id: entity_id,
                    client_channel_handle,
                    _padding: [0; 4],
                };
                *slot = Some(lease);
                self.lease_count += 1;
                return Ok(lease);
            }
        }

        // Rollback on table full
        if let Some(d) = accounting.find_domain_mut(&resource_id) {
            let _ = d.release_allocation(&requested_amount);
        }
        let _ = accounting.release_quota(entity_id, &requested_amount);
        Err(ZeroError::ObjectTableFull)
    }

    /// Voluntary lease release by caller.
    pub fn release_lease(
        &mut self,
        lease_id: &DistributedId,
        presented_gen: u32,
        accounting: &mut AccountingManager,
    ) -> Result<(), ZeroError> {
        for slot in self.leases.iter_mut() {
            if let Some(ref mut l) = slot {
                if l.lease_id == *lease_id {
                    if l.generation != presented_gen {
                        return Err(ZeroError::GenerationMismatch);
                    }
                    if l.state != LeaseState::Active {
                        return Err(ZeroError::InvalidRequest);
                    }
                    l.state = LeaseState::Released;
                    let amt = l.allocated_capacity;
                    let entity_id = l.holder_entity_id;
                    let res_id = l.resource_id;

                    if let Some(domain) = accounting.find_domain_mut(&res_id) {
                        domain.release_allocation(&amt)?;
                    }
                    let _ = accounting.release_quota(entity_id, &amt);
                    *slot = None;
                    self.lease_count = self.lease_count.saturating_sub(1);
                    return Ok(());
                }
            }
        }
        Err(ZeroError::NotFound)
    }

    /// Lease renewal check.
    pub fn renew_lease(
        &mut self,
        lease_id: &DistributedId,
        presented_gen: u32,
        additional_duration: u64,
        current_tick: u64,
    ) -> Result<u64, ZeroError> {
        for slot in self.leases.iter_mut() {
            if let Some(ref mut l) = slot {
                if l.lease_id == *lease_id {
                    if l.generation != presented_gen {
                        return Err(ZeroError::GenerationMismatch);
                    }
                    if l.state != LeaseState::Active {
                        return Err(ZeroError::InvalidRequest);
                    }
                    let new_exp = current_tick
                        .checked_add(additional_duration)
                        .ok_or(ZeroError::DeadlineExhaustion)?;

                    l.expiration_tick = new_exp;
                    l.generation = l.generation.wrapping_add(1);
                    return Ok(new_exp);
                }
            }
        }
        Err(ZeroError::NotFound)
    }

    /// Unified Quarantine Handler: TimeAuthorityLost.
    /// Transitions all active temporal leases to TimeAuthorityLost and moves capacity to C_unavail.
    pub fn handle_time_authority_loss(&mut self, accounting: &mut AccountingManager) {
        for slot in self.leases.iter_mut() {
            if let Some(ref mut l) = slot {
                if l.state == LeaseState::Active {
                    l.state = LeaseState::TimeAuthorityLost;
                    if let Some(domain) = accounting.find_domain_mut(&l.resource_id) {
                        let _ = domain.quarantine_allocation(&l.allocated_capacity);
                    }
                    let _ = accounting.record_quarantine(
                        l.resource_id,
                        l.lease_id,
                        l.holder_entity_id,
                        l.allocated_capacity,
                        1, // TimeAuthorityLost
                    );
                }
            }
        }
    }

    /// Unified Quarantine Handler: ProviderLost.
    /// Transitions all leases targeting res_id to ProviderLost.
    pub fn handle_provider_loss(&mut self, res_id: &DistributedId, accounting: &mut AccountingManager) {
        if let Some(domain) = accounting.find_domain_mut(res_id) {
            let _ = domain.quarantine_on_provider_lost();
        }

        for slot in self.leases.iter_mut() {
            if let Some(ref mut l) = slot {
                if l.resource_id == *res_id && l.state == LeaseState::Active {
                    l.state = LeaseState::ProviderLost;
                    let _ = accounting.record_quarantine(
                        l.resource_id,
                        l.lease_id,
                        l.holder_entity_id,
                        l.allocated_capacity,
                        2, // ProviderLost
                    );
                }
            }
        }
    }

    /// Unified Quarantine Handler: PeerClosed (Consumer crash or abrupt disconnection).
    pub fn handle_peer_closed(&mut self, client_channel_handle: u32, accounting: &mut AccountingManager) {
        for slot in self.leases.iter_mut() {
            if let Some(ref mut l) = slot {
                if l.client_channel_handle == client_channel_handle && l.state == LeaseState::Active {
                    l.state = LeaseState::Released;
                    if let Some(domain) = accounting.find_domain_mut(&l.resource_id) {
                        let _ = domain.quarantine_allocation(&l.allocated_capacity);
                    }
                    let _ = accounting.record_quarantine(
                        l.resource_id,
                        l.lease_id,
                        l.holder_entity_id,
                        l.allocated_capacity,
                        3, // PeerClosed
                    );
                }
            }
        }
    }

    /// Confirms driver hardware release: transitions C_unavail -> C_avail.
    pub fn confirm_driver_release(
        &mut self,
        res_id: &DistributedId,
        amount: &DimensionCapacityVector,
        accounting: &mut AccountingManager,
    ) -> Result<(), ZeroError> {
        let domain = accounting.find_domain_mut(res_id).ok_or(ZeroError::NotFound)?;
        domain.confirm_physical_release(amount)
    }

    /// Post-crash bounded reconciliation protocol:
    /// Re-attestation window cannot exceed remaining lifetime. Zero self-extension!
    pub fn reconcile_surviving_lease(
        &mut self,
        lease_id: &DistributedId,
        current_tick: u64,
        policy_max_window: u64,
    ) -> Result<u64, ZeroError> {
        for slot in self.leases.iter_mut() {
            if let Some(ref mut l) = slot {
                if l.lease_id == *lease_id {
                    if current_tick >= l.expiration_tick {
                        return Err(ZeroError::PermissionDenied); // Expired cannot resurrect
                    }
                    let remaining = l.expiration_tick - current_tick;
                    let window = if remaining < policy_max_window {
                        remaining
                    } else {
                        policy_max_window
                    };
                    l.generation = l.generation.wrapping_add(1);
                    return Ok(window);
                }
            }
        }
        Err(ZeroError::NotFound)
    }
}
