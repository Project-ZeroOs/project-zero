//! ZeroOS - libzero Workspace & Persistent Context Data Structures & Protocol
//!
//! Authoritative Contract: Stage 4D Architecture Specification Rev3 & Phase 4D Implementation Plan Rev3.

use crate::resource::DistributedId;

pub type WorkspaceId = DistributedId;


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
pub const OP_OBJECT_REGISTER:                u64 = 0x4D15;
pub const OP_OBJECT_REGISTER_RESP:           u64 = 0x4D16;
pub const OP_OBJECT_LOOKUP:                  u64 = 0x4D17;
pub const OP_OBJECT_LOOKUP_RESP:             u64 = 0x4D18;
pub const OP_OBJECT_RENAME:                  u64 = 0x4D19;
pub const OP_OBJECT_RENAME_RESP:             u64 = 0x4D1A;
pub const OP_OBJECT_UNLINK:                  u64 = 0x4D1B;
pub const OP_OBJECT_UNLINK_RESP:             u64 = 0x4D1C;
pub const OP_OBJECT_RECONCILE:               u64 = 0x4D1D;
pub const OP_OBJECT_RECONCILE_RESP:          u64 = 0x4D1E;

// REV1 Workspace Semantic Model OpCodes
pub const OP_WORKSPACE_ADD_MEMBER:          u64 = 0x4D21;
pub const OP_WORKSPACE_ADD_MEMBER_RESP:     u64 = 0x4D22;
pub const OP_WORKSPACE_REMOVE_MEMBER:       u64 = 0x4D23;
pub const OP_WORKSPACE_REMOVE_MEMBER_RESP:  u64 = 0x4D24;
pub const OP_WORKSPACE_QUERY_MEMBERS:       u64 = 0x4D25;
pub const OP_WORKSPACE_QUERY_MEMBERS_RESP:  u64 = 0x4D26;
pub const OP_WORKSPACE_ATTACH_AGENT:        u64 = 0x4D27;
pub const OP_WORKSPACE_ATTACH_AGENT_RESP:   u64 = 0x4D28;
pub const OP_WORKSPACE_DETACH_AGENT:        u64 = 0x4D29;
pub const OP_WORKSPACE_DETACH_AGENT_RESP:   u64 = 0x4D2A;
pub const OP_WORKSPACE_INTENT_ADD_NODE:     u64 = 0x4D2B;
pub const OP_WORKSPACE_INTENT_ADD_NODE_RESP:u64 = 0x4D2C;
pub const OP_WORKSPACE_INTENT_ADD_DEP:      u64 = 0x4D2D;
pub const OP_WORKSPACE_INTENT_ADD_DEP_RESP: u64 = 0x4D2E;
pub const OP_WORKSPACE_INTENT_QUERY:        u64 = 0x4D2F;
pub const OP_WORKSPACE_INTENT_QUERY_RESP:   u64 = 0x4D30;
pub const OP_WORKSPACE_ARCHIVE:             u64 = 0x4D31;
pub const OP_WORKSPACE_ARCHIVE_RESP:        u64 = 0x4D32;

pub const MAX_WORKSPACES: usize = 32;
pub const MAX_WORKLOADS_PER_WORKSPACE: usize = 32;
pub const MAX_AGENTS_PER_WORKSPACE: usize = 16;
pub const MAX_INTENT_NODES_PER_WORKSPACE: usize = 32;
pub const MAX_INTENT_DEPS_PER_WORKSPACE: usize = 64;
pub const MAX_MEMBERSHIPS_PER_WORKSPACE: usize = 64;
pub const MAX_TOTAL_MEMBERSHIP_EDGES: usize = 256;
pub const MAX_RESIDENT_NODES_PER_WORKSPACE: usize = 256;
pub const MAX_RESIDENT_EDGES_PER_WORKSPACE: usize = 1024;
pub const MAX_PERSISTENT_NODES_PER_WORKSPACE: usize = 4096;
pub const MAX_PERSISTENT_EDGES_PER_WORKSPACE: usize = 16384;
pub const MAX_OBJECT_REGISTRY_ENTRIES: usize = 256;

