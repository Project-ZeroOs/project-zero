# ADR-0022: Networking Model Architecture (Stage 3M Rev3)

## Context
Project Zero / ZeroOS requires a foundational, capability-governed, bounded, and deterministic **Networking Model**. Stages 3A through 3L established threads, scheduling, preemption, synchronization, thread lifecycle, processes, IPC, capabilities, syscalls, ELF execution, persistent storage (ZeroFS), and the device/hardware abstraction layer.

Stage 3M Architecture Rev2 was reviewed and refined to resolve the remaining architectural ambiguities:
1. Explicit mathematical mapping between 32 packet buffer slices, 16 physical DMA frames, and Stage 3L pin tracking (`I-NET-DMA-2`).
2. Concrete `NetworkDeviceBinding` structure, generation safety, and explicit loopback virtual pseudo-device exception (`I-NET-DEV-1`).
3. Multi-tier device failure/reset/link/fault propagation distinguishing transient resets from permanent detach.
4. Exact Stage 3D `WaitQueue` predicates and atomic lost-wakeup avoidance for `recv`, `send`, `accept`, and `connect`.
5. Minimal TCP semantic subset (ISN, sequence tracking, window bounds, Stop-and-Wait / 1 MSS in-flight, RTO, RST handling).
6. Deterministic 4-state ARP engine with tick expiration, bounded retries, and explicit LRU eviction.
7. Exact wildcard port-binding matrix and ephemeral port allocation.
8. Decomposed `NET_RAW` authority scope prohibiting MAC spoofing and promiscuous mode.
9. Exhaustive `.bss` accounting table (5,152 bytes) proving compliance with the 2 MiB bootstrap window.
10. Strengthened acceptance criteria for machine tests `3M-E`, `3M-F`, `3M-J`, `3M-M`, `3M-T`, `3M-U`, `3M-W`, `3M-X`.

---

## Architectural Decisions

### ADR-3M-001: Packet Buffer ↔ DMA Frame Slicing & Pin Tracking
- Fixed capacity: 32 packet buffer descriptors (`PacketBufferSlot`, 48 bytes each).
- Physical backing: 16 contiguous 4096-byte physical frames allocated as a single logical DMA buffer through Stage 3L's `alloc_dma_buffer` (`dma_buf_id`).
- Mathematical slicing:
  - Frame $k \in [0, 15]$ has base physical address $P_k = P_{\text{base}} + k \times 4096$.
  - Slice 0: offset $0 \dots 2047 \implies$ Packet buffer index $2k$ ($P_{2k} = P_k$).
  - Slice 1: offset $2048 \dots 4095 \implies$ Packet buffer index $2k + 1$ ($P_{2k+1} = P_k + 2048$).
- Invariant `I-NET-DMA-2`: A physical DMA frame remains pinned in Stage 3L's `PHYSICAL_FRAME_PIN_TABLE` while **either** of its packet buffer slices ($2k$ or $2k+1$) is owned by an active driver, interface, or socket operation.
- Invariant `I-NET-BUF-1`: Every packet slice belongs to exactly one ownership domain at every instant:
  $$\text{FreePool} \leftrightarrow \text{DriverRxRing} \to \text{InterfaceRxQueue} \to \text{SocketIngressQueue} \to \text{UserCopy} \to \text{FreePool}$$
  $$\text{FreePool} \leftrightarrow \text{SocketTxQueue} \to \text{InterfaceTxQueue} \to \text{DriverTxRing} \to \text{NIC DMA} \to \text{FreePool}$$
- TX completion releases the individual slice back to `FreePool` without releasing the physical frame. Physical DMA frame reclamation occurs solely upon interface unbinding/teardown.

### ADR-3M-002: Concrete NetworkDeviceBinding & Loopback Virtual Exception
- Concrete binding structure:
  ```rust
  pub struct NetworkDeviceBinding {
      pub occupied: bool,
      pub binding_id: u16,
      pub device_slot: u8,
      pub device_generation: u16,
      pub interface_slot: u8,
      pub interface_generation: u16,
      pub is_pseudo_device: bool, // true for loopback
      pub state: DeviceBindingState,
  }
  ```
- Subsystem ownership: `NETWORK_DEVICE_BINDINGS: [NetworkDeviceBinding; 4]` is owned by Stage 3M.
- Generation validation: Interface operations verify `DEVICE_TABLE[device_slot].generation == binding.device_generation`. Recycled `DeviceSlot` indices cannot resurrect stale network interfaces.
- Invariant `I-NET-DEV-1`: Every `NetworkInterface` is backed by either:
  1. Exactly one live Stage 3L `DeviceSlot` binding (`is_pseudo_device = false`) where `device.generation == binding.device_generation`, OR
  2. The designated internal loopback virtual pseudo-device (`is_pseudo_device = true`, `device_slot = 0xFF`, `interface_id = 1`).

### ADR-3M-003: Multi-Tier Failure & Recovery Propagation
Stage 3M distinguishes four discrete hardware/link lifecycle events:
1. **Link Carrier Down**: Physical cable disconnected. Interface marks `link_up = false`. Sockets remain in `Established`. Outgoing packets return `SyscallError::NetworkDown` (-19).
2. **Transient Device Reset**: Hardware reset underway (`DeviceLifecycleState::Resetting`). Interface marks `state = InterfaceState::Resetting`. Queues are temporarily paused. If reset succeeds within 50 ticks (500 ms), the interface returns to `Operational`. Sockets are not destroyed.
3. **Unrecoverable Device Fault**: Hardware controller unrecoverable (`DeviceLifecycleState::Faulted`). Interface marks `state = InterfaceState::Faulted`. RX/TX queues are flushed (packet slices returned to `FreePool`). All blocked socket operations wake immediately with `SyscallError::NetworkDown` (-19). Active TCP connections transition to `Closed` with `socket.error = -ECONNRESET`.
4. **Device Detached / Released**: Device unregistered from Stage 3L. Binding is invalidated, interface is destroyed, associated routing table entries are purged, and sockets are closed (`I-NET-DEV-FAIL-1`).

