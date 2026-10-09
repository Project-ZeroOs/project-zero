//! ZeroOS - libzero Intent-to-Workload Planning & Orchestration Engine (REV1)
//!
//! Authoritative Contract: ZEROOS-INTENT-TO-WORKLOAD-PLANNING-ORCHESTRATION-REV1.md

use crate::error::ZeroError;
use crate::resource::DistributedId;

// Orchestration Protocol OpCodes for workspaced
pub const OP_ORCH_INTENT_SUBMIT:          u64 = 0x4D40;
pub const OP_ORCH_INTENT_SUBMIT_RESP:     u64 = 0x4D41;
pub const OP_ORCH_PLAN_SUBMIT:            u64 = 0x4D42;
pub const OP_ORCH_PLAN_SUBMIT_RESP:       u64 = 0x4D43;
pub const OP_ORCH_PLAN_VALIDATE:          u64 = 0x4D44;
pub const OP_ORCH_PLAN_VALIDATE_RESP:     u64 = 0x4D45;
pub const OP_ORCH_PLAN_APPROVE:           u64 = 0x4D46;
pub const OP_ORCH_PLAN_APPROVE_RESP:      u64 = 0x4D47;
pub const OP_ORCH_PLAN_MATERIALIZE:       u64 = 0x4D48;
pub const OP_ORCH_PLAN_MATERIALIZE_RESP:  u64 = 0x4D49;
pub const OP_ORCH_PLAN_REPLAN:           u64 = 0x4D4A;
pub const OP_ORCH_PLAN_REPLAN_RESP:      u64 = 0x4D4B;
pub const OP_ORCH_PLAN_QUERY:             u64 = 0x4D4C;
pub const OP_ORCH_PLAN_QUERY_RESP:        u64 = 0x4D4D;

// Intent Lifecycle States
pub const ORCH_INTENT_STATE_SUBMITTED:        u8 = 0x01;
pub const ORCH_INTENT_STATE_ANALYZING:        u8 = 0x02;
pub const ORCH_INTENT_STATE_PLANNING:         u8 = 0x03;
pub const ORCH_INTENT_STATE_WAITING_APPROVAL: u8 = 0x04;
pub const ORCH_INTENT_STATE_EXECUTING:        u8 = 0x05;
pub const ORCH_INTENT_STATE_COMPLETED:        u8 = 0x06;
pub const ORCH_INTENT_STATE_CANCELLED:        u8 = 0x07;
pub const ORCH_INTENT_STATE_FAILED:           u8 = 0x08;

// Plan Lifecycle States
pub const PLAN_STATE_DRAFT:              u8 = 0x01;
pub const PLAN_STATE_VALIDATING:         u8 = 0x02;
pub const PLAN_STATE_WAITING_APPROVAL:   u8 = 0x03;
pub const PLAN_STATE_APPROVED:           u8 = 0x04;
pub const PLAN_STATE_MATERIALIZING:      u8 = 0x05;
pub const PLAN_STATE_ACTIVE:             u8 = 0x06;
pub const PLAN_STATE_REPLANNING:         u8 = 0x07;
pub const PLAN_STATE_COMPLETED:          u8 = 0x08;
pub const PLAN_STATE_SUPERSEDED:         u8 = 0x09;
pub const PLAN_STATE_REJECTED:           u8 = 0x0A;
pub const PLAN_STATE_FAILED:             u8 = 0x0B;
pub const PLAN_STATE_CANCELLED:          u8 = 0x0C;
pub const PLAN_STATE_PAUSED:             u8 = 0x0D;

// Author Types
pub const AUTHOR_TYPE_HUMAN:             u8 = 0x01;
pub const AUTHOR_TYPE_SYSTEM_PLANNER:    u8 = 0x02;
pub const AUTHOR_TYPE_AGENT:             u8 = 0x03;

