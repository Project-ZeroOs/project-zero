//! ZeroOS - libzero Personal Compute Fabric & Intent Resolution Subsystem Data Structures
//!
//! Authoritative Contract: Stage 4F Architecture Specification Rev3 & ADR-0030.
//! Implementation Plan: Stage 4F Implementation Plan Rev2.

use crate::resource::DistributedId;

// IPC Protocol OpCodes for intentd (registered with brokerd as "intentd.srv")
pub const OP_INTENT_SUBMIT:              u64 = 0x0601;
pub const OP_INTENT_SUBMIT_RESP:         u64 = 0x0681;
pub const OP_INTENT_RESOLVE:             u64 = 0x0602;
pub const OP_INTENT_RESOLVE_RESP:        u64 = 0x0682;
pub const OP_INTENT_QUERY_STATE:         u64 = 0x0603;
pub const OP_INTENT_QUERY_STATE_RESP:    u64 = 0x0683;
pub const OP_INTENT_CANCEL:              u64 = 0x0604;
pub const OP_INTENT_CANCEL_RESP:         u64 = 0x0684;

// IPC Protocol OpCodes for fabricd (registered with brokerd as "fabricd.srv")
pub const OP_FABRIC_NODE_REGISTER:       u64 = 0x0610;
pub const OP_FABRIC_NODE_REGISTER_RESP:  u64 = 0x0690;
pub const OP_FABRIC_NODE_HEARTBEAT:      u64 = 0x0611;
pub const OP_FABRIC_NODE_HEARTBEAT_RESP: u64 = 0x0691;
pub const OP_FABRIC_CSDT_DELEGATE:       u64 = 0x0612;
pub const OP_FABRIC_CSDT_DELEGATE_RESP:  u64 = 0x0692;
pub const OP_FABRIC_CSDT_REVOKE:         u64 = 0x0613;
pub const OP_FABRIC_CSDT_REVOKE_RESP:    u64 = 0x0693;
pub const OP_FABRIC_QUERY_TOPOLOGY:      u64 = 0x0614;
pub const OP_FABRIC_QUERY_TOPOLOGY_RESP: u64 = 0x0694;

// Static Configuration & Hard Bounds (Derived in Section 12 of Plan)
pub const MAX_FABRIC_NODES: usize = 16;
pub const MAX_REMOTE_RESOURCES: usize = 256;
pub const MAX_ACTIVE_CSDT: usize = 128;
pub const MAX_REMOTE_LEASES: usize = 64;
pub const MAX_NODE_SESSIONS: usize = 16;
pub const MAX_PENDING_INTENTS: usize = 32;
pub const MAX_PLAN_NODES: usize = 16;
pub const MAX_DISCOVERY_ENTRIES: usize = 32;

pub const INTENT_CLARIFICATION_TTL_TICKS: u64 = 1000;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IntentState {
    Unallocated = 0,
    Unresolved = 1,
    Clarifying = 2,
    Resolved = 3,
    Executing = 4,
    Executed = 5,
    Rejected = 6,
    Expired = 7,
    Cancelled = 8,
}

impl Default for IntentState {
    fn default() -> Self {
        Self::Unallocated
    }
}

/// 1,024-byte Intent Descriptor
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct IntentDescriptor {
    pub intent_id: DistributedId,        // 16 B
    pub principal_id: DistributedId,     // 16 B
    pub workspace_id: DistributedId,     // 16 B
    pub state: IntentState,              // 1 B
    pub ambiguity_flag: u8,              // 1 B
    pub min_ial_requirement: u8,        // 1 B (IAL-1..3)
    pub privacy_class: u8,               // 1 B (0=LocalOnly, 1=FabricPrivate, 2=Public)
    pub _pad0: [u8; 4],                  // 4 B
    pub submission_tsc: u64,             // 8 B
    pub deadline_tsc: u64,               // 8 B
    pub energy_limit_mwh: u32,           // 4 B
    pub raw_intent_len: u32,             // 4 B
    pub raw_intent_payload: [u8; 512],   // 512 B UTF-8 text buffer
    pub _padding: [u8; 432],             // 432 B padding to 1,024 B
}

impl Default for IntentDescriptor {
    fn default() -> Self {
        Self {
            intent_id: DistributedId::default(),
            principal_id: DistributedId::default(),
            workspace_id: DistributedId::default(),
            state: IntentState::Unallocated,
            ambiguity_flag: 0,
            min_ial_requirement: 1,
            privacy_class: 0,
            _pad0: [0; 4],
            submission_tsc: 0,
            deadline_tsc: 0,
            energy_limit_mwh: 0,
            raw_intent_len: 0,
            raw_intent_payload: [0; 512],
            _padding: [0; 432],
        }
    }
}

const _: () = assert!(core::mem::size_of::<IntentDescriptor>() == 1024);

/// 128-byte Capability-Scoped Delegation Token Container
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CsdtToken {
    pub csdt_id: DistributedId,          // 16 B
    pub issuer_node_id: u64,             // 8 B
    pub target_node_id: u64,             // 8 B
    pub workspace_id: DistributedId,     // 16 B
    pub capability_rights_mask: u64,     // 8 B
    pub valid_from_monotonic_tick: u64,  // 8 B
    pub expire_monotonic_tick: u64,      // 8 B
    pub signature: [u8; 56],             // 56 B signature buffer (72 + 56 = 128 B)
}

