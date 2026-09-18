# ZeroOS Phase 4A Implementation Report

**Status**: 🟢 VERIFIED & READY FOR FREEZE
**Subsystem**: Core System Service Runtime & Capability Directory (`init`, `brokerd`, `libzero`)
**Frozen Specification**: Stage 4 Architecture Rev6 & ADR-0024 Rev6
**Kernel Preservation**: Stage 3A–3N Inviolate

---

## 1. Executive Summary

Phase 4A establishes the minimal freestanding user-space system service runtime for ZeroOS. It introduces:
1. **`libzero`**: A zero-dependency, freestanding `#![no_std]` runtime library providing safe syscall abstractions, Stage 3G IPC serialization (80-byte `IpcMessage`), broker client APIs, and Two-Man Rule protocol foundations.
2. **`brokerd`**: The primary system service broker and directory daemon. Manages service registration, lookup, endpoint publication, and capability mediation with strictly bounded static memory structures.
3. **`init`**: The Ring 3 user-space supervisor responsible for process bootstrap, service lifecycle management, failure detection, bounded restart policy, and orderly shutdown coordination.
4. **Machine Verification Suite**: 12 mandatory machine tests (4A-A through 4A-L) executed in headless QEMU, verifying end-to-end user-space runtime correctness, kernel capability authority preservation, fault containment, and physical memory neutrality.

---

## 2. Implementation Mapping to Architecture Rev6

| Architecture Contract (Rev6) | Implementation Component | Verification Mechanism |
| :--- | :--- | :--- |
| **`I-KERNEL-CAPABILITY-AUTHORITY`** | Stage 3H `sys_cap_derive` / `capability_derive` | Test 4A-F & 4A-G: Broker requests attenuation; kernel authoritatively enforces monotonic subset rights. |
| **`BROKER-NO-AUTHORITY-AMPLIFICATION`** | `brokerd/src/registry.rs` & `libzero::broker` | Test 4A-G: Unauthorized rights requests trigger `RightsAmplificationRejected` from kernel. |
| **`I-SERVICE-IDENTITY-DECOUPLING`** | `ServiceId` (starts at 100), `Generation` (starts at 1) | Test 4A-C, 4A-H, 4A-K: Identity independent of array slot; restart advances generation. |
| **`I-ENDPOINT-HANDLE-LOCALITY`** | Ephemeral process-local handles; Stage 3G handle transfer | Test 4A-D, 4A-E, 4A-H: Handles never treated as global IDs; old handles invalidated on restart. |
| **`I-SUPERVISOR-LIFECYCLE-SPLIT`** | `init` owns lifecycle; `brokerd` owns directory generation | Test 4A-A, 4A-B, 4A-H, 4A-L: Clear division of authority; zero conflicting mutations. |
| **`I-BOUNDED-RESTART-POLICY`** | `DEFAULT_MAX_RETRIES = 3` in `Supervisor` | Test 4A-H: Fail-closed behavior after retry threshold; no uncontrolled infinite restart loops. |
| **`I-FAULT-CONTAINMENT`** | Stage 3G channel cleanup & peer-closed signaling | Test 4A-I: Abrupt client crash cleanly reclaimed; brokerd and kernel remain fully stable. |
| **`I-PMM-NEUTRALITY`** | Strict PMM frame tracking | Machine assertion: `baseline_free == current_free` across all test groups. |

---

## 3. Subsystem Boundaries & Authority Separation

```text
                    STAGE 3 KERNEL
                           │
              ┌────────────┴────────────┐
              ↓                         ↓
          Kernel IPC              Kernel Capabilities
      (Channels, Handles)         (sys_cap_derive)
              │                         │
              └────────────┬────────────┘
                           ↓
                        init
                  (Ring 3 User Space)
                           │
                      supervises
                           ↓
                        brokerd
                  (Ring 3 System Service)
                           │
                  service directory
                           │
                 capability-mediated
                      rendezvous
                           ↓
                        libzero
              (User-space Freestanding Library)
                           ↓
                 Stage 4 Applications
                & Future System Services
```

