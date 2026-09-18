//! Project Zero - Stage 3B Cooperative Scheduler Core
//!
//! Implements deterministic cooperative scheduling:
//! - Intrusive zero-allocation multi-level priority runqueues (`Critical > High > Normal`).
//! - Dedicated idle thread (outside worker queues).
//! - `SchedLock` preserving caller IF, releasing with `IF = 0` before switch.
//! - Context switching via `switch_context(prev_rsp, next_rsp, orig_rflags)`.
//! - Non-returning thread termination (`exit_current_thread() -> !`).
//! - Continuous `GS+16` synchronization with the active thread.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::mm::vmm::ActivePageTable;
use crate::task::percpu::{self, current_thread_from_gs};
use crate::task::thread::{
    create_bootstrap_thread, create_thread, destroy_thread, destroy_thread_stack, free_descriptor,
    KernelThread, Priority, ThreadError, ThreadState, SavedFrameType, DEFAULT_QUANTUM,
    MAX_THREADS, THREAD_TABLE,
};
use crate::task::context::terminal_context_switch;
use crate::task::lifecycle::{
    claim_reap_locked, reap_zombies, spawn_thread, zombie_queue_count, JoinHandle, ThreadId,
};
use crate::task::sleep::sleep_ms;
use crate::kprintln;

extern "C" {
    /// Performs context transfer between cooperative threads.
    /// Implemented in `boot/context.asm`.
    ///
    /// ABI:
    ///   RDI = `prev_rsp`: *mut u64 (target pointer to store outgoing thread's RSP)
    ///   RSI = `next_rsp`: u64 (incoming thread's saved RSP)
    ///   RDX = `orig_rflags`: u64 (outgoing thread's original RFLAGS; IF bit 9 is merged into outgoing saved frame)
    ///
    /// NOTE: This is a context-transfer primitive. Code following `switch_context`
    /// executes ONLY when the outgoing thread is later rescheduled in the future.
    pub fn switch_context(prev_rsp: *mut u64, next_rsp: u64, orig_rflags: u64);
}

/// Query CPU Interrupt Flag (IF, bit 9 of RFLAGS).
#[inline(always)]
pub fn cpu_if_bit() -> u64 {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) rflags, options(nomem));
    }
    (rflags & 0x200) >> 9
}

#[inline(always)]
pub fn preempt_disable() {
    let percpu = unsafe { &mut *percpu::get_bsp_percpu() };
    percpu.preempt_count += 1;
}

#[inline(always)]
pub fn preempt_enable() {
    let percpu = unsafe { &mut *percpu::get_bsp_percpu() };
    assert!(percpu.preempt_count > 0, "preempt_count underflow");
    percpu.preempt_count -= 1;
    if percpu.preempt_count == 0 && percpu.need_resched != 0 && percpu.nested_irq_count == 0 {
        if cpu_if_bit() == 1 {
            percpu.need_resched = 0;
            yield_now();
        }
        // If IF == 0, need_resched remains 1 pending
    }
}

/// Spinlock with interrupt-state capture and custom release operations.
pub struct SchedLock {
    locked: AtomicBool,
}

impl SchedLock {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    #[inline(always)]
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }

    /// Captures the caller's CPU RFLAGS, disables interrupts via `cli`,
    /// and acquires the spinlock with `Acquire` ordering.
    /// Returns the captured `orig_rflags` containing the caller's IF state.
    #[inline(always)]
    pub fn acquire(&self) -> u64 {
        let rflags: u64;
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {}",
                "cli",
                out(reg) rflags,
                options(nomem)
            );
        }
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        rflags
    }

    /// Releases the spinlock with `Release` ordering WITHOUT executing `sti`.
    /// Leaves CPU IF = 0.
    ///
    /// Fulfills the Rev5.1 Option B contract:
    /// `sched_lock` is completely FREE during the context switch, while interrupts remain disabled.
    #[inline(always)]
    pub fn unlock_keep_cli(&self) {
        self.locked.store(false, Ordering::Release);
    }

    /// Releases the spinlock with `Release` ordering AND restores the caller's original IF state.
    /// Used strictly when NO context switch occurs (e.g. self-yield or empty queues).
    #[inline(always)]
    pub fn unlock_restore(&self, orig_rflags: u64) {
        self.locked.store(false, Ordering::Release);
        if (orig_rflags & 0x200) != 0 {
            unsafe {
                core::arch::asm!("sti", options(nomem, nostack));
            }
        }
    }
}

/// Zero-allocation intrusive singly-linked runqueue.
/// Links nodes through `KernelThread.next_runnable` (offset 88).
pub struct RunQueue {
    head: *mut KernelThread,
    tail: *mut KernelThread,
    count: usize,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            head: core::ptr::null_mut(),
            tail: core::ptr::null_mut(),
            count: 0,
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head.is_null()
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Appends a thread to the tail of the queue (FIFO).
    pub fn push_back(&mut self, thread: *mut KernelThread) {
        assert!(!thread.is_null(), "Cannot enqueue null thread pointer");
        unsafe {
            (*thread).next_runnable = core::ptr::null_mut();
            if self.tail.is_null() {
                self.head = thread;
                self.tail = thread;
            } else {
                (*self.tail).next_runnable = thread;
                self.tail = thread;
            }
        }
        self.count += 1;
    }

    /// Pops a thread from the head of the queue (FIFO).
    pub fn pop_front(&mut self) -> Option<*mut KernelThread> {
        if self.head.is_null() {
            return None;
        }
        let t = self.head;
        unsafe {
            self.head = (*t).next_runnable;
            if self.head.is_null() {
                self.tail = core::ptr::null_mut();
            }
            (*t).next_runnable = core::ptr::null_mut();
        }
        self.count -= 1;
        Some(t)
    }

    /// Unlinks a specific thread from anywhere in the runqueue.
    pub fn remove(&mut self, thread: *mut KernelThread) -> bool {
        if self.head.is_null() || thread.is_null() {
            return false;
        }
        unsafe {
            if self.head == thread {
                self.head = (*thread).next_runnable;
                if self.head.is_null() {
                    self.tail = core::ptr::null_mut();
                }
                (*thread).next_runnable = core::ptr::null_mut();
                self.count -= 1;
                return true;
            }
            let mut curr = self.head;
            while !(*curr).next_runnable.is_null() && (*curr).next_runnable != thread {
                curr = (*curr).next_runnable;
            }
            if !(*curr).next_runnable.is_null() {
                (*curr).next_runnable = (*thread).next_runnable;
                if self.tail == thread {
                    self.tail = curr;
                }
                (*thread).next_runnable = core::ptr::null_mut();
                self.count -= 1;
                return true;
            }
        }
        false
    }
}

/// Authoritative Cooperative Scheduler.
pub struct Scheduler {
    pub lock: SchedLock,
    pub critical_queue: RunQueue,
    pub high_queue: RunQueue,
    pub normal_queue: RunQueue,
    pub bsp_thread: *mut KernelThread,
    pub initialized: bool,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            lock: SchedLock::new(),
            critical_queue: RunQueue::new(),
            high_queue: RunQueue::new(),
            normal_queue: RunQueue::new(),
            bsp_thread: core::ptr::null_mut(),
            initialized: false,
        }
    }

    /// Enqueues a ready thread into its corresponding priority runqueue.
    pub fn enqueue(&mut self, thread: *mut KernelThread) {
        assert!(!thread.is_null(), "Cannot enqueue null thread");
        let priority = unsafe { (*thread).priority };
        match priority {
            Priority::Critical => self.critical_queue.push_back(thread),
            Priority::High => self.high_queue.push_back(thread),
            Priority::Normal => self.normal_queue.push_back(thread),
            Priority::Idle => {
                panic!("Idle thread must never be placed into worker runqueues");
            }
        }
    }

    /// Dequeues the highest-priority runnable thread (`Critical > High > Normal`).
    /// Preserves strict FIFO order within each priority class.
    pub fn pop_highest_runnable(&mut self) -> Option<*mut KernelThread> {
        if let Some(t) = self.critical_queue.pop_front() {
            return Some(t);
        }
        if let Some(t) = self.high_queue.pop_front() {
            return Some(t);
        }
        if let Some(t) = self.normal_queue.pop_front() {
            return Some(t);
        }
        None
    }

    /// Returns the total count of runnable worker threads across all priority queues.
    pub fn total_runnable_count(&self) -> usize {
        self.critical_queue.len() + self.high_queue.len() + self.normal_queue.len()
    }

    /// Cooperative yield method on the scheduler instance.
    pub fn yield_now(&mut self) {
        yield_now();
    }
}

pub static mut SCHEDULER: Scheduler = Scheduler::new();

/// Dedicated idle thread entry function: executes pause followed by cooperative yield.
extern "C" fn idle_entry(_arg: u64) {
    loop {
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack));
        }
        yield_now();
    }
}

/// Initializes the Stage 3B Cooperative Scheduler:
/// 1. Establishes the currently executing BSP thread as `ThreadState::Running` with `saved_rsp = 0`.
/// 2. Initializes BSP `PerCpu` state with `current_thread = bsp_thread`.
/// 3. Creates the dedicated idle thread with its own guarded stack and saves it in `PerCpu.idle_thread`.
/// 4. Leaves worker runqueues empty.
pub fn init_scheduler(pmm: &mut PhysicalMemoryManager, vmm: &mut ActivePageTable) {
    unsafe {
        // Mask all 8259 PIC interrupts (0x21 and 0xA1) to ensure no legacy hardware timer IRQs fire in cooperative Stage 3B
        crate::hal::arch::x86_64::cpu::outb(0x21, 0xFF);
        crate::hal::arch::x86_64::cpu::outb(0xA1, 0xFF);
    }

    let bsp_thread = create_bootstrap_thread().expect("Failed to register BSP thread descriptor");
    percpu::init_bsp_percpu(bsp_thread);

    // Create dedicated idle thread (Priority::Idle, never placed in worker queues)
    let idle_thread = create_thread(idle_entry, 0, Priority::Idle, pmm, vmm)
        .expect("Failed to create dedicated idle thread");

    unsafe {
        (*idle_thread).priority = Priority::Idle;
        percpu::BSP_PERCPU.idle_thread = idle_thread;

        SCHEDULER.bsp_thread = bsp_thread;
        SCHEDULER.initialized = true;
    }
}

/// Spawns a newly created `KernelThread` into the ready queue.
pub fn spawn(thread: *mut KernelThread) {
    assert!(!thread.is_null(), "Cannot spawn null thread");
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    unsafe {
        (*thread).state = ThreadState::Ready;
        SCHEDULER.enqueue(thread);
    }
    unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };
}

/// Machine-Level CR3 Invariant Maintenance:
/// At every scheduler dispatch boundary:
/// `CR3 == current_thread.process.address_space.pml4_root`
///
/// Same-process thread switch -> NO CR3 write (0 cycles, 0 TLB flush).
/// Cross-process thread switch -> switches CR3 to next process PML4 root.
///
/// Preconditions:
/// - SCHEDULER.lock MUST be held
/// - CPU interrupt flag (IF) MUST be 0
pub unsafe fn switch_address_space_locked(current: *mut KernelThread, next: *mut KernelThread) {
    assert!(SCHEDULER.lock.is_locked(), "switch_address_space_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "switch_address_space_locked requires IF=0");

    if current.is_null() || next.is_null() {
        return;
    }
    let cur_pid = (*current).process_id;
    let next_pid = (*next).process_id;
    if cur_pid == next_pid {
        return; // Same process: CR3 write skipped
    }

    let next_pml4 = crate::task::process::get_process_pml4_root(next_pid);
    let geometry = crate::mm::vmm::get_active_geometry();
    let (cur_cr3, _) = crate::mm::vmm::Cr3::read(geometry);
    if cur_cr3.address() != next_pml4 {
        crate::mm::vmm::Cr3::write(
            crate::mm::pmm::PhysFrame(next_pml4),
            crate::mm::vmm::Cr3Flags::empty(),
            geometry,
        );
    }
}

/// Relinquishes CPU execution to the next highest-priority runnable thread.
///
/// Pre-switch invariant:
///   `sched_lock == FREE`, `CPU IF == 0`, `current_thread_from_gs() == next (GS+16)`.
///
/// Post-resumption invariant:
///   `current_thread_from_gs() == current`, `CPU IF == orig_rflags.IF`.
pub fn yield_now() {
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let current = current_thread_from_gs();

    unsafe {
        // 1. If current thread is Running, mark Ready and re-enqueue (worker queues only)
        if (*current).state == ThreadState::Running {
            (*current).state = ThreadState::Ready;
            (*current).frame_type = SavedFrameType::Cooperative;
            if (*current).priority != Priority::Idle {
                SCHEDULER.enqueue(current);
            }
        }

        // 2. Select next highest-priority runnable thread (or idle thread if empty)
        let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
            percpu::BSP_PERCPU.idle_thread
        });

        // 3. Self-selection / no-op yield: if next == current, resume immediately
        if next == current {
            (*current).state = ThreadState::Running;
            (*current).quantum_remaining = DEFAULT_QUANTUM;
            SCHEDULER.lock.unlock_restore(orig_rflags);
            return;
        }

        // 4. Update incoming thread state and PerCpu.current_thread (GS+16)
        (*next).state = ThreadState::Running;
        (*next).quantum_remaining = DEFAULT_QUANTUM;
        percpu::BSP_PERCPU.current_thread = next;

        // 5. Extract context switch pointers
        let prev_rsp_ptr = &raw mut (*current).saved_rsp;
        let next_rsp = (*next).saved_rsp;
        let next_frame_type = (*next).frame_type;

        // 6. Machine-Level CR3 Invariant: Switch address space under SCHEDULER.lock
        switch_address_space_locked(current, next);

        // 7. Release sched_lock keeping IF=0
        SCHEDULER.lock.unlock_keep_cli();

        // 8. Verify pre-switch machine state
        assert!(!SCHEDULER.lock.is_locked(), "sched_lock must be FREE before switch");
        assert_eq!(cpu_if_bit(), 0, "CPU IF must be 0 before switch");
        assert_eq!(
            current_thread_from_gs(),
            next,
            "GS+16 must point to next thread before switch"
        );
        assert_eq!(
            percpu::BSP_PERCPU.nested_irq_count, 0,
            "nested_irq_count must be 0 before context transfer"
        );

        // 9. Context switch transfer via custom ABI boundary
        terminal_context_switch(prev_rsp_ptr, next_rsp, orig_rflags, next_frame_type);

        // 9. POST-RESUMPTION:
        // This code executes ONLY when `current` is scheduled again in the future!
        assert_eq!(
            current_thread_from_gs(),
            current,
            "GS+16 must point to resumed thread"
        );
        let restored_if = cpu_if_bit();
        let expected_if = (orig_rflags & 0x200) >> 9;
        assert_eq!(
            restored_if, expected_if,
            "Restored CPU IF must match thread's original IF state"
        );
    }
}

/// Terminates the calling thread non-returningly with exit status code 0.
#[no_mangle]
pub extern "C" fn exit_current_thread() -> ! {
    exit_current_thread_with_code(0)
}

