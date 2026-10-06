//! ZeroOS - Spatial Grounding & Session Continuity ABI Contracts (`grounding`)
//!
//! Authoritative Specification: Stage 6F Architecture Specification Rev7 (Frozen).
//! Implementation Plan: Stage 6F Implementation Plan Rev2.
//!
//! Invariant `I-6F-ABI-ALIGNMENT`:
//! All Stage 6F query and session handoff structures SHALL satisfy compile-time size, alignment,
//! and field offset assertions.

use core::mem::{align_of, offset_of, size_of};

// ============================================================================
// Stage 6F System IPC Opcodes & Error Constants
// ============================================================================

/// Opcode: Query spatial node hierarchy from `groundd` (0x0820).
pub const OP_GROUND_QUERY_SPATIAL: u64 = 0x0820;
/// Opcode: Response for spatial node query (0x0821).
pub const OP_GROUND_QUERY_SPATIAL_RESP: u64 = 0x0821;
/// Opcode: Register surface spatial index with `groundd` (0x0822).
pub const OP_GROUND_REGISTER_SPATIAL_INDEX: u64 = 0x0822;
/// Opcode: Response for spatial index registration (0x0823).
pub const OP_GROUND_REGISTER_SPATIAL_INDEX_RESP: u64 = 0x0823;

/// Opcode: Authenticated Session Handoff Fencing Proof (0x0611).
pub const OP_SESSION_HANDOFF_FENCING_PROOF: u64 = 0x0611;
/// Opcode: Session Handoff Recovery Reconciliation (0x0612).
pub const OP_SESSION_HANDOFF_RECONCILE: u64 = 0x0612;

/// Error Code: Stale Spatial Index generation mismatch (0x0730).
pub const ERR_STALE_SPATIAL_INDEX: u64 = 0x0730;
/// Error Code: Stale Session Migration Epoch (0x0731).
pub const ERR_STALE_SESSION_EPOCH: u64 = 0x0731;

// ============================================================================
// Magic Identification Numbers
// ============================================================================

pub const SPATIAL_QUERY_MAGIC: u64 = 0x5350415449414C5F; // "SPATIAL_"
pub const SESSION_SNAPSHOT_MAGIC: u64 = 0x534553535F5A4552; // "SESS_ZER"
pub const FENCING_PROOF_MAGIC: u64 = 0x46454E43455F5A45; // "FENCE_ZE"

// ============================================================================
// Privacy Tiers & Redaction Constants
// ============================================================================

pub const PRIVACY_TIER_0_PUBLIC: u8 = 0;
pub const PRIVACY_TIER_1_WORKSPACE_PRIVATE: u8 = 1;
pub const PRIVACY_TIER_2_WORKSPACE_SENSITIVE: u8 = 2;
pub const PRIVACY_TIER_3_TRUSTED_OVERLAY: u8 = 3;

pub const FLAG_SENSITIVE: u16 = 0x0001;
pub const FLAG_INTERACTIVE: u16 = 0x0002;

pub const BOUNDS_REDACTED: [i32; 4] = [0, 0, 0, 0];
pub const NODE_REDACTED: u8 = 0xFF;

pub const QUERY_FLAG_INCLUDE_CHILDREN: u32 = 0x0001;

// ============================================================================
// Canonical ABI Data Structures
// ============================================================================

/// Canonical 64-Byte Spatial Node Query (`SpatialNodeQuery`).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialNodeQuery {
    pub workspace_id: u64,              // Offset 0..8   : Target Workspace ID
    pub expected_layout_gen: u64,       // Offset 8..16  : Expected SurfaceLayoutGeneration
    pub expected_semantic_gen: u64,     // Offset 16..24 : Expected SurfaceSemanticGeneration
    pub bounding_box_min_x: i32,        // Offset 24..28 : Region filter X min
    pub bounding_box_min_y: i32,        // Offset 28..32 : Region filter Y min
    pub bounding_box_max_x: i32,        // Offset 32..36 : Region filter X max
    pub bounding_box_max_y: i32,        // Offset 36..40 : Region filter Y max
    pub max_nodes: u32,                 // Offset 40..44 : Maximum nodes to return (max 16)
    pub query_flags: u32,               // Offset 44..48 : Query options (0x01 = IncludeChildren)
    pub _reserved: [u8; 16],            // Offset 48..64 : Zero-filled alignment padding
}

