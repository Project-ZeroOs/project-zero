# STAGE 6F — DISCOVERY & DEPENDENCY AUDIT REV2

**Subsystem**: Stage 6 Final Closure, Agent Spatial Grounding & Session Continuity  
**Document State**: Discovery & Architecture Audit Rev2  
**Author**: DeepMind Advanced Agentic Coding Team  
**Date**: October 2026  

---

## 1. Executive Summary & Rev2 Audit Scope

Stage 6 of ZeroOS establishes the **Human Interaction, Spatial Presentation & Agent-Human Collaboration Subsystem**. Stages 6A through 6E defined, implemented, and froze five core daemons:

```text
Stage 6A  shelld       User Session Substrate & Workspace Layout Manager
Stage 6B  compositord  Distributed Spatial Presentation Protocol (SHM & Damage Rects)
Stage 6C  uids         Human Input Routing & Focus Authority
Stage 6D  intentd      Human Intent Interpretation & Resolution Engine
Stage 6E  observed     Human-Agent Telemetry, Interactive Feedback & Action Recorder
```

Rev1 of Stage 6F Discovery was reviewed and received a **Revision Required** verdict due to two main architectural concerns:
1. Combining agent spatial grounding (`groundd`) with an under-justified new session daemon (`sessiond`).
2. Insufficiently specifying semantic tree ownership, spatial privacy classification, agent principal authority, and handle non-migration rules.

**Rev2 Resolves All Six Required Closures**:
- **Closure A**: `sessiond` is **eliminated**. No new session daemon is created. Multi-node continuity is satisfied by existing daemons (`shelld` + `workspaced` + `fabricd` + CSDT).
- **Closure B**: Semantic UI ownership is anchored in application surfaces (`surfaced`); `groundd` acts purely as an unprivileged, read-only spatial indexing query broker.
- **Closure C**: Spatial information is classified into 4 privacy tiers; sensitive and trusted overlays omit bounding boxes (`BOUNDS_REDACTED`) to prevent geometric leaks.
- **Closure D**: Agent spatial queries require a valid caller-presented `WorkspaceAccessCap`; `intentd` does not query `groundd` directly.
- **Closure E**: Handle migration across nodes is **strictly prohibited**. Only logical state snapshots are exported, with local runtime handles reconstructed from scratch on the receiving node.
- **Closure F**: Zero Stage 4F duplication. Session recovery reuses Stage 4F fabric, identity, and CSDT semantics.

---

## 2. Closure A — Elimination of `sessiond` Daemon

### Ownership Audit of Session State

| State Domain | Authoritative Owner | Existing Infrastructure |
|---|---|---|
| Session Records & Window Layouts | `shelld` | Stage 6A `SessionRecord`, `ViewportGrid` |
| Workspace Scope & Containment | `workspaced` | Stage 4F / Stage 6A `WorkspaceDescriptor` |
| Remote Transport & Peer Discovery | `fabricd` | Stage 4F Compute Fabric |
| CSDT Security Tokens & Delegation | `fabricd` / Stage 4F | CSDT token monotonic lifecycle |
| Capability Grants & Lineage | Stage 3H Kernel | `WorkspaceAccessCap` (`0x0041`) |
| Intent & Execution Plan Recovery | `intentd` / Stage 4F | Stage 4F recovery persistence |

### Verdict on `sessiond`
Creating a dedicated `sessiond` daemon would introduce a redundant, competing state authority over session lifecycle and multi-node transport. 

**Architectural Decision**: **No `sessiond` daemon is created.**  
Multi-node session continuity is implemented purely via protocol serialization between `shelld` (logical session snapshot), `workspaced` (workspace containment), and `fabricd` (CSDT transport authentication).

---

## 3. Closure B — Semantic Tree Ownership & `groundd` Boundary

### Ownership of UI Semantics
- `compositord`: Owns surface composition and raw raster framebuffers. It has **no knowledge** of UI element semantics (buttons, text boxes, sliders).
- `surfaced`: Owns individual application surface endpoints and buffer swapchains.
- **Application Surfaces**: Applications own their internal UI element hierarchies and publish accessibility node metadata to `surfaced` during layout updates.
- **`groundd` (Spatial Grounding Broker)**: `groundd` is an unprivileged, read-only spatial indexing broker. It aggregates accessibility node metadata from `surfaced` and surface coordinate bounds from `shelld`/`compositord`. `groundd` **does not own or mutate** UI semantic data.

