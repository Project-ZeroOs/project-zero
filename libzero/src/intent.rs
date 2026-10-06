//! ZeroOS - libzero Freestanding Human Intent & Workflow Representation
//!
//! Authoritative Contract: Stage 6D Architecture Specification Rev1 & ADR-0035.

use crate::resource::DistributedId;

pub const INTENT_TYPE_ONE_SHOT: u16          = 0x0001;
pub const INTENT_TYPE_COMMAND_PALETTE: u16    = 0x0002;
pub const INTENT_TYPE_HOTKEY: u16             = 0x0003;
pub const INTENT_TYPE_GESTURE: u16            = 0x0004;
pub const INTENT_TYPE_ACCESSIBILITY: u16      = 0x0005;
pub const INTENT_TYPE_AGENT_PROPOSAL: u16     = 0x0006;
pub const INTENT_TYPE_WORKFLOW_TEMPLATE: u16  = 0x0007;

pub const INTENT_STATE_SUBMITTED: u8         = 0x01;
pub const INTENT_STATE_PARSED: u8            = 0x02;
pub const INTENT_STATE_AMBIGUOUS: u8         = 0x03;
pub const INTENT_STATE_PENDING_AUTH: u8      = 0x04;
pub const INTENT_STATE_APPROVED: u8          = 0x05;
pub const INTENT_STATE_DISPATCHED: u8        = 0x06;
pub const INTENT_STATE_COMPLETED: u8         = 0x07;
pub const INTENT_STATE_REJECTED: u8          = 0x08;
pub const INTENT_STATE_FAILED: u8            = 0x09;

pub const SIDE_EFFECT_CLASS1_READONLY: u8    = 0x01;
pub const SIDE_EFFECT_CLASS2_SCOPED_MUTATE: u8= 0x02;
pub const SIDE_EFFECT_CLASS3_CONSEQUENTIAL: u8= 0x03;

/// 80-byte frozen binary layout for HumanIntentHeader (Little-Endian).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanIntentHeader {
    pub intent_id: DistributedId,        // 16 bytes
    pub session_id: DistributedId,       // 16 bytes
    pub workspace_id: DistributedId,     // 16 bytes
    pub agent_id: DistributedId,         // 16 bytes (0 if human direct)
    pub timestamp_tsc: u64,              // 8 bytes
    pub source_provenance: u16,          // 2 bytes (uids provenance enum)
    pub intent_type: u16,                // 2 bytes
    pub side_effect_class: u8,           // 1 byte
    pub lifecycle_state: u8,             // 1 byte
    pub is_network_dependent: u8,        // 1 byte
    pub _reserved: u8,                   // 1 byte
}

impl Default for HumanIntentHeader {
    fn default() -> Self {
        Self {
            intent_id: DistributedId::new(0, 0),
            session_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            agent_id: DistributedId::new(0, 0),
            timestamp_tsc: 0,
            source_provenance: 0,
            intent_type: INTENT_TYPE_ONE_SHOT,
            side_effect_class: SIDE_EFFECT_CLASS1_READONLY,
            lifecycle_state: INTENT_STATE_SUBMITTED,
            is_network_dependent: 0,
            _reserved: 0,
        }
    }
}

/// 128-byte frozen struct for persistent workflow templates.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowSpec {
    pub workflow_id: DistributedId,     // 16 bytes
    pub workspace_id: DistributedId,    // 16 bytes
    pub owner_session_id: DistributedId,// 16 bytes
    pub max_side_effect_class: u8,      // 1 byte
    pub requires_network: u8,           // 1 byte
    pub step_count: u16,                // 2 bytes
    pub execution_count: u32,           // 4 bytes
    pub template_name: [u8; 32],        // 32 bytes ASCII name
    pub _reserved: [u8; 40],            // 40 bytes padding
}

impl Default for WorkflowSpec {
    fn default() -> Self {
        Self {
            workflow_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            owner_session_id: DistributedId::new(0, 0),
            max_side_effect_class: SIDE_EFFECT_CLASS1_READONLY,
            requires_network: 0,
            step_count: 0,
            execution_count: 0,
            template_name: [0u8; 32],
            _reserved: [0u8; 40],
        }
    }
}