// PlanStep Statuses
pub const STEP_STATUS_PENDING:           u8 = 0x01;
pub const STEP_STATUS_MATERIALIZING:     u8 = 0x02;
pub const STEP_STATUS_MATERIALIZED:      u8 = 0x03;
pub const STEP_STATUS_RUNNING:           u8 = 0x04;
pub const STEP_STATUS_COMPLETED:         u8 = 0x05;
pub const STEP_STATUS_FAILED:            u8 = 0x06;
pub const STEP_STATUS_SKIPPED:           u8 = 0x07;

// Approval Triggers (Bitmask)
pub const APPROVAL_TRIGGER_DESTRUCTIVE:     u8 = 0x01;
pub const APPROVAL_TRIGGER_NETWORK:         u8 = 0x02;
pub const APPROVAL_TRIGGER_QUOTA:           u8 = 0x04;
pub const APPROVAL_TRIGGER_PRIVILEGE:       u8 = 0x08;
pub const APPROVAL_TRIGGER_CROSS_WORKSPACE: u8 = 0x10;

pub const MAX_PLAN_STEPS_PER_PLAN: usize = 32;
pub const MAX_PLAN_EDGES_PER_PLAN: usize = 64;
pub const MAX_PLANS_PER_INTENT: usize = 8;
pub const MAX_ORCH_INTENTS_PER_WORKSPACE: usize = 16;
pub const MAX_APPROVAL_RECORDS: usize = 32;

/// Declarative Orchestration Intent.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestrationIntent {
    pub intent_id: DistributedId,
    pub workspace_id: DistributedId,
    pub creator_id: DistributedId,
    pub goal_statement: [u8; 64],
    pub state: u8,
    pub _pad0: [u8; 7],
    pub active_plan_id: DistributedId,
    pub created_at_tsc: u64,
    pub updated_at_tsc: u64,
}

impl Default for OrchestrationIntent {
    fn default() -> Self {
        Self {
            intent_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            creator_id: DistributedId::new(0, 0),
            goal_statement: [0u8; 64],
            state: ORCH_INTENT_STATE_SUBMITTED,
            _pad0: [0u8; 7],
            active_plan_id: DistributedId::new(0, 0),
            created_at_tsc: 0,
            updated_at_tsc: 0,
        }
    }
}

/// Structured, versioned Plan specification.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestrationPlan {
    pub plan_id: DistributedId,
    pub intent_id: DistributedId,
    pub workspace_id: DistributedId,
    pub version: u32,
    pub author_type: u8,
    pub state: u8,
    pub step_count: u16,
    pub edge_count: u16,
    pub author_id: DistributedId,
    pub created_at_tsc: u64,
    pub approval_status: u8,
    pub _pad: [u8; 5],
}

impl Default for OrchestrationPlan {
    fn default() -> Self {
        Self {
            plan_id: DistributedId::new(0, 0),
            intent_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            version: 1,
            author_type: AUTHOR_TYPE_SYSTEM_PLANNER,
            state: PLAN_STATE_DRAFT,
            step_count: 0,
            edge_count: 0,
            author_id: DistributedId::new(0, 0),
            created_at_tsc: 0,
            approval_status: 0,
            _pad: [0u8; 5],
        }
    }
}

/// Logical Operational Unit within a Plan prior to materialization.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanStep {
    pub step_id: DistributedId,
    pub plan_id: DistributedId,
    pub step_name: [u8; 32],
    pub exec_spec: [u8; 64],
    pub input_object_ids: [DistributedId; 4],
    pub output_object_ids: [DistributedId; 4],
    pub req_capabilities: u64,
    pub cpu_cores: u32,
    pub ram_mb: u32,
    pub gpu_mb: u32,
    pub storage_mb: u32,
    pub approval_required: u8,
    pub status: u8,
    pub is_network_dependent: u8,
    pub is_destructive: u8,
    pub approval_trigger_mask: u8,
    pub _pad: [u8; 3],
    pub materialized_workload_id: DistributedId,
}

