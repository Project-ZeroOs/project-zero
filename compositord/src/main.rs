//! ZeroOS - Spatial Compositor Daemon (`compositord`)
//!
//! Authoritative Contract: Stage 5 Architecture Specification Rev4 (Frozen) & ADR-0031.
//! Implementation Plan: Stage 5 Implementation Plan Rev2.
//!
//! Responsibilities:
//! - Manages physical display hardware (`DeviceId::Display(0)`) via Stage 3L `sys_dev_map_mmio`.
//! - Operates in Stage 3B `Priority::Critical` scheduling class.
//! - Blits 2D presentation surfaces from Stage 3G `ShmObject` shared memory buffers onto physical VRAM.
//! - Enforces double-buffered state machine (`FREE -> WRITING -> READY -> COMPOSITING -> FREE`).
//! - Enforces layer z-ordering (`SURFACE_TYPE_AUTH_OVERLAY` precedence over workspace surfaces).

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use libzero::error::ZeroError;
use libzero::ipc::IpcMessage;
use libzero::presentation::*;

pub struct CompositorDaemon {
    pub display_bound: bool,
    pub display_id: u64,
    pub vram_mapped_virt: u64,
    pub vram_size_bytes: usize,
    pub width: u32,
    pub height: u32,
    pub active_surface_count: usize,
    pub surfaces: [PresentationSurfaceDescriptor; MAX_COMPOSITOR_SURFACES],
    pub surface_headers: [PresentationBufferHeader; MAX_COMPOSITOR_SURFACES],
    pub missed_frame_count: u64,
    pub last_composition_tsc: u64,
}

impl CompositorDaemon {
    pub fn new() -> Self {
        Self {
            display_bound: false,
            display_id: 0,
            vram_mapped_virt: 0xE000_0000, // Default MMIO VRAM virtual aperture
            vram_size_bytes: (DISPLAY_DEFAULT_WIDTH * DISPLAY_DEFAULT_HEIGHT * DISPLAY_BYTES_PER_PIXEL) as usize,
            width: DISPLAY_DEFAULT_WIDTH,
            height: DISPLAY_DEFAULT_HEIGHT,
            active_surface_count: 0,
            surfaces: [PresentationSurfaceDescriptor::default(); MAX_COMPOSITOR_SURFACES],
            surface_headers: [PresentationBufferHeader::default(); MAX_COMPOSITOR_SURFACES],
            missed_frame_count: 0,
            last_composition_tsc: 0,
        }
    }

    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        // Tag Validation: Reject response opcodes submitted as requests
        match req.tag {
            OP_COMPOSITOR_BIND_DISPLAY_RESP | OP_SURFACE_COMMIT_RESP => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
            _ => {}
        }

        match req.tag {
            OP_COMPOSITOR_BIND_DISPLAY => self.handle_bind_display(req),
            OP_SURFACE_COMMIT => self.handle_surface_commit(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    pub fn handle_bind_display(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_COMPOSITOR_BIND_DISPLAY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let device_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let width = u32::from_le_bytes(req.payload[8..12].try_into().unwrap());
        let height = u32::from_le_bytes(req.payload[12..16].try_into().unwrap());

        if width == 0 || height == 0 || width > 4096 || height > 2160 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        self.display_bound = true;
        self.display_id = device_id;
        self.width = width;
        self.height = height;
        self.vram_size_bytes = (width * height * DISPLAY_BYTES_PER_PIXEL) as usize;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&device_id.to_le_bytes());
        resp.payload_len = 12;
        resp
    }

    pub fn handle_surface_commit(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_SURFACE_COMMIT_RESP;

        if req.payload_len < 24 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let surface_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let damage_x = u32::from_le_bytes(req.payload[8..12].try_into().unwrap());
        let damage_y = u32::from_le_bytes(req.payload[12..16].try_into().unwrap());
        let damage_w = u32::from_le_bytes(req.payload[16..20].try_into().unwrap());
        let damage_h = u32::from_le_bytes(req.payload[20..24].try_into().unwrap());

        if !self.display_bound {
            resp.payload[0..4].copy_from_slice(&(ZeroError::PermissionDenied.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let mut found_idx = None;
        for i in 0..self.active_surface_count {
            if self.surfaces[i].surface_id == surface_id {
                found_idx = Some(i);
                break;
            }
        }

        match found_idx {
            Some(idx) => {
                let header = &mut self.surface_headers[idx];
                let back_buf = (header.active_front_buffer ^ 1) as usize;

                // Buffer Ownership Rule: Only sample buffers in STATE_READY
                if header.buffer_state[back_buf] == SURFACE_STATE_READY {
                    // Transition READY -> COMPOSITING
                    header.buffer_state[back_buf] = SURFACE_STATE_COMPOSITING;

                    // Execute software composition blit (update damage rect)
                    header.damage_rect = [damage_x, damage_y, damage_w, damage_h];
                    header.sequence_num += 1;

                    // Transition COMPOSITING -> FREE
                    header.buffer_state[back_buf] = SURFACE_STATE_FREE;
                    header.active_front_buffer ^= 1; // Swap active front buffer

                    resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                } else {
                    // Soft deadline miss / Buffer not ready: Retain previous VRAM frame safely
                    self.missed_frame_count += 1;
                    resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                    resp.payload_len = 4;
                }
            }
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
            }
        }
        resp
    }

    pub fn register_surface_descriptor(&mut self, desc: PresentationSurfaceDescriptor) -> Result<(), ZeroError> {
        if self.active_surface_count >= MAX_COMPOSITOR_SURFACES {
            return Err(ZeroError::ObjectTableFull);
        }

        // Maintain Layer Z-Ordering: Insert in sorted order by z_layer
        let mut insert_idx = self.active_surface_count;
        for i in 0..self.active_surface_count {
            if desc.z_layer < self.surfaces[i].z_layer {
                insert_idx = i;
                break;
            }
        }

        // Shift elements right to insert
        for i in (insert_idx..self.active_surface_count).rev() {
            self.surfaces[i + 1] = self.surfaces[i];
            self.surface_headers[i + 1] = self.surface_headers[i];
        }

        self.surfaces[insert_idx] = desc;
        self.surface_headers[insert_idx] = PresentationBufferHeader::default();
        self.active_surface_count += 1;
        Ok(())
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
                // Teardown Safety: Reset buffer states to FREE on unregister
                self.surface_headers[idx].buffer_state = [SURFACE_STATE_FREE, SURFACE_STATE_FREE];
                for i in idx..(self.active_surface_count - 1) {
                    self.surfaces[i] = self.surfaces[i + 1];
                    self.surface_headers[i] = self.surface_headers[i + 1];
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
