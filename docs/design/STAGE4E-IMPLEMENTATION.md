# Stage 4E Implementation Plan: Agent Runtime Subsystem (Rev 3)

## Executive Summary & Implementation Constraints

This document establishes the revised implementation plan for the **Stage 4E Agent Runtime Subsystem**.

The authoritative architectural contract is defined by:
- [`docs/design/STAGE4E-ARCHITECTURE-REV2.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/STAGE4E-ARCHITECTURE-REV2.md)
- [`docs/decisions/ADR-0028-agent-runtime-and-agent-security-model.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/decisions/ADR-0028-agent-runtime-and-agent-security-model.md)

### Strict Implementation Rules
1. **Architecture is Frozen**: No architectural alterations or new Ring 0 primitives are permitted.
2. **Substrate Preservation**: No code changes to Stage 3A–3N kernel nucleus, Stage 4A baseline, Stage 4B `resourced`, Stage 4C `workloadd`, or Stage 4D `workspaced`.
3. **Plan-Only Status**: This document is a planning deliverable. **No implementation code will be written during Phase 4E planning.**

---

## 1. Repository & Interface Audit

An audit of the existing codebase confirms the exact integration interfaces for Stage 4E:

```text
+-----------------------------------------------------------------------------------+
|                            REPOSITORIES & DAEMON AUDIT                            |
+-----------------------------------------------------------------------------------+
| Daemon / Component | Status      | Integration Point / IPC Contract               |
+--------------------+-------------+------------------------------------------------+
| kernel/src/        | Frozen      | sys_cap_derive, sys_cap_revoke, sys_cap_call  |
| brokerd/           | Frozen      | Services: "agentd.srv", OP_REGISTER_SERVICE    |
| resourced/         | Frozen      | OP_ALLOCATE_DISTRIBUTED_ID, ResourceLease      |
| workloadd/         | Frozen      | OP_WORKLOAD_CREATE, OP_WORKLOAD_CANCEL/QUERY   |
| workspaced/        | Frozen      | workspace.meta, context.graph, C_ws handle    |
| init/              | Frozen      | Launches agentd daemon during Stage 4 boot     |
| libzero/           | Modify      | Add libzero/src/agent.rs (opcodes 0x0501..08) |
| agentd/            | Create New  | freestanding no_std daemon binary              |
+-----------------------------------------------------------------------------------+
```

---

## 2. Stage 4E Implementation Boundary

The Stage 4E implementation is contained strictly within two components:

1. **`libzero/src/agent.rs`**: User-space library module defining IPC structs, error codes, opcodes (`0x0501`–`0x0508`), and `AgentClient` helper methods.
2. **`agentd/` Daemon Package**: Standalone `no_std` Rust package containing `agentd/Cargo.toml` and `agentd/src/main.rs`.

```text
+-----------------------------------------------------------------------------------+
|                            agentd DAEMON INTERNAL ARCHITECTURE                    |
+-----------------------------------------------------------------------------------+
|  +-----------------------------------------------------------------------------+  |
|  | AgentControlBlock Table (Static Array: [AgentControlBlock; 64])             |  |
|  |   - AgentId (u64) & WorkspaceId (u64)                                       |  |
|  |   - LifecycleState & RuntimeState                                           |  |
|  |   - C_agent_handle (Process-local u32 index into kernel process table)      |  |
|  |   - Active WorkloadId Array ([u64; 16])                                     |  |
|  |   - Trigger Table ([TriggerEntry; 16])                                      |  |
|  |   - Event Subscriptions ([EventSubscription; 32])                           |  |
|  +-----------------------------------------------------------------------------+  |
|  | Event Dispatcher & Queue Router                                             |  |
|  | Policy Evaluation Engine                                                     |  |
|  | ZeroFS Persistence Coordinator                                              |  |
|  +-----------------------------------------------------------------------------+  |
+-----------------------------------------------------------------------------------+
```

---

## 3. Capability Recovery & Derivation Path

Capabilities for Agents are derived and enforced strictly through Stage 3H kernel syscalls (`sys_cap_derive`, `sys_cap_revoke`).

### Precise Authority Derivation Path
```text
Persistent Workspace Identity (WorkspaceId)
        ↓
Request workspaced IPC for fresh Workspace capability handle root (C_ws)
        ↓
Kernel SYS_CAP_DERIVE
        ↓
Fresh Agent Capability Handle (C_agent) in agentd process capability table
        ↓
Kernel SYS_CAP_DERIVE (via workloadd IPC)
        ↓
Workload Capability Handle (C_workload)
```

