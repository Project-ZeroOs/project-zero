# Project Zero: Security Model & Capability Architecture

## 1. Core Principles of Project Zero Security

Security in Project Zero is an architectural foundation rather than a perimeter defense layer. Traditional operating systems suffer from fundamental security vulnerabilities rooted in their design:
1. **Ambient Authority**: In POSIX and Windows systems, a process running under a user identity inherits virtually all of that user's rights. If an image viewer is compromised via a buffer overflow, it can read the user's SSH keys, private photos, and browser cookies.
2. **Coarse-Grained Privileges**: Administrative access (root/Administrator) provides all-or-nothing control over kernel space, drivers, and user processes.
3. **Implicit Network Trust**: Devices on the same local area network (LAN) frequently broadcast discovery packets with minimal cryptographic verification.

Project Zero rejects ambient authority and replaces it with:
* **Strict Object-Capability (Cap-based) Security** inside the OS.
* **Cryptographic Hardware-Rooted Identity** across physical devices.
* **Dynamic, Attenuated Delegation** of permissions.
* **Instant, Verifiable Revocation** of both local capabilities and remote node trust.

---

## 2. Threat Model & Boundaries

### 2.1 Adversary Capabilities Considered
* **Malicious Local Application**: An unprivileged process attempting to escalate privileges, read another process's memory, or spy on input devices.
* **Compromised Hardware Driver**: A rogue or buggy third-party driver attempting to overwrite kernel structures or exfiltrate private user data.
* **Untrusted Network Observer (Man-in-the-Middle)**: An attacker monitoring, modifying, or replaying packets between personal devices over local Wi-Fi or the public Internet.
* **Stolen or Compromised Peer Device**: A previously trusted device (e.g., a lost smartphone) falling into physical possession of an adversary.
* **Adversarial AI Model / Hallucinated Agent**: An AI subsystem generating unauthorized destructive commands, accessing restricted files, or exfiltrating data outside the user's explicit intent.

### 2.2 Security Invariants
1. **No Ambient System Calls**: There is no `open("/etc/shadow")` or `system()` call. A process can only act on an object if it holds an unforgeable capability reference to that specific object.
2. **Kernel Non-Bypassability**: All capabilities are managed inside kernel-protected memory structures (C-Lists). User processes only hold opaque indexes.
3. **Network Encryption Everywhere**: There is no unencrypted communication across the Personal Compute Fabric, even on localhost or local Wi-Fi.

---

## 3. Capability-Based Security Inside the Node

### 3.1 What is a Capability?
A capability in Project Zero is a tuple containing:
$$\text{Capability} = (\text{Object Pointer}, \text{Rights Mask}, \text{Badge / Guard})$$

```text
+-----------------------------------------------------------------+
|                        CAPABILITY TOKEN                         |
+-------------------+--------------------+------------------------+
| Object Pointer    | Rights Bitmask     | Badge / Identifier     |
| (Kernel Address)  | (R, W, X, G, D, C) | (Unique Context ID)    |
+-------------------+--------------------+------------------------+
```

* **Object Pointer**: Direct address to the kernel-managed resource (e.g., Thread, Memory VMO, IPC Endpoint, Device MMIO, Notification Port).
* **Rights Bitmask**:
  * `R` (Read): Permission to read memory or receive IPC messages.
  * `W` (Write): Permission to write memory or send IPC messages.
  * `X` (Execute): Permission to execute code within a memory region.
  * `G` (Grant): Permission to transfer this capability to another process via IPC.
  * `D` (Delegate/Derive): Permission to create a child capability with attenuated (reduced) rights.
  * `C` (Cancel/Revoke): Permission to invalidate this capability and all its derived children.
* **Badge**: A kernel-injected cryptographic tag identifying the caller, preventing identity spoofing across IPC channels.

### 3.2 Capability Attenuation & Derivation
A process can attenuate its rights before delegating a capability.
* Example: A text editor holds a `VmoCap` with Read + Write + Grant rights.
* When delegating the buffer to a syntax highlighter or spell-checker, it attenuates the capability to **Read-Only**:

$$\text{Editor}(\text{Read, Write, Grant}) \xrightarrow{\text{Attenuate}} \text{SpellChecker}(\text{Read-Only})$$

The spell-checker has no mathematical or programmatic mechanism to upgrade its capability back to Write.

### 3.3 Revocation Tree
Every derived capability is linked in a kernel-maintained dependency tree:

```text
                  [Master Storage Capability] (Root)
                             |
            +----------------+----------------+
            |                                 |
    [Workspace A Cap]                 [Workspace B Cap]
    (Read, Write, Revoke)             (Read-Only)
            |
    [Temporary Plugin Cap]
    (Read-Only)
```

