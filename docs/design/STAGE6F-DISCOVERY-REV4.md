# STAGE 6F — DISCOVERY & DEPENDENCY AUDIT REV4

**Subsystem**: Stage 6 Final Subsystem Closure, Agent Spatial Grounding & Session Continuity  
**Document State**: Discovery & Architecture Audit Rev4  
**Author**: DeepMind Advanced Agentic Coding Team  
**Date**: October 2026  

---

## 1. Executive Summary & Audit Mission

Stage 6 of ZeroOS establishes the **Human Interaction, Spatial Presentation & Agent-Human Collaboration Subsystem**. Stages 6A through 6E defined, implemented, and froze five core daemons:

```text
Stage 6A  shelld       User Session Substrate & Workspace Layout Manager
Stage 6B  compositord  Distributed Spatial Presentation Protocol (SHM & Damage Rects)
Stage 6C  uids         Human Input Routing & Focus Authority
Stage 6D  intentd      Human Intent Interpretation & Resolution Engine
Stage 6E  observed     Human-Agent Telemetry, Interactive Feedback & Action Recorder
```

Rev3 established `groundd` as a read-only spatial indexing broker, eliminated `sessiond`, and specified geometric redaction for sensitive UI elements. 

**Rev4 Closes All Four Remaining Architectural Blockers**:
1. **Fresh-Destination Snapshot Anti-Replay**: Introduced `SessionMigrationEpoch` and destination transaction nonces (`dest_nonce`) to prevent snapshot replay attacks to fresh destination nodes lacking prior baseline history (`I-6F-SNAPSHOT-NONREPLAY`).
2. **Deterministic Generation Ownership**: Frozen strict single-authoritative writers: `shelld` exclusively owns `SurfaceLayoutGeneration`, and `surfaced` exclusively owns `SurfaceSemanticGeneration`. Consumers (`compositord`, `groundd`, `agentd`) cannot advance counters (`I-6F-GENERATION-OWNERSHIP`).
3. **Pre-Existing Remote Authority Derivation Path**: Specified that remote CSDT validation via `fabricd` passes context to `shelld`/`workspaced`, which invoke existing Stage 3H `sys_cap_derive` to issue new local `WorkspaceAccessCap` (`0x0030`) handles. Zero new derivation backdoors or authoui dependencies (`I-6F-REMOTE-AUTH-NO-AMPLIFICATION`).
4. **ABI Audit & State Taxonomy**: Verified zero opcode/error collisions for `groundd` (`0x0820`–`0x0823`, `ERR_STALE_SPATIAL_INDEX` `0x0730`) and explicitly enumerated migratable logical state vs non-migratable runtime state.

---

## 2. Area 1 — Fresh-Destination Snapshot Anti-Replay Architecture

### The Fresh-Destination Replay Problem
If a destination node has never hosted `Session S`, its local baseline record is `last_accepted(S) = None`. An attacker replaying a captured historical snapshot (e.g., `Generation 17`) to a fresh node would satisfy $\text{Generation 17} > \text{None}$, corrupting multi-node session isolation.

### The Rev4 Anti-Replay Solution

```text
Source Node (Node A)                    Destination Node (Node C - Fresh)
        │                                             │
        │ ─── 1. Initiate Handoff Request ──────────► │
        │                                             │ Generates fresh dest_nonce (random 64-bit)
        │ ◄── 2. Return dest_nonce ─────────────────── │
        │                                             │
Advances SessionMigrationEpoch (e.g., Epoch 24)        │
Signs Snapshot (Epoch 24 + dest_nonce + CSDT)          │
        │                                             │
        │ ─── 3. Transfer Signed Snapshot ──────────► │
                                                      │ Validates:
                                                      │   1. dest_nonce matches active request
                                                      │   2. Epoch 24 > local last_seen_epoch
                                                      │   3. Registers Session S Epoch 24
                                                      ▼
                                              Session Accepted
```

