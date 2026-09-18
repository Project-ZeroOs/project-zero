//! Project Zero - Stage 3M Longest-Prefix Matching IPv4 Routing Table
//!
//! Authoritative Invariant:
//! - I-NET-ROUTE-TEARDOWN-1: Interface teardown purges dependent routes automatically.

use core::sync::atomic::{AtomicBool, Ordering};
use crate::net::types::*;

pub static mut ROUTE_TABLE: [RouteEntry; MAX_ROUTES] =
    [RouteEntry::empty(); MAX_ROUTES];

static ROUTE_LOCK: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn acquire_route_lock() {
    while ROUTE_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn release_route_lock() {
    ROUTE_LOCK.store(false, Ordering::Release);
}

/// Helper generating bitmask for a given CIDR prefix length (0..32).
#[inline(always)]
pub fn prefix_to_netmask(prefix_len: u8) -> u32 {
    if prefix_len == 0 {
        0
    } else if prefix_len >= 32 {
        0xFFFFFFFF
    } else {
        !((1u32 << (32 - prefix_len)) - 1)
    }
}

/// Initializes routing table with default loopback route.
pub fn init_routes() {
    acquire_route_lock();
    unsafe {
        for entry in ROUTE_TABLE.iter_mut() {
            *entry = RouteEntry::empty();
        }

        // Route 0: 127.0.0.0/8 -> lo0 (interface_id = 1, metric = 0)
        let lo = &mut ROUTE_TABLE[0];
        lo.occupied = true;
        lo.dest_ip = 0x7F000000; // 127.0.0.0
        lo.prefix_len = 8;
        lo.interface_id = LOOPBACK_INTERFACE_ID;
        lo.gateway_ip = 0; // Directly attached
        lo.metric = 0;
    }
    release_route_lock();
}

/// Adds or updates a routing table entry.
pub fn add_route(
    dest_ip: u32,
    prefix_len: u8,
    gateway_ip: u32,
    interface_id: u16,
    metric: u32,
) -> Result<(), ()> {
    if prefix_len > 32 {
        return Err(());
    }
    let mask = prefix_to_netmask(prefix_len);
    let subnet = dest_ip & mask;

    acquire_route_lock();
    unsafe {
        // Check if route already exists
        for entry in ROUTE_TABLE.iter_mut() {
            if entry.occupied && entry.dest_ip == subnet && entry.prefix_len == prefix_len {
                entry.gateway_ip = gateway_ip;
                entry.interface_id = interface_id;
                entry.metric = metric;
                release_route_lock();
                return Ok(());
            }
        }

        // Allocate empty slot
        for entry in ROUTE_TABLE.iter_mut() {
            if !entry.occupied {
                entry.occupied = true;
                entry.dest_ip = subnet;
                entry.prefix_len = prefix_len;
                entry.gateway_ip = gateway_ip;
                entry.interface_id = interface_id;
                entry.metric = metric;
                release_route_lock();
                return Ok(());
            }
        }
    }
    release_route_lock();
    Err(()) // Route table full
}

/// Looks up the best route for a destination IP using Longest-Prefix Matching.
pub fn lookup_route(dest_ip: u32) -> Option<(u16, u32)> {
    acquire_route_lock();
    let mut best_interface: Option<u16> = None;
    let mut best_gateway: u32 = 0;
    let mut max_prefix_len: i32 = -1;
    let mut min_metric: u32 = u32::MAX;

    unsafe {
        for entry in ROUTE_TABLE.iter() {
            if !entry.occupied {
                continue;
            }

            let mask = prefix_to_netmask(entry.prefix_len);
            if (dest_ip & mask) == entry.dest_ip {
                let p = entry.prefix_len as i32;
                if p > max_prefix_len || (p == max_prefix_len && entry.metric < min_metric) {
                    max_prefix_len = p;
                    min_metric = entry.metric;
                    best_interface = Some(entry.interface_id);
                    best_gateway = entry.gateway_ip;
                }
            }
        }
    }
    release_route_lock();

    best_interface.map(|if_id| (if_id, best_gateway))
}

/// Purges all routes pointing to an interface when it goes down (I-NET-ROUTE-TEARDOWN-1).
pub fn purge_routes_for_interface(interface_id: u16) {
    acquire_route_lock();
    unsafe {
        for entry in ROUTE_TABLE.iter_mut() {
            if entry.occupied && entry.interface_id == interface_id {
                *entry = RouteEntry::empty();
            }
        }
    }
    release_route_lock();
}

/// Deletes a route from the routing table.
pub fn del_route(dest_ip: u32, prefix_len: u8) -> Result<(), ()> {
    if prefix_len > 32 {
        return Err(());
    }
    let mask = prefix_to_netmask(prefix_len);
    let subnet = dest_ip & mask;
    acquire_route_lock();
    unsafe {
        for entry in ROUTE_TABLE.iter_mut() {
            if entry.occupied && entry.dest_ip == subnet && entry.prefix_len == prefix_len {
                *entry = RouteEntry::empty();
                release_route_lock();
                return Ok(());
            }
        }
    }
    release_route_lock();
    Err(())
}
