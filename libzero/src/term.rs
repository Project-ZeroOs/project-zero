//! ZeroOS - zero-term-lib Terminal Application Integration Library
//!
//! Authoritative Contract: Stage 5 Presentation & Stage 6F Grounding Architecture Specifications.
//! Integrates terminal-oriented applications into ZeroOS spatial context and workload DAGs.

use crate::error::ZeroError;
use crate::ipc::IpcMessage;
use crate::resource::DistributedId;
use crate::grounding::{
    SpatialNodeDescriptor, SpatialNodeQuery, PRIVACY_TIER_0_PUBLIC,
    PRIVACY_TIER_2_WORKSPACE_SENSITIVE, PRIVACY_TIER_3_TRUSTED_OVERLAY,
    FLAG_SENSITIVE, FLAG_INTERACTIVE, BOUNDS_REDACTED, NODE_REDACTED,
};
use crate::presentation::InputEventDescriptor;

// Protocol Opcodes for zero-term-lib (0x4F01 .. 0x4F06)
pub const OP_TERM_REGISTER: u64   = 0x4F01;
pub const OP_TERM_WRITE: u64      = 0x4F02;
pub const OP_TERM_READ: u64       = 0x4F03;
pub const OP_TERM_RESIZE: u64     = 0x4F04;
pub const OP_TERM_GROUND_PUB: u64 = 0x4F05;
pub const OP_TERM_DISCONNECT: u64 = 0x4F06;

pub const MAX_TERM_COLS: usize = 80;
pub const MAX_TERM_NODES: usize = 16;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermSurfaceState {
    Unregistered = 0,
    Registered = 1,
    Active = 2,
    Disconnected = 3,
    Crashed = 4,
}

/// Terminal line descriptor for spatial grounding and context observation.
#[derive(Debug, Clone, Copy)]
pub struct TermLineNode {
    pub line_idx: u16,
    pub privacy_tier: u8,
    pub text_len: u16,
    pub text: [u8; MAX_TERM_COLS],
}

impl TermLineNode {
    pub const fn empty() -> Self {
        Self {
            line_idx: 0,
            privacy_tier: PRIVACY_TIER_0_PUBLIC,
            text_len: 0,
            text: [0u8; MAX_TERM_COLS],
        }
    }
}

/// Durable zero-term-lib Terminal Surface Container handle.
#[derive(Debug, Clone, Copy)]
pub struct ZeroTermSurface {
    pub surface_id: u64,
    pub workspace_id: DistributedId,
    pub capability_handle: u32,
    pub cols: u16,
    pub rows: u16,
    pub layout_gen: u64,
    pub semantic_gen: u64,
    pub state: TermSurfaceState,
    pub node_count: usize,
    pub line_nodes: [TermLineNode; MAX_TERM_NODES],
    pub last_input_event: InputEventDescriptor,
}

impl ZeroTermSurface {
    pub const fn new() -> Self {
        Self {
            surface_id: 0,
            workspace_id: DistributedId { node_id: 0, local_seq: 0 },
            capability_handle: 0,
            cols: 80,
            rows: 24,
            layout_gen: 1,
            semantic_gen: 1,
            state: TermSurfaceState::Unregistered,
            node_count: 0,
            line_nodes: [TermLineNode::empty(); MAX_TERM_NODES],
            last_input_event: InputEventDescriptor {
                event_type: 0,
                button_state: 0,
                key_code: 0,
                pointer_x: 0,
                pointer_y: 0,
                timestamp_tsc: 0,
            },
        }
    }

    /// Registers a terminal surface associated with a workspace under capability authority.
    pub fn register_surface(
        &mut self,
        surface_id: u64,
        workspace_id: DistributedId,
        capability_handle: u32,
        cols: u16,
        rows: u16,
    ) -> Result<(), ZeroError> {
        if capability_handle == 0 {
            return Err(ZeroError::PermissionDenied); // TERM-G capability check
        }
        self.surface_id = surface_id;
        self.workspace_id = workspace_id;
        self.capability_handle = capability_handle;
        self.cols = cols;
        self.rows = rows;
        self.layout_gen = 1;
        self.semantic_gen = 1;
        self.state = TermSurfaceState::Registered;
        self.node_count = 0;
        Ok(())
    }

    /// Appends or updates terminal line content for spatial grounding context observation.
    pub fn publish_line(
        &mut self,
        line_idx: u16,
        text: &[u8],
        privacy_tier: u8,
        caller_workspace_id: DistributedId,
    ) -> Result<(), ZeroError> {
        if self.workspace_id != caller_workspace_id {
            return Err(ZeroError::PermissionDenied); // TERM-E workspace containment
        }
        if self.state != TermSurfaceState::Registered && self.state != TermSurfaceState::Active {
            return Err(ZeroError::InvalidRequest);
        }
        if self.node_count >= MAX_TERM_NODES {
            return Err(ZeroError::ObjectTableFull);
        }

        let slot = self.node_count;
        let mut line = TermLineNode::empty();
        line.line_idx = line_idx;
        line.privacy_tier = privacy_tier;
        let len = text.len().min(MAX_TERM_COLS);
        line.text[..len].copy_from_slice(&text[..len]);
        line.text_len = len as u16;

        self.line_nodes[slot] = line;
        self.node_count += 1;
        self.semantic_gen += 1;
        self.state = TermSurfaceState::Active;
        Ok(())
    }

