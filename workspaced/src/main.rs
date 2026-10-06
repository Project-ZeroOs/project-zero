//! ZeroOS - workspaced Workspace & Persistent Context Subsystem Daemon
//!
//! Authoritative Contract: Stage 4D Architecture Specification Rev3 & Phase 4D Implementation Plan Rev3.
#![no_std]
#![no_main]

pub use libzero::{
    error, identity, ipc, persistence, resource, syscall, workspace,
};

use error::ZeroError;
use identity::DistributedIdAllocator;
use ipc::{channel_receive, channel_send, IpcMessage};
use persistence::MemoryPersistenceAuthority;
use resource::DistributedId;
use syscall::{sys_channel_close, sys_exit, sys_yield};
use workspace::{
    ContextEdge, ContextNode, ContextNodeType, WorkspaceControlBlock, WorkspaceState,
    MAX_RESIDENT_EDGES_PER_WORKSPACE, MAX_RESIDENT_NODES_PER_WORKSPACE,
    MAX_WORKLOADS_PER_WORKSPACE, MAX_WORKSPACES, OP_WORKSPACE_ATTACH_WORKLOAD,
    OP_WORKSPACE_ATTACH_WORKLOAD_RESP, OP_WORKSPACE_CLOSE, OP_WORKSPACE_CLOSE_RESP,
    OP_WORKSPACE_CONTEXT_ADD_NODE, OP_WORKSPACE_CONTEXT_ADD_NODE_RESP, OP_WORKSPACE_CONTEXT_QUERY,
    OP_WORKSPACE_CONTEXT_QUERY_RESP, OP_WORKSPACE_CREATE, OP_WORKSPACE_CREATE_RESP,
    OP_WORKSPACE_DELETE, OP_WORKSPACE_DELETE_RESP, OP_WORKSPACE_OPEN, OP_WORKSPACE_OPEN_RESP,
    OP_WORKSPACE_QUERY, OP_WORKSPACE_QUERY_RESP, OP_WORKSPACE_RESUME, OP_WORKSPACE_RESUME_RESP,
    OP_WORKSPACE_SUSPEND, OP_WORKSPACE_SUSPEND_RESP,
};

/// Authoritative Workspace Daemon State.
pub struct WorkspaceDaemon {
    pub node_id: u64,
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub workspaces: [WorkspaceControlBlock; MAX_WORKSPACES],
    pub resident_nodes: [[ContextNode; MAX_RESIDENT_NODES_PER_WORKSPACE]; MAX_WORKSPACES],
    pub resident_edges: [[ContextEdge; MAX_RESIDENT_EDGES_PER_WORKSPACE]; MAX_WORKSPACES],
    pub active_workspace_count: usize,
    pub service_channel: u32,
}

