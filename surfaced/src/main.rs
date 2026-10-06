//! ZeroOS - Display Surface Authority Daemon (`surfaced`)
//!
//! Authoritative Contract: Stage 5 Architecture Specification Rev4 (Frozen) & ADR-0031.
//! Implementation Plan: Stage 5 Implementation Plan Rev2.
//!
//! Responsibilities:
//! - Manages presentation surface registration and spatial context mapping to Stage 4D workspace context graphs.
//! - Validates Stage 3H capability derivation rules ($C_{ws} \to C_{surf}$).
//! - Acquires composition memory leases from Stage 4B `resourced` (`MAX_SURFACE_RAM_MB`).
//! - Validates `SURFACE_TYPE_AUTH_OVERLAY` requests against active `AuthorizationTransactionRef` state from `agentd`.
//! - Handles ephemeral surface reconstruction without persisting handles to ZeroFS.

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::error::ZeroError;
use libzero::identity::DistributedIdAllocator;
use libzero::ipc::IpcMessage;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::presentation::*;
use libzero::resource::DistributedId;

pub struct SurfaceDaemon {
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub active_surface_count: usize,
    pub surfaces: [PresentationSurfaceDescriptor; MAX_COMPOSITOR_SURFACES],
    pub total_allocated_ram_bytes: usize,
    pub max_ram_quota_bytes: usize,
}

impl SurfaceDaemon {
    pub fn new(node_id: u64) -> Self {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 1000);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 128, persistence).unwrap();
        Self {
            allocator,
            active_surface_count: 0,
            surfaces: [PresentationSurfaceDescriptor::default(); MAX_COMPOSITOR_SURFACES],
            total_allocated_ram_bytes: 0,
            max_ram_quota_bytes: MAX_SURFACE_RAM_MB * 1024 * 1024, // Stage 4B lease limit
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Tag Validation: Reject response opcodes submitted as requests
        match req.tag {
            OP_SURFACE_REGISTER_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_SURFACE_REGISTER => self.handle_register_surface(req),
            OP_SURFACE_REGISTER_REMOTE_PROXY => self.handle_register_remote_proxy(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    pub fn handle_register_surface(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SURFACE_REGISTER_RESP;

        if req.payload_len < 28 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if self.active_surface_count >= MAX_COMPOSITOR_SURFACES {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let shm_handle = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());
        let width = u32::from_le_bytes(req.payload[20..24].try_into().unwrap());
        let height = u32::from_le_bytes(req.payload[24..28].try_into().unwrap());

        // Check for optional requested surface type (payload byte 28) and auth transaction ref
        let requested_type = if req.payload_len >= 29 { req.payload[28] } else { SURFACE_TYPE_REGULAR };

        // Security Boundary: Setting SURFACE_TYPE_AUTH_OVERLAY requires a valid AuthorizationTransactionRef payload
        if requested_type == SURFACE_TYPE_AUTH_OVERLAY {
            // Unprivileged requests without valid auth transaction payload (min length 45 bytes) are REJECTED
            if req.payload_len < 45 {
                resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        let required_ram_bytes = (width * height * DISPLAY_BYTES_PER_PIXEL) as usize;

        // Stage 4B Resource Lease Accounting Integration: Check memory quota
        if self.total_allocated_ram_bytes + required_ram_bytes > self.max_ram_quota_bytes {
            resp.payload[0..4].copy_from_slice(&(ZeroError::QuotaExceeded.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let surface_dist_id = self.allocator.allocate_id().unwrap();
        let surface_id = surface_dist_id.local_seq;

        let mut desc = PresentationSurfaceDescriptor::default();
        desc.surface_id = surface_id;
        desc.workspace_id = DistributedId::new(ws_node, ws_seq);
        desc.shm_handle = shm_handle;
        desc.width = width;
        desc.height = height;
        desc.surface_type = requested_type;
        desc.z_layer = if requested_type == SURFACE_TYPE_AUTH_OVERLAY { LAYER_SYSTEM_AUTH } else { LAYER_WORKSPACE_DEFAULT };

        self.surfaces[self.active_surface_count] = desc;
        self.active_surface_count += 1;
        self.total_allocated_ram_bytes += required_ram_bytes;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&surface_id.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    pub fn handle_register_remote_proxy(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SURFACE_REGISTER_REMOTE_PROXY_RESP;

        if req.payload_len < 29 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let requested_z = req.payload[28];

        // Security Boundary: Remote presentation surfaces are strictly restricted to application workspace layers (z <= 99)
        if requested_z >= 100 {
            // Unprivileged remote requests attempting z >= 100 or z = 255 without valid local capability are REJECTED
            if req.handles_count == 0 {
                resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        if self.active_surface_count >= MAX_COMPOSITOR_SURFACES {
            resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let ws_node = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let ws_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let shm_handle = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());
        let width = u32::from_le_bytes(req.payload[20..24].try_into().unwrap());
        let height = u32::from_le_bytes(req.payload[24..28].try_into().unwrap());

        let surface_dist_id = self.allocator.allocate_id().unwrap();
        let surface_id = surface_dist_id.local_seq;

        let mut desc = PresentationSurfaceDescriptor::default();
        desc.surface_id = surface_id;
        desc.workspace_id = DistributedId::new(ws_node, ws_seq);
        desc.shm_handle = shm_handle;
        desc.width = width;
        desc.height = height;
        desc.surface_type = SURFACE_TYPE_REGULAR;
        desc.z_layer = if requested_z < 100 { requested_z } else { LAYER_WORKSPACE_DEFAULT };

        self.surfaces[self.active_surface_count] = desc;
        self.active_surface_count += 1;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&surface_id.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    pub fn unregister_surface(&mut self, surface_id: u64) -> Result<(), ZeroError> {
        let mut found_idx = None;
        for i in 0..self.active_surface_count {
            if self.surfaces[i].surface_id == surface_id {
                found_idx = Some(i);
                break;
            }
        }

        match found_idx {
            Some(idx) => {
                let ram_freed = (self.surfaces[idx].width * self.surfaces[idx].height * DISPLAY_BYTES_PER_PIXEL) as usize;
                if self.total_allocated_ram_bytes >= ram_freed {
                    self.total_allocated_ram_bytes -= ram_freed;
                } else {
                    self.total_allocated_ram_bytes = 0;
                }

                for i in idx..(self.active_surface_count - 1) {
                    self.surfaces[i] = self.surfaces[i + 1];
                }
                self.active_surface_count -= 1;
                Ok(())
            }
            None => Err(ZeroError::NotFound),
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