### ADR-3M-004: Exact WaitQueue Predicates & Lost-Wakeup Elimination
All blocking operations execute the Stage 3D atomic discipline:
$$\text{Lock acquire} \implies \text{Predicate check} \implies \text{Waiter registration} \implies \text{Lock release} \implies \text{Park current thread}$$
- **`SYS_NET_RECV`**:
  - Predicate: `rx_queue_count > 0 || state == SocketState::PeerClosed || state == SocketState::Closed || error != 0`
  - WaitQueue: `socket.rx_waitqueue`
  - Terminal: Returns 0 on EOF (`PeerClosed`), returns data if queue non-empty, returns `error` if faulted.
- **`SYS_NET_SEND`**:
  - Predicate: `tx_queue_count < tx_limit || state == SocketState::Closed || error != 0`
  - WaitQueue: `socket.tx_waitqueue`
  - Terminal: Returns bytes sent, or returns `error` (`-ECONNRESET`, `-ENETDOWN`).
- **`SYS_NET_ACCEPT`**:
  - Predicate: `accept_queue_count > 0 || state != SocketState::Listen || error != 0`
  - WaitQueue: `socket.accept_waitqueue`
  - Terminal: Returns child socket handle, or `-EINVAL` if not listening.
- **`SYS_NET_CONNECT`**:
  - Predicate: `state == SocketState::Established || state == SocketState::Closed || error != 0`
  - WaitQueue: `socket.connect_waitqueue`
  - Terminal: Returns 0 on success, `-ECONNREFUSED` on RST, `-ETIMEDOUT` on timeout (200 ticks / 2s), `-ENETUNREACH` on routing failure.
- Invariant `I-NET-WAIT-1`: Every blocking operation has an explicit predicate and registered wakeup source.

### ADR-3M-005: Minimal RFC 793 TCP Semantic Contract
Stage 3M implements an authoritative minimal subset of RFC 793:
- Initial Sequence Number (ISN): Monotonically generated: $\text{tick} \times 64,000 + \text{socket\_id}$.
- Sequence Variables:
  - Send: `SND.UNA` (oldest unacknowledged), `SND.NXT` (next sequence number), `SND.WND = 2048`.
  - Receive: `RCV.NXT` (next expected sequence number), `RCV.WND = 2048`.
- In-Flight Bound: Maximum outstanding segments = 1 MSS (Stop-and-Wait, MSS = 1460 bytes).
- SYN/FIN: SYN and FIN consume exactly 1 sequence number.
- Retransmission Timeout (RTO): 200 ms (20 ticks) initial timeout; exponential backoff up to 3 retries (max 1.6s).
- RST Semantics: Incoming RST matching `RCV.NXT` aborts connection; state transitions immediately to `Closed` with `socket.error = -ECONNRESET` (or `-ECONNREFUSED` if in `SynSent`).
- TIME_WAIT: 100 ms (10 ticks) in testing, 60s in production; auto-transitions to `Closed`.

### ADR-3M-006: Deterministic 4-State ARP Engine
- Table: `NEIGHBOR_TABLE: [NeighborEntry; 16]`.
- States: `Empty (0) -> Incomplete (1) -> Reachable (2) -> Stale (3)`.
- Retry & Pending:
  - `Incomplete`: Holds 1 pending outbound packet buffer. Transmits ARP request. Timeout: 100 ticks (1s), up to 3 retries.
  - On ARP reply: State becomes `Reachable`, `timeout_ticks = 30,000` (300s), pending packet transmitted.
  - On failure (3 timeouts): Pending packet dropped, state becomes `Empty`, caller receives `-ENETUNREACH`.
- Monotonic LRU Eviction:
  - When table is full, `Incomplete` entries are never evicted.
  - Oldest `Stale` entry (lowest `last_used_ticks`) is evicted first.
  - If no `Stale` entries exist, oldest `Reachable` (lowest `last_used_ticks`) is evicted (`I-NET-ARP-1`).

### ADR-3M-007: Authoritative Wildcard Port-Binding Matrix
- Table: `PORT_BINDING_TABLE: [PortBindingSlot; 32]`.
- Coexistence Matrix:
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
- Ephemeral Ports: `49152..65535` allocated monotonically, skipping collisions.
- Privileged Ports: Ports `1..1023` require `NET_CONFIG` right.
- Invariant `I-NET-BIND-1`: A successful bind creates exactly one authoritative binding.

### ADR-3M-008: Decomposed `NET_RAW` Authority Scope
- `NET_RAW` (Bit 14):
  - Permits opening `SocketType::Raw` to transmit and receive raw Ethernet II frames.
  - Outgoing frames **must** use the interface's assigned hardware MAC; forging source MAC is rejected with `SyscallError::PermissionDenied` (-4) unless `NET_CONFIG` is also held.
  - Promiscuous packet reception is strictly prohibited: socket only receives unicast frames destined for the interface MAC and broadcast (`FF:FF:FF:FF:FF:FF`).
  - Cannot be derived without parent holding `NET_RAW` (`I-NET-CAP-SUBSET-1`).

### ADR-3M-009: Complete `.bss` Memory Budget (5,152 Bytes)
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
- Fits safely within the bootstrap window; verified against `__kernel_end <= 0xFFFFFFFF80200000`.

---

## Status
🟡 **PROPOSED (Rev3) — AWAITING REVIEW**
