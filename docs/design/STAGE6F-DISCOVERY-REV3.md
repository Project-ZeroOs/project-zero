# STAGE 6F — DISCOVERY & DEPENDENCY AUDIT REV3

**Subsystem**: Stage 6 Final Subsystem Closure, Agent Spatial Grounding & Session Continuity  
**Document State**: Discovery & Architecture Audit Rev3  
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

Rev2 of Stage 6F Discovery eliminated `sessiond`, introduced a 4-tier privacy classification for spatial queries, and enforced strict non-migration of runtime handles. 

**Rev3 Resolves All Eight Architectural Review Directives**:
1. **Capability Identity Correction**: Fixed capability type collision; `WorkspaceAccessCap` is explicitly referenced as its frozen Stage 3H / Stage 6E type **`0x0030`**. Zero new capability types are introduced.
2. **Grounding Principal Model**: Formally specified the caller verification chain (`Agent Identity` $\to$ `agentd` $\to$ `Stage 3H IPC Caller` $\to$ `WorkspaceAccessCap` $\to$ `groundd`).
3. **Remote Principal Local Derivation**: Confirmed `CSDTToken` proves identity/delegation only ($\text{CSDT} \neq \text{WorkspaceAccessCap}$). Local node policy derives a **new local `WorkspaceAccessCap`** handle upon arrival.
4. **Snapshot Anti-Replay Security**: Defined `LogicalSessionSnapshot` header fields (source/dest node ID, session ID, monotonic generation counter, timestamp) and anti-replay validation.
5. **Offline Operational Bounds**: Disambiguated local spatial grounding (100% offline) from remote session migration (network-dependent workload).
6. **Capability Acceptance Gate Refinement**: Updated Gate 6F-12 to "Zero New Capability Authority Audit" (0 new capability types/rights).
7. **`groundd` Crash Failure Semantics**: Specified that `groundd` crash causes loss of spatial observation availability only, without impacting security, focus, or kernel capabilities.
8. **Spatial & Semantic Freshness**: Introduced `SurfaceSemanticGeneration` and `SurfaceLayoutGeneration` counters to reject stale spatial index queries.

---

## 2. Topic A — Capability Identity Reference & Pre-Existing `WorkspaceAccessCap (0x0030)`

### Correction of Capability Type Reference
In Stage 6C, input policy capabilities were frozen:
- `RemoteInputPolicyCap` = `0x0041`
- `InputFocusPolicyCap` = `0x0042`
- `SyntheticInputCap` = `0x0043`
- `AccessibilityPolicyCap` = `0x0044`
- `AgentInputCap` = `0x0045`

In Stage 6E, workspace scoping was frozen:
- **`WorkspaceAccessCap` = `0x0030`**

Stage 6F **strictly reuses `WorkspaceAccessCap` (`0x0030`)**. No new capability types, rights, or kernel structures are introduced by Stage 6F.

```text
I-6F-ZERO-NEW-CAP-TYPES:
Stage 6F SHALL NOT introduce new capability object types, rights bits,
or kernel capability derivation routines; all spatial authorization relies
strictly on pre-existing WorkspaceAccessCap (0x0030).
```

---

## 3. Topic B — Grounding Principal Model & Caller Authorization

### Multi-Layer Principal Authorization Chain

```text
Agent Identity (UUID)
        │
        ▼
agentd Principal (Workload Context)
        │
        ▼
IPC Caller Handle Verification (Stage 3H Process Table)
        │
        ▼
WorkspaceAccessCap (0x0030 presented in handle table)
        │
        ▼
Spatial Visibility Policy Check (Tiers 0–3)
        │
        ▼
groundd Query Execution
```

`groundd` MUST NOT evaluate authorization using user-supplied workspace IDs in request payloads alone. Authorization is governed by the actual Stage 3H capability handle passed in the IPC descriptor slot.

