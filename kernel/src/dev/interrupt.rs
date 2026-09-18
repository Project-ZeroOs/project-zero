//! Project Zero - Stage 3L Interrupt Subsystem & Shared IRQ Multi-Binding
//!
//! Authoritative Contract: Stage 3L Architecture Rev3 (Approved & Frozen).
//!
//! Invariants:
//! - I-DEV-IRQ-OWNERSHIP-1: Every active binding belongs to exactly one live context.
//! - I-DEV-IRQ-DELIVERY-1: Hardware interrupts broadcast to all occupied vector bindings.
//! - I-DEV-IRQ-STORM-1: Vector auto-masks after 1,000 IRQs per tick with 50-tick cooldown.

use core::sync::atomic::{AtomicBool, Ordering};
use crate::dev::types::*;
use crate::hal::arch::x86_64::lapic::lapic_eoi;

pub const MAX_IRQS_PER_TICK: u32 = 1000;
pub const STORM_COOLDOWN_TICKS: u32 = 50; // 500 ms at 100 Hz

pub static mut INTERRUPT_BINDINGS: [[InterruptBinding; MAX_SHARED_IRQ_BINDINGS]; MAX_INTERRUPT_VECTORS] =
    [[InterruptBinding::empty(); MAX_SHARED_IRQ_BINDINGS]; MAX_INTERRUPT_VECTORS];

pub static mut IRQ_LINE_STATES: [IrqLineState; MAX_INTERRUPT_VECTORS] =
    [IrqLineState::empty(); MAX_INTERRUPT_VECTORS];

