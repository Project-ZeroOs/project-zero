# Stage 3M Architecture Specification Rev2 — Networking Model

**Author**: Google DeepMind Advanced Agentic Coding (Antigravity)  
**Date**: September 17, 2026  
**Status**: 🟡 **PROPOSED (Rev2) — AWAITING REVIEW**  
**Frozen Foundation**: Stages 3A–3L (Threads, Scheduler, Preemption, Synchronization, Thread Lifecycle, Processes, IPC, Capabilities, Syscalls, ELF Loading, ZeroFS Storage, Device / Hardware Model)

---

## 1. Executive Summary & Design Principles

Stage 3M establishes the native **Networking Model** for **Project Zero / ZeroOS**. It provides a capability-governed, statically bounded, deterministic substrate connecting user-space workloads to physical network interface controllers (NICs) and virtual loopback devices without resorting to a conventional monolithic in-kernel networking stack.

```text
========================================================================================
                                     HUMAN INTENT
                                          ↓
                              WORKLOADS & AGENTS (RING 3)
                                          ↓
                         CAPABILITY HANDLES & SOCKET OBJECTS
                                          ↓
                                SYSCALL ABI (22..31)
                                          ↓
                       STAGE 3M NATIVE NETWORKING SUBSTRATE
                 [Sockets / Queues / Packets / Routing / ARP / Loopback]
                                          ↓
                    STAGE 3L HARDWARE ABSTRACTION LAYER (FROZEN)
                     [DeviceTable / DMA Pins / IRQ Bindings / Events]
                                          ↓
                          PHYSICAL NIC / DMA RINGS / PHY
========================================================================================
```

Rev2 resolves the seven architectural gaps identified in Rev1 review:
1. **Explicit NetworkDevice ↔ Stage 3L Device Contract**: Defined binding, lifecycle, generation safety, and failure propagation (`I-NET-DEV-1`).
2. **Authoritative Packet Buffer & DMA Model**: Slicing, contiguous frame allocation, exact ownership state machine, and DMA pin integration (`I-NET-BUF-1`, `I-NET-DMA-1`).
3. **Protocol-Layer Boundary**: Distinct layers (Ethernet, IPv4, ARP, ICMP, UDP, TCP) with explicit ownership of headers, checksums, MTU, and sequence tracking.
4. **Socket Lifecycle & Blocking Semantics**: Authoritative TCP/UDP state machines and WaitQueue wakeup predicates (`I-NET-SOCKET-1`, `I-NET-WAIT-1`).
5. **Port-Binding Architecture**: Bounded table, TCP/UDP namespace isolation, wildcard rules, ephemeral range, and privileged port authority (`I-NET-BIND-1`).
6. **Deterministic Routing Semantics**: Longest-prefix matching, metric tie-breaking, default gateway, and interface teardown route purging (`I-NET-ROUTE-TEARDOWN-1`).
7. **Deterministic ARP State Machine**: Four-state engine (`Empty`, `Incomplete`, `Reachable`, `Stale`), tick-driven timeouts, and deterministic eviction (`I-NET-ARP-1`).
8. **Capability Decomposition & Syscall Reconciliation**: Decomposed rights scopes (`NET_ROUTE`, `NET_CONFIG`, `NET_RAW`), reconciled error codes (-19..-29), and exact `.bss` accounting.

---

## 2. NetworkDevice ↔ Stage 3L Device Binding Contract

Stage 3M does not duplicate device discovery or physical management. Instead, it **binds** to physical peripherals registered in Stage 3L's `DEVICE_TABLE` (`DeviceClass::Network`).

```text
+---------------------------------------------------------------------------------------+
| STAGE 3L DEVICE MODEL (Frozen)                                                        |
|   DEVICE_TABLE[slot]                                                                  |
|   ├── device_id: DeviceId                                                             |
|   ├── class: DeviceClass::Network                                                     |
|   ├── state: DeviceLifecycleState (Ready, Active, Quiescing, Faulted, Detached, etc.) |
|   ├── generation: u16                                                                 |
|   └── bound_event_id: u64                                                             |
+---------------------------------------------------------------------------------------+
                                           │
                        Bound via NetworkDeviceBinding
                                           │
                                           ▼
+---------------------------------------------------------------------------------------+
| STAGE 3M NETWORKING SUBSTRATE                                                         |
|   NETWORK_DEVICE_BINDINGS[binding_idx]                                                |
|   ├── device_id: DeviceId                                                             |
|   ├── device_slot: usize                                                              |
|   ├── device_generation: u16                                                          |
|   ├── interface_id: u16                                                               |
|   └── state: DeviceBindingState (Unbound, Operational, Quiescing, Faulted, Detached)  |
|                                                                                       |
|   INTERFACE_TABLE[interface_id]                                                       |
|   ├── mtu, mac_addr, ip_addr, netmask                                                 |
|   ├── rx_queue / tx_queue                                                             |
|   └── statistics                                                                      |
+---------------------------------------------------------------------------------------+
```

