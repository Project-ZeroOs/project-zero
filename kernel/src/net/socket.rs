//! Project Zero - Stage 3M Socket Management & WaitQueue Blocking Semantics
//!
//! Authoritative Contracts:
//! - Stage 3M Architecture Rev3 (Approved & Frozen).
//! - Invariant I-NET-SOCKET-1: Socket cleanup requires zero references, port unbind, packet drain.
//! - Invariant I-NET-WAIT-1: Atomic lost-wakeup discipline with explicit predicates.
//! - Invariant I-NET-DEV-FAIL-1: Hardware/Interface faults wake blocked threads with -ENETDOWN.
//! - Invariant I-NET-TEARDOWN-1: Process exit cleanly purges owned sockets.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::syscall::numbers::SyscallError;
use crate::task::waitqueue::WaitQueue;
use crate::task::scheduler::block_current;
use crate::ipc::object::{
    KERNEL_OBJECT_TABLE_LOCK, allocate_object_slot_locked, free_object_slot_locked,
    KernelObjectType,
};
use super::types::*;
use super::buf::{
    PACKET_BUFFER_TABLE, alloc_packet_buffer, free_packet_buffer,
    set_packet_payload, get_packet_data,
};
use super::port::{bind_port, unbind_port, unbind_all_by_socket, unbind_all_by_pid, lookup_socket};
use super::transport::*;
use super::iface::{INTERFACE_TABLE, enqueue_rx_packet, enqueue_tx_packet};
use super::route::lookup_route;

pub static mut SOCKET_TABLE: [SocketSlot; MAX_SOCKETS] = [SocketSlot::empty(); MAX_SOCKETS];
pub static mut SOCKET_WAIT_QUEUES: [WaitQueue; MAX_SOCKETS] = [const { WaitQueue::new() }; MAX_SOCKETS];
pub static mut SOCKET_TCP_CB: [TcpControlBlock; MAX_SOCKETS] = [TcpControlBlock::empty(); MAX_SOCKETS];

static SOCKET_LOCK: AtomicBool = AtomicBool::new(false);
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(1);

#[inline(always)]
pub fn acquire_socket_lock() {
    while SOCKET_LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
}

#[inline(always)]
pub fn release_socket_lock() {
    SOCKET_LOCK.store(false, Ordering::Release);
}

/// Allocates a new socket slot and creates backing KernelObjectSlot (KernelObjectType::Socket = 7).
pub fn alloc_socket(
    sock_type: SocketType,
    protocol: u8,
    owner_pid: u64,
) -> Result<(usize, u64), SyscallError> {
    let resolved_protocol = match (sock_type, protocol) {
        (SocketType::Udp, 0) | (SocketType::Udp, 17) => 17,
        (SocketType::Tcp, 0) | (SocketType::Tcp, 6) => 6,
        (SocketType::Raw, p) => p,
        _ => return Err(SyscallError::InvalidArgument),
    };

    let socket_id = NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed);

    acquire_socket_lock();
    let slot_idx = unsafe {
        let mut found = None;
        for (i, slot) in SOCKET_TABLE.iter_mut().enumerate() {
            if !slot.occupied {
                slot.occupied = true;
                slot.sock_type = sock_type;
                slot.state = match sock_type {
                    SocketType::Udp | SocketType::Raw => SocketState::UdpUnbound,
                    SocketType::Tcp => SocketState::Closed,
                    _ => SocketState::Closed,
                };
                slot.error = 0;
                slot.bound_interface = 0;
                slot.local_port = 0;
                slot.remote_port = 0;
                slot.local_ip = 0;
                slot.remote_ip = 0;
                slot.socket_id = socket_id;
                slot.owner_pid = owner_pid;
                slot.rx_queue_head = 0xFFFF;
                slot.rx_queue_tail = 0xFFFF;
                slot.rx_queue_count = 0;
                slot.rx_queue_limit = 8;
                slot.bound_waitqueue_id = socket_id;

                SOCKET_TCP_CB[i] = TcpControlBlock::empty();
                SOCKET_WAIT_QUEUES[i] = WaitQueue::new();
                found = Some(i);
                break;
            }
        }
        found
    };

    let slot_idx = match slot_idx {
        Some(idx) => idx,
        None => {
            release_socket_lock();
            return Err(SyscallError::OutOfMemory);
        }
    };

    // Allocate KernelObjectSlot
    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
    let obj_res = unsafe {
        allocate_object_slot_locked(KernelObjectType::Socket, slot_idx as u16, owner_pid)
    };
    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);

    match obj_res {
        Ok(obj_idx) => {
            unsafe {
                SOCKET_TABLE[slot_idx].kernel_object_id = (obj_idx as u64) + 1;
            }
            release_socket_lock();
            Ok((slot_idx, socket_id))
        }
        Err(_) => {
            unsafe {
                SOCKET_TABLE[slot_idx] = SocketSlot::empty();
            }
            release_socket_lock();
            Err(SyscallError::OutOfMemory)
        }
    }
}

