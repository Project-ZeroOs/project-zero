//! Project Zero - Stage 3M Address Resolution Protocol (ARP) & Neighbor Table
//!
//! Authoritative Invariant:
//! - I-NET-ARP-1: Bounded 4-state engine; deterministic LRU eviction; zero dynamic heap.

use core::sync::atomic::{AtomicBool, Ordering};
use crate::net::types::*;
use crate::net::buf::free_packet_buffer;

pub static mut NEIGHBOR_TABLE: [NeighborEntry; MAX_NEIGHBORS] =
    [NeighborEntry::empty(); MAX_NEIGHBORS];

static ARP_LOCK: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn acquire_arp_lock() {
    while ARP_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn release_arp_lock() {
    ARP_LOCK.store(false, Ordering::Release);
}

/// Initializes the ARP neighbor table with static loopback mapping.
pub fn init_arp() {
    acquire_arp_lock();
    unsafe {
        for entry in NEIGHBOR_TABLE.iter_mut() {
            *entry = NeighborEntry::empty();
        }

        // Static loopback mapping
        let lo = &mut NEIGHBOR_TABLE[0];
        lo.occupied = true;
        lo.state = NeighborState::Reachable;
        lo.interface_id = LOOPBACK_INTERFACE_ID;
        lo.ip_addr = LOOPBACK_IP;
        lo.mac_addr = LOOPBACK_MAC;
        lo.timeout_ticks = 0xFFFFFFFF; // Never expires
        lo.last_used_ticks = 1;
    }
    release_arp_lock();
}

/// Resolves an IP address to a hardware MAC address. Updates last_used_ticks.
pub fn lookup_neighbor(ip_addr: u32, current_tick: u64) -> Option<[u8; 6]> {
    acquire_arp_lock();
    unsafe {
        for entry in NEIGHBOR_TABLE.iter_mut() {
            if entry.occupied && entry.ip_addr == ip_addr {
                if entry.state == NeighborState::Reachable || entry.state == NeighborState::Stale {
                    entry.last_used_ticks = current_tick;
                    let mac = entry.mac_addr;
                    release_arp_lock();
                    return Some(mac);
                }
            }
        }
    }
    release_arp_lock();
    None
}

/// Inserts or updates an ARP cache mapping. Deterministically evicts if table full.
pub fn update_neighbor(
    interface_id: u16,
    ip_addr: u32,
    mac_addr: [u8; 6],
    current_tick: u64,
) -> Result<(), ()> {
    acquire_arp_lock();
    unsafe {
        // Check if entry already exists
        for entry in NEIGHBOR_TABLE.iter_mut() {
            if entry.occupied && entry.ip_addr == ip_addr {
                entry.state = NeighborState::Reachable;
                entry.mac_addr = mac_addr;
                entry.interface_id = interface_id;
                entry.timeout_ticks = 30000; // 300 seconds
                entry.last_used_ticks = current_tick;
                if entry.pending_buffer_id != 0 {
                    let _ = free_packet_buffer((entry.pending_buffer_id & 0xFFFF) as usize);
                    entry.pending_buffer_id = 0;
                }
                release_arp_lock();
                return Ok(());
            }
        }

        // Find empty slot
        for entry in NEIGHBOR_TABLE.iter_mut() {
            if !entry.occupied {
                entry.occupied = true;
                entry.state = NeighborState::Reachable;
                entry.interface_id = interface_id;
                entry.ip_addr = ip_addr;
                entry.mac_addr = mac_addr;
                entry.timeout_ticks = 30000;
                entry.last_used_ticks = current_tick;
                entry.pending_buffer_id = 0;
                release_arp_lock();
                return Ok(());
            }
        }

        // Table full: execute deterministic LRU eviction (I-NET-ARP-1)
        // 1. Never evict Incomplete with retries remaining.
        // 2. Oldest Stale entry (lowest last_used_ticks) evicted first.
        // 3. Oldest Reachable evicted if no Stale entry exists.
        let mut candidate_idx: Option<usize> = None;
        let mut lowest_used: u64 = u64::MAX;

        // Pass 1: Stale
        for (idx, entry) in NEIGHBOR_TABLE.iter().enumerate() {
            if entry.occupied && entry.state == NeighborState::Stale && entry.ip_addr != LOOPBACK_IP {
                if entry.last_used_ticks < lowest_used {
                    lowest_used = entry.last_used_ticks;
                    candidate_idx = Some(idx);
                }
            }
        }

        // Pass 2: Reachable if no Stale
        if candidate_idx.is_none() {
            for (idx, entry) in NEIGHBOR_TABLE.iter().enumerate() {
                if entry.occupied && entry.state == NeighborState::Reachable && entry.ip_addr != LOOPBACK_IP {
                    if entry.last_used_ticks < lowest_used {
                        lowest_used = entry.last_used_ticks;
                        candidate_idx = Some(idx);
                    }
                }
            }
        }

        if let Some(evict_idx) = candidate_idx {
            let entry = &mut NEIGHBOR_TABLE[evict_idx];
            if entry.pending_buffer_id != 0 {
                let _ = free_packet_buffer((entry.pending_buffer_id & 0xFFFF) as usize);
            }
            entry.occupied = true;
            entry.state = NeighborState::Reachable;
            entry.interface_id = interface_id;
            entry.ip_addr = ip_addr;
            entry.mac_addr = mac_addr;
            entry.timeout_ticks = 30000;
            entry.last_used_ticks = current_tick;
            entry.pending_buffer_id = 0;
            release_arp_lock();
            return Ok(());
        }
    }
    release_arp_lock();
    Err(())
}

/// Registers an Incomplete resolution request with bounded retries.
pub fn request_resolution(
    interface_id: u16,
    ip_addr: u32,
    pending_buffer_id: u32,
    current_tick: u64,
) -> Result<(), ()> {
    acquire_arp_lock();
    unsafe {
        for entry in NEIGHBOR_TABLE.iter_mut() {
            if !entry.occupied {
                entry.occupied = true;
                entry.state = NeighborState::Incomplete;
                entry.interface_id = interface_id;
                entry.ip_addr = ip_addr;
                entry.mac_addr = [0; 6];
                entry.retries_left = 3;
                entry.timeout_ticks = 100; // 1 second
                entry.last_used_ticks = current_tick;
                entry.pending_buffer_id = pending_buffer_id;
                release_arp_lock();
                return Ok(());
            }
        }
    }
    release_arp_lock();
    Err(())
}

/// Periodic 10 ms timer tick update for ARP cache.
pub fn on_arp_timer_tick() {
    acquire_arp_lock();
    unsafe {
        for entry in NEIGHBOR_TABLE.iter_mut() {
            if !entry.occupied || entry.ip_addr == LOOPBACK_IP {
                continue;
            }

            if entry.timeout_ticks > 0 {
                entry.timeout_ticks -= 1;
                if entry.timeout_ticks == 0 {
                    match entry.state {
                        NeighborState::Incomplete => {
                            if entry.retries_left > 0 {
                                entry.retries_left -= 1;
                                entry.timeout_ticks = 100;
                            } else {
                                // Failed resolution
                                if entry.pending_buffer_id != 0 {
                                    let _ = free_packet_buffer((entry.pending_buffer_id & 0xFFFF) as usize);
                                }
                                *entry = NeighborEntry::empty();
                            }
                        }
                        NeighborState::Reachable => {
                            entry.state = NeighborState::Stale;
                        }
                        NeighborState::Stale => {
                            // Purge stale entry after another interval
                            *entry = NeighborEntry::empty();
                        }
                        NeighborState::Empty => {}
                    }
                }
            }
        }
    }
    release_arp_lock();
}

/// Clears all dynamic neighbor entries (retains lo0).
pub fn clear_neighbor_table() {
    acquire_arp_lock();
    unsafe {
        for entry in NEIGHBOR_TABLE.iter_mut() {
            if entry.occupied && entry.ip_addr != LOOPBACK_IP {
                if entry.pending_buffer_id != 0 {
                    let _ = free_packet_buffer((entry.pending_buffer_id & 0xFFFF) as usize);
                }
                *entry = NeighborEntry::empty();
            }
        }
    }
    release_arp_lock();
}

/// Retrieves the raw NeighborEntry for an IP address.
pub fn get_neighbor_entry(ip_addr: u32) -> Option<NeighborEntry> {
    acquire_arp_lock();
    let res = unsafe {
        let mut found = None;
        for entry in NEIGHBOR_TABLE.iter() {
            if entry.occupied && entry.ip_addr == ip_addr {
                found = Some(*entry);
                break;
            }
        }
        found
    };
    release_arp_lock();
    res
}

/// Explicitly inserts or updates a neighbor with specific initial state.
pub fn insert_or_update_neighbor(
    ip_addr: u32,
    interface_id: u16,
    mac_addr: [u8; 6],
    state: NeighborState,
    current_tick: u64,
) -> Result<usize, ()> {
    acquire_arp_lock();
    unsafe {
        for (i, entry) in NEIGHBOR_TABLE.iter_mut().enumerate() {
            if !entry.occupied || entry.ip_addr == ip_addr {
                entry.occupied = true;
                entry.state = state;
                entry.interface_id = interface_id;
                entry.ip_addr = ip_addr;
                entry.mac_addr = mac_addr;
                entry.timeout_ticks = 30000;
                entry.last_used_ticks = current_tick;
                entry.pending_buffer_id = 0;
                release_arp_lock();
                return Ok(i);
            }
        }
    }
    release_arp_lock();
    Err(())
}

/// Updates the state of an entry by index.
pub fn update_neighbor_state(idx: usize, state: NeighborState, mac: Option<[u8; 6]>) {
    if idx >= MAX_NEIGHBORS {
        return;
    }
    acquire_arp_lock();
    unsafe {
        let entry = &mut NEIGHBOR_TABLE[idx];
        entry.state = state;
        if let Some(m) = mac {
            entry.mac_addr = m;
        }
    }
    release_arp_lock();
}

/// Ages neighbor entries by a given number of ticks.
pub fn age_neighbor_entries(ticks: u32) {
    acquire_arp_lock();
    unsafe {
        for entry in NEIGHBOR_TABLE.iter_mut() {
            if !entry.occupied || entry.ip_addr == LOOPBACK_IP {
                continue;
            }
            if entry.timeout_ticks <= ticks {
                entry.timeout_ticks = 0;
                if entry.state == NeighborState::Reachable {
                    entry.state = NeighborState::Stale;
                }
            } else {
                entry.timeout_ticks -= ticks;
            }
        }
    }
    release_arp_lock();
}
