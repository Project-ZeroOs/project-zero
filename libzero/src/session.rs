//! ZeroOS - User Session Substrate Abstractions (`session`)
//!
//! Authoritative Specification: Stage 6A Architecture Specification Rev1 (Frozen) & ADR-0032.
//! Implementation Plan: Stage 6A Implementation Plan Rev2.
//!
//! Invariant `I-SHELL-ORCHESTRATOR-NOT-AUTHORITY`:
//! `shelld` is strictly a human-session orchestrator and spatial UI layout coordinator.
//! It does NOT mint capabilities, amplify rights, or bypass authorization rules.
//! All underlying system operations flow through standard Stage 3H capability handles.

use core::fmt;
use crate::resource::DistributedId;

// ============================================================================
// Stage 6A System IPC Opcodes (Range: 0x0801 - 0x0810)
// ============================================================================

pub const OP_SESSION_CREATE: u64                      = 0x0801;
pub const OP_SESSION_CREATE_RESP: u64                 = 0x0802;
pub const OP_SESSION_DESTROY: u64                     = 0x0803;
pub const OP_SESSION_DESTROY_RESP: u64                = 0x0804;
pub const OP_SESSION_SWITCH_WORKSPACE: u64            = 0x0805;
pub const OP_SESSION_SWITCH_WORKSPACE_RESP: u64       = 0x0806;
pub const OP_SESSION_SET_LAYOUT: u64                  = 0x0807;
pub const OP_SESSION_SET_LAYOUT_RESP: u64             = 0x0808;
pub const OP_SESSION_SUBSCRIBE_TELEMETRY: u64         = 0x0809;
pub const OP_SESSION_SUBSCRIBE_TELEMETRY_RESP: u64    = 0x080A;
pub const OP_SESSION_LOCK: u64                        = 0x080B;
pub const OP_SESSION_LOCK_RESP: u64                   = 0x080C;
pub const OP_SESSION_UNLOCK: u64                      = 0x080D;
pub const OP_SESSION_UNLOCK_RESP: u64                 = 0x080E;
pub const OP_SESSION_QUERY: u64                       = 0x080F;
pub const OP_SESSION_QUERY_RESP: u64                  = 0x0810;
pub const OP_SESSION_QUERY_WORKSPACE_MEMBERSHIP: u64 = 0x0811;
pub const OP_SESSION_QUERY_WORKSPACE_MEMBERSHIP_RESP: u64 = 0x0812;

// ============================================================================
// Static Bounds & Constants
// ============================================================================

pub const MAX_SESSIONS_PER_NODE: usize = 4;
pub const MAX_WORKSPACES_PER_SESSION: usize = 16;
pub const MAX_TELEMETRY_RING_ENTRIES: usize = 8;

pub const SESSION_STATE_UNINITIALIZED: u8 = 0;
pub const SESSION_STATE_RUNNING: u8       = 1;
pub const SESSION_STATE_LOCKED: u8        = 2;
pub const SESSION_STATE_TERMINATED: u8    = 3;

pub const LAYOUT_POLICY_GRID: u8          = 0;
pub const LAYOUT_POLICY_TILING: u8        = 1;
pub const LAYOUT_POLICY_FREEFORM: u8      = 2;

// ============================================================================
// Authoritative Session Record (`SessionRecord`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRecord {
    pub user_id: u64,
    pub session_id: DistributedId,
    pub state: u8,
    pub layout_policy: u8,
    pub active_workspace_count: u8,
    pub _pad0: u8,
    pub active_workspace_id: DistributedId,
    pub authorized_workspaces: [DistributedId; MAX_WORKSPACES_PER_SESSION],
}

impl Default for SessionRecord {
    fn default() -> Self {
        Self {
            user_id: 0,
            session_id: DistributedId::new(0, 0),
            state: SESSION_STATE_UNINITIALIZED,
            layout_policy: LAYOUT_POLICY_GRID,
            active_workspace_count: 0,
            _pad0: 0,
            active_workspace_id: DistributedId::new(0, 0),
            authorized_workspaces: [DistributedId::new(0, 0); MAX_WORKSPACES_PER_SESSION],
        }
    }
}

// ============================================================================
// Agent Activity Telemetry Descriptor (`AgentActivityDescriptor`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentActivityDescriptor {
    /// Agent Monotonic Identity (16 bytes).
    pub agent_id: DistributedId,
    /// Workspace Containment Context (16 bytes).
    pub workspace_id: DistributedId,
    /// Execution State: 0=Idle, 1=Planning, 2=Executing, 3=WaitingAuth, 4=Failed (1 byte).
    pub state: u8,
    /// Execution Progress Percentage: 0 .. 100% (1 byte).
    pub progress_pct: u8,
    /// Explicit Reserved Padding (2 bytes).
    pub _pad0: [u8; 2],
    /// Current Task DAG Step Index (4 bytes).
    pub current_step_index: u32,
    /// Total Planned Task DAG Steps (4 bytes).
    pub total_step_count: u32,
    /// Descriptive Audit Hash of Current Action (32 bytes).
    pub requested_action_hash: [u8; 32],
    /// UTF-8 Human Status Summary Line (64 bytes).
    pub human_status_summary: [u8; 64],
}

impl Default for AgentActivityDescriptor {
    fn default() -> Self {
        Self {
            agent_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            state: 0,
            progress_pct: 0,
            _pad0: [0; 2],
            current_step_index: 0,
            total_step_count: 0,
            requested_action_hash: [0; 32],
            human_status_summary: [0; 64],
        }
    }
}

impl fmt::Display for SessionRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Session({}: User {}, State {}, Workspaces {})",
            self.session_id.local_seq, self.user_id, self.state, self.active_workspace_count
        )
    }
}