```text
I-6F-GROUND-CALLER-BOUND:
A spatial query SHALL be authorized strictly by the caller's actual Stage 3H
capability authority (WorkspaceAccessCap 0x0030), not by workspace or session
identifiers supplied in IPC payload bytes.
```

---

## 4. Topic C — Remote Principal Local Authority Derivation

### CSDT Delegation vs Local Capability Rights

When an agent or user session migrates from Node A to Node B:
1. `CSDTToken` (Stage 4F) proves identity, delegation lineage, and cryptographic origin across the network.
2. `CSDTToken` **does NOT authorize spatial observation directly**. $(\text{CSDTToken} \neq \text{WorkspaceAccessCap})$.
3. Node B validates the `CSDTToken` via `fabricd` and `authui`.
4. Upon successful validation, Node B's local authority policy invokes Stage 3H to **derive a NEW local `WorkspaceAccessCap` (`0x0030`) handle** tied to Node B's process table.

```text
I-6F-CSDT-NOT-CAPABILITY:
A CSDT token SHALL serve strictly as a cryptographic transport and delegation container;
it SHALL NOT grant spatial observation rights until local authority policy derives
a new local WorkspaceAccessCap (0x0030) handle on the destination node.
```

---

## 5. Topic D — Snapshot Security & Anti-Replay Protection

### `LogicalSessionSnapshot` Structure & Header
A session snapshot exported for multi-node migration MUST contain complete logical state and anti-replay metadata:

```rust
pub struct LogicalSessionSnapshotHeader {
    pub magic: u64,                   // 0x534553535F5A4552 ("SESS_ZER")
    pub snapshot_generation: u64,      // Monotonically increasing snapshot counter
    pub session_id: u64,               // Logical SessionId
    pub source_node_id: u64,           // Originating node ID
    pub dest_node_id: u64,             // Intended destination node ID
    pub timestamp_tsc: u64,            // Creation timestamp (authoritative local TSC)
    pub workspace_count: u32,          // Number of active workspaces serialized
    pub surface_count: u32,            // Number of logical surfaces serialized
    pub payload_crc32: u32,            // Cryptographic checksum
    pub _reserved: [u32; 3],           // Alignment padding
}
```

### Replay Prevention Rule
Destination nodes maintain a record of the last accepted `snapshot_generation` for each active `session_id`. Any incoming snapshot with a generation less than or equal to the recorded generation is immediately discarded.

```text
I-6F-SESSION-SNAPSHOT-MONOTONIC:
A destination node SHALL reject any session snapshot whose snapshot_generation
is less than or equal to the highest generation previously accepted for that session_id.
```

---

## 6. Topic E — Offline Operational Bounds

Stage 6F functions are strictly classified into offline-autonomous and network-dependent operations:

```text
I-6F-OFFLINE-GROUNDING:
Spatial grounding queries (groundd), local session rendering (shelld / compositord),
and local layout reconstruction SHALL operate 100% offline without requiring network connectivity.

I-6F-NETWORK-DEPENDENCY:
Remote spatial session handoff across physical nodes is explicitly classified as a
network-dependent workload and SHALL fail-closed to local operation if networking is unavailable.
```

---

## 7. Topic F — Capability Gate Refinement

Machine Acceptance Gate 6F-12 is updated to reflect capability authority rules accurately:

> **Gate 6F-12: Zero New Capability Authority Audit**  
> Verify that Stage 6F introduces zero new Stage 3H capability object types, rights bits, kernel authority, or authority-amplifying derivation paths.

---

## 8. Topic G — `groundd` Failure Semantics

Because `groundd` is an unprivileged, read-only observation broker (and not an execution or authorization authority), its crash semantics are strictly bounded:

```text
I-6F-GROUND-FAILURE:
A failure or crash of groundd SHALL remove spatial observation query availability
for agents, but SHALL NOT modify workspace state, input focus, kernel capabilities,
surface ownership, workload execution, or Stage 5 authorization state.
```