```rust
pub struct LogicalSessionSnapshotHeader {
    pub magic: u64,                   // 0x534553535F5A4552 ("SESS_ZER")
    pub session_migration_epoch: u64,  // Monotonic session export epoch counter
    pub dest_transaction_nonce: u64,   // Destination-issued single-use handoff nonce
    pub session_id: u64,               // Logical SessionId
    pub source_node_id: u64,           // Originating node ID
    pub dest_node_id: u64,             // Intended destination node ID
    pub timestamp_tsc: u64,            // Originating timestamp (authoritative local TSC)
    pub workspace_count: u32,          // Number of active workspaces serialized
    pub surface_count: u32,            // Number of logical surfaces serialized
    pub payload_crc32: u32,            // Cryptographic checksum
    pub _reserved: [u32; 3],           // Structure alignment
}
```

```text
I-6F-SNAPSHOT-NONREPLAY:
A logical session snapshot SHALL be rejected by any destination node (whether fresh
or previously initialized) unless it contains a valid, single-use destination transaction
nonce (dest_nonce) and an advancing SessionMigrationEpoch greater than any epoch previously
accepted for that session.
```

---

## 3. Area 2 — Deterministic Generation Ownership

### Single-Authoritative Writer Invariant
Dual ownership of layout or semantic generation counters is strictly prohibited. Exactly one authoritative service increments each generation counter:

```text
               ┌───────────────────────────────┐
               │         shelld (Owner)        │
               │   SurfaceLayoutGeneration++   │
               └───────────────┬───────────────┘
                               │ (Read-only layout generation)
                               ▼
 ┌───────────────────────────────────────────────────────────┐
 │                      groundd Index                        │
 └─────────────────────────────▲─────────────────────────────┘
                               │ (Read-only semantic generation)
               ┌───────────────┴───────────────┐
               │        surfaced (Owner)       │
               │  SurfaceSemanticGeneration++  │
               └───────────────────────────────┘
```

- **`SurfaceLayoutGeneration`**: Owned exclusively by **`shelld`**. Incremented whenever surface bounding boxes, viewport positions, Z-order layout, or workspace focus states change.
- **`SurfaceSemanticGeneration`**: Owned exclusively by **`surfaced`**. Incremented whenever application surface accessibility node trees change.
- **Consumers (`compositord`, `groundd`, `agentd`)**: Read generation counters to detect stale indices. Consumers **MUST NOT** increment or mutate layout or semantic generation counters.

```text
I-6F-GENERATION-OWNERSHIP:
Each spatial generation counter SHALL have exactly one authoritative writer (shelld for layout,
surfaced for semantics). Consumers SHALL NOT independently advance generation counters.
```

---

## 4. Area 3 — Pre-Existing Remote Authority Derivation Path

### Remote Capability Derivation Chain
Validation of a remote cryptographic identity (`CSDTToken`) proves peer identity, but **never** grants capability authority directly. Local capability derivation follows the established Stage 3H capability tree:

```text
Remote Principal
        │
        ▼
fabricd (CSDT Cryptographic Authentication & Peer Handshake)
        │ (Hands validated remote identity context)
        ▼
shelld / workspaced (Existing Local Workspace Authority)
        │
        ▼
sys_cap_derive (Pre-Existing Stage 3H Kernel Capability API)
        │
        ▼
New Local WorkspaceAccessCap (0x0030) Handle Issued
```

1. `fabricd` authenticates the remote node and validates the `CSDTToken`.
2. `fabricd` notifies `shelld` / `workspaced` (the pre-existing local workspace authorities holding primary workspace capability handles).
3. `shelld`/`workspaced` verify workspace containment policy and invoke Stage 3H kernel `sys_cap_derive` to issue a **new local `WorkspaceAccessCap` (`0x0030`)** handle bound to the local agent's process descriptor table.
4. `authui` participation is required **only** if the workspace policy specifies Class-3 re-authentication for remote handoff; automated background handoffs use established delegation rules without prompting.

```text
I-6F-REMOTE-AUTH-NO-AMPLIFICATION:
Validation of a remote CSDT principal SHALL NOT by itself create or imply local capability
authority. Any local WorkspaceAccessCap (0x0030) derivation SHALL pass through the
pre-existing shelld/workspaced capability derivation path.
```

---

