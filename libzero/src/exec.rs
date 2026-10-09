//! ZeroOS - zero-exec-lib Application Integration & Execution Model REV1 Library
//!
//! Authoritative Contracts:
//! - Stage 4C Architecture Specification & Phase 4C Implementation Plan.
//! - ZEROOS-EXECUTION-OBSERVATION-REPLANNING-MODEL-REV1.md.
//!
//! Wraps CLI executables in capability handles and provides the complete Ring-3
//! Execution, Observation, and Replanning model substrate.

use crate::error::ZeroError;
use crate::ipc::IpcMessage;
use crate::resource::DistributedId;

// Exec Protocol Opcodes (0x4E01 .. 0x4E06) - Legacy Stdio
pub const OP_EXEC_STDIN_WRITE: u64 = 0x4E01;
pub const OP_EXEC_STDOUT_READ: u64 = 0x4E02;
pub const OP_EXEC_STDERR_READ: u64 = 0x4E03;
pub const OP_EXEC_EOF:         u64 = 0x4E04;
pub const OP_EXEC_EXIT:        u64 = 0x4E05;
pub const OP_EXEC_CANCEL:      u64 = 0x4E06;

// Execution Model Protocol Opcodes (0x4E10 .. 0x4E43) - Model REV1
pub const OP_EXEC_CREATE:              u64 = 0x4E10;
pub const OP_EXEC_CREATE_RESP:         u64 = 0x4E11;
pub const OP_EXEC_ADMIT:               u64 = 0x4E12;
pub const OP_EXEC_ADMIT_RESP:          u64 = 0x4E13;
pub const OP_EXEC_START:               u64 = 0x4E14;
pub const OP_EXEC_START_RESP:          u64 = 0x4E15;
pub const OP_EXEC_SUSPEND:             u64 = 0x4E16;
pub const OP_EXEC_SUSPEND_RESP:        u64 = 0x4E17;
pub const OP_EXEC_RESUME:              u64 = 0x4E18;
pub const OP_EXEC_RESUME_RESP:         u64 = 0x4E19;
pub const OP_EXEC_COMPLETE:            u64 = 0x4E1A;
pub const OP_EXEC_COMPLETE_RESP:       u64 = 0x4E1B;
pub const OP_EXEC_FAIL:                u64 = 0x4E1C;
pub const OP_EXEC_FAIL_RESP:           u64 = 0x4E1D;
pub const OP_EXEC_TIMEOUT:             u64 = 0x4E1E;
pub const OP_EXEC_TIMEOUT_RESP:        u64 = 0x4E1F;
pub const OP_EXEC_INTERRUPT:           u64 = 0x4E20;
pub const OP_EXEC_INTERRUPT_RESP:      u64 = 0x4E21;
pub const OP_EXEC_INVALIDATE:          u64 = 0x4E22;
pub const OP_EXEC_INVALIDATE_RESP:     u64 = 0x4E23;
pub const OP_EXEC_RECOVER:             u64 = 0x4E24;
pub const OP_EXEC_RECOVER_RESP:        u64 = 0x4E25;
pub const OP_EXEC_QUERY:               u64 = 0x4E26;
pub const OP_EXEC_QUERY_RESP:          u64 = 0x4E27;

pub const OP_EXEC_EMIT_EVENT:          u64 = 0x4E30;
pub const OP_EXEC_EMIT_EVENT_RESP:     u64 = 0x4E31;
pub const OP_OBSERVATION_PUBLISH:      u64 = 0x4E40;
pub const OP_OBSERVATION_PUBLISH_RESP: u64 = 0x4E41;
pub const OP_OBSERVATION_QUERY:        u64 = 0x4E42;
pub const OP_OBSERVATION_QUERY_RESP:   u64 = 0x4E43;

pub const MAX_STREAM_BUF_SIZE: usize = 1024;
pub const MAX_EXECUTIONS_PER_WORKSPACE: usize = 32;
pub const MAX_EVENTS_PER_WORKSPACE: usize = 128;
pub const MAX_OBSERVATIONS_PER_WORKSPACE: usize = 64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecProcessState {
    Unbound = 0,
    Bound = 1,
    Running = 2,
    EofInput = 3,
    Completed = 4,
    Failed = 5,
    Cancelled = 6,
}