static INTERRUPT_LOCK: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn acquire_int_lock() {
    while INTERRUPT_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn release_int_lock() {
    INTERRUPT_LOCK.store(false, Ordering::Release);
}

/// Initializes the interrupt dispatch table and line states.
pub fn init() {
    acquire_int_lock();
    unsafe {
        for v in 0..MAX_INTERRUPT_VECTORS {
            IRQ_LINE_STATES[v] = IrqLineState::empty();
            for b in 0..MAX_SHARED_IRQ_BINDINGS {
                INTERRUPT_BINDINGS[v][b] = InterruptBinding::empty();
            }
        }
    }
    release_int_lock();
}

/// Registers an interrupt binding for a hardware vector.
pub fn register_irq_binding(
    vector: u8,
    sharing: ResourceSharing,
    device_id: DeviceId,
    driver_pid: u64,
    event_object_id: u64,
    top_half: Option<fn(u8, DeviceId)>,
) -> Result<u16, DeviceError> {
    let v = vector as usize;
    acquire_int_lock();
    unsafe {
        let line = &mut IRQ_LINE_STATES[v];

        // If line is unoccupied, configure sharing mode
        if line.active_bindings == 0 {
            line.sharing = sharing;
        } else if line.sharing != ResourceSharing::Shared || sharing != ResourceSharing::Shared {
            release_int_lock();
            return Err(DeviceError::ResourceConflict);
        }

        if (line.active_bindings as usize) >= MAX_SHARED_IRQ_BINDINGS {
            release_int_lock();
            return Err(DeviceError::CapacityExceeded);
        }

        // Find free binding slot
        for (b_idx, slot) in INTERRUPT_BINDINGS[v].iter_mut().enumerate() {
            if !slot.occupied {
                slot.occupied = true;
                slot.binding_id = ((v as u16) << 8) | (b_idx as u16);
                slot.device_id = device_id;
                slot.driver_pid = driver_pid;
                slot.event_object_id = event_object_id;
                slot.interrupt_count = 0;
                slot.top_half = top_half;

                line.active_bindings += 1;
                let binding_id = slot.binding_id;
                release_int_lock();
                return Ok(binding_id);
            }
        }
    }
    release_int_lock();
    Err(DeviceError::CapacityExceeded)
}

/// Unregisters an interrupt binding.
pub fn unregister_irq_binding(binding_id: u16) -> Result<(), DeviceError> {
    let v = (binding_id >> 8) as usize;
    let b = (binding_id & 0xFF) as usize;

    if v >= MAX_INTERRUPT_VECTORS || b >= MAX_SHARED_IRQ_BINDINGS {
        return Err(DeviceError::InvalidParameter);
    }

    acquire_int_lock();
    unsafe {
        let slot = &mut INTERRUPT_BINDINGS[v][b];
        if !slot.occupied || slot.binding_id != binding_id {
            release_int_lock();
            return Err(DeviceError::DeviceNotFound);
        }

        *slot = InterruptBinding::empty();
        let line = &mut IRQ_LINE_STATES[v];
        line.active_bindings = line.active_bindings.saturating_sub(1);
        if line.active_bindings == 0 {
            line.sharing = ResourceSharing::Exclusive;
            line.masked = false;
            line.in_storm = false;
        }
    }
    release_int_lock();
    Ok(())
}

/// Top-half ISR dispatcher invoked on hardware interrupt vector arrival (IF = 0).
pub fn dispatch_interrupt(vector: u8) {
    let v = vector as usize;
    if v >= MAX_INTERRUPT_VECTORS {
        return;
    }

    // Always issue hardware EOI immediately
    unsafe { lapic_eoi(); }

    unsafe {
        let line = &mut IRQ_LINE_STATES[v];
        line.total_line_interrupts += 1;
        line.irq_count_current_tick += 1;

        // Storm Mitigation (I-DEV-IRQ-STORM-1): mask after 1,000 IRQs in 10 ms (Rev3)
        if line.irq_count_current_tick > MAX_IRQS_PER_TICK {
            line.masked = true;
            line.in_storm = true;
            line.cooldown_ticks_remaining = STORM_COOLDOWN_TICKS;
            return;
        }

        if line.masked {
            return;
        }

        // Broadcast delivery to all occupied bindings (I-DEV-IRQ-DELIVERY-1)
        for slot in INTERRUPT_BINDINGS[v].iter_mut() {
            if slot.occupied {
                slot.interrupt_count += 1;
                if let Some(th) = slot.top_half {
                    th(vector, slot.device_id);
                }
                if slot.event_object_id != 0 {
                    // Signal the bound kernel event object
                    signal_event_by_id(slot.event_object_id);
                }
            }
        }
    }
}

/// Signals a kernel Event object by ID outside interrupt lock.
fn signal_event_by_id(event_obj_id: u64) {
    let rflags = crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.acquire();
    unsafe {
        for slot in crate::ipc::object::KERNEL_OBJECT_TABLE.iter_mut() {
            if slot.occupied && slot.obj_type == crate::ipc::object::KernelObjectType::Event && slot.header.object_id == event_obj_id {
                slot.header.signals |= 1;
                break;
            }
        }
    }
    crate::ipc::object::KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
}

/// Periodic tick update called on every 10 ms LAPIC timer tick.
pub fn on_timer_tick() {
    unsafe {
        for v in 0..MAX_INTERRUPT_VECTORS {
            let line = &mut IRQ_LINE_STATES[v];
            line.irq_count_current_tick = 0;
            if line.cooldown_ticks_remaining > 0 {
                line.cooldown_ticks_remaining -= 1;
                if line.cooldown_ticks_remaining == 0 && line.in_storm {
                    line.in_storm = false;
                    line.masked = false;
                }
            }
        }
    }
}

/// Returns the number of active bindings on a given vector.
pub fn get_vector_binding_count(vector: u8) -> u8 {
    let v = vector as usize;
    if v >= MAX_INTERRUPT_VECTORS { return 0; }
    unsafe { IRQ_LINE_STATES[v].active_bindings }
}

/// Returns whether a vector is currently in storm state.
pub fn is_vector_in_storm(vector: u8) -> bool {
    let v = vector as usize;
    if v >= MAX_INTERRUPT_VECTORS { return false; }
    unsafe { IRQ_LINE_STATES[v].in_storm }
}

/// Returns whether a vector is currently masked.
pub fn is_vector_masked(vector: u8) -> bool {
    let v = vector as usize;
    if v >= MAX_INTERRUPT_VECTORS { return false; }
    unsafe { IRQ_LINE_STATES[v].masked }
}

/// Returns remaining cooldown ticks for a vector.
pub fn get_vector_cooldown_remaining(vector: u8) -> u32 {
    let v = vector as usize;
    if v >= MAX_INTERRUPT_VECTORS { return 0; }
    unsafe { IRQ_LINE_STATES[v].cooldown_ticks_remaining }
}