### 2.1 Interface Registration & Generation Validation
1. An interface is created either as the virtual loopback interface (`lo0`, pseudo-device 0) or by binding a Stage 3L physical device (`DeviceClass::Network`).
2. The binding stores `device_generation = DEVICE_TABLE[device_slot].generation`.
3. Every operation traversing an interface validates that the underlying device is still occupied and `device.generation == binding.device_generation`. Stale bindings fail fail-closed with `SyscallError::DeviceFault` (-18) or `SyscallError::NetworkDown` (-19).

### 2.2 Device Lifecycle Propagation
- **Ready $\to$ Active**: Network interface marks `state = InterfaceState::Up` and initializes RX/TX rings.
- **Faulted / Quiescing**: Bound interface transitions to `InterfaceState::Faulted`. Blocked threads on bound sockets wake immediately with `SyscallError::NetworkDown` (-19).
- **Detached / Released**: Network binding is invalidated (`state = Detached`). All associated routing table entries referencing `interface_id` are removed. Sockets connected through that interface transition to `Closed`.

### 2.3 Invariant `I-NET-DEV-1`
> Every `NetworkInterface` is backed by exactly one live Stage 3L `DeviceSlot` binding (or internal loopback pseudo-device 0). A `NetworkInterface` cannot outlive its bound `DeviceSlot`.

---

## 3. Authoritative Packet-Buffer & DMA Contract

Stage 3M defines an explicit, bounded memory model for packet buffers.

```text
+---------------------------------------------------------------------------------------+
| Physical Memory (PMM-pinned frames via Stage 3L alloc_dma_buffer)                     |
|                                                                                       |
| Frame 0 (4096 B)           Frame 1 (4096 B)                     Frame 15 (4096 B)     |
| +-----------+-----------+  +-----------+-----------+            +-----------+---------+
| | Packet 0  | Packet 1  |  | Packet 2  | Packet 3  |   ...      | Packet 30 |Packet 31|
| | (2048 B)  | (2048 B)  |  | (2048 B)  | (2048 B)  |            | (2048 B)  |(2048 B) |
| +-----------+-----------+  +-----------+-----------+            +-----------+---------+
+---------------------------------------------------------------------------------------+
        ▲           ▲
        │           │
+-------┴-----------┴-------------------------------------------------------------------+
| PACKET_BUFFER_TABLE in .bss: [PacketBufferSlot; 32] (48 bytes each)                   |
| Descriptors track: buffer_id, state, phys_addr, data_offset, data_len, queue links    |
+---------------------------------------------------------------------------------------+
```

### 3.1 Packet Specifications
- **Maximum Frame Size**: 2048 bytes.
  - Headroom: 64 bytes (for prepending transport/IP/Ethernet headers without reallocating).
  - Maximum Ethernet Frame: 1518 bytes (14 B Ethernet header + 1500 B payload + 4 B FCS).
  - Alignment Padding: 466 bytes.
- **Contiguity**: Each packet payload resides in a physically contiguous 2048-byte slice.
- **Physical Allocation**: The 32 packet buffers are backed by 16 physical frames allocated in a single transaction via Stage 3L `alloc_dma_buffer` at network subsystem initialization.
- **PMM Tracking**: The 16 physical frames are registered in Stage 3L's `PHYSICAL_FRAME_PIN_TABLE` and associated with `DMA_BUFFER_TABLE`. Stage 3M holds descriptor `dma_buffer_id`.

### 3.2 Single-Ownership State Machine
A packet buffer resides in exactly one state at any point in time:
```text
               +-------------------------------------------+
               |                   FREE                    |
               +---------------------+---------------------+
                                     |
                    +----------------+----------------+
                    |                                 |
             [RX Allocation]                   [TX Allocation]
                    v                                 v
        +-----------------------+         +-----------------------+
        |     RX_ALLOCATED      |         |      TX_PENDING       |
        +-----------+-----------+         +-----------+-----------+
                    |                                 |
            [Packet Ingress]                   [Transmitted]
                    v                                 v
        +-----------------------+         +-----------------------+
        |      SOCK_QUEUED      |         |      TRANSMITTED      |
        +-----------+-----------+         +-----------+-----------+
                    |                                 |
             [User Recv]                       [Completion]
                    |                                 |
                    +----------------+----------------+
                                     v
                             [Recycled to Free]
```

