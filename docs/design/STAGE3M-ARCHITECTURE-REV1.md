# Stage 3M Architecture Specification Rev1 — Networking Model

**Author**: Google DeepMind Advanced Agentic Coding (Antigravity)  
**Date**: September 17, 2026  
**Status**: 🟡 **PROPOSED — AWAITING REVIEW**  
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

### 1.1 Core Architectural Principles
1. **Capability-Native Authority**: Ambient network access is strictly prohibited. Sockets, interfaces, routing table entries, and raw packet channels are guarded by capability nodes; rights amplification is mathematically impossible: `(child_rights & !parent_rights) == 0`.
2. **Zero Dynamic Kernel Heap**: Dynamic memory allocators (`kmalloc`, `malloc`) are forbidden. All packet queues, socket slots, routing tables, and interface descriptors reside in statically sized tables in `.bss`.
3. **Stage 3L Device Model Integration**: Network devices are physical devices (`DeviceClass::Network`) registered in `DEVICE_TABLE`. Physical frame memory for packet transfer is allocated and pinned via Stage 3L's `alloc_dma_buffer` and `PHYSICAL_FRAME_PIN_TABLE`.
4. **Deterministic Lifecycles**: Process termination reclaims only process-owned sockets, bindings, and DMA buffers without disrupting shared network interfaces or sibling processes (`I-NET-LIFETIME-1`).
5. **Lock Hierarchy Compliance**: Extends the established Levels 1..9 lock hierarchy monotonically without cycles or inversion.
6. **Future SMP Readiness**: Data structures and spinlocks avoid single-CPU assumptions, ensuring an explicit migration path to Stage 3N multi-core execution.

---

## 2. Network Identity & Descriptor Model

Network entities have generation-safe, monotonically allocated identifiers decoupled from physical table indices to prevent ABA vulnerabilities.

```text
+----------------------+--------------------+---------------------------------------------+
| Identifier Type      | Underlying Type    | Semantic Role                               |
+----------------------+--------------------+---------------------------------------------+
| NetworkDeviceId      | u64 (DeviceId)     | Stage 3L physical device reference          |
| NetworkInterfaceId   | u16                | Monotonic logical interface ID (lo0=1, ...) |
| NetworkBufferId      | u32                | High 16-bit generation, low 16-bit slot idx |
| SocketId             | u64                | Monotonic unique socket object identifier   |
| ConnectionId         | u64                | Monotonic active transport connection ID    |
+----------------------+--------------------+---------------------------------------------+
```

### 2.1 Invariant `I-NET-ID-1`: Generation-Safe Descriptors
> A network descriptor cannot be resolved or operated on across generation mismatches. When a slot is recycled, its generation counter increments. Stale capability handles or descriptors are rejected with `SyscallError::BadHandle` (-3).

---

## 3. Data Structures & Memory Layouts

All networking structures have fixed, compiler-asserted byte sizes and 8-byte alignments.

### 3.1 `PacketBufferSlot` (48 bytes, 8-byte aligned)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PacketBufferSlot {
    pub buffer_id: u32,             // Generation-safe packet buffer identifier
    pub state: PacketBufferState,   // Free, RxAllocated, Queued, TxPending, Transmitted
    pub _pad0: [u8; 3],             // Explicit padding
    pub interface_id: u16,          // Bound interface identifier
    pub dma_buffer_id: u32,         // Stage 3L DmaBufferDescriptor ID
    pub phys_addr: u64,             // Base physical memory address of frame
    pub data_offset: u16,           // Offset of valid packet payload (headroom)
    pub data_len: u16,              // Length of valid packet payload
    pub flags: u32,                 // Checksum offload / loopback flags
    pub timestamp_ticks: u64,       // Timestamp tick of allocation
    pub next_buffer_idx: u16,       // Queue linkage index (0xFFFF = None)
    pub _pad1: [u8; 6],             // Padding to 48 bytes
}
const _: () = assert!(core::mem::size_of::<PacketBufferSlot>() == 48);
const _: () = assert!(core::mem::align_of::<PacketBufferSlot>() == 8);
```

### 3.2 `NetworkInterfaceSlot` (64 bytes, 8-byte aligned)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NetworkInterfaceSlot {
    pub interface_id: u16,          // Logical interface identifier
    pub device_id: u64,             // Bound Stage 3L DeviceId (0 for loopback)
    pub if_type: InterfaceType,     // Loopback (1), Ethernet (2)
    pub state: InterfaceState,      // Down (0), Up (1), Faulted (2)
    pub mtu: u16,                   // Maximum Transmission Unit (typically 1500)
    pub mac_addr: [u8; 6],          // Hardware MAC address (48-bit)
    pub ipv4_addr: u32,             // Assigned IPv4 address (Big-Endian)
    pub ipv4_netmask: u32,          // Subnet mask (e.g. 0xFFFFFF00 for /24)
    pub rx_head: u16,               // RX packet queue head index
    pub rx_tail: u16,               // RX packet queue tail index
    pub rx_count: u16,              // Current queued RX packet count
    pub tx_head: u16,               // TX packet queue head index
    pub tx_tail: u16,               // TX packet queue tail index
    pub tx_count: u16,              // Current queued TX packet count
    pub rx_packets_total: u64,      // Lifetime statistics: received packets
    pub tx_packets_total: u64,      // Lifetime statistics: transmitted packets
    pub dropped_packets: u32,       // Queue overflow dropped packet count
    pub _reserved: u32,             // Padding to 64 bytes
}
const _: () = assert!(core::mem::size_of::<NetworkInterfaceSlot>() == 64);
const _: () = assert!(core::mem::align_of::<NetworkInterfaceSlot>() == 8);
```

