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
pub mod resource;
pub mod lease;
pub mod time;
pub mod persistence;
pub mod identity;
pub mod accounting;
pub mod graph;
pub mod lease_engine;
pub mod workload;
pub mod workspace;
pub mod agent;

pub use error::ZeroError;
pub use ipc::{
    channel_close, channel_create, channel_receive, channel_send, IpcMessage,
    OP_ENERGY_GET, OP_ENERGY_GET_RESP, OP_LEASE_RECONCILE, OP_LEASE_RECONCILE_RESP,
    OP_LEASE_RELEASE, OP_LEASE_RELEASE_RESP, OP_LEASE_RENEW, OP_LEASE_RENEW_RESP,
    OP_LEASE_REQUEST, OP_LEASE_REQUEST_RESP, OP_QUOTA_QUERY, OP_QUOTA_QUERY_RESP,
    OP_RES_DISCOVER, OP_RES_DISCOVER_RESP, OP_RES_QUERY, OP_RES_QUERY_RESP,
    OP_RES_REGISTER, OP_RES_REGISTER_RESP, OP_RES_UNREGISTER, OP_RES_UNREGISTER_RESP,
};
pub use broker::{
    delegate_capability, lookup_service, register_service, unregister_service,
    DiscoveredService, ServiceName, ServiceRegistration,
    OP_DELEGATE_CAP, OP_DELEGATE_RESP, OP_LOOKUP_RESP, OP_LOOKUP_SERVICE,
    OP_REGISTER_RESP, OP_REGISTER_SERVICE, OP_STATUS_RESP, OP_UNREGISTER_RESP, OP_UNREGISTER_SERVICE,
};
pub use registry::{ServiceDirectoryState, ServiceEntry, ServiceRegistry, MAX_SERVICES};
pub use supervisor::{ServiceLifecycleState, SupervisedService, Supervisor, DEFAULT_MAX_RETRIES, MAX_SUPERVISED_SERVICES};
pub use escalation::{EscalationRequest, EscalationState};
pub use syscall::{sys_cap_derive, sys_channel_close, sys_channel_create, sys_channel_receive, sys_channel_send, sys_exit, sys_yield};
pub use persistence::{PersistenceAuthority, MemoryPersistenceAuthority, BOOT_EPOCH_SLOT, DISTRIBUTED_ID_CEILING_SLOT};
pub use resource::{
    DimensionCapacityVector, DistributedId, EnergyTier, LocalityDomain, ResourceDescriptor,
    ResourceState, ResourceType, MAX_ACCOUNTING_DIMENSIONS, MAX_RESOURCES_PER_NODE,
};
pub use lease::{
    CouplingConstraintMatrix, LeaseState, ResourceLease, MAX_LEASES_PER_NODE, MAX_QUOTA_ENTITIES,
};
pub use time::{
    evaluate_freshness, read_canonical_tsc, TimeObservation, TimeObservationFrame,
    MAX_TIME_OBSERVATION_AGE_MS, QUAL_FLAG_CALIBRATED, QUAL_FLAG_INVARIANT_TSC,
    QUAL_FLAG_SMP_SYNCED, SEQUENCE_MAX_VALID_EVEN, SEQUENCE_TERMINAL_LATCH,
};
pub use identity::DistributedIdAllocator;
pub use accounting::{AccountingDomain, AccountingManager, EntityQuota, QuarantineRecord};
pub use graph::{ResourceGraph, TopologyNode};
pub use lease_engine::LeaseEngine;
pub use workload::{
    ProcessSpawnRequest, ProcessSpawnResponse, RecoveryClass, TaskDag, TaskDescriptor,
    TaskResourceDemand, TaskState, WorkloadControlBlock, WorkloadState, DEFAULT_MAX_TASK_RETRIES,
    MAX_CONCURRENT_RUNNING_TASKS, MAX_CONCURRENT_WORKLOADS, MAX_LEASES_PER_WORKLOAD,
    MAX_TASKS_PER_WORKLOAD, OP_PROCESS_EXIT_NOTIFY, OP_PROCESS_SPAWN, OP_PROCESS_SPAWN_RESP,
    OP_PROCESS_TERMINATE, OP_PROCESS_TERMINATE_RESP, OP_WORKLOAD_CANCEL, OP_WORKLOAD_CANCEL_RESP,
    OP_WORKLOAD_CREATE, OP_WORKLOAD_CREATE_RESP, OP_WORKLOAD_EXTEND_DAG,
    OP_WORKLOAD_EXTEND_DAG_RESP, OP_WORKLOAD_QUERY, OP_WORKLOAD_QUERY_RESP,
    OP_WORKLOAD_SUBMIT_DAG, OP_WORKLOAD_SUBMIT_DAG_RESP,
};
pub use workspace::*;
pub use agent::*;
pub mod fabric;
pub use fabric::*;
pub mod presentation;
pub use presentation::*;
pub mod session;
pub use session::*;
pub mod intent;
pub use intent::*;
pub mod observed;
pub use observed::*;