pub const WS_SYSTEM_0_NODE_ID: u64 = 0;
pub const WS_SYSTEM_0_LOCAL_SEQ: u64 = 0;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceState {
    Unallocated = 0,
    Creating = 1,
    Initializing = 2,
    Active = 3,
    Suspending = 4,
    Suspended = 5,
    Resuming = 6,
    Archived = 7,
    Closing = 8,
    Reclaiming = 9,
    Reclaimed = 10,
    Destroyed = 11,
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
    pub active_agent_count: u8,
    pub membership_count: u16,
    pub associated_workloads: [DistributedId; MAX_WORKLOADS_PER_WORKSPACE],
    pub associated_agents: [DistributedId; MAX_AGENTS_PER_WORKSPACE],
    pub intent_node_count: u32,
    pub intent_dep_count: u32,
    pub root_dir_handle: u32,
    pub resident_node_count: u32,
    pub resident_edge_count: u32,
    pub _padding: [u8; 196],
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
            active_agent_count: 0,
            membership_count: 0,
            associated_workloads: [DistributedId::default(); MAX_WORKLOADS_PER_WORKSPACE],
            associated_agents: [DistributedId::default(); MAX_AGENTS_PER_WORKSPACE],
            intent_node_count: 0,
            intent_dep_count: 0,
            root_dir_handle: 0,
            resident_node_count: 0,
            resident_edge_count: 0,
            _padding: [0; 196],
        }
    }
}

const _: () = assert!(core::mem::size_of::<WorkspaceControlBlock>() == 1024);

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MembershipEdgeState {
    Unallocated = 0,
    Live = 1,
    StaleUnresolved = 2,
}

impl Default for MembershipEdgeState {
    fn default() -> Self {
        Self::Unallocated
    }
}

/// 48-byte Membership Edge Record: MEMBER_OF(WorkspaceId, ObjectId)
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceMembershipEdge {
    pub workspace_id: DistributedId,
    pub object_id: DistributedId,
    pub state: MembershipEdgeState,
    pub _pad0: [u8; 3],
    pub _padding: [u8; 12],
}

const _: () = assert!(core::mem::size_of::<WorkspaceMembershipEdge>() == 48);

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AgentBindingState {
    Unbound = 0,
    Active = 1,
    Quiesced = 2,
    UnboundArchived = 3,
}

impl Default for AgentBindingState {
    fn default() -> Self {
        Self::Unbound
    }
}

/// 48-byte Workspace-Agent Binding Record.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceAgentBinding {
    pub workspace_id: DistributedId,
    pub agent_id: DistributedId,
    pub state: AgentBindingState,
    pub _pad0: [u8; 3],
    pub _padding: [u8; 12],
}

const _: () = assert!(core::mem::size_of::<WorkspaceAgentBinding>() == 48);

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IntentNodeState {
    Pending = 1,
    InProgress = 2,
    Completed = 3,
    Failed = 4,
}

impl Default for IntentNodeState {
    fn default() -> Self {
        Self::Pending
    }
}

/// 64-byte Human Intent DAG Node Record.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IntentNode {
    pub node_id: u32,
    pub state: IntentNodeState,
    pub priority: u8,
    pub task_type: u8,
    pub _pad0: u8,
    pub label: [u8; 32],
    pub resource_handle: u32,
    pub _padding: [u8; 20],
}

const _: () = assert!(core::mem::size_of::<IntentNode>() == 64);

/// 32-byte Human Intent DAG Dependency Record.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IntentDependency {
    pub parent_node_id: u32,
    pub child_node_id: u32,
    pub valid: u8,
    pub _pad0: [u8; 3],
    pub _padding: [u8; 20],
}

const _: () = assert!(core::mem::size_of::<IntentDependency>() == 32);

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

