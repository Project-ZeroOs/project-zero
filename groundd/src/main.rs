//! ZeroOS - Agent Spatial Grounding Query Broker (`groundd`)
//!
//! Authoritative Specification: Stage 6F Architecture Specification Rev7 (Frozen).
//! Implementation Plan: Stage 6F Implementation Plan Rev2.
//!
//! Invariant `I-6F-SUBORDINATE-GROUNDD`:
//! `groundd` operates as an unprivileged, read-only spatial indexing broker.
//! It possesses 0 write/mutation IPC opcodes or syscall paths.
//! It enforces 4-tier privacy classification and geometric redaction.

#![no_std]
#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use libzero::syscall::sys_exit;

#[allow(unused_imports)]
use libzero::{
    ZeroError, SpatialNodeDescriptor, SpatialNodeQuery,
    OP_GROUND_QUERY_SPATIAL, ERR_STALE_SPATIAL_INDEX,
    PRIVACY_TIER_0_PUBLIC, PRIVACY_TIER_1_WORKSPACE_PRIVATE,
    PRIVACY_TIER_2_WORKSPACE_SENSITIVE, PRIVACY_TIER_3_TRUSTED_OVERLAY,
    FLAG_SENSITIVE, BOUNDS_REDACTED, NODE_REDACTED,
};

pub const MAX_INDEXED_NODES: usize = 256;
pub const MAX_QUERY_RESULTS: usize = 16;
pub const CAP_TYPE_WORKSPACE_ACCESS: u32 = 0x0030;

// ============================================================================
// Internal Preallocated Spatial Node Entry (`SpatialNodeEntry`)
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct SpatialNodeEntry {
    pub descriptor: SpatialNodeDescriptor,
    pub workspace_id: u64,
    pub is_auth_overlay: bool,
    pub active: bool,
}

impl Default for SpatialNodeEntry {
    fn default() -> Self {
        Self {
            descriptor: SpatialNodeDescriptor::default(),
            workspace_id: 0,
            is_auth_overlay: false,
            active: false,
        }
    }
}

// ============================================================================
// Query Result Buffer (`SpatialQueryResult`)
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct SpatialQueryResult {
    pub nodes: [SpatialNodeDescriptor; MAX_QUERY_RESULTS],
    pub count: usize,
    pub layout_gen: u64,
    pub semantic_gen: u64,
}

impl Default for SpatialQueryResult {
    fn default() -> Self {
        Self {
            nodes: [SpatialNodeDescriptor::default(); MAX_QUERY_RESULTS],
            count: 0,
            layout_gen: 0,
            semantic_gen: 0,
        }
    }
}

// ============================================================================
// Spatial Indexing Broker Core (`SpatialBroker`)
// ============================================================================

pub struct SpatialBroker {
    nodes: [SpatialNodeEntry; MAX_INDEXED_NODES],
    node_count: usize,
    layout_gen: u64,
    semantic_gen: u64,
}

impl SpatialBroker {
    pub const fn new() -> Self {
        Self {
            nodes: [SpatialNodeEntry {
                descriptor: SpatialNodeDescriptor {
                    surface_id: 0,
                    node_id: 0,
                    bounds_min_x: 0,
                    bounds_min_y: 0,
                    bounds_max_x: 0,
                    bounds_max_y: 0,
                    node_type: 0,
                    privacy_tier: PRIVACY_TIER_0_PUBLIC,
                    flags: 0,
                    _pad0: [0; 4],
                    layout_gen: 0,
                    semantic_gen: 0,
                    node_name_len: 0,
                    node_name: [0; 64],
                    _reserved: [0; 4],
                },
                workspace_id: 0,
                is_auth_overlay: false,
                active: false,
            }; MAX_INDEXED_NODES],
            node_count: 0,
            layout_gen: 1,
            semantic_gen: 1,
        }
    }

    pub fn current_layout_gen(&self) -> u64 {
        self.layout_gen
    }

    pub fn current_semantic_gen(&self) -> u64 {
        self.semantic_gen
    }

    pub fn update_generations(&mut self, layout_gen: u64, semantic_gen: u64) {
        self.layout_gen = layout_gen;
        self.semantic_gen = semantic_gen;
    }

    pub fn register_node(&mut self, workspace_id: u64, is_auth_overlay: bool, mut descriptor: SpatialNodeDescriptor) -> Result<usize, ZeroError> {
        if self.node_count >= MAX_INDEXED_NODES {
            return Err(ZeroError::ObjectTableFull);
        }

        descriptor.layout_gen = self.layout_gen;
        descriptor.semantic_gen = self.semantic_gen;

        let index = self.node_count;
        self.nodes[index] = SpatialNodeEntry {
            descriptor,
            workspace_id,
            is_auth_overlay,
            active: true,
        };
        self.node_count += 1;
        Ok(index)
    }