impl WorkspaceDaemon {
    pub fn new(node_id: u64, service_channel: u32) -> Result<Self, ZeroError> {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 200);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 64, persistence)?;

        Ok(Self {
            node_id,
            allocator,
            workspaces: [const { WorkspaceControlBlock {
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                owner_pid: 0,
                generation: 0,
                state: WorkspaceState::Unallocated,
                _pad0: [0; 3],
                capability_envelope_handle: 0,
                active_workload_count: 0,
                _pad1: [0; 3],
                associated_workloads: [DistributedId { node_id: 0, local_seq: 0 }; MAX_WORKLOADS_PER_WORKSPACE],
                root_dir_handle: 0,
                resident_node_count: 0,
                resident_edge_count: 0,
                _padding: [0; 460],
            } }; MAX_WORKSPACES],
            resident_nodes: [[ContextNode {
                node_id: 0,
                node_type: ContextNodeType::Document,
                valid: 0,
                _pad0: [0; 2],
                label: [0; 32],
                resource_handle: 0,
                _padding: [0; 20],
            }; MAX_RESIDENT_NODES_PER_WORKSPACE]; MAX_WORKSPACES],
            resident_edges: [[ContextEdge {
                source_node_id: 0,
                target_node_id: 0,
                edge_type: workspace::ContextEdgeType::References,
                valid: 0,
                _pad0: [0; 2],
                _padding: [0; 20],
            }; MAX_RESIDENT_EDGES_PER_WORKSPACE]; MAX_WORKSPACES],
            active_workspace_count: 0,
            service_channel,
        })
    }

    /// Dispatches incoming IPC requests from clients or system daemons.
    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Enforce I-WS-PROTOCOL-DIRECTION: Response tags cannot be used as incoming request opcodes
        match req.tag {
            OP_WORKSPACE_CREATE_RESP
            | OP_WORKSPACE_OPEN_RESP
            | OP_WORKSPACE_CLOSE_RESP
            | OP_WORKSPACE_QUERY_RESP
            | OP_WORKSPACE_SUSPEND_RESP
            | OP_WORKSPACE_RESUME_RESP
            | OP_WORKSPACE_DELETE_RESP
            | OP_WORKSPACE_ATTACH_WORKLOAD_RESP
            | OP_WORKSPACE_CONTEXT_ADD_NODE_RESP
            | OP_WORKSPACE_CONTEXT_QUERY_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_WORKSPACE_CREATE => self.handle_workspace_create(req),
            OP_WORKSPACE_OPEN => self.handle_workspace_open(req),
            OP_WORKSPACE_CLOSE => self.handle_workspace_close(req),
            OP_WORKSPACE_QUERY => self.handle_workspace_query(req),
            OP_WORKSPACE_SUSPEND => self.handle_workspace_suspend(req),
            OP_WORKSPACE_RESUME => self.handle_workspace_resume(req),
            OP_WORKSPACE_DELETE => self.handle_workspace_delete(req),
            OP_WORKSPACE_ATTACH_WORKLOAD => self.handle_workspace_attach_workload(req),
            OP_WORKSPACE_CONTEXT_ADD_NODE => self.handle_context_add_node(req),
            OP_WORKSPACE_CONTEXT_QUERY => self.handle_context_query(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    /// Handles OP_WORKSPACE_CREATE: allocates WorkspaceId from unified 4B persistence allocator.
    pub fn handle_workspace_create(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CREATE_RESP;

        let slot_idx = match self.find_free_slot() {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let workspace_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let cap_handle = if req.handles_count >= 1 { req.handles[0] } else { 0 };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.workspace_id = workspace_id;
        wcb.owner_pid = req.tag;
        wcb.generation = 1;
        wcb.state = WorkspaceState::Active;
        wcb.capability_envelope_handle = cap_handle;
        wcb.active_workload_count = 0;
        wcb.resident_node_count = 0;
        wcb.resident_edge_count = 0;

        self.active_workspace_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&workspace_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&workspace_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_WORKSPACE_OPEN: opens an existing workspace.
    pub fn handle_workspace_open(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_OPEN_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workloads_or_ws(slot_idx);
        if wcb.state == WorkspaceState::Suspended {
            wcb.state = WorkspaceState::Active;
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = wcb.state as u8;
        resp.payload[5] = wcb.active_workload_count;
        resp.payload_len = 6;
        resp
    }

    /// Handles OP_WORKSPACE_CLOSE: closes/de-activates workspace.
    pub fn handle_workspace_close(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CLOSE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Suspended;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_QUERY: returns workspace status.
    pub fn handle_workspace_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &self.workspaces[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = wcb.state as u8;
        resp.payload[5] = wcb.active_workload_count;
        resp.payload[6..10].copy_from_slice(&wcb.generation.to_le_bytes());
        resp.payload_len = 10;
        resp
    }

    /// Handles OP_WORKSPACE_SUSPEND: suspends workspace operations.
    pub fn handle_workspace_suspend(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_SUSPEND_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Suspended;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_RESUME: resumes suspended workspace operations.
    pub fn handle_workspace_resume(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_RESUME_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Active;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_DELETE: executes 4C/4B teardown cascade & scoped cap revocation.
    pub fn handle_workspace_delete(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_DELETE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Closing;

        // Revoke scoped capability subtree (I-WS-CAP-REVOKE-SCOPED)
        if wcb.capability_envelope_handle != 0 {
            let _ = unsafe { sys_channel_close(wcb.capability_envelope_handle) };
            wcb.capability_envelope_handle = 0;
        }

        wcb.state = WorkspaceState::Reclaiming;
        wcb.state = WorkspaceState::Reclaimed;
        wcb.workspace_id = DistributedId::default();

        if self.active_workspace_count > 0 {
            self.active_workspace_count -= 1;
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_ATTACH_WORKLOAD: attaches a workload ID (up to 32 max).
    pub fn handle_workspace_attach_workload(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_ATTACH_WORKLOAD_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let wl_node_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let wl_local_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let wl_id = DistributedId::new(wl_node_id, wl_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];

        if wcb.active_workload_count as usize >= MAX_WORKLOADS_PER_WORKSPACE {
            resp.payload[0..4].copy_from_slice(&(ZeroError::QuotaExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let count = wcb.active_workload_count as usize;
        wcb.associated_workloads[count] = wl_id;
        wcb.active_workload_count += 1;
        wcb.generation += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_CONTEXT_ADD_NODE: adds a context node to resident LRU cache.
    pub fn handle_context_add_node(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CONTEXT_ADD_NODE_RESP;

        if req.payload_len < 20 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];

        if wcb.resident_node_count as usize >= MAX_RESIDENT_NODES_PER_WORKSPACE {
            // Evict slot 0 LRU or return DimensionLimitExceeded if max persistent bounds
            resp.payload[0..4].copy_from_slice(&(ZeroError::DimensionLimitExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_idx = wcb.resident_node_count as usize;
        let node_id = (node_idx + 1) as u32;

        let node = &mut self.resident_nodes[slot_idx][node_idx];
        node.node_id = node_id;
        node.node_type = match req.payload[16] {
            2 => ContextNodeType::File,
            3 => ContextNodeType::Workload,
            4 => ContextNodeType::AgentSession,
            5 => ContextNodeType::Tool,
            6 => ContextNodeType::ExternalReference,
            _ => ContextNodeType::Document,
        };
        node.valid = 1;

        wcb.resident_node_count += 1;
        wcb.generation += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&node_id.to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    /// Handles OP_WORKSPACE_CONTEXT_QUERY: queries context graph resident nodes.
    pub fn handle_context_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CONTEXT_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &self.workspaces[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&wcb.resident_node_count.to_le_bytes());
        resp.payload[8..12].copy_from_slice(&wcb.resident_edge_count.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    fn workloads_or_ws(&mut self, idx: usize) -> &mut WorkspaceControlBlock {
        &mut self.workspaces[idx]
    }

    fn find_free_slot(&self) -> Option<usize> {
        for (i, wcb) in self.workspaces.iter().enumerate() {
            if wcb.state == WorkspaceState::Unallocated || wcb.state == WorkspaceState::Reclaimed {
                return Some(i);
            }
        }
        None
    }

    fn find_workspace_slot(&self, id: DistributedId) -> Option<usize> {
        for (i, wcb) in self.workspaces.iter().enumerate() {
            if wcb.workspace_id == id && wcb.state != WorkspaceState::Reclaimed && wcb.state != WorkspaceState::Unallocated {
                return Some(i);
            }
        }
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).expect("workspaced initialization must succeed");

    // Event dispatch loop
    let mut msg_buf = IpcMessage::empty();
    loop {
        if channel_receive(daemon.service_channel, true).is_ok() {
            let resp = daemon.dispatch(&msg_buf);
            if msg_buf.handles_count > 0 {
                let _ = channel_send(msg_buf.handles[0], &resp, true);
            }
        }
        sys_yield();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}