### 3.3 `SocketSlot` (64 bytes, 8-byte aligned)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SocketSlot {
    pub occupied: bool,             // Slot occupancy flag
    pub sock_type: SocketType,      // Udp (1), Tcp (2), Raw (3)
    pub state: SocketState,         // Closed, Listen, SynSent, Established, etc.
    pub _pad0: u8,                  // Explicit alignment
    pub socket_id: u64,             // Globally unique socket ID
    pub kernel_object_id: u64,      // Associated KernelObjectSlot ID
    pub owner_pid: u64,             // Owning ProcessId
    pub bound_interface: u16,       // Bound interface ID (0 = any)
    pub local_port: u16,            // Bound local port (Host Endian)
    pub remote_port: u16,           // Connected remote port
    pub local_ip: u32,              // Local IPv4 address
    pub remote_ip: u32,             // Remote IPv4 address
    pub rx_queue_head: u16,         // Head of socket ingress packet queue
    pub rx_queue_tail: u16,         // Tail of socket ingress packet queue
    pub rx_queue_count: u16,        // Number of queued ingress packets
    pub rx_queue_limit: u16,        // Maximum queued packets allowed (backpressure)
    pub bound_waitqueue_id: u64,    // Ticket/WaitQueue ID for blocking operations
    pub _reserved: [u8; 8],         // Padding to 64 bytes
}
const _: () = assert!(core::mem::size_of::<SocketSlot>() == 64);
const _: () = assert!(core::mem::align_of::<SocketSlot>() == 8);
```

### 3.4 `RouteEntry` (32 bytes, 8-byte aligned)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RouteEntry {
    pub occupied: bool,
    pub prefix_len: u8,             // CIDR prefix length (0..32)
    pub interface_id: u16,          // Outgoing interface ID
    pub metric: u32,                // Route metric (lower = preferred)
    pub dest_ip: u32,               // Destination subnet prefix
    pub gateway_ip: u32,            // Next-hop gateway IP (0 = direct link)
    pub gen_id: u32,                // Route generation ID
    pub _reserved: [u8; 12],        // Padding to 32 bytes
}
const _: () = assert!(core::mem::size_of::<RouteEntry>() == 32);
const _: () = assert!(core::mem::align_of::<RouteEntry>() == 8);
```