---

## 9. Topic H — Semantic & Spatial Freshness Controls

To prevent agents from proposing actions based on stale UI spatial layout data (e.g., clicking a button whose position or semantic label has changed), `groundd` tracks two generation counters per surface:

- `SurfaceLayoutGeneration` (`u64`): Incremented by `compositord`/`shelld` whenever surface position, size, or Z-order changes.
- `SurfaceSemanticGeneration` (`u64`): Incremented by `surfaced` whenever application UI accessibility nodes change.

### Query Freshness Protocol
When `agentd` queries `groundd`, it may supply an expected generation counter. If the current generation on `groundd` exceeds the requested generation, `groundd` returns `ERR_STALE_SPATIAL_INDEX` (`0x0730`), prompting `agentd` to re-fetch the updated layout.

```text
I-6F-SPATIAL-FRESHNESS:
groundd SHALL reject spatial queries presenting a stale generation counter
with ERR_STALE_SPATIAL_INDEX, ensuring agents never act on outdated UI bounds.
```

---

## 10. Summary of Subsystem Layering & Authority Roles

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                                STAGE 6 SUBSYSTEM CLOSURE                               │
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

## 11. Updated 15 Machine Acceptance Gates (`6F-1` through `6F-15`)

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
   `shelld` correctly serializes `LogicalSessionSnapshot` without raw handles.
8. **Gate 6F-8: Ephemeral Handle Non-Migration Audit**  
   Verification that zero kernel handles (`u32`) or SHM pointers are present in export payloads.
9. **Gate 6F-9: Remote CSDT Anti-Replay Handoff Authentication**  
   Session snapshot handoff requires valid `CSDTToken` and rejects non-monotonic generation counters.
10. **Gate 6F-10: Offline Autonomy Verification**  
    Local spatial queries execute 100% offline without remote network dependencies.
11. **Gate 6F-11: `groundd` Crash Non-Impact Recovery**  
    `groundd` crash removes query availability without modifying workspace, focus, capability, or authorization state.
12. **Gate 6F-12: Zero New Capability Authority Audit**  
    Verification that Stage 6F introduces zero new Stage 3H capability types, rights, or kernel derivation paths.
13. **Gate 6F-13: Physical Memory Manager (PMM) Neutrality**  
    PMM frame baseline matches final frame count after Stage 6F verification.
14. **Gate 6F-14: Stage 3A–3N Kernel Preservation**  
    Kernel nucleus files under `kernel/src/stage3/` retain zero modified bytes.
15. **Gate 6F-15: Spatial & Semantic Freshness Verification**  
    Queries with stale `SurfaceLayoutGeneration` or `SurfaceSemanticGeneration` return `ERR_STALE_SPATIAL_INDEX`.

---

## 12. Summary of Rev3 Architectural Status

| Area | Status in Rev3 |
|---|---|
| Capability identity | 🟢 Fixed to `WorkspaceAccessCap (0x0030)`. 0 new cap types. |
| Grounding principal model | 🟢 Caller Stage 3H handle verification chain defined (`I-6F-GROUND-CALLER-BOUND`). |
| Remote principal model | 🟢 `CSDTToken` proves identity/delegation; local policy derives new local `WorkspaceAccessCap`. |
| Snapshot anti-replay | 🟢 Monotonic counter & anti-replay header defined (`I-6F-SESSION-SNAPSHOT-MONOTONIC`). |
| Offline semantics | 🟢 Local grounding offline; remote handoff network-dependent. |
| Capability gate wording | 🟢 Updated to "Zero New Capability Authority Audit". |
| Groundd failure semantics | 🟢 Loss of observation only (`I-6F-GROUND-FAILURE`). |
| Semantic freshness | 🟢 Dual generation counters & `ERR_STALE_SPATIAL_INDEX` gate added (`I-6F-SPATIAL-FRESHNESS`). |

---