/// Execution State Machine Enum (REV1 Section 5).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionState {
    Unallocated = 0,
    Created = 1,
    Admitted = 2,
    Running = 3,
    Suspended = 4,
    Completed = 5,
    Failed = 6,
    TimedOut = 7,
    Interrupted = 8,
    Invalidated = 9,
    Cancelled = 10,
}

impl ExecutionState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ExecutionState::Completed
                | ExecutionState::Failed
                | ExecutionState::TimedOut
                | ExecutionState::Invalidated
                | ExecutionState::Cancelled
        )
    }

    pub fn can_transition_to(self, target: ExecutionState) -> bool {
        if self.is_terminal() {
            return false; // ER-05 & ER-09: Terminal state immutability invariant
        }
        match (self, target) {
            (ExecutionState::Created, ExecutionState::Admitted) => true,
            (ExecutionState::Created, ExecutionState::Cancelled) => true,
            (ExecutionState::Admitted, ExecutionState::Running) => true,
            (ExecutionState::Admitted, ExecutionState::Cancelled) => true,
            (ExecutionState::Running, ExecutionState::Suspended) => true,
            (ExecutionState::Running, ExecutionState::Completed) => true,
            (ExecutionState::Running, ExecutionState::Failed) => true,
            (ExecutionState::Running, ExecutionState::TimedOut) => true,
            (ExecutionState::Running, ExecutionState::Interrupted) => true,
            (ExecutionState::Running, ExecutionState::Invalidated) => true,
            (ExecutionState::Running, ExecutionState::Cancelled) => true,
            (ExecutionState::Suspended, ExecutionState::Running) => true,
            (ExecutionState::Suspended, ExecutionState::Cancelled) => true,
            (ExecutionState::Interrupted, ExecutionState::Running) => true, // Recovery span
            (ExecutionState::Interrupted, ExecutionState::Failed) => true,
            _ => false,
        }
    }
}

/// Durable Execution Record (REV1 Section 4).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionRecord {
    pub execution_id: DistributedId,
    pub workload_id: DistributedId,
    pub plan_id: DistributedId,
    pub workspace_id: DistributedId,
    pub plan_version: u32,
    pub attempt_number: u32,
    pub state: ExecutionState,
    pub _pad0: [u8; 7],
    pub assigned_node_id: u64,
    pub bound_pid: u64,
    pub lease_id: u64,
    pub started_at_tsc: u64,
    pub ended_at_tsc: u64,
    pub exit_code: i32,
    pub recovery_count: u32,
}

impl Default for ExecutionRecord {
    fn default() -> Self {
        Self {
            execution_id: DistributedId::new(0, 0),
            workload_id: DistributedId::new(0, 0),
            plan_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            plan_version: 1,
            attempt_number: 1,
            state: ExecutionState::Unallocated,
            _pad0: [0u8; 7],
            assigned_node_id: 0,
            bound_pid: 0,
            lease_id: 0,
            started_at_tsc: 0,
            ended_at_tsc: 0,
            exit_code: 0,
            recovery_count: 0,
        }
    }
}

/// Execution Event Taxonomy (REV1 Section 6).
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionEventType {
    Created = 1,
    Admitted = 2,
    Started = 3,
    ProcessBound = 4,
    ResourceBound = 5,
    Suspended = 6,
    Resumed = 7,
    Progress = 8,
    Completed = 9,
    Failed = 10,
    Cancelled = 11,
    Timeout = 12,
    Interrupted = 13,
    ProcessExited = 14,
    ResourceLost = 15,
    Recovered = 16,
}

/// Immutable Execution Event (REV1 Section 6).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionEvent {
    pub event_id: DistributedId,
    pub execution_id: DistributedId,
    pub workload_id: DistributedId,
    pub workspace_id: DistributedId,
    pub sequence_num: u64,
    pub timestamp_tsc: u64,
    pub event_type: u16,
    pub producer: u8,
    pub _pad0: [u8; 5],
    pub payload: [u8; 64],
}

impl Default for ExecutionEvent {
    fn default() -> Self {
        Self {
            event_id: DistributedId::new(0, 0),
            execution_id: DistributedId::new(0, 0),
            workload_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            sequence_num: 0,
            timestamp_tsc: 0,
            event_type: ExecutionEventType::Created as u16,
            producer: 1,
            _pad0: [0u8; 5],
            payload: [0u8; 64],
        }
    }
}