### 3.5 `NeighborEntry` (32 bytes, 8-byte aligned)
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NeighborEntry {
    pub occupied: bool,
    pub state: NeighborState,       // Incomplete (1), Reachable (2), Stale (3)
    pub interface_id: u16,          // Bound interface
    pub ip_addr: u32,               // IPv4 address
    pub mac_addr: [u8; 6],          // Resolved Ethernet MAC address
    pub updated_ticks: u64,         // Last update timestamp ticks
    pub _reserved: [u8; 8],         // Padding to 32 bytes
}
const _: () = assert!(core::mem::size_of::<NeighborEntry>() == 32);
const _: () = assert!(core::mem::align_of::<NeighborEntry>() == 8);
```

---

## 4. Bounded Resource Budget & `.bss` Memory Footprint

To ensure strict compliance with the **2 MiB bootstrap kernel window** (`__kernel_end <= 0xFFFFFFFF80200000`), table capacities are sized conservatively:

```text
+----------------------------+-----------+--------------------+---------------------+
| Static Table Name          | Capacity  | Entry Size (bytes) | Total Size (bytes)  |
+----------------------------+-----------+--------------------+---------------------+
| INTERFACE_TABLE            | 4         | 64 B               | 256 B               |
| PACKET_BUFFER_TABLE        | 32        | 48 B               | 1,536 B             |
| SOCKET_TABLE               | 16        | 64 B               | 1,024 B             |
| ROUTE_TABLE                | 8         | 32 B               | 256 B               |
| NEIGHBOR_TABLE             | 16        | 32 B               | 512 B               |
| PORT_BINDING_TABLE         | 32        | 16 B               | 512 B               |
| NETWORK_TIMER_TABLE        | 16        | 16 B               | 256 B               |
+----------------------------+-----------+--------------------+---------------------+
| TOTAL STATIC TABLE FOOTPRINT                                | 4,352 B (~4.25 KiB) |
+----------------------------+-----------+--------------------+---------------------+
```

### 4.1 Packet Payload Buffers in DMA Space
Rather than allocating 64 KiB of fixed buffer memory in `.bss`, packet payload frames (2048 bytes each) are allocated dynamically from PMM via Stage 3L `alloc_dma_buffer` during network subsystem initialization and tracked in `DMA_BUFFER_TABLE`.
- Total pinned frames: 16 frames (64 KiB total, providing 32 packet slots of 2048 bytes).
- PMM accounting: Captured during `init()` and released during teardown, guaranteeing exact PMM neutrality.

---

## 5. Packet Buffer & DMA Ownership Model

Packet buffers obey an explicit ownership state machine:

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

### 5.1 Invariant `I-NET-BUF-OWNERSHIP-1`
> Every packet buffer has exactly one owner at any instant. Double-frees, concurrent queue memberships, or orphaned buffers are strictly prevented by atomic state transitions under `NETWORK_STACK_LOCK`.

### 5.2 Invariant `I-NET-DMA-PIN-1`
> Physical memory backing packet buffers is pinned in Stage 3L's `PHYSICAL_FRAME_PIN_TABLE`. The physical frame cannot transition to `Free` in PMM while any packet buffer descriptor references it.

---

## 6. RX/TX Queue Architecture & Backpressure

Both physical network interfaces and socket endpoints utilize bounded FIFO queues constructed with intrusive indexes in `PACKET_BUFFER_TABLE`.

### 6.1 Backpressure & Deterministic Packet Drop
- When a socket ingress queue reaches `rx_queue_limit` (default: 8 packets), subsequent incoming packets addressed to that socket are dropped immediately.
- When an interface TX ring is full (`tx_count == MAX_TX_DESCRIPTORS`), non-blocking transmission returns `SyscallError::WouldBlock` (-9). Blocking transmission waits on the interface's TX `WaitQueue`.
- Invariant `I-NET-QUEUE-DROP-1`: Packet drops never panic the kernel, never leak DMA frames, and increment `dropped_packets` monotonically.

---

## 7. Interrupt & Polling Integration (Reusing Stage 3L)

Physical NIC interrupts integrate directly into Stage 3L's top-half ISR dispatcher:

```text
NIC Hardware Interrupt (e.g. Vector 48)
            ↓
cpu::lapic_eoi() (Immediate Hardware EOI)
            ↓
dev::interrupt::dispatch_interrupt(48)
            ↓
Broadcast Delivery to Network Binding Slot
            ↓
NIC Top-Half Function:
  1. Acknowledge interrupt in NIC Status Register (Port-I/O or MMIO)
  2. Signal interface's bound kernel Event object (signals |= 1)
            ↓