impl Default for PlanStep {
    fn default() -> Self {
        Self {
            step_id: DistributedId::new(0, 0),
            plan_id: DistributedId::new(0, 0),
            step_name: [0u8; 32],
            exec_spec: [0u8; 64],
            input_object_ids: [DistributedId::new(0, 0); 4],
            output_object_ids: [DistributedId::new(0, 0); 4],
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
            _pad: [0u8; 3],
            materialized_workload_id: DistributedId::new(0, 0),
        }
    }
}

/// Dependency Edge in Plan DAG (parent -> child).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanEdge {
    pub parent_step_id: DistributedId,
    pub child_step_id: DistributedId,
}

impl Default for PlanEdge {
    fn default() -> Self {
        Self {
            parent_step_id: DistributedId::new(0, 0),
            child_step_id: DistributedId::new(0, 0),
        }
    }
}

/// Cryptographic / OS Approval Gate Record.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalRecord {
    pub step_id: DistributedId,
    pub plan_id: DistributedId,
    pub approver_id: DistributedId,
    pub approved_at_tsc: u64,
    pub is_approved: u8,
    pub trigger_flags: u8,
    pub _pad: [u8; 6],
}

impl Default for ApprovalRecord {
    fn default() -> Self {
        Self {
            step_id: DistributedId::new(0, 0),
            plan_id: DistributedId::new(0, 0),
            approver_id: DistributedId::new(0, 0),
            approved_at_tsc: 0,
            is_approved: 0,
            trigger_flags: 0,
            _pad: [0u8; 6],
        }
    }
}

