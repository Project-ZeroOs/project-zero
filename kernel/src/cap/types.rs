//! Project Zero - Stage 3H Capability Types & Rights Model
//!
//! Authoritative Contract: Stage 3H Architecture Rev3 & Implementation Plan Rev3.

pub const MAX_DERIVATION_DEPTH: u8 = 8;
pub const MAX_PROCESSES: usize = crate::task::process::MAX_PROCESSES; // 16
pub const MAX_HANDLES_PER_PROCESS: usize = crate::ipc::handle::MAX_HANDLES_PER_PROCESS; // 32
pub const MAX_CAPABILITY_NODES: usize = MAX_PROCESSES * MAX_HANDLES_PER_PROCESS; // 512

pub mod node_flags {
    pub const DELEGATED: u16 = 1 << 0;
    pub const IMMUTABLE: u16 = 1 << 1;
}

pub mod cap_rights {
    // Generic Management Rights (Bits 8..15)
    pub const DUPLICATE: u16 = 1 << 8;  // 0x0100: Derive a child capability
    pub const TRANSFER:  u16 = 1 << 9;  // 0x0200: Transfer capability via IPC
    pub const REVOKE:    u16 = 1 << 10; // 0x0400: Revoke descendant capabilities
    pub const CLOSE:     u16 = 1 << 11; // 0x0800: Release/close this capability
    pub const INSPECT:   u16 = 1 << 12; // 0x1000: Query object status/signals
    pub const AUDIT:     u16 = 1 << 13; // 0x2000: Inspect capability derivation tree

    // Type-Specific Rights (Bits 0..7) - Channel (Harmonized with Stage 3G)
    pub const CHANNEL_RECEIVE: u16 = 1 << 0; // 0x0001: Receive message from ring buffer (rights::READ)
    pub const CHANNEL_SEND:    u16 = 1 << 1; // 0x0002: Transmit message to ring buffer (rights::WRITE)

    // Type-Specific Rights (Bits 0..7) - ShmObject (Harmonized with Stage 3G)
    pub const SHM_MAP_READ:  u16 = 1 << 2; // 0x0004: Map with PAGE_NX (rights::MAP_RO)
    pub const SHM_MAP_WRITE: u16 = 1 << 3; // 0x0008: Map with PAGE_WRITABLE (rights::MAP_RW)
    pub const SHM_UNMAP:     u16 = 1 << 4; // 0x0010: Remove mapping from active space

    // Type-Specific Rights (Bits 0..7) - Storage / Filesystem (Stage 3K)
    pub const FILE_READ:     u16 = 1 << 5; // 0x0020: Read bytes / traverse & enumerate directory
    pub const FILE_WRITE:    u16 = 1 << 6; // 0x0040: Write bytes / create & delete in directory
    pub const FILE_SYNC:     u16 = 1 << 7; // 0x0080: Flush dirty data to persistent storage

    // Type-Specific Rights (Bits 0..7) - Device / Hardware Model (Stage 3L)
    pub const DEV_READ:             u16 = 1 << 0; // 0x0001: Read device registers / buffers / status
    pub const DEV_WRITE:            u16 = 1 << 1; // 0x0002: Write device registers / commands
    pub const DEV_CONTROL:          u16 = 1 << 2; // 0x0004: Issue control & configuration commands
    pub const DEV_MAP_MMIO:         u16 = 1 << 3; // 0x0008: Map MMIO windows into address space
    pub const DEV_DMA_ACQUIRE:      u16 = 1 << 4; // 0x0010: Allocate / pin physical DMA buffers
    pub const DEV_INTERRUPT_LISTEN: u16 = 1 << 5; // 0x0020: Bind & receive interrupt events
    pub const DEV_RESET:            u16 = 1 << 6; // 0x0040: Issue device reset sequence
    pub const DEV_ATTACH:           u16 = 1 << 7; // 0x0080: Bind/unbind driver to device slot

    // Type-Specific Rights (Bits 0..7) - Networking Subsystem (Stage 3M)
    pub const NET_BIND:             u16 = 1 << 0; // 0x0001: Bind unprivileged local port (>= 1024)
    pub const NET_LISTEN:           u16 = 1 << 1; // 0x0002: Listen for incoming connections
    pub const NET_ACCEPT:           u16 = 1 << 2; // 0x0004: Accept incoming connection
    pub const NET_CONNECT:          u16 = 1 << 3; // 0x0008: Connect to remote address/port
    pub const NET_SEND:             u16 = 1 << 4; // 0x0010: Transmit data over socket
    pub const NET_RECV:             u16 = 1 << 5; // 0x0020: Receive data from socket
    pub const NET_ROUTE:            u16 = 1 << 6; // 0x0040: Configure routing table entries
    pub const NET_CONFIG:           u16 = 1 << 7; // 0x0080: Configure interface parameters / bind privileged ports (< 1024)

    // Administrative / Raw Rights (Bits 8..15)
    pub const NET_RAW:              u16 = 1 << 14; // 0x4000: Raw link-layer Ethernet II framing access

    /// Checks whether child rights are a strict bitwise subset of parent rights (I-DEV-CAP-SUBSET-1).
    #[inline(always)]
    pub fn is_rights_subset(child: u16, parent: u16) -> bool {
        (child & !parent) == 0
    }
}

/// Authoritative Capability Derivation Tree (CDT) Node Descriptor (24 bytes, 8-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityNode {
    /// Monotonic unique 64-bit ID (Offset 0x00..0x08).
    pub capability_id: u64,
    /// Parent process slot (0xFF if root) (Offset 0x08).
    pub parent_pslot: u8,
    /// Parent handle slot (0xFF if root) (Offset 0x09).
    pub parent_hslot: u8,
    /// Generation of parent handle when derived (Offset 0x0A..0x0C).
    pub parent_gen: u16,
    /// Derivation depth (0 = root, max 8) (Offset 0x0C).
    pub derivation_depth: u8,
    /// Explicit revocation latch (Offset 0x0D).
    pub is_revoked: bool,
    /// Delegation & node flags (Offset 0x0E..0x10).
    pub flags: u16,
    /// First child process slot (0xFF if none) (Offset 0x10).
    pub first_child_pslot: u8,
    /// First child handle slot (0xFF if none) (Offset 0x11).
    pub first_child_hslot: u8,
    /// Sibling process slot (0xFF if none) (Offset 0x12).
    pub next_sibling_pslot: u8,
    /// Sibling handle slot (0xFF if none) (Offset 0x13).
    pub next_sibling_hslot: u8,
    /// Explicit padding for 8-byte alignment (Offset 0x14..0x18).
    pub _reserved: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<CapabilityNode>() == 24);
const _: () = assert!(core::mem::align_of::<CapabilityNode>() == 8);
const _: () = assert!(core::mem::size_of::<[[CapabilityNode; MAX_HANDLES_PER_PROCESS]; MAX_PROCESSES]>() == 12288);

impl CapabilityNode {
    pub const fn empty() -> Self {
        Self {
            capability_id: 0,
            parent_pslot: 0xFF,
            parent_hslot: 0xFF,
            parent_gen: 0,
            derivation_depth: 0,
            is_revoked: false,
            flags: 0,
            first_child_pslot: 0xFF,
            first_child_hslot: 0xFF,
            next_sibling_pslot: 0xFF,
            next_sibling_hslot: 0xFF,
            _reserved: [0; 4],
        }
    }

    #[inline(always)]
    pub fn is_root(&self) -> bool {
        self.parent_pslot == 0xFF && self.parent_hslot == 0xFF
    }
}