### Process-Local Handle Rules & Recovery Protocol
- `agentd` stores process-local capability handle indices (`u32`) in RAM.
- **Process-local handles (`u32`) are NEVER written to disk**. ZeroFS stores only the persistent `WorkspaceId` and `AgentId`.
- On `agentd` crash recovery:
  1. `agentd` reads persistent `WorkspaceId` from `/ws/<ws_id>/agents/<agent_id>/agent.meta`.
  2. `agentd` invokes `workspaced` over IPC to re-obtain a valid process-local $C_{ws}$ root handle.
  3. `agentd` issues kernel `sys_cap_derive(C_ws, agent_rights)` to populate a fresh $C_{agent}$ `u32` index in `agentd`'s kernel capability table.
- Teardown of an Agent invokes `sys_cap_revoke(C_agent)`, immediately invalidating $C_{agent}$ and all derived $C_{workload}$ handles across the kernel capability tree.

```text
I-AGENT-KERNEL-AUTHORITY:
Kernel capability tables remain the sole authority. agentd cannot widen, amplify,
or synthesize capability handles independently of sys_cap_derive.
```

---

## 4. Time Authority Binding for Human Authorization

Clearance tickets for human authorization MUST be validated strictly using Stage 4B's qualified monotonic time authority (`libzero/src/time.rs`).

```text
Trusted Authorization Ticket (contains target AgentId, action_hash, deadline_ticks)
       ↓
agentd reads TimeObservationFrame via lock-free seqlock (TimeObservationFrame::read_observation())
       ↓
Evaluates freshness & deadline against qualified monotonic hardware TSC (read_canonical_tsc())
       ↓
           ├── OK: Ticket valid -> RuntimeState -> Executing -> Action Dispatched
           └── FAIL / TimeAuthorityLost: Ticket REJECTED -> RuntimeState -> Suspended
```

### Fail-Closed Behavior on TimeAuthorityLost
If `TimeObservationFrame::read_observation()` returns `ZeroError::TimeAuthorityUnavailable`, `TimeObservationStale`, or `HardwareRegressionDetected`:
1. All newly submitted authorization tickets are **immediately rejected**.
2. All active time-dependent clearances **fail closed**, transitioning the affected Agent back to `Suspended` runtime state until time qualification is restored.
3. Wall-clock or RTC time is **NEVER** used for authority or clearance validation.

---

## 5. Untrusted Model Integration Boundary

Model inference services run in user space as resource-backed Workloads. Model outputs are strictly untrusted data proposals:

$$\text{Model Output} \neq \text{Authority}$$

```text
Model / Inference Workload
          ↓
Untrusted Proposal Payload ("spawn workload X")
          ↓
agentd Policy Validation Engine
          ↓
Existing ZeroOS Authority (workloadd / workspaced / Kernel sys_cap_validate)
```

- Zero LLM SDKs, weights, prompt engines, or autonomous model execution inside `agentd`.
- Higher-level intent translation and model planning are strictly deferred to Stage 4F.

---

## 6. Event Authenticity, Header Validation & Generation Scoping

Events originate from authenticated system producers (`workspaced`, `workloadd`, `resourced`, `brokerd`, timers).

### Event Binary Payload Structure (`libzero/src/agent.rs`)
```rust
#[repr(C)]
pub struct EventMessage {
    pub producer_service_id: u64,
    pub producer_generation: u32,
    pub event_type: u32,
    pub sequence: u64,
    pub workspace_id: u64,
    pub payload_len: u32,
    pub payload: [u8; 1024],
}
```

### Generation-Scoped Monotonicity Validation Sequence
1. **Producer Authenticity**: `brokerd` attaches authenticated sender PID and Service ID to the IPC message header. `agentd` rejects events if producer PID does not match registered system daemons.
2. **Generation-Scoped Monotonicity**: `agentd` tracks `(producer_service_id, producer_generation)` tuples. When a producer restarts (`producer_generation` increments), `agentd` resets the expected `sequence` counter for that producer to 0, preventing stale sequence lockouts or authority inheritance across restarts.
3. **Cross-Agent Isolation**: Events are enqueued strictly into target Agent buffers matching both `WorkspaceId` and `event_type`.
4. **Suspended & Overflow Semantics**: When an Agent is `Suspended`, events queue up to `MAX_EVENT_SUBSCRIPTIONS` (32). Excess events are dropped, and an `EVENT_OVERFLOW` bit flag is set in the queue header.