### 3.3 Invariants
- **`I-NET-BUF-1`**: Every `PacketBufferSlot` belongs to exactly one ownership domain (`FreePool`, `DriverRxRing`, `InterfaceRxQueue`, `SocketIngressQueue`, `SocketTxQueue`, `DriverTxRing`). Concurrent queue membership or orphaned buffers are strictly prohibited.
- **`I-NET-DMA-1`**: Network DMA ownership is represented exclusively through Stage 3L DMA tracking (`PHYSICAL_FRAME_PIN_TABLE`). Stage 3M cannot independently free a DMA-owned frame to PMM.
- **`I-NET-QUEUE-1`**: An RX/TX queue descriptor belongs to exactly one queue at every instant. When a queue is full, subsequent packets are deterministically dropped and counted in `dropped_packets` (`I-NET-QUEUE-DROP-1`).

---

## 4. Protocol-Layer Boundary & Responsibility Matrix

Stage 3M defines an explicit 5-layer hierarchy:

```text
+---------------------------------------------------------------------------------------+
| LAYER 5: SOCKET & APPLICATION INTERFACE (socket.rs, syscall/dispatch.rs)              |
| - KernelObjectType::Socket = 7 integration                                            |
| - Syscalls 22..31 (SYS_NET_SOCKET, SYS_NET_BIND, SYS_NET_SEND, SYS_NET_RECV, etc.)    |
| - WaitQueue blocking / parking / wakeup logic                                         |
+---------------------------------------------------------------------------------------+
                                           │
                                           ▼
+---------------------------------------------------------------------------------------+
| LAYER 4: TRANSPORT (UDP & TCP) (transport.rs)                                         |
| - UDP: Datagram framing, 8-byte header, pseudo-header checksum, port demultiplexing   |
| - TCP: RFC 793 11-state FSM, sequence numbers, SYN/ACK handshake, FIN teardown, RTO   |
+---------------------------------------------------------------------------------------+
                                           │
                                           ▼
+---------------------------------------------------------------------------------------+
| LAYER 3: NETWORK (IPv4 & ICMP) (ipv4.rs, icmp.rs)                                     |
| - IPv4 header verification, RFC 1071 ones-complement checksum, TTL decrement          |
| - Longest-Prefix Matching (LPM) routing engine (ROUTE_TABLE: 8 entries)               |
| - ICMP: Echo Request / Reply (ping), Destination Unreachable generation               |
| - Fragmentation Policy: DF=1 enforced; oversized packets return -EMSGSIZE            |
+---------------------------------------------------------------------------------------+
                                           │
                                           ▼
+---------------------------------------------------------------------------------------+
| LAYER 2: LINK & ADDRESS RESOLUTION (ethernet.rs, arp.rs)                              |
| - Ethernet II framing (14-byte header: Dest MAC, Src MAC, EtherType)                  |
| - Loopback interface (lo0, 127.0.0.1, MAC 00:00:00:00:00:00)                          |
| - ARP Cache & State Machine (NEIGHBOR_TABLE: 16 entries)                              |
+---------------------------------------------------------------------------------------+
                                           │
                                           ▼
+---------------------------------------------------------------------------------------+
| LAYER 1: NETWORK DEVICE & DMA (dev_binding.rs, dev/)                                  |
| - Stage 3L DeviceSlot binding (DeviceClass::Network)                                  |
| - PMM frame pinning via Stage 3L alloc_dma_buffer                                      |
| - Top-half ISR dispatch & Event signaling via dev::interrupt                          |
+---------------------------------------------------------------------------------------+
```

---

## 5. Socket Lifecycle & WaitQueue Blocking Semantics

Sockets are first-class kernel objects (`KernelObjectType::Socket = 7`) managed via `SOCKET_TABLE: [SocketSlot; 16]`.

### 5.1 Socket State Machines

#### UDP Sockets:
$$\text{Unbound (0)} \xrightarrow{\text{SYS\_NET\_BIND}} \text{Bound (1)} \xrightarrow{\text{SYS\_NET\_CONNECT}} \text{Connected (2)} \xrightarrow{\text{SYS\_NET\_CLOSE}} \text{Closed (3)}$$

