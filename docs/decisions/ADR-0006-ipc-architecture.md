# ADR-0006: Inter-Process Communication (IPC) Foundation

## Status
**Accepted**

## Date
2026-09-13

## Context & Architectural Problem

In a capability microkernel operating system like Project Zero, Inter-Process Communication (IPC) is the primary architectural boundary separating user workspaces, device drivers, system services, and the Personal Compute Fabric. In monolithic kernels, intra-kernel communication occurs via function calls; in a microkernel, almost every interaction crosses an isolation boundary. Therefore, IPC performance, predictability, and memory behavior define the overall throughput and latency ceiling of the operating system.

---

## Evaluation of IPC Paradigms

### 1. Asynchronous Message Passing (Buffered Queues / Mailboxes)
* **Mechanism**: Senders write messages into kernel-allocated buffers. The sender continues execution immediately without waiting for the receiver. The receiver polls or is asynchronously notified when messages arrive.
* **Advantages**:
  * Decoupled execution timelines: sender and receiver do not need to be scheduled concurrently.
  * Natural fit for bursty, non-blocking telemetry and event notification loops.
* **Tradeoffs & Bottlenecks**:
  * **Memory Exhaustion & DoS**: Unbounded buffering requires dynamic heap allocations inside the microkernel. Senders can overwhelm the kernel with unread messages, requiring complex watermark, throttling, and drop policies.
  * **Double Copying**: Messages must be copied from sender space into kernel buffer, and later from kernel buffer into receiver space.
  * **Higher Latency**: Two independent scheduling cycles are required before a response is received.

### 2. Synchronous Rendezvous Message Passing (seL4 / L4 Microkernel Model)
* **Mechanism**: Communication occurs only when both sender and receiver meet at a shared synchronization primitive (the `Endpoint`). Whichever thread arrives first blocks until the counterpart arrives. When both are present, data is transferred directly between the execution contexts (via registers or caller/receiver stack frames), and the receiver is immediately scheduled.
* **Advantages**:
  * **Zero Kernel Dynamic Memory**: The endpoint only needs to store a pointer to the blocked thread's context. Zero message buffering occurs in kernel space; the kernel is completely immune to buffer-overflow DoS attacks.
  * **Direct Thread Handoff**: The scheduler can immediately donate the remaining time quantum of the sender directly to the receiver on the same CPU core, bypassing the general runqueue and maximizing CPU cache locality.
  * **Deterministic Capability Delegation**: Rights and capabilities are transferred atomically at the instant of rendezvous.
* **Tradeoffs & Bottlenecks**:
  * **Blocking Semantics**: Senders block until receivers are ready, requiring non-blocking variations (`try_send` / `nb_send`) or separate worker threads to avoid deadlocks.
  * **Unsuitable for Bulk Streaming**: Passing multi-megabyte video frames or audio streams through synchronous register copies is inefficient.

### 3. Shared Memory Buffers (Virtual Memory Objects / VMOs)
* **Mechanism**: Two processes map the same physical frames into their respective virtual address spaces. Coordination is achieved via atomic variables, futexes, or circular ring buffers.
* **Advantages**:
  * Maximum throughput: Zero copies of data; ideal for graphics frames, audio streams, and network packet buffers.
* **Tradeoffs & Bottlenecks**:
  * **No Implicit Synchronization**: Shared memory alone does not signal when data is ready or when buffer space is free. It requires an out-of-band signaling primitive (such as synchronous IPC or doorbells).
  * **Complex Access Control**: Granular capability revocation and cross-address-space isolation require explicit VMM synchronization and TLB shootdowns.

### 4. Hybrid IPC Architecture (Synchronous Rendezvous + Shared Memory VMOs)
* **Mechanism**: Control-plane operations, RPC handshakes, and capability delegations use synchronous rendezvous. Data-plane bulk streaming uses capability-granted shared memory ring buffers.

---

## Detailed Tradeoff Matrix

| Metric | Asynchronous Queues | Synchronous Rendezvous | Shared Memory Rings | Hybrid Model |
| :--- | :--- | :--- | :--- | :--- |
| **Round-Trip Latency** | High ($2\times$ schedule + buffer queue) | **Lowest** (Direct handoff, cache-hot) | Lowest for continuous data | **Optimal** |
| **Throughput (Small Msg)** | Moderate | **Maximum** (Register-based) | Moderate (Atomic overhead) | **Maximum** |
| **Throughput (Bulk Data)** | Poor (Copy overhead) | Poor (Frame fragmentation) | **Maximum** (Zero copy) | **Maximum** |
| **Kernel Memory Footprint** | Dynamic heap buffers (Unbounded) | **Zero** (Static endpoint pointers) | Static page tables | **Minimal** |
| **Scheduler Interaction** | Deferred wakeups | **Direct Quantum Handoff** | Userspace atomics / futex | Handoff on signal |
| **Security & Capabilities** | In-flight orphan risks | **Atomic transfer at rendezvous** | Needs granular ACLs | Optimal boundary |
| **Distributed Scaling** | Natural queuing | Transparent RPC proxying | Needs network replication | Seamless via proxy |

---

## Decision for Stage 2 & Staged Roadmap

**We select Synchronous Direct Rendezvous as the fundamental Stage 2 IPC primitive.**

### 1. Stage 2 Implementation Scope
* The kernel introduces `IpcEndpoint`, `IpcMessage`, `Sender`, and `Receiver`.
* Senders and receivers coordinate synchronously:
  * If a receiver calls `receive()` on an empty endpoint, it transitions to `ThreadState::Blocked` and records its thread handle.
  * When a sender calls `send()`, the payload (32 bytes: `sender_id`, `label`, and 24-byte inline register payload) is copied directly to the waiting receiver, unblocking it.
  * If no receiver is waiting, the sender transitions to `ThreadState::Blocked` until a receiver arrives.
* **Zero Allocation Invariant**: Message transfers require zero dynamic heap allocations.

### 2. Roadmap to Stage 3 & Personal Compute Fabric
* **Stage 3 (User Space & Capabilities)**: Capability tokens will guard endpoints (C-Lists). Direct thread handoff scheduling will transfer execution across Ring 3 address spaces.
* **Stage 4 (Distributed Fabric)**: When an endpoint references a remote node across the Personal Compute Fabric, the local microkernel stub transparently packages the synchronous request into an authenticated, encrypted peer-to-peer network packet without altering the application-level IPC contract.
