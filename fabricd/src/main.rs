//! ZeroOS - fabricd (Personal Compute Fabric Mesh & Placement Daemon)
//!
//! Authoritative Contract: Stage 4F Architecture Specification Rev3 & ADR-0030.
//! Implementation Plan: Stage 4F Implementation Plan Rev2.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::fabric::*;
use libzero::identity::DistributedIdAllocator;
use libzero::ipc::IpcMessage;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::resource::DistributedId;
use libzero::syscall::sys_exit;
use libzero::ZeroError;

pub static mut FABRIC_NODE_TABLE: [FabricNodeDescriptor; MAX_FABRIC_NODES] =
    [const { FabricNodeDescriptor {
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
    } }; MAX_FABRIC_NODES];

pub static mut CSDT_TABLE: [CsdtToken; MAX_ACTIVE_CSDT] =
    [const { CsdtToken {
        csdt_id: DistributedId { node_id: 0, local_seq: 0 },
        issuer_node_id: 0,
        target_node_id: 0,
        workspace_id: DistributedId { node_id: 0, local_seq: 0 },
        capability_rights_mask: 0,
        valid_from_monotonic_tick: 0,
        expire_monotonic_tick: 0,
        signature: [0; 56],
    } }; MAX_ACTIVE_CSDT];

pub struct FabricDaemon {
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub active_node_count: usize,
    pub active_csdt_count: usize,
    pub policy: FabricPlannerPolicy,
}

impl FabricDaemon {
    pub fn new(node_id: u64) -> Self {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 700);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 128, persistence).unwrap();
        unsafe {
            FABRIC_NODE_TABLE = [const { FabricNodeDescriptor {
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
            } }; MAX_FABRIC_NODES];

            CSDT_TABLE = [const { CsdtToken {
                csdt_id: DistributedId { node_id: 0, local_seq: 0 },
                issuer_node_id: 0,
                target_node_id: 0,
                workspace_id: DistributedId { node_id: 0, local_seq: 0 },
                capability_rights_mask: 0,
                valid_from_monotonic_tick: 0,
                expire_monotonic_tick: 0,
                signature: [0; 56],
            } }; MAX_ACTIVE_CSDT];
        }
        Self {
            allocator,
            active_node_count: 0,
            active_csdt_count: 0,
            policy: FabricPlannerPolicy::default(),
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Enforce protocol direction: Reject response opcodes submitted as incoming requests
        match req.tag {
            OP_FABRIC_NODE_REGISTER_RESP
            | OP_FABRIC_NODE_HEARTBEAT_RESP
            | OP_FABRIC_CSDT_DELEGATE_RESP
            | OP_FABRIC_CSDT_REVOKE_RESP
            | OP_FABRIC_QUERY_TOPOLOGY_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_FABRIC_NODE_REGISTER => self.handle_node_register(req),
            OP_FABRIC_NODE_HEARTBEAT => self.handle_node_heartbeat(req),
            OP_FABRIC_CSDT_DELEGATE => self.handle_csdt_delegate(req),
            OP_FABRIC_CSDT_REVOKE => self.handle_csdt_revoke(req),
            OP_FABRIC_QUERY_TOPOLOGY => self.handle_query_topology(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    fn handle_node_register(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_FABRIC_NODE_REGISTER_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if self.active_node_count >= MAX_FABRIC_NODES {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ial = req.payload[8];
        let ram_mb = u64::from_le_bytes(req.payload[9..17].try_into().unwrap());
        let npu_tops = u32::from_le_bytes(req.payload[17..21].try_into().unwrap());
        let gpu_mflops = u64::from_le_bytes(req.payload[21..29].try_into().unwrap());

        let node_desc = unsafe { &mut FABRIC_NODE_TABLE[self.active_node_count] };
        node_desc.node_id = node_id;
        node_desc.ial_level = ial;
        node_desc.state = 1; // Online
        node_desc.available_ram_mb = ram_mb;
        node_desc.total_ram_mb = ram_mb;
        node_desc.npu_tops = npu_tops;
        node_desc.gpu_compute_mflops = gpu_mflops;
        node_desc.rtt_latency_us = 1000;

        self.active_node_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&node_id.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    fn handle_node_heartbeat(&mut self, _req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_FABRIC_NODE_HEARTBEAT_RESP;
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    fn handle_csdt_delegate(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_FABRIC_CSDT_DELEGATE_RESP;

        if req.payload_len < 32 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if self.active_csdt_count >= MAX_ACTIVE_CSDT {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let csdt_id = self.allocator.allocate_id().unwrap();
        let token = unsafe { &mut CSDT_TABLE[self.active_csdt_count] };
        token.csdt_id = csdt_id;
        token.issuer_node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        token.target_node_id = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        token.capability_rights_mask = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        token.valid_from_monotonic_tick = 100;
        token.expire_monotonic_tick = 100 + INTENT_CLARIFICATION_TTL_TICKS;

        self.active_csdt_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&csdt_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&csdt_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    fn handle_csdt_revoke(&mut self, _req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_FABRIC_CSDT_REVOKE_RESP;
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    fn handle_query_topology(&mut self, _req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_FABRIC_QUERY_TOPOLOGY_RESP;
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..8].copy_from_slice(&(self.active_node_count as u32).to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    pub fn evaluate_placement(&self, task_req: &TaskDemandSpec) -> Result<u64, ZeroError> {
        let mut best_node_id: u64 = 0;
        let mut best_cost: f32 = f32::MAX;

        unsafe {
            for i in 0..self.active_node_count {
                let node = &FABRIC_NODE_TABLE[i];
                if node.state != 1 { continue; } // Must be Online

                // PHASE 1: HARD BOOLEAN CONSTRAINTS FILTER (EXPLICIT PRECEDENCE)
                if task_req.min_ial > node.ial_level { continue; }
                if task_req.required_ram_mb > node.available_ram_mb { continue; }
                if task_req.requires_npu && node.npu_tops == 0 { continue; }
                if task_req.requires_gpu && node.gpu_compute_mflops == 0 { continue; }

                // PHASE 2: DIMENSIONLESS SOFT OPTIMIZATION (ONLY FOR SURVIVING ELIGIBLE NODES)
                let n_lat = (node.rtt_latency_us as f32) / 100_000.0;
                let n_eng = if node.power_source == 0 { (100 - node.battery_level_pct) as f32 / 100.0 } else { 0.0 };
                let n_cost = 0.1;

                let cost = self.policy.w_lat * n_lat + self.policy.w_eng * n_eng + self.policy.w_cost * n_cost;
                if cost < best_cost {
                    best_cost = cost;
                    best_node_id = node.node_id;
                }
            }
        }

        if best_node_id != 0 {
            Ok(best_node_id)
        } else {
            Err(ZeroError::QuotaExceeded)
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let daemon = FabricDaemon::new(1);
    let _ = daemon.active_node_count;
    unsafe { sys_exit(0); }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { sys_exit(1); }
}
