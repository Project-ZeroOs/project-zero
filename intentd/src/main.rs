//! ZeroOS - intentd (Intent Resolution Daemon)
//!
//! Authoritative Contract: Stage 4F Architecture Specification Rev3 & ADR-0030.
//! Implementation Plan: Stage 4F Implementation Plan Rev2.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::fabric::*;
use libzero::identity::DistributedIdAllocator;
use libzero::ipc::IpcMessage;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::resource::DistributedId;
use libzero::syscall::sys_exit;
use libzero::ZeroError;

pub static mut INTENT_TABLE: [IntentDescriptor; MAX_PENDING_INTENTS] =
    [const { IntentDescriptor {
        intent_id: DistributedId { node_id: 0, local_seq: 0 },
        principal_id: DistributedId { node_id: 0, local_seq: 0 },
        workspace_id: DistributedId { node_id: 0, local_seq: 0 },
        state: IntentState::Unallocated,
        ambiguity_flag: 0,
        min_ial_requirement: 1,
        privacy_class: 0,
        _pad0: [0; 4],
        submission_tsc: 0,
        deadline_tsc: 0,
        energy_limit_mwh: 0,
        raw_intent_len: 0,
        raw_intent_payload: [0; 512],
        _padding: [0; 432],
    } }; MAX_PENDING_INTENTS];

pub struct IntentDaemon {
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub active_intent_count: usize,
}

impl IntentDaemon {
    pub fn new(node_id: u64) -> Self {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 600);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 128, persistence).unwrap();
        unsafe {
            INTENT_TABLE = [const { IntentDescriptor {
                intent_id: DistributedId { node_id: 0, local_seq: 0 },
                principal_id: DistributedId { node_id: 0, local_seq: 0 },
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                state: IntentState::Unallocated,
                ambiguity_flag: 0,
                min_ial_requirement: 1,
                privacy_class: 0,
                _pad0: [0; 4],
                submission_tsc: 0,
                deadline_tsc: 0,
                energy_limit_mwh: 0,
                raw_intent_len: 0,
                raw_intent_payload: [0; 512],
                _padding: [0; 432],
            } }; MAX_PENDING_INTENTS];
        }
        Self {
            allocator,
            active_intent_count: 0,
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Enforce protocol direction: Reject response opcodes submitted as incoming requests
        match req.tag {
            OP_INTENT_SUBMIT_RESP
            | OP_INTENT_RESOLVE_RESP
            | OP_INTENT_QUERY_STATE_RESP
            | OP_INTENT_CANCEL_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_INTENT_SUBMIT => self.handle_submit(req),
            OP_INTENT_RESOLVE => self.handle_resolve(req),
            OP_INTENT_QUERY_STATE => self.handle_query_state(req),
            OP_INTENT_CANCEL => self.handle_cancel(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    fn handle_submit(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_INTENT_SUBMIT_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if self.active_intent_count >= MAX_PENDING_INTENTS {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let intent_id = self.allocator.allocate_id().unwrap();
        let desc = unsafe { &mut INTENT_TABLE[self.active_intent_count] };
        desc.intent_id = intent_id;
        
        let ws_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        desc.workspace_id = DistributedId::new(ws_node, ws_seq);

        let p_node = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let p_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        desc.principal_id = DistributedId::new(p_node, p_seq);

        // Ambiguity check: if payload has ambiguity marker flag byte at offset 32 == 1
        if req.payload_len >= 33 && req.payload[32] == 1 {
            desc.state = IntentState::Clarifying;
            desc.ambiguity_flag = 1;
        } else {
            desc.state = IntentState::Resolved;
            desc.ambiguity_flag = 0;
        }

        self.active_intent_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&intent_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&intent_id.local_seq.to_le_bytes());
        resp.payload[20] = desc.state as u8;
        resp.payload_len = 21;
        resp
    }

    fn handle_resolve(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_INTENT_RESOLVE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let seq_id = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let target_id = DistributedId::new(node_id, seq_id);

        let mut found_idx = None;
        unsafe {
            for i in 0..self.active_intent_count {
                if INTENT_TABLE[i].intent_id == target_id {
                    found_idx = Some(i);
                    break;
                }
            }
        }

        match found_idx {
            Some(idx) => {
                let desc = unsafe { &mut INTENT_TABLE[idx] };
                if desc.state == IntentState::Clarifying {
                    // Cannot execute or resolve an ambiguous intent without clarification!
                    resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }

                desc.state = IntentState::Executing;
                let plan_id = self.allocator.allocate_id().unwrap();

                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4..12].copy_from_slice(&plan_id.node_id.to_le_bytes());
                resp.payload[12..20].copy_from_slice(&plan_id.local_seq.to_le_bytes());
                resp.payload_len = 20;
            }
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    fn handle_query_state(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_INTENT_QUERY_STATE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let seq_id = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let target_id = DistributedId::new(node_id, seq_id);

        let mut found_state = None;
        unsafe {
            for i in 0..self.active_intent_count {
                if INTENT_TABLE[i].intent_id == target_id {
                    found_state = Some(INTENT_TABLE[i].state);
                    break;
                }
            }
        }

        match found_state {
            Some(st) => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4] = st as u8;
                resp.payload_len = 5;
            }
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    fn handle_cancel(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_INTENT_CANCEL_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let seq_id = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let target_id = DistributedId::new(node_id, seq_id);

        let mut found_idx = None;
        unsafe {
            for i in 0..self.active_intent_count {
                if INTENT_TABLE[i].intent_id == target_id {
                    found_idx = Some(i);
                    break;
                }
            }
        }

        match found_idx {
            Some(idx) => {
                unsafe {
                    INTENT_TABLE[idx].state = IntentState::Cancelled;
                }
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let daemon = IntentDaemon::new(1);
    let _ = daemon.active_intent_count;
    unsafe { sys_exit(0); }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { sys_exit(1); }
}
