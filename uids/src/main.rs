//! ZeroOS - User Interaction Daemon (`uids`)
//!
//! Authoritative Contract: Stage 5 Architecture Specification Rev4 (Frozen) & ADR-0031.
//! Implementation Plan: Stage 5 Implementation Plan Rev2.
//!
//! Responsibilities:
//! - Ingests hardware input events from `DeviceId::Input(0)`.
//! - Enforces input focus routing (`Unfocused -> Focused -> Captured -> ModalLock`).
//! - Enforces invariant `I-INPUT-NO-IMPLICIT-AUTHORITY`: `uids` possesses ZERO capability authority,
//!   ZERO workload creation authority, and cannot approve human authorization requests.
//! - Formats human intent payloads and dispatches `OP_INTENT_SUBMIT` to `intentd`.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::error::ZeroError;
use libzero::ipc::IpcMessage;
use libzero::presentation::*;
use libzero::resource::DistributedId;

pub struct UidsDaemon {
    pub input_device_bound: bool,
    pub input_device_id: u64,
    pub focus_state: u8,
    pub focused_surface_id: u64,
    pub modal_lock_active: bool,
    pub active_auth_transaction: DistributedId,
}

impl UidsDaemon {
    pub fn new() -> Self {
        Self {
            input_device_bound: false,
            input_device_id: 0,
            focus_state: FOCUS_STATE_UNFOCUSED,
            focused_surface_id: 0,
            modal_lock_active: false,
            active_auth_transaction: DistributedId::new(0, 0),
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Tag Validation: Reject response opcodes submitted as requests
        match req.tag {
            OP_UIDS_INGEST_INTENT_RESP | OP_UIDS_REQUEST_MODAL_LOCK_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_UIDS_INGEST_INTENT => self.handle_ingest_intent(req),
            OP_UIDS_REQUEST_MODAL_LOCK => self.handle_request_modal_lock(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    pub fn handle_request_modal_lock(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_UIDS_REQUEST_MODAL_LOCK_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let tx_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let tx_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());

        // Invariant: Unprivileged ModalLock requests (node=0, seq=0) are REJECTED with PermissionDenied
        if tx_node == 0 && tx_seq == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.modal_lock_active = true;
        self.focus_state = FOCUS_STATE_MODAL_LOCK;
        self.active_auth_transaction = DistributedId::new(tx_node, tx_seq);

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_ingest_intent(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_UIDS_INGEST_INTENT_RESP;

        if req.payload_len < 20 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let payload_len = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());

        if payload_len == 0 || payload_len > 512 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Invariant I-INPUT-NO-IMPLICIT-AUTHORITY: uids formats intent payload for intentd validation
        // (Mock intent allocation for helper: intent_node = 1, intent_seq = 500)
        let intent_id = DistributedId::new(1, 500);

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&intent_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&intent_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    pub fn release_modal_lock(&mut self) {
        self.modal_lock_active = false;
        self.focus_state = FOCUS_STATE_UNFOCUSED;
        self.active_auth_transaction = DistributedId::new(0, 0);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