// REV1 Workspace Semantic Model Payload Structures

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceAddMemberReq {
    pub workspace_id: DistributedId,
    pub object_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceAddMemberResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceRemoveMemberReq {
    pub workspace_id: DistributedId,
    pub object_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceRemoveMemberResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceQueryMembersReq {
    pub workspace_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceQueryMembersResp {
    pub status: i32,
    pub member_count: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceAttachAgentReq {
    pub workspace_id: DistributedId,
    pub agent_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceAttachAgentResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceDetachAgentReq {
    pub workspace_id: DistributedId,
    pub agent_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceDetachAgentResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WorkspaceIntentAddNodeReq {
    pub workspace_id: DistributedId,
    pub priority: u8,
    pub task_type: u8,
    pub _pad0: [u8; 2],
    pub label: [u8; 32],
}

impl Default for WorkspaceIntentAddNodeReq {
    fn default() -> Self {
        Self {
            workspace_id: DistributedId::default(),
            priority: 1,
            task_type: 1,
            _pad0: [0; 2],
            label: [0; 32],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceIntentAddNodeResp {
    pub status: i32,
    pub node_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceIntentAddDepReq {
    pub workspace_id: DistributedId,
    pub parent_node_id: u32,
    pub child_node_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceIntentAddDepResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceIntentQueryReq {
    pub workspace_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceIntentQueryResp {
    pub status: i32,
    pub node_count: u32,
    pub dep_count: u32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceArchiveReq {
    pub workspace_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct WorkspaceArchiveResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ObjectState {
    Unallocated = 0,
    Live = 1,
    Tombstoned = 2,
}

impl Default for ObjectState {
    fn default() -> Self {
        Self::Unallocated
    }
}

/// 160-byte REV8 Object ID Registry Record layout.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ObjectIdRegistryRecord {
    pub object_id: DistributedId,
    pub device_id: u32,
    pub inode_num: u32,
    pub state: ObjectState,
    pub name_len: u8,
    pub _pad0: [u8; 2],
    pub name: [u8; 128],
    pub _padding: [u8; 4],
}

impl Default for ObjectIdRegistryRecord {
    fn default() -> Self {
        Self {
            object_id: DistributedId::default(),
            device_id: 0,
            inode_num: 0,
            state: ObjectState::Unallocated,
            name_len: 0,
            _pad0: [0; 2],
            name: [0; 128],
            _padding: [0; 4],
        }
    }
}

const _: () = assert!(core::mem::size_of::<ObjectIdRegistryRecord>() == 160);

pub const OBJECT_REGISTRY_PATH: &str = "/storage/system/object_id.registry";
pub const OBJECT_REGISTRY_MAGIC: &[u8; 8] = b"ZERO_REG";
pub const OBJECT_REGISTRY_VERSION: u32 = 1;

/// 64-byte Header layout for /storage/system/object_id.registry
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ObjectIdRegistryHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub record_count: u32,
    pub record_size: u32,
    pub checksum: u32,
    pub _reserved: [u8; 40],
}

impl Default for ObjectIdRegistryHeader {
    fn default() -> Self {
        Self {
            magic: *OBJECT_REGISTRY_MAGIC,
            version: OBJECT_REGISTRY_VERSION,
            record_count: 0,
            record_size: 160,
            checksum: 0,
            _reserved: [0; 40],
        }
    }
}

const _: () = assert!(core::mem::size_of::<ObjectIdRegistryHeader>() == 64);

impl ObjectIdRegistryHeader {
    pub fn serialize(&self, out: &mut [u8; 64]) {
        out[0..8].copy_from_slice(&self.magic);
        out[8..12].copy_from_slice(&self.version.to_le_bytes());
        out[12..16].copy_from_slice(&self.record_count.to_le_bytes());
        out[16..20].copy_from_slice(&self.record_size.to_le_bytes());
        out[20..24].copy_from_slice(&self.checksum.to_le_bytes());
        out[24..64].fill(0);
    }

    pub fn deserialize(src: &[u8; 64]) -> Result<Self, crate::error::ZeroError> {
        if &src[0..8] != OBJECT_REGISTRY_MAGIC {
            return Err(crate::error::ZeroError::InvalidRequest);
        }
        let version = u32::from_le_bytes(src[8..12].try_into().unwrap());
        if version != OBJECT_REGISTRY_VERSION {
            return Err(crate::error::ZeroError::GenerationMismatch);
        }
        let record_count = u32::from_le_bytes(src[12..16].try_into().unwrap());
        let record_size = u32::from_le_bytes(src[16..20].try_into().unwrap());
        if record_size != 160 {
            return Err(crate::error::ZeroError::InvalidRequest);
        }
        let checksum = u32::from_le_bytes(src[20..24].try_into().unwrap());
        Ok(Self {
            magic: *OBJECT_REGISTRY_MAGIC,
            version,
            record_count,
            record_size,
            checksum,
            _reserved: [0; 40],
        })
    }
}

impl ObjectIdRegistryRecord {
    pub fn serialize(&self, out: &mut [u8; 160]) {
        out[0..8].copy_from_slice(&self.object_id.node_id.to_le_bytes());
        out[8..16].copy_from_slice(&self.object_id.local_seq.to_le_bytes());
        out[16..20].copy_from_slice(&self.device_id.to_le_bytes());
        out[20..24].copy_from_slice(&self.inode_num.to_le_bytes());
        out[24] = self.state as u8;
        out[25] = self.name_len;
        out[26..28].fill(0);
        let nlen = (self.name_len as usize).min(128);
        out[28..28 + nlen].copy_from_slice(&self.name[0..nlen]);
        if nlen < 128 {
            out[28 + nlen..156].fill(0);
        }
        out[156..160].fill(0);
    }

    pub fn deserialize(src: &[u8; 160]) -> Result<Self, crate::error::ZeroError> {
        let node_id = u64::from_le_bytes(src[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(src[8..16].try_into().unwrap());
        let device_id = u32::from_le_bytes(src[16..20].try_into().unwrap());
        let inode_num = u32::from_le_bytes(src[20..24].try_into().unwrap());
        let state_byte = src[24];
        let state = match state_byte {
            1 => ObjectState::Live,
            2 => ObjectState::Tombstoned,
            _ => ObjectState::Unallocated,
        };
        let name_len = src[25];
        let mut name = [0u8; 128];
        let nlen = (name_len as usize).min(128);
        name[0..nlen].copy_from_slice(&src[28..28 + nlen]);

        Ok(Self {
            object_id: DistributedId::new(node_id, local_seq),
            device_id,
            inode_num,
            state,
            name_len,
            _pad0: [0; 2],
            name,
            _padding: [0; 4],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_header_serialization_roundtrip() {
        let hdr = ObjectIdRegistryHeader {
            magic: *OBJECT_REGISTRY_MAGIC,
            version: OBJECT_REGISTRY_VERSION,
            record_count: 42,
            record_size: 160,
            checksum: 0x1234_5678,
            _reserved: [0; 40],
        };
        let mut buf = [0u8; 64];
        hdr.serialize(&mut buf);
        let deserialized = ObjectIdRegistryHeader::deserialize(&buf).expect("header deserialization must succeed");
        assert_eq!(hdr, deserialized);
    }

    #[test]
    fn test_registry_record_serialization_roundtrip() {
        let mut name = [0u8; 128];
        name[0..8].copy_from_slice(b"test.txt");
        let rec = ObjectIdRegistryRecord {
            object_id: DistributedId::new(1, 100),
            device_id: 1,
            inode_num: 4096,
            state: ObjectState::Live,
            name_len: 8,
            _pad0: [0; 2],
            name,
            _padding: [0; 4],
        };
        let mut buf = [0u8; 160];
        rec.serialize(&mut buf);
        let deserialized = ObjectIdRegistryRecord::deserialize(&buf).expect("record deserialization must succeed");
        assert_eq!(rec.object_id, deserialized.object_id);
        assert_eq!(rec.device_id, deserialized.device_id);
        assert_eq!(rec.inode_num, deserialized.inode_num);
        assert_eq!(rec.state, deserialized.state);
        assert_eq!(rec.name_len, deserialized.name_len);
        assert_eq!(&rec.name[0..8], &deserialized.name[0..8]);
    }
}

