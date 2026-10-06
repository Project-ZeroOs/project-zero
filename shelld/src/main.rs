//! ZeroOS - User Session Substrate & Human Operating Environment Daemon (`shelld`)
//!
//! Authoritative Contract: Stage 6F Architecture Specification Rev7 (Frozen).
//! Implementation Plan: Stage 6F Implementation Plan Rev2.
//!
//! Responsibilities:
//! - Sole authoritative owner of `SessionRecord` and `SurfaceLayoutGeneration`.
//! - Monotonic `SessionMigrationEpoch` fencing (`I-6F-SESSION-EPOCH-FENCING`).
//! - 0-or-1 single-authority safety (`I-6F-SINGLE-AUTHORITY-SAFETY`).
//! - Fail-safe handoff state machine & `FencingProofDescriptor` generation & full validation.
//! - 3-Branch `STATE_RECOVERY_UNCERTAIN` reconciliation protocol.

#![no_std]
#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use libzero::syscall::sys_exit;

#[allow(unused_imports)]
use libzero::error::ZeroError;
use libzero::identity::DistributedIdAllocator;
use libzero::ipc::IpcMessage;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::presentation::*;
use libzero::resource::DistributedId;
use libzero::session::*;
use libzero::grounding::*;

// ============================================================================
// Session Migration Authority States
// ============================================================================

pub const SESSION_AUTHORITY_AUTHORITATIVE: u8 = 1;
pub const SESSION_AUTHORITY_FENCED_PENDING_COMMIT: u8 = 2;
pub const SESSION_AUTHORITY_REMOTE_COMMITTED: u8 = 3;
pub const SESSION_AUTHORITY_RECOVERY_UNCERTAIN: u8 = 4;
pub const SESSION_AUTHORITY_ABORTED_RECOVERY: u8 = 5;

// ============================================================================
// Session Substrate Daemon (`SessionDaemon`)
// ============================================================================

pub struct SessionDaemon {
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub active_session_count: usize,
    pub sessions: [SessionRecord; MAX_SESSIONS_PER_NODE],
    pub telemetry_ring: [AgentActivityDescriptor; MAX_TELEMETRY_RING_ENTRIES],
    pub telemetry_count: usize,
    pub recovery_state: u8,

    // Stage 6F Generation & Epoch Fencing State
    pub node_id: u64,
    pub layout_generation: u64,
    pub committed_epoch: u64,
    pub authority_state: u8,
    pub active_dest_nonce: u64,
    pub fence_sequence: u64,
    pub last_fencing_proof: Option<FencingProofDescriptor>,
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

