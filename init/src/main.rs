//! ZeroOS - init System Service Supervisor Entry Point
//!
//! Authoritative Contract: Stage 4A Architecture Rev6 & Stage 4B Architecture Rev12.
#![no_std]
#![no_main]

pub mod boot_epoch;
pub mod time_adapter;
pub mod supervisor;

use libzero::broker::ServiceName;
use libzero::persistence::MemoryPersistenceAuthority;
use libzero::supervisor::{Supervisor, DEFAULT_MAX_RETRIES};
use libzero::syscall::{sys_exit, sys_yield};
use libzero::time::TimeObservationFrame;

static mut SUPERVISOR: Supervisor = Supervisor::new();
static TIME_FRAME: TimeObservationFrame = TimeObservationFrame::new();

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

    // 4. Declare brokerd as primary system service
    let broker_name = ServiceName::from_str("brokerd");
    let broker_idx = (*sup)
        .declare_service(broker_name.as_bytes(), DEFAULT_MAX_RETRIES)
        .unwrap_or(0);
    let _ = (*sup).transition_starting(broker_idx);
    let _ = (*sup).transition_running(broker_idx);

    // 5. Declare resourced as local node resource manager daemon
    let resourced_name = ServiceName::from_str("resourced");
    let resourced_idx = (*sup)
        .declare_service(resourced_name.as_bytes(), DEFAULT_MAX_RETRIES)
        .unwrap_or(1);
    let _ = (*sup).transition_starting(resourced_idx);
    let _ = (*sup).transition_running(resourced_idx);

    // 6. Declare Stage 5 User Interaction & Presentation Daemons
    let compositord_name = ServiceName::from_str("compositord");
    let comp_idx = (*sup).declare_service(compositord_name.as_bytes(), DEFAULT_MAX_RETRIES).unwrap_or(2);
    let _ = (*sup).transition_starting(comp_idx);
    let _ = (*sup).transition_running(comp_idx);

    let surfaced_name = ServiceName::from_str("surfaced");
    let surf_idx = (*sup).declare_service(surfaced_name.as_bytes(), DEFAULT_MAX_RETRIES).unwrap_or(3);
    let _ = (*sup).transition_starting(surf_idx);
    let _ = (*sup).transition_running(surf_idx);

    let authui_name = ServiceName::from_str("authui");
    let auth_idx = (*sup).declare_service(authui_name.as_bytes(), DEFAULT_MAX_RETRIES).unwrap_or(4);
    let _ = (*sup).transition_starting(auth_idx);
    let _ = (*sup).transition_running(auth_idx);

    let uids_name = ServiceName::from_str("uids");
    let uids_idx = (*sup).declare_service(uids_name.as_bytes(), DEFAULT_MAX_RETRIES).unwrap_or(5);
    let _ = (*sup).transition_starting(uids_idx);
    let _ = (*sup).transition_running(uids_idx);

    // 7. Declare Stage 6A User Session Daemon (shelld)
    let shelld_name = ServiceName::from_str("shelld");
    let shell_idx = (*sup).declare_service(shelld_name.as_bytes(), DEFAULT_MAX_RETRIES).unwrap_or(6);
    let _ = (*sup).transition_starting(shell_idx);
    let _ = (*sup).transition_running(shell_idx);

    // 8. Yield to give scheduled services CPU time
    sys_yield();

    // 9. Clean shutdown: Running -> Stopping -> Stopped
    let _ = (*sup).stop_service(shell_idx);
    let _ = (*sup).stop_service(uids_idx);
    let _ = (*sup).stop_service(auth_idx);
    let _ = (*sup).stop_service(surf_idx);
    let _ = (*sup).stop_service(comp_idx);
    let _ = (*sup).stop_service(resourced_idx);
    let _ = (*sup).stop_service(broker_idx);

    // 9. Orderly system exit with status 0
    sys_exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}
