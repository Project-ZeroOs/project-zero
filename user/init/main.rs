//! Project Zero - Freestanding User Bootstrap Entry Point (_start)
#![no_std]
#![no_main]

mod syscall;
use syscall::{sys_exit, sys_yield};

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    // 1. Cooperative yield 1
    sys_yield();

    // 2. Cooperative yield 2
    sys_yield();

    // 3. Clean exit with status 0
    sys_exit(0);
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}