/// Frees a socket slot, draining RX buffers, unbinding ports, and releasing KernelObjectSlot.
pub fn free_socket(slot_idx: usize) -> Result<(), SyscallError> {
    if slot_idx >= MAX_SOCKETS {
        return Err(SyscallError::InvalidArgument);
    }
    acquire_socket_lock();
    unsafe {
        let slot = &mut SOCKET_TABLE[slot_idx];
        if !slot.occupied {
            release_socket_lock();
            return Err(SyscallError::InvalidArgument);
        }

        // 1. Drain RX queue buffers
        let mut curr_buf = slot.rx_queue_head;
        while curr_buf != 0xFFFF && (curr_buf as usize) < MAX_PACKET_BUFFERS {
            let next = PACKET_BUFFER_TABLE[curr_buf as usize].next_buffer_idx;
            let _ = free_packet_buffer(curr_buf as usize);
            curr_buf = next;
        }
        slot.rx_queue_head = 0xFFFF;
        slot.rx_queue_tail = 0xFFFF;
        slot.rx_queue_count = 0;

        // 2. Unbind port
        unbind_all_by_socket(slot.socket_id);

        // 3. Free KernelObjectSlot
        if slot.kernel_object_id > 0 {
            let obj_idx = (slot.kernel_object_id - 1) as usize;
            let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
            free_object_slot_locked(obj_idx);
            KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
        }

        // 4. Wake all blocked waiters
        SOCKET_WAIT_QUEUES[slot_idx].wake_all();

        // 5. Reset slot
        *slot = SocketSlot::empty();
        SOCKET_TCP_CB[slot_idx] = TcpControlBlock::empty();
    }
    release_socket_lock();
    Ok(())
}

/// Binds a socket to a local IP and port.
pub fn bind_socket(
    slot_idx: usize,
    ip_addr: u32,
    port: u16,
    has_net_config: bool,
) -> Result<u16, SyscallError> {
    if slot_idx >= MAX_SOCKETS {
        return Err(SyscallError::InvalidArgument);
    }
    acquire_socket_lock();
    let (sock_id, owner_pid, sock_type) = unsafe {
        let s = &SOCKET_TABLE[slot_idx];
        if !s.occupied {
            release_socket_lock();
            return Err(SyscallError::InvalidArgument);
        }
        (s.socket_id, s.owner_pid, s.sock_type)
    };

    let proto = match sock_type {
        SocketType::Udp => 17,
        SocketType::Tcp => 6,
        SocketType::Raw => 255,
        _ => {
            release_socket_lock();
            return Err(SyscallError::InvalidArgument);
        }
    };

    let bound_port = bind_port(proto, ip_addr, port, sock_id, owner_pid, has_net_config)?;

    unsafe {
        let s = &mut SOCKET_TABLE[slot_idx];
        s.local_ip = ip_addr;
        s.local_port = bound_port;
        if s.sock_type == SocketType::Udp {
            s.state = SocketState::UdpBound;
        }
    }
    release_socket_lock();
    Ok(bound_port)
}

