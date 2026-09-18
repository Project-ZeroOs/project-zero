//! Project Zero - Stage 3G Channel Subsystem
//!
//! Authoritative Contract: Stage 3G Architecture Rev10 (Approved & Frozen).
//!
//! Sizes and Alignments:
//!   - IpcMessage: 80 bytes (align 8)
//!   - ChannelRing: 328 bytes (align 8)
//!   - ChannelEndpointData: 56 bytes (align 8)
//!   - Channel: 768 bytes (align 8)
//!   - CHANNEL_TABLE: 64 * 768 = 48 KiB in .bss

use crate::ipc::types::{rights, signals, IpcError, IpcMessage};
use crate::ipc::object::{
    allocate_object_slot_locked, free_object_slot_locked, KernelObjectType,
    ObjectLifecycleState, KERNEL_OBJECT_TABLE, KERNEL_OBJECT_TABLE_LOCK, MAX_KERNEL_OBJECTS,
};
use crate::ipc::handle::{
    allocate_handle_entry_locked, close_handle_locked, resolve_current_process_slot,
    validate_handle_locked, Handle,
};
use crate::task::thread::{KernelThread, ThreadState, MAX_THREADS};
use crate::task::waitqueue::WaitQueue;
use crate::task::scheduler::{commit_block_and_switch, prepare_block_locked, wake_thread_locked, SCHEDULER};
use crate::task::percpu::current_thread_from_gs;

pub const MAX_CHANNELS: usize = 32;
pub const CHANNEL_RING_CAPACITY: usize = 4;

#[repr(C)]
#[derive(Debug)]
pub struct ChannelRing {
    /// 4 messages * 80 bytes = 320 bytes (offset 0..320)
    pub buffer: [IpcMessage; CHANNEL_RING_CAPACITY],
    /// Ring buffer indices (offset 320..323)
    pub head: u8,
    pub tail: u8,
    pub count: u8,
    /// Explicit padding for 8-byte alignment (offset 323..328)
    pub _reserved: [u8; 5],
}

const _: () = assert!(core::mem::size_of::<ChannelRing>() == 328);
const _: () = assert!(core::mem::align_of::<ChannelRing>() == 8);

impl ChannelRing {
    pub const fn empty() -> Self {
        Self {
            buffer: [const { IpcMessage::empty() }; CHANNEL_RING_CAPACITY],
            head: 0,
            tail: 0,
            count: 0,
            _reserved: [0u8; 5],
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.count == CHANNEL_RING_CAPACITY as u8
    }

    pub fn push(&mut self, msg: IpcMessage) -> bool {
        if self.is_full() {
            return false;
        }
        self.buffer[self.tail as usize] = msg;
        self.tail = ((self.tail + 1) % (CHANNEL_RING_CAPACITY as u8)) as u8;
        self.count += 1;
        true
    }

    pub fn pop(&mut self) -> Option<IpcMessage> {
        if self.is_empty() {
            return None;
        }
        let msg = self.buffer[self.head as usize];
        self.head = ((self.head + 1) % (CHANNEL_RING_CAPACITY as u8)) as u8;
        self.count -= 1;
        Some(msg)
    }
}

#[repr(C)]
pub struct ChannelEndpointData {
    /// Flags and reference counts (offset 0..8)
    pub is_closed: bool,
    pub peer_closed: bool,
    pub handle_refs: u16,
    pub _pad: u32,
    /// Waiters waiting to receive (offset 8..32) (24 bytes)
    pub waiters_rx: WaitQueue,
    /// Waiters waiting to send (offset 32..56) (24 bytes)
    pub waiters_tx: WaitQueue,
}

const _: () = assert!(core::mem::size_of::<ChannelEndpointData>() == 56);
const _: () = assert!(core::mem::align_of::<ChannelEndpointData>() == 8);

impl ChannelEndpointData {
    pub const fn empty() -> Self {
        Self {
            is_closed: false,
            peer_closed: false,
            handle_refs: 0,
            _pad: 0,
            waiters_rx: WaitQueue::new(),
            waiters_tx: WaitQueue::new(),
        }
    }
}

#[repr(C)]
pub struct Channel {
    /// Ring 0 -> 1 (offset 0..328) (328 bytes)
    pub ring_0_to_1: ChannelRing,
    /// Ring 1 -> 0 (offset 328..656) (328 bytes)
    pub ring_1_to_0: ChannelRing,
    /// Endpoint 0 control state (offset 656..712) (56 bytes)
    pub endpoint_0: ChannelEndpointData,
    /// Endpoint 1 control state (offset 712..768) (56 bytes)
    pub endpoint_1: ChannelEndpointData,
}

const _: () = assert!(core::mem::size_of::<Channel>() == 768);
const _: () = assert!(core::mem::align_of::<Channel>() == 8);

impl Channel {
    pub const fn empty() -> Self {
        Self {
            ring_0_to_1: ChannelRing::empty(),
            ring_1_to_0: ChannelRing::empty(),
            endpoint_0: ChannelEndpointData::empty(),
            endpoint_1: ChannelEndpointData::empty(),
        }
    }
}

/// Authoritative static pool of 64 Channels in .bss (64 * 768 = 48 KiB).
pub static mut CHANNEL_TABLE: [Channel; MAX_CHANNELS] =
    [const { Channel::empty() }; MAX_CHANNELS];

/// Tracking in-flight IPC operations per kernel thread (Invariant I-IPC-1).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ThreadIpcState {
    pub occupied: bool,
    pub _pad: u8,
    pub object_index: u16,
}

