//! Project Zero - Stage 3M NetworkDevice ↔ Stage 3L Device Binding Contract
//!
//! Authoritative Invariants:
//! - I-NET-DEV-1: Interface cannot outlive Stage 3L DeviceSlot (loopback virtual exception).
//! - I-NET-DEV-FAIL-1: Device fault/reset/detach propagates deterministically.

use core::sync::atomic::{AtomicBool, Ordering};
use crate::net::types::*;
use crate::dev::types::DeviceId;

pub static mut NETWORK_DEVICE_BINDINGS: [NetworkDeviceBinding; MAX_NETWORK_DEVICE_BINDINGS] =
    [NetworkDeviceBinding::empty(); MAX_NETWORK_DEVICE_BINDINGS];

static BIND_LOCK: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn acquire_bind_lock() {
    while BIND_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn release_bind_lock() {
    BIND_LOCK.store(false, Ordering::Release);
}

/// Binds a Stage 3L device to a Stage 3M interface.
pub fn bind_network_device(
    device_id: DeviceId,
    device_slot: usize,
    device_generation: u16,
    interface_slot: usize,
    is_pseudo_device: bool,
) -> Result<u16, ()> {
    acquire_bind_lock();
    unsafe {
        for (idx, slot) in NETWORK_DEVICE_BINDINGS.iter_mut().enumerate() {
            if !slot.occupied {
                slot.occupied = true;
                slot.binding_id = (idx as u16) + 1;
                slot.device_slot = device_slot as u8;
                slot.device_generation = device_generation;
                slot.interface_slot = interface_slot as u8;
                slot.interface_generation = 1;
                slot.is_pseudo_device = is_pseudo_device;
                slot.state = DeviceBindingState::Operational;
                release_bind_lock();
                return Ok(slot.binding_id);
            }
        }
    }
    release_bind_lock();
    Err(())
}

/// Validates that an interface's backing Stage 3L device is still live and generation matches (I-NET-DEV-1).
pub fn validate_interface_binding(interface_slot: usize) -> Result<(), ()> {
    acquire_bind_lock();
    unsafe {
        for slot in NETWORK_DEVICE_BINDINGS.iter() {
            if slot.occupied && slot.interface_slot == (interface_slot as u8) {
                if slot.is_pseudo_device {
                    // Loopback virtual exception (I-NET-DEV-1)
                    release_bind_lock();
                    return Ok(());
                }

                if slot.state != DeviceBindingState::Operational {
                    release_bind_lock();
                    return Err(());
                }

                let d_slot = slot.device_slot as usize;
                if d_slot >= crate::dev::types::MAX_DEVICES {
                    release_bind_lock();
                    return Err(());
                }

                // Check live Stage 3L slot generation and state
                let dev_table = &crate::dev::registry::DEVICE_TABLE;
                if !dev_table[d_slot].occupied || dev_table[d_slot].generation != slot.device_generation {
                    release_bind_lock();
                    return Err(());
                }

                release_bind_lock();
                return Ok(());
            }
        }
    }
    release_bind_lock();
    Err(())
}

/// Propagates a device failure/fault event to the bound network interface (I-NET-DEV-FAIL-1).
pub fn on_device_fault_event(device_slot: usize) {
    acquire_bind_lock();
    unsafe {
        for slot in NETWORK_DEVICE_BINDINGS.iter_mut() {
            if slot.occupied && slot.device_slot == (device_slot as u8) && !slot.is_pseudo_device {
                slot.state = DeviceBindingState::Faulted;
                // Transition bound interface to Faulted
                let if_slot = slot.interface_slot as usize;
                if if_slot < MAX_NETWORK_INTERFACES {
                    crate::net::iface::INTERFACE_TABLE[if_slot].state = InterfaceState::Faulted;
                }
            }
        }
    }
    release_bind_lock();

    // Wake all blocked socket threads with -ENETDOWN
    crate::net::socket::wake_all_sockets_on_fault(0);
}

/// Unbinds a network device on detach.
pub fn unbind_network_device(binding_id: u16) -> Result<(), ()> {
    acquire_bind_lock();
    unsafe {
        for slot in NETWORK_DEVICE_BINDINGS.iter_mut() {
            if slot.occupied && slot.binding_id == binding_id {
                slot.occupied = false;
                slot.state = DeviceBindingState::Detached;
                release_bind_lock();
                return Ok(());
            }
        }
    }
    release_bind_lock();
    Err(())
}
