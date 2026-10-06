//! ZeroOS - libzero Workspace & Persistent Context Data Structures & Protocol
//!
//! Authoritative Contract: Stage 4D Architecture Specification Rev3 & Phase 4D Implementation Plan Rev3.

use crate::error::ZeroError;
use crate::resource::DistributedId;

// Protocol OpCodes for workspaced (registered with brokerd as "workspace.service")
pub const OP_WORKSPACE_CREATE:               u64 = 0x4D01;
pub const OP_WORKSPACE_CREATE_RESP:          u64 = 0x4D02;
pub const OP_WORKSPACE_OPEN:                 u64 = 0x4D03;
pub const OP_WORKSPACE_OPEN_RESP:            u64 = 0x4D04;
pub const OP_WORKSPACE_CLOSE:                u64 = 0x4D05;
pub const OP_WORKSPACE_CLOSE_RESP:           u64 = 0x4D06;
pub const OP_WORKSPACE_QUERY:                u64 = 0x4D07;
pub const OP_WORKSPACE_QUERY_RESP:           u64 = 0x4D08;
pub const OP_WORKSPACE_SUSPEND:              u64 = 0x4D09;
pub const OP_WORKSPACE_SUSPEND_RESP:         u64 = 0x4D0A;
pub const OP_WORKSPACE_RESUME:               u64 = 0x4D0B;
pub const OP_WORKSPACE_RESUME_RESP:          u64 = 0x4D0C;
pub const OP_WORKSPACE_DELETE:               u64 = 0x4D0D;
pub const OP_WORKSPACE_DELETE_RESP:          u64 = 0x4D0E;
pub const OP_WORKSPACE_ATTACH_WORKLOAD:      u64 = 0x4D0F;
pub const OP_WORKSPACE_ATTACH_WORKLOAD_RESP: u64 = 0x4D10;
pub const OP_WORKSPACE_CONTEXT_ADD_NODE:     u64 = 0x4D11;
pub const OP_WORKSPACE_CONTEXT_ADD_NODE_RESP:u64 = 0x4D12;
pub const OP_WORKSPACE_CONTEXT_QUERY:        u64 = 0x4D13;
pub const OP_WORKSPACE_CONTEXT_QUERY_RESP:   u64 = 0x4D14;

pub const MAX_WORKSPACES: usize = 32;
pub const MAX_WORKLOADS_PER_WORKSPACE: usize = 32;
pub const MAX_RESIDENT_NODES_PER_WORKSPACE: usize = 256;
pub const MAX_RESIDENT_EDGES_PER_WORKSPACE: usize = 1024;
pub const MAX_PERSISTENT_NODES_PER_WORKSPACE: usize = 4096;
pub const MAX_PERSISTENT_EDGES_PER_WORKSPACE: usize = 16384;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceState {
    Unallocated = 0,
    Creating = 1,
    Active = 2,
    Suspending = 3,
    Suspended = 4,
    Resuming = 5,
    Closing = 6,
    Reclaiming = 7,
    Reclaimed = 8,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self::Unallocated
    }
}

/// 1024-byte Workspace Control Block.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WorkspaceControlBlock {
    pub workspace_id: DistributedId,
    pub owner_pid: u64,
    pub generation: u32,
    pub state: WorkspaceState,
    pub _pad0: [u8; 3],
    pub capability_envelope_handle: u32,
    pub active_workload_count: u8,
    pub _pad1: [u8; 3],
    pub associated_workloads: [DistributedId; MAX_WORKLOADS_PER_WORKSPACE],
    pub root_dir_handle: u32,
    pub resident_node_count: u32,
    pub resident_edge_count: u32,
    pub _padding: [u8; 460],
}

impl Default for WorkspaceControlBlock {
    fn default() -> Self {
        Self {
            workspace_id: DistributedId::default(),
            owner_pid: 0,
            generation: 0,
            state: WorkspaceState::Unallocated,
            _pad0: [0; 3],
            capability_envelope_handle: 0,
            active_workload_count: 0,
            _pad1: [0; 3],
            associated_workloads: [DistributedId::default(); MAX_WORKLOADS_PER_WORKSPACE],
            root_dir_handle: 0,
            resident_node_count: 0,
            resident_edge_count: 0,
            _padding: [0; 460],
        }
    }
}

const _: () = assert!(core::mem::size_of::<WorkspaceControlBlock>() == 1024);

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ContextNodeType {
    Document = 1,
    File = 2,
    Workload = 3,
    AgentSession = 4,
    Tool = 5,
    ExternalReference = 6,
}

impl Default for ContextNodeType {
    fn default() -> Self {
        Self::Document
    }
}

/// 64-byte Context Graph Node Record.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextNode {
    pub node_id: u32,
    pub node_type: ContextNodeType,
    pub valid: u8,
    pub _pad0: [u8; 2],
    pub label: [u8; 32],
    pub resource_handle: u32,
    pub _padding: [u8; 20],
}

const _: () = assert!(core::mem::size_of::<ContextNode>() == 64);

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ContextEdgeType {
    References = 1,
    GeneratedBy = 2,
    DependsOn = 3,
    DerivedFrom = 4,
    AttachedTo = 5,
    AuthoredBy = 6,
}

impl Default for ContextEdgeType {
    fn default() -> Self {
        Self::References
    }
}

/// 32-byte Context Graph Edge Record.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextEdge {
    pub source_node_id: u32,
    pub target_node_id: u32,
    pub edge_type: ContextEdgeType,
    pub valid: u8,
    pub _pad0: [u8; 2],
    pub _padding: [u8; 20],
}

const _: () = assert!(core::mem::size_of::<ContextEdge>() == 32);

// Protocol Payload Structures

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WorkspaceCreateReq {
    pub owner_pid: u64,
    pub cap_handle: u32,
    pub _padding: [u8; 36],
}

impl Default for WorkspaceCreateReq {
    fn default() -> Self {
        Self { owner_pid: 0, cap_handle: 0, _padding: [0; 36] }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceCreateResp {
    pub status: i32,
    pub workspace_id: DistributedId,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceOpenReq {
    pub workspace_id: DistributedId,
    pub cap_handle: u32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceOpenResp {
    pub status: i32,
    pub state: WorkspaceState,
    pub active_workload_count: u8,
    pub _padding: [u8; 14],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceCloseReq {
    pub workspace_id: DistributedId,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceCloseResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceQueryReq {
    pub workspace_id: DistributedId,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceQueryResp {
    pub status: i32,
    pub state: WorkspaceState,
    pub active_workloads: u8,
    pub generation: u32,
    pub _padding: [u8; 18],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceDeleteReq {
    pub workspace_id: DistributedId,
    pub cap_handle: u32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceDeleteResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceAttachReq {
    pub workspace_id: DistributedId,
    pub workload_id: DistributedId,
    pub cap_handle: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceAttachResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ContextAddNodeReq {
    pub workspace_id: DistributedId,
    pub node_type: ContextNodeType,
    pub _pad0: [u8; 3],
    pub label: [u8; 32],
    pub resource_handle: u32,
    pub _padding: [u8; 32],
}

impl Default for ContextAddNodeReq {
    fn default() -> Self {
        Self {
            workspace_id: DistributedId::default(),
            node_type: ContextNodeType::Document,
            _pad0: [0; 3],
            label: [0; 32],
            resource_handle: 0,
            _padding: [0; 32],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct ContextAddNodeResp {
    pub status: i32,
    pub node_id: u32,
    pub _padding: [u8; 4],
}