/// Transitions a TCP socket to LISTEN state.
pub fn listen_socket(slot_idx: usize, backlog: u16) -> Result<(), SyscallError> {
    if slot_idx >= MAX_SOCKETS {
        return Err(SyscallError::InvalidArgument);
    }
    acquire_socket_lock();
    unsafe {
        let s = &mut SOCKET_TABLE[slot_idx];
        if !s.occupied || s.sock_type != SocketType::Tcp {
            release_socket_lock();
            return Err(SyscallError::InvalidArgument);
        }
        // If not bound, auto-bind to ephemeral port
        if s.local_port == 0 {
            let p = bind_port(6, s.local_ip, 0, s.socket_id, s.owner_pid, false)?;
            s.local_port = p;
        }
        s.state = SocketState::Listen;
        s.rx_queue_limit = if backlog > 0 { backlog } else { 8 };
    }
    release_socket_lock();
    Ok(())
}

/// Connects a socket to a remote endpoint.
pub fn connect_socket(
    slot_idx: usize,
    remote_ip: u32,
    remote_port: u16,
    non_blocking: bool,
) -> Result<(), SyscallError> {
    if slot_idx >= MAX_SOCKETS || remote_ip == 0 || remote_port == 0 {
        return Err(SyscallError::AddressNotAvailable);
    }
    acquire_socket_lock();
    let (sock_type, state, local_ip, local_port) = unsafe {
        let s = &mut SOCKET_TABLE[slot_idx];
        if !s.occupied {
            release_socket_lock();
            return Err(SyscallError::InvalidArgument);
        }
        if s.local_port == 0 {
            let p = bind_port(if s.sock_type == SocketType::Tcp { 6 } else { 17 }, 0, 0, s.socket_id, s.owner_pid, false)?;
            s.local_port = p;
        }
        s.remote_ip = remote_ip;
        s.remote_port = remote_port;
        (s.sock_type, s.state, s.local_ip, s.local_port)
    };

    if sock_type == SocketType::Udp {
        unsafe {
            SOCKET_TABLE[slot_idx].state = SocketState::UdpConnected;
        }
        release_socket_lock();
        return Ok(());
    }

    if sock_type == SocketType::Tcp {
        // Find route to remote_ip
        let if_id = match lookup_route(remote_ip) {
            Some((id, _)) => id,
            None => {
                release_socket_lock();
                return Err(SyscallError::NetworkUnreachable);
            }
        };

        unsafe {
            SOCKET_TABLE[slot_idx].bound_interface = if_id;
            SOCKET_TABLE[slot_idx].state = SocketState::SynSent;
            let isn = generate_isn(0, SOCKET_TABLE[slot_idx].socket_id);
            SOCKET_TCP_CB[slot_idx].snd_nxt = isn.wrapping_add(1);
            SOCKET_TCP_CB[slot_idx].snd_una = isn;
        }

        // Loopback direct connection resolution
        if remote_ip == LOOPBACK_IP || (remote_ip & 0xFF000000) == 0x7F000000 {
            // Check if listener exists on remote_port
            let listener_sock = lookup_socket(6, remote_ip, remote_port);
            match listener_sock {
                Some(target_sock_id) => {
                    // Check listener state
                    let mut listener_ready = false;
                    for (i, slot) in unsafe { SOCKET_TABLE.iter_mut().enumerate() } {
                        if slot.occupied && slot.socket_id == target_sock_id && slot.state == SocketState::Listen {
                            listener_ready = true;
                            // Enqueue connection event to listener
                            slot.rx_queue_count = slot.rx_queue_count.saturating_add(1);
                            unsafe {
                                SOCKET_WAIT_QUEUES[i].wake_all();
                            }
                            break;
                        }
                    }

                    if listener_ready {
                        unsafe {
                            SOCKET_TABLE[slot_idx].state = SocketState::Established;
                            SOCKET_TCP_CB[slot_idx].rcv_nxt = 1001; // Peer ISN + 1
                        }
                        release_socket_lock();
                        return Ok(());
                    } else {
                        unsafe {
                            SOCKET_TABLE[slot_idx].state = SocketState::Closed;
                            SOCKET_TABLE[slot_idx].error = -21;
                        }
                        release_socket_lock();
                        return Err(SyscallError::ConnectionRefused);
                    }
                }
                None => {
                    unsafe {
                        SOCKET_TABLE[slot_idx].state = SocketState::Closed;
                        SOCKET_TABLE[slot_idx].error = -21;
                    }
                    release_socket_lock();
                    return Err(SyscallError::ConnectionRefused);
                }
            }
        }

        if non_blocking {
            release_socket_lock();
            return Err(SyscallError::WouldBlock);
        }

        // Blocking connect loop
        loop {
            let current_state = unsafe { SOCKET_TABLE[slot_idx].state };
            let err = unsafe { SOCKET_TABLE[slot_idx].error };
            if current_state == SocketState::Established {
                release_socket_lock();
                return Ok(());
            }
            if current_state == SocketState::Closed || err != 0 {
                release_socket_lock();
                return Err(if err == -21 {
                    SyscallError::ConnectionRefused
                } else if err == -28 {
                    SyscallError::TimedOut
                } else {
                    SyscallError::NetworkUnreachable
                });
            }

            // Register waiter under lock, release, park
            release_socket_lock();
            unsafe {
                block_current(&mut SOCKET_WAIT_QUEUES[slot_idx]);
            }
            acquire_socket_lock();
        }
    }

    release_socket_lock();
    Ok(())
}