#### TCP Sockets (RFC 793):
```text
                           +---------+
                           | CLOSED  |
                           +----+----+
                                |
             +------------------+------------------+
             |                                     |
     [Passive Open / Listen]               [Active Open / Connect]
             v                                     v
       +-----------+                         +-----------+
       |  LISTEN   |                         | SYN-SENT  |
       +-----+-----+                         +-----+-----+
             |                                     |
       [SYN Received]                        [SYN+ACK Recv]
             v                                     v
       +-----------+                         +-----------+
       | SYN-RCVD  |                         |ESTABLISHED|
       +-----+-----+                         +-----+-----+
             |                                     |
       [ACK Received]                              |
             v                                     |
       +-----------+                               |
       |ESTABLISHED|<------------------------------+
       +-----+-----+
             |
    +--------+--------+
    |                 |
[Close / FIN]    [Peer FIN]
    v                 v
+---------+     +------------+
|FIN-WAIT1|     | CLOSE-WAIT |
+----+----+     +-----+------+
     |                |
+----+----+     +-----+------+
|FIN-WAIT2|     |  LAST-ACK  |
+----+----+     +-----+------+
     |                |
+----+----+           |
|TIME-WAIT|           |
+----+----+           |
     |                |
     +--------+-------+
              v
          +--------+
          | CLOSED |
          +--------+
```

### 5.2 Blocking, Wakeup & Cancellation Semantics
Each socket slot contains a dedicated `WaitQueue` identifier. When an operation cannot make immediate progress, the thread is parked using the Stage 3D synchronization primitives:

```text
+-------------------+--------------------------------+--------------------------------------------------+
| Syscall           | Blocking Predicate             | Wakeup Events                                    |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_RECV      | rx_queue_count == 0            | 1. Packet enqueued to socket                     |
|                   |                                | 2. Peer closes connection (EOF / returns 0)      |
|                   |                                | 3. Interface faults -> wakes with -ENETDOWN      |
|                   |                                | 4. Timeout expires -> wakes with -ETIMEDOUT      |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_SEND      | tx_queue_count >= limit        | 1. Packet transmitted / buffer space available   |
|                   |                                | 2. Connection reset -> wakes with -ECONNRESET    |
|                   |                                | 3. Interface faults -> wakes with -ENETDOWN      |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_ACCEPT    | accept_queue_count == 0        | 1. Inbound SYN handshake reaches ESTABLISHED     |
|                   |                                | 2. Socket closed -> wakes with -EBADF            |
|                   |                                | 3. Interface faults -> wakes with -ENETDOWN      |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_CONNECT   | state == SynSent               | 1. Peer SYN-ACK received -> ESTABLISHED          |
|                   |                                | 2. Peer RST received -> -ECONNREFUSED            |
|                   |                                | 3. Connect timeout (200 ticks / 2s) -> -ETIMEDOUT|
|                   |                                | 4. Route lookup fails -> -ENETUNREACH            |
+-------------------+--------------------------------+--------------------------------------------------+
```

### 5.3 Invariants
- **`I-NET-SOCKET-1`**: A `SocketSlot` is not reusable until all capability references (`handle_refs`), in-flight operations (`in_flight_op_refs`), and waitqueue entries have reached zero, its port binding is purged, and queued packet buffers are recycled.
- **`I-NET-WAIT-1`**: Every blocking network operation has an explicit predicate and registered wakeup source.
- **`I-NET-DEV-FAIL-1`**: Device fault, hardware reset, or interface shutdown deterministically transitions affected interfaces, invalidates queues, and immediately wakes all blocked threads with `SyscallError::NetworkDown` (-19).

---

## 6. Authoritative Port-Binding Architecture

Local ports are registered in `PORT_BINDING_TABLE: [PortBindingSlot; 32]`.

### 6.1 Slot Layout
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PortBindingSlot {
    pub occupied: bool,             // Slot occupancy flag
    pub protocol: u8,               // 1 = UDP, 2 = TCP
    pub port: u16,                  // Bound local port (1..65535)
    pub ip_addr: u32,               // Bound local IP (0 = INADDR_ANY)
    pub socket_id: u64,             // Owning SocketId
    pub owner_pid: u64,             // Owning ProcessId
}
const _: () = assert!(core::mem::size_of::<PortBindingSlot>() == 24);
const _: () = assert!(core::mem::align_of::<PortBindingSlot>() == 8);
```

### 6.2 Binding Rules & Authority
1. **Namespace Isolation**: TCP and UDP maintain completely separate 65,536 port ranges.
2. **Conflict Detection**:
   - Exact collision: Same protocol, same port, matching IP $\implies$ rejected with `SyscallError::AddressInUse` (-26).
   - Wildcard collision: Binding `0.0.0.0` collides with any specific IP on that protocol/port. Binding a specific IP collides if `0.0.0.0` is already bound.
3. **Privileged Ports**: Ports `1..1023` require `NET_CONFIG` capability right. Unprivileged callers receive `SyscallError::PermissionDenied` (-4).
4. **Ephemeral Port Allocation**: Ports `49152..65535` are allocated monotonically on autobind (`SYS_NET_CONNECT` without prior `SYS_NET_BIND`).
5. **Invariant `I-NET-BIND-1`**: A successful bind creates exactly one authoritative protocol/address/port binding.

---

## 7. Deterministic Routing Engine

Routing decisions are made via `ROUTE_TABLE: [RouteEntry; 8]`.

### 7.1 Table Structure & Matching Rule
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RouteEntry {
    pub occupied: bool,             // Slot occupancy flag
    pub prefix_len: u8,             // Subnet prefix length (0..32)
    pub interface_id: u16,          // Bound interface identifier
    pub metric: u32,                // Route metric (lower = preferred)
    pub dest_ip: u32,               // Subnet network address
    pub gateway_ip: u32,            // Next-hop gateway IP (0 = directly connected link)
    pub _reserved: [u8; 12],        // Padding to 32 bytes
}
const _: () = assert!(core::mem::size_of::<RouteEntry>() == 32);
const _: () = assert!(core::mem::align_of::<RouteEntry>() == 8);
```

