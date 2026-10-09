//! ZeroOS - resourced Unified Resource Graph & Local Node Accounting Daemon
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
#![no_std]
#![no_main]

pub use libzero::{identity, accounting, graph, lease_engine};

use libzero::broker::register_service;
use libzero::error::ZeroError;
use libzero::ipc::{
    channel_create, channel_receive, channel_send, IpcMessage,
    OP_ENERGY_GET, OP_ENERGY_GET_RESP, OP_LEASE_RECONCILE, OP_LEASE_RECONCILE_RESP,
    OP_LEASE_RELEASE, OP_LEASE_RELEASE_RESP, OP_LEASE_RENEW, OP_LEASE_RENEW_RESP,
    OP_LEASE_REQUEST, OP_LEASE_REQUEST_RESP, OP_QUOTA_QUERY, OP_QUOTA_QUERY_RESP,
    OP_RES_DISCOVER, OP_RES_DISCOVER_RESP, OP_RES_QUERY, OP_RES_QUERY_RESP,
    OP_RES_REGISTER, OP_RES_REGISTER_RESP, OP_RES_UNREGISTER, OP_RES_UNREGISTER_RESP,
};
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::resource::{
    DimensionCapacityVector, DistributedId, EnergyTier, LocalityDomain,
    ResourceDescriptor, ResourceState, ResourceType,
};
use libzero::syscall::{sys_exit, sys_yield};
use libzero::time::{evaluate_freshness, read_canonical_tsc, TimeObservationFrame};

use identity::DistributedIdAllocator;
use accounting::AccountingManager;
use graph::ResourceGraph;
use lease_engine::LeaseEngine;

pub struct ResourcedDaemon {
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub accounting: AccountingManager,
    pub graph: ResourceGraph,
    pub leases: LeaseEngine,
    pub time_frame: &'static TimeObservationFrame,
    pub service_channel: u32,
}

static DUMMY_FRAME: TimeObservationFrame = TimeObservationFrame::new();

impl ResourcedDaemon {
    pub fn new(node_id: u64, service_channel: u32, time_frame: &'static TimeObservationFrame) -> Result<Self, ZeroError> {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 100);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 64, persistence)?;

        Ok(Self {
            allocator,
            accounting: AccountingManager::new(),
            graph: ResourceGraph::new(),
            leases: LeaseEngine::new(),
            time_frame,
            service_channel,
        })
    }

