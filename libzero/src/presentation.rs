//! ZeroOS - Presentation & User Interaction Substrate (`presentation`)
//!
//! Authoritative Specification: Stage 5 Architecture Specification Rev4 (Frozen) & ADR-0031.
//! Implementation Plan: Stage 5 Implementation Plan Rev2.
//!
//! Invariant `I-AUTH-TX-NOT-CAPABILITY`:
//! AuthorizationTransaction references (`AuthorizationTransactionRef`) are authenticated protocol state
//! owned by `agentd` and NEVER constitute Stage 3H capability authority. They cannot derive,
//! amplify, transfer, or revoke kernel capabilities.

use core::fmt;
use crate::resource::DistributedId;

// ============================================================================
// Stage 5 System IPC Opcodes (Range: 0x0701 - 0x0712)
// ============================================================================

pub const OP_COMPOSITOR_BIND_DISPLAY: u64 = 0x0701;
pub const OP_COMPOSITOR_BIND_DISPLAY_RESP: u64 = 0x0702;
pub const OP_SURFACE_REGISTER: u64 = 0x0703;
pub const OP_SURFACE_REGISTER_RESP: u64 = 0x0704;
pub const OP_SURFACE_COMMIT: u64 = 0x0705;
pub const OP_SURFACE_COMMIT_RESP: u64 = 0x0706;
pub const OP_AUTHUI_DISPATCH_TRANSACTION: u64 = 0x0707;
pub const OP_AUTHUI_DISPATCH_TRANSACTION_RESP: u64 = 0x0708;
pub const OP_UIDS_INGEST_INTENT: u64 = 0x0709;
pub const OP_UIDS_INGEST_INTENT_RESP: u64 = 0x070A;
pub const OP_UIDS_REQUEST_MODAL_LOCK: u64 = 0x070B;
pub const OP_UIDS_REQUEST_MODAL_LOCK_RESP: u64 = 0x070C;
pub const OP_SURFACE_REGISTER_REMOTE_PROXY: u64 = 0x0713;
pub const OP_SURFACE_REGISTER_REMOTE_PROXY_RESP: u64 = 0x0714;
pub const OP_SURFACE_UNREGISTER_REMOTE_PROXY: u64 = 0x0715;
pub const OP_SURFACE_UNREGISTER_REMOTE_PROXY_RESP: u64 = 0x0716;
pub const OP_SURFACE_QUERY_REMOTE_PROXY: u64 = 0x0717;
pub const OP_SURFACE_QUERY_REMOTE_PROXY_RESP: u64 = 0x0718;
pub const OP_UIDS_SET_FOCUS: u64 = 0x0719;
pub const OP_UIDS_SET_FOCUS_RESP: u64 = 0x071A;
pub const OP_UIDS_REQUEST_MODAL_LOCK_REV2: u64 = 0x071B;
pub const OP_UIDS_REQUEST_MODAL_LOCK_REV2_RESP: u64 = 0x071C;
pub const OP_UIDS_ROUTE_REMOTE_INPUT: u64 = 0x071D;
pub const OP_UIDS_ROUTE_REMOTE_INPUT_RESP: u64 = 0x071E;

// ============================================================================
// Stage 6C Capability Object Types
// ============================================================================

pub const CAP_TYPE_REMOTE_INPUT_POLICY: u32 = 0x0041;
pub const CAP_TYPE_INPUT_FOCUS_POLICY: u32  = 0x0042;
pub const CAP_TYPE_SYNTHETIC_INPUT: u32     = 0x0043;
pub const CAP_TYPE_ACCESSIBILITY_POLICY: u32 = 0x0044;
pub const CAP_TYPE_AGENT_INPUT: u32          = 0x0045;

// ============================================================================
// Stage 6C Event Type Enums & Source Provenance
// ============================================================================