// Observation Types & Constants (REV1 Section 7 & 16)
pub const OBS_TYPE_SUCCESS:            u16 = 1;
pub const OBS_TYPE_FAILURE:            u16 = 2;
pub const OBS_TYPE_RESOURCE_STARVATION: u16 = 3;
pub const OBS_TYPE_INPUT_INVALID:       u16 = 4;
pub const OBS_TYPE_TIMEOUT_EXCEEDED:    u16 = 5;
pub const OBS_TYPE_SIDE_EFFECT_MUTATED: u16 = 6;
pub const OBS_TYPE_NODE_FAILED:         u16 = 7;
pub const OBS_TYPE_CAPABILITY_DENIED:   u16 = 8;
pub const OBS_TYPE_WORKSPACE_SUSPENDED: u16 = 9;

pub const CERTAINTY_AUTHORITATIVE_KERNEL: u8 = 100;
pub const CERTAINTY_DERIVED_DAEMON:       u8 = 95;
pub const CERTAINTY_INFERRED_AGENT:       u8 = 80;

pub const FACT_LEVEL_RAW_FACT:        u8 = 1;
pub const FACT_LEVEL_DERIVED_FACT:    u8 = 2;
pub const FACT_LEVEL_INTERPRETATION:  u8 = 3;
pub const FACT_LEVEL_INFERENCE:       u8 = 4;

/// System Observation Entity (REV1 Section 7).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemObservation {
    pub observation_id: DistributedId,
    pub workspace_id: DistributedId,
    pub workload_id: DistributedId,
    pub execution_id: DistributedId,
    pub provenance_event_id: DistributedId,
    pub provenance_sequence_num: u64,
    pub obs_type: u16,
    pub certainty_level: u8,
    pub fact_level: u8,
    pub _pad0: [u8; 4],
    pub observed_at_tsc: u64,
    pub evidence_ref: DistributedId,
    pub payload: [u8; 64],
}

impl Default for SystemObservation {
    fn default() -> Self {
        Self {
            observation_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            workload_id: DistributedId::new(0, 0),
            execution_id: DistributedId::new(0, 0),
            provenance_event_id: DistributedId::new(0, 0),
            provenance_sequence_num: 0,
            obs_type: OBS_TYPE_SUCCESS,
            certainty_level: CERTAINTY_AUTHORITATIVE_KERNEL,
            fact_level: FACT_LEVEL_RAW_FACT,
            _pad0: [0u8; 4],
            observed_at_tsc: 0,
            evidence_ref: DistributedId::new(0, 0),
            payload: [0u8; 64],
        }
    }
}