---

## 7. Workload Termination & DETACH Protocol (`I-AGENT-DETACH-AUTHORITY`)

When an Agent moves to `Stopping`/`Terminated`, `agentd` executes an explicit protocol through **`workloadd` as the sole Workload execution manager**:

```text
Agent LifecycleState -> Stopping
       ↓
Enumerate Active WorkloadId Array ([u64; 16])
       ↓
For each WorkloadId, evaluate declared WorkloadPolicy:
       │
       ├── CANCEL (Default):
       │     agentd invokes workloadd IPC (OP_WORKLOAD_CANCEL)
       │     workloadd executes Task DAG cancellation, process teardown, lease quarantine
       │
       └── DETACH:
             agentd invokes workloadd IPC (OP_WORKLOAD_REPARENT, Target: Workspace Root)
             workloadd validates C_agent, updates WorkloadControlBlock ownership to Workspace Root,
             and notifies workspaced IPC (OP_WORKSPACE_ATTACH_WORKLOAD)
             workloadd continues executing Workload uninterrupted
       ↓
Kernel sys_cap_revoke(C_agent) invalidates capability subtree
       ↓
Agent LifecycleState -> Terminated
```

```text
I-AGENT-DETACH-AUTHORITY:
Agent termination must never directly mutate Workload execution state or ownership metadata
outside the authoritative 4C Workload service boundary. agentd routes all DETACH/CANCEL
requests exclusively to workloadd.
```

---

## 8. ZeroFS Persistence Substrate Audit & Durable Transaction Protocol

### Stage 3K / 4B Persistence Substrate Audit
Audit of `libzero/src/persistence.rs` and Stage 3K storage interfaces confirms:
- Non-volatile persistence uses `PersistenceAuthority::write_and_commit_slot()` for durable slot barriers.
- File storage uses capability-backed ZeroFS directory handles (`root_dir_handle`) via `sys_channel_send()`.
- **4E does NOT introduce POSIX-style FSYNC/RENAME syscalls** or unbacked filesystem primitives.

```text
I-AGENT-PERSISTENCE-SUBSTRATE:
Agent persistence uses only existing Stage 3K ZeroFS directory handle writes (via sys_channel_send)
and Stage 4B PersistenceAuthority::write_and_commit_slot barriers; 4E does not introduce a filesystem primitive.
```

### Transaction Ordering & Crash Recovery
```text
CREATE TRANSACTION:
  1. Write record to ZeroFS directory handle via sys_channel_send().
  2. Execute PersistenceAuthority::write_and_commit_slot() flush barrier.
  3. Update agent.meta state to Active.

CRASH RECOVERY RECONCILIATION:
  - On daemon boot, agentd scans /ws/*/agents/ on ZeroFS directory handles.
  - If agent.meta has LifecycleState = Creating:
      Incomplete creation detected -> purge record (cleanup).
  - If agent.meta has LifecycleState = Active:
      1. Restore AgentId, WorkspaceId, PrincipalId, GoalPolicy.
      2. Re-obtain fresh C_agent handle from workspaced via sys_cap_derive.
      3. Restore trigger rules and event subscriptions.
      4. Reconcile active Workload status by querying workloadd via OP_WORKLOAD_QUERY.
      5. Initialize RuntimeState to Idle.
  - If agent.meta has LifecycleState = Stopping / Terminated:
      Complete pending teardown and mark tombstone.
```

---

## 9. Static Memory Budget & Byte-Level BSS Accounting

`agentd` operates within a strict 1 MB static RAM budget:

```text
Per-AgentControlBlock RAM Footprint:
  - AgentId (u64) + WorkspaceId (u64) + PrincipalId (u64)     : 24 Bytes
  - LifecycleState (u8) + RuntimeState (u8) + flags (u16)      : 4 Bytes
  - C_agent_handle (u32 index)                                 : 4 Bytes
  - Active WorkloadId Array ([u64; 16])                        : 128 Bytes
  - Trigger Table ([TriggerEntry; 16] @ 32 B each)             : 512 Bytes
  - Event Subscriptions ([EventSubscription; 32] @ 32 B each)  : 1024 Bytes
  - Event Ring Buffer ([EventMessage; 32] @ 256 B compacted)   : 8192 Bytes
  - Goal Policy & Scratchpad Buffer                            : 1024 Bytes
  --------------------------------------------------------------------------
  Total per AgentControlBlock                                  : 11,192 Bytes (~10.9 KB)

Daemon BSS / Memory Budget Calculation (MAX_AGENTS = 64):
  - 64 × 11,192 Bytes (Control Block Table Array)              : 716,288 Bytes
  - Fixed IPC Rx/Tx Buffers (4 × 2048 B)                       : 8,192 Bytes
  - Service Lookup & Router Table Cache                        : 16,384 Bytes
  - Daemon Thread Execution Stack                              : 32,768 Bytes
  --------------------------------------------------------------------------
  TOTAL DAEMON RAM FOOTPRINT                                   : 773,632 Bytes (755.5 KB ≤ 1,048,576 Bytes / 1 MiB)
```

- Hard architectural limits (`MAX_IPC_AGENT_MSG_SIZE = 2048`, `MAX_DELEGATION_DEPTH = 4`) and configuration policy limits (`MAX_AGENTS = 64`) guarantee zero heap allocations in `agentd`.

---

## 10. IPC Protocol Specification (`0x0501`–`0x0508`)

`agentd` registers service handle `"agentd.srv"` with `brokerd`. All IPC requests use fixed 2048-byte frames:

```text
+-------------------+-------------------+-------------------+-----------------------+
| Opcode (u32)      | RequestId (u64)   | AgentId (u64)     | WorkspaceId (u64)     |
+-------------------+-------------------+-------------------+-----------------------+
| CapHandle (u32)   | Reserved (u32)    | Payload Bytes (1952 Bytes ...)                |
+-----------------------------------------------------------------------------------+
```

### Opcode Table

| Opcode | Binary Value | Capability Req. | Response Opcode | Description |
|---|---|---|---|---|
| `OP_AGENT_CREATE` | `0x0501` | $C_{ws}$ | `0x0581` | Instantiates a new Agent entity. |
| `OP_AGENT_DESTROY` | `0x0502` | $C_{agent}$ | `0x0582` | Transitions Agent to `Stopping` & evaluates Workloads.|
| `OP_AGENT_GET_STATE` | `0x0503` | $C_{agent}$ | `0x0583` | Returns Lifecycle & Runtime states. |
| `OP_AGENT_DISPATCH_GOAL`| `0x0504` | $C_{agent}$ | `0x0584` | Submits a GoalSpec for formulation. |
| `OP_AGENT_REGISTER_TRIGGER`|`0x0505`| $C_{agent}$ | `0x0585` | Registers an automated wake trigger. |
| `OP_AGENT_SUBSCRIBE_EVENT`|`0x0506`| $C_{agent}$ | `0x0586` | Registers event perception subscription. |
| `OP_AGENT_DELEGATE` | `0x0507` | $C_{agent}$ | `0x0587` | Spawns a Child Agent ($C_{child} \subseteq C_{agent}$).|
| `OP_AGENT_CLEAR_SUSPENSION`|`0x0508`| Ticket | `0x0588` | Resumes execution post human auth clearance. |

---

## 11. Machine-Level Verification Matrix (26 Tests: 4E-A to 4E-Z)

Machine-level acceptance is executed via python test harness (`tests/test_stage4e.py`) running inside Ring 3 QEMU.

