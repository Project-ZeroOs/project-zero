//! ZeroOS - libzero Freestanding IPC Abstractions

use crate::error::ZeroError;
use crate::syscall::{sys_channel_close, sys_channel_create, sys_channel_receive, sys_channel_send};

// Stage 4B Protocol Opcodes (0x2001 .. 0x2014)
pub const OP_RES_REGISTER: u64        = 0x2001;
pub const OP_RES_REGISTER_RESP: u64   = 0x2002;
pub const OP_RES_UNREGISTER: u64      = 0x2003;
pub const OP_RES_UNREGISTER_RESP: u64 = 0x2004;
pub const OP_RES_DISCOVER: u64        = 0x2005;
pub const OP_RES_DISCOVER_RESP: u64   = 0x2006;
pub const OP_RES_QUERY: u64           = 0x2007;
pub const OP_RES_QUERY_RESP: u64      = 0x2008;
pub const OP_LEASE_REQUEST: u64       = 0x2009;
pub const OP_LEASE_REQUEST_RESP: u64  = 0x200A;
pub const OP_LEASE_RENEW: u64         = 0x200B;
pub const OP_LEASE_RENEW_RESP: u64    = 0x200C;
pub const OP_LEASE_RELEASE: u64       = 0x200D;
pub const OP_LEASE_RELEASE_RESP: u64  = 0x200E;
pub const OP_LEASE_RECONCILE: u64     = 0x200F;
pub const OP_LEASE_RECONCILE_RESP: u64 = 0x2010;
pub const OP_QUOTA_QUERY: u64         = 0x2011;
pub const OP_QUOTA_QUERY_RESP: u64    = 0x2012;
pub const OP_ENERGY_GET: u64          = 0x2013;
pub const OP_ENERGY_GET_RESP: u64     = 0x2014;

/// 80-byte frozen IPC message definition (Stage 3G contract).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcMessage {
    /// Message tag / message code (offset 0..8).
    pub tag: u64,
    /// Length of payload in bytes (0..48) (offset 8..10).
    pub payload_len: u16,
    /// Count of attached handles (0..4) (offset 10..12).
    pub handles_count: u16,
    /// Explicit padding for 8-byte alignment (offset 12..16).
    pub _reserved1: u32,
    /// Inline payload data (offset 16..64).
    pub payload: [u8; 48],
    /// Attached handle descriptors transferred with message (offset 64..80).
    pub handles: [u32; 4],
}

const _: () = assert!(core::mem::size_of::<IpcMessage>() == 80);
const _: () = assert!(core::mem::align_of::<IpcMessage>() == 8);

impl IpcMessage {
    pub const fn empty() -> Self {
        Self {
            tag: 0,
            payload_len: 0,
            handles_count: 0,
            _reserved1: 0,
            payload: [0u8; 48],
            handles: [0u32; 4],
        }
    }

    pub fn new(tag: u64, data: &[u8]) -> Result<Self, ZeroError> {
        if data.len() > 48 {
            return Err(ZeroError::BadMessageSize);
        }
        let mut msg = Self::empty();
        msg.tag = tag;
        msg.payload_len = data.len() as u16;
        let mut i = 0;
        while i < data.len() {
            msg.payload[i] = data[i];
            i += 1;
        }
        Ok(msg)
    }

    pub fn with_handles(tag: u64, data: &[u8], handles: &[u32]) -> Result<Self, ZeroError> {
        if data.len() > 48 || handles.len() > 4 {
            return Err(ZeroError::BadMessageSize);
        }
        let mut msg = Self::new(tag, data)?;
        msg.handles_count = handles.len() as u16;
        let mut i = 0;
        while i < handles.len() {
            msg.handles[i] = handles[i];
            i += 1;
        }
        Ok(msg)
    }
}

/// Creates a new bidirectional channel, returning `(endpoint_a, endpoint_b)`.
pub fn channel_create() -> Result<(u32, u32), ZeroError> {
    let mut handles = [0u32; 2];
    let res = unsafe { sys_channel_create(&mut handles as *mut [u32; 2]) };
    if res == 0 {
        Ok((handles[0], handles[1]))
    } else {
        Err(ZeroError::from_i64(res))
    }
}

/// Sends an IPC message over the specified channel handle.
pub fn channel_send(handle: u32, msg: &IpcMessage, non_blocking: bool) -> Result<(), ZeroError> {
    let flags = if non_blocking { 1u32 } else { 0u32 };
    let ptr = msg as *const IpcMessage as *const u8;
    let res = unsafe { sys_channel_send(handle, ptr, flags) };
    if res == 0 {
        Ok(())
    } else {
        Err(ZeroError::from_i64(res))
    }
}

/// Receives an IPC message from the specified channel handle.
pub fn channel_receive(handle: u32, non_blocking: bool) -> Result<IpcMessage, ZeroError> {
    let mut msg = IpcMessage::empty();
    let flags = if non_blocking { 1u32 } else { 0u32 };
    let ptr = &mut msg as *mut IpcMessage as *mut u8;
    let res = unsafe { sys_channel_receive(handle, ptr, flags) };
    if res == 0 {
        Ok(msg)
    } else {
        Err(ZeroError::from_i64(res))
    }
}

/// Closes a channel handle.
pub fn channel_close(handle: u32) -> Result<(), ZeroError> {
    let res = unsafe { sys_channel_close(handle) };
    if res == 0 {
        Ok(())
    } else {
        Err(ZeroError::from_i64(res))
    }
}
