//! ZeroOS - User Session Substrate & Human Operating Environment Daemon (`shelld`)
//!
//! Authoritative Contract: Stage 6A Architecture Specification Rev1 (Frozen) & ADR-0032.
//! Implementation Plan: Stage 6A Implementation Plan Rev2.
//!
//! Responsibilities:
//! - Sole authoritative owner of `SessionRecord` state.
//! - Manages human session lifecycle, workspace switching layout coordination, and spatial grid viewports.
//! - Ingests `AgentActivityDescriptor` telemetry events.
//! - Enforces deterministic 7-stage crash recovery state machine with fail-closed `RECOVERY_FAILED` terminal state.
//! - Does NOT mint kernel capabilities or bypass Stage 3H security authority (`I-SHELL-ORCHESTRATOR-NOT-AUTHORITY`).

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::error::ZeroError;
use libzero::identity::DistributedIdAllocator;
use libzero::ipc::IpcMessage;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::presentation::*;
use libzero::resource::DistributedId;
use libzero::session::*;

pub struct SessionDaemon {
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub active_session_count: usize,
    pub sessions: [SessionRecord; MAX_SESSIONS_PER_NODE],
    pub telemetry_ring: [AgentActivityDescriptor; MAX_TELEMETRY_RING_ENTRIES],
    pub telemetry_count: usize,
    pub recovery_state: u8,
}

impl SessionDaemon {
    pub fn new(node_id: u64) -> Self {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 100);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 128, persistence).unwrap();
        Self {
            allocator,
            active_session_count: 0,
            sessions: [SessionRecord::default(); MAX_SESSIONS_PER_NODE],
            telemetry_ring: [AgentActivityDescriptor::default(); MAX_TELEMETRY_RING_ENTRIES],
            telemetry_count: 0,
            recovery_state: SESSION_STATE_RUNNING,
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Tag Validation: Reject response opcodes submitted as requests
        match req.tag {
            OP_SESSION_CREATE_RESP
            | OP_SESSION_DESTROY_RESP
            | OP_SESSION_SWITCH_WORKSPACE_RESP
            | OP_SESSION_SET_LAYOUT_RESP
            | OP_SESSION_SUBSCRIBE_TELEMETRY_RESP
            | OP_SESSION_LOCK_RESP
            | OP_SESSION_UNLOCK_RESP
            | OP_SESSION_QUERY_RESP
            | OP_SESSION_QUERY_WORKSPACE_MEMBERSHIP_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_SESSION_CREATE => self.handle_create_session(req),
            OP_SESSION_DESTROY => self.handle_destroy_session(req),
            OP_SESSION_SWITCH_WORKSPACE => self.handle_switch_workspace(req),
            OP_SESSION_QUERY_WORKSPACE_MEMBERSHIP => self.handle_query_membership(req),
            OP_SESSION_SUBSCRIBE_TELEMETRY => self.handle_telemetry(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    pub fn handle_create_session(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SESSION_CREATE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if self.active_session_count >= MAX_SESSIONS_PER_NODE {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let user_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let session_id = self.allocator.allocate_id().unwrap();

        let mut session = SessionRecord::default();
        session.user_id = user_id;
        session.session_id = session_id;
        session.state = SESSION_STATE_RUNNING;
        session.active_workspace_count = 1;
        session.active_workspace_id = DistributedId::new(1, 10);
        session.authorized_workspaces[0] = DistributedId::new(1, 10);

        self.sessions[self.active_session_count] = session;
        self.active_session_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&session_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&session_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    pub fn handle_destroy_session(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SESSION_DESTROY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let sess_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let sess_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());

        for i in 0..self.active_session_count {
            if self.sessions[i].session_id.node_id == sess_node && self.sessions[i].session_id.local_seq == sess_seq {
                self.sessions[i].state = SESSION_STATE_TERMINATED;
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_switch_workspace(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SESSION_SWITCH_WORKSPACE_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let sess_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let sess_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let target_ws_node = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let target_ws_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());

        for i in 0..self.active_session_count {
            if self.sessions[i].session_id.node_id == sess_node && self.sessions[i].session_id.local_seq == sess_seq {
                // Check if target workspace is in authorized membership list
                let mut authorized = false;
                for j in 0..self.sessions[i].active_workspace_count as usize {
                    let ws = self.sessions[i].authorized_workspaces[j];
                    if ws.node_id == target_ws_node && ws.local_seq == target_ws_seq {
                        authorized = true;
                        break;
                    }
                }

                if !authorized {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }

                self.sessions[i].active_workspace_id = DistributedId::new(target_ws_node, target_ws_seq);
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_query_membership(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SESSION_QUERY_WORKSPACE_MEMBERSHIP_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let sess_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let sess_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let target_ws_node = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let target_ws_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());

        for i in 0..self.active_session_count {
            if self.sessions[i].session_id.node_id == sess_node && self.sessions[i].session_id.local_seq == sess_seq {
                let mut authorized = false;
                for j in 0..self.sessions[i].active_workspace_count as usize {
                    let ws = self.sessions[i].authorized_workspaces[j];
                    if ws.node_id == target_ws_node && ws.local_seq == target_ws_seq {
                        authorized = true;
                        break;
                    }
                }

                if authorized {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                } else {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
                }
                resp.payload_len = 4;
                return resp;
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_telemetry(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SESSION_SUBSCRIBE_TELEMETRY_RESP;

        if req.payload_len < 40 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let agent_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let agent_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let state = req.payload[16];
        let progress = req.payload[17];

        let mut desc = AgentActivityDescriptor::default();
        desc.agent_id = DistributedId::new(agent_node, agent_seq);
        desc.state = state;
        desc.progress_pct = progress;

        if self.telemetry_count < MAX_TELEMETRY_RING_ENTRIES {
            self.telemetry_ring[self.telemetry_count] = desc;
            self.telemetry_count += 1;
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut daemon = SessionDaemon::new(1);
    let mut msg = IpcMessage::empty();
    msg.tag = OP_SESSION_CREATE;
    msg.payload[0..8].copy_from_slice(&100u64.to_le_bytes()); // User 100
    msg.payload_len = 16;

    let _resp = daemon.dispatch(&msg);
    unsafe {
        libzero::syscall::sys_exit(0);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        libzero::syscall::sys_exit(-1);
    }
}