### 7.2 Lookup Algorithm
1. Mask destination address: `masked = dest_ip & netmask(prefix_len)`.
2. Match: `masked == route.dest_ip`.
3. Candidate selection:
   - Route with the **highest `prefix_len`** (longest-prefix match) wins.
   - On equal `prefix_len`, the route with the **lowest `metric`** wins.
4. If no route matches, return `SyscallError::NetworkUnreachable` (-20).
5. **Default Gateway**: A route with `dest_ip = 0.0.0.0` and `prefix_len = 0`.
6. **Invariant `I-NET-ROUTE-TEARDOWN-1`**: When an interface transitions to `Down` or is detached, all routes with `route.interface_id == target_if` are automatically cleared.

---

## 8. Deterministic ARP State Machine & Expiration

Neighbor mappings reside in `NEIGHBOR_TABLE: [NeighborEntry; 16]`.

### 8.1 State Machine
```text
+---------+      Send ARP Request       +------------+     ARP Reply Recv     +-----------+
|  EMPTY  | --------------------------> | INCOMPLETE | ---------------------> | REACHABLE |
+---------+                             +-----+------+                        +-----+-----+
                                              |                                     |
                                        Timeout / Retries                     30,000 Ticks
                                        Exhausted (100 Ticks)                 (300 s)
                                              |                                     |
                                              v                                     v
                                        +------------+                        +-----------+
                                        |   PURGED   |                        |   STALE   |
                                        +------------+                        +-----+-----+
                                                                                    |
                                                                              LRU Eviction
                                                                                    v
                                                                              +-----------+
                                                                              |   EMPTY   |
                                                                              +-----------+
```

### 8.2 Invariant `I-NET-ARP-1`
> ARP resolution is bounded: pending requests timeout after 100 ticks (1 second) with up to 3 retries. When the table is full, the oldest `Stale` entry is deterministically evicted. ARP lookup never blocks the kernel or allocates dynamic heap memory.

---

## 9. Capability Authority Decomposition & Rights Model

Networking rights are strictly decomposed to enforce the principle of least privilege:

```text
+---------------------------------------------------------------------------------------+
| TYPE-SPECIFIC RIGHTS (Bits 0..7 for KernelObjectType::Socket)                         |
+-------------------+--------+----------------------------------------------------------+
| Right             | Mask   | Scope / Authority Permitted                              |
+-------------------+--------+----------------------------------------------------------+
| NET_BIND          | 0x0001 | Bind unprivileged local port (>= 1024)                   |
| NET_LISTEN        | 0x0002 | Transition TCP socket to LISTEN state                    |
| NET_ACCEPT        | 0x0004 | Accept incoming TCP connection                           |
| NET_CONNECT       | 0x0008 | Initiate outbound TCP connection or connect UDP socket   |
| NET_SEND          | 0x0010 | Transmit payload on connected or datagram socket         |
| NET_RECV          | 0x0020 | Receive payload from socket                              |
| NET_ROUTE         | 0x0040 | Insert, update, or remove routing table entries          |
| NET_CONFIG        | 0x0080 | Configure interface IP/MAC, bind privileged ports (<1024)|
+-------------------+--------+----------------------------------------------------------+
| ADMINISTRATIVE / RAW RIGHTS (Bits 8..15)                                              |
+-------------------+--------+----------------------------------------------------------+
| NET_RAW           | 0x4000 | Open raw link-layer Ethernet socket (SocketType::Raw)    |
+-------------------+--------+----------------------------------------------------------+
```