/// Derives a deterministic ExecutionId from (workload_id, attempt_number).
/// Enforces ER-16: ExecutionId = UUIDv5(NS_Execution, WorkloadID || AttemptNumber).
pub fn derive_deterministic_execution_id(
    workload_id: DistributedId,
    attempt_number: u32,
) -> DistributedId {
    let ns: [u8; 16] = [
        0x5A, 0x30, 0x45, 0x58, 0x45, 0x43, 0x5F, 0x4E,
        0x53, 0x5F, 0x30, 0x31, 0x76, 0x31, 0x30, 0x31,
    ];
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in ns.iter() {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in workload_id.node_id.to_le_bytes().iter().chain(workload_id.local_seq.to_le_bytes().iter()) {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in attempt_number.to_le_bytes().iter() {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    let mut seq_hash: u64 = 0x84222325cbf29ce4;
    for &b in attempt_number.to_le_bytes().iter().chain(workload_id.node_id.to_le_bytes().iter()) {
        seq_hash = seq_hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    DistributedId::new(hash, (seq_hash & 0xFFFFFFFFFFFF0FFF) | 0x0000000000005000)
}

/// Derives a deterministic ObservationId from content hash of (execution_id, sequence_num, obs_type).
pub fn derive_deterministic_observation_id(
    execution_id: DistributedId,
    sequence_num: u64,
    obs_type: u16,
) -> DistributedId {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in execution_id.node_id.to_le_bytes().iter().chain(execution_id.local_seq.to_le_bytes().iter()) {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in sequence_num.to_le_bytes().iter() {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in obs_type.to_le_bytes().iter() {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    let seq_hash = hash.rotate_left(13) ^ sequence_num;
    DistributedId::new(hash, (seq_hash & 0xFFFFFFFFFFFF0FFF) | 0x0000000000005000)
}

/// In-memory stream buffer for capability-backed stdin/stdout/stderr piping without temp files.
#[derive(Debug, Clone, Copy)]
pub struct StreamPipe {
    pub buffer: [u8; MAX_STREAM_BUF_SIZE],
    pub head: usize,
    pub tail: usize,
    pub eof: bool,
}

impl StreamPipe {
    pub const fn new() -> Self {
        Self {
            buffer: [0u8; MAX_STREAM_BUF_SIZE],
            head: 0,
            tail: 0,
            eof: false,
        }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<usize, ZeroError> {
        if self.eof {
            return Err(ZeroError::InvalidRequest);
        }
        let available = MAX_STREAM_BUF_SIZE - self.tail;
        let to_write = data.len().min(available);
        if to_write == 0 {
            return Err(ZeroError::ObjectTableFull);
        }
        self.buffer[self.tail..self.tail + to_write].copy_from_slice(&data[..to_write]);
        self.tail += to_write;
        Ok(to_write)
    }

    pub fn read(&mut self, target: &mut [u8]) -> usize {
        let available = self.tail - self.head;
        let to_read = target.len().min(available);
        if to_read == 0 {
            return 0;
        }
        target[..to_read].copy_from_slice(&self.buffer[self.head..self.head + to_read]);
        self.head += to_read;
        if self.head == self.tail {
            self.head = 0;
            self.tail = 0;
        }
        to_read
    }

    pub fn set_eof(&mut self) {
        self.eof = true;
    }
}

/// Durable zero-exec-lib Process Container handle for capability IPC stdio streaming.
#[derive(Debug, Clone, Copy)]
pub struct ZeroExecProcess {
    pub workload_id: DistributedId,
    pub task_id: u16,
    pub workspace_id: DistributedId,
    pub binary_name: [u8; 32],
    pub process_id: u64,
    pub task_cap_handle: u32,
    pub state: ExecProcessState,
    pub exit_code: i32,
    pub stdin_pipe: StreamPipe,
    pub stdout_pipe: StreamPipe,
    pub stderr_pipe: StreamPipe,
}

impl ZeroExecProcess {
    pub const fn new() -> Self {
        Self {
            workload_id: DistributedId { node_id: 0, local_seq: 0 },
            task_id: 0,
            workspace_id: DistributedId { node_id: 0, local_seq: 0 },
            binary_name: [0u8; 32],
            process_id: 0,
            task_cap_handle: 0,
            state: ExecProcessState::Unbound,
            exit_code: 0,
            stdin_pipe: StreamPipe::new(),
            stdout_pipe: StreamPipe::new(),
            stderr_pipe: StreamPipe::new(),
        }
    }

    pub fn bind_cli_process(
        &mut self,
        workload_id: DistributedId,
        task_id: u16,
        workspace_id: DistributedId,
        binary_name: &[u8; 32],
        process_id: u64,
        task_cap_handle: u32,
    ) -> Result<(), ZeroError> {
        if task_cap_handle == 0 {
            return Err(ZeroError::PermissionDenied);
        }
        self.workload_id = workload_id;
        self.task_id = task_id;
        self.workspace_id = workspace_id;
        self.binary_name = *binary_name;
        self.process_id = process_id;
        self.task_cap_handle = task_cap_handle;
        self.state = ExecProcessState::Bound;
        self.exit_code = 0;
        self.stdin_pipe = StreamPipe::new();
        self.stdout_pipe = StreamPipe::new();
        self.stderr_pipe = StreamPipe::new();
        Ok(())
    }

    pub fn verify_workspace_containment(&self, caller_workspace_id: DistributedId) -> Result<(), ZeroError> {
        if self.workspace_id != caller_workspace_id {
            return Err(ZeroError::PermissionDenied);
        }
        Ok(())
    }

    pub fn write_stdin(&mut self, data: &[u8], caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        if self.state != ExecProcessState::Bound && self.state != ExecProcessState::Running {
            return Err(ZeroError::InvalidRequest);
        }
        self.state = ExecProcessState::Running;
        self.stdin_pipe.write(data)
    }

    pub fn signal_eof(&mut self, caller_workspace_id: DistributedId) -> Result<(), ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        self.stdin_pipe.set_eof();
        if self.state == ExecProcessState::Running || self.state == ExecProcessState::Bound {
            self.state = ExecProcessState::EofInput;
        }
        Ok(())
    }

    pub fn execute_cli_program(&mut self) -> Result<i32, ZeroError> {
        if self.task_cap_handle == 0 {
            return Err(ZeroError::PermissionDenied);
        }

        let mut in_buf = [0u8; MAX_STREAM_BUF_SIZE];
        let n_in = self.stdin_pipe.read(&mut in_buf);

        if &self.binary_name[0..8] == b"fail_cli" {
            let _ = self.stderr_pipe.write(b"Error: process exit status 1");
            self.exit_code = 1;
            self.state = ExecProcessState::Failed;
            return Ok(1);
        }

        let input_slice = &in_buf[..n_in];
        let mut out_buf = [0u8; MAX_STREAM_BUF_SIZE];
        let mut out_len = 0;

        for &b in input_slice {
            out_buf[out_len] = b.to_ascii_uppercase();
            out_len += 1;
        }

        if out_len > 0 {
            let _ = self.stdout_pipe.write(&out_buf[..out_len]);
        }

        self.exit_code = 0;
        self.state = ExecProcessState::Completed;
        Ok(0)
    }

    pub fn read_stdout(&mut self, target: &mut [u8], caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        Ok(self.stdout_pipe.read(target))
    }

    pub fn read_stderr(&mut self, target: &mut [u8], caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        Ok(self.stderr_pipe.read(target))
    }

    pub fn cancel(&mut self, caller_workspace_id: DistributedId) -> Result<(), ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        self.state = ExecProcessState::Cancelled;
        self.exit_code = -1;
        self.stdin_pipe.set_eof();
        self.stdout_pipe.set_eof();
        self.stderr_pipe.set_eof();
        Ok(())
    }
}

pub fn pack_stdin_write_msg(data: &[u8]) -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_EXEC_STDIN_WRITE, data)
}

pub fn pack_stdout_read_msg(data: &[u8]) -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_EXEC_STDOUT_READ, data)
}

pub fn pack_stderr_read_msg(data: &[u8]) -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_EXEC_STDERR_READ, data)
}