pub const EVENT_TYPE_KEY: u16            = 0x0001;
pub const EVENT_TYPE_BUTTON: u16         = 0x0002;
pub const EVENT_TYPE_POINTER_MOTION: u16 = 0x0003;
pub const EVENT_TYPE_TOUCH: u16          = 0x0004;
pub const EVENT_TYPE_SCROLL: u16         = 0x0005;
pub const EVENT_TYPE_DEVICE_STATE: u16   = 0x0006;

pub const INPUT_SOURCE_PHYSICAL: u16      = 0x0000;
pub const INPUT_SOURCE_ACCESSIBILITY: u16 = 0x0001;
pub const INPUT_SOURCE_AUTOMATION: u16    = 0x0002;
pub const INPUT_SOURCE_AGENT: u16         = 0x0003;
pub const INPUT_SOURCE_REMOTE: u16        = 0x0004;
pub const INPUT_SOURCE_TRUSTED_AUTH: u16  = 0x0005;

pub const INPUT_EVENT_SIZE: usize = 64;

// ============================================================================
// System Presentation Constants & Static Bounds
// ============================================================================

pub const MAX_PHYSICAL_DISPLAYS: usize = 4;
pub const MAX_COMPOSITOR_SURFACES: usize = 64;
pub const MAX_INPUT_DEVICES: usize = 8;
pub const MAX_PENDING_AUTH_TRANSACTIONS: usize = 4;
pub const MAX_SURFACE_RAM_MB: usize = 128; // Quota acquired via Stage 4B resourced lease

pub const DISPLAY_DEFAULT_WIDTH: u32 = 1024;
pub const DISPLAY_DEFAULT_HEIGHT: u32 = 768;
pub const DISPLAY_BYTES_PER_PIXEL: u32 = 4; // ARGB8888 Format
pub const PRESENTATION_MAGIC: u32 = 0x5A505549; // "ZPUI"
pub const REMOTE_FRAME_MAGIC: u32 = 0x5A505246; // "ZPRF"
pub const REMOTE_FRAME_HEADER_SIZE: usize = 72;

pub const FLAG_FULL_FRAME: u16  = 0x0001;
pub const FLAG_DAMAGE_RECT: u16 = 0x0002;

pub const STREAM_STATE_CONNECTING: u8   = 0;
pub const STREAM_STATE_ACTIVE: u8       = 1;
pub const STREAM_STATE_STALE: u8        = 2;
pub const STREAM_STATE_DISCONNECTED: u8 = 3;

pub const LAYER_WORKSPACE_DEFAULT: u8 = 10;
pub const LAYER_SYSTEM_AUTH: u8 = 255;

pub const SURFACE_STATE_FREE: u8 = 0;
pub const SURFACE_STATE_WRITING: u8 = 1;
pub const SURFACE_STATE_READY: u8 = 2;
pub const SURFACE_STATE_COMPOSITING: u8 = 3;

pub const SURFACE_TYPE_REGULAR: u8 = 0;
pub const SURFACE_TYPE_AUTH_OVERLAY: u8 = 1;

pub const INPUT_EVENT_KEY_PRESS: u8 = 1;
pub const INPUT_EVENT_KEY_RELEASE: u8 = 2;
pub const INPUT_EVENT_POINTER_MOVE: u8 = 3;
pub const INPUT_EVENT_POINTER_BUTTON: u8 = 4;

pub const FOCUS_STATE_UNFOCUSED: u8 = 0;
pub const FOCUS_STATE_FOCUSED: u8 = 1;
pub const FOCUS_STATE_CAPTURED: u8 = 2;
pub const FOCUS_STATE_MODAL_LOCK: u8 = 3;

