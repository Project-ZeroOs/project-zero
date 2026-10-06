//! ZeroOS - libzero Resource Model & Multi-Dimensional Descriptors
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & ADR-0025 Rev12.

use crate::error::ZeroError;

pub const MAX_ACCOUNTING_DIMENSIONS: usize = 8;
pub const MAX_RESOURCES_PER_NODE: usize = 64;

/// Globally unique distributed identifier tuple (NodeId, LocalSeq).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DistributedId {
    pub node_id: u64,
    pub local_seq: u64,
}

impl DistributedId {
    pub const fn new(node_id: u64, local_seq: u64) -> Self {
        Self { node_id, local_seq }
    }

    pub const fn is_nil(&self) -> bool {
        self.node_id == 0 && self.local_seq == 0
    }
}

/// Bounded multi-dimensional capacity vector (up to 8 dimensions).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DimensionCapacityVector {
    pub dimensions: [u64; MAX_ACCOUNTING_DIMENSIONS],
    pub dimension_count: u8,
    pub _padding: [u8; 7],
}

const _: () = assert!(core::mem::size_of::<DimensionCapacityVector>() == 72);

impl DimensionCapacityVector {
    pub const fn empty() -> Self {
        Self {
            dimensions: [0; MAX_ACCOUNTING_DIMENSIONS],
            dimension_count: 0,
            _padding: [0; 7],
        }
    }

    pub fn single(amount: u64) -> Self {
        let mut v = Self::empty();
        v.dimensions[0] = amount;
        v.dimension_count = 1;
        v
    }

    pub fn new(count: u8, values: &[u64]) -> Result<Self, ZeroError> {
        if count as usize > MAX_ACCOUNTING_DIMENSIONS || values.len() > MAX_ACCOUNTING_DIMENSIONS {
            return Err(ZeroError::UnsupportedResourceShape);
        }
        let mut v = Self::empty();
        v.dimension_count = count;
        let mut i = 0;
        while i < values.len() && i < count as usize {
            v.dimensions[i] = values[i];
            i += 1;
        }
        Ok(v)
    }

    pub fn is_zero(&self) -> bool {
        let count = self.dimension_count as usize;
        for i in 0..count {
            if self.dimensions[i] != 0 {
                return false;
            }
        }
        true
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self, ZeroError> {
        let count = if self.dimension_count > other.dimension_count {
            self.dimension_count
        } else {
            other.dimension_count
        };
        if count as usize > MAX_ACCOUNTING_DIMENSIONS {
            return Err(ZeroError::UnsupportedResourceShape);
        }

        let mut res = Self::empty();
        res.dimension_count = count;
        for i in 0..count as usize {
            res.dimensions[i] = self.dimensions[i]
                .checked_add(other.dimensions[i])
                .ok_or(ZeroError::DeadlineExhaustion)?;
        }
        Ok(res)
    }

    pub fn checked_sub(&self, other: &Self) -> Result<Self, ZeroError> {
        let count = if self.dimension_count > other.dimension_count {
            self.dimension_count
        } else {
            other.dimension_count
        };
        if count as usize > MAX_ACCOUNTING_DIMENSIONS {
            return Err(ZeroError::UnsupportedResourceShape);
        }

        let mut res = Self::empty();
        res.dimension_count = count;
        for i in 0..count as usize {
            res.dimensions[i] = self.dimensions[i]
                .checked_sub(other.dimensions[i])
                .ok_or(ZeroError::VectorConservationViolated)?;
        }
        Ok(res)
    }

    pub fn le(&self, other: &Self) -> bool {
        let count = if self.dimension_count > other.dimension_count {
            self.dimension_count
        } else {
            other.dimension_count
        };
        for i in 0..count as usize {
            if self.dimensions[i] > other.dimensions[i] {
                return false;
            }
        }
        true
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ResourceType {
    Unknown = 0,
    Cpu = 1,
    Memory = 2,
    GpuCore = 3,
    GpuMemory = 4,
    Dma = 5,
    Accelerator = 6,
}

impl Default for ResourceType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LocalityDomain {
    HostLocal = 0,
    Numa0 = 1,
    Numa1 = 2,
    PcieBus = 3,
}

impl Default for LocalityDomain {
    fn default() -> Self {
        Self::HostLocal
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ResourceState {
    Available = 0,
    Quarantined = 1,
    Unavailable = 2,
    Faulted = 3,
}

impl Default for ResourceState {
    fn default() -> Self {
        Self::Available
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EnergyTier {
    Measured = 0,
    Estimated = 1,
    Declared = 2,
}

impl Default for EnergyTier {
    fn default() -> Self {
        Self::Measured
    }
}

/// 128-byte frozen Resource Descriptor (Stage 4B ABI).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceDescriptor {
    pub resource_id: DistributedId,             // 16 bytes: offset 0..16
    pub generation: u32,                        // 4 bytes: offset 16..20
    pub resource_type: ResourceType,            // 1 byte: offset 20..21
    pub locality_domain: LocalityDomain,        // 1 byte: offset 21..22
    pub state: ResourceState,                   // 1 byte: offset 22..23
    pub dimension_count: u8,                    // 1 byte: offset 23..24
    pub phys_capacity: DimensionCapacityVector, // 72 bytes: offset 24..96
    pub energy_tier: EnergyTier,                // 1 byte: offset 96..97
    pub _pad_align: u8,                         // 1 byte: offset 97..98
    pub current_temp_mxc: u16,                  // 2 bytes: offset 98..100
    pub current_power_mw: u32,                  // 4 bytes: offset 100..104
    pub provider_endpoint: u32,                 // 4 bytes: offset 104..108
    pub auth_cap_handle: u32,                   // 4 bytes: offset 108..112
    pub _padding: [u8; 16],                     // 16 bytes: offset 112..128
}

const _: () = assert!(core::mem::size_of::<ResourceDescriptor>() == 128);