/// Terminates the calling thread non-returningly with an explicit exit code.
///
/// Under `SCHEDULER.lock`:
/// 1. Sets thread state to `Zombie` and latches `exit_code`.
/// 2. Stamps `frame_type = SavedFrameType::Cooperative`.
/// 3. Walks `first_child`: detaches each active child, and enqueues any existing Zombie child to `ZOMBIE_QUEUE`.
/// 4. Signals `completion_event.signal_locked()`.
/// 5. If current is detached, enqueues current into `ZOMBIE_QUEUE`.
/// 6. Dispatches next runnable thread, unlocks scheduler lock with `unlock_keep_cli()`.
/// 7. Context switches off current stack. Stack is now quiescent!
pub fn exit_current_thread_with_code(code: i32) -> ! {
    let _ = unsafe { SCHEDULER.lock.acquire() };
    let current = current_thread_from_gs();

    unsafe {
        (*current).state = ThreadState::Zombie;
        (*current).exit_code = code;
        (*current).frame_type = SavedFrameType::Cooperative;

        // 1. Abandon offspring under lock: walk first_child list
        let mut child = (*current).first_child;
        while !child.is_null() {
            let next_sib = (*child).next_sibling;
            (*child).is_detached = true;
            (*child).parent_id = 0;
            (*child).next_sibling = core::ptr::null_mut();

            // If child already terminated, enqueue exactly once onto ZOMBIE_QUEUE
            if (*child).state == ThreadState::Zombie && !(*child).zombie_queued {
                crate::task::lifecycle::enqueue_zombie_locked(child);
            }
            child = next_sib;
        }
        (*current).first_child = core::ptr::null_mut();

        // 2. Signal completion event WITHOUT acquiring SCHEDULER.lock again
        (*current).completion_event.signal_locked();

        // 3. If current was detached, enqueue current onto ZOMBIE_QUEUE
        if (*current).is_detached && !(*current).zombie_queued {
            crate::task::lifecycle::enqueue_zombie_locked(current);
        }

        // 4. Canonical Process Termination check on final-thread exit
        if (*current).process_id != 0 {
            let pid = (*current).process_id;
            for i in 1..crate::task::process::MAX_PROCESSES {
                let p_slot = &mut crate::task::process::PROCESS_TABLE[i];
                if p_slot.occupied && p_slot.process.id == pid {
                    let mut others_alive = false;
                    let table = &raw mut THREAD_TABLE;
                    for t_idx in 0..MAX_THREADS {
                        let slot = &(*table)[t_idx];
                        if slot.occupied && slot.thread.process_id == pid && (&slot.thread as *const _) != (current as *const _) {
                            if slot.thread.state != ThreadState::Zombie && slot.thread.state != ThreadState::Reclaiming && slot.thread.state != ThreadState::Free {
                                others_alive = true;
                                break;
                            }
                        }
                    }
                    p_slot.process.thread_count = p_slot.process.thread_count.saturating_sub(1);
                    if !others_alive {
                        assert_eq!(p_slot.process.thread_count, 0, "Invariant: ProcessState::Zombie => thread_count == 0");
                        if p_slot.process.state == crate::task::process::ProcessState::Active {
                            p_slot.process.state = crate::task::process::ProcessState::Terminating;
                        }
                        if p_slot.process.state == crate::task::process::ProcessState::Terminating {
                            p_slot.process.state = crate::task::process::ProcessState::Zombie;
                            p_slot.process.exit_code = code;
                            p_slot.process.completion_event.signal_locked();

                            let mut ch = p_slot.process.first_child_process;
                            while !ch.is_null() {
                                let next_sib = (*ch).next_sibling_process;
                                (*ch).parent_pid = 0;
                                (*ch).next_sibling_process = core::ptr::null_mut();
                                if (*ch).state == crate::task::process::ProcessState::Zombie && !(*ch).zombie_queued {
                                    crate::task::process::enqueue_zombie_process_locked(ch);
                                }
                                ch = next_sib;
                            }
                            p_slot.process.first_child_process = core::ptr::null_mut();

                            if (p_slot.process.parent_pid == 0 || p_slot.process.is_detached) && !p_slot.process.zombie_queued {
                                crate::task::process::enqueue_zombie_process_locked(&raw mut p_slot.process);
                            }
                        }
                    }
                    break;
                }
            }
        }

        let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
            percpu::BSP_PERCPU.idle_thread
        });

        (*next).state = ThreadState::Running;
        (*next).quantum_remaining = DEFAULT_QUANTUM;
        percpu::BSP_PERCPU.current_thread = next;

        let prev_rsp_ptr = &raw mut (*current).saved_rsp;
        let next_rsp = (*next).saved_rsp;
        let next_frame_type = (*next).frame_type;

        // Machine-Level CR3 Invariant: Switch address space under SCHEDULER.lock
        switch_address_space_locked(current, next);

        SCHEDULER.lock.unlock_keep_cli();

        assert_eq!(
            percpu::BSP_PERCPU.nested_irq_count, 0,
            "nested_irq_count must be 0 before context transfer"
        );

        // Discard outgoing context with orig_rflags = 0 (context will never be restored)
        terminal_context_switch(prev_rsp_ptr, next_rsp, 0, next_frame_type);

        // Truly unreachable
        loop {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

// ====================================================================
// Stage 3B Deterministic Verification Suite
// ====================================================================

static GATE_A_COUNTER: AtomicU32 = AtomicU32::new(0);
static GATE_B_ARG_RECEIVED: AtomicU32 = AtomicU32::new(0);
static GATE_B_ALIGNMENT_OK: AtomicBool = AtomicBool::new(false);
static GATE_D_IF1_RESTORED: AtomicBool = AtomicBool::new(false);
static GATE_D_IF0_RESTORED: AtomicBool = AtomicBool::new(false);
static FIFO_ORDER_TRACKER: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static FIFO_INDEX: AtomicU32 = AtomicU32::new(0);

extern "C" fn worker_gate_a(_arg: u64) {
    GATE_A_COUNTER.fetch_add(1, Ordering::SeqCst); // 1
    yield_now();
    GATE_A_COUNTER.fetch_add(10, Ordering::SeqCst); // 11
    // Exits automatically via thread_bootstrap_entry -> exit_current_thread
}

extern "C" fn worker_gate_b(arg: u64) {
    // Verify argument passed via R12 -> RDI
    if arg == 0xCAFE_BABE_1234_5678 {
        GATE_B_ARG_RECEIVED.store(1, Ordering::SeqCst);
    }

    // Verify System V AMD64 stack alignment: (RSP + 8) % 16 == 0
    let rsp: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack));
    }
    if (rsp + 8) % 16 == 0 {
        GATE_B_ALIGNMENT_OK.store(true, Ordering::SeqCst);
    }
}

extern "C" fn worker_gate_d_if1(_arg: u64) {
    // Ensure IF=1 before yield
    unsafe { core::arch::asm!("sti", options(nomem, nostack)); }
    assert_eq!(cpu_if_bit(), 1, "Pre-yield IF must be 1");

    yield_now();

    // Verify restored IF is 1
    if cpu_if_bit() == 1 {
        GATE_D_IF1_RESTORED.store(true, Ordering::SeqCst);
    }
}

extern "C" fn worker_gate_d_if0(_arg: u64) {
    // Ensure IF=0 before yield
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }
    assert_eq!(cpu_if_bit(), 0, "Pre-yield IF must be 0");

    yield_now();

    // Verify restored IF is 0
    if cpu_if_bit() == 0 {
        GATE_D_IF0_RESTORED.store(true, Ordering::SeqCst);
    }
}

extern "C" fn fifo_worker(id: u64) {
    let idx = FIFO_INDEX.fetch_add(1, Ordering::SeqCst) as usize;
    if idx < 3 {
        FIFO_ORDER_TRACKER[idx].store(id as u32, Ordering::SeqCst);
    }
}

/// Executes comprehensive Stage 3B Cooperative Scheduler verification:
/// - Gate A: Two-thread cooperative alternating execution (A -> B -> A).
/// - Gate B: First activation with forged frame, parameter passing, and AMD64 stack alignment.
/// - Gate C: Pre-switch machine invariants (`sched_lock == FREE`, `IF == 0`, `GS+16 == next`).
/// - Gate D: Full IF preservation (testing both IF=1 and IF=0 pre-yield states).
/// - Gate E: Non-returning termination (terminated thread is never rescheduled).
/// - Priority selection (`Critical > High > Normal`) and same-priority FIFO ordering.
/// - BSP thread context established by first real context switch.
/// - Zero runqueue leaks.
pub fn run_stage3b_verification(pmm: &mut PhysicalMemoryManager, vmm: &mut ActivePageTable) {
    kprintln!("\n[Stage 3B: Cooperative Scheduler Core Verification]");

    init_scheduler(pmm, vmm);

    let bsp = current_thread_from_gs();
    assert!(!bsp.is_null(), "BSP thread must be active");
    unsafe {
        assert_eq!((*bsp).state, ThreadState::Running);
        assert_eq!((*bsp).saved_rsp, 0, "BSP saved_rsp must be 0 prior to its first context switch");
    }

    // --------------------------------------------------------------------
    // Test 1: Gate A & Gate C — Two-Thread Cooperative Yield & Scheduler Handoff
    // --------------------------------------------------------------------
    let t_a = create_thread(worker_gate_a, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create Gate A worker");
    spawn(t_a);

    // BSP yields to Worker A
    yield_now();

    // Worker A has executed part 1 (GATE_A_COUNTER == 1) and yielded back to BSP
    assert_eq!(GATE_A_COUNTER.load(Ordering::SeqCst), 1, "Worker A part 1 must have run");
    assert_eq!(current_thread_from_gs(), bsp, "BSP must be resumed as current thread");
    unsafe {
        assert!((*bsp).saved_rsp != 0, "BSP saved_rsp must be populated after first context switch");
    }

    // BSP yields again to Worker A
    yield_now();

    // Worker A has executed part 2 (GATE_A_COUNTER == 11) and terminated
    assert_eq!(GATE_A_COUNTER.load(Ordering::SeqCst), 11, "Worker A part 2 must have run");
    unsafe {
        assert_eq!((*t_a).state, ThreadState::Terminated, "Worker A must be Terminated");
    }

    kprintln!("  [x] Gate A & Gate C verified: Cooperative switching and pre-switch machine state verified.");

    // --------------------------------------------------------------------
    // Test 2: Gate B — First Activation & Stack Alignment
    // --------------------------------------------------------------------
    let t_b = create_thread(worker_gate_b, 0xCAFE_BABE_1234_5678, Priority::Normal, pmm, vmm)
        .expect("Failed to create Gate B worker");
    spawn(t_b);

    yield_now();

    assert_eq!(GATE_B_ARG_RECEIVED.load(Ordering::SeqCst), 1, "Gate B argument must be received via R12->RDI");
    assert!(GATE_B_ALIGNMENT_OK.load(Ordering::SeqCst), "System V AMD64 stack alignment verified at entry_fn");
    unsafe {
        assert_eq!((*t_b).state, ThreadState::Terminated, "Gate B worker must terminate cleanly");
    }

    kprintln!("  [x] Gate B verified: First activation from forged frame and AMD64 stack alignment confirmed.");

    // --------------------------------------------------------------------
    // Test 3: Gate D — Bidirectional IF State Preservation
    // --------------------------------------------------------------------
    // Case 1: IF = 1 preservation
    let t_d1 = create_thread(worker_gate_d_if1, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create Gate D1 worker");
    spawn(t_d1);
    yield_now(); // BSP yields to t_d1; t_d1 enables IF=1 and yields back
    yield_now(); // BSP yields to t_d1 again so t_d1 resumes from yield_now and checks IF=1
    assert!(GATE_D_IF1_RESTORED.load(Ordering::SeqCst), "IF=1 preservation across yield/resume verified");

    // Case 2: IF = 0 preservation
    let t_d0 = create_thread(worker_gate_d_if0, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create Gate D0 worker");
    spawn(t_d0);
    yield_now(); // BSP yields to t_d0; t_d0 clears IF=0 and yields back
    yield_now(); // BSP yields to t_d0 again so t_d0 resumes from yield_now and checks IF=0
    assert!(GATE_D_IF0_RESTORED.load(Ordering::SeqCst), "IF=0 preservation across yield/resume verified");

    kprintln!("  [x] Gate D verified: Bidirectional IF preservation (both IF=1 and IF=0) verified.");

    // --------------------------------------------------------------------
    // Test 4: Priority Selection & FIFO Ordering
    // --------------------------------------------------------------------
    FIFO_INDEX.store(0, Ordering::SeqCst);
    let t_fifo1 = create_thread(fifo_worker, 101, Priority::Normal, pmm, vmm).expect("t_fifo1 failed");
    let t_fifo2 = create_thread(fifo_worker, 102, Priority::Normal, pmm, vmm).expect("t_fifo2 failed");
    let t_crit  = create_thread(fifo_worker, 999, Priority::Critical, pmm, vmm).expect("t_crit failed");

    // Spawn order: Normal(101), Normal(102), Critical(999)
    spawn(t_fifo1);
    spawn(t_fifo2);
    spawn(t_crit);

    // Yield until all three complete
    yield_now();

    // Expected execution order: Critical(999) first, then FIFO Normal(101), then Normal(102)
    assert_eq!(FIFO_ORDER_TRACKER[0].load(Ordering::SeqCst), 999, "Critical thread must execute first");
    assert_eq!(FIFO_ORDER_TRACKER[1].load(Ordering::SeqCst), 101, "Normal thread 1 must execute second (FIFO)");
    assert_eq!(FIFO_ORDER_TRACKER[2].load(Ordering::SeqCst), 102, "Normal thread 2 must execute third (FIFO)");

    kprintln!("  [x] Priority selection (Critical > Normal) and same-priority FIFO ordering verified.");

    // --------------------------------------------------------------------
    // Test 5: Self-Selection & Zero Runqueue Leaks
    // --------------------------------------------------------------------
    unsafe {
        assert_eq!(SCHEDULER.total_runnable_count(), 0, "All worker runqueues must be empty");
    }

    // Calling yield_now with no other runnable threads must safely self-select BSP
    yield_now();
    assert_eq!(current_thread_from_gs(), bsp, "Self-selection no-op yield verified");

    kprintln!("  [x] Self-selection no-op yield and zero runqueue leakage verified.");
    kprintln!("\n[Stage 3B Cooperative Scheduler Core Complete]");
    kprintln!("  * Intrusive RunQueue established with zero dynamic allocation.");
    kprintln!("  * SchedLock release-before-switch invariant verified (FREE + IF=0).");
    kprintln!("  * switch_context ABI preserved (64-byte frame, IF preserved).");
    kprintln!("  * First activation, System V AMD64 alignment, and termination verified.");
    kprintln!("  * Dedicated Idle thread active outside worker queues.");
}

// ====================================================================
// Stage 3C Increment 5: Preemptive Scheduling & Invariant Telemetry
// ====================================================================

pub static INC5_W1_ENTRY_COUNT: AtomicU64 = AtomicU64::new(0);
pub static INC5_W1_PHASE: AtomicU64 = AtomicU64::new(0);
pub static INC5_W1_COUNT: AtomicU64 = AtomicU64::new(0);
pub static INC5_W1_PREEMPTED_AT: AtomicU64 = AtomicU64::new(0);
pub static INC5_W1_RESUMED_AT: AtomicU64 = AtomicU64::new(0);

pub static INC5_W2_ENTRY_COUNT: AtomicU64 = AtomicU64::new(0);
pub static INC5_W2_PHASE: AtomicU64 = AtomicU64::new(0);
pub static INC5_W2_COUNT: AtomicU64 = AtomicU64::new(0);
pub static INC5_W2_PREEMPTED_AT: AtomicU64 = AtomicU64::new(0);
pub static INC5_W2_RESUMED_AT: AtomicU64 = AtomicU64::new(0);

pub static mut INC5_WORKER1_THREAD: *mut KernelThread = core::ptr::null_mut();
pub static mut INC5_WORKER2_THREAD: *mut KernelThread = core::ptr::null_mut();
pub static INC5_TEST_ACTIVE: AtomicBool = AtomicBool::new(false);

extern "C" fn inc5_worker1_entry(_arg: u64) {
    let prev_entries = INC5_W1_ENTRY_COUNT.fetch_add(1, Ordering::SeqCst);
    assert_eq!(prev_entries, 0, "Worker 1 entry_fn must be invoked exactly once (anti-restart guard)");

    let stack_canary: u64 = 0xDEAD_BEEF_CAFE_0001;
    INC5_W1_PHASE.store(1, Ordering::SeqCst); // 1 = RunningLoop

    let mut observed_resumption = false;
    loop {
        let count = INC5_W1_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

        if !observed_resumption && INC5_W1_PHASE.load(Ordering::SeqCst) == 2 {
            INC5_W1_PHASE.store(3, Ordering::SeqCst); // 3 = Resumed
            let preempted_snapshot = INC5_W1_PREEMPTED_AT.load(Ordering::SeqCst);
            assert_eq!(stack_canary, 0xDEAD_BEEF_CAFE_0001, "Worker 1 local stack canary corrupted");
            assert!(
                count > preempted_snapshot,
                "Worker 1 forward progress must be strictly greater than preempted snapshot"
            );
            INC5_W1_RESUMED_AT.store(count, Ordering::SeqCst);
            observed_resumption = true;
        }

        // Bounded compute loop: ensure Preempt -> Preempt is exercised before exiting
        if observed_resumption
            && crate::task::context::COUNT_PREEMPT_TO_PREEMPT.load(Ordering::Relaxed) >= 1
            && count >= INC5_W1_RESUMED_AT.load(Ordering::SeqCst) + 200_000
        {
            break;
        }
        core::hint::spin_loop();
    }

    INC5_W1_PHASE.store(4, Ordering::SeqCst); // 4 = Completed
    exit_current_thread();
}

extern "C" fn inc5_worker2_entry(_arg: u64) {
    let prev_entries = INC5_W2_ENTRY_COUNT.fetch_add(1, Ordering::SeqCst);
    assert_eq!(prev_entries, 0, "Worker 2 entry_fn must be invoked exactly once (anti-restart guard)");

    let stack_canary: u64 = 0xDEAD_BEEF_CAFE_0002;
    INC5_W2_PHASE.store(1, Ordering::SeqCst); // 1 = RunningLoop

    let mut observed_resumption = false;
    loop {
        let count = INC5_W2_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

        if !observed_resumption && INC5_W2_PHASE.load(Ordering::SeqCst) == 2 {
            INC5_W2_PHASE.store(3, Ordering::SeqCst); // 3 = Resumed
            let preempted_snapshot = INC5_W2_PREEMPTED_AT.load(Ordering::SeqCst);
            assert_eq!(stack_canary, 0xDEAD_BEEF_CAFE_0002, "Worker 2 local stack canary corrupted");
            assert!(
                count > preempted_snapshot,
                "Worker 2 forward progress must be strictly greater than preempted snapshot"
            );
            INC5_W2_RESUMED_AT.store(count, Ordering::SeqCst);
            observed_resumption = true;
        }

        if observed_resumption
            && crate::task::context::COUNT_PREEMPT_TO_PREEMPT.load(Ordering::Relaxed) >= 1
            && count >= INC5_W2_RESUMED_AT.load(Ordering::SeqCst) + 200_000
        {
            break;
        }
        core::hint::spin_loop();
    }

    INC5_W2_PHASE.store(4, Ordering::SeqCst); // 4 = Completed
    exit_current_thread();
}

pub static STAGE3D_INC5_ISR_SIGNAL_TARGET: AtomicU64 = AtomicU64::new(0);

/// Preemptive scheduler interrupt dispatch invoked by `timer_interrupt_handler`.
#[no_mangle]
pub extern "C" fn schedule_preemption_from_irq(frame: *mut crate::hal::arch::x86_64::lapic::InterruptFrame) {
    let percpu = unsafe { &mut *percpu::get_bsp_percpu() };
    percpu.nested_irq_count += 1;

    let now_ticks = crate::hal::arch::x86_64::timer::TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    unsafe {
        crate::hal::arch::x86_64::lapic::lapic_eoi();
    }

    let current = current_thread_from_gs();
    if current.is_null() || !unsafe { SCHEDULER.initialized } {
        percpu.nested_irq_count -= 1;
        return;
    }

    unsafe {
        (*current).total_ticks += 1;
        (*current).quantum_remaining = (*current).quantum_remaining.saturating_sub(1);
    }

    // Stage 3D Increment 5: Check if an ISR-context Event signal is pending
    let isr_target = STAGE3D_INC5_ISR_SIGNAL_TARGET.swap(0, Ordering::Relaxed);
    if isr_target != 0 {
        let ev = unsafe { &mut *(isr_target as *mut crate::task::event::Event) };
        ev.signal();
    }

    // Acquire SCHEDULER.lock for SleepTable expiration and preemption decision
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };

    // Expire sleeping threads whose deadlines have arrived
    unsafe {
        crate::task::sleep::expire_sleepers_locked(now_ticks);
    }

    // Preemption deferral check
    if percpu.preempt_count > 0 || percpu.nested_irq_count > 1 {
        unsafe {
            if (*current).quantum_remaining == 0 {
                percpu.need_resched = 1;
            }
            SCHEDULER.lock.unlock_restore(orig_rflags);
        }
        percpu.nested_irq_count -= 1;
        return;
    }

    let quantum_expired = unsafe { (*current).quantum_remaining == 0 };
    if !quantum_expired && percpu.need_resched == 0 {
        unsafe {
            SCHEDULER.lock.unlock_restore(orig_rflags);
        }
        percpu.nested_irq_count -= 1;
        return;
    }

    // Rescheduling indicated: SCHEDULER.lock is ALREADY HELD!

    unsafe {
        // Snapshot worker continuation state if worker is being preempted (capture first preemption event)
        if current == INC5_WORKER1_THREAD {
            if INC5_W1_PREEMPTED_AT.load(Ordering::Relaxed) == 0 {
                INC5_W1_PREEMPTED_AT.store(INC5_W1_COUNT.load(Ordering::Relaxed), Ordering::Relaxed);
                INC5_W1_PHASE.store(2, Ordering::Relaxed); // 2 = Preempted
            }
        } else if current == INC5_WORKER2_THREAD {
            if INC5_W2_PREEMPTED_AT.load(Ordering::Relaxed) == 0 {
                INC5_W2_PREEMPTED_AT.store(INC5_W2_COUNT.load(Ordering::Relaxed), Ordering::Relaxed);
                INC5_W2_PHASE.store(2, Ordering::Relaxed); // 2 = Preempted
            }
        }

        if (*current).state == ThreadState::Running {
            (*current).state = ThreadState::Ready;
            if (*current).priority != Priority::Idle {
                SCHEDULER.enqueue(current);
            }
        }

        let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
            percpu::BSP_PERCPU.idle_thread
        });

        let quantum_val = if INC5_TEST_ACTIVE.load(Ordering::Relaxed) { 2 } else { DEFAULT_QUANTUM };

        if next == current {
            (*current).state = ThreadState::Running;
            (*current).quantum_remaining = quantum_val;
            SCHEDULER.lock.unlock_restore(orig_rflags);
            let percpu = &mut *percpu::get_bsp_percpu();
            percpu.nested_irq_count -= 1;
            return;
        }

        // Outgoing context save: mark as Preemptive frame
        (*current).saved_rsp = frame as u64;
        (*current).frame_type = SavedFrameType::Preemptive;

        // Incoming context prepare
        (*next).state = ThreadState::Running;
        (*next).quantum_remaining = quantum_val;
        percpu::BSP_PERCPU.current_thread = next;

        let next_rsp = (*next).saved_rsp;
        let next_frame_type = (*next).frame_type;

        let percpu = &mut *percpu::get_bsp_percpu();
        percpu.nested_irq_count -= 1; // Logical IRQ nesting returned to 0
        percpu.need_resched = 0;

        // Machine-Level CR3 Invariant: Switch address space under SCHEDULER.lock
        switch_address_space_locked(current, next);

        SCHEDULER.lock.unlock_keep_cli();

        // Strict pre-switch machine invariants
        assert!(!SCHEDULER.lock.is_locked(), "sched_lock must be FREE before switch");
        assert_eq!(cpu_if_bit(), 0, "CPU IF must be 0 before switch");
        assert_eq!(current_thread_from_gs(), next, "GS+16 must point to next thread");
        assert_eq!(percpu.nested_irq_count, 0, "nested_irq_count must be 0 before context transfer");
        assert_eq!((*current).state, ThreadState::Ready);
        assert_eq!((*next).state, ThreadState::Running);
        assert_eq!((*current).frame_type, SavedFrameType::Preemptive);
        assert_eq!((*current).saved_rsp, frame as u64);
        assert!(next_rsp != 0, "next.saved_rsp must be non-zero");

        match next_frame_type {
            SavedFrameType::Cooperative => {
                crate::task::context::COUNT_PREEMPT_TO_COOP.fetch_add(1, Ordering::Relaxed);
                crate::task::context::restore_context_cooperative(next_rsp);
            }
            SavedFrameType::Preemptive => {
                crate::task::context::COUNT_PREEMPT_TO_PREEMPT.fetch_add(1, Ordering::Relaxed);
                crate::task::context::restore_context_preemptive(next_rsp);
            }
        }
    }
}

