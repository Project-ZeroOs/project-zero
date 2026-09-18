//! Project Zero - Stage 3M Transport & Protocol Framing (UDP, TCP, IPv4, Ethernet)
//!
//! Authoritative Contracts:
//! - Stage 3M Architecture Rev3 (Approved & Frozen).
//! - Minimal RFC 793 11-State TCP FSM (Stop-and-Wait, 1 MSS in-flight, MSS = 1460).
//! - RFC 768 UDP Framing & Checksum.
//! - RFC 1071 Internet Checksum.

use crate::syscall::numbers::SyscallError;
use super::types::*;
use super::buf::{PACKET_BUFFER_TABLE, set_packet_payload, get_packet_data, get_packet_data_mut};

// TCP Flags
pub const TCP_FIN: u8 = 0x01;
pub const TCP_SYN: u8 = 0x02;
pub const TCP_RST: u8 = 0x04;
pub const TCP_PSH: u8 = 0x08;
pub const TCP_ACK: u8 = 0x10;
pub const TCP_URG: u8 = 0x20;

// EtherTypes (Big-Endian in wire, host values)
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;

// Headers Sizes
pub const ETHERNET_HEADER_LEN: usize = 14;
pub const IPV4_HEADER_LEN: usize = 20;
pub const UDP_HEADER_LEN: usize = 8;
pub const TCP_HEADER_LEN: usize = 20;

// Defaults
pub const TCP_DEFAULT_MSS: u16 = 1460;
pub const TCP_DEFAULT_WINDOW: u16 = 2048;
pub const TCP_INITIAL_RTO_TICKS: u32 = 20; // 200 ms (10 ms ticks)
pub const TCP_MAX_RETRIES: u8 = 3;
pub const TCP_TIME_WAIT_TICKS: u32 = 10; // 100 ms

