//! ZeroOS - libzero Agent Runtime Subsystem Data Structures & IPC Protocol
//!
//! Authoritative Contract: Stage 4E Architecture Specification Rev2 & ADR-0028 Rev2.
//! Implementation Plan: Stage 4E Implementation Plan Rev3.

use crate::resource::DistributedId;

// IPC Protocol OpCodes for agentd (registered with brokerd as "agentd.srv")
pub const OP_AGENT_CREATE:                u64 = 0x0501;
pub const OP_AGENT_CREATE_RESP:           u64 = 0x0581;
pub const OP_AGENT_DESTROY:               u64 = 0x0502;
pub const OP_AGENT_DESTROY_RESP:          u64 = 0x0582;
pub const OP_AGENT_GET_STATE:             u64 = 0x0503;
pub const OP_AGENT_GET_STATE_RESP:        u64 = 0x0583;
pub const OP_AGENT_DISPATCH_GOAL:         u64 = 0x0504;
pub const OP_AGENT_DISPATCH_GOAL_RESP:    u64 = 0x0584;
pub const OP_AGENT_REGISTER_TRIGGER:      u64 = 0x0505;
pub const OP_AGENT_REGISTER_TRIGGER_RESP: u64 = 0x0585;
pub const OP_AGENT_SUBSCRIBE_EVENT:       u64 = 0x0506;
pub const OP_AGENT_SUBSCRIBE_EVENT_RESP:  u64 = 0x0586;
pub const OP_AGENT_DELEGATE:              u64 = 0x0507;
pub const OP_AGENT_DELEGATE_RESP:         u64 = 0x0587;
pub const OP_AGENT_CLEAR_SUSPENSION:      u64 = 0x0508;
pub const OP_AGENT_CLEAR_SUSPENSION_RESP: u64 = 0x0588;

// Static Configuration & Hard Bounds
pub const MAX_AGENTS: usize = 64;
pub const MAX_AGENT_WORKLOADS: usize = 16;
pub const MAX_EVENT_SUBSCRIPTIONS: usize = 32;
pub const MAX_TRIGGERS: usize = 16;
pub const MAX_DELEGATION_DEPTH: usize = 4;
pub const MAX_IPC_AGENT_MSG_SIZE: usize = 2048;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AgentLifecycleState {
    Unallocated = 0,
    Creating = 1,
    Active = 2,
    Stopping = 3,
    Terminated = 4,
    Reclaimed = 5,
    Quiesced = 6,
    UnboundArchived = 7,
}

impl Default for AgentLifecycleState {
    fn default() -> Self {
        Self::Unallocated
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AgentRuntimeState {
    Idle = 0,
    Waiting = 1,
    Executing = 2,
    Suspended = 3,
    Recovering = 4,
}

impl Default for AgentRuntimeState {
    fn default() -> Self {
        Self::Idle
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WorkloadTerminationPolicy {
    Cancel = 0,
    Detach = 1,
}

impl Default for WorkloadTerminationPolicy {
    fn default() -> Self {
        Self::Cancel
    }
}

/// 32-byte Trigger Entry Record.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerEntry {
    pub trigger_id: u32,
    pub event_type: u32,
    pub condition_mask: u64,
    pub valid: u8,
    pub _pad0: [u8; 3],
    pub _padding: [u8; 12],
}

const _: () = assert!(core::mem::size_of::<TriggerEntry>() == 32);

/// 32-byte Event Subscription Record.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EventSubscription {
    pub subscription_id: u32,
    pub event_type: u32,
    pub workspace_id: DistributedId,
    pub valid: u8,
    pub _pad0: [u8; 7],
}

const _: () = assert!(core::mem::size_of::<EventSubscription>() == 32);

/// 256-byte Event Message Payload Structure.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct EventMessage {
    pub producer_service_id: u64,
    pub producer_generation: u32,
    pub event_type: u32,
    pub sequence: u64,
    pub workspace_id: DistributedId,
    pub payload_len: u32,
    pub _pad0: [u8; 4],
    pub payload: [u8; 208],
}

impl Default for EventMessage {
    fn default() -> Self {
        Self {
            producer_service_id: 0,
            producer_generation: 0,
            event_type: 0,
            sequence: 0,
            workspace_id: DistributedId::default(),
            payload_len: 0,
            _pad0: [0; 4],
            payload: [0; 208],
        }
    }
}

const _: () = assert!(core::mem::size_of::<EventMessage>() == 256);

/// 64-byte Human Authorization Clearance Ticket.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthorizationTicket {
    pub ticket_id: u64,
    pub agent_id: DistributedId,
    pub action_hash: u64,
    pub deadline_tsc: u64,
    pub issuer_pid: u64,
    pub valid: u8,
    pub _pad0: [u8; 7],
    pub _padding: [u8; 8],
}

