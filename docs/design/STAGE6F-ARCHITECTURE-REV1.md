# STAGE 6F ARCHITECTURE REV1

**Subsystem**: Stage 6 Final Subsystem Closure, Agent Spatial Grounding & Session Continuity (`groundd`)  
**Document State**: Architecture Specification Rev1 — Frozen Architecture Draft  
**Author**: DeepMind Advanced Agentic Coding Team  
**Date**: October 2026  

---

## 1. Executive Summary & Subsystem Architecture

Stage 6F defines the final subsystem architecture of Stage 6 in ZeroOS (**Human Interaction, Spatial Presentation & Agent-Human Collaboration Subsystem**).

Stage 6F introduces:
1. **`groundd` (Spatial Grounding Query Broker)**: An unprivileged, read-only daemon that indexes spatial window coordinates from `shelld`/`compositord` and accessibility UI element metadata from `surfaced`. It serves structured spatial queries to agents without granting raw framebuffer access or hardware input authority.
2. **Multi-Node Logical Session Handoff**: A zero-handle logical snapshot handoff protocol that allows active user sessions and workspace window topologies to transfer between physical ZeroOS nodes safely via Stage 4F `fabricd` and CSDT tokens.

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                              STAGE 6 SUBSYSTEM ARCHITECTURE                            │
├─────────────┬─────────────┬─────────────┬─────────────┬─────────────┬──────────────────┤
│  Stage 6A   │  Stage 6B   │  Stage 6C   │  Stage 6D   │  Stage 6E   │     Stage 6F     │
│   shelld    │ compositord │    uids     │   intentd   │  observed   │     groundd      │
│  (Session)  │  (Spatial)  │   (Input)   │  (Intent)   │ (Telemetry) │    (Grounding)   │
└─────────────┴─────────────┴─────────────┴─────────────┴─────────────┴──────────────────┘
                                           │
                                           ▼
                                        STAGE 7
                       Autonomous Enterprise Fabric & System Lifecycle
```

---

## 2. Frozen Subsystem Responsibilities (Non-Overlapping Authority Matrix)

$$\begin{aligned}
\texttt{groundd} &\implies \mathbf{SEE} \quad \text{(Read-Only Spatial Grounding Query Broker)} \\
\texttt{uids} &\implies \mathbf{INPUT} \quad \text{(Human Input Routing \& Focus Authority)} \\
\texttt{intentd} &\implies \mathbf{INTERPRET} \quad \text{(Intent Validation \& Execution Plan Compiler)} \\
\texttt{agentd} &\implies \mathbf{AGENT} \quad \text{(Workflow Proposal Synthesizer)} \\
\texttt{workloadd} &\implies \mathbf{EXECUTE} \quad \text{(Workload DAG Task Scheduler)} \\
\texttt{observed} &\implies \mathbf{TELEMETRY} \quad \text{(Action Recording \& Feedback Prompting)} \\
\texttt{authui} &\implies \mathbf{AUTHORIZE} \quad \text{(Class-3 Trusted Privilege Prompts)}
\end{aligned}$$

```text
I-6F-SUBORDINATE-GROUNDD:
groundd SHALL operate as a read-only observation indexing broker.
It SHALL NOT create capability grants, modify workspace focus, execute tasks,
inject input events, override resource leases, or authorize Class-3 security prompts.
```

---

## 3. ABI Data Structures & Layout Contracts

All Stage 6F IPC data structures are binary-compatible, fixed-capacity, and aligned to 64-byte or 128-byte boundaries.

### 3.1 `SpatialNodeQuery` (64 Bytes)

```rust
#[repr(C, align(64))]
pub struct SpatialNodeQuery {
    pub workspace_id: u64,              // Target Workspace ID
    pub expected_layout_gen: u64,       // Expected SurfaceLayoutGeneration
    pub expected_semantic_gen: u64,     // Expected SurfaceSemanticGeneration
    pub bounding_box_min_x: i32,        // Region filter X min
    pub bounding_box_min_y: i32,        // Region filter Y min
    pub bounding_box_max_x: i32,        // Region filter X max
    pub bounding_box_max_y: i32,        // Region filter Y max
    pub max_nodes: u32,                 // Maximum nodes to return (max 16)
    pub query_flags: u32,               // Query options (0x01 = IncludeChildren)
    pub _reserved: [u8; 16],            // Alignment padding
}
```

### 3.2 `SpatialNodeResponse` (128 Bytes per Node)

```rust
#[repr(C, align(64))]
pub struct SpatialNodeDescriptor {
    pub surface_id: u64,                // Logical Surface ID
    pub node_id: u64,                   // Accessibility Element Node ID
    pub bounds_min_x: i32,              // X min [0,0,0,0] if BOUNDS_REDACTED
    pub bounds_min_y: i32,              // Y min
    pub bounds_max_x: i32,              // X max
    pub bounds_max_y: i32,              // Y max
    pub node_type: u8,                  // Element type (0xFF if NODE_REDACTED)
    pub privacy_tier: u8,               // Privacy tier (Tier 0, 1, 2)
    pub flags: u16,                     // Node flags
    pub layout_gen: u64,                // Authoritative SurfaceLayoutGeneration
    pub semantic_gen: u64,              // Authoritative SurfaceSemanticGeneration
    pub node_name_len: u32,             // String length
    pub node_name: [u8; 64],            // Zeroed if Tier 2 Redacted
    pub _reserved: [u8; 16],            // Alignment padding
}
```

```text
I-6F-ABI-ALIGNMENT:
All Stage 6F query structures SHALL satisfy compile-time size and alignment assertions:
core::mem::size_of::<SpatialNodeQuery>() == 64
core::mem::size_of::<SpatialNodeDescriptor>() == 128
```

---

## 4. Privacy Classification & Redaction Architecture

Spatial geometry (location, dimensions, structural shape) can reveal sensitive information even when text is scrubbed. `groundd` enforces a 4-tier spatial privacy model:

```text
Tier 0: Public Spatial Metadata
  ├── Desktop shell controls, window frames, public badges
  └── Visible to all authorized processes

