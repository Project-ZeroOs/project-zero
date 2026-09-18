//! ZeroOS - libzero Service Broker Protocol & Client Interface

use crate::error::ZeroError;
use crate::ipc::{channel_receive, channel_send, IpcMessage};

// Protocol Opcodes
pub const OP_REGISTER_SERVICE: u64 = 0x1001;
pub const OP_REGISTER_RESP:    u64 = 0x1002;
pub const OP_UNREGISTER_SERVICE: u64 = 0x1003;
pub const OP_UNREGISTER_RESP:    u64 = 0x1004;
pub const OP_LOOKUP_SERVICE:   u64 = 0x1005;
pub const OP_LOOKUP_RESP:      u64 = 0x1006;
pub const OP_SERVICE_STATUS:   u64 = 0x1007;
pub const OP_STATUS_RESP:      u64 = 0x1008;
pub const OP_DELEGATE_CAP:     u64 = 0x1009;
pub const OP_DELEGATE_RESP:    u64 = 0x100A;

/// Fixed-size 32-byte Service Name.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceName(pub [u8; 32]);

impl ServiceName {
    pub const fn empty() -> Self {
        Self([0u8; 32])
    }

    pub fn from_str(s: &str) -> Self {
        let mut name = [0u8; 32];
        let bytes = s.as_bytes();
        let len = if bytes.len() > 32 { 32 } else { bytes.len() };
        let mut i = 0;
        while i < len {
            name[i] = bytes[i];
            i += 1;
        }
        Self(name)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Service Registration Record (Directory state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceRegistration {
    pub service_id: u32,
    pub generation: u32,
    pub required_rights: u16,
}

/// Register a service with `brokerd`.
///
/// Passes `endpoint_handle` via `IpcMessage.handles[0]`. The kernel translates
/// this handle into `brokerd`'s process-local handle table.
pub fn register_service(
    broker_chan: u32,
    name: &str,
    endpoint_handle: u32,
    required_rights: u16,
) -> Result<ServiceRegistration, ZeroError> {
    let mut payload = [0u8; 48];
    let s_name = ServiceName::from_str(name);
    payload[0..32].copy_from_slice(s_name.as_bytes());
    payload[32..34].copy_from_slice(&required_rights.to_le_bytes());

    let msg = IpcMessage::with_handles(OP_REGISTER_SERVICE, &payload[0..34], &[endpoint_handle])?;
    channel_send(broker_chan, &msg, false)?;

    let resp = channel_receive(broker_chan, false)?;
    if resp.tag != OP_REGISTER_RESP || resp.payload_len < 12 {
        return Err(ZeroError::InvalidRequest);
    }

    let status = i32::from_le_bytes([resp.payload[0], resp.payload[1], resp.payload[2], resp.payload[3]]);
    if status != 0 {
        return Err(ZeroError::from_i64(status as i64));
    }

    let service_id = u32::from_le_bytes([resp.payload[4], resp.payload[5], resp.payload[6], resp.payload[7]]);
    let generation = u32::from_le_bytes([resp.payload[8], resp.payload[9], resp.payload[10], resp.payload[11]]);

    Ok(ServiceRegistration {
        service_id,
        generation,
        required_rights,
    })
}

/// Discovered Service metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveredService {
    pub service_id: u32,
    pub generation: u32,
    /// Client-local endpoint handle transferred by the kernel.
    pub client_endpoint_handle: u32,
    pub required_rights: u16,
}

/// Lookup a service by name in `brokerd`.
///
/// On success, `brokerd` transfers an endpoint capability handle via `IpcMessage.handles[0]`.
/// The kernel installs this handle into the calling process's handle table.
pub fn lookup_service(broker_chan: u32, name: &str) -> Result<DiscoveredService, ZeroError> {
    let mut payload = [0u8; 48];
    let s_name = ServiceName::from_str(name);
    payload[0..32].copy_from_slice(s_name.as_bytes());

    let msg = IpcMessage::new(OP_LOOKUP_SERVICE, &payload[0..32])?;
    channel_send(broker_chan, &msg, false)?;

    let resp = channel_receive(broker_chan, false)?;
    if resp.tag != OP_LOOKUP_RESP || resp.payload_len < 14 {
        return Err(ZeroError::InvalidRequest);
    }

    let status = i32::from_le_bytes([resp.payload[0], resp.payload[1], resp.payload[2], resp.payload[3]]);
    if status != 0 {
        return Err(ZeroError::from_i64(status as i64));
    }

    if resp.handles_count == 0 {
        return Err(ZeroError::InvalidHandle);
    }

    let service_id = u32::from_le_bytes([resp.payload[4], resp.payload[5], resp.payload[6], resp.payload[7]]);
    let generation = u32::from_le_bytes([resp.payload[8], resp.payload[9], resp.payload[10], resp.payload[11]]);
    let required_rights = u16::from_le_bytes([resp.payload[12], resp.payload[13]]);
    let client_endpoint_handle = resp.handles[0];

    Ok(DiscoveredService {
        service_id,
        generation,
        client_endpoint_handle,
        required_rights,
    })
}

/// Unregister a service from `brokerd`.
pub fn unregister_service(broker_chan: u32, service_id: u32, generation: u32) -> Result<(), ZeroError> {
    let mut payload = [0u8; 8];
    payload[0..4].copy_from_slice(&service_id.to_le_bytes());
    payload[4..8].copy_from_slice(&generation.to_le_bytes());

    let msg = IpcMessage::new(OP_UNREGISTER_SERVICE, &payload)?;
    channel_send(broker_chan, &msg, false)?;

    let resp = channel_receive(broker_chan, false)?;
    if resp.tag != OP_UNREGISTER_RESP || resp.payload_len < 4 {
        return Err(ZeroError::InvalidRequest);
    }

    let status = i32::from_le_bytes([resp.payload[0], resp.payload[1], resp.payload[2], resp.payload[3]]);
    if status != 0 {
        return Err(ZeroError::from_i64(status as i64));
    }

    Ok(())
}

/// Request capability delegation via `brokerd`.
///
/// Sends `parent_cap` handle. `brokerd` validates its local directory policy,
/// executes `sys_cap_derive()`, and returns the derived handle via `IpcMessage.handles[0]`.
/// The kernel authoritatively enforces monotonic attenuation.
pub fn delegate_capability(
    broker_chan: u32,
    service_id: u32,
    parent_cap: u32,
    requested_rights: u32,
) -> Result<u32, ZeroError> {
    let mut payload = [0u8; 8];
    payload[0..4].copy_from_slice(&service_id.to_le_bytes());
    payload[4..8].copy_from_slice(&requested_rights.to_le_bytes());

    let msg = IpcMessage::with_handles(OP_DELEGATE_CAP, &payload, &[parent_cap])?;
    channel_send(broker_chan, &msg, false)?;

    let resp = channel_receive(broker_chan, false)?;
    if resp.tag != OP_DELEGATE_RESP || resp.payload_len < 4 {
        return Err(ZeroError::InvalidRequest);
    }

    let status = i32::from_le_bytes([resp.payload[0], resp.payload[1], resp.payload[2], resp.payload[3]]);
    if status != 0 {
        return Err(ZeroError::from_i64(status as i64));
    }

    if resp.handles_count == 0 {
        return Err(ZeroError::InvalidHandle);
    }

    Ok(resp.handles[0])
}