impl Default for CsdtToken {
    fn default() -> Self {
        Self {
            csdt_id: DistributedId::default(),
            issuer_node_id: 0,
            target_node_id: 0,
            workspace_id: DistributedId::default(),
            capability_rights_mask: 0,
            valid_from_monotonic_tick: 0,
            expire_monotonic_tick: 0,
            signature: [0; 56],
        }
    }
}

const _: () = assert!(core::mem::size_of::<CsdtToken>() == 128);

/// 256-byte Compute Fabric Node Descriptor
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FabricNodeDescriptor {
    pub node_id: u64,                    // 8 B
    pub ial_level: u8,                   // 1 B (1=Software, 2=Hardware, 3=Attested)
    pub state: u8,                       // 1 B (0=Offline, 1=Online, 2=Degraded)
    pub cpu_cores: u16,                  // 2 B
    pub _pad0: [u8; 4],                  // 4 B
    pub total_ram_mb: u64,               // 8 B
    pub available_ram_mb: u64,           // 8 B
    pub gpu_compute_mflops: u64,         // 8 B
    pub npu_tops: u32,                   // 4 B
    pub rtt_latency_us: u32,             // 4 B
    pub battery_level_pct: u8,           // 1 B
    pub power_source: u8,                // 1 B (0=Battery, 1=AC)
    pub _pad1: [u8; 6],                  // 6 B
    pub last_heartbeat_tsc: u64,         // 8 B
    pub node_pubkey: [u8; 32],           // 32 B Ed25519 public key
    pub _padding: [u8; 160],             // 160 B padding to 256 B
}

impl Default for FabricNodeDescriptor {
    fn default() -> Self {
        Self {
            node_id: 0,
            ial_level: 1,
            state: 0,
            cpu_cores: 0,
            _pad0: [0; 4],
            total_ram_mb: 0,
            available_ram_mb: 0,
            gpu_compute_mflops: 0,
            npu_tops: 0,
            rtt_latency_us: 0,
            battery_level_pct: 100,
            power_source: 1,
            _pad1: [0; 6],
            last_heartbeat_tsc: 0,
            node_pubkey: [0; 32],
            _padding: [0; 160],
        }
    }
}

const _: () = assert!(core::mem::size_of::<FabricNodeDescriptor>() == 256);

/// 256-byte Execution Plan Node
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlanNode {
    pub node_id: u32,
    pub task_type: u32,
    pub required_ram_mb: u64,
    pub requires_gpu: u8,
    pub requires_npu: u8,
    pub _pad0: [u8; 6],
    pub label: [u8; 32],
    pub _padding: [u8; 200],
}

impl Default for PlanNode {
    fn default() -> Self {
        Self {
            node_id: 0,
            task_type: 0,
            required_ram_mb: 0,
            requires_gpu: 0,
            requires_npu: 0,
            _pad0: [0; 6],
            label: [0; 32],
            _padding: [0; 200],
        }
    }
}

const _: () = assert!(core::mem::size_of::<PlanNode>() == 256);

/// Execution Plan Container
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ExecutionPlan {
    pub plan_id: DistributedId,
    pub intent_id: DistributedId,
    pub agent_id: DistributedId,
    pub workspace_id: DistributedId,
    pub plan_node_count: u32,
    pub estimated_total_mflops: u64,
    pub required_capabilities_mask: u64,
    pub human_auth_required_flag: u8,
    pub _pad0: [u8; 7],
    pub plan_nodes: [PlanNode; MAX_PLAN_NODES],
}

impl Default for ExecutionPlan {
    fn default() -> Self {
        Self {
            plan_id: DistributedId::default(),
            intent_id: DistributedId::default(),
            agent_id: DistributedId::default(),
            workspace_id: DistributedId::default(),
            plan_node_count: 0,
            estimated_total_mflops: 0,
            required_capabilities_mask: 0,
            human_auth_required_flag: 0,
            _pad0: [0; 7],
            plan_nodes: [PlanNode::default(); MAX_PLAN_NODES],
        }
    }
}

/// Task Demand Spec for 2-Stage Placement Planner
#[derive(Copy, Clone, Debug)]
pub struct TaskDemandSpec {
    pub min_ial: u8,
    pub required_ram_mb: u64,
    pub requires_gpu: bool,
    pub requires_npu: bool,
    pub max_latency_us: u32,
}

/// Dynamic Placement Policy Weights
#[derive(Copy, Clone, Debug)]
pub struct FabricPlannerPolicy {
    pub w_lat: f32, // Default: 0.5
    pub w_eng: f32, // Default: 0.3
    pub w_cost: f32, // Default: 0.2
}

impl Default for FabricPlannerPolicy {
    fn default() -> Self {
        Self {
            w_lat: 0.5,
            w_eng: 0.3,
            w_cost: 0.2,
        }
    }
}
