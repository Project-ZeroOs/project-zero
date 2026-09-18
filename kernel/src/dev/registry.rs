//! Project Zero - Stage 3L Device Registry & Resource Table
//!
//! Authoritative Contract: Stage 3L Architecture Rev3 (Approved & Frozen).
//!
//! Lock Order:
//!   BLOCK_DEVICE_LOCK (4) < DEVICE_REGISTRY_LOCK (5) < DEVICE_RESOURCE_LOCK (6)
//!   < KERNEL_OBJECT_TABLE_LOCK (7) < SCHEDULER.lock (8) < CPU (IF=0)

use core::sync::atomic::{AtomicBool, Ordering};
use crate::dev::types::*;

pub struct DeviceRegistryLock {
    lock: AtomicBool,
}

impl DeviceRegistryLock {
    pub const fn new() -> Self {
        Self { lock: AtomicBool::new(false) }
    }

    #[inline(always)]
    pub fn acquire(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    pub fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

pub struct DeviceResourceLock {
    lock: AtomicBool,
}

impl DeviceResourceLock {
    pub const fn new() -> Self {
        Self { lock: AtomicBool::new(false) }
    }

    #[inline(always)]
    pub fn acquire(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    pub fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

pub static DEVICE_REGISTRY_LOCK: DeviceRegistryLock = DeviceRegistryLock::new();
pub static DEVICE_RESOURCE_LOCK: DeviceResourceLock = DeviceResourceLock::new();

pub static mut DEVICE_TABLE: [DeviceSlot; MAX_DEVICES] = [DeviceSlot::empty(); MAX_DEVICES];
pub static mut RESOURCE_TABLE: [DeviceResourceSlot; MAX_DEVICE_RESOURCES] = [DeviceResourceSlot::empty(); MAX_DEVICE_RESOURCES];

/// Initializes the device registry and resource tables.
pub fn init() {
    DEVICE_REGISTRY_LOCK.acquire();
    DEVICE_RESOURCE_LOCK.acquire();
    unsafe {
        for slot in DEVICE_TABLE.iter_mut() {
            *slot = DeviceSlot::empty();
        }
        for res in RESOURCE_TABLE.iter_mut() {
            *res = DeviceResourceSlot::empty();
        }
    }
    DEVICE_RESOURCE_LOCK.release();
    DEVICE_REGISTRY_LOCK.release();
}

/// Registers a new hardware device with an explicit DeviceId and DeviceClass.
pub fn register_device(device_id: DeviceId, class: DeviceClass) -> Result<usize, DeviceError> {
    DEVICE_REGISTRY_LOCK.acquire();
    unsafe {
        // Prevent duplicate DeviceId
        for slot in DEVICE_TABLE.iter() {
            if slot.occupied && slot.device_id == device_id {
                DEVICE_REGISTRY_LOCK.release();
                return Err(DeviceError::ResourceConflict);
            }
        }

        // Find empty slot
        for (idx, slot) in DEVICE_TABLE.iter_mut().enumerate() {
            if !slot.occupied {
                slot.occupied = true;
                slot.generation = slot.generation.wrapping_add(1);
                if slot.generation == 0 { slot.generation = 1; }
                slot.device_id = device_id;
                slot.class = class;
                slot.state = DeviceLifecycleState::Discovered;
                slot.driver_pid = 0;
                slot.kernel_object_id = 0;
                slot.resource_mask = 0;
                slot.bound_event_id = 0;
                DEVICE_REGISTRY_LOCK.release();
                return Ok(idx);
            }
        }
    }
    DEVICE_REGISTRY_LOCK.release();
    Err(DeviceError::CapacityExceeded)
}

/// Registers a hardware resource for a device with strict overlap conflict detection (I-DEV-RES-CONFLICT-1).
pub fn register_resource(
    device_id: DeviceId,
    res_type: ResourceType,
    sharing: ResourceSharing,
    base: u64,
    size: u64,
) -> Result<usize, DeviceError> {
    if size == 0 {
        return Err(DeviceError::InvalidParameter);
    }
    let new_end = base.checked_add(size).ok_or(DeviceError::InvalidParameter)?;

    DEVICE_RESOURCE_LOCK.acquire();
    unsafe {
        // Check for conflicts against all existing occupied resources
        for res in RESOURCE_TABLE.iter() {
            if !res.occupied || res.res_type != res_type {
                continue;
            }
            let exist_end = res.base.saturating_add(res.size);
            let overlaps = base < exist_end && new_end > res.base;

            if overlaps {
                // Determine compatibility according to Section 6.1 Matrix
                match res_type {
                    ResourceType::IoPort => {
                        // All IoPort overlaps are strictly rejected
                        DEVICE_RESOURCE_LOCK.release();
                        return Err(DeviceError::ResourceConflict);
                    }
                    ResourceType::Mmio | ResourceType::Irq | ResourceType::DmaBuffer => {
                        // Shared with Shared is permitted; any Exclusive involvement is rejected
                        if sharing != ResourceSharing::Shared || res.sharing != ResourceSharing::Shared {
                            DEVICE_RESOURCE_LOCK.release();
                            return Err(DeviceError::ResourceConflict);
                        }
                    }
                    ResourceType::Unused => {}
                }
            }
        }

        // Allocate slot in RESOURCE_TABLE
        for (idx, res) in RESOURCE_TABLE.iter_mut().enumerate() {
            if !res.occupied {
                res.occupied = true;
                res.res_type = res_type;
                res.sharing = sharing;
                res.device_id = device_id;
                res.base = base;
                res.size = size;

                // Update device's resource_mask if device exists in DEVICE_TABLE
                for dev in DEVICE_TABLE.iter_mut() {
                    if dev.occupied && dev.device_id == device_id {
                        dev.resource_mask |= 1u64 << (idx as u64);
                        break;
                    }
                }

                DEVICE_RESOURCE_LOCK.release();
                return Ok(idx);
            }
        }
    }
    DEVICE_RESOURCE_LOCK.release();
    Err(DeviceError::CapacityExceeded)
}

/// Steps a device through its lifecycle state machine.
pub fn transition_state(slot_idx: usize, new_state: DeviceLifecycleState) -> Result<(), DeviceError> {
    if slot_idx >= MAX_DEVICES {
        return Err(DeviceError::InvalidParameter);
    }

    DEVICE_REGISTRY_LOCK.acquire();
    unsafe {
        let slot = &mut DEVICE_TABLE[slot_idx];
        if !slot.occupied {
            DEVICE_REGISTRY_LOCK.release();
            return Err(DeviceError::DeviceNotFound);
        }

        let curr = slot.state;
        let legal = match (curr, new_state) {
            (DeviceLifecycleState::Discovered, DeviceLifecycleState::Probed) => true,
            (DeviceLifecycleState::Probed, DeviceLifecycleState::Attached) => true,
            (DeviceLifecycleState::Attached, DeviceLifecycleState::Ready) => true,
            (DeviceLifecycleState::Ready, DeviceLifecycleState::Active) => true,
            (DeviceLifecycleState::Active, DeviceLifecycleState::Ready) => true,
            (DeviceLifecycleState::Ready, DeviceLifecycleState::Quiescing) => true,
            (DeviceLifecycleState::Active, DeviceLifecycleState::Quiescing) => true,
            (DeviceLifecycleState::Quiescing, DeviceLifecycleState::Detached) => true,
            (DeviceLifecycleState::Detached, DeviceLifecycleState::Released) => true,
            (DeviceLifecycleState::Ready, DeviceLifecycleState::Faulted) => true,
            (DeviceLifecycleState::Active, DeviceLifecycleState::Faulted) => true,
            (DeviceLifecycleState::Faulted, DeviceLifecycleState::Resetting) => true,
            (DeviceLifecycleState::Ready, DeviceLifecycleState::Resetting) => true,
            (DeviceLifecycleState::Quiescing, DeviceLifecycleState::Resetting) => true,
            (DeviceLifecycleState::Resetting, DeviceLifecycleState::Ready) => true,
            (DeviceLifecycleState::Faulted, DeviceLifecycleState::Detached) => true,
            _ => false,
        };

        if !legal {
            DEVICE_REGISTRY_LOCK.release();
            return Err(DeviceError::InvalidParameter);
        }

        slot.state = new_state;
        if new_state == DeviceLifecycleState::Released {
            slot.occupied = false;
        }
    }
    DEVICE_REGISTRY_LOCK.release();
    Ok(())
}

/// Finds a device slot index by DeviceId.
pub fn find_device(device_id: DeviceId) -> Option<usize> {
    DEVICE_REGISTRY_LOCK.acquire();
    unsafe {
        for (idx, slot) in DEVICE_TABLE.iter().enumerate() {
            if slot.occupied && slot.device_id == device_id {
                DEVICE_REGISTRY_LOCK.release();
                return Some(idx);
            }
        }
    }
    DEVICE_REGISTRY_LOCK.release();
    None
}

/// Finds the first device of the specified class.
pub fn find_device_by_class(class: DeviceClass) -> Option<usize> {
    DEVICE_REGISTRY_LOCK.acquire();
    unsafe {
        for (idx, slot) in DEVICE_TABLE.iter().enumerate() {
            if slot.occupied && slot.class == class {
                DEVICE_REGISTRY_LOCK.release();
                return Some(idx);
            }
        }
    }
    DEVICE_REGISTRY_LOCK.release();
    None
}

/// Returns the count of occupied device slots.
pub fn count_devices() -> usize {
    let mut count = 0;
    DEVICE_REGISTRY_LOCK.acquire();
    unsafe {
        for slot in DEVICE_TABLE.iter() {
            if slot.occupied {
                count += 1;
            }
        }
    }
    DEVICE_REGISTRY_LOCK.release();
    count
}

/// Unbinds and tears down resources for devices owned by a terminating process (I-DEV-LIFETIME-1).
pub fn teardown_process_devices(pid: u64) -> usize {
    let mut cleaned = 0;
    DEVICE_REGISTRY_LOCK.acquire();
    DEVICE_RESOURCE_LOCK.acquire();
    unsafe {
        for slot in DEVICE_TABLE.iter_mut() {
            if slot.occupied && slot.driver_pid == pid {
                slot.driver_pid = 0;
                slot.state = DeviceLifecycleState::Detached;
                cleaned += 1;
            }
        }
    }
    DEVICE_RESOURCE_LOCK.release();
    DEVICE_REGISTRY_LOCK.release();
    cleaned
}
