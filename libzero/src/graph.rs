//! ZeroOS - libzero Unified Resource Graph
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Strictly enforces I-RES-AUTHORITY-LOCAL, I-GRAPH-SEPARATION, and I-RES-DISAPPEARANCE-CASCADE.

use crate::error::ZeroError;
use crate::resource::{DistributedId, ResourceDescriptor, ResourceState, MAX_RESOURCES_PER_NODE};

pub struct TopologyNode {
    pub descriptor: ResourceDescriptor,
    pub parent_idx: Option<usize>,
    pub neighbors: [Option<usize>; 4], // Directed physical connections (can form cycles)
    pub neighbor_count: usize,
    pub active: bool,
}

pub struct ResourceGraph {
    pub nodes: [TopologyNode; MAX_RESOURCES_PER_NODE],
    pub node_count: usize,
}

impl ResourceGraph {
    pub const fn new() -> Self {
        const EMPTY_NODE: TopologyNode = TopologyNode {
            descriptor: ResourceDescriptor {
                resource_id: DistributedId { node_id: 0, local_seq: 0 },
                generation: 0,
                resource_type: crate::resource::ResourceType::Unknown,
                locality_domain: crate::resource::LocalityDomain::HostLocal,
                state: ResourceState::Unavailable,
                dimension_count: 0,
                phys_capacity: crate::resource::DimensionCapacityVector::empty(),
                energy_tier: crate::resource::EnergyTier::Measured,
                _pad_align: 0,
                current_temp_mxc: 0,
                current_power_mw: 0,
                provider_endpoint: 0,
                auth_cap_handle: 0,
                _padding: [0; 16],
            },
            parent_idx: None,
            neighbors: [None; 4],
            neighbor_count: 0,
            active: false,
        };

        Self {
            nodes: [EMPTY_NODE; MAX_RESOURCES_PER_NODE],
            node_count: 0,
        }
    }

    pub fn insert_resource(&mut self, desc: ResourceDescriptor) -> Result<usize, ZeroError> {
        for (idx, node) in self.nodes.iter_mut().enumerate() {
            if !node.active {
                node.descriptor = desc;
                node.parent_idx = None;
                node.neighbor_count = 0;
                node.neighbors = [None; 4];
                node.active = true;
                if idx >= self.node_count {
                    self.node_count = idx + 1;
                }
                return Ok(idx);
            }
        }
        Err(ZeroError::ObjectTableFull)
    }

    pub fn add_topology_edge(&mut self, from_idx: usize, to_idx: usize) -> Result<(), ZeroError> {
        if from_idx >= self.node_count || to_idx >= self.node_count {
            return Err(ZeroError::NotFound);
        }
        let from_node = &mut self.nodes[from_idx];
        if from_node.neighbor_count >= 4 {
            return Err(ZeroError::ObjectTableFull);
        }
        from_node.neighbors[from_node.neighbor_count] = Some(to_idx);
        from_node.neighbor_count += 1;
        Ok(())
    }

    pub fn find_by_id(&self, id: &DistributedId) -> Option<usize> {
        for (idx, node) in self.nodes.iter().enumerate() {
            if node.active && node.descriptor.resource_id == *id {
                return Some(idx);
            }
        }
        None
    }

    pub fn mark_provider_lost(&mut self, res_id: &DistributedId) -> Result<(), ZeroError> {
        if let Some(idx) = self.find_by_id(res_id) {
            self.nodes[idx].descriptor.state = ResourceState::Unavailable;
            self.nodes[idx].descriptor.generation = self.nodes[idx].descriptor.generation.wrapping_add(1);
            Ok(())
        } else {
            Err(ZeroError::NotFound)
        }
    }

    /// Traversal with visited set to verify structural physical cycles do not trigger infinite loops.
    pub fn count_reachable(&self, start_idx: usize) -> usize {
        let mut visited = [false; MAX_RESOURCES_PER_NODE];
        let mut stack = [0usize; MAX_RESOURCES_PER_NODE];
        let mut stack_len = 0usize;

        if start_idx >= self.node_count || !self.nodes[start_idx].active {
            return 0;
        }

        stack[stack_len] = start_idx;
        stack_len += 1;
        visited[start_idx] = true;
        let mut count = 0;

        while stack_len > 0 {
            stack_len -= 1;
            let cur = stack[stack_len];
            count += 1;

            let node = &self.nodes[cur];
            for i in 0..node.neighbor_count {
                if let Some(nxt) = node.neighbors[i] {
                    if nxt < self.node_count && self.nodes[nxt].active && !visited[nxt] {
                        visited[nxt] = true;
                        stack[stack_len] = nxt;
                        stack_len += 1;
                    }
                }
            }
        }
        count
    }
}
