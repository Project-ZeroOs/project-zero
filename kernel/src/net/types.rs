//! Project Zero - Stage 3M Networking Types & Descriptors
//!
//! Authoritative Contract: Stage 3M Architecture Rev3 (Approved & Frozen).

pub const MAX_NETWORK_INTERFACES: usize = 4;
pub const MAX_NETWORK_DEVICE_BINDINGS: usize = 4;
pub const MAX_PACKET_BUFFERS: usize = 32;
pub const MAX_SOCKETS: usize = 16;
pub const MAX_ROUTES: usize = 8;
pub const MAX_NEIGHBORS: usize = 16;
pub const MAX_PORT_BINDINGS: usize = 32;
pub const MAX_NETWORK_TIMERS: usize = 16;

pub const PACKET_BUFFER_SIZE: usize = 2048;
pub const PACKET_HEADROOM: usize = 64;
pub const MAX_ETHERNET_FRAME_SIZE: usize = 1518;
pub const DEFAULT_MTU: u16 = 1500;

pub const LOOPBACK_INTERFACE_ID: u16 = 1;
pub const LOOPBACK_IP: u32 = 0x7F000001; // 127.0.0.1 (Big-Endian)
pub const LOOPBACK_NETMASK: u32 = 0xFF000000; // 255.0.0.0
pub const LOOPBACK_MAC: [u8; 6] = [0, 0, 0, 0, 0, 0];
pub const BROADCAST_MAC: [u8; 6] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceType {
    Loopback = 1,
    Ethernet = 2,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceState {
    Down = 0,
    Up = 1,
    Resetting = 2,
    Faulted = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceBindingState {
    Unbound = 0,
    Operational = 1,
    Resetting = 2,
    Faulted = 3,
    Detached = 4,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketBufferState {
    Free = 0,
    RxAllocated = 1,
    InterfaceRxQueued = 2,
    SocketQueued = 3,
    SocketTxQueued = 4,
    DriverTxRing = 5,
    Transmitted = 6,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    Unset = 0,
    Udp = 1,
    Tcp = 2,
    Raw = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    Closed = 0,
    UdpUnbound = 1,
    UdpBound = 2,
    UdpConnected = 3,
    Listen = 4,
    SynSent = 5,
    SynReceived = 6,
    Established = 7,
    FinWait1 = 8,
    FinWait2 = 9,
    CloseWait = 10,
    Closing = 11,
    LastAck = 12,
    TimeWait = 13,
    PeerClosed = 14,
    Faulted = 15,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborState {
    Empty = 0,
    Incomplete = 1,
    Reachable = 2,
    Stale = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkProtocol {
    HopByHop = 0,
    Icmp = 1,
    Tcp = 6,
    Udp = 17,
    RawEthernet = 255,
}

/// PacketBufferSlot: 48 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct PacketBufferSlot {
    pub phys_addr: u64,             // 8 B
    pub timestamp_ticks: u64,       // 8 B
    pub buffer_id: u32,             // 4 B
    pub dma_buffer_id: u32,         // 4 B
    pub flags: u32,                 // 4 B
    pub interface_id: u16,          // 2 B
    pub data_offset: u16,           // 2 B
    pub data_len: u16,              // 2 B
    pub next_buffer_idx: u16,       // 2 B
    pub state: PacketBufferState,   // 1 B
    pub _pad: [u8; 11],             // 11 B
}
const _: () = assert!(core::mem::size_of::<PacketBufferSlot>() == 48);
const _: () = assert!(core::mem::align_of::<PacketBufferSlot>() == 8);

impl PacketBufferSlot {
    pub const fn empty() -> Self {
        Self {
            phys_addr: 0,
            timestamp_ticks: 0,
            buffer_id: 0,
            dma_buffer_id: 0,
            flags: 0,
            interface_id: 0,
            data_offset: PACKET_HEADROOM as u16,
            data_len: 0,
            next_buffer_idx: 0xFFFF,
            state: PacketBufferState::Free,
            _pad: [0; 11],
        }
    }
}

/// NetworkDeviceBinding: 32 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct NetworkDeviceBinding {
    pub binding_id: u16,            // 2 B
    pub device_generation: u16,     // 2 B
    pub interface_generation: u16,  // 2 B
    pub device_slot: u8,            // 1 B
    pub interface_slot: u8,         // 1 B
    pub occupied: bool,             // 1 B
    pub is_pseudo_device: bool,     // 1 B
    pub state: DeviceBindingState,  // 1 B
    pub _pad: [u8; 5],              // 5 B
    pub _reserved: [u8; 16],        // 16 B
}
const _: () = assert!(core::mem::size_of::<NetworkDeviceBinding>() == 32);
const _: () = assert!(core::mem::align_of::<NetworkDeviceBinding>() == 8);

impl NetworkDeviceBinding {
    pub const fn empty() -> Self {
        Self {
            binding_id: 0,
            device_generation: 0,
            interface_generation: 0,
            device_slot: 0xFF,
            interface_slot: 0,
            occupied: false,
            is_pseudo_device: false,
            state: DeviceBindingState::Unbound,
            _pad: [0; 5],
            _reserved: [0; 16],
        }
    }
}

/// NetworkInterfaceSlot: 64 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct NetworkInterfaceSlot {
    pub device_id: u64,             // 8 B
    pub ipv4_addr: u32,             // 4 B
    pub ipv4_netmask: u32,          // 4 B
    pub rx_packets_total: u32,      // 4 B
    pub tx_packets_total: u32,      // 4 B
    pub dropped_packets: u32,       // 4 B
    pub interface_id: u16,          // 2 B
    pub mtu: u16,                   // 2 B
    pub rx_head: u16,               // 2 B
    pub rx_tail: u16,               // 2 B
    pub rx_count: u16,              // 2 B
    pub tx_head: u16,               // 2 B
    pub tx_tail: u16,               // 2 B
    pub tx_count: u16,              // 2 B
    pub mac_addr: [u8; 6],          // 6 B
    pub if_type: InterfaceType,     // 1 B
    pub state: InterfaceState,      // 1 B
    pub _reserved: [u8; 12],        // 12 B (total 64 B)
}
const _: () = assert!(core::mem::size_of::<NetworkInterfaceSlot>() == 64);
const _: () = assert!(core::mem::align_of::<NetworkInterfaceSlot>() == 8);

impl NetworkInterfaceSlot {
    pub const fn empty() -> Self {
        Self {
            device_id: 0,
            ipv4_addr: 0,
            ipv4_netmask: 0,
            rx_packets_total: 0,
            tx_packets_total: 0,
            dropped_packets: 0,
            interface_id: 0,
            mtu: DEFAULT_MTU,
            rx_head: 0xFFFF,
            rx_tail: 0xFFFF,
            rx_count: 0,
            tx_head: 0xFFFF,
            tx_tail: 0xFFFF,
            tx_count: 0,
            mac_addr: [0; 6],
            if_type: InterfaceType::Loopback,
            state: InterfaceState::Down,
            _reserved: [0; 12],
        }
    }
}

/// SocketSlot: 64 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct SocketSlot {
    pub socket_id: u64,             // 8 B
    pub kernel_object_id: u64,      // 8 B
    pub owner_pid: u64,             // 8 B
    pub bound_waitqueue_id: u64,    // 8 B
    pub local_ip: u32,              // 4 B
    pub remote_ip: u32,             // 4 B
    pub error: i16,                 // 2 B
    pub bound_interface: u16,       // 2 B
    pub local_port: u16,            // 2 B
    pub remote_port: u16,           // 2 B
    pub rx_queue_head: u16,         // 2 B
    pub rx_queue_tail: u16,         // 2 B
    pub rx_queue_count: u16,        // 2 B
    pub rx_queue_limit: u16,        // 2 B
    pub occupied: bool,             // 1 B
    pub sock_type: SocketType,      // 1 B
    pub state: SocketState,         // 1 B
    pub _reserved: [u8; 5],         // 5 B
}
const _: () = assert!(core::mem::size_of::<SocketSlot>() == 64);
const _: () = assert!(core::mem::align_of::<SocketSlot>() == 8);

impl SocketSlot {
    pub const fn empty() -> Self {
        Self {
            socket_id: 0,
            kernel_object_id: 0,
            owner_pid: 0,
            bound_waitqueue_id: 0,
            local_ip: 0,
            remote_ip: 0,
            error: 0,
            bound_interface: 0,
            local_port: 0,
            remote_port: 0,
            rx_queue_head: 0xFFFF,
            rx_queue_tail: 0xFFFF,
            rx_queue_count: 0,
            rx_queue_limit: 8,
            occupied: false,
            sock_type: SocketType::Unset,
            state: SocketState::Closed,
            _reserved: [0; 5],
        }
    }
}

/// RouteEntry: 32 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct RouteEntry {
    pub dest_ip: u32,               // 4 B
    pub gateway_ip: u32,            // 4 B
    pub metric: u32,                // 4 B
    pub interface_id: u16,          // 2 B
    pub prefix_len: u8,             // 1 B
    pub occupied: bool,             // 1 B
    pub _reserved: [u8; 16],        // 16 B
}
const _: () = assert!(core::mem::size_of::<RouteEntry>() == 32);
const _: () = assert!(core::mem::align_of::<RouteEntry>() == 8);