/// Computes standard RFC 1071 16-bit one's-complement checksum over a byte slice.
pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < data.len() {
        let word = u16::from_be_bytes([data[i], 0]);
        sum = sum.wrapping_add(word as u32);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Computes transport layer (UDP / TCP) checksum using the IPv4 pseudo-header.
pub fn compute_transport_checksum(src_ip: u32, dst_ip: u32, protocol: u8, transport_segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Pseudo-header:
    // Source IP (4 bytes)
    sum = sum.wrapping_add((src_ip >> 16) as u32);
    sum = sum.wrapping_add((src_ip & 0xFFFF) as u32);
    // Dest IP (4 bytes)
    sum = sum.wrapping_add((dst_ip >> 16) as u32);
    sum = sum.wrapping_add((dst_ip & 0xFFFF) as u32);
    // Zero (1 byte) + Protocol (1 byte)
    sum = sum.wrapping_add(protocol as u32);
    // Transport segment length (2 bytes)
    sum = sum.wrapping_add(transport_segment.len() as u32);

    // Segment data (checksum field in header must be treated as 0)
    let mut i = 0;
    while i + 1 < transport_segment.len() {
        let word = u16::from_be_bytes([transport_segment[i], transport_segment[i + 1]]);
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }
    if i < transport_segment.len() {
        let word = u16::from_be_bytes([transport_segment[i], 0]);
        sum = sum.wrapping_add(word as u32);
    }

    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    let res = !(sum as u16);
    if res == 0 && protocol == 17 {
        0xFFFF // RFC 768: If computed checksum is zero, transmit 0xFFFF
    } else {
        res
    }
}

/// Generates a monotonic TCP Initial Sequence Number (ISN).
#[inline]
pub fn generate_isn(current_ticks: u64, socket_id: u64) -> u32 {
    let base = current_ticks.wrapping_mul(64_000);
    (base.wrapping_add(socket_id)) as u32
}

// -----------------------------------------------------------------------------
// Ethernet Framing
// -----------------------------------------------------------------------------

pub fn write_ethernet_header(
    buf: &mut [u8],
    dst_mac: [u8; 6],
    src_mac: [u8; 6],
    ethertype: u16,
) -> Result<(), SyscallError> {
    if buf.len() < ETHERNET_HEADER_LEN {
        return Err(SyscallError::MessageTooLarge);
    }
    buf[0..6].copy_from_slice(&dst_mac);
    buf[6..12].copy_from_slice(&src_mac);
    buf[12..14].copy_from_slice(&ethertype.to_be_bytes());
    Ok(())
}

pub fn parse_ethernet_header(frame: &[u8]) -> Option<([u8; 6], [u8; 6], u16, &[u8])> {
    if frame.len() < ETHERNET_HEADER_LEN {
        return None;
    }
    let mut dst_mac = [0u8; 6];
    let mut src_mac = [0u8; 6];
    dst_mac.copy_from_slice(&frame[0..6]);
    src_mac.copy_from_slice(&frame[6..12]);
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    Some((dst_mac, src_mac, ethertype, &frame[14..]))
}

// -----------------------------------------------------------------------------
// IPv4 Framing
// -----------------------------------------------------------------------------

pub fn write_ipv4_header(
    buf: &mut [u8],
    src_ip: u32,
    dst_ip: u32,
    protocol: u8,
    payload_len: u16,
    identification: u16,
) -> Result<(), SyscallError> {
    if buf.len() < IPV4_HEADER_LEN {
        return Err(SyscallError::MessageTooLarge);
    }
    let total_len = IPV4_HEADER_LEN as u16 + payload_len;
    buf[0] = 0x45; // Version 4, IHL 5 (20 bytes)
    buf[1] = 0x00; // DSCP / ECN
    buf[2..4].copy_from_slice(&total_len.to_be_bytes());
    buf[4..6].copy_from_slice(&identification.to_be_bytes());
    buf[6..8].copy_from_slice(&0x4000u16.to_be_bytes()); // Flags: DF=1, Frag Offset=0
    buf[8] = 64; // TTL
    buf[9] = protocol;
    buf[10..12].copy_from_slice(&[0, 0]); // Zero checksum for calculation
    buf[12..16].copy_from_slice(&src_ip.to_be_bytes());
    buf[16..20].copy_from_slice(&dst_ip.to_be_bytes());

    let csum = internet_checksum(&buf[0..IPV4_HEADER_LEN]);
    buf[10..12].copy_from_slice(&csum.to_be_bytes());
    Ok(())
}

pub fn parse_ipv4_header(packet: &[u8]) -> Option<(u32, u32, u8, u16, &[u8])> {
    if packet.len() < IPV4_HEADER_LEN {
        return None;
    }
    let ver_ihl = packet[0];
    if (ver_ihl >> 4) != 4 {
        return None; // Not IPv4
    }
    let ihl = (ver_ihl & 0x0F) as usize * 4;
    if ihl < IPV4_HEADER_LEN || packet.len() < ihl {
        return None;
    }
    // Verify checksum
    if internet_checksum(&packet[0..ihl]) != 0 {
        return None;
    }

    let total_len = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if packet.len() < total_len || total_len < ihl {
        return None;
    }

    let protocol = packet[9];
    let src_ip = u32::from_be_bytes([packet[12], packet[13], packet[14], packet[15]]);
    let dst_ip = u32::from_be_bytes([packet[16], packet[17], packet[18], packet[19]]);
    let payload = &packet[ihl..total_len];

    Some((src_ip, dst_ip, protocol, total_len as u16, payload))
}

// -----------------------------------------------------------------------------
// UDP Framing
// -----------------------------------------------------------------------------

pub fn write_udp_header(
    buf: &mut [u8],
    src_ip: u32,
    dst_ip: u32,
    src_port: u16,
    dst_port: u16,
    payload: &[u8],
) -> Result<usize, SyscallError> {
    let total_len = UDP_HEADER_LEN + payload.len();
    if buf.len() < total_len {
        return Err(SyscallError::MessageTooLarge);
    }
    buf[0..2].copy_from_slice(&src_port.to_be_bytes());
    buf[2..4].copy_from_slice(&dst_port.to_be_bytes());
    buf[4..6].copy_from_slice(&(total_len as u16).to_be_bytes());
    buf[6..8].copy_from_slice(&[0, 0]); // Zero checksum for computation
    buf[8..total_len].copy_from_slice(payload);

    let csum = compute_transport_checksum(src_ip, dst_ip, 17, &buf[0..total_len]);
    buf[6..8].copy_from_slice(&csum.to_be_bytes());
    Ok(total_len)
}

pub fn parse_udp_header(segment: &[u8], src_ip: u32, dst_ip: u32) -> Option<(u16, u16, &[u8])> {
    if segment.len() < UDP_HEADER_LEN {
        return None;
    }
    let src_port = u16::from_be_bytes([segment[0], segment[1]]);
    let dst_port = u16::from_be_bytes([segment[2], segment[3]]);
    let length = u16::from_be_bytes([segment[4], segment[5]]) as usize;
    let checksum = u16::from_be_bytes([segment[6], segment[7]]);

    if length < UDP_HEADER_LEN || segment.len() < length {
        return None;
    }

    if checksum != 0 && src_ip != 0 && dst_ip != 0 {
        let computed = compute_transport_checksum(src_ip, dst_ip, 17, &segment[0..length]);
        if computed != 0 {
            return None; // Checksum failure
        }
    }

    Some((src_port, dst_port, &segment[8..length]))
}

// -----------------------------------------------------------------------------
// TCP Minimal RFC 793 Framing & State
// -----------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TcpControlBlock {
    pub snd_una: u32,       // Oldest unacknowledged sequence number
    pub snd_nxt: u32,       // Next sequence number to transmit
    pub snd_wnd: u16,       // Send window
    pub rcv_nxt: u32,       // Next expected sequence number
    pub rcv_wnd: u16,       // Receive window
    pub rto_ticks: u32,     // Retransmission timeout in ticks
    pub retries: u8,        // Number of retransmission retries
    pub in_flight: bool,    // True if 1 MSS segment is currently in flight
}

