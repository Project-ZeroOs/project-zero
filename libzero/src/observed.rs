//! ZeroOS - Stage 6E Human-Agent Telemetry, Interactive Feedback & Workflow Synthesis Subsystem ABI
//!
//! Authoritative Contract: Stage 6E Architecture Specification Rev1 & ADR-0036.

use crate::resource::DistributedId;

/// IPC Opcodes for Stage 6E observed daemon
pub const OP_OBSERVED_SUBSCRIBE_TELEMETRY: u64 = 0x0801;
pub const OP_OBSERVED_SUBSCRIBE_TELEMETRY_RESP: u64 = 0x0802;
pub const OP_OBSERVED_EMIT_TELEMETRY: u64 = 0x0803;
pub const OP_OBSERVED_EMIT_TELEMETRY_RESP: u64 = 0x0804;
pub const OP_OBSERVED_EMIT_PROMPT: u64 = 0x0805;
pub const OP_OBSERVED_EMIT_PROMPT_RESP: u64 = 0x0806;
pub const OP_OBSERVED_RESPOND_PROMPT: u64 = 0x0807;
pub const OP_OBSERVED_RESPOND_PROMPT_RESP: u64 = 0x0808;
pub const OP_OBSERVED_START_RECORDING: u64 = 0x0809;
pub const OP_OBSERVED_START_RECORDING_RESP: u64 = 0x080A;
pub const OP_OBSERVED_STOP_RECORDING: u64 = 0x080B;
pub const OP_OBSERVED_STOP_RECORDING_RESP: u64 = 0x080C;

pub const OP_INTENT_COMPILE_PROPOSAL: u64 = 0x0720;
pub const OP_INTENT_COMPILE_PROPOSAL_RESP: u64 = 0x0721;

/// Telemetry types
pub const TELEMETRY_TYPE_STATUS: u8 = 1;
pub const TELEMETRY_TYPE_PROGRESS: u8 = 2;
pub const TELEMETRY_TYPE_LOG: u8 = 3;
pub const TELEMETRY_TYPE_METRIC: u8 = 4;
pub const TELEMETRY_TYPE_TASK_COMPLETE: u8 = 5;
pub const TELEMETRY_TYPE_TASK_FAILED: u8 = 6;

/// Prompt Class types
pub const PROMPT_CLASS_INFORMATIONAL_CHOICE: u8 = 1;
pub const PROMPT_CLASS_PARAMETER_SELECTION: u8 = 2;

/// Feedback Response Status
pub const FEEDBACK_RESPONSE_CANCEL: u8 = 0;
pub const FEEDBACK_RESPONSE_ACCEPTED: u8 = 1;
pub const FEEDBACK_RESPONSE_SELECTED_OPTION: u8 = 2;

/// Input Sensitive Flag
pub const INPUT_FLAG_SENSITIVE: u8 = 0x80;

/// Recording Status
pub const RECORDING_STATUS_RECORDING: u8 = 1;
pub const RECORDING_STATUS_STOPPED: u8 = 2;
pub const RECORDING_STATUS_SYNTHESIZED: u8 = 3;
pub const RECORDING_STATUS_INTERRUPTED: u8 = 4;

/// 64-byte TelemetryFrame (64-byte aligned)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelemetryFrame {
    pub frame_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub timestamp_tsc: u64,
    pub progress_pct: u8,
    pub telemetry_type: u8,
    pub status_code: u16,
    pub reserved: [u8; 4],
    pub status_message: [u8; 24],
}

impl Default for TelemetryFrame {
    fn default() -> Self {
        Self {
            frame_id: 0,
            task_id: 0,
            workspace_id: 0,
            timestamp_tsc: 0,
            progress_pct: 0,
            telemetry_type: 0,
            status_code: 0,
            reserved: [0; 4],
            status_message: [0; 24],
        }
    }
}

/// 64-byte FeedbackPrompt (64-byte aligned)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackPrompt {
    pub prompt_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub timeout_ms: u32,
    pub prompt_class: u8,
    pub default_action: u8,
    pub reserved: [u8; 2],
    pub prompt_title: [u8; 32],
}

impl Default for FeedbackPrompt {
    fn default() -> Self {
        Self {
            prompt_id: 0,
            task_id: 0,
            workspace_id: 0,
            timeout_ms: 30000,
            prompt_class: PROMPT_CLASS_INFORMATIONAL_CHOICE,
            default_action: FEEDBACK_RESPONSE_CANCEL,
            reserved: [0; 2],
            prompt_title: [0; 32],
        }
    }
}

/// 64-byte FeedbackResponse (64-byte aligned)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackResponse {
    pub prompt_id: u64,
    pub task_id: u64,
    pub workspace_id: u64,
    pub response_status: u8,
    pub selected_option: u8,
    pub reserved: [u8; 6],
    pub response_payload: [u8; 32],
}

impl Default for FeedbackResponse {
    fn default() -> Self {
        Self {
            prompt_id: 0,
            task_id: 0,
            workspace_id: 0,
            response_status: FEEDBACK_RESPONSE_CANCEL,
            selected_option: 0,
            reserved: [0; 6],
            response_payload: [0; 32],
        }
    }
}

/// 64-byte ActionRecordHeader (64-byte aligned)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionRecordHeader {
    pub session_id: u64,
    pub workspace_id: u64,
    pub start_timestamp_tsc: u64,
    pub event_count: u32,
    pub status: u8,
    pub reserved: [u8; 27],
}

impl Default for ActionRecordHeader {
    fn default() -> Self {
        Self {
            session_id: 0,
            workspace_id: 0,
            start_timestamp_tsc: 0,
            event_count: 0,
            status: RECORDING_STATUS_STOPPED,
            reserved: [0; 27],
        }
    }
}

/// 128-byte WorkflowProposal
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowProposal {
    pub proposal_id: u64,
    pub workspace_id: u64,
    pub session_id: u64,
    pub timestamp_tsc: u64,
    pub step_count: u32,
    pub status: u8,
    pub reserved: [u8; 27],
    pub proposal_name: [u8; 64],
}

impl Default for WorkflowProposal {
    fn default() -> Self {
        Self {
            proposal_id: 0,
            workspace_id: 0,
            session_id: 0,
            timestamp_tsc: 0,
            step_count: 0,
            status: RECORDING_STATUS_STOPPED,
            reserved: [0; 27],
            proposal_name: [0; 64],
        }
    }
}

// Compile-time static assertions
const _: () = assert!(core::mem::size_of::<TelemetryFrame>() == 64);
const _: () = assert!(core::mem::size_of::<FeedbackPrompt>() == 64);
const _: () = assert!(core::mem::size_of::<FeedbackResponse>() == 64);
const _: () = assert!(core::mem::size_of::<ActionRecordHeader>() == 64);
const _: () = assert!(core::mem::size_of::<WorkflowProposal>() == 128);
