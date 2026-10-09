//! ZeroOS - libzero Persistence Authority Interface
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Mediates non-volatile persistence via capability-backed slot abstraction.

use crate::error::ZeroError;

pub const BOOT_EPOCH_SLOT: u32 = 0;
pub const DISTRIBUTED_ID_CEILING_SLOT: u32 = 1;

/// Trait representing an authoritative persistence provider.
/// Implementations must guarantee that write_and_commit_slot only returns
/// Ok(()) after durable media commit / flush barriers complete.
pub trait PersistenceAuthority {
    /// Read the persisted 64-bit value at the authorized slot.
    fn read_slot(&self, slot: u32) -> Result<u64, ZeroError>;

    /// Commit a new 64-bit value to durable non-volatile media.
    /// Must not return Ok(()) until media flush / non-volatile commit barrier completes.
    fn write_and_commit_slot(&mut self, slot: u32, value: u64) -> Result<(), ZeroError>;
}

/// In-memory persistence authority for tests and volatile execution environments.
#[derive(Clone, Copy, Debug)]
pub struct MemoryPersistenceAuthority {
    slots: [u64; 512],
    write_barrier_count: u64,
}

impl MemoryPersistenceAuthority {
    pub const fn new() -> Self {
        Self {
            slots: [0u64; 512],
            write_barrier_count: 0,
        }
    }

    pub fn with_initial_values(boot_epoch: u64, id_ceiling: u64) -> Self {
        let mut auth = Self::new();
        auth.slots[BOOT_EPOCH_SLOT as usize] = boot_epoch;
        auth.slots[DISTRIBUTED_ID_CEILING_SLOT as usize] = id_ceiling;
        auth
    }

    pub fn barrier_count(&self) -> u64 {
        self.write_barrier_count
    }
}

impl Default for MemoryPersistenceAuthority {
    fn default() -> Self {
        Self::new()
    }
}

impl PersistenceAuthority for MemoryPersistenceAuthority {
    fn read_slot(&self, slot: u32) -> Result<u64, ZeroError> {
        if (slot as usize) < self.slots.len() {
            Ok(self.slots[slot as usize])
        } else {
            Err(ZeroError::InvalidRequest)
        }
    }

    fn write_and_commit_slot(&mut self, slot: u32, value: u64) -> Result<(), ZeroError> {
        if (slot as usize) < self.slots.len() {
            self.slots[slot as usize] = value;
            self.write_barrier_count += 1;
            Ok(())
        } else {
            Err(ZeroError::InvalidRequest)
        }
    }
}

pub const VFS_SESSION_SNAPSHOT_PATH: &str = "/var/session/snapshot.bin";

/// Standard IEEE 802.3 CRC32 calculation.
pub fn calculate_crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Durable VFS Session Snapshot Block layout for ZeroOS v1.0 recovery across reset.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct VfsSessionSnapshotBlock {
    pub header: crate::grounding::LogicalSessionSnapshotHeader,
    pub session_record: crate::session::SessionRecord,
    pub workspace_count: u32,
    pub _pad0: u32,
    pub workspaces: [crate::workspace::WorkspaceControlBlock; crate::session::MAX_WORKSPACES_PER_SESSION],
}

impl Default for VfsSessionSnapshotBlock {
    fn default() -> Self {
        Self {
            header: crate::grounding::LogicalSessionSnapshotHeader::default(),
            session_record: crate::session::SessionRecord::default(),
            workspace_count: 0,
            _pad0: 0,
            workspaces: [crate::workspace::WorkspaceControlBlock::default(); crate::session::MAX_WORKSPACES_PER_SESSION],
        }
    }
}

impl VfsSessionSnapshotBlock {
    /// Computes CRC32 checksum over the snapshot payload bytes (session_record + workspaces).
    pub fn compute_payload_crc32(&self) -> u32 {
        let payload_ptr = &self.session_record as *const crate::session::SessionRecord as *const u8;
        let payload_len = core::mem::size_of::<crate::session::SessionRecord>()
            + 8
            + (self.workspace_count as usize * core::mem::size_of::<crate::workspace::WorkspaceControlBlock>());
        let slice = unsafe { core::slice::from_raw_parts(payload_ptr, payload_len) };
        calculate_crc32(slice)
    }

    /// Validates snapshot header magic, payload CRC32 checksum, and epoch bounds.
    pub fn validate(&self, max_allowed_epoch: u64) -> Result<(), ZeroError> {
        if self.header.magic != crate::grounding::SESSION_SNAPSHOT_MAGIC {
            return Err(ZeroError::InvalidRequest);
        }
        if self.header.session_migration_epoch > max_allowed_epoch {
            return Err(ZeroError::StaleSessionEpoch);
        }
        let expected_crc = self.compute_payload_crc32();
        if self.header.payload_crc32 != expected_crc {
            return Err(ZeroError::InvalidRequest);
        }
        Ok(())
    }
}