impl Default for SpatialNodeQuery {
    fn default() -> Self {
        Self {
            workspace_id: 0,
            expected_layout_gen: 0,
            expected_semantic_gen: 0,
            bounding_box_min_x: 0,
            bounding_box_min_y: 0,
            bounding_box_max_x: 0,
            bounding_box_max_y: 0,
            max_nodes: 16,
            query_flags: 0,
            _reserved: [0; 16],
        }
    }
}

/// Canonical 128-Byte Spatial Node Descriptor (`SpatialNodeDescriptor`).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpatialNodeDescriptor {
    pub surface_id: u64,                // Offset 0..8   : Logical Surface ID
    pub node_id: u64,                   // Offset 8..16  : Accessibility Element Node ID
    pub bounds_min_x: i32,              // Offset 16..20 : X min [0,0,0,0] if BOUNDS_REDACTED
    pub bounds_min_y: i32,              // Offset 20..24 : Y min
    pub bounds_max_x: i32,              // Offset 24..28 : X max
    pub bounds_max_y: i32,              // Offset 28..32 : Y max
    pub node_type: u8,                  // Offset 32..33 : Element type (0xFF if NODE_REDACTED)
    pub privacy_tier: u8,               // Offset 33..34 : Privacy tier (Tier 0, 1, 2)
    pub flags: u16,                     // Offset 34..36 : Node flags
    pub _pad0: [u8; 4],                 // Offset 36..40 : Explicit padding for 8-byte alignment
    pub layout_gen: u64,                // Offset 40..48 : Authoritative SurfaceLayoutGeneration
    pub semantic_gen: u64,              // Offset 48..56 : Authoritative SurfaceSemanticGeneration
    pub node_name_len: u32,             // Offset 56..60 : String length
    pub node_name: [u8; 64],            // Offset 60..124: Zeroed if Tier 2 Redacted
    pub _reserved: [u8; 4],             // Offset 124..128: Zero-filled alignment padding
}

impl Default for SpatialNodeDescriptor {
    fn default() -> Self {
        Self {
            surface_id: 0,
            node_id: 0,
            bounds_min_x: 0,
            bounds_min_y: 0,
            bounds_max_x: 0,
            bounds_max_y: 0,
            node_type: 0,
            privacy_tier: PRIVACY_TIER_0_PUBLIC,
            flags: 0,
            _pad0: [0; 4],
            layout_gen: 0,
            semantic_gen: 0,
            node_name_len: 0,
            node_name: [0; 64],
            _reserved: [0; 4],
        }
    }
}

/// Canonical 64-Byte Logical Session Snapshot Header (`LogicalSessionSnapshotHeader`).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicalSessionSnapshotHeader {
    pub magic: u64,                    // Offset 0..8   : 0x534553535F5A4552 ("SESS_ZER")
    pub session_migration_epoch: u64,  // Offset 8..16  : Monotonic session export epoch counter
    pub dest_transaction_nonce: u64,   // Offset 16..24 : Destination-issued single-use handoff nonce
    pub session_id: u64,               // Offset 24..32 : Logical SessionId
    pub source_node_id: u64,           // Offset 32..40 : Originating node ID
    pub dest_node_id: u64,             // Offset 40..48 : Intended destination node ID
    pub workspace_count: u32,          // Offset 48..52 : Number of active workspaces serialized
    pub surface_count: u32,            // Offset 52..56 : Number of logical surfaces serialized
    pub payload_crc32: u32,            // Offset 56..60 : CRC32 over logical payload bytes
    pub _reserved: u32,                // Offset 60..64 : Zero-filled padding
}

impl Default for LogicalSessionSnapshotHeader {
    fn default() -> Self {
        Self {
            magic: SESSION_SNAPSHOT_MAGIC,
            session_migration_epoch: 0,
            dest_transaction_nonce: 0,
            session_id: 0,
            source_node_id: 0,
            dest_node_id: 0,
            workspace_count: 0,
            surface_count: 0,
            payload_crc32: 0,
            _reserved: 0,
        }
    }
}

