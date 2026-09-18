//! ZeroOS - init System Service Supervisor Entry Point
#![no_std]
#![no_main]

use libzero::broker::ServiceName;
use libzero::supervisor::{Supervisor, DEFAULT_MAX_RETRIES};
use libzero::syscall::{sys_exit, sys_yield};

static mut SUPERVISOR: Supervisor = Supervisor::new();

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // 1. Bootstrap: Initialize supervisor state
    let sup = &raw mut SUPERVISOR;

    // 2. Declare brokerd as primary system service
    let broker_name = ServiceName::from_str("brokerd");
    let broker_idx = (*sup)
        .declare_service(broker_name.as_bytes(), DEFAULT_MAX_RETRIES)
        .unwrap_or(0);

    // 3. Service startup: Declared -> Starting -> Running
    let _ = (*sup).transition_starting(broker_idx);
    let _ = (*sup).transition_running(broker_idx);

    // 4. Yield to give scheduled services CPU time
    sys_yield();

    // 5. Clean shutdown: Running -> Stopping -> Stopped
    let _ = (*sup).stop_service(broker_idx);

    // 6. Orderly system exit with status 0
    sys_exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}
