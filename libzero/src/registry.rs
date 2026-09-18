//! ZeroOS - libzero Bounded Service Registry & Directory State

use crate::error::ZeroError;
use crate::ipc::IpcMessage;
use crate::syscall::sys_cap_derive;
use crate::broker::{
    OP_DELEGATE_CAP, OP_DELEGATE_RESP, OP_LOOKUP_RESP, OP_LOOKUP_SERVICE,
    OP_REGISTER_RESP, OP_REGISTER_SERVICE, OP_SERVICE_STATUS, OP_STATUS_RESP,
    OP_UNREGISTER_RESP, OP_UNREGISTER_SERVICE,
};

pub const MAX_SERVICES: usize = 16;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceDirectoryState {
    Unused = 0,
    Active = 1,
    Stale = 2,
    Suspended = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceEntry {
    pub occupied: bool,
    /// Directory identity: unique, monotonic, independent of array slot.
    pub service_id: u32,
    pub name: [u8; 32],
    /// Service instance identity: incremented on every registration/restart.
    pub generation: u32,
    pub state: ServiceDirectoryState,
    /// Ephemeral process-local capability handle in brokerd's handle table.
    /// Invariant: Must never be treated as global service identity.
    pub broker_local_endpoint: u32,
    pub required_rights: u16,
}

impl ServiceEntry {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            service_id: 0,
            name: [0u8; 32],
            generation: 0,
            state: ServiceDirectoryState::Unused,
            broker_local_endpoint: 0,
            required_rights: 0,
        }
    }
}

pub struct ServiceRegistry {
    pub entries: [ServiceEntry; MAX_SERVICES],
    pub next_service_id: u32,
}