    /// Queries spatially grounded terminal context for a caller workspace.
    /// Performs privacy tier masking for Tier 2 sensitive content and excludes Tier 3 overlays.
    pub fn query_spatial_grounding(
        &self,
        query: &SpatialNodeQuery,
        results: &mut [SpatialNodeDescriptor; MAX_TERM_NODES],
    ) -> Result<usize, ZeroError> {
        // Enforce workspace isolation boundary (TERM-E)
        if query.workspace_id != self.workspace_id.local_seq {
            return Ok(0); // Unauthorized workspace sees 0 nodes
        }

        let mut matched = 0;
        for i in 0..self.node_count {
            let line = &self.line_nodes[i];
            if line.privacy_tier == PRIVACY_TIER_3_TRUSTED_OVERLAY {
                continue; // Exclude trusted overlays from spatial queries
            }

            let mut node = SpatialNodeDescriptor::default();
            node.surface_id = self.surface_id;
            node.node_id = (self.surface_id << 16) | (line.line_idx as u64);
            node.privacy_tier = line.privacy_tier;
            node.layout_gen = self.layout_gen;
            node.semantic_gen = self.semantic_gen;

            if line.privacy_tier == PRIVACY_TIER_2_WORKSPACE_SENSITIVE {
                // Apply privacy masking rules (TERM-F)
                node.bounds_min_x = BOUNDS_REDACTED[0];
                node.bounds_min_y = BOUNDS_REDACTED[1];
                node.bounds_max_x = BOUNDS_REDACTED[2];
                node.bounds_max_y = BOUNDS_REDACTED[3];
                node.node_type = NODE_REDACTED;
                node.flags = FLAG_SENSITIVE;
                node.node_name_len = 0;
                node.node_name = [0u8; 64];
            } else {
                // Public or Workspace Private content (TERM-C, TERM-D)
                node.bounds_min_x = 0;
                node.bounds_min_y = line.line_idx as i32 * 16;
                node.bounds_max_x = self.cols as i32 * 8;
                node.bounds_max_y = (line.line_idx as i32 + 1) * 16;
                node.node_type = 1; // Text element type
                node.flags = FLAG_INTERACTIVE;

                let name_len = (line.text_len as usize).min(64);
                node.node_name[..name_len].copy_from_slice(&line.text[..name_len]);
                node.node_name_len = name_len as u32;
            }

            results[matched] = node;
            matched += 1;
            if matched >= query.max_nodes as usize || matched >= MAX_TERM_NODES {
                break;
            }
        }
        Ok(matched)
    }

    /// Ingests physical/validated human input routed from Stage 6C uids.
    pub fn ingest_uids_input(
        &mut self,
        event: &InputEventDescriptor,
        caller_workspace_id: DistributedId,
    ) -> Result<(), ZeroError> {
        if self.workspace_id != caller_workspace_id {
            return Err(ZeroError::PermissionDenied); // TERM-G workspace check
        }
        self.last_input_event = *event;
        Ok(())
    }

    /// Disconnects or tears down terminal surface upon application completion or crash.
    pub fn disconnect(&mut self) -> Result<(), ZeroError> {
        self.state = TermSurfaceState::Disconnected;
        self.node_count = 0;
        Ok(())
    }

    /// Handles application crash gracefully without corrupting workspace state.
    pub fn handle_crash(&mut self) {
        self.state = TermSurfaceState::Crashed;
        self.node_count = 0;
    }
}

pub fn pack_term_register_msg(surface_id: u64, cols: u16, rows: u16) -> Result<IpcMessage, ZeroError> {
    let mut payload = [0u8; 48];
    payload[0..8].copy_from_slice(&surface_id.to_le_bytes());
    payload[8..10].copy_from_slice(&cols.to_le_bytes());
    payload[10..12].copy_from_slice(&rows.to_le_bytes());
    IpcMessage::new(OP_TERM_REGISTER, &payload[..12])
}

pub fn pack_term_write_msg(line_idx: u16, data: &[u8]) -> Result<IpcMessage, ZeroError> {
    let mut payload = [0u8; 48];
    payload[0..2].copy_from_slice(&line_idx.to_le_bytes());
    let copy_len = data.len().min(46);
    payload[2..2 + copy_len].copy_from_slice(&data[..copy_len]);
    IpcMessage::new(OP_TERM_WRITE, &payload[..2 + copy_len])
}

pub fn pack_term_disconnect_msg(surface_id: u64) -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_TERM_DISCONNECT, &surface_id.to_le_bytes())
}