Tier 1: Workspace-Private Metadata
  ├── Standard UI buttons, labels, scrollable regions within caller's workspace
  └── Visible to agents presenting valid WorkspaceAccessCap (0x0030)

Tier 2: Workspace-Sensitive Metadata
  ├── Password fields, PIN dialogs, credit card inputs, confidential documents
  └── TEXT = ZEROED; Bounds = BOUNDS_REDACTED [0, 0, 0, 0]; Node Type = NODE_REDACTED (0xFF)

Tier 3: Trusted-Only Overlay Metadata
  ├── Stage 5 authui modal confirmation prompts, capability dialogs
  └── COMPLETELY HIDDEN from groundd spatial queries (returns 0 elements)
```

```text
I-6F-GEOMETRIC-REDACTION:
UI elements marked FLAG_SENSITIVE (Tier 2) SHALL return bounds [0,0,0,0]
and node_type 0xFF. Stage 5 authui overlays (Tier 3) SHALL be completely omitted
from groundd spatial queries.
```

---

## 5. Grounding Principal Model & Caller Authorization

`groundd` MUST NOT evaluate caller authorization using unverified workspace IDs in IPC payload bytes alone.

### Caller Capability Verification Chain

```text
Agent Identity (UUID)
        │
        ▼
agentd Principal (Workload Context)
        │
        ▼
Stage 3H IPC Caller Handle Table Inspection
        │
        ▼
WorkspaceAccessCap (0x0030 presented in caller handle table)
        │
        ▼
Spatial Visibility Policy Check (Tiers 0–3)
        │
        ▼
groundd Query Execution
```

```text
I-6F-GROUND-CALLER-BOUND:
A spatial query SHALL be authorized strictly by the caller's actual Stage 3H
capability handle (WorkspaceAccessCap 0x0030) present in the IPC descriptor table,
not by workspace or session identifiers supplied in payload bytes.
```

---

## 6. Single-Authoritative Generation Ownership & Freshness

To prevent agents from acting on stale UI layout bounds (e.g., clicking a button whose position has moved), `groundd` tracks two generation counters per surface:

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

- **`SurfaceLayoutGeneration`**: Owned exclusively by **`shelld`**. Incremented whenever surface bounds, window positions, Z-order, or focus change.
- **`SurfaceSemanticGeneration`**: Owned exclusively by **`surfaced`**. Incremented whenever application surface accessibility node trees change.
- **Consumers (`compositord`, `groundd`, `agentd`)**: Read counters to verify index freshness; consumers **MUST NOT** increment generation counters.

### Query Freshness Rule
If an agent submits a query with `expected_layout_gen` or `expected_semantic_gen` less than the authoritative current generation on `groundd`, `groundd` returns `ERR_STALE_SPATIAL_INDEX` (`0x0730`).

```text
I-6F-GENERATION-OWNERSHIP:
Each spatial generation counter SHALL have exactly one authoritative writer (shelld for layout,
surfaced for semantics). Consumers SHALL NOT independently advance generation counters.