impl ServiceRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [ServiceEntry::empty(); MAX_SERVICES],
            next_service_id: 100, // Decoupled from slot index (starts at 100)
        }
    }

    /// Registers or restarts a service.
    ///
    /// If a service with `name` already exists, advances its generation and updates its
    /// endpoint handle (closing the old handle). Otherwise, allocates a new `ServiceId`.
    pub fn register(
        &mut self,
        name: &[u8; 32],
        endpoint_handle: u32,
        required_rights: u16,
    ) -> Result<(u32, u32), ZeroError> {
        // 1. Check if service already exists (restart or update)
        for entry in self.entries.iter_mut() {
            if entry.occupied && &entry.name == name {
                entry.generation = entry.generation.wrapping_add(1);
                entry.broker_local_endpoint = endpoint_handle;
                entry.required_rights = required_rights;
                entry.state = ServiceDirectoryState::Active;
                return Ok((entry.service_id, entry.generation));
            }
        }

        // 2. Allocate new entry
        for entry in self.entries.iter_mut() {
            if !entry.occupied {
                entry.occupied = true;
                entry.service_id = self.next_service_id;
                self.next_service_id = self.next_service_id.wrapping_add(1);
                entry.name = *name;
                entry.generation = 1;
                entry.state = ServiceDirectoryState::Active;
                entry.broker_local_endpoint = endpoint_handle;
                entry.required_rights = required_rights;
                return Ok((entry.service_id, entry.generation));
            }
        }

        Err(ZeroError::ObjectTableFull)
    }

    /// Unregisters an existing service instance, verifying generation match.
    pub fn unregister(&mut self, service_id: u32, generation: u32) -> Result<(), ZeroError> {
        for entry in self.entries.iter_mut() {
            if entry.occupied && entry.service_id == service_id {
                if entry.generation != generation {
                    return Err(ZeroError::GenerationMismatch);
                }
                *entry = ServiceEntry::empty();
                return Ok(());
            }
        }
        Err(ZeroError::NotFound)
    }

    /// Discovers an active service by name.
    pub fn lookup(&self, name: &[u8; 32]) -> Result<&ServiceEntry, ZeroError> {
        for entry in self.entries.iter() {
            if entry.occupied && &entry.name == name {
                if entry.state == ServiceDirectoryState::Active {
                    return Ok(entry);
                } else {
                    return Err(ZeroError::StaleEndpoint);
                }
            }
        }
        Err(ZeroError::NotFound)
    }

    /// Look up by ServiceId.
    pub fn lookup_by_id(&self, service_id: u32) -> Result<&ServiceEntry, ZeroError> {
        for entry in self.entries.iter() {
            if entry.occupied && entry.service_id == service_id {
                return Ok(entry);
            }
        }
        Err(ZeroError::NotFound)
    }

    /// Dispatches an incoming broker protocol message and generates an authoritative response.
    pub fn handle_message(&mut self, msg: &IpcMessage) -> IpcMessage {
        match msg.tag {
            OP_REGISTER_SERVICE => {
                if msg.payload_len < 34 || msg.handles_count == 0 {
                    let mut resp = IpcMessage::empty();
                    resp.tag = OP_REGISTER_RESP;
                    resp.payload_len = 4;
                    resp.payload[0..4].copy_from_slice(&ZeroError::InvalidRequest.as_i32().to_le_bytes());
                    return resp;
                }

                let mut name = [0u8; 32];
                name.copy_from_slice(&msg.payload[0..32]);
                let req_rights = u16::from_le_bytes([msg.payload[32], msg.payload[33]]);
                let endpoint = msg.handles[0];

                let mut resp = IpcMessage::empty();
                resp.tag = OP_REGISTER_RESP;
                resp.payload_len = 12;

                match self.register(&name, endpoint, req_rights) {
                    Ok((s_id, gen)) => {
                        resp.payload[0..4].copy_from_slice(&0i32.to_le_bytes());
                        resp.payload[4..8].copy_from_slice(&s_id.to_le_bytes());
                        resp.payload[8..12].copy_from_slice(&gen.to_le_bytes());
                    }
                    Err(e) => {
                        resp.payload[0..4].copy_from_slice(&e.as_i32().to_le_bytes());
                    }
                }
                resp
            }

            OP_LOOKUP_SERVICE => {
                if msg.payload_len < 32 {
                    let mut resp = IpcMessage::empty();
                    resp.tag = OP_LOOKUP_RESP;
                    resp.payload_len = 4;
                    resp.payload[0..4].copy_from_slice(&ZeroError::InvalidRequest.as_i32().to_le_bytes());
                    return resp;
                }

                let mut name = [0u8; 32];
                name.copy_from_slice(&msg.payload[0..32]);

                let mut resp = IpcMessage::empty();
                resp.tag = OP_LOOKUP_RESP;

                match self.lookup(&name) {
                    Ok(entry) => {
                        resp.payload_len = 14;
                        resp.payload[0..4].copy_from_slice(&0i32.to_le_bytes());
                        resp.payload[4..8].copy_from_slice(&entry.service_id.to_le_bytes());
                        resp.payload[8..12].copy_from_slice(&entry.generation.to_le_bytes());
                        resp.payload[12..14].copy_from_slice(&entry.required_rights.to_le_bytes());
                        // Transfer broker's local endpoint capability to client via handles[0]
                        resp.handles_count = 1;
                        resp.handles[0] = entry.broker_local_endpoint;
                    }
                    Err(e) => {
                        resp.payload_len = 4;
                        resp.payload[0..4].copy_from_slice(&e.as_i32().to_le_bytes());
                    }
                }
                resp
            }

            OP_UNREGISTER_SERVICE => {
                if msg.payload_len < 8 {
                    let mut resp = IpcMessage::empty();
                    resp.tag = OP_UNREGISTER_RESP;
                    resp.payload_len = 4;
                    resp.payload[0..4].copy_from_slice(&ZeroError::InvalidRequest.as_i32().to_le_bytes());
                    return resp;
                }

                let service_id = u32::from_le_bytes([msg.payload[0], msg.payload[1], msg.payload[2], msg.payload[3]]);
                let generation = u32::from_le_bytes([msg.payload[4], msg.payload[5], msg.payload[6], msg.payload[7]]);

                let mut resp = IpcMessage::empty();
                resp.tag = OP_UNREGISTER_RESP;
                resp.payload_len = 4;

                match self.unregister(service_id, generation) {
                    Ok(()) => {
                        resp.payload[0..4].copy_from_slice(&0i32.to_le_bytes());
                    }
                    Err(e) => {
                        resp.payload[0..4].copy_from_slice(&e.as_i32().to_le_bytes());
                    }
                }
                resp
            }

            OP_SERVICE_STATUS => {
                if msg.payload_len < 4 {
                    let mut resp = IpcMessage::empty();
                    resp.tag = OP_STATUS_RESP;
                    resp.payload_len = 4;
                    resp.payload[0..4].copy_from_slice(&ZeroError::InvalidRequest.as_i32().to_le_bytes());
                    return resp;
                }

                let service_id = u32::from_le_bytes([msg.payload[0], msg.payload[1], msg.payload[2], msg.payload[3]]);

                let mut resp = IpcMessage::empty();
                resp.tag = OP_STATUS_RESP;

                match self.lookup_by_id(service_id) {
                    Ok(entry) => {
                        resp.payload_len = 12;
                        resp.payload[0..4].copy_from_slice(&0i32.to_le_bytes());
                        resp.payload[4..8].copy_from_slice(&entry.generation.to_le_bytes());
                        resp.payload[8..12].copy_from_slice(&(entry.state as u32).to_le_bytes());
                    }
                    Err(e) => {
                        resp.payload_len = 4;
                        resp.payload[0..4].copy_from_slice(&e.as_i32().to_le_bytes());
                    }
                }
                resp
            }

            OP_DELEGATE_CAP => {
                if msg.payload_len < 8 || msg.handles_count == 0 {
                    let mut resp = IpcMessage::empty();
                    resp.tag = OP_DELEGATE_RESP;
                    resp.payload_len = 4;
                    resp.payload[0..4].copy_from_slice(&ZeroError::InvalidRequest.as_i32().to_le_bytes());
                    return resp;
                }

                let parent_cap = msg.handles[0];
                let requested_rights = u32::from_le_bytes([msg.payload[4], msg.payload[5], msg.payload[6], msg.payload[7]]);

                // Kernel capability derivation invocation (Stage 3H authoritative check)
                let mut derived_cap = 0u32;
                let k_res = unsafe { sys_cap_derive(parent_cap, requested_rights, &mut derived_cap) };

                let mut resp = IpcMessage::empty();
                resp.tag = OP_DELEGATE_RESP;
                resp.payload_len = 4;

                if k_res == 0 {
                    resp.payload[0..4].copy_from_slice(&0i32.to_le_bytes());
                    resp.handles_count = 1;
                    resp.handles[0] = derived_cap;
                } else {
                    let err = ZeroError::from_i64(k_res);
                    resp.payload[0..4].copy_from_slice(&err.as_i32().to_le_bytes());
                }
                resp
            }

            _ => {
                // Unknown / malformed opcode
                let mut resp = IpcMessage::empty();
                resp.tag = msg.tag.wrapping_add(1);
                resp.payload_len = 4;
                resp.payload[0..4].copy_from_slice(&ZeroError::InvalidRequest.as_i32().to_le_bytes());
                resp
            }
        }
    }
}
