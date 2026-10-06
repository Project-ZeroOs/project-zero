//! ZeroOS - init System Service Supervisor Entry Point
//!
//! Authoritative Contract: Stage 4A Architecture Rev6 & Stage 4B Architecture Rev12.
//! WI-09: Minimum user-space service supervisor for ZeroOS Workstation Edition v1.0.
#![no_std]
#![no_main]

pub mod boot_epoch;
pub mod time_adapter;
pub mod supervisor;

use libzero::broker::ServiceName;
use libzero::error::ZeroError;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::supervisor::{Supervisor, DEFAULT_MAX_RETRIES, ServiceLifecycleState};
use libzero::syscall::{sys_exit, sys_yield};
use libzero::time::TimeObservationFrame;

static mut SUPERVISOR: Supervisor = Supervisor::new();
static TIME_FRAME: TimeObservationFrame = TimeObservationFrame::new();

#[derive(Debug, Clone, Copy)]
pub struct ServiceBootDescriptor {
    pub name: &'static str,
    pub max_retries: u32,
}

/// Authoritative Minimum v1.0 User-Space Service Startup Graph
pub const V1_SERVICE_GRAPH: &[ServiceBootDescriptor] = &[
    ServiceBootDescriptor { name: "brokerd", max_retries: DEFAULT_MAX_RETRIES },
    ServiceBootDescriptor { name: "resourced", max_retries: DEFAULT_MAX_RETRIES },
    ServiceBootDescriptor { name: "workspaced", max_retries: DEFAULT_MAX_RETRIES },
    ServiceBootDescriptor { name: "intentd", max_retries: DEFAULT_MAX_RETRIES },
    ServiceBootDescriptor { name: "groundd", max_retries: DEFAULT_MAX_RETRIES },
    ServiceBootDescriptor { name: "surfaced", max_retries: DEFAULT_MAX_RETRIES },
    ServiceBootDescriptor { name: "shelld", max_retries: DEFAULT_MAX_RETRIES },
];

/// Bootstraps all core system services in deterministic dependency-safe order.
pub unsafe fn bootstrap_service_graph(sup: *mut Supervisor) -> Result<usize, ZeroError> {
    let mut active = 0usize;
    for descriptor in V1_SERVICE_GRAPH.iter() {
        let service_name = ServiceName::from_str(descriptor.name);
        let idx = (*sup).declare_service(service_name.as_bytes(), descriptor.max_retries)?;
        (*sup).transition_starting(idx)?;
        (*sup).transition_running(idx)?;
        active += 1;
    }
    Ok(active)
}

/// Orderly shutdown of all services in dependency-safe reverse order.
pub unsafe fn shutdown_service_graph(sup: *mut Supervisor) {
    for idx in (0..V1_SERVICE_GRAPH.len()).rev() {
        let _ = (*sup).stop_service(idx);
    }
}

/// Handles service failure according to bounded retry policy (`DEFAULT_MAX_RETRIES` = 3).
/// Returns `Ok(true)` if service was restarted, `Ok(false)` if max retries exceeded (fail-closed).
pub unsafe fn handle_service_failure(sup: *mut Supervisor, index: usize) -> Result<bool, ZeroError> {
    match (*sup).handle_failure(index) {
        Ok(true) => {
            // Service transitioned to Restarting; perform restart: Restarting -> Starting -> Running
            (*sup).transition_starting(index)?;
            (*sup).transition_running(index)?;
            Ok(true)
        }
        Ok(false) => {
            // Retries exhausted: service remains in terminal Failed state
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Returns count of services currently in Running state.
pub unsafe fn get_running_service_count(sup: *mut Supervisor) -> usize {
    let mut count = 0usize;
    for svc in (*sup).services.iter() {
        if svc.occupied && svc.state == ServiceLifecycleState::Running {
            count += 1;
        }
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // 1. Bootstrap: Initialize supervisor state
    let sup = &raw mut SUPERVISOR;

    // 2. Boot Persistence Authority: Recover and increment BootEpochId
    let mut persistence = MemoryPersistenceAuthority::with_initial_values(1, 100);
    let boot_epoch = boot_epoch::advance_boot_epoch(&mut persistence).unwrap_or(2);

    // 3. Initialize Time Authority Adapter (1 GHz nominal frequency, 100ns SMP drift bound)
    let mut adapter = time_adapter::TimeAuthorityAdapter::new(&TIME_FRAME, boot_epoch, 1_000_000_000, 100)
        .expect("Time Authority Adapter initialization must succeed");
    let _ = adapter.publish_tick();

    // 4. Declare and start ZeroOS v1.0 core user-space service graph
    let _ = bootstrap_service_graph(sup);

    // 5. Init Persistent Supervision Loop
    // init remains alive for the lifetime of the ZeroOS user-space system.
    loop {
        sys_yield();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}