When the user closes Workspace A or revokes the plugin's access, calling `Revoke(Workspace A Cap)` automatically and instantaneously traverses the tree, nullifying all subordinate capabilities across all process tables.

---

## 4. Cryptographic Device Identity & The Trust Fabric

Across the distributed Personal Compute Fabric, physical machines identify themselves using modern asymmetric cryptography.

```text
+-------------------------------------------------------------------------+
|                         PHYSICAL DEVICE IDENTITY                        |
|                                                                         |
|  +---------------------------+       +-------------------------------+  |
|  | Hardware Root of Trust    |       | Ephemeral Identity Engine     |  |
|  | (TPM 2.0 / Apple SE /     | ----> | (Ed25519 Signing Keypair,     |  |
|  |  ARM CryptoCell / RISC-V) |       |  X25519 Key Exchange Keypair) |  |
|  +---------------------------+       +-------------------------------+  |
+-------------------------------------------------------------------------+
```

### 4.1 Device Enrollment & Mutual Pairing
Devices do not automatically form a cluster. Enrolling a new device into a user's Personal Fabric requires an **Out-of-Band (OOB) Cryptographic Handshake**:
1. **Device Announcement**: The new device (e.g., Laptop) broadcasts an ephemeral public key over a localized, short-range channel (BLE, Wi-Fi Direct, or optical QR code).
2. **Visual Verification**: Both devices display a high-entropy SAS (Short Authentication String) or visual cryptographic pattern.
3. **Mutual Signature Exchange**: Once verified by the user, the master device signs the new device's permanent public key with the User's Root Identity Key.
4. **Certificate Issuance**: The new device receives an identity certificate containing:
   * Unique Device UUID.
   * Public Key ($K_{\text{device}}$).
   * Hardware Attestation Proof.
   * Authorized Capability Scope (e.g., "Authorized for background compilation and GPU rendering; forbidden from biometric vault").

### 4.2 Secure Channel Transport: Noise Protocol Framework
All device-to-device traffic uses the **Noise Protocol Framework** (specifically `Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s`):
* Mutual authentication.
* Immediate forward secrecy via ephemeral Diffie-Hellman key exchange on every session.
* Identity hiding against passive eavesdroppers.
* Cryptographic resistance to replay attacks via monotonic nonce tracking.

---

## 5. Emergency Revocation & The "Lost Device" Protocol

If a trusted device (e.g., a smartphone or laptop) is stolen or lost, the user can trigger an **Emergency Severance** from any remaining trusted node:

```text
[User Triggers "Sever Device (Laptop-02)" on Phone]
                        │
                        ▼
[Generate Cryptographically Signed Revocation Certificate]
   (Signed by User Root Identity Key with Monotonic Counter)
                        │
                        ▼
[Broadcast Revocation over Compute Fabric & Push to Peer Relays]
                        │
                        ▼
[All Active Nodes Invalidate Session Keys for Laptop-02]
                        │
                        ▼
[Laptop-02 Memory Shredding (Self-Destruct if reachable)]
   * Ephemeral keys zeroized in RAM
   * Cached capability tokens wiped
   * Local encrypted storage keys locked
```

Even if the adversary prevents the lost device from connecting to the network, the device is completely severed: no other device in the fabric will accept its packets or share workloads.

---

## 6. The AI Capability Sandbox

Because Project Zero integrates AI as an OS-level subsystem, AI execution must be bounded by ironclad constraints:

```text
[User Prompt / Intent]
         │
         ▼
[AI Intent Parser (Isolated Sandbox)]
         │
         ▼ (Proposes Action: "Compile project in workspace X")
[Capability Policy Guard]
         │
    +----+----+
    |         |
[Allowed]  [Requires Escalation]
    |         |
    |         ▼
    |     [User Visual Confirmation Prompt]
    |         |
    v         v
[Issue Attenuated Capability Token]
    │
    ▼
[Dispatch Workload to Compute Fabric]
```

### 6.1 Invariants for AI Subsystems
1. **No Ambient File Access**: The AI cannot scan the filesystem arbitrarily. It can only inspect data within workspaces where the user has explicitly granted an active `WorkspaceReadCap`.
2. **Zero Direct Network Access**: AI models cannot open raw network sockets. Any external web lookup or cloud offload must be mediated by the system's Capability Broker.
3. **Immutable Audit Ledger**: Every capability invocation requested by an AI subsystem is logged to a write-only, tamper-evident cryptographic event log.
