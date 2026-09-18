//! Project Zero - Stage 3M Network Interfaces & Queue Management
//!
//! Authoritative Invariant:
//! - I-NET-QUEUE-1: An RX/TX descriptor belongs to exactly one queue; drop on overflow.

use core::sync::atomic::{AtomicBool, Ordering};
use crate::net::types::*;
use crate::net::buf::{PACKET_BUFFER_TABLE, free_packet_buffer};

pub static mut INTERFACE_TABLE: [NetworkInterfaceSlot; MAX_NETWORK_INTERFACES] =
    [NetworkInterfaceSlot::empty(); MAX_NETWORK_INTERFACES];

static IF_LOCK: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub fn acquire_if_lock() {
    while IF_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
pub fn release_if_lock() {
    IF_LOCK.store(false, Ordering::Release);
}

/// Initializes the loopback interface (lo0) and interface table.
pub fn init_interfaces() {
    acquire_if_lock();
    unsafe {
        for slot in INTERFACE_TABLE.iter_mut() {
            *slot = NetworkInterfaceSlot::empty();
        }

        // Configure Loopback interface (lo0, interface_id = 1)
        let lo = &mut INTERFACE_TABLE[0];
        lo.interface_id = LOOPBACK_INTERFACE_ID;
        lo.device_id = 0;
        lo.if_type = InterfaceType::Loopback;
        lo.state = InterfaceState::Up;
        lo.mtu = DEFAULT_MTU;
        lo.mac_addr = LOOPBACK_MAC;
        lo.ipv4_addr = LOOPBACK_IP;
        lo.ipv4_netmask = LOOPBACK_NETMASK;
    }
    release_if_lock();
}

/// Registers a new physical Ethernet interface.
pub fn register_ethernet_interface(
    device_id: u64,
    mac_addr: [u8; 6],
    ipv4_addr: u32,
    ipv4_netmask: u32,
) -> Result<u16, ()> {
    acquire_if_lock();
    unsafe {
        for (idx, slot) in INTERFACE_TABLE.iter_mut().enumerate() {
            if slot.state == InterfaceState::Down && slot.interface_id == 0 {
                slot.interface_id = (idx as u16) + 1;
                slot.device_id = device_id;
                slot.if_type = InterfaceType::Ethernet;
                slot.state = InterfaceState::Up;
                slot.mtu = DEFAULT_MTU;
                slot.mac_addr = mac_addr;
                slot.ipv4_addr = ipv4_addr;
                slot.ipv4_netmask = ipv4_netmask;
                release_if_lock();
                return Ok(slot.interface_id);
            }
        }
    }
    release_if_lock();
    Err(())
}

/// Enqueues a packet buffer to an interface's RX queue. Drops packet if queue exceeds 16.
pub fn enqueue_rx_packet(if_idx: usize, buf_idx: usize) -> Result<(), ()> {
    if if_idx >= MAX_NETWORK_INTERFACES || buf_idx >= MAX_PACKET_BUFFERS {
        return Err(());
    }
    acquire_if_lock();
    unsafe {
        let iface = &mut INTERFACE_TABLE[if_idx];
        if iface.state != InterfaceState::Up {
            release_if_lock();
            let _ = free_packet_buffer(buf_idx);
            return Err(());
        }

        if iface.rx_count >= 16 {
            // Queue full: deterministic drop (I-NET-QUEUE-1)
            iface.dropped_packets = iface.dropped_packets.saturating_add(1);
            release_if_lock();
            let _ = free_packet_buffer(buf_idx);
            return Err(());
        }

        PACKET_BUFFER_TABLE[buf_idx].state = PacketBufferState::InterfaceRxQueued;
        PACKET_BUFFER_TABLE[buf_idx].interface_id = iface.interface_id;
        PACKET_BUFFER_TABLE[buf_idx].next_buffer_idx = 0xFFFF;

        if iface.rx_head == 0xFFFF {
            iface.rx_head = buf_idx as u16;
            iface.rx_tail = buf_idx as u16;
        } else {
            let old_tail = iface.rx_tail as usize;
            PACKET_BUFFER_TABLE[old_tail].next_buffer_idx = buf_idx as u16;
            iface.rx_tail = buf_idx as u16;
        }
        iface.rx_count += 1;
        iface.rx_packets_total += 1;
    }
    release_if_lock();
    Ok(())
}

/// Dequeues a packet buffer from an interface's RX queue.
pub fn dequeue_rx_packet(if_idx: usize) -> Option<usize> {
    if if_idx >= MAX_NETWORK_INTERFACES {
        return None;
    }
    acquire_if_lock();
    let res = unsafe {
        let iface = &mut INTERFACE_TABLE[if_idx];
        if iface.rx_head == 0xFFFF {
            None
        } else {
            let buf_idx = iface.rx_head as usize;
            iface.rx_head = PACKET_BUFFER_TABLE[buf_idx].next_buffer_idx;
            if iface.rx_head == 0xFFFF {
                iface.rx_tail = 0xFFFF;
            }
            iface.rx_count = iface.rx_count.saturating_sub(1);
            PACKET_BUFFER_TABLE[buf_idx].next_buffer_idx = 0xFFFF;
            Some(buf_idx)
        }
    };
    release_if_lock();
    res
}

/// Enqueues a packet buffer to an interface's TX queue. Drops packet if queue exceeds 16.
pub fn enqueue_tx_packet(if_idx: usize, buf_idx: usize) -> Result<(), ()> {
    if if_idx >= MAX_NETWORK_INTERFACES || buf_idx >= MAX_PACKET_BUFFERS {
        return Err(());
    }
    acquire_if_lock();
    unsafe {
        let iface = &mut INTERFACE_TABLE[if_idx];
        if iface.state != InterfaceState::Up {
            release_if_lock();
            let _ = free_packet_buffer(buf_idx);
            return Err(());
        }

        if iface.tx_count >= 16 {
            // Queue full: deterministic drop (I-NET-QUEUE-1)
            iface.dropped_packets = iface.dropped_packets.saturating_add(1);
            release_if_lock();
            let _ = free_packet_buffer(buf_idx);
            return Err(());
        }

        PACKET_BUFFER_TABLE[buf_idx].state = PacketBufferState::DriverTxRing;
        PACKET_BUFFER_TABLE[buf_idx].interface_id = iface.interface_id;
        PACKET_BUFFER_TABLE[buf_idx].next_buffer_idx = 0xFFFF;

        if iface.tx_head == 0xFFFF {
            iface.tx_head = buf_idx as u16;
            iface.tx_tail = buf_idx as u16;
        } else {
            let old_tail = iface.tx_tail as usize;
            PACKET_BUFFER_TABLE[old_tail].next_buffer_idx = buf_idx as u16;
            iface.tx_tail = buf_idx as u16;
        }
        iface.tx_count += 1;
        iface.tx_packets_total += 1;
    }
    release_if_lock();
    Ok(())
}

/// Dequeues a packet buffer from an interface's TX queue for transmission.
pub fn dequeue_tx_packet(if_idx: usize) -> Option<usize> {
    if if_idx >= MAX_NETWORK_INTERFACES {
        return None;
    }
    acquire_if_lock();
    let res = unsafe {
        let iface = &mut INTERFACE_TABLE[if_idx];
        if iface.tx_head == 0xFFFF {
            None
        } else {
            let buf_idx = iface.tx_head as usize;
            iface.tx_head = PACKET_BUFFER_TABLE[buf_idx].next_buffer_idx;
            if iface.tx_head == 0xFFFF {
                iface.tx_tail = 0xFFFF;
            }
            iface.tx_count = iface.tx_count.saturating_sub(1);
            PACKET_BUFFER_TABLE[buf_idx].next_buffer_idx = 0xFFFF;
            Some(buf_idx)
        }
    };
    release_if_lock();
    res
}
