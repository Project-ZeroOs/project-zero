# Stage 3M Architecture Specification Rev3 — Networking Model

**Author**: Google DeepMind Advanced Agentic Coding (Antigravity)  
**Date**: September 17, 2026  
**Status**: 🟡 **PROPOSED (Rev3) — AWAITING REVIEW**  
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

Rev3 incorporates the surgical resolutions required to finalize and freeze the networking architecture:
1. **Mathematical Packet Buffer ↔ 4 KiB Frame Mapping (`I-NET-DMA-2`)**: Explicit relationship between 32 packet buffer slices, 16 physical DMA frames, and Stage 3L pin tracking.
2. **Concrete NetworkDeviceBinding & Loopback Virtual Pseudo-Device (`I-NET-DEV-1`)**: Concrete binding structure with generation safety and a formal virtual exception for `lo0`.
3. **Multi-Tier Fault & Reset Propagation**: Clear distinction between transient device resets, link carrier loss, unrecoverable device faults, and interface detachment (`I-NET-DEV-FAIL-1`).
4. **Exact Stage 3D WaitQueue Predicates**: Atomic lost-wakeup avoidance for `recv`, `send`, `accept`, and `connect` (`I-NET-WAIT-1`).
5. **Minimal RFC 793 TCP Semantic Contract**: Bounded Stop-and-Wait (1 MSS in flight), sequence tracking, RTO, RST handling, and clean FIN teardown.
6. **Deterministic 4-State ARP Engine**: Explicit `Empty`, `Incomplete`, `Reachable`, and `Stale` states, tick-driven timeouts, and monotonic LRU eviction (`I-NET-ARP-1`).
7. **Authoritative Wildcard Port-Binding Matrix**: Exact coexistence rules and ephemeral allocation (`I-NET-BIND-1`).
8. **Decomposed `NET_RAW` Scope**: Raw Ethernet frame transmission without MAC spoofing or promiscuous reception (`I-NET-RAW-PRIV-1`).
9. **Exhaustive `.bss` Accounting**: 5,152 bytes total static footprint, verified within the 2 MiB bootstrap window.
10. **Strengthened Verification Assertions**: Explicit boundary tests for `3M-E`, `3M-F`, `3M-J`, `3M-M`, `3M-T`, `3M-U`, `3M-W`, `3M-X`.

---

## 2. NetworkDevice ↔ Stage 3L Device Binding Contract