### Authority Matrix

| Authority Domain | Kernel | `init` | `brokerd` | `libzero` |
| :--- | :---: | :---: | :---: | :---: |
| **Capability Rights Enforcement** | **Authoritative** | None | Client Policy | None |
| **Capability Derivation Tree (CDT)** | **Authoritative** | None | None | None |
| **IPC Message & Handle Transfer** | **Authoritative** | None | None | None |
| **Service Lifecycle State Machine** | None | **Authoritative** | None | None |
| **Process Supervision & Restart Policy** | None | **Authoritative** | None | None |
| **Service Directory Records** | None | None | **Authoritative** | None |
| **Service Generation Monotonicity** | None | None | **Authoritative** | None |
| **User-Space Syscall Convenience** | None | None | None | **Authoritative** |

---

## 4. Protocol Definitions (Stage 3G IPC)

All communication uses the frozen 80-byte `IpcMessage` layout:
```rust
#[repr(C)]
pub struct IpcMessage {
    pub tag: u64,             // Opcodes 0x1001 .. 0x100A
    pub payload_len: u16,     // 0 .. 48 bytes
    pub handles_count: u16,   // 0 .. 4 attached handle descriptors
    pub _reserved1: u32,      // Explicit padding
    pub payload: [u8; 48],    // Inline packet data
    pub handles: [u32; 4],    // Transferred process-local capability handles
}
```

### Protocol Opcodes & Payloads

| Opcode | Tag | Request Payload | Attached Handles | Response Payload | Attached Handles |
| :--- | :---: | :--- | :---: | :--- | :---: |
| `REGISTER_SERVICE` | `0x1001` | Name (32B) + Rights (2B) | `[srv_endpoint]` | Status (4B) + ServiceId (4B) + Gen (4B) | None |
| `UNREGISTER_SERVICE` | `0x1003` | ServiceId (4B) + Gen (4B) | None | Status (4B) | None |
| `LOOKUP_SERVICE` | `0x1005` | Name (32B) | None | Status (4B) + ServiceId (4B) + Gen (4B) + Rights (2B) | `[transferred_endpoint]` |
| `SERVICE_STATUS` | `0x1007` | ServiceId (4B) | None | Status (4B) + Gen (4B) + State (4B) | None |
| `DELEGATE_CAP` | `0x1009` | ServiceId (4B) + Rights (4B) | `[parent_cap]` | Status (4B) | `[derived_cap]` |

---

## 5. Capability-Mediated Rendezvous & Endpoint Transfer

1. **Handle Locality Invariant**: Handle values (`u32`) are process-local indices into each process's `HandleTable`. They are never passed as bare integer values representing global identities.
2. **Registration Transfer**: When a service provider issues `REGISTER_SERVICE`, the provider's local channel handle is attached in `IpcMessage.handles[0]`. The kernel IPC subsystem (`channel_send` / `channel_receive`) moves or copies the handle reference into `brokerd`'s handle table, creating `broker_local_endpoint`.
3. **Lookup & Discovery**: When a client issues `LOOKUP_SERVICE`, `brokerd` inspects the directory record, retrieves `broker_local_endpoint`, and sends it back in `IpcMessage.handles[0]`. The kernel installs a new valid capability handle into the client's handle table.
4. **Delegation**: When deriving attenuated capabilities, `brokerd` calls `sys_cap_derive(broker_local_endpoint, requested_rights, &mut child_handle)`. The kernel verifies monotonic rights reduction (`child_rights ⊆ parent_rights`) and depth limits before granting the child handle.

---

## 6. Service Lifecycle State Machine

`init` implements a deterministic, bounded lifecycle state machine for all supervised system processes:

```text
Declared ──> Starting ──> Running ──> Failed ──> Restarting ──> Running
                           │
                           └──> Stopping ──> Stopped
```

- **Bounded Restart Policy**: Every service tracks `restart_count`. If `restart_count < max_retries` (default: 3), the supervisor transitions `Failed -> Restarting`. If the threshold is exceeded, the service remains `Failed` (fail-closed), eliminating uncontrolled restart loops.
- **Orderly Shutdown**: `shutdown_all()` cleanly transitions all active services through `Stopping -> Stopped`, releasing resources prior to calling `sys_exit(0)`.

---

## 7. Machine Verification Suite (4A-A through 4A-L)

| Test ID | Test Name | Invariant / Contract Verified | Status |
| :--- | :--- | :--- | :---: |
| **4A-A** | User Init Boot | Supervisor state transitions: `Declared -> Starting -> Running`. | 🟢 PASS |
| **4A-B** | Broker Startup | `init` bootstraps `brokerd`; initializes directory with `next_service_id = 100`. | 🟢 PASS |
| **4A-C** | Service Registration | Service registers channel endpoint; broker records Generation 1. | 🟢 PASS |
| **4A-D** | Service Discovery | Lookup returns `ServiceId`, `Generation`, and transfers endpoint handle. | 🟢 PASS |
| **4A-E** | IPC Rendezvous | Real bidirectional `PING`/`PONG` exchange over transferred Stage 3G channel. | 🟢 PASS |
| **4A-F** | Capability Delegation | Broker calls `capability_derive`; proves `child_rights ⊆ parent_rights`. | 🟢 PASS |
| **4A-G** | Amplification Rejection | Rights amplification rejected by kernel with `RightsAmplificationRejected`. | 🟢 PASS |
| **4A-H** | Service Restart | Generation advances ($1 \to 2$); old endpoint handle cannot be reused. | 🟢 PASS |
| **4A-I** | Fault Containment | Client abruptly terminates; peer-closed signal asserted; broker/kernel stable. | 🟢 PASS |
| **4A-J** | Malformed Request | Fuzzed opcodes/lengths rejected with `InvalidRequest`; broker remains alive. | 🟢 PASS |
| **4A-K** | Identity Generation | Stale generation token rejected with `GenerationMismatch`. | 🟢 PASS |
| **4A-L** | Clean Shutdown | Supervisor transitions to `Stopped`; clean termination via `sys_exit`. | 🟢 PASS |
| **PMM** | PMM Neutrality | Zero net physical memory frame leakage (`before == after`). | 🟢 PASS |

---

## 8. Stage 3 ABI Preservation Audit

A rigorous audit of Stage 3 interfaces confirms zero modifications:
- `KernelThread ABI`: Unmodified.
- `PerCpu ABI`: Unmodified.
- `Process ABI`: Unmodified.
- `Handle ABI`: Unmodified (`Handle(pub u32)`).
- `CapabilityNode ABI`: Unmodified.
- `IpcMessage ABI`: Exactly 80 bytes, 8-byte aligned, identical offsets.
- `Channel ABI`: Unmodified.
- `SyscallFrame ABI`: Exactly 144 bytes, identical register map.
- Syscall numbering: Unmodified (`1=sys_exit`, `2=sys_yield`, `3=sys_channel_create`, ..., `10=sys_cap_derive`).

---

## 9. Known Limitations & Deferred Scopes

- **Two-Man Rule**: Phase 4A implements only the protocol foundation descriptor (`EscalationRequest`). Visual confirmation UI and cryptographic signatures are deferred to later phases.
- **Dynamic Schedulers**: Phase 4A supervisor uses static bounded tables (`MAX_SUPERVISED_SERVICES = 8`). Complex DAG scheduling belongs to Phase 4E.
- **Distributed Directory**: The service directory in 4A is strictly node-local. Distributed fabric federation belongs to Phase 4B/4C.