impl RouteEntry {
    pub const fn empty() -> Self {
        Self {
            dest_ip: 0,
            gateway_ip: 0,
            metric: 0,
            interface_id: 0,
            prefix_len: 0,
            occupied: false,
            _reserved: [0; 16],
        }
    }
}

/// NeighborEntry (ARP): 48 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct NeighborEntry {
    pub last_used_ticks: u64,       // 8 B
    pub ip_addr: u32,               // 4 B
    pub timeout_ticks: u32,         // 4 B
    pub pending_buffer_id: u32,     // 4 B
    pub interface_id: u16,          // 2 B
    pub occupied: bool,             // 1 B
    pub state: NeighborState,       // 1 B
    pub retries_left: u8,           // 1 B
    pub _pad0: u8,                  // 1 B
    pub mac_addr: [u8; 6],          // 6 B
    pub _reserved: [u8; 16],        // 16 B
}
const _: () = assert!(core::mem::size_of::<NeighborEntry>() == 48);
const _: () = assert!(core::mem::align_of::<NeighborEntry>() == 8);

impl NeighborEntry {
    pub const fn empty() -> Self {
        Self {
            last_used_ticks: 0,
            ip_addr: 0,
            timeout_ticks: 0,
            pending_buffer_id: 0,
            interface_id: 0,
            occupied: false,
            state: NeighborState::Empty,
            retries_left: 0,
            _pad0: 0,
            mac_addr: [0; 6],
            _reserved: [0; 16],
        }
    }
}

