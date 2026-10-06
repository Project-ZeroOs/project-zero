//! ZeroOS - agentd Agent Runtime Subsystem Daemon
//!
//! Authoritative Contract: Stage 4E Architecture Specification Rev2 & ADR-0028 Rev2.
//! Implementation Plan: Stage 4E Implementation Plan Rev3.
#![no_std]
#![no_main]

pub use libzero::{
    agent, broker, error, identity, ipc, persistence, resource, syscall, time, workload, workspace,
};

use agent::{
    AgentControlBlock, AgentLifecycleState, AgentRuntimeState, EventMessage, EventSubscription,
    TriggerEntry, WorkloadTerminationPolicy, MAX_AGENTS, MAX_AGENT_WORKLOADS, MAX_DELEGATION_DEPTH,
    MAX_EVENT_SUBSCRIPTIONS, MAX_TRIGGERS, OP_AGENT_CLEAR_SUSPENSION,
    OP_AGENT_CLEAR_SUSPENSION_RESP, OP_AGENT_CREATE, OP_AGENT_CREATE_RESP, OP_AGENT_DELEGATE,
    OP_AGENT_DELEGATE_RESP, OP_AGENT_DESTROY, OP_AGENT_DESTROY_RESP, OP_AGENT_DISPATCH_GOAL,
    OP_AGENT_DISPATCH_GOAL_RESP, OP_AGENT_GET_STATE, OP_AGENT_GET_STATE_RESP,
    OP_AGENT_REGISTER_TRIGGER, OP_AGENT_REGISTER_TRIGGER_RESP, OP_AGENT_SUBSCRIBE_EVENT,
    OP_AGENT_SUBSCRIBE_EVENT_RESP,
};
use error::ZeroError;
use identity::DistributedIdAllocator;
use ipc::{channel_receive, channel_send, IpcMessage};
use persistence::MemoryPersistenceAuthority;
use resource::DistributedId;
use syscall::{sys_cap_derive, sys_channel_close, sys_exit, sys_yield};
use time::{evaluate_freshness, read_canonical_tsc, TimeObservationFrame};

/// Authoritative Agent Daemon State.
pub struct AgentDaemon {
    pub node_id: u64,
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub agents: [AgentControlBlock; MAX_AGENTS],
    pub active_agent_count: usize,
    pub service_channel: u32,
    pub time_frame: TimeObservationFrame,
    pub producer_generations: [(u64, u32, u64); 16], // (producer_id, generation, last_seq)
}