```text
Application Surface ──► surfaced (Accessibility Nodes) ──┐
                                                          ├──► groundd (Query Broker) ──► agentd
shelld / compositord ──► Surface Window Bounds ───────────┘      (Read-Only, Sanitized)
```

---

## 4. Closure C — Spatial Information Classification & Geometric Masking

Spatial geometry (location, width, height, surrounding element layout) can reveal sensitive information even if textual content is scrubbed (e.g., identifying a 6-digit PIN pad layout or password input location).

### 4-Tier Spatial Privacy Classification

```text
Tier 0: Public Spatial Metadata
  ├── Shell desktop controls, window frames, system badges
  └── Visible to all authorized processes

Tier 1: Workspace-Private Metadata
  ├── Standard UI buttons, labels, scrollable regions within caller's workspace
  └── Visible to agents presenting valid WorkspaceAccessCap

Tier 2: Workspace-Sensitive Metadata
  ├── Password fields, PIN dialogs, credit card inputs, confidential documents
  └── TEXT ZEROED; Bounding Box = BOUNDS_REDACTED [0, 0, 0, 0]; Node Type = NODE_REDACTED (0xFF)

Tier 3: Trusted-Only Overlay Metadata
  ├── Stage 5 authui modal confirmation prompts, Class-3 authorization dialogs
  └── COMPLETELY HIDDEN from groundd spatial queries; returns empty result set
```

### Privacy Enforcing Guarantee
> **Agents receive only spatial and semantic information permitted by the workspace's existing authority policy. Sensitive bounding boxes are completely redacted to prevent geometric inferencing.**

---

## 5. Closure D — Principal & Authority Model for Spatial Queries

### Query Flow & Authority Checks

```text
Agent (agentd) ──► SpatialQuery Request + WorkspaceAccessCap ──► groundd
                                                                     │
                                                   Capability Check & Containment
                                                                     │
                                                   ┌─────────────────┴─────────────────┐
                                                   ▼                                   ▼
                                           Valid Handle                     Invalid Handle / Redacted
                                                   │                                   │
                                      Return Tier 1 Bounding Boxes            Return PermissionDenied
```

1. **Explicit Capability Presentation**: The caller MUST present a valid `WorkspaceAccessCap` (`0x0041`) handle matching the target workspace ID in the `SpatialQuery` IPC payload.
2. **`intentd` Separation**: `intentd` does NOT query `groundd` directly. `intentd` remains the pure Intent Interpretation Authority (Stage 6D), receiving proposals compiled by `agentd` or `observed`.
3. **Cross-Workspace Isolation**: An agent in Workspace 1 cannot query spatial geometry in Workspace 2 without a valid Workspace 2 `WorkspaceAccessCap`.
4. **Remote Agent Containment**: Remote agents operating via CSDT cannot query local spatial trees unless holding a local `WorkspaceAccessCap` delegate.
5. **Focus Independence**: Queries inspect active workspace surface trees regardless of instantaneous input focus, but inactive/hidden surfaces in non-visible workspaces omit detailed geometry.

---

## 6. Closure E — Migration Safety & Ephemeral Handle Isolation

### Strict Handle Non-Migration Guarantee
The following ephemeral runtime primitives **must NEVER be transferred across physical nodes**:

$$\text{PROHIBITED FROM MIGRATION}: \begin{cases} 
\text{Kernel Capability Handles } (u32) \\
\text{IPC Socket Descriptors} \\
\text{SHM Shared Memory Pointers / Offsets} \\
\text{Hardware Device Handles} \\
\text{Network Sockets / File Descriptors} \\
\text{Kernel Process IDs (PIDs) / Thread IDs} 
\end{cases}$$

### Logical State Handoff Architecture
When migrating a session from Node A to Node B:

```text
Node A (Source)
  │  1. Serialize LogicalSessionSnapshot (Workspace IDs, Window layout ratios, Title hashes)
  │  2. Sign snapshot using CSDTToken via Stage 4F fabricd
  ▼
Node B (Destination)
  │  3. Validate CSDTToken and authenticate workspace owner via Stage 5 authui
  │  4. Reconstruct local runtime state:
  │       ├── Allocate fresh local SHM buffers via local compositord
  │       ├── Establish fresh IPC channels via local surfaced
  │       └── Derive local WorkspaceAccessCap handles via kernel Stage 3H
  ▼
Session Restored
```

---

## 7. Closure F — Stage 4F Duplication Audit

Session continuity reuses existing Stage 4F architecture completely:
- Identity & Allocation: `DistributedIdAllocator` (Stage 4F).
- Node Interconnect: `fabricd` (Stage 4F Compute Fabric).
- Delegation & Security: `CSDTToken` (Stage 4F CSDT authority).
- Execution Plan Recovery: Stage 4F `intentd` recovery persistence.

**Zero duplication of Stage 4F orchestration or workflow engines.**

---

## 8. Clear Architectural Separation of Responsibility

The Stage 6 subsystem preserves clean, non-overlapping daemon roles:

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

## 9. Proposed Machine Acceptance Gates (`6F-1` through `6F-14`)

1. **Gate 6F-1: Spatial Query ABI Alignment**  
   `SpatialNodeQuery` (64 bytes) and `SpatialNodeResponse` (128 bytes) verified via compile-time `core::mem::size_of` assertions.
2. **Gate 6F-2: Spatial Element Grounding**  
   `groundd` returns valid surface bounding boxes for Tier 1 elements.
3. **Gate 6F-3: Sensitive Element Geometric Masking**  
   Elements marked `FLAG_SENSITIVE` return `BOUNDS_REDACTED` `[0, 0, 0, 0]` and `NODE_REDACTED` (`0xFF`).
4. **Gate 6F-4: Trusted Overlay Exclusion**  
   Stage 5 `authui` overlays return 0 elements to `groundd` queries.
5. **Gate 6F-5: Workspace Containment Isolation**  
   Queries lacking a matching `WorkspaceAccessCap` return `PermissionDenied`.
6. **Gate 6F-6: Read-Only Query Mutation Rejection**  
   Write/mutation IPC opcodes sent to `groundd` return `InvalidRequest`.
7. **Gate 6F-7: Logical Session Snapshot Serialization**  
   `shelld` correctly serializes `LogicalSessionSnapshot` without raw handles.
8. **Gate 6F-8: Ephemeral Handle Non-Migration Audit**  
   Verification that zero kernel handles (`u32`) or SHM pointers are present in export payloads.
9. **Gate 6F-9: Remote CSDT Handoff Authentication**  
   Session snapshot handoff across nodes requires valid `CSDTToken` validation via `fabricd`.
10. **Gate 6F-10: 100% Offline Autonomy Verification**  
    `groundd` spatial queries execute 100% offline without remote network dependencies.
11. **Gate 6F-11: `groundd` Crash Fail-Closed Recovery**  
    Daemon crash clears query handles and fails closed to caller errors.
12. **Gate 6F-12: Zero Capability Creation Audit**  
    `groundd` holds zero capability creation rights and cannot issue Stage 3H grants.
13. **Gate 6F-13: Physical Memory Manager (PMM) Neutrality**  
    PMM frame baseline matches final frame count after Stage 6F verification.
14. **Gate 6F-14: Stage 3A–3N Kernel Preservation**  
    Kernel nucleus files under `kernel/src/stage3/` retain zero modified bytes.

---

## 10. Summary of Architectural Verdicts

| Area | Verdict |
|---|---|
| `sessiond` necessity | 🔴 **Eliminated** (Reuses `shelld` + `workspaced` + `fabricd`) |
| Semantic tree ownership | 🟢 Applications (`surfaced`); `groundd` is query broker only |
| Spatial privacy masking | 🟢 4 Tiers; Tier 2 redacted (`[0,0,0,0]`), Tier 3 hidden |
| Agent principal model | 🟢 Explicit `WorkspaceAccessCap` presentation required |
| Migration safety | 🟢 Ephemeral handles strictly non-migratable |
| Stage 4F duplication | 🟢 Zero duplication |
| Stage 3A–3N preservation | 🟢 Explicitly preserved |

---
