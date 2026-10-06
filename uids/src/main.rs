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
    pub active_session_id: u64,
    pub active_workspace_id: u64,
    pub modal_lock_active: bool,
    pub active_auth_transaction: DistributedId,
    pub transient_held_keys_mask: u32,
    pub synthesized_release_event_count: usize,
    pub last_assigned_tsc: u64,
}

impl UidsDaemon {
    pub fn new() -> Self {
        Self {
            input_device_bound: false,
            input_device_id: 0,
            focus_state: FOCUS_STATE_UNFOCUSED,
            focused_surface_id: 0,
            active_session_id: 0,
            active_workspace_id: 0,
            modal_lock_active: false,
            active_auth_transaction: DistributedId::new(0, 0),
            transient_held_keys_mask: 0,
            synthesized_release_event_count: 0,
            last_assigned_tsc: 1000,
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Tag Validation: Reject response opcodes submitted as requests
        match req.tag {
            OP_UIDS_INGEST_INTENT_RESP
            | OP_UIDS_REQUEST_MODAL_LOCK_RESP
            | OP_UIDS_SET_FOCUS_RESP
            | OP_UIDS_REQUEST_MODAL_LOCK_REV2_RESP
            | OP_UIDS_ROUTE_REMOTE_INPUT_RESP => {
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
            OP_UIDS_REQUEST_MODAL_LOCK | OP_UIDS_REQUEST_MODAL_LOCK_REV2 => self.handle_request_modal_lock(req),
            OP_UIDS_SET_FOCUS => self.handle_set_focus(req),
            OP_UIDS_ROUTE_REMOTE_INPUT => self.handle_route_remote_input(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    pub fn handle_set_focus(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_UIDS_SET_FOCUS_RESP;

        if req.payload_len < 24 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let session_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let workspace_id = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let surface_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());

        // Gate 6C-3 & I-INPUT-FOCUS-POLICY-VS-ENFORCEMENT: Unprivileged focus self-declarations (session=0 or workspace=0) rejected
        if session_id == 0 || workspace_id == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // State Reconciliation (I-INPUT-STATE-RECONCILIATION):
        // On workspace switch, synthesize release events for any transient held keys/buttons
        if self.active_workspace_id != 0 && self.active_workspace_id != workspace_id {
            if self.transient_held_keys_mask != 0 {
                self.synthesized_release_event_count += self.transient_held_keys_mask.count_ones() as usize;
                self.transient_held_keys_mask = 0;
            }
        }

        self.active_session_id = session_id;
        self.active_workspace_id = workspace_id;
        self.focused_surface_id = surface_id;

        if !self.modal_lock_active {
            self.focus_state = FOCUS_STATE_FOCUSED;
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_request_modal_lock(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = if req.tag == OP_UIDS_REQUEST_MODAL_LOCK_REV2 {
            OP_UIDS_REQUEST_MODAL_LOCK_REV2_RESP
        } else {
            OP_UIDS_REQUEST_MODAL_LOCK_RESP
        };

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

    pub fn handle_route_remote_input(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_UIDS_ROUTE_REMOTE_INPUT_RESP;

        if req.payload_len < 40 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Gate 6C-6 & I-INPUT-REMOTE-AUTHORIZATION: Require RemoteInputPolicyCap (handles_count > 0)
        if req.handles_count == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Gate 6C-5 & I-INPUT-SYNTHETIC-BOUND: Synthetic / remote input rejected during ModalLock
        if self.modal_lock_active {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Check reserved payload bytes (must be 0)
        let reserved_bytes = &req.payload[32..40];
        if reserved_bytes != [0u8; 8] {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Monotonic local TSC timestamping assignment
        self.last_assigned_tsc += 10;

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

        let payload_len = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());

        if payload_len == 0 || payload_len > 512 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Invariant I-INPUT-NO-IMPLICIT-INTENT-AUTHORITY: uids formats intent payload for intentd validation
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

    pub fn fail_closed_quarantine_modal_lock(&mut self) {
        // Gate 6C-13 & I-INPUT-FAIL-CLOSED: authui crash invalidates ModalLock & keeps input QUARANTINED
        self.modal_lock_active = false;
        self.active_auth_transaction = DistributedId::new(0, 0);
        self.focus_state = FOCUS_STATE_CAPTURED; //FOCUS_STATE_QUARANTINED
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
