//! Project Zero - Stage 3G Inter-Process Communication (IPC) Core Definitions
//!
//! Authoritative Contract: Stage 3G Architecture Rev10 (Approved & Frozen).

/// Monotonically increasing unique 64-bit object identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

/// Error codes returned by IPC and Kernel Object subsystems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    InvalidHandle,
    BadHandleGeneration,
    PermissionDenied,
    ObjectTableFull,
    ChannelTableFull,
    ShmTableFull,
    MappingTableFull,
    HandleTableFull,
    ObjectIdExhausted,
    InvalidProcess,
    InvalidEndpoint,
    EndpointClosed,
    PeerClosed,
    WouldBlock,
    ConcurrentOperation,
    BadMessageSize,
    BadAlignment,
    InvalidAddress,
    PmmExhausted,
    VmmError,
    Timeout,
    // Stage 3H Capability Specific Codes
    CapabilityRevoked,
    RightsAmplificationRejected,
    MaxDerivationDepthExceeded,
    NotCapabilityOwner,
    CapabilityNotTransferable,
    CapabilityNotDuplicable,
    CapabilityIdExhausted,
    InvalidCapability,
}

/// Access rights bitmask for Handles.
pub mod rights {
    pub const READ: u16 = 1 << 0;
    pub const WRITE: u16 = 1 << 1;
    pub const MAP_RO: u16 = 1 << 2;
    pub const MAP_RW: u16 = 1 << 3;
    pub const TRANSFER: u16 = 1 << 4;
    pub const DUPLICATE: u16 = 1 << 5;
    pub const SIGNAL: u16 = 1 << 6;
    pub const WAIT: u16 = 1 << 7;
}

/// Signals asserted on kernel objects.
pub mod signals {
    pub const READABLE: u32 = 1 << 0;
    pub const WRITABLE: u32 = 1 << 1;
    pub const PEER_CLOSED: u32 = 1 << 2;
    pub const OBJECT_DESTROYED: u32 = 1 << 3;
}

/// 80-byte frozen IPC message definition.
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

    pub fn new(tag: u64, data: &[u8]) -> Result<Self, IpcError> {
        if data.len() > 48 {
            return Err(IpcError::BadMessageSize);
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

    pub fn with_handles(tag: u64, data: &[u8], handles: &[u32]) -> Result<Self, IpcError> {
        if data.len() > 48 || handles.len() > 4 {
            return Err(IpcError::BadMessageSize);
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

    pub fn from_words(tag: u64, words: [u64; 6]) -> Self {
        let mut msg = Self::empty();
        msg.tag = tag;
        msg.payload_len = 48;
        for i in 0..6 {
            let bytes = words[i].to_le_bytes();
            for b in 0..8 {
                msg.payload[i * 8 + b] = bytes[b];
            }
        }
        msg
    }
}