### 9.1 Invariants
- **`I-NET-CAP-SUBSET-1`**: Capability derivation verifies `(child & !parent) == 0`. No derivation can amplify rights.
- **`I-NET-RAW-PRIV-1`**: `NET_RAW` allows receiving and sending raw Ethernet frames on a bound interface. It does **not** permit forging the physical interface's MAC address unless `NET_CONFIG` is also possessed. Promiscuous packet sniffing is strictly prohibited.
- **`I-NET-REVOKE-1`**: Revoking a socket capability prevents subsequent syscall execution on derived handles without affecting other processes.

---

## 10. Reconciled Syscall ABI Surface

### 10.1 System Call Numbers (22..31)
```text
+--------+------------------+------------------------------------+--------------------+
| Number | Name             | Signature                          | Required Right     |
+--------+------------------+------------------------------------+--------------------+
| 22     | SYS_NET_SOCKET   | (sock_type, protocol, flags)       | [Implicit Creator] |
| 23     | SYS_NET_BIND     | (handle, ip_be, port_le)           | NET_BIND           |
| 24     | SYS_NET_LISTEN   | (handle, backlog)                  | NET_LISTEN         |
| 25     | SYS_NET_ACCEPT   | (handle, out_sock_info_ptr)        | NET_ACCEPT         |
| 26     | SYS_NET_CONNECT  | (handle, ip_be, port_le)           | NET_CONNECT        |
| 27     | SYS_NET_SEND     | (handle, buf_ptr, len, flags)      | NET_SEND           |
| 28     | SYS_NET_RECV     | (handle, buf_ptr, len, flags)      | NET_RECV           |
| 29     | SYS_NET_CLOSE    | (handle)                           | CLOSE              |
| 30     | SYS_NET_QUERY    | (handle_or_zero, query_type, ptr)  | INSPECT            |
| 31     | SYS_NET_CONFIG   | (cmd, target_id, config_ptr)       | NET_CONFIG/ROUTE   |
+--------+------------------+------------------------------------+--------------------+
```

### 10.2 Reconciled Error Codes
Reconciled against existing frozen syscall error codes (-1..-18):
```rust
#[repr(i64)]
pub enum SyscallError {
    // Frozen 3I..3L Codes:
    Success          = 0,
    InvalidSyscall   = -1,   // -ENOSYS
    InvalidArgument  = -2,   // -EINVAL
    BadHandle        = -3,   // -EBADF
    PermissionDenied = -4,   // -EACCES
    BadAddress       = -5,   // -EFAULT
    OutOfMemory      = -6,   // -ENOMEM
    ResourceBusy     = -7,   // -EBUSY
    NotFound         = -8,   // -ENOENT
    WouldBlock       = -9,   // -EAGAIN
    PeerClosed       = -10,  // -EPIPE
    NoSpace          = -11,  // -ENOSPC
    FileExists       = -12,  // -EEXIST
    IoError          = -13,  // -EIO
    NotADirectory    = -14,  // -ENOTDIR
    IsADirectory     = -15,  // -EISDIR
    TableFull        = -16,  // -ENFILE
    ResourceConflict = -17,  // -EEXIST/-EBUSY
    DeviceFault      = -18,  // -EIO / Hardware fault

    // Reconciled Stage 3M Networking Codes:
    NetworkDown          = -19,  // -ENETDOWN: Interface or physical device down/faulted
    NetworkUnreachable   = -20,  // -ENETUNREACH: No route to destination host
    ConnectionRefused    = -21,  // -ECONNREFUSED: Peer rejected connection (TCP RST)
    ConnectionReset      = -22,  // -ECONNRESET: Established connection reset by peer
    ConnectionAborted    = -23,  // -ECONNABORTED: Connection aborted locally
    AlreadyConnected     = -24,  // -EISCONN: Socket already connected
    NotConnected         = -25,  // -ENOTCONN: Operation requires connected socket
    AddressInUse         = -26,  // -EADDRINUSE: Local port/IP binding collision
    AddressNotAvailable  = -27,  // -EADDRNOTAVAIL: Requested IP address not assigned
    TimedOut             = -28,  // -ETIMEDOUT: Connection or transmission timed out
    MessageTooLarge      = -29,  // -EMSGSIZE: Packet exceeds MTU (DF=1)
}
```

---

## 11. Exact `.bss` Memory Budget & 2 MiB Window Compliance