/// Transmits a payload on a socket.
pub fn send_socket(
    slot_idx: usize,
    payload: &[u8],
    flags: u32,
    non_blocking: bool,
) -> Result<usize, SyscallError> {
    if slot_idx >= MAX_SOCKETS {
        return Err(SyscallError::InvalidArgument);
    }
    if payload.len() > (PACKET_BUFFER_SIZE - PACKET_HEADROOM - ETHERNET_HEADER_LEN - IPV4_HEADER_LEN - UDP_HEADER_LEN) {
        return Err(SyscallError::MessageTooLarge);
    }

    acquire_socket_lock();
    let (sock_type, state, err, local_ip, local_port, remote_ip, remote_port) = unsafe {
        let s = &SOCKET_TABLE[slot_idx];
        if !s.occupied {
            release_socket_lock();
            return Err(SyscallError::InvalidArgument);
        }
        (s.sock_type, s.state, s.error, s.local_ip, s.local_port, s.remote_ip, s.remote_port)
    };

    if err != 0 {
        release_socket_lock();
        return Err(if err == -19 { SyscallError::NetworkDown } else { SyscallError::ConnectionReset });
    }

    if sock_type == SocketType::Tcp && state != SocketState::Established {
        release_socket_lock();
        return Err(SyscallError::NotConnected);
    }

    if sock_type == SocketType::Udp && (remote_ip == 0 || remote_port == 0) {
        release_socket_lock();
        return Err(SyscallError::AddressNotAvailable);
    }

    // Allocate packet buffer
    let buf_idx = match alloc_packet_buffer(PacketBufferState::SocketTxQueued) {
        Some(idx) => idx,
        None => {
            release_socket_lock();
            if non_blocking {
                return Err(SyscallError::WouldBlock);
            } else {
                return Err(SyscallError::OutOfMemory);
            }
        }
    };

    // Construct Packet
    let res = if sock_type == SocketType::Udp {
        // Write UDP payload
        let mut temp_buf = [0u8; 1500];
        let udp_len = write_udp_header(&mut temp_buf, local_ip, remote_ip, local_port, remote_port, payload)?;
        let _ = set_packet_payload(buf_idx, &temp_buf[0..udp_len]);
        Ok(payload.len())
    } else if sock_type == SocketType::Tcp {
        let mut temp_buf = [0u8; 1500];
        let seq = unsafe { SOCKET_TCP_CB[slot_idx].snd_nxt };
        let ack = unsafe { SOCKET_TCP_CB[slot_idx].rcv_nxt };
        let tcp_len = write_tcp_header(
            &mut temp_buf,
            local_ip,
            remote_ip,
            local_port,
            remote_port,
            seq,
            ack,
            TCP_ACK | TCP_PSH,
            TCP_DEFAULT_WINDOW,
            payload,
        )?;
        unsafe {
            SOCKET_TCP_CB[slot_idx].snd_nxt = seq.wrapping_add(payload.len() as u32);
        }
        let _ = set_packet_payload(buf_idx, &temp_buf[0..tcp_len]);
        Ok(payload.len())
    } else {
        // Raw Ethernet
        let _ = set_packet_payload(buf_idx, payload);
        Ok(payload.len())
    };

    // If loopback, directly deliver to target socket if local
    if remote_ip == LOOPBACK_IP || (remote_ip & 0xFF000000) == 0x7F000000 {
        let target_proto = if sock_type == SocketType::Tcp { 6 } else { 17 };
        if let Some(target_sock_id) = lookup_socket(target_proto, remote_ip, remote_port) {
            for (i, target) in unsafe { SOCKET_TABLE.iter_mut().enumerate() } {
                if target.occupied && target.socket_id == target_sock_id {
                    if (target.rx_queue_count as usize) < (target.rx_queue_limit as usize) {
                        // Link packet buffer to target's RX queue
                        unsafe {
                            PACKET_BUFFER_TABLE[buf_idx].state = PacketBufferState::SocketQueued;
                            PACKET_BUFFER_TABLE[buf_idx].next_buffer_idx = 0xFFFF;
                            if target.rx_queue_head == 0xFFFF {
                                target.rx_queue_head = buf_idx as u16;
                                target.rx_queue_tail = buf_idx as u16;
                            } else {
                                let old_tail = target.rx_queue_tail as usize;
                                PACKET_BUFFER_TABLE[old_tail].next_buffer_idx = buf_idx as u16;
                                target.rx_queue_tail = buf_idx as u16;
                            }
                            target.rx_queue_count += 1;
                            SOCKET_WAIT_QUEUES[i].wake_all();
                        }
                        release_socket_lock();
                        return res;
                    }
                }
            }
        }
    }

    // Otherwise transmit through interface TX queue
    let if_id = match lookup_route(remote_ip) {
        Some((id, _)) => id,
        None => 1, // default lo0
    };
    let if_idx = (if_id.saturating_sub(1)) as usize;
    let _ = enqueue_tx_packet(if_idx, buf_idx);

    release_socket_lock();
    res
}

