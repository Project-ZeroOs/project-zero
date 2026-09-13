//! Project Zero - Cooperative Round-Robin Scheduler
//!
//! Manages thread execution queues and coordinates context switching.

use super::thread::{Thread, ThreadState};
use core::sync::atomic::{AtomicBool, Ordering};

const MAX_THREADS: usize = 8;

extern "C" {
    fn switch_context(prev_rsp: *mut *mut u8, next_rsp: *const u8);
}

pub struct Scheduler {
    lock: AtomicBool,
    threads: [Option<Thread>; MAX_THREADS],
    current_index: usize,
    thread_count: usize,
}

impl Scheduler {
    pub const fn new() -> Self {
        const INIT_OPT: Option<Thread> = None;
        Self {
            lock: AtomicBool::new(false),
            threads: [INIT_OPT; MAX_THREADS],
            current_index: 0,
            thread_count: 0,
        }
    }

    fn acquire(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }

    /// Registers the main kernel bootstrap thread as thread 0.
    pub fn init(&mut self) {
        let main_thread = Thread {
            id: super::thread::ThreadId(0),
            name: "kmain_master",
            state: ThreadState::Running,
            stack_memory: core::ptr::null_mut(),
            stack_size: 0,
            rsp: core::ptr::null_mut(),
        };

        self.threads[0] = Some(main_thread);
        self.current_index = 0;
        self.thread_count = 1;
    }

    /// Spawns a new thread into the ready queue.
    pub fn spawn(&mut self, thread: Thread) -> Result<(), &'static str> {
        self.acquire();
        for slot in self.threads.iter_mut() {
            if slot.is_none() {
                *slot = Some(thread);
                self.thread_count += 1;
                self.release();
                return Ok(());
            }
        }
        self.release();
        Err("Scheduler thread queue is full")
    }

    pub fn mark_current_terminated(&mut self) {
        self.acquire();
        if let Some(ref mut current) = self.threads[self.current_index] {
            current.state = ThreadState::Terminated;
        }
        self.release();
    }

    pub fn active_worker_threads(&self) -> usize {
        let mut count = 0;
        for i in 1..MAX_THREADS {
            if let Some(ref t) = self.threads[i] {
                if t.state != ThreadState::Terminated {
                    count += 1;
                }
            }
        }
        count
    }

    /// Yields CPU execution to the next ready thread.
    pub fn yield_now(&mut self) {
        self.acquire();

        if self.thread_count <= 1 {
            self.release();
            return;
        }

        let current_idx = self.current_index;
        let mut next_idx = (current_idx + 1) % MAX_THREADS;

        // Search for next ready thread
        let mut found = false;
        for _ in 0..MAX_THREADS {
            if let Some(ref thread) = self.threads[next_idx] {
                if thread.state == ThreadState::Ready || thread.state == ThreadState::Running {
                    found = true;
                    break;
                }
            }
            next_idx = (next_idx + 1) % MAX_THREADS;
        }

        if !found || next_idx == current_idx {
            self.release();
            return;
        }

        // Update states
        if let Some(ref mut current) = self.threads[current_idx] {
            if current.state == ThreadState::Running {
                current.state = ThreadState::Ready;
            }
        }
        if let Some(ref mut next) = self.threads[next_idx] {
            next.state = ThreadState::Running;
        }

        let prev_rsp_ptr = (&raw mut self.threads[current_idx].as_mut().unwrap().rsp);
        let next_rsp = self.threads[next_idx].as_ref().unwrap().rsp;
        self.current_index = next_idx;

        self.release();

        unsafe {
            switch_context(prev_rsp_ptr, next_rsp);
        }
    }
}

pub static mut SCHEDULER: Scheduler = Scheduler::new();