pub static mut GLOBAL_SNAPSHOT_BUF: VfsSessionSnapshotBlock = VfsSessionSnapshotBlock {
    header: crate::grounding::LogicalSessionSnapshotHeader {
        magic: crate::grounding::SESSION_SNAPSHOT_MAGIC,
        session_migration_epoch: 0,
        dest_transaction_nonce: 0,
        session_id: 0,
        source_node_id: 0,
        dest_node_id: 0,
        workspace_count: 0,
        surface_count: 0,
        payload_crc32: 0,
        _reserved: 0,
    },
    session_record: crate::session::SessionRecord {
        user_id: 0,
        session_id: crate::resource::DistributedId { node_id: 0, local_seq: 0 },
        state: 0,
        layout_policy: 0,
        active_workspace_count: 0,
        _pad0: 0,
        active_workspace_id: crate::resource::DistributedId { node_id: 0, local_seq: 0 },
        authorized_workspaces: [crate::resource::DistributedId { node_id: 0, local_seq: 0 }; crate::session::MAX_WORKSPACES_PER_SESSION],
    },
    workspace_count: 0,
    _pad0: 0,
    workspaces: [crate::workspace::WorkspaceControlBlock {
        workspace_id: crate::resource::DistributedId { node_id: 0, local_seq: 0 },
        owner_pid: 0,
        generation: 0,
        state: crate::workspace::WorkspaceState::Unallocated,
        _pad0: [0; 3],
        capability_envelope_handle: 0,
        active_workload_count: 0,
        active_agent_count: 0,
        membership_count: 0,
        associated_workloads: [crate::resource::DistributedId { node_id: 0, local_seq: 0 }; crate::workspace::MAX_WORKLOADS_PER_WORKSPACE],
        associated_agents: [crate::resource::DistributedId { node_id: 0, local_seq: 0 }; crate::workspace::MAX_AGENTS_PER_WORKSPACE],
        intent_node_count: 0,
        intent_dep_count: 0,
        root_dir_handle: 0,
        resident_node_count: 0,
        resident_edge_count: 0,
        _padding: [0; 196],
    }; crate::session::MAX_WORKSPACES_PER_SESSION],
};

pub struct VfsSessionJournal;

impl VfsSessionJournal {
    /// Builds snapshot in target buffer with computed CRC32 checksum.
    pub unsafe fn build_snapshot_in_buf(
        epoch: u64,
        session: &crate::session::SessionRecord,
        workspaces: &[crate::workspace::WorkspaceControlBlock],
        buf: *mut VfsSessionSnapshotBlock,
    ) {
        let ws_count = workspaces.len().min(crate::session::MAX_WORKSPACES_PER_SESSION);
        (*buf).session_record = *session;
        (*buf).workspace_count = ws_count as u32;
        for i in 0..ws_count {
            (*buf).workspaces[i] = workspaces[i];
        }

        let payload_crc = (*buf).compute_payload_crc32();

        (*buf).header = crate::grounding::LogicalSessionSnapshotHeader {
            magic: crate::grounding::SESSION_SNAPSHOT_MAGIC,
            session_migration_epoch: epoch,
            dest_transaction_nonce: 0,
            session_id: session.session_id.local_seq,
            source_node_id: session.session_id.node_id,
            dest_node_id: session.session_id.node_id,
            workspace_count: ws_count as u32,
            surface_count: session.active_workspace_count as u32,
            payload_crc32: payload_crc,
            _reserved: 0,
        };
    }

    /// Commits session snapshot to non-volatile persistence authority slots.
    pub fn commit_to_authority<P: PersistenceAuthority>(
        auth: &mut P,
        epoch: u64,
        session: &crate::session::SessionRecord,
        workspaces: &[crate::workspace::WorkspaceControlBlock],
    ) -> Result<(), ZeroError> {
        unsafe {
            let buf = &raw mut GLOBAL_SNAPSHOT_BUF;
            Self::build_snapshot_in_buf(epoch, session, workspaces, buf);
            let raw_ptr = buf as *const u64;
            let ws_count = (*buf).workspace_count as usize;
            let payload_words = (core::mem::size_of::<crate::session::SessionRecord>()
                + 8
                + (ws_count * core::mem::size_of::<crate::workspace::WorkspaceControlBlock>())) / 8;
            let header_words = core::mem::size_of::<crate::grounding::LogicalSessionSnapshotHeader>() / 8;
            let total_words = header_words + payload_words;

            for i in 0..total_words {
                let val = *raw_ptr.add(i);
                auth.write_and_commit_slot(2 + i as u32, val)?;
            }
        }
        Ok(())
    }

    /// Restores session snapshot into target static buffer and validates checksum.
    pub fn restore_from_authority<P: PersistenceAuthority>(
        auth: &P,
        current_epoch: u64,
    ) -> Result<(*const crate::session::SessionRecord, usize), ZeroError> {
        unsafe {
            let buf = &raw mut GLOBAL_SNAPSHOT_BUF;
            let raw_ptr = buf as *mut u64;
            let header_words = core::mem::size_of::<crate::grounding::LogicalSessionSnapshotHeader>() / 8;
            let base_payload_words = (core::mem::size_of::<crate::session::SessionRecord>() + 8) / 8;

            // Read header, session record, and workspace_count
            for i in 0..(header_words + base_payload_words) {
                let val = auth.read_slot(2 + i as u32)?;
                *raw_ptr.add(i) = val;
            }

            let ws_count = ((*buf).workspace_count as usize).min(crate::session::MAX_WORKSPACES_PER_SESSION);
            let total_words = header_words + base_payload_words + (ws_count * core::mem::size_of::<crate::workspace::WorkspaceControlBlock>()) / 8;

            for i in (header_words + base_payload_words)..total_words {
                let val = auth.read_slot(2 + i as u32)?;
                *raw_ptr.add(i) = val;
            }

            (*buf).validate(current_epoch)?;
            Ok((&(*buf).session_record as *const _, ws_count))
        }
    }
}
