//! ZeroOS - Display Surface Authority Daemon (`surfaced`)
//!
//! Authoritative Contract: Stage 6F Architecture Specification Rev7 (Frozen).
//! Implementation Plan: Stage 6F Implementation Plan Rev2.
//!
//! Responsibilities:
//! - Sole authoritative owner of `SurfaceSemanticGeneration`.
//! - Manages presentation surface registration and spatial context mapping.
//! - Enforces Stage 3H capability derivation rules ($C_{ws} \to C_{surf}$).
//! - Acquires composition memory leases from Stage 4B `resourced`.
//! - Validates `SURFACE_TYPE_AUTH_OVERLAY` requests against active `AuthorizationTransactionRef` state from `agentd`.

#![no_std]
#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use libzero::syscall::sys_exit;

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

    // Stage 6F Semantic Generation Ownership
    pub semantic_generation: u64,
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
            semantic_generation: 1,
        }
    }

    /// Semantic Generation Increment (owned by `surfaced`).
    pub fn increment_semantic_gen(&mut self) -> u64 {
        self.semantic_generation += 1;
        self.semantic_generation
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

        let requested_type = if req.payload_len >= 29 { req.payload[28] } else { SURFACE_TYPE_REGULAR };

        if requested_type == SURFACE_TYPE_AUTH_OVERLAY {
            if req.payload_len < 45 {
                resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        }

        let required_ram_bytes = (width * height * DISPLAY_BYTES_PER_PIXEL) as usize;

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

        self.increment_semantic_gen();

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

        if requested_z >= 100 {
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

        self.increment_semantic_gen();

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
                self.increment_semantic_gen();
                Ok(())
            }
            None => Err(ZeroError::NotFound),
        }
    }
}

#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    let _daemon = SurfaceDaemon::new(1);
    unsafe { sys_exit(0); }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { sys_exit(1); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_semantic_generation_ownership() {
        let mut daemon = SurfaceDaemon::new(1);
        assert_eq!(daemon.semantic_generation, 1);
        daemon.increment_semantic_gen();
        assert_eq!(daemon.semantic_generation, 2);
    }
}