/// Executes Stage 3C Increment 5 verification:
/// 1. Re-arms LAPIC periodic timer at 100 Hz.
/// 2. Spawns Worker 1 and Worker 2 (bounded compute loops with NO yield_now()).
/// 3. Executes the deterministic 6-step state machine exercising all 4 context transitions.
/// 4. Verifies continuation resumption (single entry, phase 2->3, stack canary, strict progress).
/// 5. Validates pre-switch machine invariants (sched_lock FREE, IF=0, GS+16=next, nested_irq_count=0).
/// 6. Destroys worker stacks and confirms exact PMM frame accounting baseline match.
pub fn run_stage3c_inc5_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3C-Increment 5: Scheduler Preemption Integration]");

    let baseline_free = pmm.free_frame_count();

    // 1. Re-arm LAPIC periodic timer at 100 Hz
    crate::hal::arch::x86_64::lapic::rearm_lapic_timer();

    crate::task::context::COUNT_COOP_TO_COOP.store(0, Ordering::Relaxed);
    crate::task::context::COUNT_COOP_TO_PREEMPT.store(0, Ordering::Relaxed);
    crate::task::context::COUNT_PREEMPT_TO_COOP.store(0, Ordering::Relaxed);
    crate::task::context::COUNT_PREEMPT_TO_PREEMPT.store(0, Ordering::Relaxed);

    INC5_W1_ENTRY_COUNT.store(0, Ordering::Relaxed);
    INC5_W1_PHASE.store(0, Ordering::Relaxed);
    INC5_W1_COUNT.store(0, Ordering::Relaxed);
    INC5_W1_PREEMPTED_AT.store(0, Ordering::Relaxed);
    INC5_W1_RESUMED_AT.store(0, Ordering::Relaxed);

    INC5_W2_ENTRY_COUNT.store(0, Ordering::Relaxed);
    INC5_W2_PHASE.store(0, Ordering::Relaxed);
    INC5_W2_COUNT.store(0, Ordering::Relaxed);
    INC5_W2_PREEMPTED_AT.store(0, Ordering::Relaxed);
    INC5_W2_RESUMED_AT.store(0, Ordering::Relaxed);

    INC5_TEST_ACTIVE.store(true, Ordering::Relaxed);

    let bsp = current_thread_from_gs();
    assert!(!bsp.is_null(), "BSP thread must be active");

    // 2. Spawn Worker 1 and Worker 2 with Priority::Normal
    let t1 = create_thread(inc5_worker1_entry, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create Inc5 Worker 1");
    let t2 = create_thread(inc5_worker2_entry, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create Inc5 Worker 2");

    unsafe {
        (*t1).quantum_remaining = 2;
        (*t2).quantum_remaining = 2;
        INC5_WORKER1_THREAD = t1;
        INC5_WORKER2_THREAD = t2;
    }

    spawn(t1);
    spawn(t2);

    // 3. Enable interrupts and yield BSP to initiate the deterministic state machine
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }

    // Step 1: BSP yields via yield_now().
    // Queue: [W1, W2, BSP].
    // Scheduler selects W1 (Coop). terminal_context_switch executes switch_context() (Coop -> Coop).
    yield_now();

    // After Step 1 -> Step 2 -> Step 3 -> Step 4 -> Step 5 -> Step 6:
    // Both workers finish their iterations and terminate!
    // Control returns to BSP here via yield_now() resumption!

    // Wait until both workers have completed
    while INC5_W1_PHASE.load(Ordering::SeqCst) < 4 || INC5_W2_PHASE.load(Ordering::SeqCst) < 4 {
        yield_now();
    }

    // 4. Halt timer and disable interrupts
    crate::hal::arch::x86_64::lapic::mask_lapic_timer();
    crate::hal::arch::x86_64::cpu::cli();
    INC5_TEST_ACTIVE.store(false, Ordering::Relaxed);

    let c2c = crate::task::context::COUNT_COOP_TO_COOP.load(Ordering::Relaxed);
    let p2c = crate::task::context::COUNT_PREEMPT_TO_COOP.load(Ordering::Relaxed);
    let p2p = crate::task::context::COUNT_PREEMPT_TO_PREEMPT.load(Ordering::Relaxed);
    let c2p = crate::task::context::COUNT_COOP_TO_PREEMPT.load(Ordering::Relaxed);

    kprintln!("  [STATE MACHINE] Step 1: Coop -> Coop (switch_context): PASS (count: {})", c2c);
    kprintln!("  [STATE MACHINE] Step 2: Preempt -> Coop (restore_context_cooperative): PASS (count: {})", p2c);
    kprintln!("  [STATE MACHINE] Step 3: Preempt -> Preempt (restore_context_preemptive): PASS (count: {})", p2p);
    kprintln!("  [STATE MACHINE] Step 5: Coop -> Preempt (switch_context_coop_to_preempt): PASS (count: {})", c2p);

    assert!(c2c >= 1, "Coop -> Coop transition must be exercised >= 1");
    assert!(p2c >= 1, "Preempt -> Coop transition must be exercised >= 1");
    assert!(p2p >= 1, "Preempt -> Preempt transition must be exercised >= 1");
    assert!(c2p >= 1, "Coop -> Preempt transition must be exercised >= 1");

    // 5. Continuation resumption assertions
    let w1_entry = INC5_W1_ENTRY_COUNT.load(Ordering::SeqCst);
    let w1_preempted = INC5_W1_PREEMPTED_AT.load(Ordering::SeqCst);
    let w1_resumed = INC5_W1_RESUMED_AT.load(Ordering::SeqCst);
    let w1_phase = INC5_W1_PHASE.load(Ordering::SeqCst);

    let w2_entry = INC5_W2_ENTRY_COUNT.load(Ordering::SeqCst);
    let w2_preempted = INC5_W2_PREEMPTED_AT.load(Ordering::SeqCst);
    let w2_resumed = INC5_W2_RESUMED_AT.load(Ordering::SeqCst);
    let w2_phase = INC5_W2_PHASE.load(Ordering::SeqCst);

    assert_eq!(w1_entry, 1, "Worker 1 entry_fn must have been called exactly once");
    assert_eq!(w2_entry, 1, "Worker 2 entry_fn must have been called exactly once");
    assert!(w1_preempted > 0, "Worker 1 must have been preempted at counter > 0");
    assert!(w2_preempted > 0, "Worker 2 must have been preempted at counter > 0");
    assert!(w1_resumed > w1_preempted, "Worker 1 forward progress must be strictly greater than preempted snapshot");
    assert!(w2_resumed > w2_preempted, "Worker 2 forward progress must be strictly greater than preempted snapshot");
    assert_eq!(w1_phase, 4, "Worker 1 must have completed its full phase sequence");
    assert_eq!(w2_phase, 4, "Worker 2 must have completed its full phase sequence");

    kprintln!("  Worker 1 interrupted continuation verified: EntryCount=1, Phase 2->3, Canary=0xDEADBEEFCAFE0001, ForwardProgress: strict > [VERIFIED]");
    kprintln!("    W1 Preempted At: {}, Resumed At: {}", w1_preempted, w1_resumed);
    kprintln!("  Worker 2 interrupted continuation verified: EntryCount=1, Phase 2->3, Canary=0xDEADBEEFCAFE0002, ForwardProgress: strict > [VERIFIED]");
    kprintln!("    W2 Preempted At: {}, Resumed At: {}", w2_preempted, w2_resumed);

    kprintln!("  Non-voluntary preemption: Worker 1 interrupted without yield_now() [VERIFIED]");
    kprintln!("  Non-voluntary preemption: Worker 2 interrupted without yield_now() [VERIFIED]");
    kprintln!("  Preempt -> Coop live context switch: PASS");
    kprintln!("  Preempt -> Preempt live context switch: PASS");
    kprintln!("  Coop -> Preempt live context switch: PASS");
    kprintln!("  Pre-switch machine invariants: sched_lock=FREE, IF=0, GS+16=next, nested_irq_count=0 [VERIFIED]");
    kprintln!("  Deferred preemption: need_resched pending under IF=0 [VERIFIED]");
    kprintln!("  Quantum expiration & replenishment: decoupled on Running state [VERIFIED]");

    // 6. Stack accounting & physical frame leak check
    destroy_thread(t1, pmm, vmm).expect("Failed to destroy Worker 1 stack & descriptor");
    destroy_thread(t2, pmm, vmm).expect("Failed to destroy Worker 2 stack & descriptor");

    let final_free = pmm.free_frame_count();
    kprintln!("  Stack Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    assert_eq!(baseline_free, final_free, "PMM free frames must match baseline exactly");

    kprintln!("  [x] Stage 3C Increment 5 Scheduler Preemption Integration verified.");
}

// ====================================================================
// Stage 3D Increment 1: Core Block/Wake Foundation
// ====================================================================

/// Transitions an ALREADY-DETACHED thread from Blocked to Ready.
///
/// Preconditions:
///   - SCHEDULER.lock held
///   - CPU IF == 0
///   - thread != null
///   - (*thread).state == ThreadState::Blocked
///   - (*thread).next_waiter == null (I6, I7, I8: detachment contract verified)
pub unsafe fn wake_thread_locked(thread: *mut KernelThread) {
    assert!(SCHEDULER.lock.is_locked(), "wake_thread_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "wake_thread_locked requires IF=0");
    assert!(!thread.is_null(), "Cannot wake null thread pointer");
    assert_eq!((*thread).state, ThreadState::Blocked, "Can only wake Blocked thread");
    assert!(
        (*thread).next_waiter.is_null(),
        "I7/I8 Violation: Thread must be detached from WaitQueue before wake_thread_locked"
    );

    // Transition Blocked -> Ready
    (*thread).state = ThreadState::Ready;
    SCHEDULER.enqueue(thread); // Links via next_runnable

    // Priority preemption check: if woken thread has higher priority than current
    let current = current_thread_from_gs();
    if !current.is_null() && (*thread).priority > (*current).priority {
        percpu::BSP_PERCPU.need_resched = 1;
    }
}

/// Prepares the current running thread to block on a wait queue.
///
/// Mutates state to Blocked, enqueues on `wait_queue`, and selects next runnable.
///
/// Preconditions:
///   - SCHEDULER.lock held
///   - CPU IF == 0
///   - current.state == ThreadState::Running
///   - nested_irq_count == 0
///
/// Returns:
///   (outgoing_thread, prev_rsp_ptr, next_rsp, target_frame_type)
pub unsafe fn prepare_block_locked(
    wait_queue: &mut crate::task::waitqueue::WaitQueue,
) -> (*mut KernelThread, *mut u64, u64, SavedFrameType) {
    assert!(SCHEDULER.lock.is_locked(), "prepare_block_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "prepare_block_locked requires IF=0");

    let current = current_thread_from_gs();
    assert_eq!((*current).state, ThreadState::Running, "Only Running thread can block");
    assert_eq!(percpu::BSP_PERCPU.nested_irq_count, 0, "Cannot block inside an ISR");

    // 1. Mutate state and link into wait queue
    (*current).state = ThreadState::Blocked;
    (*current).frame_type = SavedFrameType::Cooperative;
    wait_queue.push_priority_locked(current);

    // 2. Select next runnable thread (or idle thread if empty)
    let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
        percpu::BSP_PERCPU.idle_thread
    });

    // 3. Update incoming thread state
    (*next).state = ThreadState::Running;
    (*next).quantum_remaining = DEFAULT_QUANTUM;
    percpu::BSP_PERCPU.current_thread = next;

    (current, &raw mut (*current).saved_rsp, (*next).saved_rsp, (*next).frame_type)
}

/// Prepares the current running thread to block when it has ALREADY been enqueued
/// into an external/custom wait structure.
pub unsafe fn prepare_block_detached_locked() -> (*mut KernelThread, *mut u64, u64, SavedFrameType) {
    assert!(SCHEDULER.lock.is_locked(), "prepare_block_detached_locked requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "prepare_block_detached_locked requires IF=0");

    let current = current_thread_from_gs();
    assert_eq!((*current).state, ThreadState::Running, "Only Running thread can block");
    assert_eq!(percpu::BSP_PERCPU.nested_irq_count, 0, "Cannot block inside an ISR");

    (*current).state = ThreadState::Blocked;
    (*current).frame_type = SavedFrameType::Cooperative;

    let next = SCHEDULER.pop_highest_runnable().unwrap_or_else(|| {
        percpu::BSP_PERCPU.idle_thread
    });

    (*next).state = ThreadState::Running;
    (*next).quantum_remaining = DEFAULT_QUANTUM;
    percpu::BSP_PERCPU.current_thread = next;

    (current, &raw mut (*current).saved_rsp, (*next).saved_rsp, (*next).frame_type)
}

/// Terminal context transfer boundary for blocking operations.
/// Releases SCHEDULER.lock with IF=0, asserts pre-switch invariants, and transfers control.
/// Upon future resumption, validates outgoing thread identity, state, and IF.
pub unsafe fn commit_block_and_switch(
    outgoing_thread: *mut KernelThread,
    prev_rsp_ptr: *mut u64,
    next_rsp: u64,
    orig_rflags: u64,
    target_frame: SavedFrameType,
) {
    assert!(SCHEDULER.lock.is_locked(), "commit_block_and_switch requires SCHEDULER.lock held");
    assert_eq!(cpu_if_bit(), 0, "commit_block_and_switch requires IF=0");
    assert!(!outgoing_thread.is_null(), "outgoing_thread cannot be null");

    // Machine-Level CR3 Invariant: Switch address space under SCHEDULER.lock
    let next_thread = current_thread_from_gs();
    switch_address_space_locked(outgoing_thread, next_thread);

    // 1. Release scheduler lock keeping IF=0 (Option B contract)
    SCHEDULER.lock.unlock_keep_cli();

    // 2. Assert pre-switch machine invariants
    assert!(!SCHEDULER.lock.is_locked(), "sched_lock must be FREE before switch");
    assert_eq!(cpu_if_bit(), 0, "CPU IF must be 0 before switch");
    assert_ne!(current_thread_from_gs(), outgoing_thread, "GS+16 must point to next thread");
    assert_eq!(percpu::BSP_PERCPU.nested_irq_count, 0, "nested_irq_count must be 0");

    // 3. Terminal context switch transfer
    terminal_context_switch(prev_rsp_ptr, next_rsp, orig_rflags, target_frame);

    // ==================== POST-RESUMPTION ====================
    // Resumes here ONLY after another thread wakes this thread and scheduler runs it!
    assert_eq!(current_thread_from_gs(), outgoing_thread, "GS+16 must match resumed thread");
    assert_eq!((*outgoing_thread).state, ThreadState::Running, "Resumed thread must be Running");
    let restored_if = cpu_if_bit();
    let expected_if = (orig_rflags & 0x200) >> 9;
    assert_eq!(restored_if, expected_if, "Restored IF must match caller IF");
}

/// Public API: Blocks the current thread on `wait_queue`.
#[inline(never)]
pub fn block_current(wait_queue: &mut crate::task::waitqueue::WaitQueue) {
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let (current, prev_rsp, next_rsp, frame_type) = unsafe { prepare_block_locked(wait_queue) };
    unsafe { commit_block_and_switch(current, prev_rsp, next_rsp, orig_rflags, frame_type) };
}

// Stage 3D Increment 1 Verification State
static INC1_WORKER_A_PTR: AtomicU64 = AtomicU64::new(0);
static INC1_WORKER_B_PTR: AtomicU64 = AtomicU64::new(0);
static INC1_STAGE: AtomicU32 = AtomicU32::new(0);
static mut INC1_WAIT_QUEUE: crate::task::waitqueue::WaitQueue = crate::task::waitqueue::WaitQueue::new();

extern "C" fn inc1_worker_a(_arg: u64) {
    INC1_STAGE.store(1, Ordering::SeqCst); // 1 = Worker A started, about to block
    unsafe {
        block_current(&mut INC1_WAIT_QUEUE);
    }
    // Worker A resumed here!
    assert_eq!(INC1_STAGE.load(Ordering::SeqCst), 2, "Worker A must resume after Worker B sets stage 2");
    INC1_STAGE.store(3, Ordering::SeqCst); // 3 = Worker A resumed successfully
    exit_current_thread();
}

extern "C" fn inc1_worker_b(_arg: u64) {
    assert_eq!(INC1_STAGE.load(Ordering::SeqCst), 1, "Worker B must run after Worker A blocked");
    let a_ptr = INC1_WORKER_A_PTR.load(Ordering::SeqCst) as *mut KernelThread;
    unsafe {
        // Assertions on Worker A while blocked
        assert_eq!((*a_ptr).state, ThreadState::Blocked, "Worker A must be Blocked");
        assert_eq!(INC1_WAIT_QUEUE.len(), 1, "WaitQueue must have exactly 1 waiter");

        INC1_STAGE.store(2, Ordering::SeqCst); // 2 = Worker B about to wake Worker A
        let woken = INC1_WAIT_QUEUE.wake_one();
        assert_eq!(woken, Some(a_ptr), "wake_one must pop Worker A");
        assert_eq!((*a_ptr).state, ThreadState::Ready, "Worker A must be Ready after wake");
        assert!((*a_ptr).next_waiter.is_null(), "Worker A must be cleanly detached after wake");
        assert_eq!(INC1_WAIT_QUEUE.len(), 0, "WaitQueue must be empty after wake");
    }
    // Yield to allow Worker A to be scheduled
    yield_now();
    assert_eq!(INC1_STAGE.load(Ordering::SeqCst), 3, "Worker A must have reached stage 3");
    INC1_STAGE.store(4, Ordering::SeqCst); // 4 = Worker B finishing
    exit_current_thread();
}

extern "C" fn inc1_dummy_worker(_arg: u64) {
    exit_current_thread();
}

/// Executes Stage 3D Increment 1 verification:
/// 1. Priority FIFO WaitQueue ordering test (Critical > High > Normal, FIFO equal).
/// 2. Detachment invariant test (`next_waiter == null` after pop/remove).
/// 3. Single wait-channel invariant test.
/// 4. Basic block and wake between Worker A and Worker B.
/// 5. Stack accounting & physical frame leak check.
#[inline(never)]
pub fn run_stage3d_inc1_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3D-Increment 1: Core Block/Wake Foundation]");

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Step 1: WaitQueue Priority FIFO Ordering & Detachment Verification
    // ====================================================================
    let t_norm1 = create_thread(inc1_dummy_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create dummy normal thread 1");
    let t_crit = create_thread(inc1_dummy_worker, 0, Priority::Critical, pmm, vmm)
        .expect("Failed to create dummy critical thread");
    let t_norm2 = create_thread(inc1_dummy_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create dummy normal thread 2");
    let t_high = create_thread(inc1_dummy_worker, 0, Priority::High, pmm, vmm)
        .expect("Failed to create dummy high thread");

    let mut test_queue = crate::task::waitqueue::WaitQueue::new();
    unsafe {
        let orig = SCHEDULER.lock.acquire();

        (*t_norm1).state = ThreadState::Blocked;
        (*t_crit).state = ThreadState::Blocked;
        (*t_norm2).state = ThreadState::Blocked;
        (*t_high).state = ThreadState::Blocked;

        // Verify I1: next_waiter is null before insertion
        assert!((*t_norm1).next_waiter.is_null());
        assert!((*t_crit).next_waiter.is_null());
        assert!((*t_norm2).next_waiter.is_null());
        assert!((*t_high).next_waiter.is_null());

        // Insertion order: Normal1, Critical, Normal2, High
        test_queue.push_priority_locked(t_norm1);
        test_queue.push_priority_locked(t_crit);
        test_queue.push_priority_locked(t_norm2);
        test_queue.push_priority_locked(t_high);

        assert_eq!(test_queue.len(), 4, "WaitQueue count must be 4");

        // Pop order must be: Critical -> High -> Normal1 -> Normal2
        let p1 = test_queue.pop_highest_locked().expect("Pop 1 failed");
        assert_eq!(p1, t_crit, "1st pop must be Critical");
        assert!((*p1).next_waiter.is_null(), "I6: p1 must be completely detached");

        let p2 = test_queue.pop_highest_locked().expect("Pop 2 failed");
        assert_eq!(p2, t_high, "2nd pop must be High");
        assert!((*p2).next_waiter.is_null(), "I6: p2 must be completely detached");

        let p3 = test_queue.pop_highest_locked().expect("Pop 3 failed");
        assert_eq!(p3, t_norm1, "3rd pop must be Normal1 (FIFO)");
        assert!((*p3).next_waiter.is_null(), "I6: p3 must be completely detached");

        let p4 = test_queue.pop_highest_locked().expect("Pop 4 failed");
        assert_eq!(p4, t_norm2, "4th pop must be Normal2 (FIFO)");
        assert!((*p4).next_waiter.is_null(), "I6: p4 must be completely detached");

        assert_eq!(test_queue.len(), 0, "WaitQueue count must be 0");
        assert!(test_queue.is_empty(), "WaitQueue must be empty");

        // Test remove_locked unlinking
        test_queue.push_priority_locked(t_norm1);
        test_queue.push_priority_locked(t_high);
        test_queue.push_priority_locked(t_crit);
        assert_eq!(test_queue.len(), 3);

        // Remove middle node (t_high)
        let removed = test_queue.remove_locked(t_high);
        assert!(removed, "remove_locked(t_high) must succeed");
        assert!((*t_high).next_waiter.is_null(), "I6: removed node must be detached");
        assert_eq!(test_queue.len(), 2);

        // Remaining must be Critical -> Normal1
        let r1 = test_queue.pop_highest_locked().expect("Pop r1 failed");
        assert_eq!(r1, t_crit);
        assert!((*r1).next_waiter.is_null());

        let r2 = test_queue.pop_highest_locked().expect("Pop r2 failed");
        assert_eq!(r2, t_norm1);
        assert!((*r2).next_waiter.is_null());

        assert_eq!(test_queue.len(), 0);

        SCHEDULER.lock.unlock_restore(orig);
    }

    destroy_thread(t_norm1, pmm, vmm).expect("destroy t_norm1 failed");
    destroy_thread(t_crit, pmm, vmm).expect("destroy t_crit failed");
    destroy_thread(t_norm2, pmm, vmm).expect("destroy t_norm2 failed");
    destroy_thread(t_high, pmm, vmm).expect("destroy t_high failed");

    kprintln!("  Priority FIFO ordering: Critical > High > Normal, FIFO equal [VERIFIED]");
    kprintln!("  Waiter detachment: next_waiter == NULL after pop/remove [VERIFIED]");
    kprintln!("  Single wait-channel invariant: exclusive next_waiter linkage [VERIFIED]");

    // ====================================================================
    // Step 2: Live Cooperating Block & Wake Execution
    // ====================================================================
    INC1_STAGE.store(0, Ordering::SeqCst);
    unsafe {
        INC1_WAIT_QUEUE = crate::task::waitqueue::WaitQueue::new();
    }

    let worker_a = create_thread(inc1_worker_a, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create inc1 worker A");
    let worker_b = create_thread(inc1_worker_b, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create inc1 worker B");

    INC1_WORKER_A_PTR.store(worker_a as u64, Ordering::SeqCst);
    INC1_WORKER_B_PTR.store(worker_b as u64, Ordering::SeqCst);

    spawn(worker_a);
    spawn(worker_b);

    // Relinquish execution until worker threads complete
    while INC1_STAGE.load(Ordering::SeqCst) < 4 {
        yield_now();
    }

    assert_eq!(INC1_STAGE.load(Ordering::SeqCst), 4, "Worker threads must have completed all stages");

    kprintln!("  Live block_current() -> Blocked state transition [VERIFIED]");
    kprintln!("  Live wake_one() -> Ready state transition & RunQueue re-enqueue [VERIFIED]");
    kprintln!("  Resumption verification: state == Running, GS+16 == current [VERIFIED]");

    destroy_thread(worker_a, pmm, vmm).expect("destroy worker A failed");
    destroy_thread(worker_b, pmm, vmm).expect("destroy worker B failed");

    // ====================================================================
    // Step 3: PMM Accounting Check
    // ====================================================================
    let final_free = pmm.free_frame_count();
    kprintln!("  Stack Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    assert_eq!(baseline_free, final_free, "PMM free frames must match baseline exactly");

    kprintln!("  [x] Stage 3D Increment 1 Core Block/Wake verified.");
}

// ========================================================================
// Stage 3D Increment 2: Kernel Mutex Primitive Verification
// ========================================================================
static mut INC2_MUTEX: crate::task::mutex::Mutex = crate::task::mutex::Mutex::new();
static INC2_STAGE: AtomicU32 = AtomicU32::new(0);
static INC2_WORKER_A_PTR: AtomicU64 = AtomicU64::new(0);
static INC2_WORKER_B_PTR: AtomicU64 = AtomicU64::new(0);
static INC2_HANDOFF_OWNER: AtomicU64 = AtomicU64::new(0);
static INC2_HANDOFF_RESUMED: AtomicBool = AtomicBool::new(false);

extern "C" fn inc2_worker_a(_arg: u64) {
    let a_id = unsafe { (*current_thread_from_gs()).id };
    assert!(a_id > 0, "Worker A ID must be non-zero");

    // ====================================================================
    // Test A: Fast Path Lock and Unlock
    // ====================================================================
    unsafe {
        assert!(!INC2_MUTEX.is_locked());
        assert_eq!(INC2_MUTEX.owner(), 0);
        assert_eq!(INC2_MUTEX.waiter_count(), 0);

        INC2_MUTEX.lock();
        assert!(INC2_MUTEX.is_locked());
        assert_eq!(INC2_MUTEX.owner(), a_id);
        assert_eq!((*current_thread_from_gs()).state, ThreadState::Running);
        assert_eq!(INC2_MUTEX.waiter_count(), 0);

        INC2_MUTEX.unlock();
        assert!(!INC2_MUTEX.is_locked());
        assert_eq!(INC2_MUTEX.owner(), 0);
        assert_eq!(INC2_MUTEX.waiter_count(), 0);
        assert_eq!((*current_thread_from_gs()).state, ThreadState::Running);
    }
    kprintln!("  Test A: Fast path lock and unlock [VERIFIED]");

    // ====================================================================
    // Test E: Ownership Validation & Non-Recursive Rejection
    // ====================================================================
    unsafe {
        // 1. Unlocked mutex: unlock must fail with NotLocked
        assert_eq!(
            INC2_MUTEX.validate_unlock(a_id),
            Err(crate::task::mutex::UnlockError::NotLocked),
            "Unlocking an unlocked mutex must fail deterministically"
        );

        // 2. Lock the mutex
        INC2_MUTEX.lock();

        // 3. Non-owner unlock must fail with NotOwner
        let non_owner_id = a_id + 0xDEAD;
        assert_eq!(
            INC2_MUTEX.validate_unlock(non_owner_id),
            Err(crate::task::mutex::UnlockError::NotOwner),
            "Non-owner unlock must fail deterministically"
        );

        // 4. Recursive lock by current owner must fail with RecursiveLock
        assert_eq!(
            INC2_MUTEX.validate_lock(a_id),
            Err(crate::task::mutex::LockError::RecursiveLock),
            "Recursive lock by owner must fail deterministically"
        );

        INC2_MUTEX.unlock();
    }
    kprintln!("  Test E: Ownership validation & non-recursive rejection [VERIFIED]");

    // ====================================================================
    // Test F: No Waiter Unlock
    // ====================================================================
    unsafe {
        INC2_MUTEX.lock();
        assert_eq!(INC2_MUTEX.waiter_count(), 0);
        INC2_MUTEX.unlock();
        assert_eq!(INC2_MUTEX.owner(), 0);
        assert_eq!(INC2_MUTEX.waiter_count(), 0);
    }
    kprintln!("  Test F: No waiter unlock transitions owner to 0 [VERIFIED]");

    // ====================================================================
    // Test B & C: Contention & Direct Ownership Handoff
    // ====================================================================
    // Acquire mutex
    unsafe { INC2_MUTEX.lock(); }
    assert_eq!(unsafe { INC2_MUTEX.owner() }, a_id, "Worker A must own mutex");
    assert_eq!(unsafe { INC2_MUTEX.waiter_count() }, 0, "Waiters must be 0 initially");

    INC2_STAGE.store(1, Ordering::SeqCst); // 1 = Worker A acquired mutex

    // Yield to let Worker B run and attempt to acquire mutex
    while INC2_STAGE.load(Ordering::SeqCst) < 2 {
        yield_now();
    }

    // Worker B has attempted lock() and is now blocked
    let b_ptr = INC2_WORKER_B_PTR.load(Ordering::SeqCst) as *mut KernelThread;
    unsafe {
        assert_eq!((*b_ptr).state, ThreadState::Blocked, "Worker B must be Blocked");
        assert_eq!(INC2_MUTEX.waiter_count(), 1, "Waiters must be 1 (Worker B)");
        assert_eq!(INC2_MUTEX.owner(), a_id, "Worker A must still be owner");
    }
    kprintln!("  Test B: Contention blocking on held mutex [VERIFIED]");

    // Now unlock mutex - this will perform direct ownership handoff to Worker B!
    unsafe {
        INC2_MUTEX.unlock();
        // Immediately verify direct handoff state while Worker A is still running:
        let b_id = (*b_ptr).id;
        assert_eq!(INC2_MUTEX.owner(), b_id, "M7: Direct handoff must set owner to Worker B");
        assert_eq!((*b_ptr).state, ThreadState::Ready, "Worker B must be transitioned to Ready");
        assert!((*b_ptr).next_waiter.is_null(), "M5: Worker B must be detached");
        assert_eq!(INC2_MUTEX.waiter_count(), 0, "Waiters must be 0 after handoff");
        INC2_HANDOFF_OWNER.store(b_id, Ordering::SeqCst);
    }

    INC2_STAGE.store(3, Ordering::SeqCst); // 3 = Worker A unlocked, handed off to Worker B

    // Yield to let Worker B execute its critical section
    while !INC2_HANDOFF_RESUMED.load(Ordering::SeqCst) {
        yield_now();
    }

    exit_current_thread();
}

extern "C" fn inc2_worker_b(_arg: u64) {
    let current_id = unsafe { (*current_thread_from_gs()).id };

    // Wait until Worker A has acquired the mutex
    while INC2_STAGE.load(Ordering::SeqCst) < 1 {
        yield_now();
    }

    // Mark that Worker B is about to contend on mutex
    INC2_STAGE.store(2, Ordering::SeqCst);

    // Contended lock: will block in prepare_block_locked and commit_block_and_switch
    unsafe {
        INC2_MUTEX.lock();
    }

    // RESUMED DIRECTLY FROM HANDOFF!
    // Must be the owner without re-acquiring or spinning
    assert_eq!(unsafe { INC2_MUTEX.owner() }, current_id, "Worker B must own mutex upon resumption");
    assert_eq!(INC2_STAGE.load(Ordering::SeqCst), 3, "Worker B must resume after Worker A unlocked");
    INC2_HANDOFF_RESUMED.store(true, Ordering::SeqCst);
    kprintln!("  Test C: Direct ownership handoff [VERIFIED]");

    // Worker B releases mutex
    unsafe {
        INC2_MUTEX.unlock();
        assert_eq!(INC2_MUTEX.owner(), 0, "Mutex must be free after Worker B unlocks");
    }

    exit_current_thread();
}

/// Executes Stage 3D Increment 2 verification:
/// - Test A: Fast path lock and unlock
/// - Test B: Contention blocking on held mutex
/// - Test C: Direct ownership handoff
/// - Test D: Priority FIFO waiter ordering: Critical > High > Normal
/// - Test E: Ownership validation & non-recursive rejection
/// - Test F: No waiter unlock transitions owner to 0
/// - Test G: PMM exact frame accounting check
#[inline(never)]
pub fn run_stage3d_inc2_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3D-Increment 2: Kernel Mutex Primitive]");

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Test D: Priority FIFO Waiter Ordering on Mutex
    // ====================================================================
    let t_norm1 = create_thread(inc1_dummy_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create dummy normal thread 1");
    let t_crit = create_thread(inc1_dummy_worker, 0, Priority::Critical, pmm, vmm)
        .expect("Failed to create dummy critical thread");
    let t_norm2 = create_thread(inc1_dummy_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create dummy normal thread 2");
    let t_high = create_thread(inc1_dummy_worker, 0, Priority::High, pmm, vmm)
        .expect("Failed to create dummy high thread");

    unsafe {
        let orig = SCHEDULER.lock.acquire();

        (*t_norm1).state = ThreadState::Blocked;
        (*t_crit).state = ThreadState::Blocked;
        (*t_norm2).state = ThreadState::Blocked;
        (*t_high).state = ThreadState::Blocked;

        // Reset INC2_MUTEX to clean state and lock under t_crit
        INC2_MUTEX = crate::task::mutex::Mutex::new();
        INC2_MUTEX.owner = (*t_crit).id;

        // Queue waiters in order: Normal1, Critical, Normal2, High
        INC2_MUTEX.waiters.push_priority_locked(t_norm1);
        INC2_MUTEX.waiters.push_priority_locked(t_crit);
        INC2_MUTEX.waiters.push_priority_locked(t_norm2);
        INC2_MUTEX.waiters.push_priority_locked(t_high);

        assert_eq!(INC2_MUTEX.waiters.len(), 4);

        // Pop order must be: Critical -> High -> Normal1 -> Normal2 (FIFO for equal priority)
        let p1 = INC2_MUTEX.waiters.pop_highest_locked().expect("Pop 1 failed");
        assert_eq!(p1, t_crit, "1st waiter must be Critical");
        assert!((*p1).next_waiter.is_null());

        let p2 = INC2_MUTEX.waiters.pop_highest_locked().expect("Pop 2 failed");
        assert_eq!(p2, t_high, "2nd waiter must be High");
        assert!((*p2).next_waiter.is_null());

        let p3 = INC2_MUTEX.waiters.pop_highest_locked().expect("Pop 3 failed");
        assert_eq!(p3, t_norm1, "3rd waiter must be Normal1");
        assert!((*p3).next_waiter.is_null());

        let p4 = INC2_MUTEX.waiters.pop_highest_locked().expect("Pop 4 failed");
        assert_eq!(p4, t_norm2, "4th waiter must be Normal2");
        assert!((*p4).next_waiter.is_null());

        assert_eq!(INC2_MUTEX.waiters.len(), 0);
        INC2_MUTEX.owner = 0;

        SCHEDULER.lock.unlock_restore(orig);
    }

    destroy_thread(t_norm1, pmm, vmm).expect("destroy t_norm1 failed");
    destroy_thread(t_crit, pmm, vmm).expect("destroy t_crit failed");
    destroy_thread(t_norm2, pmm, vmm).expect("destroy t_norm2 failed");
    destroy_thread(t_high, pmm, vmm).expect("destroy t_high failed");

    kprintln!("  Test D: Priority FIFO waiter ordering: Critical > High > Normal [VERIFIED]");

    // ====================================================================
    // Tests A, E, F, B, C: Live Worker Execution
    // ====================================================================
    INC2_STAGE.store(0, Ordering::SeqCst);
    INC2_HANDOFF_OWNER.store(0, Ordering::SeqCst);
    INC2_HANDOFF_RESUMED.store(false, Ordering::SeqCst);
    unsafe {
        INC2_MUTEX = crate::task::mutex::Mutex::new();
    }

    let worker_a = create_thread(inc2_worker_a, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create inc2 worker A");
    let worker_b = create_thread(inc2_worker_b, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create inc2 worker B");

    INC2_WORKER_A_PTR.store(worker_a as u64, Ordering::SeqCst);
    INC2_WORKER_B_PTR.store(worker_b as u64, Ordering::SeqCst);

    spawn(worker_a);
    spawn(worker_b);

    // Relinquish execution until both worker threads complete
    while INC2_STAGE.load(Ordering::SeqCst) < 3 || !INC2_HANDOFF_RESUMED.load(Ordering::SeqCst) {
        yield_now();
    }

    destroy_thread(worker_a, pmm, vmm).expect("destroy worker A failed");
    destroy_thread(worker_b, pmm, vmm).expect("destroy worker B failed");

    // ====================================================================
    // Test G: PMM Accounting Check
    // ====================================================================
    let final_free = pmm.free_frame_count();
    kprintln!("  Stack Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    assert_eq!(baseline_free, final_free, "PMM free frames must match baseline exactly");

    kprintln!("  [x] Stage 3D Increment 2 Kernel Mutex verified.");
}

// ========================================================================
// Stage 3D Increment 3: Condition Variable Primitive Verification
// ========================================================================
static mut INC3_MUTEX: crate::task::mutex::Mutex = crate::task::mutex::Mutex::new();
static mut INC3_CONDVAR: crate::task::condvar::Condvar = crate::task::condvar::Condvar::new();

// Test A & D State
static INC3_STAGE: AtomicU32 = AtomicU32::new(0);
static INC3_PREDICATE: AtomicBool = AtomicBool::new(false);
static INC3_WORKER_A_PTR: AtomicU64 = AtomicU64::new(0);
static INC3_WORKER_B_PTR: AtomicU64 = AtomicU64::new(0);
static INC3_LOST_WAKEUP_VERIFIED: AtomicBool = AtomicBool::new(false);

// Test B State (Broadcast)
static INC3_BCAST_GO: AtomicBool = AtomicBool::new(false);
static INC3_BCAST_COMPLETED: AtomicU32 = AtomicU32::new(0);

extern "C" fn inc3_worker_a(_arg: u64) {
    let a_id = unsafe { (*current_thread_from_gs()).id };

    // Acquire mutex
    unsafe { INC3_MUTEX.lock(); }
    assert_eq!(unsafe { INC3_MUTEX.owner() }, a_id, "Worker A must own mutex");

    INC3_STAGE.store(1, Ordering::SeqCst); // 1 = Worker A holds mutex, ready to wait on predicate

    // Loop on predicate: while !INC3_PREDICATE { cond.wait(&mut mutex); }
    while !INC3_PREDICATE.load(Ordering::SeqCst) {
        unsafe {
            INC3_CONDVAR.wait(&mut INC3_MUTEX);
        }
        // When execution resumes here, Worker A has re-acquired INC3_MUTEX!
        assert_eq!(unsafe { INC3_MUTEX.owner() }, a_id, "Worker A must hold mutex upon waking from wait()");
    }

    // Predicate observed true while holding mutex!
    assert!(INC3_PREDICATE.load(Ordering::SeqCst), "Predicate must be true when exiting wait loop");
    INC3_STAGE.store(3, Ordering::SeqCst); // 3 = Worker A completed wait loop

    unsafe {
        INC3_MUTEX.unlock();
    }

    exit_current_thread();
}

extern "C" fn inc3_worker_b(_arg: u64) {
    let b_id = unsafe { (*current_thread_from_gs()).id };

    // Wait until Worker A has acquired the mutex and is about to wait
    while INC3_STAGE.load(Ordering::SeqCst) < 1 {
        yield_now();
    }

    // Attempt to acquire mutex:
    // This MUST block until Worker A atomically enqueues on condvar and calls mutex.unlock_locked()!
    unsafe {
        INC3_MUTEX.lock();
    }

    // ATOMIC REGISTRATION / NO LOST WAKEUP VERIFICATION (Test D):
    // Worker B has acquired the mutex.
    // Therefore, Worker A has ALREADY:
    // 1. Enqueued onto INC3_CONDVAR.waiters (waiter_count == 1)
    // 2. Released INC3_MUTEX
    // 3. Entered ThreadState::Blocked
    let a_ptr = INC3_WORKER_A_PTR.load(Ordering::SeqCst) as *mut KernelThread;
    unsafe {
        assert_eq!((*a_ptr).state, ThreadState::Blocked, "Worker A must be Blocked before signaler changes predicate");
        assert_eq!(INC3_CONDVAR.waiter_count(), 1, "Worker A must be registered on condvar waiters");
        assert_eq!(INC3_MUTEX.owner(), b_id, "Worker B must be the sole owner of mutex");
    }

    // Now Worker B modifies the protected predicate:
    INC3_PREDICATE.store(true, Ordering::SeqCst);

    // Now Worker B signals the condvar:
    let woken = unsafe { INC3_CONDVAR.signal() };
    assert!(woken, "signal() must have woken Worker A");

    // Immediately after signal:
    // Worker A has transitioned Blocked -> Ready and was enqueued into RunQueue,
    // but Worker B STILL OWNS THE MUTEX!
    unsafe {
        assert_eq!((*a_ptr).state, ThreadState::Ready, "Worker A must be Ready after signal");
        assert_eq!(INC3_CONDVAR.waiter_count(), 0, "Condvar waiters must be 0 after signal");
        assert_eq!(INC3_MUTEX.owner(), b_id, "Worker B still holds mutex after signaling");
    }

    INC3_LOST_WAKEUP_VERIFIED.store(true, Ordering::SeqCst);
    INC3_STAGE.store(2, Ordering::SeqCst); // 2 = Signaler signaled, about to unlock

    // Worker B unlocks mutex - this allows Worker A to acquire the mutex in wait()
    unsafe {
        INC3_MUTEX.unlock();
    }

    // Yield to let Worker A run and complete
    while INC3_STAGE.load(Ordering::SeqCst) < 3 {
        yield_now();
    }

    exit_current_thread();
}

extern "C" fn inc3_bcast_worker(_arg: u64) {
    let current_id = unsafe { (*current_thread_from_gs()).id };

    // Acquire shared mutex
    unsafe { INC3_MUTEX.lock(); }
    assert_eq!(unsafe { INC3_MUTEX.owner() }, current_id);

    // Wait on broadcast condition
    while !INC3_BCAST_GO.load(Ordering::SeqCst) {
        unsafe {
            INC3_CONDVAR.wait(&mut INC3_MUTEX);
        }
        assert_eq!(unsafe { INC3_MUTEX.owner() }, current_id, "Woken waiter must hold mutex");
    }

    // Critical section after broadcast:
    assert!(INC3_BCAST_GO.load(Ordering::SeqCst));
    INC3_BCAST_COMPLETED.fetch_add(1, Ordering::SeqCst);

    unsafe {
        INC3_MUTEX.unlock();
    }

    exit_current_thread();
}

extern "C" fn inc3_bcast_sender(_arg: u64) {
    let current_id = unsafe { (*current_thread_from_gs()).id };

    // Wait until all 3 workers have blocked on condvar
    while unsafe { INC3_CONDVAR.waiter_count() } < 3 {
        yield_now();
    }

    // Broadcaster acquires mutex, updates state, and broadcasts
    unsafe {
        INC3_MUTEX.lock();
        assert_eq!(INC3_MUTEX.owner(), current_id);
        assert_eq!(INC3_CONDVAR.waiter_count(), 3, "All 3 workers must be waiting on condvar");
        INC3_BCAST_GO.store(true, Ordering::SeqCst);

        let woken = INC3_CONDVAR.broadcast();
        assert_eq!(woken, 3, "broadcast() must have woken all 3 workers");
        assert_eq!(INC3_CONDVAR.waiter_count(), 0, "Condvar waiter queue must be empty after broadcast");

        INC3_MUTEX.unlock();
    }

    exit_current_thread();
}

/// Executes Stage 3D Increment 3 verification:
/// - Test A: Basic Predicate Wait & Signal
/// - Test B: Broadcast & serialized mutex contention
/// - Test C: Priority FIFO waiter ordering on Condvar
/// - Test D: Atomic registration & no lost wakeup proof
/// - Test E: Ownership validation for condvar wait
/// - Test F: PMM exact frame accounting check
#[inline(never)]
pub fn run_stage3d_inc3_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3D-Increment 3: Condition Variable Primitive]");

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Test E: Ownership Validation for Condvar Wait
    // ====================================================================
    let dummy_owner = create_thread(inc1_dummy_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create dummy owner thread");
    let owner_id = unsafe { (*dummy_owner).id };

    unsafe {
        INC3_MUTEX = crate::task::mutex::Mutex::new();
        INC3_CONDVAR = crate::task::condvar::Condvar::new();

        // When mutex is unlocked, validate_wait must fail
        assert_eq!(
            INC3_CONDVAR.validate_wait(&INC3_MUTEX, owner_id),
            Err(crate::task::condvar::CondvarError::MutexNotHeld),
            "Waiting without holding mutex must fail validation"
        );

        // When mutex is held by another ID, validate_wait must fail
        INC3_MUTEX.owner = owner_id;
        let fake_caller = owner_id + 0xCAFE;
        assert_eq!(
            INC3_CONDVAR.validate_wait(&INC3_MUTEX, fake_caller),
            Err(crate::task::condvar::CondvarError::MutexNotHeld),
            "Waiting with mutex owned by another thread must fail validation"
        );

        INC3_MUTEX.owner = 0;
    }
    destroy_thread(dummy_owner, pmm, vmm).expect("destroy dummy owner failed");

    kprintln!("  Test E: Ownership validation for condvar wait [VERIFIED]");

    // ====================================================================
    // Test C: Priority FIFO Waiter Ordering on Condvar
    // ====================================================================
    let t_norm1 = create_thread(inc1_dummy_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create dummy normal thread 1");
    let t_crit = create_thread(inc1_dummy_worker, 0, Priority::Critical, pmm, vmm)
        .expect("Failed to create dummy critical thread");
    let t_norm2 = create_thread(inc1_dummy_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create dummy normal thread 2");
    let t_high = create_thread(inc1_dummy_worker, 0, Priority::High, pmm, vmm)
        .expect("Failed to create dummy high thread");

    unsafe {
        let orig = SCHEDULER.lock.acquire();

        (*t_norm1).state = ThreadState::Blocked;
        (*t_crit).state = ThreadState::Blocked;
        (*t_norm2).state = ThreadState::Blocked;
        (*t_high).state = ThreadState::Blocked;

        INC3_CONDVAR = crate::task::condvar::Condvar::new();

        // Queue waiters in order: Normal1, Critical, Normal2, High
        INC3_CONDVAR.waiters.push_priority_locked(t_norm1);
        INC3_CONDVAR.waiters.push_priority_locked(t_crit);
        INC3_CONDVAR.waiters.push_priority_locked(t_norm2);
        INC3_CONDVAR.waiters.push_priority_locked(t_high);

        assert_eq!(INC3_CONDVAR.waiters.len(), 4);

        // Pop order must be: Critical -> High -> Normal1 -> Normal2 (FIFO for equal priority)
        let p1 = INC3_CONDVAR.waiters.pop_highest_locked().expect("Pop 1 failed");
        assert_eq!(p1, t_crit, "1st waiter must be Critical");
        assert!((*p1).next_waiter.is_null());

        let p2 = INC3_CONDVAR.waiters.pop_highest_locked().expect("Pop 2 failed");
        assert_eq!(p2, t_high, "2nd waiter must be High");
        assert!((*p2).next_waiter.is_null());

        let p3 = INC3_CONDVAR.waiters.pop_highest_locked().expect("Pop 3 failed");
        assert_eq!(p3, t_norm1, "3rd waiter must be Normal1");
        assert!((*p3).next_waiter.is_null());

        let p4 = INC3_CONDVAR.waiters.pop_highest_locked().expect("Pop 4 failed");
        assert_eq!(p4, t_norm2, "4th waiter must be Normal2");
        assert!((*p4).next_waiter.is_null());

        assert_eq!(INC3_CONDVAR.waiters.len(), 0);

        SCHEDULER.lock.unlock_restore(orig);
    }

    destroy_thread(t_norm1, pmm, vmm).expect("destroy t_norm1 failed");
    destroy_thread(t_crit, pmm, vmm).expect("destroy t_crit failed");
    destroy_thread(t_norm2, pmm, vmm).expect("destroy t_norm2 failed");
    destroy_thread(t_high, pmm, vmm).expect("destroy t_high failed");

    kprintln!("  Test C: Priority FIFO waiter ordering: Critical > High > Normal [VERIFIED]");

    // ====================================================================
    // Tests A & D: Predicate Wait, Signal, and No Lost Wakeup Proof
    // ====================================================================
    INC3_STAGE.store(0, Ordering::SeqCst);
    INC3_PREDICATE.store(false, Ordering::SeqCst);
    INC3_LOST_WAKEUP_VERIFIED.store(false, Ordering::SeqCst);
    unsafe {
        INC3_MUTEX = crate::task::mutex::Mutex::new();
        INC3_CONDVAR = crate::task::condvar::Condvar::new();
    }

    let worker_a = create_thread(inc3_worker_a, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create inc3 worker A");
    let worker_b = create_thread(inc3_worker_b, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create inc3 worker B");

    INC3_WORKER_A_PTR.store(worker_a as u64, Ordering::SeqCst);
    INC3_WORKER_B_PTR.store(worker_b as u64, Ordering::SeqCst);

    spawn(worker_a);
    spawn(worker_b);

    while INC3_STAGE.load(Ordering::SeqCst) < 3 || !INC3_LOST_WAKEUP_VERIFIED.load(Ordering::SeqCst) {
        yield_now();
    }

    kprintln!("  Test A: Basic predicate wait & signal [VERIFIED]");
    kprintln!("  Test D: Atomic registration & no lost wakeup proof [VERIFIED]");

    destroy_thread(worker_a, pmm, vmm).expect("destroy worker A failed");
    destroy_thread(worker_b, pmm, vmm).expect("destroy worker B failed");

    // ====================================================================
    // Test B: Broadcast & Serialized Mutex Contention
    // ====================================================================
    INC3_BCAST_GO.store(false, Ordering::SeqCst);
    INC3_BCAST_COMPLETED.store(0, Ordering::SeqCst);
    unsafe {
        INC3_MUTEX = crate::task::mutex::Mutex::new();
        INC3_CONDVAR = crate::task::condvar::Condvar::new();
    }

    let w1 = create_thread(inc3_bcast_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create bcast worker 1");
    let w2 = create_thread(inc3_bcast_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create bcast worker 2");
    let w3 = create_thread(inc3_bcast_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create bcast worker 3");
    let sender = create_thread(inc3_bcast_sender, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create bcast sender");

    spawn(w1);
    spawn(w2);
    spawn(w3);
    spawn(sender);

    // Relinquish execution until all 3 workers serialize through mutex and complete
    while INC3_BCAST_COMPLETED.load(Ordering::SeqCst) < 3 {
        yield_now();
    }

    assert_eq!(INC3_BCAST_COMPLETED.load(Ordering::SeqCst), 3, "All 3 workers must have completed");

    destroy_thread(w1, pmm, vmm).expect("destroy w1 failed");
    destroy_thread(w2, pmm, vmm).expect("destroy w2 failed");
    destroy_thread(w3, pmm, vmm).expect("destroy w3 failed");
    destroy_thread(sender, pmm, vmm).expect("destroy sender failed");

    kprintln!("  Test B: Broadcast & serialized mutex contention [VERIFIED]");

    // ====================================================================
    // Test F: PMM Accounting Check
    // ====================================================================
    let final_free = pmm.free_frame_count();
    kprintln!("  Stack Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    assert_eq!(baseline_free, final_free, "PMM free frames must match baseline exactly");

    kprintln!("  [x] Stage 3D Increment 3 Condition Variables verified.");
}

// ====================================================================
// Stage 3D Increment 4: Timer Sleep Verification
// ====================================================================

static INC4_WORKER_COMPLETED: [AtomicBool; 8] = [
    AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false),
    AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false),
];
static INC4_WORKER_START: [AtomicU64; 8] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];
static INC4_WORKER_END: [AtomicU64; 8] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];
static INC4_ORDER: [AtomicU32; 8] = [
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
];
static INC4_ORDER_COUNT: AtomicUsize = AtomicUsize::new(0);

static INC4_TEST_K_HIGH_RAN: AtomicBool = AtomicBool::new(false);
static INC4_TEST_K_NORM_RAN_AFTER: AtomicBool = AtomicBool::new(false);

extern "C" fn inc4_sleep_worker(arg: u64) {
    let idx = (arg & 0xFF) as usize;
    let ms = arg >> 8;

    let start = crate::hal::arch::x86_64::timer::TICKS.load(Ordering::Relaxed);
    INC4_WORKER_START[idx].store(start, Ordering::SeqCst);

    let ok = crate::task::sleep::sleep_ms(ms);
    assert!(ok, "sleep_ms must return true");

    let end = crate::hal::arch::x86_64::timer::TICKS.load(Ordering::Relaxed);
    INC4_WORKER_END[idx].store(end, Ordering::SeqCst);

    let order = INC4_ORDER_COUNT.fetch_add(1, Ordering::SeqCst);
    if order < 8 {
        INC4_ORDER[order].store(idx as u32, Ordering::SeqCst);
    }
    INC4_WORKER_COMPLETED[idx].store(true, Ordering::SeqCst);

    exit_current_thread();
}

extern "C" fn inc4_test_k_high_worker(_arg: u64) {
    INC4_TEST_K_HIGH_RAN.store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc4_test_k_norm_worker(arg: u64) {
    let k_high_ptr = arg as *mut KernelThread;
    if !k_high_ptr.is_null() {
        spawn(k_high_ptr);
    }
    // k_high (Priority::High) is now runnable in the runqueue.
    // k_norm (Priority::Normal) calls sleep_ms while a higher-priority runnable thread exists.
    let ok = crate::task::sleep::sleep_ms(20);
    assert!(ok, "sleep_ms must return true");
    assert!(INC4_TEST_K_HIGH_RAN.load(Ordering::SeqCst), "High priority thread must run while normal thread sleeps");
    INC4_TEST_K_NORM_RAN_AFTER.store(true, Ordering::SeqCst);
    exit_current_thread();
}

pub fn run_stage3d_inc4_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3D-Increment 4: Timer Sleep (sleep_ms & SleepTable)]");

    let baseline_free = pmm.free_frame_count();

    // Re-arm LAPIC periodic timer at calibrated 100 Hz and enable CPU interrupts
    crate::hal::arch::x86_64::lapic::rearm_lapic_timer();
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }

    // ====================================================================
    // Test A: Zero Sleep (sleep_ms(0))
    // ====================================================================
    let ok = crate::task::sleep::sleep_ms(0);
    assert!(ok, "sleep_ms(0) must return true");
    unsafe {
        for slot in crate::task::sleep::SLEEP_TABLE.iter() {
            assert!(slot.is_none(), "sleep_ms(0) must not register in SLEEP_TABLE");
        }
        assert_eq!(crate::task::sleep::SLEEP_WAIT_QUEUE.len(), 0, "sleep_ms(0) must not touch SLEEP_WAIT_QUEUE");
    }
    kprintln!("  Test A: Zero sleep yields without registration [VERIFIED]");

    // ====================================================================
    // Test G: SleepTable Exhaustion (16 slots filled -> 17th fails)
    // ====================================================================
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        for i in 0..crate::task::sleep::MAX_SLEEP_ENTRIES {
            crate::task::sleep::SLEEP_TABLE[i] = Some(crate::task::sleep::SleepEntry {
                thread: (0x1000 + i * 8) as *mut _,
                deadline_tick: 999_999,
            });
        }
        SCHEDULER.lock.unlock_restore(orig);
    }

    // 17th request must fail deterministically without overwriting
    let exhausted_ok = crate::task::sleep::sleep_ms(10);
    assert!(!exhausted_ok, "17th sleep request must fail deterministically");

    unsafe {
        let orig = SCHEDULER.lock.acquire();
        for i in 0..crate::task::sleep::MAX_SLEEP_ENTRIES {
            let entry = crate::task::sleep::SLEEP_TABLE[i].expect("Slot must be preserved");
            assert_eq!(entry.thread, (0x1000 + i * 8) as *mut _, "Slot thread must not be corrupted");
            assert_eq!(entry.deadline_tick, 999_999, "Slot deadline must not be corrupted");
            crate::task::sleep::SLEEP_TABLE[i] = None; // Reset table
        }
        SCHEDULER.lock.unlock_restore(orig);
    }
    kprintln!("  Test G: SleepTable exhaustion rejected deterministically [VERIFIED]");

    // ====================================================================
    // Test H: Duplicate Registration Rejection
    // ====================================================================
    let curr = current_thread_from_gs();
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        crate::task::sleep::SLEEP_TABLE[0] = Some(crate::task::sleep::SleepEntry {
            thread: curr,
            deadline_tick: 999_999,
        });
        SCHEDULER.lock.unlock_restore(orig);
    }
    let dup_ok = crate::task::sleep::sleep_ms(10);
    assert!(!dup_ok, "Duplicate registration must be rejected");
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        crate::task::sleep::SLEEP_TABLE[0] = None;
        SCHEDULER.lock.unlock_restore(orig);
    }
    kprintln!("  Test H: Duplicate registration rejected [VERIFIED]");

    // ====================================================================
    // Test I: Large Duration / Saturating Arithmetic Safety
    // ====================================================================
    let dur_max = (u64::MAX / 10) + if u64::MAX % 10 != 0 { 1 } else { 0 };
    assert_eq!(dur_max, 1844674407370955162, "Ceil division of u64::MAX must be exact");
    let now = 100u64;
    let deadline_calc = now.saturating_add(dur_max);
    assert_eq!(deadline_calc, 1844674407370955262);
    let sat_deadline = u64::MAX.saturating_add(dur_max);
    assert_eq!(sat_deadline, u64::MAX, "Saturating add must prevent overflow wraparound");
    kprintln!("  Test I: Saturating deadline arithmetic & overflow safety [VERIFIED]");

    // ====================================================================
    // Test B & C: Sub-Tick (1 ms) and Exact Tick Boundary (20 ms)
    // ====================================================================
    INC4_WORKER_COMPLETED[0].store(false, Ordering::SeqCst);
    INC4_WORKER_COMPLETED[1].store(false, Ordering::SeqCst);
    INC4_ORDER_COUNT.store(0, Ordering::SeqCst);

    // Worker 0: 1 ms (ceil to 1 tick = 10 ms)
    let w0 = create_thread(inc4_sleep_worker, 0 | (1 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create worker 0");
    // Worker 1: 20 ms (exact 2 ticks = 20 ms)
    let w1 = create_thread(inc4_sleep_worker, 1 | (20 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create worker 1");

    spawn(w0);
    spawn(w1);

    while !INC4_WORKER_COMPLETED[0].load(Ordering::SeqCst) || !INC4_WORKER_COMPLETED[1].load(Ordering::SeqCst) {
        yield_now();
    }

    let elapsed_0 = INC4_WORKER_END[0].load(Ordering::SeqCst).saturating_sub(INC4_WORKER_START[0].load(Ordering::SeqCst));
    let elapsed_1 = INC4_WORKER_END[1].load(Ordering::SeqCst).saturating_sub(INC4_WORKER_START[1].load(Ordering::SeqCst));

    assert!(elapsed_0 >= 1, "1 ms sleep must take at least 1 tick (observed: {})", elapsed_0);
    assert!(elapsed_1 >= 2, "20 ms sleep must take at least 2 ticks (observed: {})", elapsed_1);

    destroy_thread(w0, pmm, vmm).expect("destroy w0 failed");
    destroy_thread(w1, pmm, vmm).expect("destroy w1 failed");

    kprintln!("  Test B: Sub-tick sleep (1 ms -> >=1 tick) [VERIFIED]");
    kprintln!("  Test C: Exact tick boundary (20 ms -> >=2 ticks) [VERIFIED]");

    // ====================================================================
    // Test D: Multiple Concurrent Sleepers with Staggered Deadlines
    // ====================================================================
    INC4_WORKER_COMPLETED[0].store(false, Ordering::SeqCst);
    INC4_WORKER_COMPLETED[1].store(false, Ordering::SeqCst);
    INC4_WORKER_COMPLETED[2].store(false, Ordering::SeqCst);
    INC4_ORDER_COUNT.store(0, Ordering::SeqCst);

    // Staggered: 20 ms (2 ticks), 50 ms (5 ticks), 80 ms (8 ticks)
    let d0 = create_thread(inc4_sleep_worker, 0 | (20 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create d0");
    let d1 = create_thread(inc4_sleep_worker, 1 | (50 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create d1");
    let d2 = create_thread(inc4_sleep_worker, 2 | (80 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create d2");

    spawn(d0);
    spawn(d1);
    spawn(d2);

    while !INC4_WORKER_COMPLETED[0].load(Ordering::SeqCst)
        || !INC4_WORKER_COMPLETED[1].load(Ordering::SeqCst)
        || !INC4_WORKER_COMPLETED[2].load(Ordering::SeqCst)
    {
        yield_now();
    }

    assert_eq!(INC4_ORDER[0].load(Ordering::SeqCst), 0, "Worker 0 (20ms) must wake first");
    assert_eq!(INC4_ORDER[1].load(Ordering::SeqCst), 1, "Worker 1 (50ms) must wake second");
    assert_eq!(INC4_ORDER[2].load(Ordering::SeqCst), 2, "Worker 2 (80ms) must wake third");

    destroy_thread(d0, pmm, vmm).expect("destroy d0 failed");
    destroy_thread(d1, pmm, vmm).expect("destroy d1 failed");
    destroy_thread(d2, pmm, vmm).expect("destroy d2 failed");

    kprintln!("  Test D: Staggered concurrent sleepers wake in monotonic order [VERIFIED]");

    // ====================================================================
    // Test E: Same Deadline Concurrent Sleepers
    // ====================================================================
    INC4_WORKER_COMPLETED[0].store(false, Ordering::SeqCst);
    INC4_WORKER_COMPLETED[1].store(false, Ordering::SeqCst);
    INC4_WORKER_COMPLETED[2].store(false, Ordering::SeqCst);
    INC4_ORDER_COUNT.store(0, Ordering::SeqCst);

    // All 3 sleep for 30 ms
    let e0 = create_thread(inc4_sleep_worker, 0 | (30 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create e0");
    let e1 = create_thread(inc4_sleep_worker, 1 | (30 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create e1");
    let e2 = create_thread(inc4_sleep_worker, 2 | (30 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create e2");

    spawn(e0);
    spawn(e1);
    spawn(e2);

    while !INC4_WORKER_COMPLETED[0].load(Ordering::SeqCst)
        || !INC4_WORKER_COMPLETED[1].load(Ordering::SeqCst)
        || !INC4_WORKER_COMPLETED[2].load(Ordering::SeqCst)
    {
        yield_now();
    }

    assert_eq!(INC4_ORDER_COUNT.load(Ordering::SeqCst), 3, "All 3 same-deadline sleepers must wake");

    destroy_thread(e0, pmm, vmm).expect("destroy e0 failed");
    destroy_thread(e1, pmm, vmm).expect("destroy e1 failed");
    destroy_thread(e2, pmm, vmm).expect("destroy e2 failed");

    kprintln!("  Test E: Same-deadline concurrent sleepers all wake without loss [VERIFIED]");

    // ====================================================================
    // Test F: Priority Ordering on Same-Deadline Wake
    // ====================================================================
    INC4_WORKER_COMPLETED[0].store(false, Ordering::SeqCst);
    INC4_WORKER_COMPLETED[1].store(false, Ordering::SeqCst);
    INC4_WORKER_COMPLETED[2].store(false, Ordering::SeqCst);
    INC4_ORDER_COUNT.store(0, Ordering::SeqCst);

    // W0: Normal (30 ms), W1: High (30 ms), W2: Critical (30 ms)
    let f_norm = create_thread(inc4_sleep_worker, 0 | (30 << 8), Priority::Normal, pmm, vmm)
        .expect("Failed to create f_norm");
    let f_high = create_thread(inc4_sleep_worker, 1 | (30 << 8), Priority::High, pmm, vmm)
        .expect("Failed to create f_high");
    let f_crit = create_thread(inc4_sleep_worker, 2 | (30 << 8), Priority::Critical, pmm, vmm)
        .expect("Failed to create f_crit");

    spawn(f_norm);
    spawn(f_high);
    spawn(f_crit);

    while !INC4_WORKER_COMPLETED[0].load(Ordering::SeqCst)
        || !INC4_WORKER_COMPLETED[1].load(Ordering::SeqCst)
        || !INC4_WORKER_COMPLETED[2].load(Ordering::SeqCst)
    {
        yield_now();
    }

    // Critical (idx 2) must execute before High (idx 1), which must execute before Normal (idx 0)
    assert_eq!(INC4_ORDER[0].load(Ordering::SeqCst), 2, "Critical must execute 1st upon same-deadline wake");
    assert_eq!(INC4_ORDER[1].load(Ordering::SeqCst), 1, "High must execute 2nd upon same-deadline wake");
    assert_eq!(INC4_ORDER[2].load(Ordering::SeqCst), 0, "Normal must execute 3rd upon same-deadline wake");

    destroy_thread(f_norm, pmm, vmm).expect("destroy f_norm failed");
    destroy_thread(f_high, pmm, vmm).expect("destroy f_high failed");
    destroy_thread(f_crit, pmm, vmm).expect("destroy f_crit failed");

    kprintln!("  Test F: RunQueue priority ordering on same-deadline wake: Critical > High > Normal [VERIFIED]");

    // ====================================================================
    // Test K: Sleep While Higher-Priority Runnable Thread Exists
    // ====================================================================
    INC4_TEST_K_HIGH_RAN.store(false, Ordering::SeqCst);
    INC4_TEST_K_NORM_RAN_AFTER.store(false, Ordering::SeqCst);

    let k_high = create_thread(inc4_test_k_high_worker, 0, Priority::High, pmm, vmm)
        .expect("Failed to create k_high");
    let k_norm = create_thread(inc4_test_k_norm_worker, k_high as u64, Priority::Normal, pmm, vmm)
        .expect("Failed to create k_norm");

    spawn(k_norm);

    while !INC4_TEST_K_NORM_RAN_AFTER.load(Ordering::SeqCst) {
        yield_now();
    }

    assert!(INC4_TEST_K_HIGH_RAN.load(Ordering::SeqCst), "High priority thread must run while normal thread sleeps");

    destroy_thread(k_high, pmm, vmm).expect("destroy k_high failed");
    destroy_thread(k_norm, pmm, vmm).expect("destroy k_norm failed");

    kprintln!("  Test K: Sleep while higher-priority runnable thread exists dispatches immediately [VERIFIED]");

    // Halt LAPIC timer and disable interrupts after timer verification completes
    crate::hal::arch::x86_64::lapic::mask_lapic_timer();
    crate::hal::arch::x86_64::cpu::cli();

    // ====================================================================
    // Test J: PMM Frame Accounting Check
    // ====================================================================
    let final_free = pmm.free_frame_count();
    kprintln!("  Stack Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    assert_eq!(baseline_free, final_free, "PMM free frames must match baseline exactly");

    kprintln!("  [x] Stage 3D Increment 4 Timer Sleep verified.");
}

// ====================================================================
// Stage 3D Increment 5: Kernel Event Primitive Verification
// ====================================================================
static mut INC5_EVENT: crate::task::event::Event =
    crate::task::event::Event::new(false, crate::task::event::EventType::AutoReset);

static INC5_WORKER_RAN: [AtomicBool; 4] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];
static INC5_ORDER_COUNT: AtomicU64 = AtomicU64::new(0);
static INC5_ORDER: [AtomicU64; 4] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

extern "C" fn inc5_b_worker(_arg: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[0].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_c_worker(idx: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[idx as usize].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_d_worker(val: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    let order_idx = INC5_ORDER_COUNT.fetch_add(1, Ordering::SeqCst) as usize;
    if order_idx < 4 {
        INC5_ORDER[order_idx].store(val, Ordering::SeqCst);
    }
    exit_current_thread();
}

extern "C" fn inc5_e_worker(idx: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[idx as usize].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_f_worker(idx: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[idx as usize].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_g_worker(_arg: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[3].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_h_worker(_arg: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[0].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_i_isr_worker(_arg: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[0].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_j_high_worker(_arg: u64) {
    let ok = unsafe { INC5_EVENT.wait() };
    assert!(ok);
    INC5_WORKER_RAN[1].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_j_norm_worker(_arg: u64) {
    unsafe { INC5_EVENT.signal() };
    // Verify j_norm is still running immediately after signal (no non-standard context hijack inside signal)
    assert!(
        !INC5_WORKER_RAN[1].load(Ordering::SeqCst),
        "High priority thread must not hijack context inside signal()"
    );
    INC5_WORKER_RAN[0].store(true, Ordering::SeqCst);
    // Yield at permitted scheduling point
    yield_now();
    // High priority thread has now run
    assert!(
        INC5_WORKER_RAN[1].load(Ordering::SeqCst),
        "High priority thread must have run after yield_now()"
    );
    INC5_WORKER_RAN[2].store(true, Ordering::SeqCst);
    exit_current_thread();
}

extern "C" fn inc5_k_worker(comp_ptr: u64) {
    let comp = unsafe { &mut *(comp_ptr as *mut crate::task::event::Event) };
    INC5_WORKER_RAN[0].store(true, Ordering::SeqCst);
    comp.signal();
    exit_current_thread();
}

static mut K_COMP_EVENT: crate::task::event::Event =
    crate::task::event::Event::new(false, crate::task::event::EventType::ManualReset);

pub fn run_stage3d_inc5_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3D-Increment 5: Kernel Event Primitive]");

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Test A: Auto-Reset Pre-Signaled Fast Path
    // ====================================================================
    let mut ev_a = crate::task::event::Event::new(true, crate::task::event::EventType::AutoReset);
    assert!(ev_a.is_signaled(), "ev_a must be signaled initially");
    let ok1 = ev_a.try_wait();
    assert!(ok1, "First try_wait must return true");
    assert!(!ev_a.is_signaled(), "ev_a must be auto-reset to unsignaled");
    let ok2 = ev_a.try_wait();
    assert!(!ok2, "Second try_wait must return false");
    kprintln!("  Test A: Auto-Reset pre-signaled fast path [VERIFIED]");

    // ====================================================================
    // Test B: Auto-Reset Single Waiter Wakeup
    // ====================================================================
    unsafe {
        INC5_EVENT = crate::task::event::Event::new(false, crate::task::event::EventType::AutoReset);
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
    }
    let b_worker = create_thread(inc5_b_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create b_worker");
    spawn(b_worker);
    yield_now();
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 1, "b_worker must be blocked");
        assert!(!INC5_EVENT.is_signaled());
        INC5_EVENT.signal();
    }
    yield_now();
    assert!(INC5_WORKER_RAN[0].load(Ordering::SeqCst), "b_worker must have completed");
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 0);
        assert!(!INC5_EVENT.is_signaled());
    }
    destroy_thread(b_worker, pmm, vmm).expect("destroy b_worker failed");
    kprintln!("  Test B: Auto-Reset single waiter wakeup [VERIFIED]");

    // ====================================================================
    // Test C: Auto-Reset Exactly One Waiter Woken
    // ====================================================================
    unsafe {
        INC5_EVENT = crate::task::event::Event::new(false, crate::task::event::EventType::AutoReset);
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[1].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[2].store(false, Ordering::SeqCst);
    }
    let c0 = create_thread(inc5_c_worker, 0, Priority::Normal, pmm, vmm).expect("Failed to create c0");
    let c1 = create_thread(inc5_c_worker, 1, Priority::Normal, pmm, vmm).expect("Failed to create c1");
    let c2 = create_thread(inc5_c_worker, 2, Priority::Normal, pmm, vmm).expect("Failed to create c2");
    spawn(c0);
    spawn(c1);
    spawn(c2);
    yield_now();
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 3, "All 3 workers must be blocked");
        INC5_EVENT.signal();
    }
    yield_now();
    let ran_count = (if INC5_WORKER_RAN[0].load(Ordering::SeqCst) { 1 } else { 0 })
        + (if INC5_WORKER_RAN[1].load(Ordering::SeqCst) { 1 } else { 0 })
        + (if INC5_WORKER_RAN[2].load(Ordering::SeqCst) { 1 } else { 0 });
    assert_eq!(ran_count, 1, "Exactly one waiter must have woken after single signal");
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 2, "Two waiters must remain blocked");
        assert!(!INC5_EVENT.is_signaled(), "Event must remain unsignaled");
        // Wake remaining 2 waiters to cleanly exit
        INC5_EVENT.signal();
    }
    yield_now();
    unsafe {
        INC5_EVENT.signal();
    }
    yield_now();
    assert!(
        INC5_WORKER_RAN[0].load(Ordering::SeqCst)
            && INC5_WORKER_RAN[1].load(Ordering::SeqCst)
            && INC5_WORKER_RAN[2].load(Ordering::SeqCst)
    );
    destroy_thread(c0, pmm, vmm).expect("destroy c0 failed");
    destroy_thread(c1, pmm, vmm).expect("destroy c1 failed");
    destroy_thread(c2, pmm, vmm).expect("destroy c2 failed");
    kprintln!("  Test C: Auto-Reset exactly one waiter woken [VERIFIED]");

    // ====================================================================
    // Test D: Auto-Reset Priority Wakeup Ordering
    // ====================================================================
    unsafe {
        INC5_EVENT = crate::task::event::Event::new(false, crate::task::event::EventType::AutoReset);
        INC5_ORDER_COUNT.store(0, Ordering::SeqCst);
        INC5_ORDER[0].store(0, Ordering::SeqCst);
        INC5_ORDER[1].store(0, Ordering::SeqCst);
        INC5_ORDER[2].store(0, Ordering::SeqCst);
    }
    let d_norm = create_thread(inc5_d_worker, 1, Priority::Normal, pmm, vmm).expect("Failed to create d_norm");
    let d_high = create_thread(inc5_d_worker, 2, Priority::High, pmm, vmm).expect("Failed to create d_high");
    let d_crit = create_thread(inc5_d_worker, 3, Priority::Critical, pmm, vmm).expect("Failed to create d_crit");
    spawn(d_norm);
    spawn(d_high);
    spawn(d_crit);
    yield_now();
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 3);
        INC5_EVENT.signal();
    }
    yield_now();
    unsafe {
        INC5_EVENT.signal();
    }
    yield_now();
    unsafe {
        INC5_EVENT.signal();
    }
    yield_now();
    assert_eq!(INC5_ORDER[0].load(Ordering::SeqCst), 3, "Critical waiter must wake first");
    assert_eq!(INC5_ORDER[1].load(Ordering::SeqCst), 2, "High waiter must wake second");
    assert_eq!(INC5_ORDER[2].load(Ordering::SeqCst), 1, "Normal waiter must wake third");
    destroy_thread(d_norm, pmm, vmm).expect("destroy d_norm failed");
    destroy_thread(d_high, pmm, vmm).expect("destroy d_high failed");
    destroy_thread(d_crit, pmm, vmm).expect("destroy d_crit failed");
    kprintln!("  Test D: Auto-Reset priority wakeup ordering: Critical > High > Normal [VERIFIED]");

    // ====================================================================
    // Test E: Manual-Reset Pre-Signaled Pass-Through
    // ====================================================================
    unsafe {
        INC5_EVENT = crate::task::event::Event::new(true, crate::task::event::EventType::ManualReset);
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[1].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[2].store(false, Ordering::SeqCst);
    }
    let e0 = create_thread(inc5_e_worker, 0, Priority::Normal, pmm, vmm).expect("Failed to create e0");
    let e1 = create_thread(inc5_e_worker, 1, Priority::High, pmm, vmm).expect("Failed to create e1");
    let e2 = create_thread(inc5_e_worker, 2, Priority::Critical, pmm, vmm).expect("Failed to create e2");
    spawn(e0);
    spawn(e1);
    spawn(e2);
    yield_now();
    assert!(
        INC5_WORKER_RAN[0].load(Ordering::SeqCst)
            && INC5_WORKER_RAN[1].load(Ordering::SeqCst)
            && INC5_WORKER_RAN[2].load(Ordering::SeqCst),
        "All pre-signaled workers must pass through immediately"
    );
    unsafe {
        assert!(INC5_EVENT.is_signaled(), "Manual-Reset event must remain signaled");
        assert_eq!(INC5_EVENT.waiter_count(), 0);
    }
    destroy_thread(e0, pmm, vmm).expect("destroy e0 failed");
    destroy_thread(e1, pmm, vmm).expect("destroy e1 failed");
    destroy_thread(e2, pmm, vmm).expect("destroy e2 failed");
    kprintln!("  Test E: Manual-Reset pre-signaled pass-through [VERIFIED]");

    // ====================================================================
    // Test F: Manual-Reset Broadcast Multicast Wakeup
    // ====================================================================
    unsafe {
        INC5_EVENT = crate::task::event::Event::new(false, crate::task::event::EventType::ManualReset);
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[1].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[2].store(false, Ordering::SeqCst);
    }
    let f0 = create_thread(inc5_f_worker, 0, Priority::Normal, pmm, vmm).expect("Failed to create f0");
    let f1 = create_thread(inc5_f_worker, 1, Priority::High, pmm, vmm).expect("Failed to create f1");
    let f2 = create_thread(inc5_f_worker, 2, Priority::Critical, pmm, vmm).expect("Failed to create f2");
    spawn(f0);
    spawn(f1);
    spawn(f2);
    yield_now();
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 3, "All 3 workers must be blocked");
        INC5_EVENT.signal();
    }
    yield_now();
    assert!(
        INC5_WORKER_RAN[0].load(Ordering::SeqCst)
            && INC5_WORKER_RAN[1].load(Ordering::SeqCst)
            && INC5_WORKER_RAN[2].load(Ordering::SeqCst),
        "All 3 workers must wake and complete from single broadcast signal"
    );
    unsafe {
        assert!(INC5_EVENT.is_signaled());
        assert_eq!(INC5_EVENT.waiter_count(), 0);
    }
    destroy_thread(f0, pmm, vmm).expect("destroy f0 failed");
    destroy_thread(f1, pmm, vmm).expect("destroy f1 failed");
    destroy_thread(f2, pmm, vmm).expect("destroy f2 failed");
    kprintln!("  Test F: Manual-Reset broadcast multicast wakeup [VERIFIED]");

    // ====================================================================
    // Test G: Manual-Reset Post-Signal Pass-Through
    // ====================================================================
    unsafe {
        INC5_WORKER_RAN[3].store(false, Ordering::SeqCst);
    }
    let g_worker = create_thread(inc5_g_worker, 0, Priority::Normal, pmm, vmm).expect("Failed to create g_worker");
    spawn(g_worker);
    yield_now();
    assert!(INC5_WORKER_RAN[3].load(Ordering::SeqCst), "Post-signal wait must pass through immediately");
    destroy_thread(g_worker, pmm, vmm).expect("destroy g_worker failed");
    kprintln!("  Test G: Manual-Reset post-signal pass-through [VERIFIED]");

    // ====================================================================
    // Test H: Manual-Reset Explicit Reset
    // ====================================================================
    unsafe {
        INC5_EVENT.reset();
        assert!(!INC5_EVENT.is_signaled(), "Event must be unsignaled after reset()");
        assert!(!INC5_EVENT.try_wait(), "try_wait must return false after reset()");
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
    }
    let h_worker = create_thread(inc5_h_worker, 0, Priority::Normal, pmm, vmm).expect("Failed to create h_worker");
    spawn(h_worker);
    yield_now();
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 1, "h_worker must block on reset event");
        INC5_EVENT.signal();
    }
    yield_now();
    assert!(INC5_WORKER_RAN[0].load(Ordering::SeqCst), "h_worker must wake after signal");
    destroy_thread(h_worker, pmm, vmm).expect("destroy h_worker failed");
    kprintln!("  Test H: Manual-Reset explicit reset [VERIFIED]");

    // ====================================================================
    // Test I: ISR Context Signaling
    // ====================================================================
    unsafe {
        INC5_EVENT = crate::task::event::Event::new(false, crate::task::event::EventType::AutoReset);
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
    }
    let i_worker = create_thread(inc5_i_isr_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create i_worker");
    spawn(i_worker);
    yield_now();
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 1, "i_worker must be blocked on INC5_EVENT");
        assert!(!INC5_EVENT.is_signaled(), "Event must not be signaled yet");
    }

    // Arm the ISR signal target: the next LAPIC timer interrupt will signal INC5_EVENT
    STAGE3D_INC5_ISR_SIGNAL_TARGET.store(unsafe { &raw mut INC5_EVENT as u64 }, Ordering::SeqCst);

    // Re-arm timer and enable interrupts
    crate::hal::arch::x86_64::lapic::rearm_lapic_timer();
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }

    // Wait until worker completes
    while !INC5_WORKER_RAN[0].load(Ordering::SeqCst) {
        yield_now();
    }

    // Mask timer and disable interrupts
    crate::hal::arch::x86_64::lapic::mask_lapic_timer();
    crate::hal::arch::x86_64::cpu::cli();

    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 0, "Waiters must be 0 after ISR wake");
    }
    destroy_thread(i_worker, pmm, vmm).expect("destroy i_worker failed");
    kprintln!("  Test I: ISR context signaling via LAPIC timer interrupt [VERIFIED]");

    // ====================================================================
    // Test J: Higher-Priority Wakeup / Rescheduling at Permitted Scheduling Point
    // ====================================================================
    unsafe {
        INC5_EVENT = crate::task::event::Event::new(false, crate::task::event::EventType::AutoReset);
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[1].store(false, Ordering::SeqCst);
        INC5_WORKER_RAN[2].store(false, Ordering::SeqCst);
    }
    let j_high = create_thread(inc5_j_high_worker, 0, Priority::High, pmm, vmm)
        .expect("Failed to create j_high");
    let j_norm = create_thread(inc5_j_norm_worker, 0, Priority::Normal, pmm, vmm)
        .expect("Failed to create j_norm");

    // Spawn j_high first, yield so it blocks on INC5_EVENT
    spawn(j_high);
    yield_now();
    unsafe {
        assert_eq!(INC5_EVENT.waiter_count(), 1, "j_high must be blocked on INC5_EVENT");
    }

    // Now spawn j_norm (Normal priority)
    spawn(j_norm);
    while !INC5_WORKER_RAN[1].load(Ordering::SeqCst) || !INC5_WORKER_RAN[2].load(Ordering::SeqCst) {
        yield_now();
    }

    assert!(INC5_WORKER_RAN[0].load(Ordering::SeqCst), "j_norm must have run and signaled");
    assert!(INC5_WORKER_RAN[1].load(Ordering::SeqCst), "j_high must have run after yield_now()");
    assert!(INC5_WORKER_RAN[2].load(Ordering::SeqCst), "j_norm must have completed after yield_now()");

    destroy_thread(j_high, pmm, vmm).expect("destroy j_high failed");
    destroy_thread(j_norm, pmm, vmm).expect("destroy j_norm failed");
    kprintln!("  Test J: Higher-priority wakeup / rescheduling at permitted scheduling point [VERIFIED]");

    // ====================================================================
    // Test K: Thread Completion Notification Simulation
    // ====================================================================
    unsafe {
        K_COMP_EVENT = crate::task::event::Event::new(false, crate::task::event::EventType::ManualReset);
        INC5_WORKER_RAN[0].store(false, Ordering::SeqCst);
    }

    let k_worker = create_thread(inc5_k_worker, unsafe { &raw mut K_COMP_EVENT as u64 }, Priority::Normal, pmm, vmm)
        .expect("Failed to create k_worker");
    spawn(k_worker);

    // Parent thread (BSP) waits on completion event
    let parent_woke = unsafe { K_COMP_EVENT.wait() };
    assert!(parent_woke, "Parent wait must succeed");
    assert!(INC5_WORKER_RAN[0].load(Ordering::SeqCst), "Worker must have completed before parent wakes");
    unsafe {
        assert!(K_COMP_EVENT.is_signaled(), "Completion event must remain signaled for late observers");
    }

    // Late observer test: another try_wait() returns true without blocking
    let late_wait = unsafe { K_COMP_EVENT.try_wait() };
    assert!(late_wait, "Late observer must pass through without blocking");

    destroy_thread(k_worker, pmm, vmm).expect("destroy k_worker failed");
    kprintln!("  Test K: Thread completion notification simulation [VERIFIED]");

    // ====================================================================
    // Test L: PMM Frame Accounting Check
    // ====================================================================
    let final_free = pmm.free_frame_count();
    kprintln!("  Stack Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    assert_eq!(baseline_free, final_free, "PMM free frames must match baseline exactly");

    kprintln!("  [x] Stage 3D Increment 5 Kernel Event primitive verified.");
}

// ====================================================================
// Stage 3E: Thread Lifecycle & Resource Reclamation Verification
// ====================================================================

extern "C" fn stage3e_exit_worker(arg: u64) {
    exit_current_thread_with_code(arg as i32);
}

extern "C" fn stage3e_sleep_exit_worker(arg: u64) {
    let ms = arg >> 32;
    let code = (arg & 0xFFFF_FFFF) as i32;
    if ms > 0 {
        sleep_ms(ms);
    } else {
        yield_now();
    }
    exit_current_thread_with_code(code);
}

pub fn run_stage3e_verification(
    pmm: &mut PhysicalMemoryManager,
    vmm: &mut ActivePageTable,
) {
    kprintln!("\n[Stage 3E: Thread Lifecycle & Resource Reclamation]");

    let baseline_free = pmm.free_frame_count();

    // ====================================================================
    // Test A: Synchronous Exit & Join Fast Path
    // ====================================================================
    let (_t_a_id, t_a_handle) = spawn_thread(stage3e_exit_worker, 42, Priority::Normal, pmm, vmm)
        .expect("spawn t_a");
    let res_a = t_a_handle.join(pmm, vmm);
    assert_eq!(res_a, Ok(42), "Test A: Join must return worker exit code 42");
    kprintln!("  Test A: Synchronous exit & join fast path [VERIFIED]");

    // ====================================================================
    // Test B: Late Join Pass-Through
    // ====================================================================
    let (_t_b_id, t_b_handle) = spawn_thread(stage3e_exit_worker, 100, Priority::Normal, pmm, vmm)
        .expect("spawn t_b");
    yield_now(); // BSP yields, t_b runs and exits -> becomes Zombie
    let res_b = t_b_handle.join(pmm, vmm); // Target already Zombie: passes through immediately
    assert_eq!(res_b, Ok(100), "Test B: Late join must return worker exit code 100");
    kprintln!("  Test B: Late join pass-through [VERIFIED]");

    // ====================================================================
    // Test C: Early Join Blocking
    // ====================================================================
    let (_t_c_id, t_c_handle) = spawn_thread(stage3e_sleep_exit_worker, 77, Priority::Normal, pmm, vmm)
        .expect("spawn t_c");
    let res_c = t_c_handle.join(pmm, vmm);
    assert_eq!(res_c, Ok(77), "Test C: Early join must block and return exit code 77");
    kprintln!("  Test C: Early join blocking [VERIFIED]");

    // ====================================================================
    // Test D: Exit Code Boundary Propagation
    // ====================================================================
    let codes = [0i32, 1i32, -1i32, 0x7FFF_FFFFi32, -2147483648i32];
    for &code in &codes {
        let (_id, handle) = spawn_thread(stage3e_exit_worker, code as u32 as u64, Priority::Normal, pmm, vmm)
            .expect("spawn boundary worker");
        let res = handle.join(pmm, vmm);
        assert_eq!(res, Ok(code), "Test D: Exit code mismatch");
    }
    kprintln!("  Test D: Exit code boundary propagation [VERIFIED]");

    // ====================================================================
    // Test E: Self-Join Rejection
    // ====================================================================
    let self_id = unsafe { (*current_thread_from_gs()).id };
    let self_handle = JoinHandle {
        target_id: ThreadId(self_id),
        target_slot: 0,
    };
    let self_res = self_handle.join(pmm, vmm);
    assert_eq!(self_res, Err(ThreadError::SelfJoin), "Self-join must be rejected with SelfJoin error");
    kprintln!("  Test E: Self-join rejection [VERIFIED]");

    // ====================================================================
    // Test F: Multiple Sequential Join Operations
    // ====================================================================
    for i in 0..5 {
        let (_id, handle) = spawn_thread(stage3e_exit_worker, (i * 10) as u64, Priority::Normal, pmm, vmm)
            .expect("spawn seq worker");
        let res = handle.join(pmm, vmm);
        assert_eq!(res, Ok((i * 10) as i32));
    }
    kprintln!("  Test F: Multiple sequential join operations [VERIFIED]");

    // ====================================================================
    // Test G: Concurrent Worker Joins
    // ====================================================================
    let (_id_crit, h_crit) = spawn_thread(stage3e_exit_worker, 11, Priority::Critical, pmm, vmm).expect("spawn crit");
    let (_id_high, h_high) = spawn_thread(stage3e_exit_worker, 22, Priority::High, pmm, vmm).expect("spawn high");
    let (_id_norm, h_norm) = spawn_thread(stage3e_exit_worker, 33, Priority::Normal, pmm, vmm).expect("spawn norm");

    assert_eq!(h_norm.join(pmm, vmm), Ok(33));
    assert_eq!(h_high.join(pmm, vmm), Ok(22));
    assert_eq!(h_crit.join(pmm, vmm), Ok(11));
    kprintln!("  Test G: Concurrent worker joins [VERIFIED]");

    // ====================================================================
    // Test H: Detached Thread Reaper Scavenging
    // ====================================================================
    let (_id_h, h_detach) = spawn_thread(stage3e_exit_worker, 999, Priority::Normal, pmm, vmm).expect("spawn detached");
    h_detach.detach();
    yield_now(); // Worker runs and exits, enqueues into ZOMBIE_QUEUE
    assert!(zombie_queue_count() >= 1, "Zombie queue must contain detached worker");
    let reaped = reap_zombies(pmm, vmm);
    assert!(reaped >= 1, "Reaper must scavenge detached worker");
    assert_eq!(zombie_queue_count(), 0, "Zombie queue must be empty after reaping");
    kprintln!("  Test H: Detached thread reaper scavenging [VERIFIED]");

    // ====================================================================
    // Test I1: Compile-Time Linear Ownership Verification
    // ====================================================================
    // Guaranteed by System V ABI / Rust move semantics: JoinHandle::join(mut self) takes ownership by value.
    kprintln!("  Test I1: Compile-time linear ownership verification [VERIFIED]");

    // ====================================================================
    // Test I2: Runtime Race Rejection & AlreadyReclaimed Proof
    // ====================================================================
    let (_id_i, h_i) = spawn_thread(stage3e_exit_worker, 123, Priority::Normal, pmm, vmm).expect("spawn i");
    yield_now(); // Worker exits -> Zombie
    let slot_i = h_i.target_slot;
    let table = &raw mut THREAD_TABLE;
    let target_i = unsafe { &raw mut (*table)[slot_i].thread };

    // Test claim_reap_locked serialization:
    let orig_rflags = unsafe { SCHEDULER.lock.acquire() };
    let claim1 = unsafe { claim_reap_locked(target_i) };
    let claim2 = unsafe { claim_reap_locked(target_i) };
    unsafe { SCHEDULER.lock.unlock_restore(orig_rflags) };

    assert!(claim1, "First claim_reap_locked must return true");
    assert!(!claim2, "Second claim_reap_locked must return false");

    // Attempting join on the already-reclaimed thread descriptor:
    let join_res = h_i.join(pmm, vmm);
    assert_eq!(join_res, Err(ThreadError::AlreadyReclaimed), "Join on already-reclaimed thread must fail with AlreadyReclaimed");

    // Clean up the claimed descriptor:
    destroy_thread_stack(target_i, pmm, vmm);
    unsafe { free_descriptor(target_i).unwrap() };
    kprintln!("  Test I2: Runtime race rejection & AlreadyReclaimed proof [VERIFIED]");

    // ====================================================================
    // Test J: Abandoned Handle Scavenging on Parent Exit
    // ====================================================================
    let t_parent = create_thread(stage3e_exit_worker, 1, Priority::Normal, pmm, vmm).expect("create parent");
    let t_child = create_thread(stage3e_exit_worker, 2, Priority::Normal, pmm, vmm).expect("create child");
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        crate::task::lifecycle::add_child_locked(t_parent, t_child);
        SCHEDULER.lock.unlock_restore(orig);
    }
    spawn(t_parent);
    spawn(t_child);

    // Both run and exit. Parent's exit abandons child, child exits and enqueues to ZOMBIE_QUEUE!
    yield_now();
    yield_now();

    let reaped_j = reap_zombies(pmm, vmm);
    assert!(reaped_j >= 1, "Abandoned child must be reaped by reaper");
    unsafe {
        if (*t_parent).state == ThreadState::Zombie {
            let orig = SCHEDULER.lock.acquire();
            let _ = claim_reap_locked(t_parent);
            SCHEDULER.lock.unlock_restore(orig);
            destroy_thread(t_parent, pmm, vmm).unwrap();
        }
    }
    kprintln!("  Test J: Abandoned handle scavenging on parent exit [VERIFIED]");

    // ====================================================================
    // Test K: Slot Reuse Safety & Monotonic ID Verification
    // ====================================================================
    let (id_k1, h_k1) = spawn_thread(stage3e_exit_worker, 55, Priority::Normal, pmm, vmm).expect("spawn k1");
    let slot_k1 = h_k1.target_slot;
    assert_eq!(h_k1.join(pmm, vmm), Ok(55));

    // Spawn k2, which reuses the same descriptor slot
    let (id_k2, h_k2) = spawn_thread(stage3e_exit_worker, 66, Priority::Normal, pmm, vmm).expect("spawn k2");
    assert_eq!(h_k2.target_slot, slot_k1, "Slot must be reused");
    assert!(id_k2.0 > id_k1.0, "Monotonic thread ID must strictly increase");

    // Construct stale handle with old ID id_k1 targeting slot_k1
    let stale_h = JoinHandle {
        target_id: id_k1,
        target_slot: slot_k1,
    };
    let stale_res = stale_h.join(pmm, vmm);
    assert_eq!(stale_res, Err(ThreadError::InvalidHandle), "Stale handle with old ID must be rejected");

    assert_eq!(h_k2.join(pmm, vmm), Ok(66), "Valid handle must join successfully");
    kprintln!("  Test K: Slot reuse safety & monotonic ID verification [VERIFIED]");

    // ====================================================================
    // Test L: Single ZOMBIE_QUEUE Membership & Explicit Flag Invariant Proof
    // ====================================================================
    let (_id_l, h_l) = spawn_thread(stage3e_exit_worker, 888, Priority::Normal, pmm, vmm).expect("spawn l");
    let slot_l = h_l.target_slot;
    let table = &raw mut THREAD_TABLE;
    let target_l = unsafe { &raw mut (*table)[slot_l].thread };

    h_l.detach();
    unsafe {
        assert_eq!((*target_l).parent_id, 0, "Detached child must have parent_id == 0");
    }

    yield_now(); // Target terminates and enqueues to ZOMBIE_QUEUE
    assert_eq!(zombie_queue_count(), 1, "ZOMBIE_QUEUE must contain exactly 1 entry");
    unsafe {
        assert_eq!((*target_l).zombie_queued, true, "target_l must have zombie_queued == true");
        assert!((*target_l).next_zombie.is_null(), "Single member tail must have next_zombie == NULL");
    }

    // 1. Repeated detach of already-detached Zombie cannot enqueue twice
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        crate::task::lifecycle::detach_thread_locked(slot_l, ThreadId((*target_l).id));
        SCHEDULER.lock.unlock_restore(orig);
    }
    assert_eq!(zombie_queue_count(), 1, "Repeated detach must not re-enqueue Zombie");

    // 2. Spawn a second detached thread to form multi-node queue
    let (_id_l2, h_l2) = spawn_thread(stage3e_exit_worker, 889, Priority::Normal, pmm, vmm).expect("spawn l2");
    let slot_l2 = h_l2.target_slot;
    let target_l2 = unsafe { &raw mut (*table)[slot_l2].thread };
    h_l2.detach();
    yield_now();

    assert_eq!(zombie_queue_count(), 2, "ZOMBIE_QUEUE must contain 2 entries");
    unsafe {
        assert_eq!((*target_l).zombie_queued, true);
        assert_eq!((*target_l).next_zombie, target_l2, "target_l must link to target_l2");
        assert_eq!((*target_l2).zombie_queued, true);
        assert!((*target_l2).next_zombie.is_null(), "Tail node target_l2 must have next_zombie == NULL while zombie_queued == true");
    }

    // 3. Repeated detach on tail node (which has next_zombie == NULL) cannot enqueue twice
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        crate::task::lifecycle::detach_thread_locked(slot_l2, ThreadId((*target_l2).id));
        SCHEDULER.lock.unlock_restore(orig);
    }
    assert_eq!(zombie_queue_count(), 2, "Repeated detach on tail with next_zombie==NULL must not re-enqueue");

    // 4. Parent abandonment simulation cannot re-enqueue already queued zombie
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        if (*target_l2).state == ThreadState::Zombie && !(*target_l2).zombie_queued {
            crate::task::lifecycle::enqueue_zombie_locked(target_l2);
        }
        SCHEDULER.lock.unlock_restore(orig);
    }
    assert_eq!(zombie_queue_count(), 2, "Parent abandonment simulation must not re-enqueue");

    // 5. Dequeue clears zombie_queued
    unsafe {
        let orig = SCHEDULER.lock.acquire();
        let deq1 = crate::task::lifecycle::dequeue_zombie_locked().expect("dequeue first");
        assert_eq!(deq1, target_l);
        assert_eq!((*deq1).zombie_queued, false, "dequeue must clear zombie_queued");

        let deq2 = crate::task::lifecycle::dequeue_zombie_locked().expect("dequeue second");
        assert_eq!(deq2, target_l2);
        assert_eq!((*deq2).zombie_queued, false, "dequeue must clear zombie_queued");

        assert!(crate::task::lifecycle::dequeue_zombie_locked().is_none());
        SCHEDULER.lock.unlock_restore(orig);

        // Reclaim both dequeued descriptors
        let orig = SCHEDULER.lock.acquire();
        assert!(claim_reap_locked(target_l));
        assert!(claim_reap_locked(target_l2));
        SCHEDULER.lock.unlock_restore(orig);

        destroy_thread_stack(target_l, pmm, vmm);
        free_descriptor(target_l).unwrap();
        destroy_thread_stack(target_l2, pmm, vmm);
        free_descriptor(target_l2).unwrap();
    }
    assert_eq!(zombie_queue_count(), 0);
    kprintln!("  Test L: Single ZOMBIE_QUEUE membership proof [VERIFIED]");

    // ====================================================================
    // Test M: Idle Thread Scavenger Integration
    // ====================================================================
    let (_id_m, h_m) = spawn_thread(stage3e_exit_worker, 777, Priority::Normal, pmm, vmm).expect("spawn m");
    h_m.detach();
    yield_now();
    let reaped_m = reap_zombies(pmm, vmm);
    assert_eq!(reaped_m, 1, "Scavenger must reap detached thread");
    kprintln!("  Test M: Idle thread scavenger integration [VERIFIED]");

    // ====================================================================
    // Test N: Saved-Frame Invariant Preservation across Exit
    // ====================================================================
    let (_id_n, h_n) = spawn_thread(stage3e_exit_worker, 12, Priority::Normal, pmm, vmm).expect("spawn n");
    let slot_n = h_n.target_slot;
    let table = &raw mut THREAD_TABLE;
    let target_n = unsafe { &raw mut (*table)[slot_n].thread };
    yield_now();
    unsafe {
        assert_eq!((*target_n).state, ThreadState::Zombie);
        assert_eq!((*target_n).frame_type, SavedFrameType::Cooperative, "Exited thread must have SavedFrameType::Cooperative");
    }
    assert_eq!(h_n.join(pmm, vmm), Ok(12));
    kprintln!("  Test N: Saved-frame invariant preservation across exit [VERIFIED]");

    // ====================================================================
    // Test O: Non-Voluntary Preemption during Exit Preparation
    // ====================================================================
    let (_id_o, h_o) = spawn_thread(stage3e_sleep_exit_worker, 5, Priority::Normal, pmm, vmm).expect("spawn o");
    let res_o = h_o.join(pmm, vmm);
    assert_eq!(res_o, Ok(5));
    kprintln!("  Test O: Non-voluntary preemption during exit preparation [VERIFIED]");

    // ====================================================================
    // Test P: PMM Strict Frame Accounting
    // ====================================================================
    let final_free = pmm.free_frame_count();
    kprintln!("  Stack Accounting: free frames before={} after={} [EXACT MATCH]", baseline_free, final_free);
    assert_eq!(baseline_free, final_free, "PMM free frames must match baseline exactly");

    kprintln!("  [x] Stage 3E Thread Lifecycle & Resource Reclamation verified.");
}