            node_id,
            layout_generation: 1,
            committed_epoch: 1,
            authority_state: SESSION_AUTHORITY_AUTHORITATIVE,
            active_dest_nonce: 0,
            fence_sequence: 1,
            last_fencing_proof: None,
        }
    }

    /// `I-6F-MUTATION-FENCE`: Check if local node possesses active mutation authority for Epoch.
    pub fn check_mutation_authority(&self, epoch: u64) -> Result<(), ZeroError> {
        if self.authority_state != SESSION_AUTHORITY_AUTHORITATIVE {
            return Err(ZeroError::StaleSessionEpoch);
        }
        if epoch > 0 && epoch != self.committed_epoch {
            return Err(ZeroError::StaleSessionEpoch);
        }
        Ok(())
    }

    /// Layout Generation Increment (owned by `shelld`).
    pub fn increment_layout_gen(&mut self) -> u64 {
        self.layout_generation += 1;
        self.layout_generation
    }

    /// Step 1 of Handoff: Destination issues single-use transaction nonce.
    pub fn issue_dest_nonce(&mut self, nonce: u64) -> u64 {
        self.active_dest_nonce = nonce;
        nonce
    }

    /// Step 2 of Handoff: Source Node durably fences local session state,
    /// advances draft epoch $N+1$, and constructs `FencingProofDescriptor`.
    pub fn execute_source_fence(
        &mut self,
        session_id: u64,
        source_node_id: u64,
        dest_node_id: u64,
        dest_nonce: u64,
    ) -> Result<FencingProofDescriptor, ZeroError> {
        if self.authority_state != SESSION_AUTHORITY_AUTHORITATIVE {
            return Err(ZeroError::StaleSessionEpoch);
        }

        let previous_epoch = self.committed_epoch;
        let proposed_epoch = previous_epoch + 1;
        self.authority_state = SESSION_AUTHORITY_FENCED_PENDING_COMMIT;

        let proof = FencingProofDescriptor {
            magic: FENCING_PROOF_MAGIC,
            session_id,
            source_node_id,
            dest_node_id,
            previous_epoch,
            proposed_epoch,
            dest_transaction_nonce: dest_nonce,
            fence_sequence: self.fence_sequence + 1,
        };
        self.fence_sequence += 1;
        self.last_fencing_proof = Some(proof);
        Ok(proof)
    }

    /// Step 3 of Handoff: Destination Node performs complete 8-point validation of `FencingProofDescriptor`
    /// and commits Epoch $N+1$ (`STATE_AUTHORITATIVE`).
    pub fn receive_fencing_proof(
        &mut self,
        proof: &FencingProofDescriptor,
        expected_session_id: u64,
        expected_source_node_id: u64,
        expected_dest_node_id: u64,
        expected_dest_nonce: u64,
    ) -> Result<(), ZeroError> {
        // 1. Magic check
        if proof.magic != FENCING_PROOF_MAGIC {
            return Err(ZeroError::InvalidRequest);
        }
        // 2. Session binding check
        if proof.session_id == 0 || proof.session_id != expected_session_id {
            return Err(ZeroError::PermissionDenied);
        }
        // 3. Source node binding check
        if proof.source_node_id == 0 || proof.source_node_id != expected_source_node_id {
            return Err(ZeroError::PermissionDenied);
        }
        // 4. Destination node binding check
        if proof.dest_node_id == 0 || proof.dest_node_id != expected_dest_node_id {
            return Err(ZeroError::PermissionDenied);
        }
        // 5. Single-use destination transaction nonce check (anti-replay)
        if expected_dest_nonce == 0 || proof.dest_transaction_nonce != expected_dest_nonce || proof.dest_transaction_nonce != self.active_dest_nonce {
            return Err(ZeroError::StaleEndpoint);
        }
        // 6. Previous epoch binding check
        if proof.previous_epoch != self.committed_epoch {
            return Err(ZeroError::StaleSessionEpoch);
        }
        // 7. Proposed epoch transition check (must be exact N+1 increment, 0 invalid jumps)
        if proof.proposed_epoch != proof.previous_epoch + 1 {
            return Err(ZeroError::StaleSessionEpoch);
        }
        // 8. Persistent fence sequence monotonicity check
        if proof.fence_sequence <= self.fence_sequence {
            return Err(ZeroError::StaleSessionEpoch);
        }

        // All 8 validations pass cleanly: Commit epoch N+1, become sole authoritative owner, consume single-use nonce
        self.committed_epoch = proof.proposed_epoch;
        self.fence_sequence = proof.fence_sequence;
        self.authority_state = SESSION_AUTHORITY_AUTHORITATIVE;
        self.active_dest_nonce = 0; // Consume single-use nonce
        Ok(())
    }

    /// 3-Branch `STATE_RECOVERY_UNCERTAIN` Reconciliation Protocol.
    pub fn reconcile_recovery(&mut self, remote_committed: bool, remote_aborted: bool) -> u8 {
        if self.authority_state != SESSION_AUTHORITY_RECOVERY_UNCERTAIN
            && self.authority_state != SESSION_AUTHORITY_FENCED_PENDING_COMMIT {
            return self.authority_state;
        }

        if remote_committed {
            // Branch 1: Destination committed -> Source transitions to REMOTE_COMMITTED
            self.authority_state = SESSION_AUTHORITY_REMOTE_COMMITTED;
        } else if remote_aborted {
            // Branch 2: Reconciliation proves transaction never committed -> Aborted recovery, advance epoch to N+2
            self.committed_epoch += 2;
            self.authority_state = SESSION_AUTHORITY_AUTHORITATIVE; // Resume authority under Epoch N+2
        } else {
            // Branch 3: Inconclusive / active partition -> REMAIN RECOVERY_UNCERTAIN (0 mutation authority)
            self.authority_state = SESSION_AUTHORITY_RECOVERY_UNCERTAIN;
        }

        self.authority_state
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Enforce IPC mutation fencing: Reject mutations if not AUTHORITATIVE
        match req.tag {
            OP_SESSION_CREATE | OP_SESSION_DESTROY | OP_SESSION_SWITCH_WORKSPACE | OP_SESSION_SET_LAYOUT => {
                if let Err(err) = self.check_mutation_authority(self.committed_epoch) {
                    let mut resp = IpcMessage::empty();
                    resp.tag = req.tag | 1;
                    resp.payload[0..4].copy_from_slice(&(err.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }
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

// ============================================================================
// Freestanding Entry Points
// ============================================================================

#[cfg(not(test))]
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

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe {
        libzero::syscall::sys_exit(-1);
    }
}

// ============================================================================
// Unit Tests (Executed via `cargo test`)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_fencing_handoff_sequence() {
        let mut node_a = SessionDaemon::new(1);
        let mut node_b = SessionDaemon::new(2);

        let session_id = 100;
        let dest_nonce = node_b.issue_dest_nonce(0xABCD_1234_5678);

        // Node A durably fences local session state, advances draft epoch to 2, and emits fencing proof
        let proof = node_a.execute_source_fence(session_id, 1, 2, dest_nonce).unwrap();
        assert_eq!(node_a.authority_state, SESSION_AUTHORITY_FENCED_PENDING_COMMIT);
        assert_eq!(proof.previous_epoch, 1);
        assert_eq!(proof.proposed_epoch, 2);

        // During FENCED_PENDING_COMMIT, Node A has 0 mutation authority
        let mut req = IpcMessage::empty();
        req.tag = OP_SESSION_CREATE;
        req.payload[0..8].copy_from_slice(&200u64.to_le_bytes());
        req.payload_len = 16;
        let resp = node_a.dispatch(&req);
        let err_code = i32::from_le_bytes(resp.payload[0..4].try_into().unwrap());
        assert_eq!(err_code, ZeroError::StaleSessionEpoch.as_i32());

        // Node B performs complete 8-point validation, commits Epoch 2, becomes sole authoritative owner
        node_b.receive_fencing_proof(&proof, session_id, 1, 2, dest_nonce).unwrap();
        assert_eq!(node_b.authority_state, SESSION_AUTHORITY_AUTHORITATIVE);
        assert_eq!(node_b.committed_epoch, 2);
    }

    #[test]
    fn test_zero_owner_interval_lost_fencing_proof() {
        let mut node_a = SessionDaemon::new(1);
        let mut node_b = SessionDaemon::new(2);

        let dest_nonce = node_b.issue_dest_nonce(0x9999_8888);
        let _proof = node_a.execute_source_fence(100, 1, 2, dest_nonce).unwrap();

        // Node A is fenced (0 authority)
        assert_eq!(node_a.check_mutation_authority(1).unwrap_err(), ZeroError::StaleSessionEpoch);

        // Node B has NOT received proof yet (0 authority under Epoch 2)
        assert_ne!(node_b.committed_epoch, 2);
    }

    #[test]
    fn test_source_reboot_recovery_uncertain_branches() {
        let mut node_a = SessionDaemon::new(1);

        // Node A rebooted while in FENCED_PENDING_COMMIT -> Enter RECOVERY_UNCERTAIN
        node_a.authority_state = SESSION_AUTHORITY_RECOVERY_UNCERTAIN;

        // Mutation while RECOVERY_UNCERTAIN is rejected
        assert_eq!(node_a.check_mutation_authority(1).unwrap_err(), ZeroError::StaleSessionEpoch);

        // Branch 3: Inconclusive reconciliation (timeout/partition) -> REMAIN RECOVERY_UNCERTAIN
        node_a.reconcile_recovery(false, false);
        assert_eq!(node_a.authority_state, SESSION_AUTHORITY_RECOVERY_UNCERTAIN);

        // Branch 1: Commit proven -> REMOTE_COMMITTED
        node_a.reconcile_recovery(true, false);
        assert_eq!(node_a.authority_state, SESSION_AUTHORITY_REMOTE_COMMITTED);

        // Branch 2: Abort proven -> ABORTED_RECOVERY -> N+2 AUTHORITATIVE
        node_a.authority_state = SESSION_AUTHORITY_RECOVERY_UNCERTAIN;
        node_a.reconcile_recovery(false, true);
        assert_eq!(node_a.authority_state, SESSION_AUTHORITY_AUTHORITATIVE);
        assert_eq!(node_a.committed_epoch, 3); // Advanced to N+2
    }

    #[test]
    fn test_negative_proof_validation_wrong_magic() {
        let mut node_b = SessionDaemon::new(2);
        let nonce = node_b.issue_dest_nonce(0x1111);
        let mut proof = FencingProofDescriptor {
            magic: 0xBAD_MAGIC,
            session_id: 100,
            source_node_id: 1,
            dest_node_id: 2,
            previous_epoch: 1,
            proposed_epoch: 2,
            dest_transaction_nonce: nonce,
            fence_sequence: 2,
        };

        let err = node_b.receive_fencing_proof(&proof, 100, 1, 2, nonce).unwrap_err();
        assert_eq!(err, ZeroError::InvalidRequest);
    }

    #[test]
    fn test_negative_proof_validation_wrong_session_id() {
        let mut node_b = SessionDaemon::new(2);
        let nonce = node_b.issue_dest_nonce(0x1111);
        let proof = FencingProofDescriptor {
            magic: FENCING_PROOF_MAGIC,
            session_id: 999, // Wrong session ID
            source_node_id: 1,
            dest_node_id: 2,
            previous_epoch: 1,
            proposed_epoch: 2,
            dest_transaction_nonce: nonce,
            fence_sequence: 2,
        };

        let err = node_b.receive_fencing_proof(&proof, 100, 1, 2, nonce).unwrap_err();
        assert_eq!(err, ZeroError::PermissionDenied);
    }

    #[test]
    fn test_negative_proof_validation_wrong_source_or_dest_node() {
        let mut node_b = SessionDaemon::new(2);
        let nonce = node_b.issue_dest_nonce(0x1111);

        let mut proof_wrong_source = FencingProofDescriptor {
            magic: FENCING_PROOF_MAGIC,
            session_id: 100,
            source_node_id: 99, // Wrong source node ID
            dest_node_id: 2,
            previous_epoch: 1,
            proposed_epoch: 2,
            dest_transaction_nonce: nonce,
            fence_sequence: 2,
        };
        assert_eq!(
            node_b.receive_fencing_proof(&proof_wrong_source, 100, 1, 2, nonce).unwrap_err(),
            ZeroError::PermissionDenied
        );

        let proof_wrong_dest = FencingProofDescriptor {
            magic: FENCING_PROOF_MAGIC,
            session_id: 100,
            source_node_id: 1,
            dest_node_id: 99, // Wrong dest node ID
            previous_epoch: 1,
            proposed_epoch: 2,
            dest_transaction_nonce: nonce,
            fence_sequence: 2,
        };
        assert_eq!(
            node_b.receive_fencing_proof(&proof_wrong_dest, 100, 1, 2, nonce).unwrap_err(),
            ZeroError::PermissionDenied
        );
    }

    #[test]
    fn test_negative_proof_validation_wrong_previous_epoch_and_invalid_epoch_jump() {
        let mut node_b = SessionDaemon::new(2);
        let nonce = node_b.issue_dest_nonce(0x1111);

        // Previous epoch mismatch (e.g. expected 1, received 5)
        let proof_wrong_prev = FencingProofDescriptor {
            magic: FENCING_PROOF_MAGIC,
            session_id: 100,
            source_node_id: 1,
            dest_node_id: 2,
            previous_epoch: 5,
            proposed_epoch: 6,
            dest_transaction_nonce: nonce,
            fence_sequence: 2,
        };
        assert_eq!(
            node_b.receive_fencing_proof(&proof_wrong_prev, 100, 1, 2, nonce).unwrap_err(),
            ZeroError::StaleSessionEpoch
        );

        // Invalid epoch jump (e.g. previous = 1, proposed = 5 -> expected 2)
        let proof_epoch_jump = FencingProofDescriptor {
            magic: FENCING_PROOF_MAGIC,
            session_id: 100,
            source_node_id: 1,
            dest_node_id: 2,
            previous_epoch: 1,
            proposed_epoch: 5,
            dest_transaction_nonce: nonce,
            fence_sequence: 2,
        };
        assert_eq!(
            node_b.receive_fencing_proof(&proof_epoch_jump, 100, 1, 2, nonce).unwrap_err(),
            ZeroError::StaleSessionEpoch
        );
    }

    #[test]
    fn test_negative_proof_validation_reused_fence_sequence_and_consumed_nonce() {
        let mut node_b = SessionDaemon::new(2);
        let nonce = node_b.issue_dest_nonce(0x1111);

        let proof = FencingProofDescriptor {
            magic: FENCING_PROOF_MAGIC,
            session_id: 100,
            source_node_id: 1,
            dest_node_id: 2,
            previous_epoch: 1,
            proposed_epoch: 2,
            dest_transaction_nonce: nonce,
            fence_sequence: 2,
        };

        // First application passes
        node_b.receive_fencing_proof(&proof, 100, 1, 2, nonce).unwrap();

        // Reused proof / consumed nonce -> StaleEndpoint
        assert_eq!(
            node_b.receive_fencing_proof(&proof, 100, 1, 2, nonce).unwrap_err(),
            ZeroError::StaleEndpoint
        );
    }
}
