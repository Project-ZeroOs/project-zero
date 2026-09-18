//! ZeroOS - libzero Freestanding User-Space Runtime Library
//!
//! Authoritative Contract: Stage 4 Architecture Rev6 & Phase 4A Implementation Plan Rev2.
#![no_std]

pub mod error;
pub mod syscall;
pub mod ipc;
pub mod broker;
pub mod registry;
pub mod supervisor;
pub mod escalation;

pub use error::ZeroError;
pub use ipc::{channel_close, channel_create, channel_receive, channel_send, IpcMessage};
pub use broker::{
    delegate_capability, lookup_service, register_service, unregister_service,
    DiscoveredService, ServiceName, ServiceRegistration,
    OP_DELEGATE_CAP, OP_DELEGATE_RESP, OP_LOOKUP_RESP, OP_LOOKUP_SERVICE,
    OP_REGISTER_RESP, OP_REGISTER_SERVICE, OP_SERVICE_STATUS, OP_STATUS_RESP,
    OP_UNREGISTER_RESP, OP_UNREGISTER_SERVICE,
};
pub use registry::{ServiceDirectoryState, ServiceEntry, ServiceRegistry, MAX_SERVICES};
pub use supervisor::{ServiceLifecycleState, SupervisedService, Supervisor, DEFAULT_MAX_RETRIES, MAX_SUPERVISED_SERVICES};
pub use escalation::{EscalationRequest, EscalationState};
pub use syscall::{sys_cap_derive, sys_channel_close, sys_channel_create, sys_channel_receive, sys_channel_send, sys_exit, sys_yield};