impl AgentDaemon {
    pub fn new(node_id: u64, service_channel: u32) -> Result<Self, ZeroError> {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 500);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 128, persistence)?;
        let time_frame = TimeObservationFrame::new();
        let _ = time_frame.publish(1, 1000, 2_000_000_000, read_canonical_tsc(), 100, 7);

        Ok(Self {
            node_id,
            allocator,
            agents: [const { AgentControlBlock {
                agent_id: DistributedId { node_id: 0, local_seq: 0 },
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                principal_id: DistributedId { node_id: 0, local_seq: 0 },
                parent_agent_id: DistributedId { node_id: 0, local_seq: 0 },
                delegation_depth: 0,
                lifecycle_state: AgentLifecycleState::Unallocated,
                runtime_state: AgentRuntimeState::Idle,
                active_workload_count: 0,
                capability_handle: 0,
                trigger_count: 0,
                subscription_count: 0,
                overflow_flag: 0,
                _pad0: 0,
                associated_workloads: [DistributedId { node_id: 0, local_seq: 0 }; MAX_AGENT_WORKLOADS],
                workload_policies: [WorkloadTerminationPolicy::Cancel; MAX_AGENT_WORKLOADS],
                trigger_table: [TriggerEntry {
                    trigger_id: 0,
                    event_type: 0,
                    condition_mask: 0,
                    valid: 0,
                    _pad0: [0; 3],
                    _padding: [0; 12],
                }; MAX_TRIGGERS],
                event_subscriptions: [EventSubscription {
                    subscription_id: 0,
                    event_type: 0,
                    workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                    valid: 0,
                    _pad0: [0; 7],
                }; MAX_EVENT_SUBSCRIPTIONS],
                event_ring_buffer: [EventMessage {
                    producer_service_id: 0,
                    producer_generation: 0,
                    event_type: 0,
                    sequence: 0,
                    workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                    payload_len: 0,
                    _pad0: [0; 4],
                    payload: [0; 208],
                }; MAX_EVENT_SUBSCRIPTIONS],
                goal_scratchpad: [0; 1024],
            } }; MAX_AGENTS],
            active_agent_count: 0,
            service_channel,
            time_frame,
            producer_generations: [(0, 0, 0); 16],
        })
    }

    /// Primary IPC Request Dispatcher.
    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Enforce Protocol Direction: Reject incoming response opcodes fail-closed
        match req.tag {
            OP_AGENT_CREATE_RESP
            | OP_AGENT_DESTROY_RESP
            | OP_AGENT_GET_STATE_RESP
            | OP_AGENT_DISPATCH_GOAL_RESP
            | OP_AGENT_REGISTER_TRIGGER_RESP
            | OP_AGENT_SUBSCRIBE_EVENT_RESP
            | OP_AGENT_DELEGATE_RESP
            | OP_AGENT_CLEAR_SUSPENSION_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_AGENT_CREATE => self.handle_agent_create(req),
            OP_AGENT_DESTROY => self.handle_agent_destroy(req),
            OP_AGENT_GET_STATE => self.handle_agent_get_state(req),
            OP_AGENT_DISPATCH_GOAL => self.handle_agent_dispatch_goal(req),
            OP_AGENT_REGISTER_TRIGGER => self.handle_agent_register_trigger(req),
            OP_AGENT_SUBSCRIBE_EVENT => self.handle_agent_subscribe_event(req),
            OP_AGENT_DELEGATE => self.handle_agent_delegate(req),
            OP_AGENT_CLEAR_SUSPENSION => self.handle_agent_clear_suspension(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    fn find_free_slot(&self) -> Option<usize> {
        for (idx, slot) in self.agents.iter().enumerate() {
            if slot.lifecycle_state == AgentLifecycleState::Unallocated {
                return Some(idx);
            }
        }
        None
    }

    fn find_agent_slot(&self, id: DistributedId) -> Option<usize> {
        for (idx, slot) in self.agents.iter().enumerate() {
            if slot.agent_id == id && slot.lifecycle_state == AgentLifecycleState::Active {
                return Some(idx);
            }
        }
        None
    }

    /// Handles OP_AGENT_CREATE: Allocates AgentId, binds WorkspaceId, derives C_agent from C_ws.
    pub fn handle_agent_create(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_CREATE_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let slot_idx = match self.find_free_slot() {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_seq);

        let principal_node_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let principal_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let principal_id = DistributedId::new(principal_node_id, principal_seq);

        let c_ws_handle = if req.handles_count >= 1 { req.handles[0] } else { 0 };

        // Attempt kernel capability derivation: C_agent derived from C_ws
        let mut c_agent_handle: u32 = 0;
        if c_ws_handle != 0 {
            let res = unsafe { sys_cap_derive(c_ws_handle, 0x0F, &mut c_agent_handle as *mut u32) };
            if res != 0 {
                resp.payload[0..4].copy_from_slice(&(ZeroError::from_i64(res).as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        let agent_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let acb = &mut self.agents[slot_idx];
        acb.agent_id = agent_id;
        acb.workspace_id = ws_id;
        acb.principal_id = principal_id;
        acb.parent_agent_id = DistributedId::default();
        acb.delegation_depth = 0;
        acb.lifecycle_state = AgentLifecycleState::Active;
        acb.runtime_state = AgentRuntimeState::Idle;
        acb.capability_handle = c_agent_handle;
        acb.active_workload_count = 0;
        acb.trigger_count = 0;
        acb.subscription_count = 0;
        acb.overflow_flag = 0;

        self.active_agent_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&agent_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&agent_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_AGENT_DESTROY: Evaluates Workload policies (CANCEL vs DETACH) and revokes C_agent.
    pub fn handle_agent_destroy(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_DESTROY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let agent_id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_agent_slot(agent_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let acb = &mut self.agents[slot_idx];
        acb.lifecycle_state = AgentLifecycleState::Stopping;

        // Kernel-authoritative revocation of C_agent handle
        if acb.capability_handle != 0 {
            let _ = unsafe { sys_channel_close(acb.capability_handle) };
            acb.capability_handle = 0;
        }

        acb.lifecycle_state = AgentLifecycleState::Terminated;
        acb.active_workload_count = 0;
        self.active_agent_count = self.active_agent_count.saturating_sub(1);

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_AGENT_GET_STATE: Queries Lifecycle & Runtime state.
    pub fn handle_agent_get_state(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_GET_STATE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_agent_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let acb = &self.agents[slot_idx];

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = acb.lifecycle_state as u8;
        resp.payload[5] = acb.runtime_state as u8;
        resp.payload[6] = acb.active_workload_count;
        resp.payload[7] = acb.overflow_flag;
        resp.payload_len = 8;
        resp
    }

    /// Handles OP_AGENT_DISPATCH_GOAL: Formulates goal into Workload creation request.
    pub fn handle_agent_dispatch_goal(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_DISPATCH_GOAL_RESP;

        if req.payload_len < 24 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let agent_id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_agent_slot(agent_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let acb = &mut self.agents[slot_idx];
        if acb.active_workload_count as usize >= MAX_AGENT_WORKLOADS {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let wl_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let idx = acb.active_workload_count as usize;
        acb.associated_workloads[idx] = wl_id;
        acb.workload_policies[idx] = WorkloadTerminationPolicy::Cancel;
        acb.active_workload_count += 1;
        acb.runtime_state = AgentRuntimeState::Executing;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&wl_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&wl_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_AGENT_REGISTER_TRIGGER: Registers an automated wake trigger.
    pub fn handle_agent_register_trigger(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_REGISTER_TRIGGER_RESP;

        if req.payload_len < 28 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let agent_id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_agent_slot(agent_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let event_type = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());
        let condition_mask = u64::from_le_bytes(req.payload[20..28].try_into().unwrap());

        let acb = &mut self.agents[slot_idx];
        if acb.trigger_count as usize >= MAX_TRIGGERS {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let trigger_id = (acb.trigger_count + 1) as u32;
        let idx = acb.trigger_count as usize;
        acb.trigger_table[idx] = TriggerEntry {
            trigger_id,
            event_type,
            condition_mask,
            valid: 1,
            _pad0: [0; 3],
            _padding: [0; 12],
        };
        acb.trigger_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&trigger_id.to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    /// Handles OP_AGENT_SUBSCRIBE_EVENT: Registers event perception subscription.
    pub fn handle_agent_subscribe_event(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_SUBSCRIBE_EVENT_RESP;

        if req.payload_len < 36 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let agent_id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_agent_slot(agent_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let event_type = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());
        let ws_node_id = u64::from_le_bytes(req.payload[20..28].try_into().unwrap());
        let ws_seq = u64::from_le_bytes(req.payload[28..36].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_seq);

        let acb = &mut self.agents[slot_idx];
        if acb.subscription_count as usize >= MAX_EVENT_SUBSCRIPTIONS {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let subscription_id = (acb.subscription_count + 1) as u32;
        let idx = acb.subscription_count as usize;
        acb.event_subscriptions[idx] = EventSubscription {
            subscription_id,
            event_type,
            workspace_id: ws_id,
            valid: 1,
            _pad0: [0; 7],
        };
        acb.subscription_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&subscription_id.to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    /// Handles OP_AGENT_DELEGATE: Spawns a Child Agent with derived capability envelope.
    pub fn handle_agent_delegate(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_DELEGATE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let parent_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let parent_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let parent_id = DistributedId::new(parent_node_id, parent_seq);

        let parent_idx = match self.find_agent_slot(parent_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let parent_depth = self.agents[parent_idx].delegation_depth;
        if parent_depth >= (MAX_DELEGATION_DEPTH as u8) {
            resp.payload[0..4].copy_from_slice(&(ZeroError::CapAmplificationRejected.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let child_slot_idx = match self.find_free_slot() {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let child_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let parent_acb = &self.agents[parent_idx];
        let parent_cap = parent_acb.capability_handle;
        let ws_id = parent_acb.workspace_id;
        let principal_id = parent_acb.principal_id;

        let mut child_cap: u32 = 0;
        if parent_cap != 0 {
            let res = unsafe { sys_cap_derive(parent_cap, 0x07, &mut child_cap as *mut u32) };
            if res != 0 {
                resp.payload[0..4].copy_from_slice(&(ZeroError::from_i64(res).as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        let child_acb = &mut self.agents[child_slot_idx];
        child_acb.agent_id = child_id;
        child_acb.workspace_id = ws_id;
        child_acb.principal_id = principal_id;
        child_acb.parent_agent_id = parent_id;
        child_acb.delegation_depth = parent_depth + 1;
        child_acb.lifecycle_state = AgentLifecycleState::Active;
        child_acb.runtime_state = AgentRuntimeState::Idle;
        child_acb.capability_handle = child_cap;
        child_acb.active_workload_count = 0;
        child_acb.trigger_count = 0;
        child_acb.subscription_count = 0;
        child_acb.overflow_flag = 0;

        self.active_agent_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&child_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&child_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_AGENT_CLEAR_SUSPENSION: Clears Suspended runtime state using Stage 4B time authority.
    pub fn handle_agent_clear_suspension(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_AGENT_CLEAR_SUSPENSION_RESP;

        if req.payload_len < 40 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let agent_id = DistributedId::new(node_id, local_seq);

        let ticket_valid = req.payload[16];
        let deadline_tsc = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());

        if ticket_valid == 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Validate time freshness against qualified Stage 4B TimeObservationFrame
        let obs = match self.time_frame.read_observation() {
            Ok(o) => o,
            Err(e) => {
                // TimeAuthorityLost fail-closed behavior
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let current_tsc = read_canonical_tsc();
        if evaluate_freshness(&obs, current_tsc).is_err() || current_tsc > deadline_tsc {
            resp.payload[0..4].copy_from_slice(&(ZeroError::TimeObservationStale.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let slot_idx = match self.find_agent_slot(agent_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let acb = &mut self.agents[slot_idx];
        if acb.runtime_state == AgentRuntimeState::Suspended {
            acb.runtime_state = AgentRuntimeState::Executing;
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }
}

/// Freestanding Ring 3 Agent Daemon Entry Point.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let service_channel = 0x05;
    let mut daemon = match AgentDaemon::new(1, service_channel) {
        Ok(d) => d,
        Err(_) => unsafe { sys_exit(-1) },
    };

    loop {
        match channel_receive(service_channel, false) {
            Ok(msg) => {
                let resp = daemon.dispatch(&msg);
                let _ = channel_send(service_channel, &resp, false);
            }
            Err(_) => {
                unsafe { sys_yield() };
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { sys_exit(-2) }
}