// ============================================================================
// Deterministic Wire-Level Remote Frame Header (`RemoteFrameHeader`)
// ============================================================================

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteFrameHeader {
    /// Magic identifier ("ZPRF" = 0x5A505246) (4 bytes).
    pub magic: u32,
    /// Protocol version (1 byte).
    pub version: u8,
    /// Pixel format: 1=ARGB8888 (1 byte).
    pub format: u8,
    /// Stream Flags: Bit 0=FULL_FRAME, Bit 1=DAMAGE_RECT, Bits 2..15 reserved (2 bytes).
    pub flags: u16,
    /// Target Local Session Context (16 bytes).
    pub target_session_id: DistributedId,
    /// Monotonic Stream Generation Incarnation (8 bytes).
    pub stream_generation: u64,
    /// Monotonic Frame Sequence Number (8 bytes).
    pub frame_sequence: u64,
    /// Viewport Width in Pixels (4 bytes).
    pub width: u32,
    /// Viewport Height in Pixels (4 bytes).
    pub height: u32,
    /// Damage Rect: [x, y, damage_width, damage_height] (16 bytes).
    pub damage_rect: [u32; 4],
    /// Length of Frame Payload following header in bytes (4 bytes).
    pub payload_len: u32,
    /// CRC32 Checksum of Payload Data (4 bytes).
    pub payload_crc32: u32,
}

const _: () = assert!(core::mem::size_of::<RemoteFrameHeader>() == REMOTE_FRAME_HEADER_SIZE);

// ============================================================================
// Remote Surface Proxy Descriptor (`RemoteSurfaceProxyDescriptor`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RemoteSurfaceProxyDescriptor {
    pub local_surface_id: u64,
    pub workspace_id: DistributedId,
    pub source_node_id: u64,
    pub stream_generation: u64,
    pub csdt_id: DistributedId,
    pub local_shm_handle: u32,
    pub width: u32,
    pub height: u32,
    pub z_layer: u8,
    pub stream_state: u8,
    pub _pad0: [u8; 2],
    pub last_valid_frame_received_tsc: u64,
    pub dropped_frame_count: u64,
}

impl Default for RemoteSurfaceProxyDescriptor {
    fn default() -> Self {
        Self {
            local_surface_id: 0,
            workspace_id: DistributedId::new(0, 0),
            source_node_id: 0,
            stream_generation: 0,
            csdt_id: DistributedId::new(0, 0),
            local_shm_handle: 0,
            width: DISPLAY_DEFAULT_WIDTH,
            height: DISPLAY_DEFAULT_HEIGHT,
            z_layer: LAYER_WORKSPACE_DEFAULT,
            stream_state: STREAM_STATE_CONNECTING,
            _pad0: [0; 2],
            last_valid_frame_received_tsc: 0,
            dropped_frame_count: 0,
        }
    }
}

// ============================================================================
// Zero-Copy Presentation Buffer Header (`PresentationBufferHeader`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PresentationBufferHeader {
    pub magic: u32,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub format: u32,
    pub active_front_buffer: u8,
    pub buffer_state: [u8; 2],
    pub _pad0: u8,
    pub damage_rect: [u32; 4],
    pub sequence_num: u64,
}

impl Default for PresentationBufferHeader {
    fn default() -> Self {
        Self {
            magic: PRESENTATION_MAGIC,
            width: DISPLAY_DEFAULT_WIDTH,
            height: DISPLAY_DEFAULT_HEIGHT,
            stride_bytes: DISPLAY_DEFAULT_WIDTH * DISPLAY_BYTES_PER_PIXEL,
            format: 1, // ARGB8888
            active_front_buffer: 0,
            buffer_state: [SURFACE_STATE_FREE, SURFACE_STATE_FREE],
            _pad0: 0,
            damage_rect: [0, 0, DISPLAY_DEFAULT_WIDTH, DISPLAY_DEFAULT_HEIGHT],
            sequence_num: 0,
        }
    }
}

// ============================================================================
// Presentation Surface Descriptor (`PresentationSurfaceDescriptor`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PresentationSurfaceDescriptor {
    pub surface_id: u64,
    pub workspace_id: DistributedId,
    pub owner_pid: u64,
    pub shm_handle: u32,
    pub surface_type: u8,
    pub z_layer: u8,
    pub _pad0: [u8; 2],
    pub x_offset: i32,
    pub y_offset: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor_q8: u16, // Q8.8 Fixed point scale
    pub opacity_pct: u8,
    pub _padding: [u8; 45],
}

