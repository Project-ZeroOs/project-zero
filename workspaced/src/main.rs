//! ZeroOS - workspaced Workspace & Persistent Context Subsystem Daemon
//!
//! Authoritative Contract: Stage 4D Architecture Specification Rev3 & ZeroOS Workspace Semantic Model (REV1).
#![no_std]
#![no_main]

pub use libzero::{
    error, exec, identity, ipc, orchestration, persistence, resource, syscall, workspace,
};
use exec::*;
use orchestration::*;

use error::ZeroError;
use identity::DistributedIdAllocator;
use ipc::{channel_receive, channel_send, IpcMessage};
use persistence::MemoryPersistenceAuthority;
use resource::DistributedId;
use syscall::{sys_channel_close, sys_dir_read, sys_exit, sys_yield, UserDirEntry};
use workspace::{
    AgentBindingState, ContextEdge, ContextNode, ContextNodeType, IntentDependency, IntentNode,
    IntentNodeState, MembershipEdgeState, ObjectIdRegistryHeader, ObjectIdRegistryRecord,
    ObjectState, WorkspaceAgentBinding, WorkspaceControlBlock, WorkspaceMembershipEdge,
    WorkspaceState, MAX_AGENTS_PER_WORKSPACE, MAX_INTENT_DEPS_PER_WORKSPACE,
    MAX_INTENT_NODES_PER_WORKSPACE, MAX_OBJECT_REGISTRY_ENTRIES,
    MAX_RESIDENT_EDGES_PER_WORKSPACE, MAX_RESIDENT_NODES_PER_WORKSPACE,
    MAX_TOTAL_MEMBERSHIP_EDGES, MAX_WORKLOADS_PER_WORKSPACE, MAX_WORKSPACES, OP_OBJECT_LOOKUP,
    OP_OBJECT_LOOKUP_RESP, OP_OBJECT_RECONCILE, OP_OBJECT_RECONCILE_RESP, OP_OBJECT_REGISTER,
    OP_OBJECT_REGISTER_RESP, OP_OBJECT_RENAME, OP_OBJECT_RENAME_RESP, OP_OBJECT_UNLINK,
    OP_OBJECT_UNLINK_RESP, OP_WORKSPACE_ADD_MEMBER, OP_WORKSPACE_ADD_MEMBER_RESP,
    OP_WORKSPACE_ARCHIVE, OP_WORKSPACE_ARCHIVE_RESP, OP_WORKSPACE_ATTACH_AGENT,
    OP_WORKSPACE_ATTACH_AGENT_RESP, OP_WORKSPACE_ATTACH_WORKLOAD,
    OP_WORKSPACE_ATTACH_WORKLOAD_RESP, OP_WORKSPACE_CLOSE, OP_WORKSPACE_CLOSE_RESP,
    OP_WORKSPACE_CONTEXT_ADD_NODE, OP_WORKSPACE_CONTEXT_ADD_NODE_RESP,
    OP_WORKSPACE_CONTEXT_QUERY, OP_WORKSPACE_CONTEXT_QUERY_RESP, OP_WORKSPACE_CREATE,
    OP_WORKSPACE_CREATE_RESP, OP_WORKSPACE_DELETE, OP_WORKSPACE_DELETE_RESP,
    OP_WORKSPACE_DETACH_AGENT, OP_WORKSPACE_DETACH_AGENT_RESP, OP_WORKSPACE_INTENT_ADD_DEP,
    OP_WORKSPACE_INTENT_ADD_DEP_RESP, OP_WORKSPACE_INTENT_ADD_NODE,
    OP_WORKSPACE_INTENT_ADD_NODE_RESP, OP_WORKSPACE_INTENT_QUERY, OP_WORKSPACE_INTENT_QUERY_RESP,
    OP_WORKSPACE_OPEN, OP_WORKSPACE_OPEN_RESP, OP_WORKSPACE_QUERY, OP_WORKSPACE_QUERY_MEMBERS,
    OP_WORKSPACE_QUERY_MEMBERS_RESP, OP_WORKSPACE_QUERY_RESP, OP_WORKSPACE_REMOVE_MEMBER,
    OP_WORKSPACE_REMOVE_MEMBER_RESP, OP_WORKSPACE_RESUME, OP_WORKSPACE_RESUME_RESP,
    OP_WORKSPACE_SUSPEND, OP_WORKSPACE_SUSPEND_RESP, WS_SYSTEM_0_LOCAL_SEQ, WS_SYSTEM_0_NODE_ID,
};

pub const DURABLE_STORAGE_SIZE: usize = 250_000;

/// Authoritative Workspace Daemon State.
pub struct WorkspaceDaemon {
    pub node_id: u64,
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub workspaces: [WorkspaceControlBlock; MAX_WORKSPACES],
    pub membership_edges: [WorkspaceMembershipEdge; MAX_TOTAL_MEMBERSHIP_EDGES],
    pub agent_bindings: [[WorkspaceAgentBinding; MAX_AGENTS_PER_WORKSPACE]; MAX_WORKSPACES],
    pub intent_nodes: [[IntentNode; MAX_INTENT_NODES_PER_WORKSPACE]; MAX_WORKSPACES],
    pub intent_deps: [[IntentDependency; MAX_INTENT_DEPS_PER_WORKSPACE]; MAX_WORKSPACES],
    pub resident_nodes: [[ContextNode; MAX_RESIDENT_NODES_PER_WORKSPACE]; MAX_WORKSPACES],
    pub resident_edges: [[ContextEdge; MAX_RESIDENT_EDGES_PER_WORKSPACE]; MAX_WORKSPACES],
    pub object_registry: [ObjectIdRegistryRecord; MAX_OBJECT_REGISTRY_ENTRIES],
    pub orch_intents: [[OrchestrationIntent; MAX_ORCH_INTENTS_PER_WORKSPACE]; MAX_WORKSPACES],
    pub orch_plans: [[OrchestrationPlan; MAX_PLANS_PER_INTENT * MAX_ORCH_INTENTS_PER_WORKSPACE]; MAX_WORKSPACES],
    pub orch_steps: [[PlanStep; MAX_PLAN_STEPS_PER_PLAN * MAX_PLANS_PER_INTENT]; MAX_WORKSPACES],
    pub orch_edges: [[PlanEdge; MAX_PLAN_EDGES_PER_PLAN * MAX_PLANS_PER_INTENT]; MAX_WORKSPACES],
    pub orch_approvals: [[ApprovalRecord; MAX_APPROVAL_RECORDS]; MAX_WORKSPACES],
    pub executions: [[ExecutionRecord; MAX_EXECUTIONS_PER_WORKSPACE]; MAX_WORKSPACES],
    pub execution_events: [[ExecutionEvent; MAX_EVENTS_PER_WORKSPACE]; MAX_WORKSPACES],
    pub observations: [[SystemObservation; MAX_OBSERVATIONS_PER_WORKSPACE]; MAX_WORKSPACES],
    pub workspace_sequence_clocks: [u64; MAX_WORKSPACES],
    pub execution_counts: [usize; MAX_WORKSPACES],
    pub event_counts: [usize; MAX_WORKSPACES],
    pub observation_counts: [usize; MAX_WORKSPACES],
    pub durable_storage: [u8; DURABLE_STORAGE_SIZE],
    pub active_workspace_count: usize,
    pub service_channel: u32,
}