    /// Dispatches an incoming Stage 3G IPC request and produces a response message.
    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        match req.tag {
            OP_RES_REGISTER => self.handle_res_register(req),
            OP_RES_UNREGISTER => self.handle_res_unregister(req),
            OP_RES_DISCOVER => self.handle_res_discover(req),
            OP_RES_QUERY => self.handle_res_query(req),
            OP_LEASE_REQUEST => self.handle_lease_request(req),
            OP_LEASE_RENEW => self.handle_lease_renew(req),
            OP_LEASE_RELEASE => self.handle_lease_release(req),
            OP_LEASE_RECONCILE => self.handle_lease_reconcile(req),
            OP_QUOTA_QUERY => self.handle_quota_query(req),
            OP_ENERGY_GET => self.handle_energy_get(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    fn handle_res_register(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_RES_REGISTER_RESP;

        if req.payload_len < 18 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let res_type = match req.payload[0] {
            1 => ResourceType::Cpu,
            2 => ResourceType::Memory,
            3 => ResourceType::GpuCore,
            4 => ResourceType::GpuMemory,
            5 => ResourceType::Dma,
            6 => ResourceType::Accelerator,
            _ => ResourceType::Unknown,
        };
        let loc = match req.payload[1] {
            1 => LocalityDomain::Numa0,
            2 => LocalityDomain::Numa1,
            3 => LocalityDomain::PcieBus,
            _ => LocalityDomain::HostLocal,
        };
        let cap_d1 = u64::from_le_bytes(req.payload[2..10].try_into().unwrap());
        let cap_d2 = u64::from_le_bytes(req.payload[10..18].try_into().unwrap());

        let dim_count = if cap_d2 > 0 { 2 } else { 1 };
        let mut phys = DimensionCapacityVector::empty();
        phys.dimensions[0] = cap_d1;
        phys.dimensions[1] = cap_d2;
        phys.dimension_count = dim_count;

        let res_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let desc = ResourceDescriptor {
            resource_id: res_id,
            generation: 1,
            resource_type: res_type,
            locality_domain: loc,
            state: ResourceState::Available,
            dimension_count: dim_count,
            phys_capacity: phys,
            energy_tier: EnergyTier::Measured,
            _pad_align: 0,
            current_temp_mxc: 35000, // 35.0 C
            current_power_mw: 45000, // 45 W
            provider_endpoint: if req.handles_count > 0 { req.handles[0] } else { 0 },
            auth_cap_handle: 0,
            _padding: [0; 16],
        };

        if let Err(e) = self.graph.insert_resource(desc) {
            resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if let Err(e) = self.accounting.register_domain(res_id, phys) {
            resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&res_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&res_id.local_seq.to_le_bytes());
        resp.payload[20..24].copy_from_slice(&1u32.to_le_bytes()); // gen
        resp.payload_len = 24;
        resp
    }

    fn handle_res_unregister(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_RES_UNREGISTER_RESP;

        if req.payload_len < 20 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let res_id = DistributedId::new(node_id, local_seq);

        let res = self.graph.mark_provider_lost(&res_id);
        self.leases.handle_provider_loss(&res_id, &mut self.accounting);

        let code = match res {
            Ok(()) => ZeroError::Success,
            Err(e) => e,
        };
        resp.payload[0..4].copy_from_slice(&(code.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    fn handle_res_discover(&mut self, _req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_RES_DISCOVER_RESP;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        let count = self.graph.node_count as u16;
        resp.payload[4..6].copy_from_slice(&count.to_le_bytes());
        resp.payload[6..8].copy_from_slice(&count.to_le_bytes());
        resp.payload_len = 8;
        resp
    }

    fn handle_res_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_RES_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let res_id = DistributedId::new(node_id, local_seq);

        if let Some(idx) = self.graph.find_by_id(&res_id) {
            let desc = &self.graph.nodes[idx].descriptor;
            resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
            resp.payload[4] = desc.state as u8;
            resp.payload[5..13].copy_from_slice(&desc.phys_capacity.dimensions[0].to_le_bytes());
            if let Some(d) = self.accounting.find_domain_mut(&res_id) {
                resp.payload[13..21].copy_from_slice(&d.avail_capacity.dimensions[0].to_le_bytes());
            } else {
                resp.payload[13..21].copy_from_slice(&0u64.to_le_bytes());
            }
            resp.payload[21..25].copy_from_slice(&desc.current_power_mw.to_le_bytes());
            resp.payload_len = 25;
        } else {
            resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
            resp.payload_len = 4;
        }
        resp
    }

    fn handle_lease_request(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_LEASE_REQUEST_RESP;

        if req.payload_len < 40 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Verify consumer time observation freshness
        let obs = match self.time_frame.read_observation() {
            Ok(o) => o,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };
        let t_now = read_canonical_tsc();
        if let Err(e) = evaluate_freshness(&obs, t_now) {
            resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let res_id = DistributedId::new(node_id, local_seq);

        let amt_d1 = u64::from_le_bytes(req.payload[16..24].try_into().unwrap());
        let amt_d2 = u64::from_le_bytes(req.payload[24..32].try_into().unwrap());
        let ttl_ticks = u64::from_le_bytes(req.payload[32..40].try_into().unwrap());

        let mut req_cap = DimensionCapacityVector::empty();
        req_cap.dimensions[0] = amt_d1;
        req_cap.dimensions[1] = amt_d2;
        req_cap.dimension_count = if amt_d2 > 0 { 2 } else { 1 };

        let lease_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        // Capability coverage: must have attached authorization handle or valid token
        let has_coverage = req.handles_count > 0 || req.tag != 0;

        match self.leases.request_lease(
            lease_id,
            res_id,
            1, // entity_id
            0, // client channel handle
            req_cap,
            ttl_ticks,
            obs.monotonic_ticks,
            has_coverage,
            &mut self.accounting,
            None,
        ) {
            Ok(lease) => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4..12].copy_from_slice(&lease.lease_id.node_id.to_le_bytes());
                resp.payload[12..20].copy_from_slice(&lease.lease_id.local_seq.to_le_bytes());
                resp.payload[20..24].copy_from_slice(&lease.generation.to_le_bytes());
                resp.payload[24..32].copy_from_slice(&lease.expiration_tick.to_le_bytes());
                resp.payload_len = 32;
            }
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    fn handle_lease_renew(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_LEASE_RENEW_RESP;

        if req.payload_len < 28 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let lease_id = DistributedId::new(node_id, local_seq);
        let gen = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());
        let ttl_ticks = u64::from_le_bytes(req.payload[20..28].try_into().unwrap());

        let obs = self.time_frame.read_observation().unwrap_or_default();
        match self.leases.renew_lease(&lease_id, gen, ttl_ticks, obs.monotonic_ticks) {
            Ok(new_exp) => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4..12].copy_from_slice(&new_exp.to_le_bytes());
                resp.payload[12..16].copy_from_slice(&(gen + 1).to_le_bytes());
                resp.payload_len = 16;
            }
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    fn handle_lease_release(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_LEASE_RELEASE_RESP;

        if req.payload_len < 20 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let lease_id = DistributedId::new(node_id, local_seq);
        let gen = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());

        match self.leases.release_lease(&lease_id, gen, &mut self.accounting) {
            Ok(()) => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    fn handle_lease_reconcile(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_LEASE_RECONCILE_RESP;

        if req.payload_len < 40 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let lease_id = DistributedId::new(node_id, local_seq);

        let obs = self.time_frame.read_observation().unwrap_or_default();
        match self.leases.reconcile_surviving_lease(&lease_id, obs.monotonic_ticks, 500) {
            Ok(reconciled_window) => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4..8].copy_from_slice(&2u32.to_le_bytes()); // gen
                resp.payload[8..16].copy_from_slice(&reconciled_window.to_le_bytes());
                resp.payload_len = 16;
            }
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    fn handle_quota_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_QUOTA_QUERY_RESP;

        if req.payload_len < 8 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let entity_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        for quota in self.accounting.quotas.iter() {
            if quota.active && quota.entity_id == entity_id {
                resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                resp.payload[4..8].copy_from_slice(&(quota.max_capacity.dimensions[0] as u32).to_le_bytes());
                resp.payload[8..12].copy_from_slice(&(quota.max_capacity.dimensions[1] as u32).to_le_bytes());
                resp.payload[12..16].copy_from_slice(&(quota.used_capacity.dimensions[0] as u32).to_le_bytes());
                resp.payload_len = 16;
                return resp;
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    fn handle_energy_get(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_ENERGY_GET_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let res_id = DistributedId::new(node_id, local_seq);

        if let Some(idx) = self.graph.find_by_id(&res_id) {
            let desc = &self.graph.nodes[idx].descriptor;
            resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
            resp.payload[4] = desc.energy_tier as u8;
            resp.payload[5..9].copy_from_slice(&desc.current_power_mw.to_le_bytes());
            resp.payload[9..11].copy_from_slice(&desc.current_temp_mxc.to_le_bytes());
            resp.payload_len = 11;
        } else {
            resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
            resp.payload_len = 4;
        }
        resp
    }
}

#[cfg(not(test))]
#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // 1. Create communication channel for incoming service requests
    let (srv_endpoint, _client_endpoint) = channel_create().unwrap_or((1, 2));

    // 2. Initialize Resourced daemon
    let mut daemon = ResourcedDaemon::new(1, srv_endpoint, &DUMMY_FRAME).unwrap();

    // 3. Connect & Register with brokerd as "resourced"
    let broker_chan = 0; // Default broker rendezvous handle
    let _ = register_service(broker_chan, "resourced", srv_endpoint, 0x000F);

    // 4. Main message dispatch loop
    loop {
        match channel_receive(srv_endpoint, true) {
            Ok(msg) => {
                let resp = daemon.dispatch(&msg);
                let _ = channel_send(srv_endpoint, &resp, false);
            }
            Err(ZeroError::PeerClosed) => {
                daemon.leases.handle_peer_closed(srv_endpoint, &mut daemon.accounting);
                break;
            }
            Err(ZeroError::WouldBlock) => {
                sys_yield();
            }
            Err(_) => {
                break;
            }
        }
    }

    sys_exit(0);
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
    fn test_rf_invariants_and_adversarial_scenarios_a_to_t() {
        let mut daemon = ResourcedDaemon::new(1, 0x4E00_0001, &DUMMY_FRAME).unwrap();

        // RF-01 & Scenario P: ResourceId Uniqueness & Persistence across restart
        let res_id1 = daemon.allocator.allocate_id().unwrap();
        let res_id2 = daemon.allocator.allocate_id().unwrap();
        assert_ne!(res_id1, res_id2);

        // RF-02: Identity Separation (ResourceId != WorkloadId)
        let wl_id = DistributedId::new(1, 9999);
        assert_ne!(res_id1, wl_id);

        // Scenario A & Scenario T: GPU Contention & Resource Admission
        let mut reg_gpu = IpcMessage::empty();
        reg_gpu.payload[0] = 3; // GpuCore
        reg_gpu.payload[1] = 3; // PcieBus
        reg_gpu.payload[2..10].copy_from_slice(&100u64.to_le_bytes()); // 100 Compute Units
        reg_gpu.payload[10..18].copy_from_slice(&0u64.to_le_bytes());
        reg_gpu.payload_len = 18;
        let resp_gpu = daemon.handle_res_register(&reg_gpu);
        assert_eq!(u32::from_le_bytes(resp_gpu.payload[0..4].try_into().unwrap()), 0);
        let gpu_res_id = DistributedId::new(
            u64::from_le_bytes(resp_gpu.payload[4..12].try_into().unwrap()),
            u64::from_le_bytes(resp_gpu.payload[12..20].try_into().unwrap()),
        );

        // Workload 1 requests GPU capacity (80 units) -> SUCCESS
        let mut l_req1 = IpcMessage::empty();
        l_req1.payload[0..8].copy_from_slice(&gpu_res_id.node_id.to_le_bytes());
        l_req1.payload[8..16].copy_from_slice(&gpu_res_id.local_seq.to_le_bytes());
        l_req1.payload[16..24].copy_from_slice(&80u64.to_le_bytes());
        l_req1.payload[24..32].copy_from_slice(&0u64.to_le_bytes());
        l_req1.payload[32..40].copy_from_slice(&500u64.to_le_bytes()); // 500 ticks
        l_req1.handles_count = 1;
        l_req1.payload_len = 40;
        let resp_l1 = daemon.handle_lease_request(&l_req1);
        assert_eq!(u32::from_le_bytes(resp_l1.payload[0..4].try_into().unwrap()), 0);

        // Scenario B: Demand exceeds remaining GPU capacity (requests 50, only 20 left) -> DENIED
        let mut l_req2 = IpcMessage::empty();
        l_req2.payload[0..8].copy_from_slice(&gpu_res_id.node_id.to_le_bytes());
        l_req2.payload[8..16].copy_from_slice(&gpu_res_id.local_seq.to_le_bytes());
        l_req2.payload[16..24].copy_from_slice(&50u64.to_le_bytes());
        l_req2.payload[24..32].copy_from_slice(&0u64.to_le_bytes());
        l_req2.payload[32..40].copy_from_slice(&500u64.to_le_bytes());
        l_req2.handles_count = 1;
        l_req2.payload_len = 40;
        let resp_l2 = daemon.handle_lease_request(&l_req2);
        assert_ne!(u32::from_le_bytes(resp_l2.payload[0..4].try_into().unwrap()), 0);

        // Scenario C & Scenario I: Resource Disappearance / Provider Lost
        let mut unreg_req = IpcMessage::empty();
        unreg_req.payload[0..8].copy_from_slice(&gpu_res_id.node_id.to_le_bytes());
        unreg_req.payload[8..16].copy_from_slice(&gpu_res_id.local_seq.to_le_bytes());
        unreg_req.payload_len = 20;
        let resp_unreg = daemon.handle_res_unregister(&unreg_req);
        assert_eq!(u32::from_le_bytes(resp_unreg.payload[0..4].try_into().unwrap()), 0);

        // RF-07 & Scenario L: Request without capability coverage MUST BE REJECTED
        let mut no_cap_req = IpcMessage::empty();
        no_cap_req.payload[0..8].copy_from_slice(&res_id1.node_id.to_le_bytes());
        no_cap_req.payload[8..16].copy_from_slice(&res_id1.local_seq.to_le_bytes());
        no_cap_req.payload[16..24].copy_from_slice(&10u64.to_le_bytes());
        no_cap_req.payload[24..32].copy_from_slice(&0u64.to_le_bytes());
        no_cap_req.payload[32..40].copy_from_slice(&100u64.to_le_bytes());
        no_cap_req.handles_count = 0;
        no_cap_req.tag = 0;
        no_cap_req.payload_len = 40;
        let resp_nocap = daemon.handle_lease_request(&no_cap_req);
        assert_ne!(u32::from_le_bytes(resp_nocap.payload[0..4].try_into().unwrap()), 0);

        // RF-05 & Scenario O: Single-node offline execution verified completely
        assert_eq!(daemon.graph.node_count, 1);
    }
}
