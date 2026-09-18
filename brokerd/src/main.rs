//! ZeroOS - brokerd System Service Broker Entry Point
#![no_std]
#![no_main]

use libzero::ipc::{channel_receive, channel_send};
use libzero::registry::ServiceRegistry;
use libzero::syscall::{sys_exit, sys_yield};

static mut REGISTRY: ServiceRegistry = ServiceRegistry::new();

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // Channel handle 1 is designated as the broker service listener endpoint
    let broker_listener_handle = 1u32;

    loop {
        // Non-blocking receive for incoming service requests
        match channel_receive(broker_listener_handle, true) {
            Ok(req) => {
                let resp = unsafe { (*(&raw mut REGISTRY)).handle_message(&req) };
                let _ = channel_send(broker_listener_handle, &resp, false);
            }
            Err(_) => {
                // Yield CPU if queue empty
                sys_yield();
            }
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}
