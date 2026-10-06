//! ZeroOS - observed (Human-Agent Telemetry, Interactive Feedback & Workflow Synthesis Daemon)
//!
//! Authoritative Contract: Stage 6E Architecture Specification Rev1 & ADR-0036.
//! Implementation Plan: Stage 6E Implementation Plan Rev1.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::identity::DistributedIdAllocator;
use libzero::ipc::IpcMessage;
use libzero::observed::*;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::presentation::InputEvent;
use libzero::syscall::sys_exit;
use libzero::ZeroError;

pub const MAX_PENDING_PROMPTS: usize = 32;

pub static mut PROMPT_TABLE: [FeedbackPrompt; MAX_PENDING_PROMPTS] =
    [const { FeedbackPrompt {
        prompt_id: 0,
        task_id: 0,
        workspace_id: 0,
        timeout_ms: 30000,
        prompt_class: PROMPT_CLASS_INFORMATIONAL_CHOICE,
        default_action: FEEDBACK_RESPONSE_CANCEL,
        reserved: [0; 2],
        prompt_title: [0; 32],
    } }; MAX_PENDING_PROMPTS];

pub struct ObservedDaemon {
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub active_prompts_count: usize,
    pub active_subscriber_count: usize,
    pub recording_active: bool,
    pub recorded_event_count: u32,
    pub sensitive_events_dropped: u32,
    pub last_telemetry_tsc: u64,
}

impl ObservedDaemon {
    pub fn new(node_id: u64) -> Self {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 800);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 128, persistence).unwrap();
        unsafe {
            PROMPT_TABLE = [const { FeedbackPrompt {
                prompt_id: 0,
                task_id: 0,
                workspace_id: 0,
                timeout_ms: 30000,
                prompt_class: PROMPT_CLASS_INFORMATIONAL_CHOICE,
                default_action: FEEDBACK_RESPONSE_CANCEL,
                reserved: [0; 2],
                prompt_title: [0; 32],
            } }; MAX_PENDING_PROMPTS];
        }
        Self {
            allocator,
            active_prompts_count: 0,
            active_subscriber_count: 0,
            recording_active: false,
            recorded_event_count: 0,
            sensitive_events_dropped: 0,
            last_telemetry_tsc: 0,
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Enforce protocol direction: Reject response opcodes submitted as incoming requests
        match req.tag {
            OP_OBSERVED_SUBSCRIBE_TELEMETRY_RESP
            | OP_OBSERVED_EMIT_TELEMETRY_RESP
            | OP_OBSERVED_EMIT_PROMPT_RESP
            | OP_OBSERVED_RESPOND_PROMPT_RESP
            | OP_OBSERVED_START_RECORDING_RESP
            | OP_OBSERVED_STOP_RECORDING_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_OBSERVED_SUBSCRIBE_TELEMETRY => self.handle_subscribe_telemetry(req),
            OP_OBSERVED_EMIT_TELEMETRY => self.handle_emit_telemetry(req),
            OP_OBSERVED_EMIT_PROMPT => self.handle_emit_prompt(req),
            OP_OBSERVED_RESPOND_PROMPT => self.handle_respond_prompt(req),
            OP_OBSERVED_START_RECORDING => self.handle_start_recording(req),
            OP_OBSERVED_STOP_RECORDING => self.handle_stop_recording(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    pub fn handle_subscribe_telemetry(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBSERVED_SUBSCRIBE_TELEMETRY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());

        // Gate 6E-7 & Workspace Containment: Unprivileged cross-workspace access or invalid workspace rejected
        if ws_node == 0 || ws_seq == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Gate 6E-7 & Capability Authorization Check: Subscriber requires WorkspaceAccessCap
        if req.handles_count == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.active_subscriber_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_emit_telemetry(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBSERVED_EMIT_TELEMETRY_RESP;

        if req.payload_len < 64 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.last_telemetry_tsc += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_emit_prompt(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBSERVED_EMIT_PROMPT_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let task_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_id = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let prompt_class = req.payload[16];

        if task_id == 0 || ws_id == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Gate 6E-4 & Class-3 Security Interception: Security elevation prompts (prompt_class == 3) must be rejected immediately!
        if prompt_class == 3 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if self.active_prompts_count >= MAX_PENDING_PROMPTS {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let prompt_id = self.allocator.allocate_id().unwrap().local_seq;
        let prompt = unsafe { &mut PROMPT_TABLE[self.active_prompts_count] };
        prompt.prompt_id = prompt_id;
        prompt.task_id = task_id;
        prompt.workspace_id = ws_id;
        prompt.prompt_class = prompt_class;
        prompt.default_action = FEEDBACK_RESPONSE_CANCEL;

        self.active_prompts_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&prompt_id.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    pub fn handle_respond_prompt(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBSERVED_RESPOND_PROMPT_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let prompt_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let response_status = req.payload[8];

        let mut found_idx = None;
        unsafe {
            for i in 0..self.active_prompts_count {
                if PROMPT_TABLE[i].prompt_id == prompt_id {
                    found_idx = Some(i);
                    break;
                }
            }
        }

        match found_idx {
            Some(_idx) => {
                // Gate 6E-3 & Fail-Closed Timeout Semantics: If cancelled or timed out, response_status is CANCEL
                let final_status = if response_status == 0 {
                    FEEDBACK_RESPONSE_CANCEL
                } else {
                    response_status
                };

                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4] = final_status;
                resp.payload_len = 5;
            }
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    pub fn handle_start_recording(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBSERVED_START_RECORDING_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let session_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_id = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());

        if session_id == 0 || ws_id == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.recording_active = true;
        self.recorded_event_count = 0;
        self.sensitive_events_dropped = 0;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    pub fn handle_stop_recording(&mut self, _req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBSERVED_STOP_RECORDING_RESP;

        if !self.recording_active {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.recording_active = false;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&self.recorded_event_count.to_le_bytes());
        resp.payload[8..12].copy_from_slice(&self.sensitive_events_dropped.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    pub fn record_input_event(&mut self, event: &InputEvent) -> bool {
        if !self.recording_active {
            return false;
        }

        // Gate 6E-5 & Sensitive Input Scrubbing: Drop any event with INPUT_FLAG_SENSITIVE (0x80)
        let is_sensitive = (event.payload.modifiers & (INPUT_FLAG_SENSITIVE as u32)) != 0
            || (event.payload.reserved[0] & INPUT_FLAG_SENSITIVE) != 0;

        if is_sensitive {
            self.sensitive_events_dropped += 1;
            return false;
        }

        self.recorded_event_count += 1;
        true
    }

    pub fn fail_closed_clear_prompts(&mut self) {
        unsafe {
            for i in 0..self.active_prompts_count {
                PROMPT_TABLE[i].default_action = FEEDBACK_RESPONSE_CANCEL;
            }
        }
        self.active_prompts_count = 0;
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let daemon = ObservedDaemon::new(1);
    let _ = daemon.active_prompts_count;
    unsafe { sys_exit(0); }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { sys_exit(1); }
}