const _: () = assert!(core::mem::size_of::<ThreadIpcState>() == 4);

pub static mut THREAD_IPC_STATE: [ThreadIpcState; MAX_THREADS] =
    [ThreadIpcState { occupied: false, _pad: 0, object_index: 0 }; MAX_THREADS];


/// Resolves the thread slot index in THREAD_TABLE (0..MAX_THREADS).
#[inline(always)]
fn current_thread_slot() -> usize {
    let t = current_thread_from_gs();
    assert!(!t.is_null());
    let base = unsafe { &raw const crate::task::thread::THREAD_TABLE as usize };
    let ptr = t as usize;
    assert!(ptr >= base);
    let offset = ptr - base;
    let slot = offset / core::mem::size_of::<crate::task::thread::ThreadSlot>();
    assert!(slot < MAX_THREADS);
    slot
}

/// Creates a bidirectional Channel object, allocating two handles (one for each endpoint).
pub fn channel_create() -> Result<(Handle, Handle), IpcError> {
    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;

    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    // 1. Locate free channel in CHANNEL_TABLE
    let mut channel_idx = None;
    unsafe {
        for i in 0..MAX_CHANNELS {
            let ch = &CHANNEL_TABLE[i];
            // If both endpoints have 0 handle_refs and are not in use
            if ch.endpoint_0.handle_refs == 0 && ch.endpoint_1.handle_refs == 0 && !ch.endpoint_0.is_closed && !ch.endpoint_1.is_closed {
                // Check if any object slot points to it
                let mut in_use = false;
                for o in 0..MAX_KERNEL_OBJECTS {
                    let obj = &KERNEL_OBJECT_TABLE[o];
                    if obj.occupied && obj.obj_type == KernelObjectType::Channel && obj.pool_index == i as u16 {
                        in_use = true;
                        break;
                    }
                }
                if !in_use {
                    channel_idx = Some(i);
                    break;
                }
            }
        }
    }

    let ch_idx = match channel_idx {
        Some(idx) => idx,
        None => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::ChannelTableFull);
        }
    };

    // 2. Allocate KernelObjectSlot
    let obj_idx = match unsafe { allocate_object_slot_locked(KernelObjectType::Channel, ch_idx as u16, cur_pid) } {
        Ok(idx) => idx,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    // 3. Initialize Channel structure
    unsafe {
        let ch = &mut CHANNEL_TABLE[ch_idx];
        *ch = Channel::empty();
        ch.endpoint_0.handle_refs = 1;
        ch.endpoint_1.handle_refs = 1;
    }

    // 4. Allocate handles for Endpoint 0 and Endpoint 1
    let default_rights = rights::READ | rights::WRITE | rights::TRANSFER | rights::DUPLICATE | rights::SIGNAL | rights::WAIT
        | crate::cap::types::cap_rights::CHANNEL_SEND
        | crate::cap::types::cap_rights::CHANNEL_RECEIVE
        | crate::cap::types::cap_rights::DUPLICATE
        | crate::cap::types::cap_rights::TRANSFER
        | crate::cap::types::cap_rights::REVOKE
        | crate::cap::types::cap_rights::CLOSE
        | crate::cap::types::cap_rights::INSPECT
        | crate::cap::types::cap_rights::AUDIT;

    let h0 = match unsafe { allocate_handle_entry_locked(proc_slot, obj_idx, default_rights, 0) } {
        Ok(h) => h,
        Err(e) => {
            unsafe {
                free_object_slot_locked(obj_idx);
            }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    let h1 = match unsafe { allocate_handle_entry_locked(proc_slot, obj_idx, default_rights, 1) } {
        Ok(h) => h,
        Err(e) => {
            unsafe {
                let _ = close_handle_locked(proc_slot, h0);
                free_object_slot_locked(obj_idx);
            }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    Ok((h0, h1))
}

/// Sends an 80-byte message to the specified channel handle.
///
/// If `blocking` is true, the calling thread blocks until space is available or peer closes.
pub fn channel_send(handle: Handle, msg: &IpcMessage, blocking: bool) -> Result<(), IpcError> {
    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;
    let thread_slot = current_thread_slot();

    let mut rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    // 1. Handle validation
    let (obj_idx, endpoint, rights_mask) = match unsafe { validate_handle_locked(proc_slot, handle, 0) } {
        Ok(res) => res,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    if (rights_mask & (rights::WRITE | crate::cap::types::cap_rights::CHANNEL_SEND)) == 0 {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::PermissionDenied);
    }

    let obj_slot = unsafe { &mut KERNEL_OBJECT_TABLE[obj_idx] };
    assert_eq!(obj_slot.obj_type, KernelObjectType::Channel);
    let ch_idx = obj_slot.pool_index as usize;
    let channel = unsafe { &mut CHANNEL_TABLE[ch_idx] };

    // 2. In-flight operation pin (Invariant I-IPC-1 & I-CHAN-5)
    unsafe {
        let t_state = &mut THREAD_IPC_STATE[thread_slot];
        if t_state.occupied {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::ConcurrentOperation);
        }
        t_state.occupied = true;
        t_state.object_index = obj_idx as u16;
        obj_slot.header.in_flight_op_refs += 1;
    }

    // 3. Send execution loop
    loop {
        let (my_ep, peer_ep, tx_ring) = if endpoint == 0 {
            (&mut channel.endpoint_0, &mut channel.endpoint_1, &mut channel.ring_0_to_1)
        } else {
            (&mut channel.endpoint_1, &mut channel.endpoint_0, &mut channel.ring_1_to_0)
        };

        // Check closure
        if my_ep.is_closed {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::EndpointClosed);
        }
        if my_ep.peer_closed || peer_ep.is_closed {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::PeerClosed);
        }

        // Try pushing message
        if tx_ring.push(*msg) {
            // Success! Wake any receiver waiting on peer endpoint
            let sched_rflags = unsafe { SCHEDULER.lock.acquire() };
            unsafe {
                if let Some(waiter) = peer_ep.waiters_rx.pop_highest_locked() {
                    wake_thread_locked(waiter);
                }
                SCHEDULER.lock.unlock_restore(sched_rflags);
            }

            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Ok(());
        }

        // Ring is full
        if !blocking {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::WouldBlock);
        }

        // Enroll in waiters_tx under SCHEDULER.lock
        let sched_rflags = unsafe { SCHEDULER.lock.acquire() };
        unsafe {
            let (out_t, prev_rsp, next_rsp, frame_type) = prepare_block_locked(&mut my_ep.waiters_tx);
            // Release KERNEL_OBJECT_TABLE_LOCK before context switch
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            commit_block_and_switch(out_t, prev_rsp, next_rsp, sched_rflags, frame_type);
        }

        // Re-acquire KERNEL_OBJECT_TABLE_LOCK after wake
        rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

        // Check if handle is still valid or closed
        let (cur_ep, cur_peer) = if endpoint == 0 {
            (&channel.endpoint_0, &channel.endpoint_1)
        } else {
            (&channel.endpoint_1, &channel.endpoint_0)
        };
        if cur_ep.is_closed {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::EndpointClosed);
        }
        if cur_ep.peer_closed || cur_peer.is_closed {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::PeerClosed);
        }
    }
}

