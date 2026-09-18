//! ZeroOS - libzero Escalation Protocol & Two-Man Rule Foundation

/// Escalation Request Lifecycle State.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationState {
    Requested = 0,
    PendingAuthorization = 1,
    Approved = 2,
    Denied = 3,
    Consumed = 4,
    Expired = 5,
}

/// Two-Man Rule Escalation Request Descriptor.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscalationRequest {
    /// Monotonically allocated request identifier.
    pub request_id: u64,
    /// PID of the requesting process.
    pub requester_pid: u32,
    /// Action opcode requiring escalation.
    pub action_code: u32,
    /// Target resource/service identifier.
    pub target_id: u64,
    /// Authority bitmask requested.
    pub required_authority: u16,
    /// Current authorization lifecycle state.
    pub state: EscalationState,
    /// Reservation / padding for alignment.
    pub _padding: [u8; 5],
}

const _: () = assert!(core::mem::size_of::<EscalationRequest>() == 32);

impl EscalationRequest {
    pub const fn new(
        request_id: u64,
        requester_pid: u32,
        action_code: u32,
        target_id: u64,
        required_authority: u16,
    ) -> Self {
        Self {
            request_id,
            requester_pid,
            action_code,
            target_id,
            required_authority,
            state: EscalationState::Requested,
            _padding: [0u8; 5],
        }
    }

    pub fn transition_to(&mut self, next: EscalationState) -> bool {
        match (self.state, next) {
            (EscalationState::Requested, EscalationState::PendingAuthorization) => {
                self.state = next;
                true
            }
            (EscalationState::PendingAuthorization, EscalationState::Approved)
            | (EscalationState::PendingAuthorization, EscalationState::Denied) => {
                self.state = next;
                true
            }
            (EscalationState::Approved, EscalationState::Consumed) => {
                self.state = next;
                true
            }
            (EscalationState::PendingAuthorization, EscalationState::Expired)
            | (EscalationState::Approved, EscalationState::Expired) => {
                self.state = next;
                true
            }
            _ => false,
        }
    }
}
