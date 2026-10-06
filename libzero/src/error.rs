//! ZeroOS - libzero Error Codes and Definitions
//!
//! Authoritative Contract: Stage 4A & Stage 4B Architecture Rev12.

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
    UnsupportedResourceShape = -18,
    DimensionLimitExceeded = -19,
    VectorConservationViolated = -20,
    CouplingViolation = -21,
    TimeAuthorityUnavailable = -22,
    TimeObservationStale = -23,
    HardwareRegressionDetected = -24,
    EpochExhaustion = -25,
    DeadlineExhaustion = -26,
    IdentifierExhausted = -27,
    CapacityQuarantined = -28,
    ProviderConfirmationRequired = -29,
    QuotaExceeded = -30,
    CyclicDependency = -31,
    StaleSpatialIndex = -32,
    StaleSessionEpoch = -33,
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
            -18 => Self::UnsupportedResourceShape,
            -19 => Self::DimensionLimitExceeded,
            -20 => Self::VectorConservationViolated,
            -21 => Self::CouplingViolation,
            -22 => Self::TimeAuthorityUnavailable,
            -23 => Self::TimeObservationStale,
            -24 => Self::HardwareRegressionDetected,
            -25 => Self::EpochExhaustion,
            -26 => Self::DeadlineExhaustion,
            -27 => Self::IdentifierExhausted,
            -28 => Self::CapacityQuarantined,
            -29 => Self::ProviderConfirmationRequired,
            -30 => Self::QuotaExceeded,
            -31 => Self::CyclicDependency,
            -32 => Self::StaleSpatialIndex,
            -33 => Self::StaleSessionEpoch,
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