/// Receives an 80-byte message from the specified channel handle.
///
/// If `blocking` is true, the calling thread blocks until a message is available or peer closes.
pub fn channel_receive(handle: Handle, blocking: bool) -> Result<IpcMessage, IpcError> {
    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;
    let thread_slot = current_thread_slot();

    let mut rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    // 1. Handle validation
    let (obj_idx, endpoint, rights_mask) = match unsafe { validate_handle_locked(proc_slot, handle, 0) } {
        Ok(res) => res,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    if (rights_mask & (rights::READ | crate::cap::types::cap_rights::CHANNEL_RECEIVE)) == 0 {
        KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        return Err(IpcError::PermissionDenied);
    }

    let obj_slot = unsafe { &mut KERNEL_OBJECT_TABLE[obj_idx] };
    assert_eq!(obj_slot.obj_type, KernelObjectType::Channel);
    let ch_idx = obj_slot.pool_index as usize;
    let channel = unsafe { &mut CHANNEL_TABLE[ch_idx] };

    // 2. In-flight operation pin (Invariant I-IPC-1 & I-CHAN-5)
    unsafe {
        let t_state = &mut THREAD_IPC_STATE[thread_slot];
        if t_state.occupied {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::ConcurrentOperation);
        }
        t_state.occupied = true;
        t_state.object_index = obj_idx as u16;
        obj_slot.header.in_flight_op_refs += 1;
    }

    // 3. Receive execution loop
    loop {
        let (my_ep, peer_ep, rx_ring) = if endpoint == 0 {
            (&mut channel.endpoint_0, &mut channel.endpoint_1, &mut channel.ring_1_to_0)
        } else {
            (&mut channel.endpoint_1, &mut channel.endpoint_0, &mut channel.ring_0_to_1)
        };

        // Try popping message
        if let Some(msg) = rx_ring.pop() {
            // Success! Wake any sender waiting on peer endpoint
            let sched_rflags = unsafe { SCHEDULER.lock.acquire() };
            unsafe {
                if let Some(waiter) = peer_ep.waiters_tx.pop_highest_locked() {
                    wake_thread_locked(waiter);
                }
                SCHEDULER.lock.unlock_restore(sched_rflags);
            }

            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Ok(msg);
        }

        // Empty ring: check closure
        if my_ep.is_closed {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::EndpointClosed);
        }
        if my_ep.peer_closed || peer_ep.is_closed {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::PeerClosed);
        }

        if !blocking {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::WouldBlock);
        }

        // Enroll in waiters_rx under SCHEDULER.lock
        let sched_rflags = unsafe { SCHEDULER.lock.acquire() };
        unsafe {
            let (out_t, prev_rsp, next_rsp, frame_type) = prepare_block_locked(&mut my_ep.waiters_rx);
            // Release KERNEL_OBJECT_TABLE_LOCK before switching
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            commit_block_and_switch(out_t, prev_rsp, next_rsp, sched_rflags, frame_type);
        }


        // Re-acquire KERNEL_OBJECT_TABLE_LOCK after wake
        rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

        // Check if handle is still valid or closed
        let (cur_ep, cur_peer, cur_rx) = if endpoint == 0 {
            (&channel.endpoint_0, &channel.endpoint_1, &channel.ring_1_to_0)
        } else {
            (&channel.endpoint_1, &channel.endpoint_0, &channel.ring_0_to_1)
        };

        if cur_ep.is_closed {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::EndpointClosed);
        }
        if (cur_ep.peer_closed || cur_peer.is_closed) && cur_rx.is_empty() {
            unsafe { release_in_flight_pin_locked(thread_slot, obj_idx); }
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(IpcError::PeerClosed);
        }
    }
}

