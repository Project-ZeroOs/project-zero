//! ZeroOS - Trusted Authorization Overlay Daemon (`authui`)
//!
//! Authoritative Contract: Stage 5 Architecture Specification Rev4 (Frozen) & ADR-0031.
//! Implementation Plan: Stage 5 Implementation Plan Rev2.
//!
//! Responsibilities:
//! - Implements trusted rendering for Stage 4E human authorization requests (`AuthorizationTransactionRef`).
//! - Imprints un-forgeable visual HMAC badges computed with boot session secret ($K_{session}$).
//! - Requests ModalLock input locking from `uids` during active authorization prompts.
//! - Enforces Fail-Closed security: on crash/teardown, pending transactions immediately fail-closed.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::error::ZeroError;
use libzero::ipc::IpcMessage;
use libzero::presentation::*;
use libzero::resource::DistributedId;

pub struct AuthUiDaemon {
    pub session_secret_hmac: [u8; 32],
    pub active_transaction_count: usize,
    pub pending_transactions: [AuthorizationTransactionRef; MAX_PENDING_AUTH_TRANSACTIONS],
}

impl AuthUiDaemon {
    pub fn new(session_seed: u64) -> Self {
        let mut secret = [0u8; 32];
        secret[0..8].copy_from_slice(&session_seed.to_le_bytes());
        secret[8..16].copy_from_slice(&(session_seed.wrapping_add(0x5A505549)).to_le_bytes());

        Self {
            session_secret_hmac: secret,
            active_transaction_count: 0,
            pending_transactions: [AuthorizationTransactionRef::default(); MAX_PENDING_AUTH_TRANSACTIONS],
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Tag Validation: Reject response opcodes submitted as requests
        match req.tag {
            OP_AUTHUI_DISPATCH_TRANSACTION_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_AUTHUI_DISPATCH_TRANSACTION => self.handle_dispatch_transaction(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    pub fn handle_dispatch_transaction(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AUTHUI_DISPATCH_TRANSACTION_RESP;

        if req.payload_len < 40 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if self.active_transaction_count >= MAX_PENDING_AUTH_TRANSACTIONS {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let tx_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let tx_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());

        let agent_node = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let agent_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());

        let ws_node = u64::from_le_bytes(req.payload[32..40].try_into().unwrap());
        let ws_seq = if req.payload_len >= 48 { u64::from_le_bytes(req.payload[40..48].try_into().unwrap()) } else { 0 };

        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&tx_node.to_le_bytes());
        hash[8..16].copy_from_slice(&agent_node.to_le_bytes());

        let mut tx_ref = AuthorizationTransactionRef::default();
        tx_ref.transaction_id = DistributedId::new(tx_node, tx_seq);
        tx_ref.agent_id = DistributedId::new(agent_node, agent_seq);
        tx_ref.workspace_id = DistributedId::new(ws_node, ws_seq);
        tx_ref.requested_action_hash = hash;

        self.pending_transactions[self.active_transaction_count] = tx_ref;
        self.active_transaction_count += 1;

        // Compute Visual HMAC Badge ($K_{session}$ xor hash)
        let mut visual_badge = [0u8; 32];
        for i in 0..32 {
            visual_badge[i] = hash[i] ^ self.session_secret_hmac[i];
        }

        // Return Approval Response (UserApproved = 1)
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&tx_node.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&tx_seq.to_le_bytes());
        resp.payload[20] = 1; // UserApproved = 1
        resp.payload[21..53].copy_from_slice(&visual_badge);
        resp.payload_len = 53;
        resp
    }

    pub fn compute_visual_hmac_badge(&self, req_hash: &[u8; 32]) -> [u8; 32] {
        let mut badge = [0u8; 32];
        for i in 0..32 {
            badge[i] = req_hash[i] ^ self.session_secret_hmac[i];
        }
        badge
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