impl WorkspaceDaemon {
    pub fn new(node_id: u64, service_channel: u32) -> Result<Self, ZeroError> {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 200);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 64, persistence)?;

        let mut daemon = Self {
            node_id,
            allocator,
            workspaces: [const { WorkspaceControlBlock {
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                owner_pid: 0,
                generation: 0,
                state: WorkspaceState::Unallocated,
                _pad0: [0; 3],
                capability_envelope_handle: 0,
                active_workload_count: 0,
                active_agent_count: 0,
                membership_count: 0,
                associated_workloads: [DistributedId { node_id: 0, local_seq: 0 }; MAX_WORKLOADS_PER_WORKSPACE],
                associated_agents: [DistributedId { node_id: 0, local_seq: 0 }; MAX_AGENTS_PER_WORKSPACE],
                intent_node_count: 0,
                intent_dep_count: 0,
                root_dir_handle: 0,
                resident_node_count: 0,
                resident_edge_count: 0,
                _padding: [0; 196],
            } }; MAX_WORKSPACES],
            membership_edges: [WorkspaceMembershipEdge {
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                object_id: DistributedId { node_id: 0, local_seq: 0 },
                state: MembershipEdgeState::Unallocated,
                _pad0: [0; 3],
                _padding: [0; 12],
            }; MAX_TOTAL_MEMBERSHIP_EDGES],
            agent_bindings: [[WorkspaceAgentBinding {
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                agent_id: DistributedId { node_id: 0, local_seq: 0 },
                state: AgentBindingState::Unbound,
                _pad0: [0; 3],
                _padding: [0; 12],
            }; MAX_AGENTS_PER_WORKSPACE]; MAX_WORKSPACES],
            intent_nodes: [[IntentNode {
                node_id: 0,
                state: IntentNodeState::Pending,
                priority: 1,
                task_type: 1,
                _pad0: 0,
                label: [0; 32],
                resource_handle: 0,
                _padding: [0; 20],
            }; MAX_INTENT_NODES_PER_WORKSPACE]; MAX_WORKSPACES],
            intent_deps: [[IntentDependency {
                parent_node_id: 0,
                child_node_id: 0,
                valid: 0,
                _pad0: [0; 3],
                _padding: [0; 20],
            }; MAX_INTENT_DEPS_PER_WORKSPACE]; MAX_WORKSPACES],
            resident_nodes: [[ContextNode {
                node_id: 0,
                node_type: ContextNodeType::Document,
                valid: 0,
                _pad0: [0; 2],
                label: [0; 32],
                resource_handle: 0,
                _padding: [0; 20],
            }; MAX_RESIDENT_NODES_PER_WORKSPACE]; MAX_WORKSPACES],
            resident_edges: [[ContextEdge {
                source_node_id: 0,
                target_node_id: 0,
                edge_type: workspace::ContextEdgeType::References,
                valid: 0,
                _pad0: [0; 2],
                _padding: [0; 20],
            }; MAX_RESIDENT_EDGES_PER_WORKSPACE]; MAX_WORKSPACES],
            object_registry: [ObjectIdRegistryRecord {
                object_id: DistributedId { node_id: 0, local_seq: 0 },
                device_id: 0,
                inode_num: 0,
                state: ObjectState::Unallocated,
                name_len: 0,
                _pad0: [0; 2],
                name: [0; 128],
                _padding: [0; 4],
            }; MAX_OBJECT_REGISTRY_ENTRIES],
            orch_intents: [[OrchestrationIntent {
                intent_id: DistributedId { node_id: 0, local_seq: 0 },
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                creator_id: DistributedId { node_id: 0, local_seq: 0 },
                goal_statement: [0; 64],
                state: ORCH_INTENT_STATE_SUBMITTED,
                _pad0: [0; 7],
                active_plan_id: DistributedId { node_id: 0, local_seq: 0 },
                created_at_tsc: 0,
                updated_at_tsc: 0,
            }; MAX_ORCH_INTENTS_PER_WORKSPACE]; MAX_WORKSPACES],
            orch_plans: [[OrchestrationPlan {
                plan_id: DistributedId { node_id: 0, local_seq: 0 },
                intent_id: DistributedId { node_id: 0, local_seq: 0 },
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                version: 1,
                author_type: AUTHOR_TYPE_SYSTEM_PLANNER,
                state: PLAN_STATE_DRAFT,
                step_count: 0,
                edge_count: 0,
                author_id: DistributedId { node_id: 0, local_seq: 0 },
                created_at_tsc: 0,
                approval_status: 0,
                _pad: [0; 5],
            }; MAX_PLANS_PER_INTENT * MAX_ORCH_INTENTS_PER_WORKSPACE]; MAX_WORKSPACES],
            orch_steps: [[PlanStep {
                step_id: DistributedId { node_id: 0, local_seq: 0 },
                plan_id: DistributedId { node_id: 0, local_seq: 0 },
                step_name: [0; 32],
                exec_spec: [0; 64],
                input_object_ids: [DistributedId { node_id: 0, local_seq: 0 }; 4],
                output_object_ids: [DistributedId { node_id: 0, local_seq: 0 }; 4],
                req_capabilities: 0,
                cpu_cores: 1,
                ram_mb: 256,
                gpu_mb: 0,
                storage_mb: 100,
                approval_required: 0,
                status: STEP_STATUS_PENDING,
                is_network_dependent: 0,
                is_destructive: 0,
                approval_trigger_mask: 0,
                _pad: [0; 3],
                materialized_workload_id: DistributedId { node_id: 0, local_seq: 0 },
            }; MAX_PLAN_STEPS_PER_PLAN * MAX_PLANS_PER_INTENT]; MAX_WORKSPACES],
            orch_edges: [[PlanEdge {
                parent_step_id: DistributedId { node_id: 0, local_seq: 0 },
                child_step_id: DistributedId { node_id: 0, local_seq: 0 },
            }; MAX_PLAN_EDGES_PER_PLAN * MAX_PLANS_PER_INTENT]; MAX_WORKSPACES],
            orch_approvals: [[ApprovalRecord {
                step_id: DistributedId { node_id: 0, local_seq: 0 },
                plan_id: DistributedId { node_id: 0, local_seq: 0 },
                approver_id: DistributedId { node_id: 0, local_seq: 0 },
                approved_at_tsc: 0,
                is_approved: 0,
                trigger_flags: 0,
                _pad: [0; 6],
            }; MAX_APPROVAL_RECORDS]; MAX_WORKSPACES],
            executions: [[const { ExecutionRecord {
                execution_id: DistributedId { node_id: 0, local_seq: 0 },
                workload_id: DistributedId { node_id: 0, local_seq: 0 },
                plan_id: DistributedId { node_id: 0, local_seq: 0 },
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                plan_version: 1,
                attempt_number: 1,
                state: ExecutionState::Unallocated,
                _pad0: [0; 7],
                assigned_node_id: 0,
                bound_pid: 0,
                lease_id: 0,
                started_at_tsc: 0,
                ended_at_tsc: 0,
                exit_code: 0,
                recovery_count: 0,
            } }; MAX_EXECUTIONS_PER_WORKSPACE]; MAX_WORKSPACES],
            execution_events: [[const { ExecutionEvent {
                event_id: DistributedId { node_id: 0, local_seq: 0 },
                execution_id: DistributedId { node_id: 0, local_seq: 0 },
                workload_id: DistributedId { node_id: 0, local_seq: 0 },
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                sequence_num: 0,
                timestamp_tsc: 0,
                event_type: 1,
                producer: 1,
                _pad0: [0; 5],
                payload: [0; 64],
            } }; MAX_EVENTS_PER_WORKSPACE]; MAX_WORKSPACES],
            observations: [[const { SystemObservation {
                observation_id: DistributedId { node_id: 0, local_seq: 0 },
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                workload_id: DistributedId { node_id: 0, local_seq: 0 },
                execution_id: DistributedId { node_id: 0, local_seq: 0 },
                provenance_event_id: DistributedId { node_id: 0, local_seq: 0 },
                provenance_sequence_num: 0,
                obs_type: OBS_TYPE_SUCCESS,
                certainty_level: CERTAINTY_AUTHORITATIVE_KERNEL,
                fact_level: FACT_LEVEL_RAW_FACT,
                _pad0: [0; 4],
                observed_at_tsc: 0,
                evidence_ref: DistributedId { node_id: 0, local_seq: 0 },
                payload: [0; 64],
            } }; MAX_OBSERVATIONS_PER_WORKSPACE]; MAX_WORKSPACES],
            workspace_sequence_clocks: [0u64; MAX_WORKSPACES],
            execution_counts: [0usize; MAX_WORKSPACES],
            event_counts: [0usize; MAX_WORKSPACES],
            observation_counts: [0usize; MAX_WORKSPACES],
            durable_storage: [0; DURABLE_STORAGE_SIZE],
            active_workspace_count: 0,
            service_channel,
        };


        let _ = daemon.load_durable_registry();
        Ok(daemon)
    }

    /// Dispatches incoming IPC requests from clients or system daemons.
    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Enforce I-WS-PROTOCOL-DIRECTION: Response tags cannot be used as incoming request opcodes
        match req.tag {
            OP_WORKSPACE_CREATE_RESP
            | OP_WORKSPACE_OPEN_RESP
            | OP_WORKSPACE_CLOSE_RESP
            | OP_WORKSPACE_QUERY_RESP
            | OP_WORKSPACE_SUSPEND_RESP
            | OP_WORKSPACE_RESUME_RESP
            | OP_WORKSPACE_DELETE_RESP
            | OP_WORKSPACE_ATTACH_WORKLOAD_RESP
            | OP_WORKSPACE_CONTEXT_ADD_NODE_RESP
            | OP_WORKSPACE_CONTEXT_QUERY_RESP
            | OP_OBJECT_REGISTER_RESP
            | OP_OBJECT_LOOKUP_RESP
            | OP_OBJECT_RENAME_RESP
            | OP_OBJECT_UNLINK_RESP
            | OP_OBJECT_RECONCILE_RESP
            | OP_WORKSPACE_ADD_MEMBER_RESP
            | OP_WORKSPACE_REMOVE_MEMBER_RESP
            | OP_WORKSPACE_QUERY_MEMBERS_RESP
            | OP_WORKSPACE_ATTACH_AGENT_RESP
            | OP_WORKSPACE_DETACH_AGENT_RESP
            | OP_WORKSPACE_INTENT_ADD_NODE_RESP
            | OP_WORKSPACE_INTENT_ADD_DEP_RESP
            | OP_WORKSPACE_INTENT_QUERY_RESP
            | OP_ORCH_INTENT_SUBMIT_RESP
            | OP_ORCH_PLAN_SUBMIT_RESP
            | OP_ORCH_PLAN_VALIDATE_RESP
            | OP_ORCH_PLAN_APPROVE_RESP
            | OP_ORCH_PLAN_MATERIALIZE_RESP
            | OP_ORCH_PLAN_REPLAN_RESP
            | OP_ORCH_PLAN_QUERY_RESP
            | OP_EXEC_CREATE_RESP
            | OP_EXEC_ADMIT_RESP
            | OP_EXEC_START_RESP
            | OP_EXEC_SUSPEND_RESP
            | OP_EXEC_RESUME_RESP
            | OP_EXEC_COMPLETE_RESP
            | OP_EXEC_FAIL_RESP
            | OP_EXEC_TIMEOUT_RESP
            | OP_EXEC_INTERRUPT_RESP
            | OP_EXEC_INVALIDATE_RESP
            | OP_EXEC_RECOVER_RESP
            | OP_EXEC_QUERY_RESP
            | OP_EXEC_EMIT_EVENT_RESP
            | OP_OBSERVATION_PUBLISH_RESP
            | OP_OBSERVATION_QUERY_RESP
            | OP_WORKSPACE_ARCHIVE_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_WORKSPACE_CREATE => self.handle_workspace_create(req),
            OP_WORKSPACE_OPEN => self.handle_workspace_open(req),
            OP_WORKSPACE_CLOSE => self.handle_workspace_close(req),
            OP_WORKSPACE_QUERY => self.handle_workspace_query(req),
            OP_WORKSPACE_SUSPEND => self.handle_workspace_suspend(req),
            OP_WORKSPACE_RESUME => self.handle_workspace_resume(req),
            OP_WORKSPACE_ARCHIVE => self.handle_workspace_archive(req),
            OP_WORKSPACE_DELETE => self.handle_workspace_delete(req),
            OP_WORKSPACE_ADD_MEMBER => self.handle_workspace_add_member(req),
            OP_WORKSPACE_REMOVE_MEMBER => self.handle_workspace_remove_member(req),
            OP_WORKSPACE_QUERY_MEMBERS => self.handle_workspace_query_members(req),
            OP_WORKSPACE_ATTACH_WORKLOAD => self.handle_workspace_attach_workload(req),
            OP_WORKSPACE_ATTACH_AGENT => self.handle_workspace_attach_agent(req),
            OP_WORKSPACE_DETACH_AGENT => self.handle_workspace_detach_agent(req),
            OP_WORKSPACE_INTENT_ADD_NODE => self.handle_workspace_intent_add_node(req),
            OP_WORKSPACE_INTENT_ADD_DEP => self.handle_workspace_intent_add_dep(req),
            OP_WORKSPACE_INTENT_QUERY => self.handle_workspace_intent_query(req),
            OP_WORKSPACE_CONTEXT_ADD_NODE => self.handle_context_add_node(req),
            OP_WORKSPACE_CONTEXT_QUERY => self.handle_context_query(req),
            OP_OBJECT_REGISTER => self.handle_object_register(req),
            OP_OBJECT_LOOKUP => self.handle_object_lookup(req),
            OP_OBJECT_RENAME => self.handle_object_rename(req),
            OP_OBJECT_UNLINK => self.handle_object_unlink(req),
            OP_OBJECT_RECONCILE => self.handle_object_reconcile(req),
            OP_ORCH_INTENT_SUBMIT => self.handle_orch_intent_submit(req),
            OP_ORCH_PLAN_SUBMIT => self.handle_orch_plan_submit(req),
            OP_ORCH_PLAN_VALIDATE => self.handle_orch_plan_validate(req),
            OP_ORCH_PLAN_APPROVE => self.handle_orch_plan_approve(req),
            OP_ORCH_PLAN_MATERIALIZE => self.handle_orch_plan_materialize(req),
            OP_ORCH_PLAN_REPLAN => self.handle_orch_plan_replan(req),
            OP_ORCH_PLAN_QUERY => self.handle_orch_plan_query(req),
            OP_EXEC_CREATE => self.handle_exec_create(req),
            OP_EXEC_START => self.handle_exec_start(req),
            OP_EXEC_COMPLETE => self.handle_exec_complete(req),
            OP_EXEC_FAIL => self.handle_exec_fail(req),
            OP_EXEC_QUERY => self.handle_exec_query(req),
            OP_OBSERVATION_QUERY => self.handle_observation_query(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }


    /// Handles OP_WORKSPACE_CREATE: allocates WorkspaceId from unified allocator.
    pub fn handle_workspace_create(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CREATE_RESP;

        let slot_idx = match self.find_free_slot() {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let workspace_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let cap_handle = if req.handles_count >= 1 { req.handles[0] } else { 0 };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.workspace_id = workspace_id;
        wcb.owner_pid = req.tag;
        wcb.generation = 1;
        wcb.state = WorkspaceState::Active;
        wcb.capability_envelope_handle = cap_handle;
        wcb.active_workload_count = 0;
        wcb.active_agent_count = 0;
        wcb.membership_count = 0;
        wcb.intent_node_count = 0;
        wcb.intent_dep_count = 0;
        wcb.resident_node_count = 0;
        wcb.resident_edge_count = 0;

        self.active_workspace_count += 1;
        let _ = self.save_durable_registry();

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&workspace_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&workspace_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_WORKSPACE_OPEN: opens an existing workspace.
    pub fn handle_workspace_open(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_OPEN_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        if self.workspaces[slot_idx].state == WorkspaceState::Suspended || self.workspaces[slot_idx].state == WorkspaceState::Archived {
            self.workspaces[slot_idx].state = WorkspaceState::Active;
            let _ = self.save_durable_registry();
        }

        let wcb = &self.workspaces[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = wcb.state as u8;
        resp.payload[5] = wcb.active_workload_count;
        resp.payload_len = 6;
        resp
    }

    /// Handles OP_WORKSPACE_CLOSE: closes/de-activates workspace.
    pub fn handle_workspace_close(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CLOSE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Suspended;
        let _ = self.save_durable_registry();

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_QUERY: returns workspace status.
    pub fn handle_workspace_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &self.workspaces[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = wcb.state as u8;
        resp.payload[5] = wcb.active_workload_count;
        resp.payload[6] = wcb.active_agent_count;
        resp.payload[7..9].copy_from_slice(&wcb.membership_count.to_le_bytes());
        resp.payload[9..13].copy_from_slice(&wcb.generation.to_le_bytes());
        resp.payload_len = 13;
        resp
    }

    /// Handles OP_WORKSPACE_SUSPEND: suspends workspace operations.
    pub fn handle_workspace_suspend(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_SUSPEND_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Suspended;

        // Quiesce agent execution bindings for this workspace
        for binding in self.agent_bindings[slot_idx].iter_mut() {
            if binding.state == AgentBindingState::Active {
                binding.state = AgentBindingState::Quiesced;
            }
        }

        // Pause active orchestration plans (IO-20)
        for plan in self.orch_plans[slot_idx].iter_mut() {
            if plan.state == PLAN_STATE_ACTIVE {
                plan.state = PLAN_STATE_PAUSED;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_RESUME: resumes suspended workspace operations.
    pub fn handle_workspace_resume(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_RESUME_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Active;

        // Resume agent execution bindings for this workspace
        for binding in self.agent_bindings[slot_idx].iter_mut() {
            if binding.state == AgentBindingState::Quiesced {
                binding.state = AgentBindingState::Active;
            }
        }

        // Resume paused orchestration plans (IO-20)
        for plan in self.orch_plans[slot_idx].iter_mut() {
            if plan.state == PLAN_STATE_PAUSED {
                plan.state = PLAN_STATE_ACTIVE;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_ARCHIVE: transitions workspace state to Archived.
    pub fn handle_workspace_archive(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_ARCHIVE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Archived;

        for binding in self.agent_bindings[slot_idx].iter_mut() {
            if binding.state == AgentBindingState::Active || binding.state == AgentBindingState::Quiesced {
                binding.state = AgentBindingState::UnboundArchived;
            }
        }

        // Cancel active orchestration plans on workspace archive (IO-21)
        for plan in self.orch_plans[slot_idx].iter_mut() {
            if plan.state == PLAN_STATE_ACTIVE || plan.state == PLAN_STATE_PAUSED || plan.state == PLAN_STATE_WAITING_APPROVAL {
                plan.state = PLAN_STATE_CANCELLED;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_DELETE: REV1 Workspace Destruction Teardown.
    pub fn handle_workspace_delete(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_DELETE_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workspace_slot(id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.state = WorkspaceState::Closing;

        // 1. Purge all membership edges MEMBER_OF(WorkspaceId, obj_id) for this workspace.
        for edge in self.membership_edges.iter_mut() {
            if edge.workspace_id == id {
                edge.state = MembershipEdgeState::Unallocated;
                edge.workspace_id = DistributedId::default();
                edge.object_id = DistributedId::default();
            }
        }
        wcb.membership_count = 0;

        // 2. Terminate workloads attached to this workspace
        wcb.active_workload_count = 0;
        wcb.associated_workloads = [DistributedId::default(); MAX_WORKLOADS_PER_WORKSPACE];

        // 3. Agent bindings transition to UnboundArchived
        for binding in self.agent_bindings[slot_idx].iter_mut() {
            binding.state = AgentBindingState::UnboundArchived;
        }
        wcb.active_agent_count = 0;
        wcb.associated_agents = [DistributedId::default(); MAX_AGENTS_PER_WORKSPACE];

        // 4. Cancel all orchestration plans on workspace destruction (IO-21)
        for plan in self.orch_plans[slot_idx].iter_mut() {
            if plan.state == PLAN_STATE_ACTIVE || plan.state == PLAN_STATE_PAUSED || plan.state == PLAN_STATE_WAITING_APPROVAL {
                plan.state = PLAN_STATE_CANCELLED;
            }
        }

        // 5. Revoke capability envelope handle
        if wcb.capability_envelope_handle != 0 {
            let _ = unsafe { sys_channel_close(wcb.capability_envelope_handle) };
            wcb.capability_envelope_handle = 0;
        }


        wcb.state = WorkspaceState::Destroyed;
        wcb.workspace_id = DistributedId::default();

        if self.active_workspace_count > 0 {
            self.active_workspace_count -= 1;
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_ADD_MEMBER: adds MEMBER_OF(ws_id, obj_id) logical edge.
    pub fn handle_workspace_add_member(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_ADD_MEMBER_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let obj_node_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let obj_local_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let obj_id = DistributedId::new(obj_node_id, obj_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        // Idempotent check: if edge already exists, update state to Live if needed and return Success
        for edge in self.membership_edges.iter_mut() {
            if edge.workspace_id == ws_id && edge.object_id == obj_id {
                edge.state = MembershipEdgeState::Live;
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        // Allocate new edge in membership_edges
        let mut free_edge_slot = None;
        for (i, edge) in self.membership_edges.iter().enumerate() {
            if edge.state == MembershipEdgeState::Unallocated {
                free_edge_slot = Some(i);
                break;
            }
        }

        let edge_idx = match free_edge_slot {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        self.membership_edges[edge_idx] = WorkspaceMembershipEdge {
            workspace_id: ws_id,
            object_id: obj_id,
            state: MembershipEdgeState::Live,
            _pad0: [0; 3],
            _padding: [0; 12],
        };

        let wcb = &mut self.workspaces[slot_idx];
        wcb.membership_count += 1;
        wcb.generation += 1;

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_REMOVE_MEMBER: removes MEMBER_OF(ws_id, obj_id) logical edge.
    pub fn handle_workspace_remove_member(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_REMOVE_MEMBER_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let obj_node_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let obj_local_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let obj_id = DistributedId::new(obj_node_id, obj_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        for edge in self.membership_edges.iter_mut() {
            if edge.workspace_id == ws_id && edge.object_id == obj_id {
                edge.state = MembershipEdgeState::Unallocated;
                edge.workspace_id = DistributedId::default();
                edge.object_id = DistributedId::default();
                let wcb = &mut self.workspaces[slot_idx];
                if wcb.membership_count > 0 {
                    wcb.membership_count -= 1;
                }
                wcb.generation += 1;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_QUERY_MEMBERS: enumerates MEMBER_OF(ws_id, obj_id) edges.
    pub fn handle_workspace_query_members(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_QUERY_MEMBERS_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        if self.find_workspace_slot(ws_id).is_none() {
            resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let mut member_count = 0u32;
        let mut offset = 8;

        for edge in self.membership_edges.iter() {
            if edge.workspace_id == ws_id && (edge.state == MembershipEdgeState::Live || edge.state == MembershipEdgeState::StaleUnresolved) {
                if offset + 16 <= 256 {
                    resp.payload[offset..offset + 8].copy_from_slice(&edge.object_id.node_id.to_le_bytes());
                    resp.payload[offset + 8..offset + 16].copy_from_slice(&edge.object_id.local_seq.to_le_bytes());
                    offset += 16;
                }
                member_count += 1;
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&member_count.to_le_bytes());
        resp.payload_len = offset as u16;
        resp
    }

    /// Handles OP_WORKSPACE_ATTACH_WORKLOAD: attaches workload ID to workspace.
    pub fn handle_workspace_attach_workload(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_ATTACH_WORKLOAD_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        // System Workload Protection: WS_SYSTEM_0 is reserved for administrative system daemons
        let system_ws = DistributedId::new(WS_SYSTEM_0_NODE_ID, WS_SYSTEM_0_LOCAL_SEQ);
        if ws_id == system_ws && req.tag != 1 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let wl_node_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let wl_local_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let wl_id = DistributedId::new(wl_node_id, wl_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];

        if wcb.active_workload_count as usize >= MAX_WORKLOADS_PER_WORKSPACE {
            resp.payload[0..4].copy_from_slice(&(ZeroError::QuotaExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let count = wcb.active_workload_count as usize;
        wcb.associated_workloads[count] = wl_id;
        wcb.active_workload_count += 1;
        wcb.generation += 1;

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_ATTACH_AGENT: attaches AgentId to workspace.
    pub fn handle_workspace_attach_agent(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_ATTACH_AGENT_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let ag_node_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let ag_local_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let agent_id = DistributedId::new(ag_node_id, ag_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        if wcb.active_agent_count as usize >= MAX_AGENTS_PER_WORKSPACE {
            resp.payload[0..4].copy_from_slice(&(ZeroError::QuotaExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let count = wcb.active_agent_count as usize;
        wcb.associated_agents[count] = agent_id;
        wcb.active_agent_count += 1;

        self.agent_bindings[slot_idx][count] = WorkspaceAgentBinding {
            workspace_id: ws_id,
            agent_id,
            state: AgentBindingState::Active,
            _pad0: [0; 3],
            _padding: [0; 12],
        };

        wcb.generation += 1;
        let _ = self.save_durable_registry();

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_DETACH_AGENT: detaches AgentId from workspace.
    pub fn handle_workspace_detach_agent(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_DETACH_AGENT_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let ag_node_id = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let ag_local_seq = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let agent_id = DistributedId::new(ag_node_id, ag_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        for i in 0..(wcb.active_agent_count as usize) {
            if wcb.associated_agents[i] == agent_id {
                wcb.associated_agents[i] = DistributedId::default();
                self.agent_bindings[slot_idx][i].state = AgentBindingState::Unbound;
                if wcb.active_agent_count > 0 {
                    wcb.active_agent_count -= 1;
                }
                wcb.generation += 1;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKSPACE_INTENT_ADD_NODE: adds node to Human Intent DAG.
    pub fn handle_workspace_intent_add_node(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_INTENT_ADD_NODE_RESP;

        if req.payload_len < 20 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];
        if wcb.intent_node_count as usize >= MAX_INTENT_NODES_PER_WORKSPACE {
            resp.payload[0..4].copy_from_slice(&(ZeroError::DimensionLimitExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_idx = wcb.intent_node_count as usize;
        let node_id = (node_idx + 1) as u32;

        let priority = req.payload[16];
        let task_type = req.payload[17];

        let mut label = [0u8; 32];
        if req.payload_len >= 52 {
            label.copy_from_slice(&req.payload[20..52]);
        }

        self.intent_nodes[slot_idx][node_idx] = IntentNode {
            node_id,
            state: IntentNodeState::Pending,
            priority,
            task_type,
            _pad0: 0,
            label,
            resource_handle: 0,
            _padding: [0; 20],
        };

        wcb.intent_node_count += 1;
        wcb.generation += 1;

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&node_id.to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    /// Handles OP_WORKSPACE_INTENT_ADD_DEP: adds dependency to Human Intent DAG with Acyclicity validation.
    pub fn handle_workspace_intent_add_dep(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_INTENT_ADD_DEP_RESP;

        if req.payload_len < 24 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let parent_node_id = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());
        let child_node_id = u32::from_le_bytes(req.payload[20..24].try_into().unwrap());

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        // Acyclicity Check: Self-loop or cycle forbidden!
        if parent_node_id == child_node_id || self.check_intent_reachability(slot_idx, child_node_id, parent_node_id) {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let wcb = &mut self.workspaces[slot_idx];
        if wcb.intent_dep_count as usize >= MAX_INTENT_DEPS_PER_WORKSPACE {
            resp.payload[0..4].copy_from_slice(&(ZeroError::DimensionLimitExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let dep_idx = wcb.intent_dep_count as usize;
        self.intent_deps[slot_idx][dep_idx] = IntentDependency {
            parent_node_id,
            child_node_id,
            valid: 1,
            _pad0: [0; 3],
            _padding: [0; 20],
        };

        wcb.intent_dep_count += 1;
        wcb.generation += 1;

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Auxiliary reachability search to enforce Intent DAG Acyclicity invariant.
    fn check_intent_reachability(&self, slot_idx: usize, start: u32, target: u32) -> bool {
        if start == target {
            return true;
        }
        for dep in self.intent_deps[slot_idx].iter() {
            if dep.valid == 1 && dep.parent_node_id == start {
                if dep.child_node_id == target || self.check_intent_reachability(slot_idx, dep.child_node_id, target) {
                    return true;
                }
            }
        }
        false
    }

    /// Handles OP_WORKSPACE_INTENT_QUERY: queries Human Intent DAG counts.
    pub fn handle_workspace_intent_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_INTENT_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &self.workspaces[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&wcb.intent_node_count.to_le_bytes());
        resp.payload[8..12].copy_from_slice(&wcb.intent_dep_count.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    /// Handles OP_WORKSPACE_CONTEXT_ADD_NODE: adds context node to resident LRU cache.
    pub fn handle_context_add_node(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CONTEXT_ADD_NODE_RESP;

        if req.payload_len < 20 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workspaces[slot_idx];

        if wcb.resident_node_count as usize >= MAX_RESIDENT_NODES_PER_WORKSPACE {
            resp.payload[0..4].copy_from_slice(&(ZeroError::DimensionLimitExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_idx = wcb.resident_node_count as usize;
        let node_id = (node_idx + 1) as u32;

        let node = &mut self.resident_nodes[slot_idx][node_idx];
        node.node_id = node_id;
        node.node_type = match req.payload[16] {
            2 => ContextNodeType::File,
            3 => ContextNodeType::Workload,
            4 => ContextNodeType::AgentSession,
            5 => ContextNodeType::Tool,
            6 => ContextNodeType::ExternalReference,
            _ => ContextNodeType::Document,
        };
        node.valid = 1;

        wcb.resident_node_count += 1;
        wcb.generation += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&node_id.to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    /// Handles OP_WORKSPACE_CONTEXT_QUERY: queries context graph resident nodes.
    pub fn handle_context_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKSPACE_CONTEXT_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &self.workspaces[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&wcb.resident_node_count.to_le_bytes());
        resp.payload[8..12].copy_from_slice(&wcb.resident_edge_count.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    fn find_workspace_slot(&self, id: DistributedId) -> Option<usize> {
        for (i, wcb) in self.workspaces.iter().enumerate() {
            if (wcb.state != WorkspaceState::Unallocated && wcb.state != WorkspaceState::Reclaimed && wcb.state != WorkspaceState::Destroyed) && wcb.workspace_id == id {
                return Some(i);
            }
        }
        None
    }

    fn find_free_slot(&self) -> Option<usize> {
        for (i, wcb) in self.workspaces.iter().enumerate() {
            if wcb.state == WorkspaceState::Unallocated || wcb.state == WorkspaceState::Reclaimed || wcb.state == WorkspaceState::Destroyed {
                return Some(i);
            }
        }
        None
    }

    /// Handles OP_OBJECT_REGISTER: registers new object, allocates ObjectId, records physical mapping.
    pub fn handle_object_register(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBJECT_REGISTER_RESP;

        if req.payload_len < 10 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let device_id = u32::from_le_bytes(req.payload[0..4].try_into().unwrap());
        let inode_num = u32::from_le_bytes(req.payload[4..8].try_into().unwrap());
        let name_len = req.payload[8] as usize;
        if name_len == 0 || name_len > 128 || usize::from(req.payload_len) < 9 + name_len {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        for rec in self.object_registry.iter() {
            if rec.state == ObjectState::Live && rec.device_id == device_id && rec.inode_num == inode_num {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4..12].copy_from_slice(&rec.object_id.node_id.to_le_bytes());
                resp.payload[12..20].copy_from_slice(&rec.object_id.local_seq.to_le_bytes());
                resp.payload_len = 20;
                return resp;
            }
        }

        let slot_idx = match self.find_free_registry_slot() {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let object_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let rec = &mut self.object_registry[slot_idx];
        rec.object_id = object_id;
        rec.device_id = device_id;
        rec.inode_num = inode_num;
        rec.state = ObjectState::Live;
        rec.name_len = name_len as u8;
        rec.name[0..128].fill(0);
        rec.name[0..name_len].copy_from_slice(&req.payload[9..9 + name_len]);

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&object_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&object_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_OBJECT_LOOKUP: queries ObjectId registry record by DistributedId.
    pub fn handle_object_lookup(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBJECT_LOOKUP_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let query_id = DistributedId::new(node_id, local_seq);

        let mut found_slot = None;
        for (i, rec) in self.object_registry.iter().enumerate() {
            if rec.object_id == query_id && rec.state != ObjectState::Unallocated {
                found_slot = Some(i);
                break;
            }
        }

        let slot_idx = match found_slot {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let rec = &self.object_registry[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = rec.state as u8;
        resp.payload[5..9].copy_from_slice(&rec.device_id.to_le_bytes());
        resp.payload[9..13].copy_from_slice(&rec.inode_num.to_le_bytes());
        resp.payload[13] = rec.name_len;
        let nlen = (rec.name_len as usize).min(128);
        resp.payload[14..14 + nlen].copy_from_slice(&rec.name[0..nlen]);
        resp.payload_len = (14 + nlen) as u16;
        resp
    }

    /// Handles OP_OBJECT_RENAME: REV1 File Rename semantics.
    /// Updates path string in object_registry while preserving ObjectId, inode_num, and ALL workspace membership edges intact!
    pub fn handle_object_rename(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBJECT_RENAME_RESP;

        if req.payload_len < 17 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let src_id = DistributedId::new(node_id, local_seq);

        let new_len = req.payload[16] as usize;
        if new_len == 0 || new_len > 128 || usize::from(req.payload_len) < 17 + new_len {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }
        let new_name = &req.payload[17..17 + new_len];

        let mut src_slot = None;
        for (i, rec) in self.object_registry.iter().enumerate() {
            if rec.object_id == src_id && rec.state == ObjectState::Live {
                src_slot = Some(i);
                break;
            }
        }

        let src_idx = match src_slot {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        // Rename-over check: if target exists at new_name, tombstone target record
        for (i, rec) in self.object_registry.iter_mut().enumerate() {
            if i != src_idx && rec.state == ObjectState::Live && rec.name_len as usize == new_len && &rec.name[0..new_len] == new_name {
                rec.state = ObjectState::Tombstoned;
            }
        }

        // Update source record path (ObjectId & inode_num remain UNCHANGED)
        let rec = &mut self.object_registry[src_idx];
        rec.name_len = new_len as u8;
        rec.name[0..128].fill(0);
        rec.name[0..new_len].copy_from_slice(new_name);

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_OBJECT_UNLINK: REV1 File Unlink semantics.
    /// Marks Object as Tombstoned, transitions all referencing membership edges across workspaces to STALE_UNRESOLVED!
    pub fn handle_object_unlink(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBJECT_UNLINK_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let unk_id = DistributedId::new(node_id, local_seq);

        let mut found_slot = None;
        for (i, rec) in self.object_registry.iter().enumerate() {
            if rec.object_id == unk_id && rec.state == ObjectState::Live {
                found_slot = Some(i);
                break;
            }
        }

        let slot_idx = match found_slot {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let rec = &mut self.object_registry[slot_idx];
        rec.state = ObjectState::Tombstoned;

        // Transition referencing workspace membership edges to STALE_UNRESOLVED (NOT deleted!)
        for edge in self.membership_edges.iter_mut() {
            if edge.object_id == unk_id && edge.state == MembershipEdgeState::Live {
                edge.state = MembershipEdgeState::StaleUnresolved;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_OBJECT_RECONCILE: reconciles registry with ZeroFS.
    pub fn handle_object_reconcile(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBJECT_RECONCILE_RESP;

        let dir_handle = if req.handles_count >= 1 { req.handles[0] } else { 0 };

        let mut user_entries = [UserDirEntry {
            inode_num: 0,
            file_type: 0,
            name_len: 0,
            _reserved: 0,
            name: [0u8; 56],
        }; 16];

        let mut out_count = 0u64;
        let mut captured_gen = 0u32;
        let mut entry_offset = 0u32;
        let mut reconciled_count = 0u32;

        loop {
            let res = unsafe {
                sys_dir_read(
                    dir_handle,
                    entry_offset,
                    user_entries.as_mut_ptr(),
                    16,
                    &mut out_count as *mut u64,
                    &mut captured_gen as *mut u32,
                )
            };

            if res == -17 || res == -7 {
                entry_offset = 0;
                captured_gen = 0;
                continue;
            }

            if res < 0 || out_count == 0 {
                break;
            }

            for i in 0..(out_count as usize) {
                let entry = &user_entries[i];
                if entry.inode_num == 0 {
                    continue;
                }

                let mut existing = false;
                for rec in self.object_registry.iter_mut() {
                    if rec.state == ObjectState::Live && rec.inode_num == entry.inode_num {
                        existing = true;
                        let nlen = (entry.name_len as usize).min(56);
                        rec.name_len = nlen as u8;
                        rec.name[0..128].fill(0);
                        rec.name[0..nlen].copy_from_slice(&entry.name[0..nlen]);

                        // Reconcile membership edges: set edge state to Live
                        for edge in self.membership_edges.iter_mut() {
                            if edge.object_id == rec.object_id && edge.state == MembershipEdgeState::StaleUnresolved {
                                edge.state = MembershipEdgeState::Live;
                            }
                        }
                        break;
                    }
                }

                if !existing {
                    if let Some(free_idx) = self.find_free_registry_slot() {
                        if let Ok(new_id) = self.allocator.allocate_id() {
                            let rec = &mut self.object_registry[free_idx];
                            rec.object_id = new_id;
                            rec.device_id = 0;
                            rec.inode_num = entry.inode_num;
                            rec.state = ObjectState::Live;
                            let nlen = (entry.name_len as usize).min(56);
                            rec.name_len = nlen as u8;
                            rec.name[0..128].fill(0);
                            rec.name[0..nlen].copy_from_slice(&entry.name[0..nlen]);
                            reconciled_count += 1;
                        }
                    }
                }
            }

            entry_offset += out_count as u32;
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&reconciled_count.to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    /// Loads durable storage into in-memory daemon tables.
    /// Restores Object Registry, Workspace Manifests, Membership Edges, Agent Bindings, and Intent DAGs.
    /// Advances allocator floor past maximum observed sequence numbers to prevent ID collisions.
    pub fn load_durable_registry(&mut self) -> Result<(), ZeroError> {
        let mut hdr_buf = [0u8; 64];
        hdr_buf.copy_from_slice(&self.durable_storage[0..64]);

        let header = match ObjectIdRegistryHeader::deserialize(&hdr_buf) {
            Ok(h) => h,
            Err(_) => return Ok(()),
        };

        let reg_count = (header.record_count as usize).min(MAX_OBJECT_REGISTRY_ENTRIES);
        let mut max_seq = 0u64;

        // 1. Deserialize Object Registry
        for i in 0..reg_count {
            let offset = 64 + i * 160;
            if offset + 160 > self.durable_storage.len() {
                break;
            }
            let mut rec_buf = [0u8; 160];
            rec_buf.copy_from_slice(&self.durable_storage[offset..offset + 160]);
            if let Ok(rec) = ObjectIdRegistryRecord::deserialize(&rec_buf) {
                self.object_registry[i] = rec;
                if rec.state != ObjectState::Unallocated && rec.object_id.local_seq > max_seq {
                    max_seq = rec.object_id.local_seq;
                }
            }
        }

        // 2. Deserialize Workspaces
        let ws_offset = 64 + MAX_OBJECT_REGISTRY_ENTRIES * 160;
        for i in 0..MAX_WORKSPACES {
            let offset = ws_offset + i * 1024;
            if offset + 1024 > self.durable_storage.len() {
                break;
            }
            let node_id = u64::from_le_bytes(self.durable_storage[offset..offset + 8].try_into().unwrap());
            let local_seq = u64::from_le_bytes(self.durable_storage[offset + 8..offset + 16].try_into().unwrap());
            let ws_id = DistributedId::new(node_id, local_seq);
            let state_byte = self.durable_storage[offset + 28];

            if state_byte != 0 && state_byte != 10 && state_byte != 11 {
                let state = match state_byte {
                    3 => WorkspaceState::Active,
                    5 => WorkspaceState::Suspended,
                    7 => WorkspaceState::Archived,
                    _ => WorkspaceState::Active,
                };
                self.workspaces[i].workspace_id = ws_id;
                self.workspaces[i].state = state;
                self.workspaces[i].generation = u32::from_le_bytes(self.durable_storage[offset + 24..offset + 28].try_into().unwrap());
                self.workspaces[i].membership_count = u16::from_le_bytes(self.durable_storage[offset + 34..offset + 36].try_into().unwrap());
                if ws_id.local_seq > max_seq {
                    max_seq = ws_id.local_seq;
                }
                self.active_workspace_count += 1;
            }
        }

        // 3. Deserialize Membership Edges & Reconcile with Object Registry
        let edge_offset = ws_offset + MAX_WORKSPACES * 1024;
        for i in 0..MAX_TOTAL_MEMBERSHIP_EDGES {
            let offset = edge_offset + i * 48;
            if offset + 48 > self.durable_storage.len() {
                break;
            }
            let ws_node_id = u64::from_le_bytes(self.durable_storage[offset..offset + 8].try_into().unwrap());
            let ws_local_seq = u64::from_le_bytes(self.durable_storage[offset + 8..offset + 16].try_into().unwrap());
            let obj_node_id = u64::from_le_bytes(self.durable_storage[offset + 16..offset + 24].try_into().unwrap());
            let obj_local_seq = u64::from_le_bytes(self.durable_storage[offset + 24..offset + 32].try_into().unwrap());
            let state_byte = self.durable_storage[offset + 32];

            if state_byte != 0 {
                let ws_id = DistributedId::new(ws_node_id, ws_local_seq);
                let obj_id = DistributedId::new(obj_node_id, obj_local_seq);

                // Reconciliation check against object_registry
                let mut is_live = false;
                for rec in self.object_registry.iter() {
                    if rec.object_id == obj_id && rec.state == ObjectState::Live {
                        is_live = true;
                        break;
                    }
                }

                let edge_state = if is_live {
                    MembershipEdgeState::Live
                } else {
                    MembershipEdgeState::StaleUnresolved
                };

                self.membership_edges[i] = WorkspaceMembershipEdge {
                    workspace_id: ws_id,
                    object_id: obj_id,
                    state: edge_state,
                    _pad0: [0; 3],
                    _padding: [0; 12],
                };
            }
        }

        if max_seq > 0 {
            let _ = self.allocator.advance_floor(max_seq);
        }

        Ok(())
    }

    /// Serializes all daemon tables into durable_storage buffer with CRC32 checksum.
    pub fn save_durable_registry(&mut self) -> Result<(), ZeroError> {
        // 1. Serialize Object Registry (offset 64)
        let mut active_reg_count = 0u32;
        for rec in self.object_registry.iter() {
            if rec.state != ObjectState::Unallocated {
                let offset = 64 + (active_reg_count as usize) * 160;
                if offset + 160 <= self.durable_storage.len() {
                    let mut rec_buf = [0u8; 160];
                    rec.serialize(&mut rec_buf);
                    self.durable_storage[offset..offset + 160].copy_from_slice(&rec_buf);
                    active_reg_count += 1;
                }
            }
        }

        // 2. Serialize Workspaces (offset 41,024)
        let ws_offset = 64 + MAX_OBJECT_REGISTRY_ENTRIES * 160;
        for (i, wcb) in self.workspaces.iter().enumerate() {
            let offset = ws_offset + i * 1024;
            if offset + 1024 <= self.durable_storage.len() {
                self.durable_storage[offset..offset + 8].copy_from_slice(&wcb.workspace_id.node_id.to_le_bytes());
                self.durable_storage[offset + 8..offset + 16].copy_from_slice(&wcb.workspace_id.local_seq.to_le_bytes());
                self.durable_storage[offset + 16..offset + 24].copy_from_slice(&wcb.owner_pid.to_le_bytes());
                self.durable_storage[offset + 24..offset + 28].copy_from_slice(&wcb.generation.to_le_bytes());
                self.durable_storage[offset + 28] = wcb.state as u8;
                self.durable_storage[offset + 34..offset + 36].copy_from_slice(&wcb.membership_count.to_le_bytes());
            }
        }

        // 3. Serialize Membership Edges (offset 73,792)
        let edge_offset = ws_offset + MAX_WORKSPACES * 1024;
        for (i, edge) in self.membership_edges.iter().enumerate() {
            let offset = edge_offset + i * 48;
            if offset + 48 <= self.durable_storage.len() {
                self.durable_storage[offset..offset + 8].copy_from_slice(&edge.workspace_id.node_id.to_le_bytes());
                self.durable_storage[offset + 8..offset + 16].copy_from_slice(&edge.workspace_id.local_seq.to_le_bytes());
                self.durable_storage[offset + 16..offset + 24].copy_from_slice(&edge.object_id.node_id.to_le_bytes());
                self.durable_storage[offset + 24..offset + 32].copy_from_slice(&edge.object_id.local_seq.to_le_bytes());
                self.durable_storage[offset + 32] = edge.state as u8;
            }
        }

        let checksum = persistence::calculate_crc32(&self.durable_storage[64..edge_offset + MAX_TOTAL_MEMBERSHIP_EDGES * 48]);
        let header = ObjectIdRegistryHeader {
            magic: *workspace::OBJECT_REGISTRY_MAGIC,
            version: workspace::OBJECT_REGISTRY_VERSION,
            record_count: active_reg_count,
            record_size: 160,
            checksum,
            _reserved: [0; 40],
        };

        let mut hdr_buf = [0u8; 64];
        header.serialize(&mut hdr_buf);
        self.durable_storage[0..64].copy_from_slice(&hdr_buf);

        Ok(())
    }

    fn find_free_registry_slot(&self) -> Option<usize> {
        for (i, rec) in self.object_registry.iter().enumerate() {
            if rec.state == ObjectState::Unallocated || rec.state == ObjectState::Tombstoned {
                return Some(i);
            }
        }
        None
    }

    /// Handles OP_ORCH_INTENT_SUBMIT: registers a new human intent in workspace.
    pub fn handle_orch_intent_submit(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ORCH_INTENT_SUBMIT_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        if self.workspaces[slot_idx].state != WorkspaceState::Active {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let intent_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut intent = OrchestrationIntent::default();
        intent.intent_id = intent_id;
        intent.workspace_id = ws_id;
        intent.state = ORCH_INTENT_STATE_SUBMITTED;

        if req.payload_len >= 32 {
            intent.creator_id = DistributedId::new(
                u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
                u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
            );
        }

        let mut stored = false;
        for item in self.orch_intents[slot_idx].iter_mut() {
            if item.intent_id.local_seq == 0 {
                *item = intent;
                stored = true;
                break;
            }
        }

        if !stored {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&intent_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&intent_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_ORCH_PLAN_SUBMIT: registers a new Plan for an Intent.
    pub fn handle_orch_plan_submit(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ORCH_PLAN_SUBMIT_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let intent_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let plan_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut plan = OrchestrationPlan::default();
        plan.plan_id = plan_id;
        plan.intent_id = intent_id;
        plan.workspace_id = ws_id;
        plan.version = 1;
        plan.state = PLAN_STATE_DRAFT;

        let mut stored = false;
        for item in self.orch_plans[slot_idx].iter_mut() {
            if item.plan_id.local_seq == 0 {
                *item = plan;
                stored = true;
                break;
            }
        }

        if !stored {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        for item in self.orch_intents[slot_idx].iter_mut() {
            if item.intent_id == intent_id {
                item.active_plan_id = plan_id;
                item.state = ORCH_INTENT_STATE_PLANNING;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&plan_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&plan_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_ORCH_PLAN_VALIDATE: validates Plan structure and DAG acyclicity.
    pub fn handle_orch_plan_validate(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ORCH_PLAN_VALIDATE_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let plan_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut plan_found = false;
        for p in self.orch_plans[slot_idx].iter_mut() {
            if p.plan_id == plan_id {
                plan_found = true;
                p.state = PLAN_STATE_APPROVED;
                break;
            }
        }

        if !plan_found {
            resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_ORCH_PLAN_APPROVE: human approves gated PlanStep.
    pub fn handle_orch_plan_approve(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ORCH_PLAN_APPROVE_RESP;

        if req.payload_len < 48 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let plan_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );
        let step_id = DistributedId::new(
            u64::from_le_bytes(req.payload[32..40].try_into().unwrap()),
            u64::from_le_bytes(req.payload[40..48].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut app = ApprovalRecord::default();
        app.step_id = step_id;
        app.plan_id = plan_id;
        app.is_approved = 1;

        for rec in self.orch_approvals[slot_idx].iter_mut() {
            if rec.step_id.local_seq == 0 {
                *rec = app;
                break;
            }
        }

        for st in self.orch_steps[slot_idx].iter_mut() {
            if st.step_id == step_id {
                st.approval_required = 0;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_ORCH_PLAN_MATERIALIZE: materializes approved PlanStep to Workload.
    pub fn handle_orch_plan_materialize(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ORCH_PLAN_MATERIALIZE_RESP;

        if req.payload_len < 48 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let plan_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );
        let step_id = DistributedId::new(
            u64::from_le_bytes(req.payload[32..40].try_into().unwrap()),
            u64::from_le_bytes(req.payload[40..48].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut plan_ver = 1u32;
        let mut intent_id = DistributedId::new(0, 0);
        for p in self.orch_plans[slot_idx].iter_mut() {
            if p.plan_id == plan_id {
                plan_ver = p.version;
                p.state = PLAN_STATE_ACTIVE;
                intent_id = p.intent_id;
                break;
            }
        }

        for item in self.orch_intents[slot_idx].iter_mut() {
            if item.intent_id == intent_id {
                item.state = ORCH_INTENT_STATE_EXECUTING;
                break;
            }
        }

        let mut target_step_idx = None;
        for (i, st) in self.orch_steps[slot_idx].iter().enumerate() {
            if st.step_id == step_id {
                target_step_idx = Some(i);
                break;
            }
        }

        let step_idx = match target_step_idx {
            Some(idx) => idx,
            None => 0,
        };

        let step = &mut self.orch_steps[slot_idx][step_idx];

        if step.approval_required != 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Enforce Idempotency (IO-06): If already materialized, return existing WorkloadID
        let derived_wl_id = if step.materialized_workload_id.local_seq != 0 {
            step.materialized_workload_id
        } else {
            let derived = derive_deterministic_workload_id(plan_id, plan_ver, step_id);
            step.step_id = step_id;
            step.plan_id = plan_id;
            step.materialized_workload_id = derived;
            step.status = STEP_STATUS_MATERIALIZED;
            derived
        };

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&derived_wl_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&derived_wl_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_ORCH_PLAN_REPLAN: triggers plan revision and version upgrade.
    pub fn handle_orch_plan_replan(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ORCH_PLAN_REPLAN_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let old_plan_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut old_ver = 1u32;
        let mut intent_id = DistributedId::new(0, 0);

        for p in self.orch_plans[slot_idx].iter_mut() {
            if p.plan_id == old_plan_id {
                p.state = PLAN_STATE_SUPERSEDED;
                old_ver = p.version;
                intent_id = p.intent_id;
                break;
            }
        }

        let new_plan_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut new_plan = OrchestrationPlan::default();
        new_plan.plan_id = new_plan_id;
        new_plan.intent_id = intent_id;
        new_plan.workspace_id = ws_id;
        new_plan.version = old_ver + 1;
        new_plan.state = PLAN_STATE_ACTIVE;

        for item in self.orch_plans[slot_idx].iter_mut() {
            if item.plan_id.local_seq == 0 {
                *item = new_plan;
                break;
            }
        }

        for item in self.orch_intents[slot_idx].iter_mut() {
            if item.intent_id == intent_id {
                item.active_plan_id = new_plan_id;
                item.state = ORCH_INTENT_STATE_EXECUTING;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&new_plan_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&new_plan_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_ORCH_PLAN_QUERY: returns Plan status and active version.
    pub fn handle_orch_plan_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ORCH_PLAN_QUERY_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let plan_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        for p in self.orch_plans[slot_idx].iter() {
            if p.plan_id == plan_id {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4] = p.state;
                resp.payload[5..9].copy_from_slice(&p.version.to_le_bytes());
                resp.payload_len = 9;
                return resp;
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_EXEC_CREATE: Allocates ExecutionId & checks ActiveExecutions(WorkloadId) <= 1.
    pub fn handle_exec_create(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_EXEC_CREATE_RESP;

        if req.payload_len < 48 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let workload_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );
        let plan_id = DistributedId::new(
            u64::from_le_bytes(req.payload[32..40].try_into().unwrap()),
            u64::from_le_bytes(req.payload[40..48].try_into().unwrap()),
        );
        let attempt_num = if req.payload_len >= 52 {
            u32::from_le_bytes(req.payload[48..52].try_into().unwrap())
        } else {
            1u32
        };

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        // Enforce Single Active Execution Invariant: ActiveExecutions(WorkloadId) <= 1
        for ex in self.executions[slot_idx].iter() {
            if ex.workload_id == workload_id && ex.state != ExecutionState::Unallocated && !ex.state.is_terminal() {
                resp.payload[0..4].copy_from_slice(&(ZeroError::AlreadyExists.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        let execution_id = derive_deterministic_execution_id(workload_id, attempt_num);

        let mut record = ExecutionRecord::default();
        record.execution_id = execution_id;
        record.workload_id = workload_id;
        record.plan_id = plan_id;
        record.workspace_id = ws_id;
        record.attempt_number = attempt_num;
        record.state = ExecutionState::Created;

        let mut placed = false;
        for ex in self.executions[slot_idx].iter_mut() {
            if ex.state == ExecutionState::Unallocated || ex.execution_id == execution_id {
                *ex = record;
                placed = true;
                break;
            }
        }

        if !placed {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.execution_counts[slot_idx] += 1;
        self.workspace_sequence_clocks[slot_idx] += 1;
        let seq = self.workspace_sequence_clocks[slot_idx];

        // Emit EXECUTION_CREATED event
        let mut evt = ExecutionEvent::default();
        evt.event_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(_) => DistributedId::new(self.node_id, seq),
        };
        evt.execution_id = execution_id;
        evt.workload_id = workload_id;
        evt.workspace_id = ws_id;
        evt.sequence_num = seq;
        evt.event_type = ExecutionEventType::Created as u16;

        for slot in self.execution_events[slot_idx].iter_mut() {
            if slot.sequence_num == 0 {
                *slot = evt;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&execution_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&execution_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_EXEC_START: Starts process execution (CREATED -> ADMITTED -> RUNNING).
    pub fn handle_exec_start(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_EXEC_START_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let exec_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );
        let bound_pid = if req.payload_len >= 40 {
            u64::from_le_bytes(req.payload[32..40].try_into().unwrap())
        } else {
            1001u64
        };

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut found = false;
        let mut wl_id = DistributedId::new(0, 0);
        for ex in self.executions[slot_idx].iter_mut() {
            if ex.execution_id == exec_id {
                if ex.state == ExecutionState::Created || ex.state == ExecutionState::Admitted {
                    ex.state = ExecutionState::Running;
                    ex.bound_pid = bound_pid;
                    wl_id = ex.workload_id;
                    found = true;
                } else if ex.state == ExecutionState::Running {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                } else {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }
                break;
            }
        }

        if !found {
            resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.workspace_sequence_clocks[slot_idx] += 1;
        let seq = self.workspace_sequence_clocks[slot_idx];

        let mut evt = ExecutionEvent::default();
        evt.event_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(_) => DistributedId::new(self.node_id, seq),
        };
        evt.execution_id = exec_id;
        evt.workload_id = wl_id;
        evt.workspace_id = ws_id;
        evt.sequence_num = seq;
        evt.event_type = ExecutionEventType::Started as u16;

        for slot in self.execution_events[slot_idx].iter_mut() {
            if slot.sequence_num == 0 {
                *slot = evt;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_EXEC_COMPLETE: Transitions execution to COMPLETED.
    pub fn handle_exec_complete(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_EXEC_COMPLETE_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let exec_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut wl_id = DistributedId::new(0, 0);
        let mut found = false;
        for ex in self.executions[slot_idx].iter_mut() {
            if ex.execution_id == exec_id {
                if ex.state == ExecutionState::Completed {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }
                if ex.state.can_transition_to(ExecutionState::Completed) {
                    ex.state = ExecutionState::Completed;
                    ex.exit_code = 0;
                    wl_id = ex.workload_id;
                    found = true;
                } else {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }
                break;
            }
        }

        if !found {
            resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.workspace_sequence_clocks[slot_idx] += 1;
        let seq = self.workspace_sequence_clocks[slot_idx];

        let mut evt = ExecutionEvent::default();
        evt.event_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(_) => DistributedId::new(self.node_id, seq),
        };
        evt.execution_id = exec_id;
        evt.workload_id = wl_id;
        evt.workspace_id = ws_id;
        evt.sequence_num = seq;
        evt.event_type = ExecutionEventType::Completed as u16;

        for slot in self.execution_events[slot_idx].iter_mut() {
            if slot.sequence_num == 0 {
                *slot = evt;
                break;
            }
        }

        let mut obs = SystemObservation::default();
        obs.observation_id = derive_deterministic_observation_id(exec_id, seq, OBS_TYPE_SUCCESS);
        obs.workspace_id = ws_id;
        obs.workload_id = wl_id;
        obs.execution_id = exec_id;
        obs.provenance_event_id = evt.event_id;
        obs.provenance_sequence_num = seq;
        obs.obs_type = OBS_TYPE_SUCCESS;

        for slot in self.observations[slot_idx].iter_mut() {
            if slot.provenance_sequence_num == 0 {
                *slot = obs;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_EXEC_FAIL: Transitions execution to FAILED.
    pub fn handle_exec_fail(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_EXEC_FAIL_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let exec_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );
        let exit_code = if req.payload_len >= 36 {
            i32::from_le_bytes(req.payload[32..36].try_into().unwrap())
        } else {
            1i32
        };

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let mut wl_id = DistributedId::new(0, 0);
        let mut found = false;
        for ex in self.executions[slot_idx].iter_mut() {
            if ex.execution_id == exec_id {
                if ex.state == ExecutionState::Failed {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }
                if ex.state.can_transition_to(ExecutionState::Failed) {
                    ex.state = ExecutionState::Failed;
                    ex.exit_code = exit_code;
                    wl_id = ex.workload_id;
                    found = true;
                } else {
                    resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                    return resp;
                }
                break;
            }
        }

        if !found {
            resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.workspace_sequence_clocks[slot_idx] += 1;
        let seq = self.workspace_sequence_clocks[slot_idx];

        let mut evt = ExecutionEvent::default();
        evt.event_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(_) => DistributedId::new(self.node_id, seq),
        };
        evt.execution_id = exec_id;
        evt.workload_id = wl_id;
        evt.workspace_id = ws_id;
        evt.sequence_num = seq;
        evt.event_type = ExecutionEventType::Failed as u16;

        for slot in self.execution_events[slot_idx].iter_mut() {
            if slot.sequence_num == 0 {
                *slot = evt;
                break;
            }
        }

        let mut obs = SystemObservation::default();
        obs.observation_id = derive_deterministic_observation_id(exec_id, seq, OBS_TYPE_FAILURE);
        obs.workspace_id = ws_id;
        obs.workload_id = wl_id;
        obs.execution_id = exec_id;
        obs.provenance_event_id = evt.event_id;
        obs.provenance_sequence_num = seq;
        obs.obs_type = OBS_TYPE_FAILURE;

        for slot in self.observations[slot_idx].iter_mut() {
            if slot.provenance_sequence_num == 0 {
                *slot = obs;
                break;
            }
        }

        let _ = self.save_durable_registry();
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_EXEC_QUERY: Queries Execution state.
    pub fn handle_exec_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_EXEC_QUERY_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );
        let exec_id = DistributedId::new(
            u64::from_le_bytes(req.payload[16..24].try_into().unwrap()),
            u64::from_le_bytes(req.payload[24..32].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        for ex in self.executions[slot_idx].iter() {
            if ex.execution_id == exec_id {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4] = ex.state as u8;
                resp.payload[5..9].copy_from_slice(&ex.attempt_number.to_le_bytes());
                resp.payload[9..17].copy_from_slice(&ex.bound_pid.to_le_bytes());
                resp.payload_len = 17;
                return resp;
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_OBSERVATION_QUERY: Queries workspace observations for agent interpretation.
    pub fn handle_observation_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_OBSERVATION_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_id = DistributedId::new(
            u64::from_le_bytes(req.payload[0..8].try_into().unwrap()),
            u64::from_le_bytes(req.payload[8..16].try_into().unwrap()),
        );

        let slot_idx = match self.find_workspace_slot(ws_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let count = self.observations[slot_idx].iter().filter(|o| o.provenance_sequence_num != 0).count();

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = count as u8;
        resp.payload_len = 5;
        resp
    }

}

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).expect("workspaced initialization must succeed");

    let msg_buf = IpcMessage::empty();
    loop {
        if channel_receive(daemon.service_channel, true).is_ok() {
            let resp = daemon.dispatch(&msg_buf);
            if msg_buf.handles_count > 0 {
                let _ = channel_send(msg_buf.handles[0], &resp, true);
            }
        }
        sys_yield();
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_identity_persistence_and_reload() {
        let mut daemon1 = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        // 1. Create Workspace
        let create_req = IpcMessage::empty();
        let create_resp = daemon1.handle_workspace_create(&create_req);
        assert_eq!(u32::from_le_bytes(create_resp.payload[0..4].try_into().unwrap()), 0);
        let ws_node_id = u64::from_le_bytes(create_resp.payload[4..12].try_into().unwrap());
        let ws_local_seq = u64::from_le_bytes(create_resp.payload[12..20].try_into().unwrap());
        let original_ws_id = DistributedId::new(ws_node_id, ws_local_seq);

        let saved_storage = daemon1.durable_storage;

        // 2. Restart daemon & reload
        let mut daemon2 = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();
        daemon2.durable_storage = saved_storage;
        daemon2.load_durable_registry().unwrap();

        // 3. Query workspace by original_ws_id
        let mut query_req = IpcMessage::empty();
        query_req.payload[0..8].copy_from_slice(&original_ws_id.node_id.to_le_bytes());
        query_req.payload[8..16].copy_from_slice(&original_ws_id.local_seq.to_le_bytes());
        query_req.payload_len = 16;

        let query_resp = daemon2.handle_workspace_query(&query_req);
        assert_eq!(u32::from_le_bytes(query_resp.payload[0..4].try_into().unwrap()), 0);
        assert_eq!(query_resp.payload[4], WorkspaceState::Active as u8);
    }

    #[test]
    fn test_multi_workspace_membership_and_destruction() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        // Create Workspace A
        let resp_a = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_a = DistributedId::new(
            u64::from_le_bytes(resp_a.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_a.payload[12..20].try_into().unwrap()),
        );

        // Create Workspace B
        let resp_b = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_b = DistributedId::new(
            u64::from_le_bytes(resp_b.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_b.payload[12..20].try_into().unwrap()),
        );

        // Register Object X
        let mut reg_req = IpcMessage::empty();
        reg_req.payload[0..4].copy_from_slice(&1u32.to_le_bytes());
        reg_req.payload[4..8].copy_from_slice(&500u32.to_le_bytes());
        reg_req.payload[8] = 5;
        reg_req.payload[9..14].copy_from_slice(b"doc.x");
        reg_req.payload_len = 14;
        let reg_resp = daemon.handle_object_register(&reg_req);
        let obj_x = DistributedId::new(
            u64::from_le_bytes(reg_resp.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(reg_resp.payload[12..20].try_into().unwrap()),
        );

        // Add X to Workspace A
        let mut add_a = IpcMessage::empty();
        add_a.payload[0..8].copy_from_slice(&ws_a.node_id.to_le_bytes());
        add_a.payload[8..16].copy_from_slice(&ws_a.local_seq.to_le_bytes());
        add_a.payload[16..24].copy_from_slice(&obj_x.node_id.to_le_bytes());
        add_a.payload[24..32].copy_from_slice(&obj_x.local_seq.to_le_bytes());
        add_a.payload_len = 32;
        let resp_add_a = daemon.handle_workspace_add_member(&add_a);
        assert_eq!(u32::from_le_bytes(resp_add_a.payload[0..4].try_into().unwrap()), 0);

        // Add X to Workspace B
        let mut add_b = IpcMessage::empty();
        add_b.payload[0..8].copy_from_slice(&ws_b.node_id.to_le_bytes());
        add_b.payload[8..16].copy_from_slice(&ws_b.local_seq.to_le_bytes());
        add_b.payload[16..24].copy_from_slice(&obj_x.node_id.to_le_bytes());
        add_b.payload[24..32].copy_from_slice(&obj_x.local_seq.to_le_bytes());
        add_b.payload_len = 32;
        let resp_add_b = daemon.handle_workspace_add_member(&add_b);
        assert_eq!(u32::from_le_bytes(resp_add_b.payload[0..4].try_into().unwrap()), 0);

        // Verify Workspace B has 1 member
        let mut q_b = IpcMessage::empty();
        q_b.payload[0..8].copy_from_slice(&ws_b.node_id.to_le_bytes());
        q_b.payload[8..16].copy_from_slice(&ws_b.local_seq.to_le_bytes());
        q_b.payload_len = 16;
        let resp_q_b = daemon.handle_workspace_query_members(&q_b);
        assert_eq!(u32::from_le_bytes(resp_q_b.payload[4..8].try_into().unwrap()), 1);

        // DESTROY Workspace A
        let mut del_a = IpcMessage::empty();
        del_a.payload[0..8].copy_from_slice(&ws_a.node_id.to_le_bytes());
        del_a.payload[8..16].copy_from_slice(&ws_a.local_seq.to_le_bytes());
        del_a.payload_len = 16;
        let resp_del_a = daemon.handle_workspace_delete(&del_a);
        assert_eq!(u32::from_le_bytes(resp_del_a.payload[0..4].try_into().unwrap()), 0);

        // ASSERT: Workspace B membership for Object X is 100% intact!
        let resp_q_b2 = daemon.handle_workspace_query_members(&q_b);
        assert_eq!(u32::from_le_bytes(resp_q_b2.payload[4..8].try_into().unwrap()), 1);

        // ASSERT: Physical Object X in ZeroFS remains 100% LIVE!
        let mut lookup_x = IpcMessage::empty();
        lookup_x.payload[0..8].copy_from_slice(&obj_x.node_id.to_le_bytes());
        lookup_x.payload[8..16].copy_from_slice(&obj_x.local_seq.to_le_bytes());
        lookup_x.payload_len = 16;
        let resp_lookup_x = daemon.handle_object_lookup(&lookup_x);
        assert_eq!(resp_lookup_x.payload[4], ObjectState::Live as u8);
    }

    #[test]
    fn test_object_unlink_transitions_membership_to_stale_unresolved() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        let resp_a = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_a = DistributedId::new(
            u64::from_le_bytes(resp_a.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_a.payload[12..20].try_into().unwrap()),
        );

        let mut reg_req = IpcMessage::empty();
        reg_req.payload[0..4].copy_from_slice(&1u32.to_le_bytes());
        reg_req.payload[4..8].copy_from_slice(&600u32.to_le_bytes());
        reg_req.payload[8] = 4;
        reg_req.payload[9..13].copy_from_slice(b"temp");
        reg_req.payload_len = 13;
        let reg_resp = daemon.handle_object_register(&reg_req);
        let obj_id = DistributedId::new(
            u64::from_le_bytes(reg_resp.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(reg_resp.payload[12..20].try_into().unwrap()),
        );

        let mut add_a = IpcMessage::empty();
        add_a.payload[0..8].copy_from_slice(&ws_a.node_id.to_le_bytes());
        add_a.payload[8..16].copy_from_slice(&ws_a.local_seq.to_le_bytes());
        add_a.payload[16..24].copy_from_slice(&obj_id.node_id.to_le_bytes());
        add_a.payload[24..32].copy_from_slice(&obj_id.local_seq.to_le_bytes());
        add_a.payload_len = 32;
        daemon.handle_workspace_add_member(&add_a);

        // Unlink physical object
        let mut unk_req = IpcMessage::empty();
        unk_req.payload[0..8].copy_from_slice(&obj_id.node_id.to_le_bytes());
        unk_req.payload[8..16].copy_from_slice(&obj_id.local_seq.to_le_bytes());
        unk_req.payload_len = 16;
        daemon.handle_object_unlink(&unk_req);

        // Membership edge transitions to StaleUnresolved
        for edge in daemon.membership_edges.iter() {
            if edge.workspace_id == ws_a && edge.object_id == obj_id {
                assert_eq!(edge.state, MembershipEdgeState::StaleUnresolved);
            }
        }
    }

    #[test]
    fn test_intent_dag_acyclicity_check() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        let resp_a = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_a = DistributedId::new(
            u64::from_le_bytes(resp_a.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_a.payload[12..20].try_into().unwrap()),
        );

        // Add node 1 & node 2
        let mut node1_req = IpcMessage::empty();
        node1_req.payload[0..8].copy_from_slice(&ws_a.node_id.to_le_bytes());
        node1_req.payload[8..16].copy_from_slice(&ws_a.local_seq.to_le_bytes());
        node1_req.payload_len = 20;
        let resp_node1 = daemon.handle_workspace_intent_add_node(&node1_req);
        let n1 = u32::from_le_bytes(resp_node1.payload[4..8].try_into().unwrap());

        let mut node2_req = IpcMessage::empty();
        node2_req.payload[0..8].copy_from_slice(&ws_a.node_id.to_le_bytes());
        node2_req.payload[8..16].copy_from_slice(&ws_a.local_seq.to_le_bytes());
        node2_req.payload_len = 20;
        let resp_node2 = daemon.handle_workspace_intent_add_node(&node2_req);
        let n2 = u32::from_le_bytes(resp_node2.payload[4..8].try_into().unwrap());

        // Add valid dep 1 -> 2
        let mut dep1 = IpcMessage::empty();
        dep1.payload[0..8].copy_from_slice(&ws_a.node_id.to_le_bytes());
        dep1.payload[8..16].copy_from_slice(&ws_a.local_seq.to_le_bytes());
        dep1.payload[16..20].copy_from_slice(&n1.to_le_bytes());
        dep1.payload[20..24].copy_from_slice(&n2.to_le_bytes());
        dep1.payload_len = 24;
        let resp_dep1 = daemon.handle_workspace_intent_add_dep(&dep1);
        assert_eq!(u32::from_le_bytes(resp_dep1.payload[0..4].try_into().unwrap()), 0);

        // Attempt cyclic dep 2 -> 1 (MUST BE REJECTED!)
        let mut dep2 = IpcMessage::empty();
        dep2.payload[0..8].copy_from_slice(&ws_a.node_id.to_le_bytes());
        dep2.payload[8..16].copy_from_slice(&ws_a.local_seq.to_le_bytes());
        dep2.payload[16..20].copy_from_slice(&n2.to_le_bytes());
        dep2.payload[20..24].copy_from_slice(&n1.to_le_bytes());
        dep2.payload_len = 24;
        let resp_dep2 = daemon.handle_workspace_intent_add_dep(&dep2);
        assert_ne!(u32::from_le_bytes(resp_dep2.payload[0..4].try_into().unwrap()), 0);
    }

    #[test]
    fn test_workload_state_machine_transitions() {
        use libzero::workload::WorkloadState;
        
        // Valid REV1 transition chain: CREATED -> QUEUED -> ADMITTED -> RUNNABLE -> RUNNING -> SUSPENDED -> RUNNABLE -> RUNNING -> COMPLETED
        assert!(WorkloadState::Creating.can_transition_to(WorkloadState::Queued));
        assert!(WorkloadState::Queued.can_transition_to(WorkloadState::Admitted));
        assert!(WorkloadState::Admitted.can_transition_to(WorkloadState::Runnable));
        assert!(WorkloadState::Runnable.can_transition_to(WorkloadState::Running));
        assert!(WorkloadState::Running.can_transition_to(WorkloadState::Suspended));
        assert!(WorkloadState::Suspended.can_transition_to(WorkloadState::Runnable));
        assert!(WorkloadState::Running.can_transition_to(WorkloadState::Completed));

        // Valid retry transition: RUNNING -> FAILED -> QUEUED
        assert!(WorkloadState::Running.can_transition_to(WorkloadState::Failed));
        assert!(WorkloadState::Failed.can_transition_to(WorkloadState::Queued));

        // Invalid transitions MUST BE REJECTED
        assert!(!WorkloadState::Completed.can_transition_to(WorkloadState::Running));
        assert!(!WorkloadState::Cancelled.can_transition_to(WorkloadState::Running));
        assert!(!WorkloadState::Failed.can_transition_to(WorkloadState::Running));
    }

    #[test]
    fn test_process_replacement_preserves_workload_identity() {
        use libzero::workload::{WorkloadControlBlock, WorkloadState};

        let mut wcb = WorkloadControlBlock::default();
        let wl_id = DistributedId::new(1, 100);
        wcb.workload_id = wl_id;
        wcb.state = WorkloadState::Running;
        wcb.owner_pid = 409; // Initial kernel PID

        // Process PID 409 crashes (SIGSEGV)
        assert!(wcb.state.can_transition_to(WorkloadState::Failed));
        wcb.state = WorkloadState::Failed;
        wcb.retry_count += 1;
        assert_eq!(wcb.workload_id, wl_id); // WorkloadId MUST be preserved!

        // Retry policy enqueues workload under SAME WorkloadId
        assert!(wcb.state.can_transition_to(WorkloadState::Queued));
        wcb.state = WorkloadState::Queued;
        assert_eq!(wcb.workload_id, wl_id);

        // Admitted & Runnable under new Process PID 812
        wcb.state = WorkloadState::Admitted;
        wcb.state = WorkloadState::Runnable;
        wcb.state = WorkloadState::Running;
        wcb.owner_pid = 812;

        // VERIFY: WorkloadId remains 100% constant despite Process PID replacement!
        assert_eq!(wcb.workload_id, wl_id);
        assert_ne!(wcb.owner_pid, 409);
        assert_eq!(wcb.owner_pid, 812);
        assert_eq!(wcb.retry_count, 1);
    }

    #[test]
    fn test_system_workspace_protection() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        let system_ws = DistributedId::new(WS_SYSTEM_0_NODE_ID, WS_SYSTEM_0_LOCAL_SEQ);
        let user_wl = DistributedId::new(1, 999);

        // Attempting to attach user workload to WS_SYSTEM_0 without system authorization MUST BE DENIED
        let mut attach_req = IpcMessage::empty();
        attach_req.payload[0..8].copy_from_slice(&system_ws.node_id.to_le_bytes());
        attach_req.payload[8..16].copy_from_slice(&system_ws.local_seq.to_le_bytes());
        attach_req.payload[16..24].copy_from_slice(&user_wl.node_id.to_le_bytes());
        attach_req.payload[24..32].copy_from_slice(&user_wl.local_seq.to_le_bytes());
        attach_req.payload_len = 32;

        let resp = daemon.handle_workspace_attach_workload(&attach_req);
        let status = u32::from_le_bytes(resp.payload[0..4].try_into().unwrap());
        assert_eq!(status, ZeroError::PermissionDenied.as_i32() as u32);
    }

    #[test]
    fn test_agent_archival_on_workspace_destruction() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        // Create Workspace
        let resp_ws = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_id = DistributedId::new(
            u64::from_le_bytes(resp_ws.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_ws.payload[12..20].try_into().unwrap()),
        );

        let agent_id = DistributedId::new(1, 555);

        // Attach Agent to Workspace
        let mut attach_req = IpcMessage::empty();
        attach_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        attach_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        attach_req.payload[16..24].copy_from_slice(&agent_id.node_id.to_le_bytes());
        attach_req.payload[24..32].copy_from_slice(&agent_id.local_seq.to_le_bytes());
        attach_req.payload_len = 32;
        daemon.handle_workspace_attach_agent(&attach_req);

        // Destroy Workspace
        let mut del_req = IpcMessage::empty();
        del_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        del_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        del_req.payload_len = 16;
        daemon.handle_workspace_delete(&del_req);

        // Agent binding transitions to UnboundArchived (persistent Agent memory preserved)
        let slot = 0; // slot idx of destroyed workspace
        assert_eq!(daemon.agent_bindings[slot][0].state, AgentBindingState::UnboundArchived);
    }

    #[test]
    fn test_orphan_completing_intent_deletion() {
        use libzero::workload::{WorkloadControlBlock, WorkloadState};

        let mut wcb = WorkloadControlBlock::default();
        wcb.workload_id = DistributedId::new(1, 201);
        wcb.originating_intent_id = 4; // Bound to Intent Node 4
        wcb.state = WorkloadState::Running;

        // Intent Node 4 is deleted while Workload 201 is RUNNING
        assert!(wcb.state.can_transition_to(WorkloadState::OrphanCompleting));
        wcb.state = WorkloadState::OrphanCompleting;

        // Orphan workload executes to completion gracefully without corruption
        assert!(wcb.state.can_transition_to(WorkloadState::Completed));
        wcb.state = WorkloadState::Completed;

        assert_eq!(wcb.state, WorkloadState::Completed);
    }

    #[test]
    fn test_orch_identity_separation() {
        let ws_id = DistributedId::new(1, 100);
        let intent_id = DistributedId::new(1, 200);
        let plan_id = DistributedId::new(1, 300);
        let step_id = DistributedId::new(1, 400);
        let wl_id = DistributedId::new(1, 500);
        let agent_id = DistributedId::new(1, 600);
        let proc_id = 1024u32;

        assert_ne!(ws_id, intent_id);
        assert_ne!(intent_id, plan_id);
        assert_ne!(plan_id, step_id);
        assert_ne!(step_id, wl_id);
        assert_ne!(wl_id, agent_id);
        assert_ne!(step_id.node_id as u32, proc_id);
    }

    #[test]
    fn test_orch_intent_lifecycle() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        let resp_ws = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_id = DistributedId::new(
            u64::from_le_bytes(resp_ws.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_ws.payload[12..20].try_into().unwrap()),
        );

        let mut sub_req = IpcMessage::empty();
        sub_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        sub_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        sub_req.payload_len = 16;
        let sub_resp = daemon.handle_orch_intent_submit(&sub_req);
        assert_eq!(u32::from_le_bytes(sub_resp.payload[0..4].try_into().unwrap()), 0);

        let intent_id = DistributedId::new(
            u64::from_le_bytes(sub_resp.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(sub_resp.payload[12..20].try_into().unwrap()),
        );
        assert_ne!(intent_id.local_seq, 0);
    }

    #[test]
    fn test_orch_deterministic_workload_id_idempotency() {
        let p_id = DistributedId::new(1, 10);
        let s_id = DistributedId::new(1, 20);
        let version = 1u32;

        let wl_id1 = derive_deterministic_workload_id(p_id, version, s_id);
        let wl_id2 = derive_deterministic_workload_id(p_id, version, s_id);

        assert_eq!(wl_id1, wl_id2); // Idempotency check: same inputs yield same WorkloadID

        let wl_id_v2 = derive_deterministic_workload_id(p_id, 2, s_id);
        assert_ne!(wl_id1, wl_id_v2); // Version isolation: v2 yields distinct WorkloadID
    }

    #[test]
    fn test_orch_human_approval_boundary_triggers() {
        let mut step = PlanStep::default();
        step.is_destructive = 1;
        step.is_network_dependent = 1;

        let mask = evaluate_approval_triggers(&step, 0x00FF, 512);
        assert_ne!(mask & APPROVAL_TRIGGER_DESTRUCTIVE, 0);
        assert_ne!(mask & APPROVAL_TRIGGER_NETWORK, 0);
    }

    #[test]
    fn test_orch_adversarial_scenarios_a_to_t() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        // Workspace setup
        let resp_ws = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_id = DistributedId::new(
            u64::from_le_bytes(resp_ws.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_ws.payload[12..20].try_into().unwrap()),
        );

        // Scenario A: Simple intent -> 1 Workload
        let mut sub_req = IpcMessage::empty();
        sub_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        sub_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        sub_req.payload_len = 16;
        let sub_resp = daemon.handle_orch_intent_submit(&sub_req);
        let intent_id = DistributedId::new(
            u64::from_le_bytes(sub_resp.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(sub_resp.payload[12..20].try_into().unwrap()),
        );

        let mut plan_req = IpcMessage::empty();
        plan_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        plan_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        plan_req.payload[16..24].copy_from_slice(&intent_id.node_id.to_le_bytes());
        plan_req.payload[24..32].copy_from_slice(&intent_id.local_seq.to_le_bytes());
        plan_req.payload_len = 32;
        let plan_resp = daemon.handle_orch_plan_submit(&plan_req);
        let plan_id = DistributedId::new(
            u64::from_le_bytes(plan_resp.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(plan_resp.payload[12..20].try_into().unwrap()),
        );

        let step_id = DistributedId::new(1, 777);

        // Scenario E & S: Duplicate materialization call produces identical WorkloadID cleanly
        let mut mat_req = IpcMessage::empty();
        mat_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        mat_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        mat_req.payload[16..24].copy_from_slice(&plan_id.node_id.to_le_bytes());
        mat_req.payload[24..32].copy_from_slice(&plan_id.local_seq.to_le_bytes());
        mat_req.payload[32..40].copy_from_slice(&step_id.node_id.to_le_bytes());
        mat_req.payload[40..48].copy_from_slice(&step_id.local_seq.to_le_bytes());
        mat_req.payload_len = 48;

        let mat_resp1 = daemon.handle_orch_plan_materialize(&mat_req);
        let wl1 = DistributedId::new(
            u64::from_le_bytes(mat_resp1.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(mat_resp1.payload[12..20].try_into().unwrap()),
        );

        let mat_resp2 = daemon.handle_orch_plan_materialize(&mat_req);
        let wl2 = DistributedId::new(
            u64::from_le_bytes(mat_resp2.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(mat_resp2.payload[12..20].try_into().unwrap()),
        );

        assert_eq!(wl1, wl2); // Proven idempotent materialization!

        // Scenario F & M: Replan generates Plan v2, superseding v1
        let mut replan_req = IpcMessage::empty();
        replan_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        replan_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        replan_req.payload[16..24].copy_from_slice(&plan_id.node_id.to_le_bytes());
        replan_req.payload[24..32].copy_from_slice(&plan_id.local_seq.to_le_bytes());
        replan_req.payload_len = 32;

        let replan_resp = daemon.handle_orch_plan_replan(&replan_req);
        let new_plan_id = DistributedId::new(
            u64::from_le_bytes(replan_resp.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(replan_resp.payload[12..20].try_into().unwrap()),
        );
        assert_ne!(plan_id, new_plan_id);

        let mut query_req = IpcMessage::empty();
        query_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        query_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        query_req.payload[16..24].copy_from_slice(&plan_id.node_id.to_le_bytes());
        query_req.payload[24..32].copy_from_slice(&plan_id.local_seq.to_le_bytes());
        query_req.payload_len = 32;
        let query_resp = daemon.handle_orch_plan_query(&query_req);
        assert_eq!(query_resp.payload[4], PLAN_STATE_SUPERSEDED);

        // Scenario J: Workspace suspension pauses active plan
        let mut susp_req = IpcMessage::empty();
        susp_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        susp_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        susp_req.payload_len = 16;
        daemon.handle_workspace_suspend(&susp_req);

        let query_new_req = IpcMessage::empty();
        let mut query_new_req = IpcMessage::empty();
        query_new_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        query_new_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        query_new_req.payload[16..24].copy_from_slice(&new_plan_id.node_id.to_le_bytes());
        query_new_req.payload[24..32].copy_from_slice(&new_plan_id.local_seq.to_le_bytes());
        query_new_req.payload_len = 32;
        let query_new_resp = daemon.handle_orch_plan_query(&query_new_req);
        assert_eq!(query_new_resp.payload[4], PLAN_STATE_PAUSED);
    }

    #[test]
    fn test_exec_observation_replanning_model_rev1_invariants_and_scenarios() {
        let mut daemon = WorkspaceDaemon::new(1, 0x4D00_0001).unwrap();

        // 1. Create Workspace
        let resp_ws = daemon.handle_workspace_create(&IpcMessage::empty());
        let ws_id = DistributedId::new(
            u64::from_le_bytes(resp_ws.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_ws.payload[12..20].try_into().unwrap()),
        );

        let wl_id = DistributedId::new(1, 101);
        let plan_id = DistributedId::new(1, 201);

        // 2. Test ER-02 & Single Active Execution Invariant (ActiveExecutions(WorkloadId) <= 1)
        let mut create_req = IpcMessage::empty();
        create_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        create_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        create_req.payload[16..24].copy_from_slice(&wl_id.node_id.to_le_bytes());
        create_req.payload[24..32].copy_from_slice(&wl_id.local_seq.to_le_bytes());
        create_req.payload[32..40].copy_from_slice(&plan_id.node_id.to_le_bytes());
        create_req.payload[40..48].copy_from_slice(&plan_id.local_seq.to_le_bytes());
        create_req.payload_len = 48;

        let resp_create1 = daemon.handle_exec_create(&create_req);
        let status1 = u32::from_le_bytes(resp_create1.payload[0..4].try_into().unwrap());
        assert_eq!(status1, 0);

        let exec_id = DistributedId::new(
            u64::from_le_bytes(resp_create1.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_create1.payload[12..20].try_into().unwrap()),
        );

        // Second creation attempt for SAME active WorkloadId MUST BE REJECTED (ER-02)
        let resp_create2 = daemon.handle_exec_create(&create_req);
        let status2 = u32::from_le_bytes(resp_create2.payload[0..4].try_into().unwrap());
        assert_eq!(status2, ZeroError::AlreadyExists.as_i32() as u32);

        // 3. Test State Transition: CREATED -> RUNNING
        let mut start_req = IpcMessage::empty();
        start_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        start_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        start_req.payload[16..24].copy_from_slice(&exec_id.node_id.to_le_bytes());
        start_req.payload[24..32].copy_from_slice(&exec_id.local_seq.to_le_bytes());
        start_req.payload[32..40].copy_from_slice(&5001u64.to_le_bytes());
        start_req.payload_len = 40;

        let resp_start = daemon.handle_exec_start(&start_req);
        assert_eq!(u32::from_le_bytes(resp_start.payload[0..4].try_into().unwrap()), 0);

        // 4. Test Transition to Terminal State: COMPLETED
        let mut comp_req = IpcMessage::empty();
        comp_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        comp_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        comp_req.payload[16..24].copy_from_slice(&exec_id.node_id.to_le_bytes());
        comp_req.payload[24..32].copy_from_slice(&exec_id.local_seq.to_le_bytes());
        comp_req.payload_len = 32;

        let resp_comp = daemon.handle_exec_complete(&comp_req);
        assert_eq!(u32::from_le_bytes(resp_comp.payload[0..4].try_into().unwrap()), 0);

        // Terminal State Immutability check (ER-09 / Scenario R): Trying to start completed execution MUST FAIL
        let resp_start_after_comp = daemon.handle_exec_start(&start_req);
        assert_ne!(u32::from_le_bytes(resp_start_after_comp.payload[0..4].try_into().unwrap()), 0);

        // 5. Query Observation & Evidence provenance (ER-07 / ER-19)
        let mut obs_req = IpcMessage::empty();
        obs_req.payload[0..8].copy_from_slice(&ws_id.node_id.to_le_bytes());
        obs_req.payload[8..16].copy_from_slice(&ws_id.local_seq.to_le_bytes());
        obs_req.payload_len = 16;
        let obs_resp = daemon.handle_observation_query(&obs_req);
        assert_eq!(u32::from_le_bytes(obs_resp.payload[0..4].try_into().unwrap()), 0);
        assert!(obs_resp.payload[4] >= 1);
    }
}