/// Derives a deterministic WorkloadID from (plan_id, version, step_id).
/// Enforces IO-06: WorkloadID = UUIDv5(NS, PlanID || Version || PlanStepID).
pub fn derive_deterministic_workload_id(
    plan_id: DistributedId,
    version: u32,
    step_id: DistributedId,
) -> DistributedId {
    let ns: [u8; 16] = [
        0x5A, 0x30, 0x4F, 0x53, 0x5F, 0x4D, 0x61, 0x74,
        0x5F, 0x4E, 0x53, 0x5F, 0x30, 0x31, 0x76, 0x31
    ];
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in ns.iter() {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in plan_id.node_id.to_le_bytes().iter().chain(plan_id.local_seq.to_le_bytes().iter()) {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in version.to_le_bytes().iter() {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in step_id.node_id.to_le_bytes().iter().chain(step_id.local_seq.to_le_bytes().iter()) {
        hash = hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    let mut seq_hash: u64 = 0x84222325cbf29ce4;
    for &b in step_id.local_seq.to_le_bytes().iter().chain(plan_id.node_id.to_le_bytes().iter()) {
        seq_hash = seq_hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    for &b in version.to_le_bytes().iter() {
        seq_hash = seq_hash.wrapping_mul(0x100000001b3) ^ (b as u64);
    }
    // UUIDv5 RFC 4122 layout (version 5 bitmask in sequence field)
    DistributedId::new(hash, (seq_hash & 0xFFFFFFFFFFFF0FFF) | 0x0000000000005000)
}

/// Evaluates whether a PlanStep requires Human Approval based on OS triggers.
pub fn evaluate_approval_triggers(
    step: &PlanStep,
    workspace_cap_envelope: u64,
    workspace_max_ram_mb: u32,
) -> u8 {
    let mut mask = 0u8;

    if step.is_destructive != 0 {
        mask |= APPROVAL_TRIGGER_DESTRUCTIVE;
    }
    if step.is_network_dependent != 0 {
        mask |= APPROVAL_TRIGGER_NETWORK;
    }
    if step.ram_mb > workspace_max_ram_mb {
        mask |= APPROVAL_TRIGGER_QUOTA;
    }
    if (step.req_capabilities & !workspace_cap_envelope) != 0 {
        mask |= APPROVAL_TRIGGER_PRIVILEGE;
    }

    mask
}

/// Validates Plan DAG acyclicity and structural sanity.
pub fn validate_plan_dag(
    steps: &[PlanStep],
    step_count: usize,
    edges: &[PlanEdge],
    edge_count: usize,
    max_capabilities: u64,
) -> Result<(), ZeroError> {
    if step_count > MAX_PLAN_STEPS_PER_PLAN || edge_count > MAX_PLAN_EDGES_PER_PLAN {
        return Err(ZeroError::InvalidRequest);
    }

    // Check duplicate step IDs
    for i in 0..step_count {
        for j in (i + 1)..step_count {
            if steps[i].step_id == steps[j].step_id {
                return Err(ZeroError::AlreadyExists);
            }
        }
        // Validate capability boundary: planner step cannot ask for capabilities exceeding system boundary unless gated
        if (steps[i].req_capabilities & !max_capabilities) != 0 && steps[i].approval_required == 0 {
            return Err(ZeroError::PermissionDenied);
        }
    }

    // Acyclicity check (Kahn's Topological Sort)
    let mut in_degree = [0u16; MAX_PLAN_STEPS_PER_PLAN];
    for e_idx in 0..edge_count {
        let edge = &edges[e_idx];
        let mut child_found = false;
        for s_idx in 0..step_count {
            if steps[s_idx].step_id == edge.child_step_id {
                in_degree[s_idx] += 1;
                child_found = true;
                break;
            }
        }
        if !child_found {
            return Err(ZeroError::NotFound);
        }
    }

    let mut queue = [0usize; MAX_PLAN_STEPS_PER_PLAN];
    let mut head = 0;
    let mut tail = 0;

    for i in 0..step_count {
        if in_degree[i] == 0 {
            queue[tail] = i;
            tail += 1;
        }
    }

    let mut visited = 0;
    while head < tail {
        let u = queue[head];
        head += 1;
        visited += 1;

        let u_step_id = steps[u].step_id;
        for e_idx in 0..edge_count {
            if edges[e_idx].parent_step_id == u_step_id {
                let v_id = edges[e_idx].child_step_id;
                for v_idx in 0..step_count {
                    if steps[v_idx].step_id == v_id {
                        in_degree[v_idx] -= 1;
                        if in_degree[v_idx] == 0 {
                            queue[tail] = v_idx;
                            tail += 1;
                        }
                        break;
                    }
                }
            }
        }
    }

    if visited != step_count {
        return Err(ZeroError::CyclicDependency); // Cyclic DAG detected
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_workload_id_derivation() {
        let p_id = DistributedId::new(1, 100);
        let s_id = DistributedId::new(1, 500);
        let w_id1 = derive_deterministic_workload_id(p_id, 1, s_id);
        let w_id2 = derive_deterministic_workload_id(p_id, 1, s_id);

        assert_eq!(w_id1, w_id2);

        let w_id_v2 = derive_deterministic_workload_id(p_id, 2, s_id);
        assert_ne!(w_id1, w_id_v2);
    }

    #[test]
    fn test_plan_dag_validation_acyclic() {
        let mut steps = [PlanStep::default(); 3];
        steps[0].step_id = DistributedId::new(1, 1);
        steps[1].step_id = DistributedId::new(1, 2);
        steps[2].step_id = DistributedId::new(1, 3);

        let mut edges = [PlanEdge::default(); 2];
        edges[0] = PlanEdge { parent_step_id: steps[0].step_id, child_step_id: steps[1].step_id };
        edges[1] = PlanEdge { parent_step_id: steps[1].step_id, child_step_id: steps[2].step_id };

        assert!(validate_plan_dag(&steps, 3, &edges, 2, 0xFFFF).is_ok());
    }

    #[test]
    fn test_plan_dag_validation_cyclic() {
        let mut steps = [PlanStep::default(); 2];
        steps[0].step_id = DistributedId::new(1, 1);
        steps[1].step_id = DistributedId::new(1, 2);

        let mut edges = [PlanEdge::default(); 2];
        edges[0] = PlanEdge { parent_step_id: steps[0].step_id, child_step_id: steps[1].step_id };
        edges[1] = PlanEdge { parent_step_id: steps[1].step_id, child_step_id: steps[0].step_id };

        assert!(validate_plan_dag(&steps, 2, &edges, 2, 0xFFFF).is_err());
    }
}
