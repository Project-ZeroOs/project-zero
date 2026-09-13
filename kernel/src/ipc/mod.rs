//! Project Zero - Inter-Process Communication (IPC) Foundation
//!
//! Provides synchronous endpoint rendezvous messaging between execution contexts
//! without dynamic kernel buffering.

use core::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcMessage {
    pub sender_id: u64,
    pub label: u64,
    pub payload: [u64; 4],
}

impl IpcMessage {
    pub const fn new(sender_id: u64, label: u64, p0: u64, p1: u64, p2: u64, p3: u64) -> Self {
        Self {
            sender_id,
            label,
            payload: [p0, p1, p2, p3],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendezvousState {
    Empty,
    SenderWaiting,
    ReceiverWaiting,
}

pub struct IpcEndpoint {
    lock: AtomicBool,
    state: RendezvousState,
    buffer: Option<IpcMessage>,
}

impl IpcEndpoint {
    pub const fn new() -> Self {
        Self {
            lock: AtomicBool::new(false),
            state: RendezvousState::Empty,
            buffer: None,
        }
    }

    /// Fast non-blocking deposit used for intra-thread benchmark measurement
    pub fn send_direct(&mut self, msg: IpcMessage) {
        self.acquire();
        self.buffer = Some(msg);
        self.state = RendezvousState::SenderWaiting;
        self.release();
    }

    fn acquire(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }

    /// Synchronously sends a message. If no receiver is waiting, stores message and waits.
    pub fn send(&mut self, msg: IpcMessage) {
        loop {
            self.acquire();
            match self.state {
                RendezvousState::ReceiverWaiting => {
                    // Direct handoff to waiting receiver
                    self.buffer = Some(msg);
                    self.state = RendezvousState::Empty;
                    self.release();
                    return;
                }
                RendezvousState::Empty => {
                    // Store message and wait for receiver
                    self.buffer = Some(msg);
                    self.state = RendezvousState::SenderWaiting;
                    self.release();

                    // Wait until receiver consumes message
                    while self.state == RendezvousState::SenderWaiting {
                        unsafe { crate::task::scheduler::SCHEDULER.yield_now(); }
                    }
                    return;
                }
                RendezvousState::SenderWaiting => {
                    self.release();
                    unsafe { crate::task::scheduler::SCHEDULER.yield_now(); }
                }
            }
        }
    }

    /// Synchronously receives a message. If no sender is waiting, blocks until arrival.
    pub fn receive(&mut self) -> IpcMessage {
        loop {
            self.acquire();
            match self.state {
                RendezvousState::SenderWaiting => {
                    // Consume message from waiting sender
                    let msg = self.buffer.take().expect("Expected sender message");
                    self.state = RendezvousState::Empty;
                    self.release();
                    return msg;
                }
                RendezvousState::Empty => {
                    // Mark receiver waiting
                    self.state = RendezvousState::ReceiverWaiting;
                    self.release();

                    // Wait until sender deposits message
                    while self.state == RendezvousState::ReceiverWaiting {
                        unsafe { crate::task::scheduler::SCHEDULER.yield_now(); }
                    }

                    // Retrieve deposited message
                    self.acquire();
                    let msg = self.buffer.take().expect("Expected received message");
                    self.release();
                    return msg;
                }
                RendezvousState::ReceiverWaiting => {
                    self.release();
                    unsafe { crate::task::scheduler::SCHEDULER.yield_now(); }
                }
            }
        }
    }
}