```text
+----------------------------+-----------+--------------------+---------------------+
| Table / Structure          | Count     | Size per Entry     | Total Size (Bytes)  |
+----------------------------+-----------+--------------------+---------------------+
| INTERFACE_TABLE            | 4         | 64 B               | 256 B               |
| NETWORK_DEVICE_BINDINGS    | 4         | 32 B               | 128 B               |
| PACKET_BUFFER_TABLE        | 32        | 48 B               | 1,536 B             |
| SOCKET_TABLE               | 16        | 64 B               | 1,024 B             |
| ROUTE_TABLE                | 8         | 32 B               | 256 B               |
| NEIGHBOR_TABLE             | 16        | 32 B               | 512 B               |
| PORT_BINDING_TABLE         | 32        | 24 B               | 768 B               |
| NETWORK_TIMER_TABLE        | 16        | 16 B               | 256 B               |
| SUBSYSTEM SPINLOCKS        | 4         | 8 B                | 32 B                |
+----------------------------+-----------+--------------------+---------------------+
| TOTAL STATIC .BSS FOOTPRINT                                 | 4,768 B (~4.65 KiB) |
+----------------------------+-----------+--------------------+---------------------+
```

### Memory Headroom Analysis:
- In debug build, kernel `.bss` ends at `0xFFFFFFFF801E6C99`.
- The fixed sections (`.pmm_metadata`, `.page_tables`, `.stack_guard`, `.stack`) occupy exactly 100 KiB from `0xFFFFFFFF801E7000` to `0xFFFFFFFF80200000`.
- Remaining margin before the 4 KiB alignment boundary is 871 bytes.
- When new static variables exceed 871 bytes, the alignment pushes `__kernel_end` to `0xFFFFFFFF80201000`, which would violate the bootstrap assertion unless optimized.
- **Optimization Contract**:
  - `BlockBuffer` in `kernel/src/fs/buf.rs` is currently 65,920 bytes placed in `.data` due to non-zero `device_id: 0xFF`. Changing default initialization to 0 moves `BUFFERS` into `.bss` without affecting semantics.
  - Test printout string constants in `tests.rs` are deduplicated.
  - Net result: Stage 3M easily satisfies `__kernel_end <= 0xFFFFFFFF80200000`.

---

## 12. Monotonic Lock Hierarchy (Levels 1..9)

The lock hierarchy strictly preserves the frozen Stages 3A–3L ordering:

$$\begin{aligned}
\text{Level 1: } & \text{FILESYSTEM\_LOCK} \\
\text{Level 2: } & \text{STORAGE\_OBJECT\_TABLE\_LOCK} \\
\text{Level 3: } & \text{BLOCK\_CACHE\_LOCK} \\
\text{Level 4: } & \text{BLOCK\_DEVICE\_LOCK} \\
\text{Level 5A: } & \text{NETWORK\_STACK\_LOCK} \quad (\text{Subsystem state, buffer pool, routing, ARP}) \\
\text{Level 5B: } & \text{NETWORK\_INTERFACE\_LOCK} \quad (\text{Per-interface RX/TX queue rings}) \\
\text{Level 5C: } & \text{NETWORK\_SOCKET\_TABLE\_LOCK} \quad (\text{Socket table lookup, port binding}) \\
\text{Level 5D: } & \text{DEVICE\_REGISTRY\_LOCK} \quad (\text{Stage 3L device table}) \\
\text{Level 6: } & \text{DEVICE\_RESOURCE\_LOCK} \quad (\text{Stage 3L resource table}) \\
\text{Level 7: } & \text{KERNEL\_OBJECT\_TABLE\_LOCK} \quad (\text{Stage 3G object table}) \\
\text{Level 8: } & \text{SCHEDULER.lock} \quad (\text{Stage 3B scheduler}) \\
\text{Level 9: } & \text{CPU (IF=0)} \quad (\text{Interrupt-disabled boundary})
\end{aligned}$$

- Locks must always be acquired in increasing numerical order ($\text{Level } N < \text{Level } (N+1)$) and released in reverse order.
- Top-half ISR routines execute at Level 9 and never acquire Level 1..8 spinlocks.

---

## 13. Process Exit Teardown & Lifetime Invariants

### 13.1 Invariant `I-NET-TEARDOWN-1`
> When a process terminates (`sys_exit`):
> 1. All capability handles in its `HandleTable` are closed.
> 2. Sockets with `owner_pid == terminating_pid` have:
>    - Established TCP connections terminated with `RST`.
>    - Bound ports freed in `PORT_BINDING_TABLE`.
>    - Queued inbound packet buffers recycled to `FreePool`.
>    - Sockets marked `Closed` and slots freed once `in_flight_op_refs == 0`.
> 3. Blocked threads on those sockets are dequeued.
> 4. Shared network interfaces and physical devices remain operational for sibling processes.
> 5. Zero PMM frames or DMA pins are leaked (`baseline_free == post_test_free`).

---

## 14. Machine-Level Verification Suite (26 Tests: 3M-A through 3M-Z)

The implementation must pass all 26 machine verification tests in QEMU:

