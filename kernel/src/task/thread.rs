//! Project Zero - Kernel Thread Subsystem
//!
//! Provides thread descriptors, private kernel stacks, and execution state management.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicU64, Ordering};
use crate::mm::heap::ALLOCATOR;

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

pub struct Thread {
    pub id: ThreadId,
    pub name: &'static str,
    pub state: ThreadState,
    pub stack_memory: *mut u8,
    pub stack_size: usize,
    pub rsp: *mut u8,
}

impl Thread {
    /// Spawns a new kernel thread with a dedicated stack and entry function.
    pub fn new(name: &'static str, entry: fn()) -> Self {
        let id = ThreadId(NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed));
        let stack_size = 16384; // 16 KiB private kernel stack
        let layout = Layout::from_size_align(stack_size, 16).unwrap();
        let stack_memory = unsafe { ALLOCATOR.alloc(layout) };

        if stack_memory.is_null() {
            panic!("Failed to allocate stack for thread '{}'", name);
        }

        // Forge the initial stack frame to match switch_context restore order:
        // [rsp + 0]:  rbx (0)
        // [rsp + 8]:  rbp (0)
        // [rsp + 16]: r12 (0)
        // [rsp + 24]: r13 (0)
        // [rsp + 32]: r14 (0)
        // [rsp + 40]: r15 (0)
        // [rsp + 48]: rflags (0x202 = IF enabled)
        // [rsp + 56]: return address (thread_wrapper)
        let stack_top = (stack_memory as usize + stack_size) & !15;
        let mut sp = stack_top as *mut u64;

        unsafe {
            // Return address for `ret` instruction in switch_context
            sp = sp.sub(1); *sp = thread_wrapper as usize as u64;

            // Registers in reverse order of pop in switch_context:
            // pop rbx, pop rbp, pop r12, pop r13, pop r14, pop r15, popfq
            sp = sp.sub(1); *sp = entry as usize as u64; // rbx = entry function
            sp = sp.sub(1); *sp = 0;                     // rbp
            sp = sp.sub(1); *sp = 0;                     // r12
            sp = sp.sub(1); *sp = 0;                     // r13
            sp = sp.sub(1); *sp = 0;                     // r14
            sp = sp.sub(1); *sp = 0;                     // r15
            sp = sp.sub(1); *sp = 0x0202;                // rflags (IF = 1, reserved bit 1 = 1)
        }

        Self {
            id,
            name,
            state: ThreadState::Ready,
            stack_memory,
            stack_size,
            rsp: sp as *mut u8,
        }
    }
}

/// Trampoline executing the thread entry function and marking thread termination upon return.
extern "C" fn thread_wrapper() {
    // In thread_wrapper, rbx holds the function pointer passed during initialization
    let entry_fn: fn();
    unsafe {
        let rbx_val: usize;
        core::arch::asm!("mov {}, rbx", out(reg) rbx_val, options(nomem, nostack, preserves_flags));
        entry_fn = core::mem::transmute(rbx_val);
    }

    // Execute user thread function
    entry_fn();

    // Mark terminated and yield forever
    unsafe {
        crate::task::scheduler::SCHEDULER.mark_current_terminated();
        loop {
            crate::task::scheduler::SCHEDULER.yield_now();
        }
    }
}