```text
+-----------------------------------------------------------------------------------+
|                        STAGE 4E ACCEPTANCE MATRIX (4E-A to 4E-Z)                  |
+-----------------------------------------------------------------------------------+
| Test ID  | Target Area             | Verified Machine Behavior                    |
+----------+-------------------------+----------------------------------------------+
| 4E-A     | agentd Startup          | agentd registers "agentd.srv" with brokerd   |
| 4E-B     | Broker Registration     | Service directory returns agentd.srv handle  |
| 4E-C     | AgentId Allocation      | DistributedIdAllocator generates unique u64  |
| 4E-D     | Agent Creation          | OP_AGENT_CREATE returns valid AgentId        |
| 4E-E     | Workspace Containment   | Agent bound strictly to single WorkspaceId   |
| 4E-F     | Cap Attenuation         | C_agent derived from C_ws via sys_cap_derive |
| 4E-G     | Amplification Rejection | C_agent exceeding C_ws rejected by kernel    |
| 4E-H     | Agent Lifecycle         | Creating -> Active -> Stopping validated     |
| 4E-I     | Workload Creation       | agentd creates Workload via workloadd IPC    |
| 4E-J     | Workload Observation    | agentd queries Workload status via workloadd |
| 4E-K     | Workload Cancellation   | agentd cancels running Workload via workloadd|
| 4E-L     | Termination Policy      | CANCEL cancels workload; DETACH reparents it |
| 4E-M     | Authenticated Events    | Event from valid producer wakes Agent        |
| 4E-N     | Cross-Agent Event Rej.  | Event from unauthorized sender rejected      |
| 4E-O     | Event Buffer Overflow   | 33rd event drops and sets EVENT_OVERFLOW     |
| 4E-P     | Trigger Execution       | Matching event condition fires trigger       |
| 4E-Q     | Human Auth Request      | High-impact action moves state -> Suspended  |
| 4E-R     | Unauthorized Auth Rej.  | Invalid, expired or TimeAuthorityLost ticket |
| 4E-S     | Model Non-Authority     | Model output cannot invoke kernel syscall    |
| 4E-T     | Persistent State        | Slot write barrier on ZeroFS verified        |
| 4E-U     | Agent Crash Recovery    | agentd recovers Active agents on restart     |
| 4E-V     | Cap Handle Recon.       | Fresh C_agent re-derived on startup          |
| 4E-W     | Workspace Deletion      | Workspace deletion destroys child Agents     |
| 4E-X     | Protocol Robustness     | Malformed/wrong-direction frames rejected    |
| 4E-Y     | PMM Neutrality          | Zero frame leak across 100 Agent lifecycles  |
| 4E-Z     | Substrate Preservation  | Stage 3A-3N kernel code 100% untouched       |
+-----------------------------------------------------------------------------------+
```

---

## 12. Phased Implementation Roadmap

Implementation will proceed in 10 sequential phases:

```text
+-----------------------------------------------------------------------------------+
|                          10-PHASE IMPLEMENTATION ROADMAP                          |
+-----------------------------------------------------------------------------------+
| Phase   | Focus Area               | Target Deliverables                          |
+---------+--------------------------+----------------------------------------------+
| 4E.1    | libzero Protocol Module  | Create libzero/src/agent.rs & opcodes        |
| 4E.2    | agentd Package Scaffolding| Create agentd/Cargo.toml & main.rs skeleton   |
| 4E.3    | Identity & Control Block | Implement AgentControlBlock & resourced ID   |
| 4E.4    | Workspace Integration    | Implement C_agent derivation & workspaced IPC|
| 4E.5    | Workload Integration     | Implement workloadd IPC & termination policy |
| 4E.6    | Event Perception Loop    | Implement event queue, router & filters      |
| 4E.7    | Trigger Engine           | Implement TriggerEntry matching engine       |
| 4E.8    | Human Auth Boundary      | Implement Suspended state & ticket clearance |
| 4E.9    | ZeroFS Persistence       | Implement atomic metadata transactions       |
| 4E.10   | Ring 3 QEMU Verification | Create tests/test_stage4e.py & 26-test suite |
+-----------------------------------------------------------------------------------+
```

---

## 13. Implementation Plan Risk Register

| Risk / Dependency | Impact | Mitigation Strategy |
|---|---|---|
| **IPC Buffer Overflow** | High | Fixed 2048-byte message alignment enforced by `libzero/src/agent.rs`. |
| **Capability Leaks on Crash** | Critical | Kernel capability table handles are process-isolated and reclaimed on daemon exit. |
| **Unbounded Event Buffering**| Medium | Fixed array size 32 per Agent with explicit `EVENT_OVERFLOW` drop policy. |
| **Stale Authorization Tickets**| High | Strict TTL checking against qualified Stage 4B monotonic time in `evaluate_freshness()`. |

---

```text
STATUS: APPROVED — IMPLEMENTATION AUTHORIZED
STAGE 3 MODIFICATIONS: NONE
STAGE 4A MODIFICATIONS: NONE
STAGE 4B MODIFICATIONS: NONE
STAGE 4C MODIFICATIONS: NONE
STAGE 4D MODIFICATIONS: NONE
```