```text
+------+------------------------------------+---------------------------------------------------------+
| Test | Name                               | Invariant / Focus Exercised                             |
+------+------------------------------------+---------------------------------------------------------+
| 3M-A | Network Identity & Monotonicity    | Generation-safe IDs, no ABA reuse (I-NET-ID-1)           |
| 3M-B | Interface Registration & lo0 Setup | Loopback interface setup, MAC, MTU (1500), IP 127.0.0.1 |
| 3M-C | Stage 3L Device Binding & Lifetime | Interface cannot outlive Stage 3L Device (I-NET-DEV-1)  |
| 3M-D | Packet Buffer Pool Allocation      | Bounded pool allocation, recycling, and state machine   |
| 3M-E | Single Packet Ownership Model      | Exactly one owner at every instant (I-NET-BUF-1)        |
| 3M-F | Stage 3L DMA Frame Pin Integration | Physical frame pinning & DMA tracking (I-NET-DMA-1)     |
| 3M-G | RX Queue Enqueue & Dequeue         | Intrusive FIFO queue order and boundary checks          |
| 3M-H | TX Queue Backpressure & Drops      | Queue-full deterministic drop semantics (I-NET-QUEUE-1) |
| 3M-I | Top-Half IRQ & Event Signaling     | Interrupt dispatch to network binding and event wake    |
| 3M-J | IRQ Storm Isolation on Network Line| Storm threshold 1000/10ms, 50-tick cooldown (I-NET-IRQ) |
| 3M-K | Ethernet Frame Validation          | MAC match, EtherType parsing, length checks             |
| 3M-L | IPv4 Header & Checksum Fidelity    | RFC 1071 ones-complement checksum, TTL, DF=1 check      |
| 3M-M | ARP Cache & Deterministic Eviction | State walk (Empty->Incomplete->Reachable->Stale), LRU   |
| 3M-N | Longest-Prefix Matching Routing    | Longest prefix wins, metric tie-breaker, default route  |
| 3M-O | Route Teardown on Interface Down   | Routes purged when interface detaches (I-NET-ROUTE-1)   |
| 3M-P | Socket Creation & Object Table     | KernelObjectType::Socket=7 integration, slot pinning   |
| 3M-Q | Port Binding & Collision Rejection | Ephemeral ports, duplicate bind rejected (-EADDRINUSE)  |
| 3M-R | Privileged Port Authority (1..1023)| NET_CONFIG required for ports < 1024, unpriv rejected   |
| 3M-S | UDP Datagram Flow (Loopback)       | Datagram send/recv bit-identical round-trip             |
| 3M-T | TCP 11-State Machine Handshake Walk| Closed -> Listen -> SynRcvd -> Established -> Closed    |
| 3M-U | Blocking Recv & Thread Wakeup      | Thread parking on WaitQueue and wakeup on packet arrival|
| 3M-V | Non-Blocking Operations (-EAGAIN)  | Immediate return of SyscallError::WouldBlock            |
| 3M-W | Device Fault & Blocked Wakeup      | Device fault immediately wakes blocked threads (I-NET-1)|
| 3M-X | Process Exit Teardown Cleanup      | Terminating PID sockets/buffers freed (I-NET-TEARDOWN-1)|
| 3M-Y | Capability Revocation Cascade      | Invalidation of derived handles prevents further I/O    |
| 3M-Z | Concurrency, Lock Order & PMM Neutral| Simultaneous lock acquisition; baseline_free == post  |
+------+------------------------------------+---------------------------------------------------------+
```

---

## 15. Acceptance Criteria

1. **QEMU Bare-Metal Verification**:
   All 26 tests (`3M-A` through `3M-Z`) must execute sequentially in QEMU and output:
   ```text
   [Stage 3M Verification Complete: 26/26 tests PASSED]
   Cumulative Machine Tests: 287 tests (181 baseline + 18 3I + 16 3J + 20 3K + 26 3L + 26 3M)
   ```
2. **PMM Neutrality**:
   `baseline_free == post_test_free` (zero net physical frames leaked).
3. **Clean Shutdown**:
   `isa-debug-exit` returns exit code 33 (0x21).
4. **Host Pytest Suite**:
   All 30 test files (including new `tests/test_stage3m.py`, 113+ test items) must pass with zero failures.
5. **Frozen ABI Integrity**:
   `Process` (128 B), `KernelThread` (176 B), `PerCpu` (48 B), `HandleTable` (520 B), `CapabilityNode` (24 B), and `KernelObjectSlot` (40 B) remain byte-identical.
6. **Kernel Footprint**:
   `__kernel_end <= 0xFFFFFFFF80200000` (fits completely within the 2 MiB bootstrap window).

---

## ARCHITECTURE STATUS:
🟡 **PROPOSED (Rev2) — AWAITING REVIEW**
