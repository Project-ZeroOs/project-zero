//! Project Zero - PerCpu Architecture (Stage 3A)
//!
//! Provides minimal, hardware-aligned per-CPU state for the BSP (CPU 0).
//! Accessible via `%gs` base register configured with `IA32_GS_BASE`.

use crate::hal::arch::x86_64::cpu::{get_gs_base, read_gs_u64_at_16, set_gs_base};
use super::thread::KernelThread;

/// Minimal architectural PerCpu structure for Project Zero (CPU 0 / BSP).
#[repr(C)]
pub struct PerCpu {
    /// Self-pointer for GS-relative dereferencing (offset 0).
    pub self_ptr: *mut PerCpu,

    /// Unique logical core identifier (0 for BSP) (offset 8).
    pub cpu_id: u32,

    /// Hardware Local APIC ID (offset 12).
    pub lapic_id: u32,

    /// Pointer to currently executing KernelThread (offset 16).
    pub current_thread: *mut KernelThread,

    /// Pointer to CPU idle thread (offset 24).
    pub idle_thread: *mut KernelThread,

    /// Preemption disable nesting counter (offset 32).
    pub preempt_count: u32,

    /// Interrupt nesting depth counter (offset 36).
    pub nested_irq_count: u32,

    /// Deferred preemption request flag (offset 40, Stage 3C extension).
    pub need_resched: u32,

    /// Explicit padding to maintain 8-byte alignment (offset 44).
    pub _pad: u32,
}

// ====================================================================
// Compile-Time Offset and Size Assertions (Stage 3C PerCpu Layout)
// ====================================================================

const _: () = assert!(core::mem::offset_of!(PerCpu, self_ptr) == 0);
const _: () = assert!(core::mem::offset_of!(PerCpu, cpu_id) == 8);
const _: () = assert!(core::mem::offset_of!(PerCpu, lapic_id) == 12);
const _: () = assert!(core::mem::offset_of!(PerCpu, current_thread) == 16);
const _: () = assert!(core::mem::offset_of!(PerCpu, idle_thread) == 24);
const _: () = assert!(core::mem::offset_of!(PerCpu, preempt_count) == 32);
const _: () = assert!(core::mem::offset_of!(PerCpu, nested_irq_count) == 36);
const _: () = assert!(core::mem::offset_of!(PerCpu, need_resched) == 40);
const _: () = assert!(core::mem::offset_of!(PerCpu, _pad) == 44);
const _: () = assert!(core::mem::size_of::<PerCpu>() == 48);
const _: () = assert!(core::mem::align_of::<PerCpu>() == 8);

/// Authoritative BSP PerCpu instance.
pub static mut BSP_PERCPU: PerCpu = PerCpu {
    self_ptr: core::ptr::null_mut(),
    cpu_id: 0,
    lapic_id: 0,
    current_thread: core::ptr::null_mut(),
    idle_thread: core::ptr::null_mut(),
    preempt_count: 0,
    nested_irq_count: 0,
    need_resched: 0,
    _pad: 0,
};

/// Stage 3N: Per-CPU PerCpu instance array (MAX_CPUS = 4).
/// Index 0 = BSP (aliased to BSP_PERCPU, kept in sync by init_bsp_percpu).
/// Indices 1..3 = APs (initialized by `smp::bootstrap::ap_startup_entry`).
#[used]
#[export_name = "PER_CPU_INSTANCES"]
pub static mut PER_CPU_INSTANCES: [PerCpu; 4] = [
    PerCpu {
        self_ptr: core::ptr::null_mut(),
        cpu_id: 0, lapic_id: 0,
        current_thread: core::ptr::null_mut(),
        idle_thread: core::ptr::null_mut(),
        preempt_count: 0, nested_irq_count: 0, need_resched: 0, _pad: 0,
    },
    PerCpu {
        self_ptr: core::ptr::null_mut(),
        cpu_id: 1, lapic_id: 0,
        current_thread: core::ptr::null_mut(),
        idle_thread: core::ptr::null_mut(),
        preempt_count: 0, nested_irq_count: 0, need_resched: 0, _pad: 0,
    },
    PerCpu {
        self_ptr: core::ptr::null_mut(),
        cpu_id: 2, lapic_id: 0,
        current_thread: core::ptr::null_mut(),
        idle_thread: core::ptr::null_mut(),
        preempt_count: 0, nested_irq_count: 0, need_resched: 0, _pad: 0,
    },
    PerCpu {
        self_ptr: core::ptr::null_mut(),
        cpu_id: 3, lapic_id: 0,
        current_thread: core::ptr::null_mut(),
        idle_thread: core::ptr::null_mut(),
        preempt_count: 0, nested_irq_count: 0, need_resched: 0, _pad: 0,
    },
];

#[inline(always)]
pub fn set_current_thread(thread: *mut KernelThread) {
    unsafe {
        BSP_PERCPU.current_thread = thread;
    }
}

#[inline(always)]
pub fn set_idle_thread(thread: *mut KernelThread) {
    unsafe {
        BSP_PERCPU.idle_thread = thread;
    }
}

#[inline(always)]
pub fn get_idle_thread() -> *mut KernelThread {
    unsafe { BSP_PERCPU.idle_thread }
}

/// Initializes the BSP PerCpu structure and sets IA32_GS_BASE.
pub fn init_bsp_percpu(initial_thread: *mut KernelThread) {
    unsafe {
        let bsp_ptr = &raw mut BSP_PERCPU;
        (*bsp_ptr).self_ptr = bsp_ptr;
        (*bsp_ptr).cpu_id = 0;
        (*bsp_ptr).lapic_id = 0;
        (*bsp_ptr).current_thread = initial_thread;
        (*bsp_ptr).idle_thread = core::ptr::null_mut();
        (*bsp_ptr).preempt_count = 0;
        (*bsp_ptr).nested_irq_count = 0;

        // Write the authoritative PerCpu pointer to IA32_GS_BASE
        set_gs_base(bsp_ptr as u64);
    }
}

/// Returns a raw pointer to the BSP PerCpu instance.
#[inline(always)]
pub fn get_bsp_percpu() -> *mut PerCpu {
    &raw mut BSP_PERCPU
}

/// Returns the current thread pointer read directly through `%gs:[16]`.
#[inline(always)]
pub fn current_thread_from_gs() -> *mut KernelThread {
    unsafe { read_gs_u64_at_16() as *mut KernelThread }
}

/// Verifies that IA32_GS_BASE matches BSP_PERCPU and that GS-relative
/// dereferencing returns the expected thread pointer.
pub fn verify_gs_access(expected_thread: *mut KernelThread) -> bool {
    let gs_base = get_gs_base();
    let expected_base = &raw mut BSP_PERCPU as u64;

    if gs_base != expected_base {
        return false;
    }

    let read_thread = current_thread_from_gs();
    read_thread == expected_thread
}

/// Returns a pointer to the current CPU's PerCpu instance (via IA32_GS_BASE).
#[inline(always)]
pub fn current_percpu_ptr() -> *mut PerCpu {
    let base = get_gs_base();
    if base != 0 {
        base as *mut PerCpu
    } else {
        &raw mut BSP_PERCPU
    }
}

/// Returns the logical CPU ID of the currently executing CPU.
#[inline(always)]
pub fn current_cpu_id() -> u32 {
    let ptr = current_percpu_ptr();
    unsafe { (*ptr).cpu_id }
}
