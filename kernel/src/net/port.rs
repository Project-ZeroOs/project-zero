//! Project Zero - Stage 3M Port Binding Management
//!
//! Authoritative Contract: Stage 3M Architecture Rev3 (Approved & Frozen).
//! Manages `PORT_BINDING_TABLE: [PortBindingSlot; 32]`.
//! Implements Invariant `I-NET-BIND-1` (Wildcard & Exact Coexistence Matrix).

use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use crate::syscall::numbers::SyscallError;
use super::types::{PortBindingSlot, MAX_PORT_BINDINGS};

pub static mut PORT_BINDING_TABLE: [PortBindingSlot; MAX_PORT_BINDINGS] =
    [PortBindingSlot::empty(); MAX_PORT_BINDINGS];

static PORT_LOCK: AtomicBool = AtomicBool::new(false);
static NEXT_EPHEMERAL: AtomicU16 = AtomicU16::new(49152);

#[inline(always)]
fn acquire_port_lock() {
    while PORT_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn release_port_lock() {
    PORT_LOCK.store(false, Ordering::Release);
}

/// Attempt to allocate and bind a local port.
/// If `requested_port == 0`, an ephemeral port in range [49152, 65535] is allocated.
/// Privileged ports (< 1024) require `has_net_config == true`.
pub fn bind_port(
    protocol: u8,
    ip_addr: u32,
    requested_port: u16,
    socket_id: u64,
    owner_pid: u64,
    has_net_config: bool,
) -> Result<u16, SyscallError> {
    acquire_port_lock();

    let target_port = if requested_port == 0 {
        // Ephemeral allocation
        let mut port = NEXT_EPHEMERAL.fetch_add(1, Ordering::Relaxed);
        if port < 49152 {
            port = 49152;
            NEXT_EPHEMERAL.store(49153, Ordering::Relaxed);
        }

        let start_port = port;
        let mut found = None;

        for _ in 0..16384 {
            let p = port;
            port = if port == 65535 { 49152 } else { port + 1 };

            // Check if available
            let mut collides = false;
            unsafe {
                for slot in PORT_BINDING_TABLE.iter() {
                    if !slot.occupied {
                        continue;
                    }
                    if slot.protocol == protocol && slot.port == p {
                        if slot.ip_addr == 0 || ip_addr == 0 || slot.ip_addr == ip_addr {
                            collides = true;
                            break;
                        }
                    }
                }
            }

            if !collides {
                found = Some(p);
                NEXT_EPHEMERAL.store(port, Ordering::Relaxed);
                break;
            }

            if port == start_port {
                break;
            }
        }

        match found {
            Some(p) => p,
            None => {
                release_port_lock();
                return Err(SyscallError::AddressInUse);
            }
        }
    } else {
        // Privileged port check (< 1024 requires NET_CONFIG)
        if requested_port < 1024 && !has_net_config {
            release_port_lock();
            return Err(SyscallError::PermissionDenied);
        }

        // Collision check against existing bindings
        unsafe {
            for slot in PORT_BINDING_TABLE.iter() {
                if !slot.occupied {
                    continue;
                }
                if slot.protocol == protocol && slot.port == requested_port {
                    // Wildcard & exact collision matrix
                    if slot.ip_addr == 0 || ip_addr == 0 || slot.ip_addr == ip_addr {
                        release_port_lock();
                        return Err(SyscallError::AddressInUse);
                    }
                }
            }
        }

        requested_port
    };

    // Find free slot
    unsafe {
        for slot in PORT_BINDING_TABLE.iter_mut() {
            if !slot.occupied {
                slot.occupied = true;
                slot.protocol = protocol;
                slot.port = target_port;
                slot.ip_addr = ip_addr;
                slot.socket_id = socket_id;
                slot.owner_pid = owner_pid;
                release_port_lock();
                return Ok(target_port);
            }
        }
    }

    release_port_lock();
    Err(SyscallError::OutOfMemory)
}

/// Unbind a specific port for a socket.
pub fn unbind_port(protocol: u8, ip_addr: u32, port: u16, socket_id: u64) -> Result<(), SyscallError> {
    acquire_port_lock();
    unsafe {
        for slot in PORT_BINDING_TABLE.iter_mut() {
            if slot.occupied
                && slot.protocol == protocol
                && slot.port == port
                && slot.socket_id == socket_id
                && (ip_addr == 0 || slot.ip_addr == ip_addr)
            {
                *slot = PortBindingSlot::empty();
                release_port_lock();
                return Ok(());
            }
        }
    }
    release_port_lock();
    Err(SyscallError::InvalidArgument)
}

/// Unbind all port bindings associated with a socket ID.
pub fn unbind_all_by_socket(socket_id: u64) {
    acquire_port_lock();
    unsafe {
        for slot in PORT_BINDING_TABLE.iter_mut() {
            if slot.occupied && slot.socket_id == socket_id {
                *slot = PortBindingSlot::empty();
            }
        }
    }
    release_port_lock();
}

/// Unbind all port bindings associated with a process ID (on exit).
pub fn unbind_all_by_pid(pid: u64) {
    acquire_port_lock();
    unsafe {
        for slot in PORT_BINDING_TABLE.iter_mut() {
            if slot.occupied && slot.owner_pid == pid {
                *slot = PortBindingSlot::empty();
            }
        }
    }
    release_port_lock();
}

/// Look up socket ID bound to (protocol, ip_addr, port).
/// Prefers exact IP match; falls back to wildcard (0.0.0.0) match.
pub fn lookup_socket(protocol: u8, ip_addr: u32, port: u16) -> Option<u64> {
    acquire_port_lock();
    let mut wildcard_match = None;

    unsafe {
        for slot in PORT_BINDING_TABLE.iter() {
            if !slot.occupied {
                continue;
            }
            if slot.protocol == protocol && slot.port == port {
                if slot.ip_addr == ip_addr {
                    release_port_lock();
                    return Some(slot.socket_id);
                }
                if slot.ip_addr == 0 && wildcard_match.is_none() {
                    wildcard_match = Some(slot.socket_id);
                }
            }
        }
    }

    release_port_lock();
    wildcard_match
}