/// Closes a user handle to a channel endpoint.
///
/// Implements the exact ordering of the Last-Handle-Close Path under Invariant `I-CHAN-5`.
pub fn channel_close(handle: Handle) -> Result<(), IpcError> {
    let cur_t = current_thread_from_gs();
    assert!(!cur_t.is_null());
    let cur_pid = unsafe { (*cur_t).process_id };
    let proc_slot = resolve_current_process_slot(cur_pid)?;

    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();

    let (obj_idx, endpoint) = match unsafe { close_handle_locked(proc_slot, handle) } {
        Ok(res) => res,
        Err(e) => {
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
            return Err(e);
        }
    };

    let obj_slot = unsafe { &mut KERNEL_OBJECT_TABLE[obj_idx] };
    assert_eq!(obj_slot.obj_type, KernelObjectType::Channel);
    let ch_idx = obj_slot.pool_index as usize;
    let channel = unsafe { &mut CHANNEL_TABLE[ch_idx] };

    // Decrement endpoint.handle_refs
    let (my_ep, peer_ep) = if endpoint == 0 {
        (&mut channel.endpoint_0, &mut channel.endpoint_1)
    } else {
        (&mut channel.endpoint_1, &mut channel.endpoint_0)
    };

    my_ep.handle_refs = my_ep.handle_refs.saturating_sub(1);
    if my_ep.handle_refs == 0 {
        my_ep.is_closed = true;
        peer_ep.peer_closed = true;
        obj_slot.header.signals |= signals::PEER_CLOSED;

        // Wake affected waiters under nested SCHEDULER.lock
        let sched_rflags = unsafe { SCHEDULER.lock.acquire() };
        unsafe {
            while let Some(w) = my_ep.waiters_rx.pop_highest_locked() {
                wake_thread_locked(w);
            }
            while let Some(w) = my_ep.waiters_tx.pop_highest_locked() {
                wake_thread_locked(w);
            }
            while let Some(w) = peer_ep.waiters_rx.pop_highest_locked() {
                wake_thread_locked(w);
            }
            while let Some(w) = peer_ep.waiters_tx.pop_highest_locked() {
                wake_thread_locked(w);
            }
            SCHEDULER.lock.unlock_restore(sched_rflags);
        }
    }

    unsafe {
        check_and_reclaim_channel_locked(obj_idx);
    }

    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
    Ok(())
}