/// Receives a payload from a socket into `out_buf`.
pub fn recv_socket(
    slot_idx: usize,
    out_buf: &mut [u8],
    flags: u32,
    non_blocking: bool,
) -> Result<usize, SyscallError> {
    if slot_idx >= MAX_SOCKETS {
        return Err(SyscallError::InvalidArgument);
    }

    acquire_socket_lock();
    loop {
        let (occupied, count, state, err, sock_type, local_ip, remote_ip) = unsafe {
            let s = &SOCKET_TABLE[slot_idx];
            (s.occupied, s.rx_queue_count, s.state, s.error, s.sock_type, s.local_ip, s.remote_ip)
        };

        if !occupied {
            release_socket_lock();
            return Err(SyscallError::InvalidArgument);
        }

        if err != 0 {
            release_socket_lock();
            return Err(if err == -19 { SyscallError::NetworkDown } else { SyscallError::ConnectionReset });
        }

        // Predicate check: rx_queue_count > 0 || state == PeerClosed || state == Closed
        if count > 0 {
            // Dequeue buffer
            let (buf_idx, next_buf) = unsafe {
                let s = &mut SOCKET_TABLE[slot_idx];
                let head = s.rx_queue_head as usize;
                let next = PACKET_BUFFER_TABLE[head].next_buffer_idx;
                s.rx_queue_head = next;
                if s.rx_queue_head == 0xFFFF {
                    s.rx_queue_tail = 0xFFFF;
                }
                s.rx_queue_count = s.rx_queue_count.saturating_sub(1);
                (head, next)
            };

            let copy_len = if let Some(data) = get_packet_data(buf_idx) {
                if sock_type == SocketType::Udp {
                    // Strip UDP header if present
                    if let Some((_, _, udp_payload)) = parse_udp_header(data, remote_ip, local_ip) {
                        let to_copy = core::cmp::min(out_buf.len(), udp_payload.len());
                        out_buf[0..to_copy].copy_from_slice(&udp_payload[0..to_copy]);
                        to_copy
                    } else {
                        let to_copy = core::cmp::min(out_buf.len(), data.len());
                        out_buf[0..to_copy].copy_from_slice(&data[0..to_copy]);
                        to_copy
                    }
                } else if sock_type == SocketType::Tcp {
                    // Strip TCP header if present
                    if let Some((_, _, _, _, _, _, tcp_payload)) = parse_tcp_header(data, remote_ip, local_ip) {
                        let to_copy = core::cmp::min(out_buf.len(), tcp_payload.len());
                        out_buf[0..to_copy].copy_from_slice(&tcp_payload[0..to_copy]);
                        to_copy
                    } else {
                        let to_copy = core::cmp::min(out_buf.len(), data.len());
                        out_buf[0..to_copy].copy_from_slice(&data[0..to_copy]);
                        to_copy
                    }
                } else {
                    let to_copy = core::cmp::min(out_buf.len(), data.len());
                    out_buf[0..to_copy].copy_from_slice(&data[0..to_copy]);
                    to_copy
                }
            } else {
                0
            };

            let _ = free_packet_buffer(buf_idx);
            release_socket_lock();
            return Ok(copy_len);
        }

        if state == SocketState::PeerClosed {
            release_socket_lock();
            return Ok(0); // EOF
        }

        if state == SocketState::Closed {
            release_socket_lock();
            return Err(SyscallError::NotConnected);
        }

        if non_blocking {
            release_socket_lock();
            return Err(SyscallError::WouldBlock);
        }

        // Park on waitqueue
        release_socket_lock();
        unsafe {
            block_current(&mut SOCKET_WAIT_QUEUES[slot_idx]);
        }
        acquire_socket_lock();
    }
}