I-6F-SPATIAL-FRESHNESS:
groundd SHALL reject spatial queries presenting a stale generation counter
with ERR_STALE_SPATIAL_INDEX (0x0730), ensuring agents never act on outdated UI bounds.
```

---

## 7. Multi-Node Session Continuity & Anti-Replay Architecture

Session continuity allows active user sessions and workspace window topologies to migrate across physical ZeroOS nodes via Stage 4F `fabricd`.

### 7.1 Fresh-Destination Anti-Replay Handshake

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

### 7.2 `LogicalSessionSnapshotHeader` (64 Bytes)

```rust
#[repr(C, align(64))]
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
A logical session snapshot SHALL be rejected by any destination node unless it contains
a valid, single-use destination transaction nonce (dest_nonce) and an advancing
SessionMigrationEpoch greater than any epoch previously accepted for that session.
```

---

## 8. Ephemeral Authority Non-Migration & Remote Capability Derivation

### 8.1 State Serialization Taxonomy

$$\begin{array}{l|l}
\textbf{Migratable Logical State} & \textbf{Non-Migratable Runtime State (PROHIBITED)} \\
\hline
\text{SessionId (u64) \& WorkspaceId(s) (u64)} & \text{Kernel Capability Handles (u32)} \\
\text{Focused WorkspaceId (u64)} & \text{Process IDs (PIDs) / Thread IDs (TIDs)} \\
\text{Logical SurfaceId(s) \& Relative Layout Ratios} & \text{IPC Socket Descriptors / Endpoint IDs} \\
\text{Z-Order Hierarchy \& Focus State} & \text{SHM Shared Memory Pointers / Handles} \\
\text{Title Hashes \& Application Role Tags} & \text{Physical Hardware Device / Display Handles} \\
\text{Visualization Bindings \& Telemetry Parameters} & \text{Network Sockets / Socket Descriptors} \\
\text{SessionMigrationEpoch \& Snapshot Counters} & \text{Physical Memory Addresses}
\end{array}$$

```text
I-6F-HANDLE-NON-MIGRATION:
Logical session snapshots SHALL NOT contain raw kernel handles, PIDs, IPC descriptors,
SHM pointers, or physical addresses. Destination nodes SHALL reconstruct local runtime
resources from scratch upon receiving a logical snapshot.
```

### 8.2 Remote Local Capability Derivation Chain

Validation of a remote `CSDTToken` proves peer identity and delegation lineage, but **does not grant local capabilities directly**:

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

```text
I-6F-REMOTE-AUTH-NO-AMPLIFICATION:
Validation of a remote CSDT principal SHALL NOT by itself create local capability
authority. Any local WorkspaceAccessCap (0x0030) derivation SHALL pass strictly
through the pre-existing shelld/workspaced capability derivation path.
```

---

## 9. Offline Operational Bounds & Failure Semantics

```text
I-6F-OFFLINE-GROUNDING:
Spatial grounding queries (groundd), local session rendering (shelld / compositord),
and local layout reconstruction SHALL operate 100% offline without requiring network connectivity.

I-6F-NETWORK-DEPENDENCY:
Remote spatial session handoff across physical nodes is explicitly classified as a
network-dependent workload and SHALL fail-closed to local operation if networking is unavailable.

I-6F-GROUND-FAILURE:
A crash or failure of groundd SHALL remove spatial query availability for agents,
but SHALL NOT modify workspace state, input focus, kernel capabilities, surface ownership,
workload execution, or Stage 5 authorization state.
```

---

## 10. ABI Registry & Error Namespace Allocations

Stage 6F registers non-overlapping opcodes and errors in the ZeroOS ABI registry:

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

## 11. 16 Machine Acceptance Gates (`6F-1` through `6F-16`)

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

## 12. Stage 3A–3N Preservation Verification

```text
kernel/src/stage3/            0 bytes modified (Preserved)
Stage 3H Capability Types      0 new capability types (Reuses 0x0030)
Stage 4F Execution Plan Engine  100% preserved and reused
Stage 5 Compositor / authui    100% preserved and reused
```

---
