//! ZeroOS - zero-doc-lib Document & Artifact Integration Library
//!
//! Authoritative Contract: Stage 4D Workspace & Stage 5 Presentation Architecture Specifications.
//! Integrates document/artifact producers into ZeroOS workspace context nodes and presentation substrate.

use crate::error::ZeroError;
use crate::ipc::IpcMessage;
use crate::resource::DistributedId;
use crate::workspace::{ContextNode, ContextNodeType};
use crate::presentation::{
    PresentationSurfaceDescriptor, DISPLAY_DEFAULT_HEIGHT, DISPLAY_DEFAULT_WIDTH,
    LAYER_WORKSPACE_DEFAULT, SURFACE_TYPE_REGULAR,
};

// Protocol Opcodes for zero-doc-lib (0x4D01 .. 0x4D06)
pub const OP_DOC_CREATE:    u64 = 0x4D01;
pub const OP_DOC_WRITE_LINE:u64 = 0x4D02;
pub const OP_DOC_RENDER:    u64 = 0x4D03;
pub const OP_DOC_READ:      u64 = 0x4D04;
pub const OP_DOC_ATTACH_WS: u64 = 0x4D05;
pub const OP_DOC_CLOSE:     u64 = 0x4D06;

pub const MAX_DOC_TITLE_LEN: usize = 32;
pub const MAX_DOC_LINES: usize = 16;
pub const MAX_DOC_LINE_LEN: usize = 64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocArtifactState {
    Unbound = 0,
    Created = 1,
    Rendered = 2,
    Attached = 3,
    Closed = 4,
    Crashed = 5,
}

#[derive(Debug, Clone, Copy)]
pub struct DocLine {
    pub line_idx: u16,
    pub text_len: u16,
    pub text: [u8; MAX_DOC_LINE_LEN],
}

impl DocLine {
    pub const fn empty() -> Self {
        Self {
            line_idx: 0,
            text_len: 0,
            text: [0u8; MAX_DOC_LINE_LEN],
        }
    }
}

/// Durable zero-doc-lib Document Artifact Container handle.
#[derive(Debug, Clone, Copy)]
pub struct ZeroDocArtifact {
    pub artifact_id: u64,
    pub workspace_id: DistributedId,
    pub capability_handle: u32,
    pub is_persistent: bool,
    pub title: [u8; MAX_DOC_TITLE_LEN],
    pub title_len: usize,
    pub state: DocArtifactState,
    pub line_count: usize,
    pub lines: [DocLine; MAX_DOC_LINES],
    pub surface_descriptor: PresentationSurfaceDescriptor,
    pub context_node: ContextNode,
}

impl ZeroDocArtifact {
    pub const fn new() -> Self {
        Self {
            artifact_id: 0,
            workspace_id: DistributedId { node_id: 0, local_seq: 0 },
            capability_handle: 0,
            is_persistent: false,
            title: [0u8; MAX_DOC_TITLE_LEN],
            title_len: 0,
            state: DocArtifactState::Unbound,
            line_count: 0,
            lines: [DocLine::empty(); MAX_DOC_LINES],
            surface_descriptor: PresentationSurfaceDescriptor {
                surface_id: 0,
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                owner_pid: 0,
                shm_handle: 0,
                surface_type: SURFACE_TYPE_REGULAR,
                z_layer: LAYER_WORKSPACE_DEFAULT,
                _pad0: [0; 2],
                x_offset: 0,
                y_offset: 0,
                width: DISPLAY_DEFAULT_WIDTH,
                height: DISPLAY_DEFAULT_HEIGHT,
                scale_factor_q8: 256,
                opacity_pct: 100,
                _padding: [0; 45],
            },
            context_node: ContextNode {
                node_id: 0,
                node_type: ContextNodeType::Document,
                valid: 0,
                _pad0: [0; 2],
                label: [0u8; 32],
                resource_handle: 0,
                _padding: [0; 20],
            },
        }
    }

    /// Creates and binds a new document artifact to a workspace under capability authority.
    pub fn create_artifact(
        &mut self,
        artifact_id: u64,
        workspace_id: DistributedId,
        capability_handle: u32,
        title: &[u8],
        is_persistent: bool,
    ) -> Result<(), ZeroError> {
        if capability_handle == 0 {
            return Err(ZeroError::PermissionDenied); // DOC-F capability validation
        }
        if title.is_empty() {
            return Err(ZeroError::InvalidRequest); // DOC-F malformed input
        }

        self.artifact_id = artifact_id;
        self.workspace_id = workspace_id;
        self.capability_handle = capability_handle;
        self.is_persistent = is_persistent;
        let t_len = title.len().min(MAX_DOC_TITLE_LEN);
        self.title[..t_len].copy_from_slice(&title[..t_len]);
        self.title_len = t_len;
        self.state = DocArtifactState::Created;
        self.line_count = 0;

        // Initialize presentation surface descriptor (DOC-C)
        self.surface_descriptor.surface_id = artifact_id;
        self.surface_descriptor.workspace_id = workspace_id;
        self.surface_descriptor.owner_pid = 1000 + artifact_id;

        // Initialize workspace context node descriptor (DOC-B, DOC-H)
        self.context_node.node_id = artifact_id as u32;
        self.context_node.node_type = ContextNodeType::Document;
        self.context_node.valid = 1;
        self.context_node.label[..t_len].copy_from_slice(&title[..t_len]);
        self.context_node.resource_handle = capability_handle;

        Ok(())
    }