impl Default for PresentationSurfaceDescriptor {
    fn default() -> Self {
        Self {
            surface_id: 0,
            workspace_id: DistributedId::new(0, 0),
            owner_pid: 0,
            shm_handle: 0,
            surface_type: SURFACE_TYPE_REGULAR,
            z_layer: LAYER_WORKSPACE_DEFAULT,
            _pad0: [0; 2],
            x_offset: 0,
            y_offset: 0,
            width: DISPLAY_DEFAULT_WIDTH,
            height: DISPLAY_DEFAULT_HEIGHT,
            scale_factor_q8: 256, // 1.0 in Q8.8
            opacity_pct: 100,
            _padding: [0; 45],
        }
    }
}

// ============================================================================
// Authenticated Authorization Transaction Protocol Reference
// (`AuthorizationTransactionRef`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationTransactionRef {
    pub transaction_id: DistributedId,
    pub agent_id: DistributedId,
    pub workspace_id: DistributedId,
    pub operation_id: DistributedId,
    pub requested_action_hash: [u8; 32],
    pub nonce: u64,
    pub expiry_tsc: u64,
}

impl Default for AuthorizationTransactionRef {
    fn default() -> Self {
        Self {
            transaction_id: DistributedId::new(0, 0),
            agent_id: DistributedId::new(0, 0),
            workspace_id: DistributedId::new(0, 0),
            operation_id: DistributedId::new(0, 0),
            requested_action_hash: [0; 32],
            nonce: 0,
            expiry_tsc: 0,
        }
    }
}

// ============================================================================
// Hardware Input Event Descriptor (`InputEventDescriptor`)
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InputEventDescriptor {
    pub event_type: u8,
    pub button_state: u8,
    pub key_code: u16,
    pub pointer_x: i32,
    pub pointer_y: i32,
    pub timestamp_tsc: u64,
}

impl Default for InputEventDescriptor {
    fn default() -> Self {
        Self {
            event_type: 0,
            button_state: 0,
            key_code: 0,
            pointer_x: 0,
            pointer_y: 0,
            timestamp_tsc: 0,
        }
    }
}

impl fmt::Display for PresentationSurfaceDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Surface({}: WS {}:{}, PID {}, Layer {})",
            self.surface_id, self.workspace_id.node_id, self.workspace_id.local_seq, self.owner_pid, self.z_layer
        )
    }
}

// ============================================================================
// Stage 6C Canonical 64-Byte Input Event ABI (`InputEvent`)
// ============================================================================

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEventHeader {
    pub device_id: u64,
    pub timestamp_monotonic_tsc: u64,
    pub sequence: u64,
    pub event_type: u16,
    pub source_provenance: u16,
    pub device_generation: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEventPayload {
    pub code_or_button: u32,
    pub x: i32,
    pub y: i32,
    pub modifiers: u32,
    pub touch_id: u32,
    pub pressure: u32,
    pub reserved: [u8; 8],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    pub header: InputEventHeader,
    pub payload: InputEventPayload,
}

const _: () = assert!(core::mem::size_of::<InputEvent>() == INPUT_EVENT_SIZE);

impl Default for InputEvent {
    fn default() -> Self {
        Self {
            header: InputEventHeader {
                device_id: 0,
                timestamp_monotonic_tsc: 0,
                sequence: 0,
                event_type: EVENT_TYPE_KEY,
                source_provenance: INPUT_SOURCE_PHYSICAL,
                device_generation: 0,
            },
            payload: InputEventPayload {
                code_or_button: 0,
                x: 0,
                y: 0,
                modifiers: 0,
                touch_id: 0,
                pressure: 0,
                reserved: [0; 8],
            },
        }
    }
}