/// Releases the in-flight operation pin for a thread under KERNEL_OBJECT_TABLE_LOCK.
pub unsafe fn release_in_flight_pin_locked(thread_slot: usize, obj_idx: usize) {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    let t_state = &mut THREAD_IPC_STATE[thread_slot];
    if t_state.occupied {
        t_state.occupied = false;
        let obj_slot = &mut KERNEL_OBJECT_TABLE[obj_idx];
        obj_slot.header.in_flight_op_refs = obj_slot.header.in_flight_op_refs.saturating_sub(1);
        check_and_reclaim_channel_locked(obj_idx);
    }
}

/// Checks if a channel object can be reclaimed and freed under KERNEL_OBJECT_TABLE_LOCK.
pub unsafe fn check_and_reclaim_channel_locked(obj_idx: usize) {
    assert!(KERNEL_OBJECT_TABLE_LOCK.is_locked());
    let obj_slot = &mut KERNEL_OBJECT_TABLE[obj_idx];
    if !obj_slot.occupied || obj_slot.obj_type != KernelObjectType::Channel {
        return;
    }

    let ch_idx = obj_slot.pool_index as usize;
    let channel = &CHANNEL_TABLE[ch_idx];

    // Reclamation requires:
    // ref_count == 0 && all wait queues empty
    if obj_slot.header.ref_count() == 0
        && channel.endpoint_0.waiters_rx.is_empty()
        && channel.endpoint_0.waiters_tx.is_empty()
        && channel.endpoint_1.waiters_rx.is_empty()
        && channel.endpoint_1.waiters_tx.is_empty()
    {
        obj_slot.header.state = ObjectLifecycleState::Reclaiming;
        // Clear channel storage
        CHANNEL_TABLE[ch_idx] = Channel::empty();
        // Free object slot and increment generation
        free_object_slot_locked(obj_idx);
    }
}

/// Cancels all channel waiters belonging to a terminating process.
///
/// SAFETY: Caller MUST hold SCHEDULER.lock (IF=0).
pub unsafe fn cancel_channel_waiters_for_process_locked(pid: u64) {
    assert!(SCHEDULER.lock.is_locked());
    for i in 0..MAX_CHANNELS {
        let ch = &mut CHANNEL_TABLE[i];
        cancel_waiters_in_queue_locked(&mut ch.endpoint_0.waiters_rx, pid);
        cancel_waiters_in_queue_locked(&mut ch.endpoint_0.waiters_tx, pid);
        cancel_waiters_in_queue_locked(&mut ch.endpoint_1.waiters_rx, pid);
        cancel_waiters_in_queue_locked(&mut ch.endpoint_1.waiters_tx, pid);
    }
}

unsafe fn cancel_waiters_in_queue_locked(queue: &mut WaitQueue, pid: u64) {
    let mut curr = queue.head;
    while !curr.is_null() {
        let next = (*curr).next_waiter;
        if (*curr).process_id == pid {
            queue.remove_locked(curr);
        }
        curr = next;
    }
}


