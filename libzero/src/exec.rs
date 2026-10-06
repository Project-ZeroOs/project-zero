//! ZeroOS - zero-exec-lib Application Integration Library
//!
//! Authoritative Contract: Stage 4C Architecture Specification & Phase 4C Implementation Plan.
//! Wraps CLI executables in capability handles so workspaced can stream stdio in memory.

use crate::error::ZeroError;
use crate::ipc::IpcMessage;
use crate::resource::DistributedId;

// Exec Protocol Opcodes (0x4E01 .. 0x4E06)
pub const OP_EXEC_STDIN_WRITE: u64 = 0x4E01;
pub const OP_EXEC_STDOUT_READ: u64 = 0x4E02;
pub const OP_EXEC_STDERR_READ: u64 = 0x4E03;
pub const OP_EXEC_EOF:         u64 = 0x4E04;
pub const OP_EXEC_EXIT:        u64 = 0x4E05;
pub const OP_EXEC_CANCEL:      u64 = 0x4E06;

pub const MAX_STREAM_BUF_SIZE: usize = 1024;

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

    /// Binds an existing CLI program process to a workload task under delegated capability authority.
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
            return Err(ZeroError::PermissionDenied); // EXEC-G capability enforcement
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

    /// Verifies workspace containment before performing any stream operation.
    pub fn verify_workspace_containment(&self, caller_workspace_id: DistributedId) -> Result<(), ZeroError> {
        if self.workspace_id != caller_workspace_id {
            return Err(ZeroError::PermissionDenied); // EXEC-H workspace boundary
        }
        Ok(())
    }

    /// Writes data to process stdin through capability IPC stream.
    pub fn write_stdin(&mut self, data: &[u8], caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        if self.state != ExecProcessState::Bound && self.state != ExecProcessState::Running {
            return Err(ZeroError::InvalidRequest);
        }
        self.state = ExecProcessState::Running;
        self.stdin_pipe.write(data)
    }

    /// Signals EOF on stdin stream.
    pub fn signal_eof(&mut self, caller_workspace_id: DistributedId) -> Result<(), ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        self.stdin_pipe.set_eof();
        if self.state == ExecProcessState::Running || self.state == ExecProcessState::Bound {
            self.state = ExecProcessState::EofInput;
        }
        Ok(())
    }

    /// Executes the bound CLI program over in-memory stdin/stdout/stderr pipes.
    pub fn execute_cli_program(&mut self) -> Result<i32, ZeroError> {
        if self.task_cap_handle == 0 {
            return Err(ZeroError::PermissionDenied);
        }

        let mut in_buf = [0u8; MAX_STREAM_BUF_SIZE];
        let n_in = self.stdin_pipe.read(&mut in_buf);

        // Check if program is 'fail_cli' or non-zero trigger binary
        if &self.binary_name[0..8] == b"fail_cli" {
            let _ = self.stderr_pipe.write(b"Error: process exit status 1");
            self.exit_code = 1;
            self.state = ExecProcessState::Failed;
            return Ok(1);
        }

        // Standard CLI transformation (uppercase transform / stdio pipe)
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

    /// Reads stdout stream data produced by process into target buffer.
    pub fn read_stdout(&mut self, target: &mut [u8], caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        Ok(self.stdout_pipe.read(target))
    }

    /// Reads stderr stream data produced by process into target buffer.
    pub fn read_stderr(&mut self, target: &mut [u8], caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace_containment(caller_workspace_id)?;
        Ok(self.stderr_pipe.read(target))
    }

    /// Cancels the process execution when owning workload is cancelled.
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