/// PortBindingSlot: 24 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct PortBindingSlot {
    pub socket_id: u64,             // 8 B
    pub owner_pid: u64,             // 8 B
    pub ip_addr: u32,               // 4 B
    pub port: u16,                  // 2 B
    pub protocol: u8,               // 1 B
    pub occupied: bool,             // 1 B
}
const _: () = assert!(core::mem::size_of::<PortBindingSlot>() == 24);
const _: () = assert!(core::mem::align_of::<PortBindingSlot>() == 8);

impl PortBindingSlot {
    pub const fn empty() -> Self {
        Self {
            socket_id: 0,
            owner_pid: 0,
            ip_addr: 0,
            port: 0,
            protocol: 0,
            occupied: false,
        }
    }
}

/// NetworkTimerSlot: 16 bytes, 8-byte aligned.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy)]
pub struct NetworkTimerSlot {
    pub callback_data: u64,         // 8 B
    pub remaining_ticks: u32,       // 4 B
    pub target_id: u16,             // 2 B
    pub timer_type: u8,             // 1 B
    pub occupied: bool,             // 1 B
}
const _: () = assert!(core::mem::size_of::<NetworkTimerSlot>() == 16);
const _: () = assert!(core::mem::align_of::<NetworkTimerSlot>() == 8);

impl NetworkTimerSlot {
    pub const fn empty() -> Self {
        Self {
            callback_data: 0,
            remaining_ticks: 0,
            target_id: 0,
            timer_type: 0,
            occupied: false,
        }
    }
}