## 5. Area 4 — ABI & Error Namespace Audit + Explicit State Taxonomy

### Opcode & Error Code Allocations
To prevent ABI collisions across Stage 3–6 namespaces, Stage 6F opcodes and errors are explicitly registered:

| Constant Identifier | Value | Namespace / Target | Purpose |
|---|---|---|---|
| `OP_GROUND_QUERY_SPATIAL` | `0x0820` | `groundd` IPC | Query spatial node hierarchy |
| `OP_GROUND_QUERY_SPATIAL_RESP` | `0x0821` | `groundd` IPC | Spatial query response |
| `OP_GROUND_SUBSCRIBE_CHANGES` | `0x0822` | `groundd` IPC | Subscribe to layout/semantic changes |
| `OP_GROUND_SUBSCRIBE_CHANGES_RESP` | `0x0823` | `groundd` IPC | Subscription acknowledgement |
| `ERR_STALE_SPATIAL_INDEX` | `0x0730` | ZeroError | Returned when layout/semantic generation is stale |

```text
I-6F-ABI-NAMESPACE-PRESERVED:
All Stage 6F IPC opcodes (0x0820–0x0823) and error codes (0x0730) SHALL occupy
unallocated, non-overlapping ranges within the ZeroOS ABI registry.
```

---

### Explicit State Serialization Taxonomy

To ensure runtime safety during multi-node session handoff, snapshot payload contents are strictly categorized:

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                              MIGRATABLE LOGICAL STATE                                  │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ • SessionId (u64) & WorkspaceId(s) (u64)                                               │
│ • Focused WorkspaceId (u64)                                                            │
│ • Logical SurfaceId(s) (u64) & Relative Window Layout Geometry Ratios                  │
│ • Surface Z-order hierarchy & Surface Focus State                                      │
│ • Surface Title Hashes & Logical Application Role Tags                                 │
│ • Active Agent Visualization Bindings & Telemetry Subscription Parameters               │
│ • SessionMigrationEpoch (u64) & Snapshot Generation Counters                           │
└────────────────────────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────────────────────────┐
│                        NON-MIGRATABLE EPHEMERATION RUNTIME STATE                       │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ ❌ Kernel Capability Handles (u32)                                                     │
│ ❌ Process IDs (PIDs) / Thread IDs (TIDs)                                              │
│ ❌ IPC Socket Descriptors / Endpoint IDs                                               │
│ ❌ SHM Shared Memory Pointers, Offsets, or Memory Handles                              │
│ ❌ Physical Hardware Device IDs / Display Output Handles                               │
│ ❌ Network Sockets or Socket File Descriptors                                          │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Summary of Architectural Subsystem Roles

$$\begin{aligned}
\texttt{groundd} &\implies \mathbf{SEE} \quad \text{(Read-Only Spatial Grounding Query Broker)} \\
\texttt{uids} &\implies \mathbf{INPUT} \quad \text{(Human Input Observation \& Focus Routing)} \\
\texttt{intentd} &\implies \mathbf{INTERPRET} \quad \text{(Intent Validation \& Execution Plan Compiler)} \\
\texttt{agentd} &\implies \mathbf{AGENT} \quad \text{(Workflow Proposal Synthesizer)} \\
\texttt{workloadd} &\implies \mathbf{EXECUTE} \quad \text{(Workload DAG Task Scheduler)} \\
\texttt{observed} &\implies \mathbf{TELEMETRY} \quad \text{(Action Recording \& Feedback Prompting)} \\
\texttt{authui} &\implies \mathbf{AUTHORIZE} \quad \text{(Class-3 Trusted Privilege Prompts)}
\end{aligned}$$

---

## 7. Complete 16 Machine Acceptance Gates (`6F-1` through `6F-16`)

1. **Gate 6F-1: Spatial Query ABI Alignment**  
   `SpatialNodeQuery` (64 bytes) and `SpatialNodeResponse` (128 bytes) verified via compile-time `core::mem::size_of` assertions.
2. **Gate 6F-2: Spatial Element Grounding**  
   `groundd` returns valid surface bounding boxes for Tier 1 elements.