    /// Primary Spatial Grounding Query Handler.
    /// Enforces:
    /// - Generation freshness check (`ERR_STALE_SPATIAL_INDEX` 0x0730)
    /// - Workspace Containment (`WorkspaceAccessCap` 0x0030 check)
    /// - 4-Tier Privacy Classification & Redaction
    /// - Tier 3 Trusted Overlay Exclusion
    pub fn query_spatial(
        &self,
        query: &SpatialNodeQuery,
        caller_has_workspace_cap: bool,
    ) -> Result<SpatialQueryResult, ZeroError> {
        // 1. Generation Freshness Check
        if query.expected_layout_gen > 0 && query.expected_layout_gen != self.layout_gen {
            return Err(ZeroError::StaleSpatialIndex);
        }
        if query.expected_semantic_gen > 0 && query.expected_semantic_gen != self.semantic_gen {
            return Err(ZeroError::StaleSpatialIndex);
        }

        let mut result = SpatialQueryResult {
            nodes: [SpatialNodeDescriptor::default(); MAX_QUERY_RESULTS],
            count: 0,
            layout_gen: self.layout_gen,
            semantic_gen: self.semantic_gen,
        };

        let max_results = if query.max_nodes > 0 && (query.max_nodes as usize) < MAX_QUERY_RESULTS {
            query.max_nodes as usize
        } else {
            MAX_QUERY_RESULTS
        };

        // 2. Iterate Indexed Spatial Nodes
        for i in 0..self.node_count {
            let entry = &self.nodes[i];
            if !entry.active {
                continue;
            }

            // Filter by target Workspace ID
            if entry.workspace_id != query.workspace_id {
                continue;
            }

            // Tier 3 Check: Trusted Overlay (authui) Exclusion -> MUST return 0 elements to queries
            if entry.is_auth_overlay || entry.descriptor.privacy_tier == PRIVACY_TIER_3_TRUSTED_OVERLAY {
                continue;
            }

            // Tier 1 Check: Workspace Containment Isolation -> Requires WorkspaceAccessCap (0x0030)
            if entry.descriptor.privacy_tier == PRIVACY_TIER_1_WORKSPACE_PRIVATE && !caller_has_workspace_cap {
                return Err(ZeroError::PermissionDenied);
            }

            let mut node_desc = entry.descriptor;

            // Tier 2 & FLAG_SENSITIVE Check: Geometric & Semantic Redaction
            if node_desc.privacy_tier == PRIVACY_TIER_2_WORKSPACE_SENSITIVE || (node_desc.flags & FLAG_SENSITIVE != 0) {
                node_desc.bounds_min_x = BOUNDS_REDACTED[0];
                node_desc.bounds_min_y = BOUNDS_REDACTED[1];
                node_desc.bounds_max_x = BOUNDS_REDACTED[2];
                node_desc.bounds_max_y = BOUNDS_REDACTED[3];
                node_desc.node_type = NODE_REDACTED;
                node_desc.node_name_len = 0;
                node_desc.node_name = [0; 64];
            }

            // Bounding Box Spatial Region Filtering (if non-zero query bounding box is specified)
            if query.bounding_box_max_x > query.bounding_box_min_x && node_desc.privacy_tier != PRIVACY_TIER_2_WORKSPACE_SENSITIVE {
                let intersects = !(node_desc.bounds_max_x < query.bounding_box_min_x
                    || node_desc.bounds_min_x > query.bounding_box_max_x
                    || node_desc.bounds_max_y < query.bounding_box_min_y
                    || node_desc.bounds_min_y > query.bounding_box_max_y);
                if !intersects {
                    continue;
                }
            }

            if result.count < max_results {
                result.nodes[result.count] = node_desc;
                result.count += 1;
            } else {
                break;
            }
        }

        Ok(result)
    }

    /// Read-Only IPC Opcode Dispatcher.
    /// Strictly rejects all mutation opcodes with `InvalidRequest` (-9).
    pub fn dispatch_ipc(&self, opcode: u64, query: Option<&SpatialNodeQuery>, caller_has_workspace_cap: bool) -> Result<SpatialQueryResult, ZeroError> {
        match opcode {
            OP_GROUND_QUERY_SPATIAL => {
                let q = query.ok_or(ZeroError::InvalidRequest)?;
                self.query_spatial(q, caller_has_workspace_cap)
            }
            _ => Err(ZeroError::InvalidRequest), // All write/mutation opcodes strictly rejected
        }
    }
}

// ============================================================================
// Freestanding Entry Points
// ============================================================================

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let broker = SpatialBroker::new();
    let _ = broker.node_count;
    unsafe { sys_exit(0); }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { sys_exit(1); }
}

