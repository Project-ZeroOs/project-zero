# Project Zero: Peer-to-Peer Networking & Transport Subsystem

## 1. Network Philosophy: Zero Configuration, End-to-End Cryptography

Traditional networking stacks in general-purpose operating systems were conceived around static IP addresses, manual port forwarding, and centralized DNS servers. 

In Project Zero, devices belong to an ad-hoc, mobile mesh:
* A smartphone continuously transitions between 5G cellular, home Wi-Fi, coffee shop networks, and direct Wi-Fi Direct/Bluetooth links.
* Devices must discover each other, traverse arbitrary carrier-grade NATs (CGNAT), establish direct encrypted tunnels, and maintain active connections transparently without user intervention.
* **Invariant**: Network security is absolute. Unencrypted packets never leave the user's protection domain.

```text
+-------------------------------------------------------------------------+
|                        PROJECT ZERO NETWORK STACK                       |
|                                                                         |
|  +-------------------------------------------------------------------+  |
|  |           DISTRIBUTED WORKSPACE SYNC (CRDT State Layer)           |  |
|  +-------------------------------------------------------------------+  |
|  +-------------------------------------------------------------------+  |
|  |            MULTIPLEXED FABRIC TRANSPORT (QUIC-Inspired)           |  |
|  |   * Bi-directional Streams    * Zero-RTT Resumption               |  |
|  |   * Congestion Control (BBR)  * Flow Control Per Stream           |  |
|  +-------------------------------------------------------------------+  |
|  +-------------------------------------------------------------------+  |
|  |                ENCRYPTED TUNNEL LAYER (Noise IK Protocol)         |  |
|  |   * ChaCha20-Poly1305 AEAD    * Curve25519 ECDH Keys              |  |
|  |   * BLAKE2s Hashing           * Perfect Forward Secrecy           |  |
|  +-------------------------------------------------------------------+  |
|  +-------------------------------------------------------------------+  |
|  |                    RENDEZVOUS & NAT TRAVERSAL                     |  |
|  |   * Local BLE/mDNS Beaconing  * STUN/ICE UDP Hole Punching        |  |
|  |   * UPnP / PCP Mapping        * Encrypted Relay Fallback (DERP)   |  |
|  +-------------------------------------------------------------------+  |
+-------------------------------------------------------------------------+
```

---

## 2. Peer Discovery & Rendezvous

Devices locate each other through a tiered discovery mechanism that prioritizes local proximity and minimal power consumption:

### 2.1 Proximity Discovery (Local Neighborhood)
1. **Bluetooth Low Energy (BLE) Advertisements**:
   * Devices emit rotating, encrypted ephemeral beacons containing a hash of the User's Fabric Identity:
     $$\text{Beacon} = \text{HMAC}_{K_{\text{fabric}}}(\text{DeviceID} \parallel \text{Timestamp}_{15\text{s}})$$
   * Observers cannot track the device across physical locations due to rotating MAC and beacon entropy.
2. **Local Multicast DNS (mDNS) & IPv6 Multicast**:
   * When connected to the same Wi-Fi or local switch, devices exchange cryptographically signed discovery messages over UDP port `7890`.

### 2.2 Global Rendezvous & NAT Traversal (Wide Area Network)
When devices reside on different networks (e.g., phone on 5G, desktop at home behind a double NAT):
1. **Interactive Connectivity Establishment (ICE / STUN)**:
   * Each device contacts a lightweight STUN server to discover its public reflexive IP and UDP port bindings.
2. **Symmetric NAT Hole Punching**:
   * Devices exchange reflexive candidates and execute synchronized UDP hole punching, opening direct peer-to-peer pinholes through router firewalls.
3. **Encrypted Relay Fallback**:
   * If symmetric carrier firewalls block direct UDP communication, traffic routes through an **Encrypted Zero-Knowledge Relay**. The relay sees only encrypted WireGuard/Noise frames; it has zero access to headers, metadata, or packet contents.

---

## 3. Cryptographic Transport Layer: Noise Protocol

All Project Zero fabric connections are established using the **Noise Protocol Framework**:

$$\textbf{Noise\_IKpsk2\_25519\_ChaChaPoly\_BLAKE2s}$$

```text
Initiator (Phone)                                 Responder (Laptop)
  |                                                     |
  |  e, es, s, ss                                       |
  | --------------------------------------------------> |
  |                                                     |
  |  e, ee, se                                          |
  | <-------------------------------------------------- |
  |                                                     |
  |  [Handshake Complete: Symmetrical Session Keys]     |
  | <=================================================> |
```

### 3.1 Properties Guaranteed
* **Mutual Authentication**: Both peers cryptographically prove ownership of their enrolled Ed25519 identity keys.
* **Immediate Forward Secrecy**: Even if a physical device is seized months later, recorded network traffic cannot be decrypted because ephemeral Diffie-Hellman keys are zeroized immediately upon session termination.
* **Pre-Shared Key Hardening (`psk2`)**: An additional layer of post-quantum resilience injected during pairing.

---

## 4. Multiplexed Fabric Protocol (QUIC-Inspired)

Once the encrypted tunnel is established, communications are multiplexed over a single UDP socket:

```text
[Fabric Connection]
  │
  ├── Stream 0 (Priority 0 - Critical): Heartbeat & Network Telemetry (RTT, loss)
  ├── Stream 1 (Priority 1 - High):     Compositor Direct Input Events
  ├── Stream 2 (Priority 2 - High):     Interactive RPC / Fast IPC Messages
  ├── Stream 3 (Priority 3 - Normal):   CRDT Workspace Delta Synchronization
  └── Stream 4 (Priority 4 - Bulk):     Large File / Texture Frame Offloading
```

### 4.1 Head-of-Line Blocking Elimination
In traditional TCP, a single lost packet halts all streams. Project Zero's transport guarantees that a packet drop on the bulk file transfer stream (Stream 4) **never delays** real-time touch input packets (Stream 1).

### 4.2 Dynamic Multipath & Connection Migration
* If Wi-Fi signal drops below a defined signal-to-noise ratio (SNR), the network stack automatically bonds cellular data and migrates active UDP session endpoints without terminating open streams or invalidating workspace state.

---

## 5. Distributed State Synchronization: Conflict-Free Replicated Data Types (CRDTs)

Workspaces and user contexts migrate between devices using **State-based and Operation-based CRDTs**:

```text
[Phone Workspace Node]                          [Laptop Workspace Node]
  │                                                     │
  ├─ User edits note / opens tab                        │
  ├─ State vector updated: V_phone = [4, 0]             │
  │                                                     │
  │              Sync Delta: Δ(State)                   │
  │ ──────────────────────────────────────────────────> │
  │                                                     ├─ Applies Merge Rule:
  │                                                     │  State = State_local ⊔ Δ
  │                                                     ├─ State vector: V_laptop = [4, 1]
  │              Ack Delta: Δ(LaptopState)              │
  │ <────────────────────────────────────────────────── │
```

### 5.1 Convergence Invariant
Regardless of packet arrival order, temporary network partitions, or concurrent multi-device editing, all nodes mathematically converge to the exact same state without centralized database locks or merge conflict dialogs.