impl TcpControlBlock {
    pub const fn empty() -> Self {
        Self {
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: TCP_DEFAULT_WINDOW,
            rcv_nxt: 0,
            rcv_wnd: TCP_DEFAULT_WINDOW,
            rto_ticks: TCP_INITIAL_RTO_TICKS,
            retries: 0,
            in_flight: false,
        }
    }
}

pub fn write_tcp_header(
    buf: &mut [u8],
    src_ip: u32,
    dst_ip: u32,
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
) -> Result<usize, SyscallError> {
    let total_len = TCP_HEADER_LEN + payload.len();
    if buf.len() < total_len {
        return Err(SyscallError::MessageTooLarge);
    }
    buf[0..2].copy_from_slice(&src_port.to_be_bytes());
    buf[2..4].copy_from_slice(&dst_port.to_be_bytes());
    buf[4..8].copy_from_slice(&seq.to_be_bytes());
    buf[8..12].copy_from_slice(&ack.to_be_bytes());
    // Data Offset: 5 words (20 bytes), Reserved: 0, Flags: flags
    let data_offset_flags = ((5u16) << 12) | (flags as u16);
    buf[12..14].copy_from_slice(&data_offset_flags.to_be_bytes());
    buf[14..16].copy_from_slice(&window.to_be_bytes());
    buf[16..18].copy_from_slice(&[0, 0]); // Zero checksum
    buf[18..20].copy_from_slice(&0u16.to_be_bytes()); // Urgent pointer

    if !payload.is_empty() {
        buf[20..total_len].copy_from_slice(payload);
    }

    let csum = compute_transport_checksum(src_ip, dst_ip, 6, &buf[0..total_len]);
    buf[16..18].copy_from_slice(&csum.to_be_bytes());
    Ok(total_len)
}

pub fn parse_tcp_header(
    segment: &[u8],
    src_ip: u32,
    dst_ip: u32,
) -> Option<(u16, u16, u32, u32, u8, u16, &[u8])> {
    if segment.len() < TCP_HEADER_LEN {
        return None;
    }
    let src_port = u16::from_be_bytes([segment[0], segment[1]]);
    let dst_port = u16::from_be_bytes([segment[2], segment[3]]);
    let seq = u32::from_be_bytes([segment[4], segment[5], segment[6], segment[7]]);
    let ack = u32::from_be_bytes([segment[8], segment[9], segment[10], segment[11]]);
    let data_offset_flags = u16::from_be_bytes([segment[12], segment[13]]);
    let header_len = ((data_offset_flags >> 12) as usize) * 4;
    let flags = (data_offset_flags & 0xFF) as u8;
    let window = u16::from_be_bytes([segment[14], segment[15]]);

    if header_len < TCP_HEADER_LEN || segment.len() < header_len {
        return None;
    }

    if src_ip != 0 && dst_ip != 0 {
        let computed = compute_transport_checksum(src_ip, dst_ip, 6, segment);
        if computed != 0 {
            return None; // Checksum mismatch
        }
    }

    Some((src_port, dst_port, seq, ack, flags, window, &segment[header_len..]))
}
