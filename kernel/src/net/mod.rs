//! Project Zero - Stage 3M Native Networking Substrate
//!
//! Authoritative Contract: Stage 3M Architecture Rev3 (Approved & Frozen).
//!
//! Provides a capability-governed, statically bounded, deterministic substrate connecting
//! user-space workloads to physical NICs and virtual loopback devices without dynamic kernel heap.
//!
//! Monotonic Lock Hierarchy (Levels 1..9):
//! Level 1:  FILESYSTEM_LOCK
//! Level 2:  STORAGE_OBJECT_TABLE_LOCK
//! Level 3:  BLOCK_CACHE_LOCK
//! Level 4:  BLOCK_DEVICE_LOCK
//! Level 5A: NETWORK_STACK_LOCK (Subsystem state, buffer pool, routing, ARP)
//! Level 5B: NETWORK_INTERFACE_LOCK (Per-interface RX/TX queue rings)
//! Level 5C: NETWORK_SOCKET_TABLE_LOCK (Socket table lookup, port binding)
//! Level 5D: DEVICE_REGISTRY_LOCK (Stage 3L device table)
//! Level 6:  DEVICE_RESOURCE_LOCK (Stage 3L resource table)
//! Level 7:  KERNEL_OBJECT_TABLE_LOCK (Stage 3G object table)
//! Level 8:  SCHEDULER.lock (Stage 3B scheduler)
//! Level 9:  CPU (IF=0) (Interrupt-disabled boundary)

pub mod types;
pub mod buf;
pub mod dev_binding;
pub mod iface;
pub mod arp;
pub mod route;
pub mod port;
pub mod transport;
pub mod socket;
pub mod tests;

use crate::mm::pmm::PhysicalMemoryManager;
use crate::dev::types::{DeviceId, ResourceSharing};
use types::*;

/// Global initialisation of Stage 3M Networking Subsystem.
pub fn init(pmm: &mut PhysicalMemoryManager) {
    crate::kprintln!("[NET] Initializing Stage 3M Native Networking Substrate...");

    // 1. Initialize Network Interfaces (lo0 loopback at interface_id = 1)
    iface::init_interfaces();

    // 2. Install Loopback Route (127.0.0.0/8 via lo0, metric 1)
    let _ = route::add_route(LOOPBACK_IP & LOOPBACK_NETMASK, 8, 0, LOOPBACK_INTERFACE_ID, 1);

    // 3. Bind loopback pseudo-device (I-NET-DEV-1 exception)
    let pseudo_dev_id = DeviceId(0);
    let _ = dev_binding::bind_network_device(pseudo_dev_id, 0xFF, 0, 0, true);

    // 4. Allocate 16 physical DMA frames (64 KiB) for 32 packet buffer slices via Stage 3L
    match crate::dev::dma::alloc_dma_buffer(pseudo_dev_id, 0, 16, ResourceSharing::Exclusive, pmm) {
        Ok((buf_id, base_phys)) => {
            buf::init_packet_buffer_pool(buf_id, base_phys);
            crate::kprintln!(
                "  [NET] Packet buffer pool initialized: 32 slices across 16 frames (DMA Buffer ID {}, Phys 0x{:016X})",
                buf_id, base_phys
            );
        }
        Err(e) => {
            crate::kprintln!("  [WARN] Stage 3M: Failed to allocate DMA buffer for packet pool: {:?}", e);
        }
    }

    crate::kprintln!("  [NET] Stage 3M Substrate Online.");
}