    /// Verifies workspace containment before performing document operations.
    pub fn verify_workspace(&self, caller_workspace_id: DistributedId) -> Result<(), ZeroError> {
        if self.workspace_id != caller_workspace_id {
            return Err(ZeroError::PermissionDenied); // DOC-E workspace rejection
        }
        Ok(())
    }

    /// Writes a line of content to the document artifact.
    pub fn write_line(
        &mut self,
        text: &[u8],
        caller_workspace_id: DistributedId,
    ) -> Result<u16, ZeroError> {
        self.verify_workspace(caller_workspace_id)?;
        if self.state != DocArtifactState::Created && self.state != DocArtifactState::Rendered && self.state != DocArtifactState::Attached {
            return Err(ZeroError::InvalidRequest);
        }
        if self.line_count >= MAX_DOC_LINES {
            return Err(ZeroError::ObjectTableFull);
        }

        let idx = self.line_count as u16;
        let mut line = DocLine::empty();
        line.line_idx = idx;
        let len = text.len().min(MAX_DOC_LINE_LEN);
        line.text[..len].copy_from_slice(&text[..len]);
        line.text_len = len as u16;

        self.lines[self.line_count] = line;
        self.line_count += 1;
        Ok(idx)
    }

    /// Renders the document artifact (e.g. Markdown formatted rendering) into presentation surface.
    pub fn render_to_surface(&mut self, caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace(caller_workspace_id)?;
        if self.line_count == 0 {
            return Err(ZeroError::InvalidRequest);
        }
        self.state = DocArtifactState::Rendered;
        Ok(self.line_count)
    }

    /// Attaches the rendered document artifact to the workspace context node table.
    pub fn attach_to_workspace(&mut self, caller_workspace_id: DistributedId) -> Result<ContextNode, ZeroError> {
        self.verify_workspace(caller_workspace_id)?;
        self.state = DocArtifactState::Attached;
        Ok(self.context_node)
    }

    /// Reads rendered document lines for downstream workload consumption via capability IPC.
    pub fn read_doc_line(&self, line_idx: usize, target: &mut [u8], caller_workspace_id: DistributedId) -> Result<usize, ZeroError> {
        self.verify_workspace(caller_workspace_id)?;
        if line_idx >= self.line_count {
            return Ok(0);
        }
        let line = &self.lines[line_idx];
        let copy_len = target.len().min(line.text_len as usize);
        target[..copy_len].copy_from_slice(&line.text[..copy_len]);
        Ok(copy_len)
    }

    /// Handles application crash cleanly without corrupting workspace state.
    pub fn handle_crash(&mut self) {
        self.state = DocArtifactState::Crashed;
        self.line_count = 0;
    }

    /// Closes document artifact cleanly.
    pub fn close(&mut self) -> Result<(), ZeroError> {
        self.state = DocArtifactState::Closed;
        Ok(())
    }
}

pub fn pack_doc_create_msg(artifact_id: u64, title: &[u8]) -> Result<IpcMessage, ZeroError> {
    let mut payload = [0u8; 48];
    payload[0..8].copy_from_slice(&artifact_id.to_le_bytes());
    let copy_len = title.len().min(40);
    payload[8..8 + copy_len].copy_from_slice(&title[..copy_len]);
    IpcMessage::new(OP_DOC_CREATE, &payload[..8 + copy_len])
}

pub fn pack_doc_write_msg(line_idx: u16, data: &[u8]) -> Result<IpcMessage, ZeroError> {
    let mut payload = [0u8; 48];
    payload[0..2].copy_from_slice(&line_idx.to_le_bytes());
    let copy_len = data.len().min(46);
    payload[2..2 + copy_len].copy_from_slice(&data[..copy_len]);
    IpcMessage::new(OP_DOC_WRITE_LINE, &payload[..2 + copy_len])
}

pub fn pack_doc_close_msg(artifact_id: u64) -> Result<IpcMessage, ZeroError> {
    IpcMessage::new(OP_DOC_CLOSE, &artifact_id.to_le_bytes())
}