const _: () = assert!(core::mem::size_of::<AuthorizationTicket>() == 64);

/// 11,104-byte Authoritative Agent Control Block.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AgentControlBlock {
    pub agent_id: DistributedId,
    pub workspace_id: DistributedId,
    pub principal_id: DistributedId,
    pub parent_agent_id: DistributedId,
    pub delegation_depth: u8,
    pub lifecycle_state: AgentLifecycleState,
    pub runtime_state: AgentRuntimeState,
    pub active_workload_count: u8,
    pub capability_handle: u32,
    pub trigger_count: u8,
    pub subscription_count: u8,
    pub overflow_flag: u8,
    pub _pad0: u8,
    pub associated_workloads: [DistributedId; MAX_AGENT_WORKLOADS],
    pub workload_policies: [WorkloadTerminationPolicy; MAX_AGENT_WORKLOADS],
    pub trigger_table: [TriggerEntry; MAX_TRIGGERS],
    pub event_subscriptions: [EventSubscription; MAX_EVENT_SUBSCRIPTIONS],
    pub event_ring_buffer: [EventMessage; MAX_EVENT_SUBSCRIPTIONS],
    pub goal_scratchpad: [u8; 1024],
}

impl Default for AgentControlBlock {
    fn default() -> Self {
        Self {
            agent_id: DistributedId::default(),
            workspace_id: DistributedId::default(),
            principal_id: DistributedId::default(),
            parent_agent_id: DistributedId::default(),
            delegation_depth: 0,
            lifecycle_state: AgentLifecycleState::Unallocated,
            runtime_state: AgentRuntimeState::Idle,
            active_workload_count: 0,
            capability_handle: 0,
            trigger_count: 0,
            subscription_count: 0,
            overflow_flag: 0,
            _pad0: 0,
            associated_workloads: [DistributedId::default(); MAX_AGENT_WORKLOADS],
            workload_policies: [WorkloadTerminationPolicy::Cancel; MAX_AGENT_WORKLOADS],
            trigger_table: [TriggerEntry::default(); MAX_TRIGGERS],
            event_subscriptions: [EventSubscription::default(); MAX_EVENT_SUBSCRIPTIONS],
            event_ring_buffer: [EventMessage::default(); MAX_EVENT_SUBSCRIPTIONS],
            goal_scratchpad: [0; 1024],
        }
    }
}

const _: () = assert!(core::mem::size_of::<AgentControlBlock>() == 11104);

// Protocol Request / Response Structures for libzero IPC

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AgentCreateReq {
    pub workspace_id: DistributedId,
    pub principal_id: DistributedId,
    pub cap_handle: u32,
    pub _padding: [u8; 36],
}

impl Default for AgentCreateReq {
    fn default() -> Self {
        Self {
            workspace_id: DistributedId::default(),
            principal_id: DistributedId::default(),
            cap_handle: 0,
            _padding: [0; 36],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentCreateResp {
    pub status: i32,
    pub agent_id: DistributedId,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentDestroyReq {
    pub agent_id: DistributedId,
    pub cap_handle: u32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentDestroyResp {
    pub status: i32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentGetStateReq {
    pub agent_id: DistributedId,
    pub _padding: [u8; 8],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentGetStateResp {
    pub status: i32,
    pub lifecycle_state: AgentLifecycleState,
    pub runtime_state: AgentRuntimeState,
    pub active_workload_count: u8,
    pub overflow_flag: u8,
    pub _padding: [u8; 8],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AgentDispatchGoalReq {
    pub agent_id: DistributedId,
    pub cap_handle: u32,
    pub goal_len: u32,
    pub goal_data: [u8; 512],
}

impl Default for AgentDispatchGoalReq {
    fn default() -> Self {
        Self {
            agent_id: DistributedId::default(),
            cap_handle: 0,
            goal_len: 0,
            goal_data: [0; 512],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentDispatchGoalResp {
    pub status: i32,
    pub workload_id: DistributedId,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentRegisterTriggerReq {
    pub agent_id: DistributedId,
    pub cap_handle: u32,
    pub event_type: u32,
    pub condition_mask: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentRegisterTriggerResp {
    pub status: i32,
    pub trigger_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentSubscribeEventReq {
    pub agent_id: DistributedId,
    pub cap_handle: u32,
    pub event_type: u32,
    pub workspace_id: DistributedId,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentSubscribeEventResp {
    pub status: i32,
    pub subscription_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentDelegateReq {
    pub parent_agent_id: DistributedId,
    pub cap_handle: u32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentDelegateResp {
    pub status: i32,
    pub child_agent_id: DistributedId,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentClearSuspensionReq {
    pub agent_id: DistributedId,
    pub ticket: AuthorizationTicket,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AgentClearSuspensionResp {
    pub status: i32,
    pub _padding: [u8; 4],
}