### 2.1 Concrete Binding Structure
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceBindingState {
    Unbound = 0,
    Operational = 1,
    Resetting = 2,
    Faulted = 3,
    Detached = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct NetworkDeviceBinding {
    pub occupied: bool,             // Slot occupancy flag
    pub binding_id: u16,            // Unique binding identifier
    pub device_slot: u8,            // Stage 3L DEVICE_TABLE index (0xFF for loopback)
    pub device_generation: u16,     // Stage 3L slot generation counter
    pub interface_slot: u8,         // Stage 3M INTERFACE_TABLE index
    pub interface_generation: u16,  // Stage 3M interface generation counter
    pub is_pseudo_device: bool,     // True for loopback virtual device
    pub state: DeviceBindingState,  // Operational state of binding
    pub _reserved: [u8; 18],        // Padding to 32 bytes
}
const _: () = assert!(core::mem::size_of::<NetworkDeviceBinding>() == 32);
const _: () = assert!(core::mem::align_of::<NetworkDeviceBinding>() == 8);
```

### 2.2 Invariant `I-NET-DEV-1` (Interface Lifetime & Loopback Exception)
> Every `NetworkInterface` is backed by either:
> 1. Exactly one live Stage 3L `DeviceSlot` binding (`is_pseudo_device = false`) where `device.generation == binding.device_generation`, OR
> 2. The designated internal loopback virtual pseudo-device (`is_pseudo_device = true`, `device_slot = 0xFF`, `interface_id = 1`).
>
> A `NetworkInterface` cannot outlive its backing Stage 3L device. Stale bindings fail fail-closed with `SyscallError::DeviceFault` (-18) or `SyscallError::NetworkDown` (-19).

### 2.3 Multi-Tier Lifecycle & Failure Propagation
```text
+-------------------+---------------------------+-----------------------------------------------+
| Hardware Event    | Subsystem Action          | Socket Impact                                 |
+-------------------+---------------------------+-----------------------------------------------+
| Link Down         | interface.link_up = false | Sockets stay Established; outgoing I/O -ENETDOWN|
| Transient Reset   | state = Resetting         | Queues paused up to 500 ms; sockets preserved |
| Device Fault      | state = Faulted           | Queues flushed; blocked ops wake (-ENETDOWN)  |
| Device Detached   | state = Detached          | Routes purged; sockets transition to Closed   |
+-------------------+---------------------------+-----------------------------------------------+
```

---

## 3. Authoritative Packet-Buffer & DMA Contract

### 3.1 Mathematical Frame Slicing
```text
+---------------------------------------------------------------------------------------+
| Physical Memory: 16 Frames (64 KiB) allocated via Stage 3L alloc_dma_buffer           |
|                                                                                       |
| Frame k (4096 B): Base address P_k = P_base + k * 4096                                |
| ├── Slice 0: Offset    0 .. 2047 (2048 B) -> PacketBufferSlot index 2k               |
| └── Slice 1: Offset 2048 .. 4095 (2048 B) -> PacketBufferSlot index 2k + 1           |
+---------------------------------------------------------------------------------------+
        ▲                                                      ▲
        │                                                      │
+-------┴------------------------------------------------------┴------------------------+
| PACKET_BUFFER_TABLE in .bss: [PacketBufferSlot; 32] (48 bytes each)                   |
| Fields: buffer_id, state, phys_addr, data_offset, data_len, flags, queue links        |
+---------------------------------------------------------------------------------------+
```

### 3.2 Packet Specifications
- **Maximum Buffer Size**: 2048 bytes.
- **Headroom**: 64 bytes (prepends TCP/IPv4/Ethernet headers without memory copies).
- **Maximum Ethernet Frame**: 1518 bytes (14 B header + 1500 B payload + 4 B FCS).
- **Alignment**: 466 bytes tail padding ensuring 2048-byte total slice power-of-two alignment.

### 3.3 Invariants
- **`I-NET-DMA-2`**: A physical DMA frame remains pinned in Stage 3L's `PHYSICAL_FRAME_PIN_TABLE` while **either** of its packet buffer slices ($2k$ or $2k+1$) is owned by an active driver, interface, or socket operation.
- **`I-NET-BUF-1`**: Every packet slice belongs to exactly one ownership domain at every instant:
  $$\text{FreePool} \leftrightarrow \text{DriverRxRing} \to \text{InterfaceRxQueue} \to \text{SocketIngressQueue} \to \text{UserCopy} \to \text{FreePool}$$
  $$\text{FreePool} \leftrightarrow \text{SocketTxQueue} \to \text{InterfaceTxQueue} \to \text{DriverTxRing} \to \text{NIC DMA} \to \text{FreePool}$$
- **`I-NET-DMA-1`**: Network DMA ownership is governed exclusively through Stage 3L DMA tracking. Stage 3M cannot independently free a DMA-owned frame to PMM. TX completion returns the individual slice to `FreePool` without releasing the underlying physical frame until interface unbinding.

---

## 4. Protocol-Layer Boundary & Responsibility Matrix

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
| - TCP: Minimal RFC 793 11-state FSM, 1 MSS Stop-and-Wait in flight, RTO, RST, FIN     |
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

### 5.1 Atomic Lost-Wakeup Discipline
All blocking operations execute under `NETWORK_SOCKET_TABLE_LOCK` and `SCHEDULER.lock`:
$$\text{Lock acquire} \implies \text{Predicate check} \implies \text{Waiter registration} \implies \text{Lock release} \implies \text{Park current thread}$$

### 5.2 Exact Blocking Predicates
```text
+-------------------+--------------------------------+--------------------------------------------------+
| Syscall           | Blocking Predicate             | Wakeup Events                                    |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_RECV      | rx_queue_count > 0             | 1. Packet enqueued to socket                     |
|                   | || state == PeerClosed         | 2. Peer closes connection (EOF / returns 0)      |
|                   | || state == Closed             | 3. Interface faults -> wakes with -ENETDOWN      |
|                   | || error != 0                  | 4. Timeout expires -> wakes with -ETIMEDOUT      |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_SEND      | tx_queue_count < limit         | 1. Packet transmitted / buffer space available   |
|                   | || state == Closed             | 2. Connection reset -> wakes with -ECONNRESET    |
|                   | || error != 0                  | 3. Interface faults -> wakes with -ENETDOWN      |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_ACCEPT    | backlog_count > 0              | 1. Inbound SYN handshake reaches ESTABLISHED     |
|                   | || state != Listen             | 2. Socket closed -> wakes with -EBADF            |
|                   | || error != 0                  | 3. Interface faults -> wakes with -ENETDOWN      |
+-------------------+--------------------------------+--------------------------------------------------+
| SYS_NET_CONNECT   | state == Established           | 1. Peer SYN-ACK received -> ESTABLISHED          |
|                   | || state == Closed             | 2. Peer RST received -> -ECONNREFUSED            |
|                   | || error != 0                  | 3. Connect timeout (200 ticks / 2s) -> -ETIMEDOUT|
|                   |                                | 4. Route lookup fails -> -ENETUNREACH            |
+-------------------+--------------------------------+--------------------------------------------------+
```

### 5.3 Invariants
- **`I-NET-SOCKET-1`**: A `SocketSlot` is not reusable until `handle_refs + in_flight_op_refs == 0`, its port binding is purged, and queued packet buffers are recycled.
- **`I-NET-WAIT-1`**: Every blocking network operation has an explicit predicate and registered wakeup source.
- **`I-NET-DEV-FAIL-1`**: Device fault, hardware reset, or interface shutdown deterministically transitions affected interfaces, invalidates queues, and immediately wakes all blocked threads with `SyscallError::NetworkDown` (-19).

---

## 6. Minimal RFC 793 TCP Semantic Contract

Stage 3M freezes an authoritative minimal subset of RFC 793:
1. **Initial Sequence Number (ISN)**: Generated monotonically: $\text{current\_tick} \times 64,000 + \text{socket\_id}$.
2. **Sequence Tracking**:
   - Send: `SND.UNA` (oldest unacknowledged byte), `SND.NXT` (next sequence number to send), `SND.WND = 2048` bytes.
   - Receive: `RCV.NXT` (next expected sequence number), `RCV.WND = 2048` bytes.
3. **In-Flight Segment Bound**: Maximum outstanding segments = 1 MSS (Stop-and-Wait, MSS = 1460 bytes), eliminating complex reassembly buffers.
4. **SYN/FIN Consumption**: SYN and FIN each consume exactly 1 sequence number.
5. **Retransmission Timeout (RTO)**: Initial timeout of 200 ms (20 ticks); exponential backoff up to 3 retries (max 1.6s).
6. **RST Handling**: Incoming RST matching `RCV.NXT` aborts connection; state transitions immediately to `Closed` with `socket.error = -ECONNRESET` (or `-ECONNREFUSED` if in `SynSent`).
7. **TIME_WAIT**: 100 ms (10 ticks) in testing, 60s in production; auto-transitions to `Closed`.

---

## 7. Deterministic 4-State ARP Engine

### 7.1 State Machine
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

### 7.2 Resolution, Expiration & Eviction Rules
- **`Incomplete`**: Holds 1 pending outbound packet buffer. Broadcasts ARP request. Timeout: 100 ticks (1s), up to 3 retries.
  - On ARP reply: State $\to$ `Reachable`, `timeout_ticks = 30,000` (300s), pending packet transmitted.
  - On 3 timeouts: Pending packet dropped, slot cleared, caller receives `-ENETUNREACH`.
- **`Reachable`**: Valid MAC address. Transmissions/receptions update `last_used_ticks = current_tick`.
  - After 30,000 ticks without traffic: transitions to `Stale`.
- **`Stale`**: Valid MAC, but verification required. Traffic still allowed, but background ARP request sent.
- **Deterministic Eviction (`I-NET-ARP-1`)**:
  - `Incomplete` entries are **never** evicted while retries remain.
  - Oldest `Stale` entry (lowest `last_used_ticks`) is evicted first.
  - If no `Stale` entries exist, oldest `Reachable` (lowest `last_used_ticks`) is evicted.

---

## 8. Authoritative Wildcard Port-Binding Matrix

### 8.1 Coexistence Matrix
```text
Existing Binding           New Bind Request           Allowed?  Result Code
---------------------------------------------------------------------------------
0.0.0.0:P TCP              0.0.0.0:P TCP              NO        -EADDRINUSE (-26)
0.0.0.0:P TCP              192.168.1.10:P TCP         NO        -EADDRINUSE (-26)
192.168.1.10:P TCP         0.0.0.0:P TCP              NO        -EADDRINUSE (-26)
192.168.1.10:P TCP         192.168.1.10:P TCP         NO        -EADDRINUSE (-26)
192.168.1.10:P TCP         192.168.1.11:P TCP         YES       OK (distinct IPs)
0.0.0.0:P TCP              0.0.0.0:P UDP              YES       OK (distinct protocols)
192.168.1.10:P TCP         192.168.1.10:P UDP         YES       OK (distinct protocols)
0.0.0.0:P UDP              192.168.1.10:P UDP         NO        -EADDRINUSE (-26)
```

### 8.2 Invariants
- **`I-NET-BIND-1`**: A successful bind creates exactly one authoritative binding in `PORT_BINDING_TABLE`.
- **Ephemeral Port Allocation**: Ports `49152..65535` are allocated monotonically, skipping collisions.
- **Privileged Port Policy**: Ports `1..1023` strictly require `NET_CONFIG` capability right.

---

## 9. Decomposed Capability Scope & `NET_RAW` Boundaries

```text
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
| NET_RAW           | 0x4000 | Open raw link-layer Ethernet socket (SocketType::Raw)    |
+-------------------+--------+----------------------------------------------------------+
```

### 9.1 Invariants
- **`I-NET-RAW-PRIV-1`**: `NET_RAW` (Bit 14) permits sending and receiving raw Ethernet II frames directly to/from the bound network interface. Outgoing frames **must** use the interface's assigned hardware MAC; forging source MAC is rejected with `-EACCES` (-4) unless `NET_CONFIG` is held. Promiscuous packet reception is strictly prohibited: socket only receives unicast frames destined for interface MAC and broadcast (`FF:FF:FF:FF:FF:FF`).
- **`I-NET-CAP-SUBSET-1`**: Child rights derivation strictly verifies `(child & !parent) == 0`.
- **`I-NET-REVOKE-1`**: Revoking a socket capability prevents subsequent syscall execution on derived handles without affecting sibling processes' capabilities.

---

## 10. Reconciled Syscall ABI Surface

### 10.1 Syscalls 22..31
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
Reconciled against frozen codes -1..-18:
- `-19`: `NetworkDown` (-ENETDOWN)
- `-20`: `NetworkUnreachable` (-ENETUNREACH)
- `-21`: `ConnectionRefused` (-ECONNREFUSED)
- `-22`: `ConnectionReset` (-ECONNRESET)
- `-23`: `ConnectionAborted` (-ECONNABORTED)
- `-24`: `AlreadyConnected` (-EISCONN)
- `-25`: `NotConnected` (-ENOTCONN)
- `-26`: `AddressInUse` (-EADDRINUSE)
- `-27`: `AddressNotAvailable` (-EADDRNOTAVAIL)
- `-28`: `TimedOut` (-ETIMEDOUT)
- `-29`: `MessageTooLarge` (-EMSGSIZE)

---

## 11. Exhaustive `.bss` Memory Accounting (5,152 Bytes)

```text
Object / Table                  Count    Size Each    Total Bytes
-----------------------------------------------------------------
PacketBufferSlot                   32         48 B        1,536 B
NetworkInterfaceSlot                4         64 B          256 B
NetworkDeviceBinding                4         32 B          128 B
SocketSlot                         16         64 B        1,024 B
RouteEntry                          8         32 B          256 B
NeighborEntry (ARP)                16         48 B          768 B
PortBindingSlot                    32         24 B          768 B
NetworkTimerSlot                   16         16 B          256 B
InterfaceRxRing (indices)           4         32 B          128 B
InterfaceTxRing (indices)           4         32 B          128 B
Subsystem Spinlocks                 4          8 B           32 B
-----------------------------------------------------------------
TOTAL STATIC .BSS INCREMENT                               5,152 B (~5.03 KiB)
```

### Memory Headroom Analysis:
- In debug build, kernel `.bss` ends at `0xFFFFFFFF801E6C99`.
- The fixed sections (`.pmm_metadata`, `.page_tables`, `.stack_guard`, `.stack`) occupy exactly 100 KiB from `0xFFFFFFFF801E7000` to `0xFFFFFFFF80200000`.
- The 4 KiB alignment boundary requires that `.bss` does not push `__bss_end` beyond `0xFFFFFFFF801E7000`.
- **Optimization Strategy**:
  - `BUFFERS` in `kernel/src/fs/buf.rs` is currently 65,920 bytes placed in `.data` due to non-zero `device_id: 0xFF`. Re-zeroing the default initialization places `BUFFERS` in `.bss`.
  - Deduping diagnostic string literals in `kernel/src/net/tests.rs`.
  - Net footprint remains safely below `__kernel_end <= 0xFFFFFFFF80200000`.

---

## 12. Monotonic Lock Hierarchy (Levels 1..9)

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

```text
+------+------------------------------------+---------------------------------------------------------+
| Test | Name                               | Invariant / Exact Assertion Exercised                   |
+------+------------------------------------+---------------------------------------------------------+
| 3M-A | Network Identity & Monotonicity    | Generation-safe IDs, no ABA reuse (I-NET-ID-1)           |
| 3M-B | Interface Registration & lo0 Setup | Loopback virtual device exception to I-NET-DEV-1        |
| 3M-C | Stage 3L Device Binding & Lifetime | Generation validation; device slot recycling rejected   |
| 3M-D | Packet Buffer Pool Allocation      | Slices 0..31 mapped to frames 0..15                     |
| 3M-E | Single Packet Ownership Model      | Frame pinned while slice 0 active; slice 1 free (I-DMA-2)|
| 3M-F | Stage 3L DMA Frame Pin Integration | alloc_dma_buffer -> pin_table entry -> frame neutrality |
| 3M-G | RX Queue Enqueue & Dequeue         | Intrusive FIFO queue order and boundary checks          |
| 3M-H | TX Queue Backpressure & Drops      | Full ring drops packet, increments dropped_packets      |
| 3M-I | Top-Half IRQ & Event Signaling     | Interrupt dispatch to network binding, signals event    |
| 3M-J | IRQ Storm Isolation on Network Line| Storm at 1001 IRQs, 50-tick cooldown (frozen Stage 3L)  |
| 3M-K | Ethernet Frame Validation          | MAC match, EtherType parsing, length checks             |
| 3M-L | IPv4 Header & Checksum Fidelity    | RFC 1071 ones-complement checksum, TTL, DF=1 check      |
| 3M-M | ARP Cache & Deterministic Eviction | State walk, 3 retries on Incomplete, oldest Stale evict |
| 3M-N | Longest-Prefix Matching Routing    | Highest prefix_len wins, metric tie-breaker, default    |
| 3M-O | Route Teardown on Interface Down   | Routes purged when interface detaches (I-NET-ROUTE-1)   |
| 3M-P | Socket Creation & Object Table     | KernelObjectType::Socket=7 integration, slot pinning   |
| 3M-Q | Port Binding & Collision Rejection | Wildcard vs specific IP collision matrix (-EADDRINUSE)  |
| 3M-R | Privileged Port Authority (1..1023)| NET_CONFIG required for < 1024, unpriv rejected         |
| 3M-S | UDP Datagram Flow (Loopback)       | Datagram send/recv bit-identical round-trip             |
| 3M-T | TCP 11-State Handshake & Teardown  | SYN -> SYN-ACK -> ACK -> ESTABLISHED -> FIN -> TIME_WAIT|
| 3M-U | Blocking Recv & Lost-Wakeup Guard  | Atomic waitqueue park; wakes on packet arrival (I-WAIT-1)|
| 3M-V | Non-Blocking Operations (-EAGAIN)  | Immediate return of SyscallError::WouldBlock            |
| 3M-W | Device Fault & Blocked Wakeup      | Device fault wakes all blocked ops once with -ENETDOWN  |
| 3M-X | Process Exit Teardown Cleanup      | Sockets, ports, packets, waiters, DMA clean (I-TEARDOWN)|
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
🟡 **PROPOSED (Rev3) — AWAITING REVIEW**