// ============================================================================
// Unit Tests (Executed via `cargo test`)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_grounding_tier1_public() {
        let mut broker = SpatialBroker::new();
        let mut node = SpatialNodeDescriptor::default();
        node.surface_id = 1;
        node.privacy_tier = PRIVACY_TIER_0_PUBLIC;
        node.bounds_min_x = 5;
        node.bounds_min_y = 5;
        node.bounds_max_x = 50;
        node.bounds_max_y = 50;
        broker.register_node(1, false, node).unwrap();

        let query = SpatialNodeQuery {
            workspace_id: 1,
            expected_layout_gen: 1,
            expected_semantic_gen: 1,
            ..Default::default()
        };

        let res = broker.query_spatial(&query, false).unwrap();
        assert_eq!(res.count, 1);
        assert_eq!(res.nodes[0].surface_id, 1);
        assert_eq!(res.nodes[0].bounds_min_x, 5);
    }

    #[test]
    fn test_sensitive_element_geometric_masking() {
        let mut broker = SpatialBroker::new();
        let mut node = SpatialNodeDescriptor::default();
        node.surface_id = 2;
        node.privacy_tier = PRIVACY_TIER_2_WORKSPACE_SENSITIVE;
        node.flags = FLAG_SENSITIVE;
        node.bounds_min_x = 100;
        node.bounds_min_y = 100;
        node.bounds_max_x = 500;
        node.bounds_max_y = 500;
        node.node_type = 12;
        node.node_name_len = 11;
        node.node_name[..11].copy_from_slice(b"Secret Title");
        broker.register_node(1, false, node).unwrap();

        let query = SpatialNodeQuery {
            workspace_id: 1,
            expected_layout_gen: 1,
            expected_semantic_gen: 1,
            ..Default::default()
        };

        let res = broker.query_spatial(&query, true).unwrap();
        assert_eq!(res.count, 1);
        assert_eq!(res.nodes[0].bounds_min_x, 0);
        assert_eq!(res.nodes[0].bounds_min_y, 0);
        assert_eq!(res.nodes[0].bounds_max_x, 0);
        assert_eq!(res.nodes[0].bounds_max_y, 0);
        assert_eq!(res.nodes[0].node_type, 0xFF);
        assert_eq!(res.nodes[0].node_name_len, 0);
        assert_eq!(res.nodes[0].node_name[0], 0);
    }

    #[test]
    fn test_trusted_overlay_exclusion() {
        let mut broker = SpatialBroker::new();
        let mut node = SpatialNodeDescriptor::default();
        node.surface_id = 99;
        node.privacy_tier = PRIVACY_TIER_3_TRUSTED_OVERLAY;
        broker.register_node(1, true, node).unwrap();

        let query = SpatialNodeQuery {
            workspace_id: 1,
            expected_layout_gen: 1,
            expected_semantic_gen: 1,
            ..Default::default()
        };

        let res = broker.query_spatial(&query, true).unwrap();
        assert_eq!(res.count, 0); // Must return zero elements
    }

    #[test]
    fn test_workspace_containment_isolation() {
        let mut broker = SpatialBroker::new();
        let mut node = SpatialNodeDescriptor::default();
        node.surface_id = 5;
        node.privacy_tier = PRIVACY_TIER_1_WORKSPACE_PRIVATE;
        broker.register_node(1, false, node).unwrap();

        let query = SpatialNodeQuery {
            workspace_id: 1,
            expected_layout_gen: 1,
            expected_semantic_gen: 1,
            ..Default::default()
        };

        // Query lacking WorkspaceAccessCap -> PermissionDenied
        let err = broker.query_spatial(&query, false).unwrap_err();
        assert_eq!(err, ZeroError::PermissionDenied);

        // Query with WorkspaceAccessCap -> Success
        let res = broker.query_spatial(&query, true).unwrap();
        assert_eq!(res.count, 1);
    }

    #[test]
    fn test_absence_of_mutation_authority() {
        let broker = SpatialBroker::new();
        // Any mutation opcode (e.g. 0x0822 or arbitrary 0x9999) must be rejected with InvalidRequest
        let err = broker.dispatch_ipc(0x0822, None, true).unwrap_err();
        assert_eq!(err, ZeroError::InvalidRequest);
    }

    #[test]
    fn test_stale_spatial_index_generation() {
        let mut broker = SpatialBroker::new();
        broker.update_generations(5, 10);

        let query = SpatialNodeQuery {
            workspace_id: 1,
            expected_layout_gen: 4, // Stale expected generation
            expected_semantic_gen: 10,
            ..Default::default()
        };

        let err = broker.query_spatial(&query, true).unwrap_err();
        assert_eq!(err, ZeroError::StaleSpatialIndex);
    }
}