/// Canonical 64-Byte Fencing Proof Descriptor (`FencingProofDescriptor`).
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FencingProofDescriptor {
    pub magic: u64,                    // Offset 0..8   : 0x46454E43455F5A45 ("FENCE_ZE")
    pub session_id: u64,               // Offset 8..16  : Logical SessionId
    pub source_node_id: u64,           // Offset 16..24 : Source Node ID
    pub dest_node_id: u64,             // Offset 24..32 : Destination Node ID
    pub previous_epoch: u64,           // Offset 32..40 : Epoch N
    pub proposed_epoch: u64,           // Offset 40..48 : Epoch N+1
    pub dest_transaction_nonce: u64,   // Offset 48..56 : Matching single-use dest_nonce
    pub fence_sequence: u64,           // Offset 56..64 : Monotonic persistent fence sequence number
}

impl Default for FencingProofDescriptor {
    fn default() -> Self {
        Self {
            magic: FENCING_PROOF_MAGIC,
            session_id: 0,
            source_node_id: 0,
            dest_node_id: 0,
            previous_epoch: 0,
            proposed_epoch: 0,
            dest_transaction_nonce: 0,
            fence_sequence: 0,
        }
    }
}

// ============================================================================
// Compile-Time Layout, Size, Alignment & Offset Assertions (Gate 6F-1)
// ============================================================================

const _: () = assert!(size_of::<SpatialNodeQuery>() == 64);
const _: () = assert!(align_of::<SpatialNodeQuery>() == 64);
const _: () = assert!(offset_of!(SpatialNodeQuery, workspace_id) == 0);
const _: () = assert!(offset_of!(SpatialNodeQuery, expected_layout_gen) == 8);
const _: () = assert!(offset_of!(SpatialNodeQuery, expected_semantic_gen) == 16);
const _: () = assert!(offset_of!(SpatialNodeQuery, bounding_box_min_x) == 24);
const _: () = assert!(offset_of!(SpatialNodeQuery, bounding_box_min_y) == 28);
const _: () = assert!(offset_of!(SpatialNodeQuery, bounding_box_max_x) == 32);
const _: () = assert!(offset_of!(SpatialNodeQuery, bounding_box_max_y) == 36);
const _: () = assert!(offset_of!(SpatialNodeQuery, max_nodes) == 40);
const _: () = assert!(offset_of!(SpatialNodeQuery, query_flags) == 44);
const _: () = assert!(offset_of!(SpatialNodeQuery, _reserved) == 48);

const _: () = assert!(size_of::<SpatialNodeDescriptor>() == 128);
const _: () = assert!(align_of::<SpatialNodeDescriptor>() == 64);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, surface_id) == 0);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, node_id) == 8);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, bounds_min_x) == 16);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, bounds_min_y) == 20);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, bounds_max_x) == 24);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, bounds_max_y) == 28);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, node_type) == 32);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, privacy_tier) == 33);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, flags) == 34);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, _pad0) == 36);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, layout_gen) == 40);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, semantic_gen) == 48);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, node_name_len) == 56);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, node_name) == 60);
const _: () = assert!(offset_of!(SpatialNodeDescriptor, _reserved) == 124);


const _: () = assert!(size_of::<LogicalSessionSnapshotHeader>() == 64);
const _: () = assert!(align_of::<LogicalSessionSnapshotHeader>() == 64);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, magic) == 0);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, session_migration_epoch) == 8);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, dest_transaction_nonce) == 16);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, session_id) == 24);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, source_node_id) == 32);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, dest_node_id) == 40);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, workspace_count) == 48);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, surface_count) == 52);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, payload_crc32) == 56);
const _: () = assert!(offset_of!(LogicalSessionSnapshotHeader, _reserved) == 60);

const _: () = assert!(size_of::<FencingProofDescriptor>() == 64);
const _: () = assert!(align_of::<FencingProofDescriptor>() == 64);
const _: () = assert!(offset_of!(FencingProofDescriptor, magic) == 0);
const _: () = assert!(offset_of!(FencingProofDescriptor, session_id) == 8);
const _: () = assert!(offset_of!(FencingProofDescriptor, source_node_id) == 16);
const _: () = assert!(offset_of!(FencingProofDescriptor, dest_node_id) == 24);
const _: () = assert!(offset_of!(FencingProofDescriptor, previous_epoch) == 32);
const _: () = assert!(offset_of!(FencingProofDescriptor, proposed_epoch) == 40);
const _: () = assert!(offset_of!(FencingProofDescriptor, dest_transaction_nonce) == 48);
const _: () = assert!(offset_of!(FencingProofDescriptor, fence_sequence) == 56);