3. **Gate 6F-3: Sensitive Element Geometric Masking**  
   Elements marked `FLAG_SENSITIVE` return `BOUNDS_REDACTED` `[0, 0, 0, 0]` and `NODE_REDACTED` (`0xFF`).
4. **Gate 6F-4: Trusted Overlay Exclusion**  
   Stage 5 `authui` overlays return 0 elements to `groundd` queries.
5. **Gate 6F-5: Workspace Containment Isolation**  
   Queries lacking a matching `WorkspaceAccessCap` (`0x0030`) return `PermissionDenied`.
6. **Gate 6F-6: Read-Only Query Mutation Rejection**  
   Write/mutation IPC opcodes sent to `groundd` return `InvalidRequest`.
7. **Gate 6F-7: Logical Session Snapshot Serialization**  
   `shelld` correctly serializes `LogicalSessionSnapshot` containing only logical state.
8. **Gate 6F-8: Ephemeral Handle Non-Migration Audit**  
   Verification that zero kernel handles (`u32`), PIDs, or SHM pointers are present in export payloads.
9. **Gate 6F-9: Fresh-Destination Snapshot Anti-Replay**  
   Verification that snapshots replayed to fresh destination nodes lacking baseline history are rejected via `dest_nonce` and `SessionMigrationEpoch` checks.
10. **Gate 6F-10: Remote CSDT Authority Derivation Audit**  
    `CSDTToken` validation derives new local `WorkspaceAccessCap` (`0x0030`) via `shelld`/`workspaced` without capability amplification.
11. **Gate 6F-11: Offline Autonomy Verification**  
    Local spatial queries execute 100% offline without remote network dependencies.
12. **Gate 6F-12: `groundd` Crash Non-Impact Recovery**  
    `groundd` crash removes query availability without modifying workspace, focus, capability, or authorization state.
13. **Gate 6F-13: Zero New Capability Authority Audit**  
    Verification that Stage 6F introduces zero new Stage 3H capability types, rights, or kernel derivation paths.
14. **Gate 6F-14: Physical Memory Manager (PMM) Neutrality**  
    PMM frame baseline matches final frame count after Stage 6F verification.
15. **Gate 6F-15: Stage 3A–3N Kernel Preservation**  
    Kernel nucleus files under `kernel/src/stage3/` retain zero modified bytes.
16. **Gate 6F-16: Spatial & Semantic Freshness Verification**  
    Queries with stale `SurfaceLayoutGeneration` or `SurfaceSemanticGeneration` return `ERR_STALE_SPATIAL_INDEX` (`0x0730`).

---

## 8. Summary of Rev4 Architectural Closures

| Area | Status in Rev4 |
|---|---|
| Snapshot anti-replay | 🟢 Destination transaction nonce (`dest_nonce`) + `SessionMigrationEpoch` (`I-6F-SNAPSHOT-NONREPLAY`). |
| Generation ownership | 🟢 `shelld` owns layout generation; `surfaced` owns semantic generation (`I-6F-GENERATION-OWNERSHIP`). |
| Remote local authority | 🟢 `CSDTToken` $\to$ `fabricd` $\to$ `shelld`/`workspaced` $\to$ `sys_cap_derive` $\to$ `WorkspaceAccessCap (0x0030)`. |
| ABI & error namespace | 🟢 Registered `0x0820`–`0x0823` and `ERR_STALE_SPATIAL_INDEX` (`0x0730`). |
| State taxonomy | 🟢 Explicitly enumerated migratable logical state vs non-migratable runtime state. |
| Capability identity | 🟢 Fixed to `WorkspaceAccessCap (0x0030)`. 0 new capability types. |
| Grounding principal model | 🟢 Caller Stage 3H handle verification chain (`I-6F-GROUND-CALLER-BOUND`). |
| Offline semantics | 🟢 Local grounding offline; remote handoff network-dependent. |
| Failure semantics | 🟢 Loss of observation only (`I-6F-GROUND-FAILURE`). |
| Semantic freshness | 🟢 `ERR_STALE_SPATIAL_INDEX` (`0x0730`) and Gate 6F-16. |

---