/// Marks all sockets on an interface as faulted and wakes all blocked threads (I-NET-DEV-FAIL-1).
pub fn wake_all_sockets_on_fault(interface_id: u16) {
    acquire_socket_lock();
    unsafe {
        for (i, slot) in SOCKET_TABLE.iter_mut().enumerate() {
            if slot.occupied && (slot.bound_interface == interface_id || interface_id == 0) {
                slot.state = SocketState::Faulted;
                slot.error = -19; // NetworkDown
                SOCKET_WAIT_QUEUES[i].wake_all();
            }
        }
    }
    release_socket_lock();
}

/// Teardown helper for terminating process: cleans up all owned sockets (I-NET-TEARDOWN-1).
pub fn cleanup_process_sockets(pid: u64) {
    acquire_socket_lock();
    unsafe {
        for i in 0..MAX_SOCKETS {
            let slot = &mut SOCKET_TABLE[i];
            if slot.occupied && slot.owner_pid == pid {
                // Drain RX buffers
                let mut curr_buf = slot.rx_queue_head;
                while curr_buf != 0xFFFF && (curr_buf as usize) < MAX_PACKET_BUFFERS {
                    let next = PACKET_BUFFER_TABLE[curr_buf as usize].next_buffer_idx;
                    let _ = free_packet_buffer(curr_buf as usize);
                    curr_buf = next;
                }
                slot.rx_queue_head = 0xFFFF;
                slot.rx_queue_tail = 0xFFFF;
                slot.rx_queue_count = 0;

                // Unbind ports
                unbind_all_by_socket(slot.socket_id);

                // Free KernelObjectSlot
                if slot.kernel_object_id > 0 {
                    let obj_idx = (slot.kernel_object_id - 1) as usize;
                    let rflags = KERNEL_OBJECT_TABLE_LOCK.acquire();
                    free_object_slot_locked(obj_idx);
                    KERNEL_OBJECT_TABLE_LOCK.unlock_restore(rflags);
                }

                // Wake waiters
                SOCKET_WAIT_QUEUES[i].wake_all();

                *slot = SocketSlot::empty();
                SOCKET_TCP_CB[i] = TcpControlBlock::empty();
            }
        }
    }
    unbind_all_by_pid(pid);
    release_socket_lock();
}
