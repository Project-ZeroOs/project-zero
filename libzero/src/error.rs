//! ZeroOS - libzero Error Codes and Definitions

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZeroError {
    Success = 0,
    InvalidSyscall = -1,
    BadAddress = -2,
    InvalidHandle = -3,
    PermissionDenied = -4,
    ObjectTableFull = -5,
    BadMessageSize = -6,
    NotFound = -7,
    AlreadyExists = -8,
    InvalidRequest = -9,
    CapAmplificationRejected = -10,
    GenerationMismatch = -11,
    StaleEndpoint = -12,
    ServiceFailed = -13,
    MaxRetriesExceeded = -14,
    WouldBlock = -15,
    PeerClosed = -16,
    Timeout = -17,
    Unknown = -99,
}

impl ZeroError {
    pub const fn from_i64(code: i64) -> Self {
        match code {
            0 => Self::Success,
            -1 => Self::InvalidSyscall,
            -2 => Self::BadAddress,
            -3 => Self::InvalidHandle,
            -4 => Self::PermissionDenied,
            -5 => Self::ObjectTableFull,
            -6 => Self::BadMessageSize,
            -7 => Self::NotFound,
            -8 => Self::AlreadyExists,
            -9 => Self::InvalidRequest,
            -10 => Self::CapAmplificationRejected,
            -11 => Self::GenerationMismatch,
            -12 => Self::StaleEndpoint,
            -13 => Self::ServiceFailed,
            -14 => Self::MaxRetriesExceeded,
            -15 => Self::WouldBlock,
            -16 => Self::PeerClosed,
            -17 => Self::Timeout,
            _ => Self::Unknown,
        }
    }

    pub const fn as_i64(self) -> i64 {
        self as i32 as i64
    }

    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}