Scheduler wakes bottom-half network worker or calling thread
```

### 7.1 Invariant `I-NET-IRQ-STORM-1`
> NIC interrupt lines are subject to Stage 3L's storm mitigation contract (`MAX_IRQS_PER_TICK = 1000`, `STORM_COOLDOWN_TICKS = 50`). If an erroneous NIC floods interrupts (> 1,000 in a 10 ms tick), the line is hardware-masked and cooled down for 500 ms without affecting the loopback interface, other drivers, or the kernel.

---

## 8. Link Layer: Ethernet & Loopback Subsystems

### 8.1 Ethernet Framing
- Standard Ethernet II frame format:
  - Destination MAC: 6 bytes
  - Source MAC: 6 bytes
  - EtherType: 2 bytes (`0x0800` IPv4, `0x0806` ARP)
  - Payload: 46 to 1500 bytes (padded with zeroes if payload < 46 bytes)
  - FCS: 4 bytes (verified by hardware or loopback driver)

### 8.2 Loopback Interface (`lo0`)
- `interface_id = 1`, MAC: `00:00:00:00:00:00`, IPv4: `127.0.0.1/8`.
- Transmitted packets on `lo0` are directly routed to the loopback RX queue via an atomic pointer flip without physical DMA operations.
- Guarantees intra-node networking functionality even in the absence of physical hardware.

---

## 9. Network Layer: Addressing, ARP & Routing

### 9.1 Address Resolution Protocol (ARP)
- Bounded ARP Cache: `NEIGHBOR_TABLE: [NeighborEntry; 16]`.
- States: `Incomplete` (request sent), `Reachable` (resolved), `Stale` (requires refresh).
- LRU eviction when all 16 slots are populated.
- Pre-configured entry: `127.0.0.1` maps to `00:00:00:00:00:00`.

### 9.2 IPv4 Processing & Longest-Prefix Matching
- IPv4 header verification: version == 4, IHL >= 5, total_length <= packet_len, header checksum verification via RFC 1071 ones-complement sum.
- Routing Table: `ROUTE_TABLE: [RouteEntry; 8]`.
- Lookup algorithm: Longest-Prefix Matching (LPM):
  $$\text{match}(dest, route) \iff (dest \ \& \ \text{mask}(route.prefix\_len)) == route.dest\_ip$$
  Among all matches, the route with the highest `prefix_len` (and lowest `metric` on tie) is selected.
- Default route (`0.0.0.0/0`) supported as fallback.

---

## 10. Transport Layer: UDP Datagrams & TCP State Machine

### 10.1 UDP (User Datagram Protocol)
- Connectionless, message-preserving endpoint.
- 8-byte UDP header: `source_port`, `dest_port`, `length`, `checksum`.
- Supports atomic datagram reception (`SYS_NET_RECV`) and transmission (`SYS_NET_SEND`).

### 10.2 TCP (Transmission Control Protocol) Finite State Machine
TCP endpoints strictly implement the authoritative RFC 793 state machine:

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

Stage 3M Architecture Rev1 defines the full state machine and packet wire layout. Initial machine verification exercises connection establishment, loopback data flow, and tear-down transitions.

---

## 11. Socket & Endpoint Model (`KernelObjectType::Socket = 7`)

Sockets are instantiated as first-class kernel objects.

### 11.1 Integration with `KernelObjectSlot`
- `KernelObjectType::Socket = 7` is appended to `KernelObjectType` enum.
- Sockets use `KernelObjectHeader`:
  - `object_id`: Unique 64-bit ID.
  - `creator_pid`: Process that created the socket.
  - `handle_refs`: Active capability handles pointing to this socket.
  - `signals`: Bit 0 (`SOCK_SIGNAL_READABLE`), Bit 1 (`SOCK_SIGNAL_WRITABLE`), Bit 2 (`SOCK_SIGNAL_PEER_CLOSED`), Bit 3 (`SOCK_SIGNAL_ERROR`).

---

## 12. Capability Model & Networking Rights

Networking operations are strictly authorized through `CapabilityNode.rights` (Bits 0..7 for `KernelObjectType::Socket`):

```rust
pub mod net_rights {
    pub const NET_BIND:    u16 = 1 << 0; // 0x0001: Bind local IP and port
    pub const NET_LISTEN:  u16 = 1 << 1; // 0x0002: Listen for incoming connections
    pub const NET_ACCEPT:  u16 = 1 << 2; // 0x0004: Accept incoming connection
    pub const NET_CONNECT: u16 = 1 << 3; // 0x0008: Connect to remote endpoint
    pub const NET_SEND:    u16 = 1 << 4; // 0x0010: Transmit data
    pub const NET_RECV:    u16 = 1 << 5; // 0x0020: Receive data
    pub const NET_CONFIG:  u16 = 1 << 6; // 0x0040: Configure interface/routes
    pub const NET_RAW:     u16 = 1 << 7; // 0x0080: Send/receive raw Ethernet frames
}
```

### 12.1 Invariant `I-NET-CAP-SUBSET-1`
> When deriving a child capability handle via `SYS_CAP_DERIVE` (10), child rights must satisfy:
> `(child_rights & !parent_rights) == 0`
> Attempting to derive `NET_RAW` or `NET_CONFIG` from a socket lacking those rights is rejected with `SyscallError::PermissionDenied` (-4).

---

## 13. Syscall ABI Surface (Syscalls 22..31)

Stage 3M introduces 10 dedicated system calls without modifying existing numbers 1..21:

```text
+--------+------------------+----------------------------------+--------------------+
| Number | Name             | Signature                        | Required Right     |
+--------+------------------+----------------------------------+--------------------+
| 22     | SYS_NET_SOCKET   | (sock_type, protocol, flags)     | [Implicit Creator] |
| 23     | SYS_NET_BIND     | (handle, ip_be, port_le)         | NET_BIND           |
| 24     | SYS_NET_LISTEN   | (handle, backlog)                | NET_LISTEN         |
| 25     | SYS_NET_ACCEPT   | (handle, out_sock_info_ptr)      | NET_ACCEPT         |
| 26     | SYS_NET_CONNECT  | (handle, ip_be, port_le)         | NET_CONNECT        |
| 27     | SYS_NET_SEND     | (handle, buf_ptr, len, flags)    | NET_SEND           |
| 28     | SYS_NET_RECV     | (handle, buf_ptr, len, flags)    | NET_RECV           |
| 29     | SYS_NET_CLOSE    | (handle)                         | CLOSE              |
| 30     | SYS_NET_QUERY    | (handle_or_zero, query_type, ptr)| INSPECT            |
| 31     | SYS_NET_CONFIG   | (cmd, target_id, config_ptr)     | NET_CONFIG         |
+--------+------------------+----------------------------------+--------------------+
```

### 13.1 Syscall Error Codes
Existing `SyscallError` codes are extended monotonically:
- `SyscallError::NetworkDown = -19` (-ENETDOWN)
- `SyscallError::NetworkUnreachable = -20` (-ENETUNREACH)
- `SyscallError::ConnectionRefused = -21` (-ECONNREFUSED)
- `SyscallError::ConnectionReset = -22` (-ECONNRESET)
- `SyscallError::AlreadyConnected = -24` (-EISCONN)
- `SyscallError::NotConnected = -25` (-ENOTCONN)
- `SyscallError::AddressInUse = -26` (-EADDRINUSE)
- `SyscallError::AddressNotAvailable = -27` (-EADDRNOTAVAIL)
- `SyscallError::TimedOut = -28` (-ETIMEDOUT)

---

## 14. Blocking, Timeout & Asynchronous Semantics

- **Non-blocking Mode**: If `flags & SOCK_NONBLOCK != 0` and progress cannot be made (e.g. empty RX queue or full TX queue), syscall returns immediately with `SyscallError::WouldBlock` (-9).
- **Blocking Mode**: The calling thread assigns its thread ID to the socket's `WaitQueue` and transitions to `ThreadState::Blocked` via `SCHEDULER.park_current()`.
- **Wakeup Trigger**: Arrival of an inbound packet, completion of a connection handshake, or peer closure invokes `SCHEDULER.wake_with_ticket()`.
- **Hardware Fault Wakeup (`I-NET-BLOCK-1`)**: If the underlying network device faults or is detached, all threads blocked on that interface's sockets are woken immediately with `SyscallError::NetworkDown` (-19).

---

## 15. Timers & Expiration Engine

Networking timers are tracked in `NETWORK_TIMER_TABLE: [NetworkTimerSlot; 16]` and driven by LAPIC timer ticks (`dev::interrupt::on_timer_tick` / 10 ms resolution):
1. **ARP Cache Timeout**: 300 seconds (30,000 ticks); stale entries transition to `Stale` and are purged.
2. **TCP Retransmission Timeout (RTO)**: Exponential backoff starting at 200 ms (20 ticks).
3. **TCP TIME_WAIT Timeout**: 60 seconds (6,000 ticks) before transitioning to `Closed`.

---

## 16. Port Allocation & Binding Semantics

- Bounded Table: `PORT_BINDING_TABLE: [PortBindingSlot; 32]`.
- Ephemeral Port Range: Ports `49152..65535` are allocated sequentially with wraparound and collision checking.
- Privileged Port Policy: Ports `1..1023` require `NET_CONFIG` right.
- Invariant `I-NET-PORT-CONFLICT-1`: Attempting to bind a local port already bound to the same protocol on the same interface returns `SyscallError::AddressInUse` (-26).

---

## 17. Security & Isolation Model

1. **Ambient Access Elimination**: A process has zero network visibility unless granted an explicit capability handle.
2. **Address & Identity Isolation**: Knowing an arbitrary `SocketId` or `NetworkBufferId` grants no authority; operations require valid capability handle resolution within the caller's `HandleTable`.
3. **Raw Packet Protection**: Raw socket creation (`SYS_NET_SOCKET` with `SocketType::Raw`) requires `NET_RAW` right; prevents unprivileged processes from sniffing link-layer traffic or forging Ethernet headers.
4. **DMA Memory Isolation**: Packet DMA buffers are accessible only to the NIC driver; user space interacts solely through sanitized system call copy buffers (`SYS_NET_SEND` / `SYS_NET_RECV`).

---

## 18. Process Teardown & Lifetime Invariants

### 18.1 Invariant `I-NET-LIFETIME-1`
> When a process terminates (`sys_exit`):
> 1. All capability handles in its `HandleTable` are closed.
> 2. Sockets with `handle_refs == 0` have active connections terminated with TCP RST, port bindings released, and queued packet buffers returned to the free pool.
> 3. Blocked threads on those sockets are dequeued.
> 4. Shared network interfaces and physical devices remain active for sibling processes.
> 5. Zero physical frames or DMA pins are leaked.

---

## 19. Monotonic Lock Hierarchy

The lock hierarchy strictly inserts networking locks between Level 4 and Level 6:

```text
Level 1:  FILESYSTEM_LOCK
Level 2:  STORAGE_OBJECT_TABLE_LOCK
Level 3:  BLOCK_CACHE_LOCK
Level 4:  BLOCK_DEVICE_LOCK
Level 5A: NETWORK_STACK_LOCK          <-- Global network subsystem & tables
Level 5B: NETWORK_INTERFACE_LOCK      <-- Per-interface queue synchronization
Level 5C: NETWORK_SOCKET_TABLE_LOCK   <-- Socket table lookup & state updates
Level 5D: DEVICE_REGISTRY_LOCK        <-- Stage 3L device table lock
Level 6:  DEVICE_RESOURCE_LOCK        <-- Stage 3L resource table lock
Level 7:  KERNEL_OBJECT_TABLE_LOCK    <-- Stage 3G object table lock
Level 8:  SCHEDULER.lock              <-- Stage 3B scheduler lock
Level 9:  CPU (IF=0)                  <-- Interrupt disable boundary
```

Cross-subsystem acquisition must always proceed downwards: $\text{Level } N < \text{Level } (N+1)$. ISR top-half routines execute at Level 9 without acquiring Level 1..8 spinlocks.

---

## 20. Future SMP & Personal Compute Fabric Compatibility

While single-core execution is maintained in Stage 3M, all data structures are designed to be SMP-safe for Stage 3N:
- Spinlocks employ `AtomicBool` with `Acquire`/`Release` ordering and interrupt-save/restore semantics.
- Per-interface RX/TX queues are decoupled from global state, enabling per-core affinity in Stage 3N.
- Sockets use fine-grained waitqueues rather than a single kernel-wide event loop.

---

## 21. ZeroFS Storage Interaction

Networking is independent of persistent storage:
- Static network configuration (IP addresses, static routes) can optionally be read from a ZeroFS configuration file (`/etc/net.cfg`) via standard `SYS_FILE_OPEN` / `SYS_FILE_READ`.
- The storage subsystem and ZeroFS contracts remain completely frozen and unaffected.

---

## 22. Machine Verification Suite (26 Tests: 3M-A through 3M-Z)

To verify the architecture deterministically, the implementation must pass all 26 machine tests:

```text
+------+------------------------------------+---------------------------------------------------------+
| Test | Name                               | Focus / Invariant Exercised                             |
+------+------------------------------------+---------------------------------------------------------+
| 3M-A | Network Identity & Monotonicity    | Generation-safe IDs, no ABA reuse (I-NET-ID-1)           |
| 3M-B | Interface Registration & lo0 Setup | Loopback interface setup, MAC, MTU (1500), IP 127.0.0.1 |
| 3M-C | Interface Lifecycle (Up/Down/Fault)| State machine transitions (Down -> Up -> Faulted)       |
| 3M-D | Packet Buffer Pool Allocation      | Bounded pool allocation, recycling, and state machine   |
| 3M-E | RX Queue Enqueue & Dequeue         | Intrusive FIFO queue order and boundary checks          |
| 3M-F | TX Queue Backpressure & Drops      | Queue full deterministic drop semantics (I-NET-QUEUE-1) |
| 3M-G | Stage 3L DMA Integration           | Physical frame pinning & DMA descriptor binding         |
| 3M-H | Top-Half IRQ & Event Signaling     | Interrupt dispatch to network binding and event wake    |
| 3M-I | Ethernet Frame Validation          | MAC match, EtherType parsing, length checks             |
| 3M-J | IPv4 Address Configuration         | Subnet mask, IP assignment, duplicate rejection         |
| 3M-K | ARP Neighbor Resolution & Cache    | Resolution, bounded LRU eviction, lookup                |
| 3M-L | Routing Table & Longest Prefix     | Longest-prefix match, default gateway fallback          |
| 3M-M | Socket Creation & Object Table     | KernelObjectType::Socket=7 integration, slot pinning   |
| 3M-N | Capability Rights Enforcement      | Strict subset rule (child & !parent == 0) (I-NET-CAP-1) |
| 3M-O | Port Binding & Conflict Handling   | Ephemeral allocation, port reuse conflict (-EADDRINUSE) |
| 3M-P | UDP Datagram Flow (Loopback)       | Datagram send/recv bit-identical round-trip             |
| 3M-Q | TCP State Machine Walk             | Closed -> Listen -> SynRcvd -> Established -> Closed    |
| 3M-R | Blocking Recv & Wakeup             | Thread parking and wakeup upon packet arrival           |
| 3M-S | Non-Blocking Operations (-EAGAIN)  | Immediate return of SyscallError::WouldBlock            |
| 3M-T | Device Failure & Blocked Wakeup    | Device fault immediately wakes blocked threads (I-NET-1)|
| 3M-U | Process Exit Resource Cleanup      | Terminating PID sockets/buffers freed (I-NET-LIFETIME-1)|
| 3M-V | Capability Revocation Cascade      | Invalidation of derived handles prevents further I/O    |
| 3M-W | Resource Exhaustion Limits         | Hard rejection when socket/buffer/route tables are full |
| 3M-X | Raw Socket Privilege Boundary      | NET_RAW required for raw Ethernet access                |
| 3M-Y | Concurrency & Monotonic Lock Order | Simultaneous acquisition in order Level 5A..8           |
| 3M-Z | PMM Frame Neutrality & ZeroFS Test | baseline_free == post_test_free; zero storage regression|
+------+------------------------------------+---------------------------------------------------------+
```

---

## 23. Acceptance Criteria

1. **QEMU Bare-Metal Verification**:
   All 26 tests (`3M-A` through `3M-Z`) must execute sequentially in QEMU and print:
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
   `Process` (128 B), `KernelThread` (176 B), `PerCpu` (48 B), `HandleTable` (520 B), `CapabilityNode` (24 B), and `KernelObjectSlot` (40 B) remain untouched.
6. **Kernel Footprint**:
   `__kernel_end <= 0xFFFFFFFF80200000` (fits completely within the 2 MiB bootstrap window).

---

## 24. Unresolved Questions & Design Alignment
- **Physical NIC Driver Scope**: Stage 3M implements the loopback driver, full network device integration, packet buffer model, link/routing/socket layer, and generic DMA integration. Physical hardware drivers (such as Intel e1000 or virtio-net) will run as Ring-3 user-space drivers using Stage 3L device capability primitives in a subsequent increment.

---

## ARCHITECTURE STATUS:
🟡 **PROPOSED — AWAITING REVIEW**