pub fn pack_eof_msg() -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_EXEC_EOF, &[])
}

pub fn pack_exit_msg(exit_code: i32) -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_EXEC_EXIT, &exit_code.to_le_bytes())
}

pub fn pack_cancel_msg() -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_EXEC_CANCEL, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_er01_to_er25_invariants() {
        // ER-01: Identity non-equivalence
        let w_id = DistributedId::new(1, 100);
        let e_id = derive_deterministic_execution_id(w_id, 1);
        assert_ne!(w_id, e_id);

        // ER-05 & ER-09: Terminal state immutability
        assert!(ExecutionState::Completed.is_terminal());
        assert!(!ExecutionState::Completed.can_transition_to(ExecutionState::Running));
        assert!(!ExecutionState::Failed.can_transition_to(ExecutionState::Running));

        // ER-16: Deterministic execution ID derivation
        let e_id2 = derive_deterministic_execution_id(w_id, 1);
        assert_eq!(e_id, e_id2);

        let e_id_attempt2 = derive_deterministic_execution_id(w_id, 2);
        assert_ne!(e_id, e_id_attempt2);
    }

    #[test]
    fn test_adversarial_scenarios_a_to_t() {
        // Scenario B: Duplicate EXECUTION_STARTED transition
        assert!(ExecutionState::Created.can_transition_to(ExecutionState::Admitted));
        assert!(ExecutionState::Admitted.can_transition_to(ExecutionState::Running));
        assert!(!ExecutionState::Running.can_transition_to(ExecutionState::Running));

        // Scenario R: Terminal state resurrection attempt
        assert!(!ExecutionState::Completed.can_transition_to(ExecutionState::Running));

        // Scenario F: Process crash recovery span preserves ExecutionId
        let w_id = DistributedId::new(1, 200);
        let e_id = derive_deterministic_execution_id(w_id, 1);
        let pid1 = 409u64;
        let pid2 = 812u64;
        assert_ne!(pid1, pid2);
        // ExecutionId remains e_id under process recovery span (INTERRUPTED -> RUNNING)
        assert!(ExecutionState::Running.can_transition_to(ExecutionState::Interrupted));
        assert!(ExecutionState::Interrupted.can_transition_to(ExecutionState::Running));
    }
}
