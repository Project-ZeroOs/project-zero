# STAGE 6F — DISCOVERY & DEPENDENCY AUDIT REV1

**Subsystem**: Stage 6 Final Subsystem Closure, Agent Spatial Grounding & Session Continuity  
**Document State**: Discovery & Architecture Audit Rev1  
**Author**: DeepMind Advanced Agentic Coding Team  
**Date**: October 2026  

---

## 1. Executive Summary & Audit Mission

Stage 6 of ZeroOS establishes the **Human Interaction, Spatial Presentation & Agent-Human Collaboration Subsystem**. Across Stages 6A through 6E, five core daemons and contracts were specified, implemented, and frozen:

```text
Stage 6A  shelld       User Session Substrate & Workspace Session Manager
Stage 6B  compositord  Distributed Spatial Presentation Protocol (SHM & Damage Rects)
Stage 6C  uids         Human Input Observation & Routing Authority
Stage 6D  intentd      Human Intent Interpretation & Resolution Engine
Stage 6E  observed     Human-Agent Telemetry, Feedback & Demonstration Synthesizer
```

### Audit Mission
The primary objective of **Stage 6F Discovery** is to perform a comprehensive architectural dependency audit to answer three critical questions:

1. **What critical capability is missing in the Stage 6 human-agent interaction loop?**  
   While 6C observes input, 6D resolves intent, 4F executes workloads, and 6E records demonstrations and routes telemetry, agents currently lack a **read-only, capability-bounded Spatial Semantic Grounding Authority (`groundd`)** to inspect rendered user interfaces (accessibility trees, spatial viewport coordinates) without bypassing `compositord` or violating privacy containment.

2. **How is multi-node spatial session state persisted and migrated across hardware boundaries?**  
   Session state (`shelld`), focus state (`uids`), active workflow bindings (`intentd`), and recording streams (`observed`) must support multi-node migration and crash recovery without leaking capability grants or allowing unauthenticated session hijacking.

3. **What is the final closure boundary for Stage 6 before transitioning to Stage 7?**  
   Stage 6F defines the explicit, unalterable boundary between **Human-Agent Interaction & Spatial Presentation (Stage 6)** and **Autonomous Enterprise Fabric & System Lifecycle (Stage 7)**.

---

## 2. Frozen Foundation Audit (Immutable Contracts)

Stage 6F strictly respects all existing, frozen architecture contracts:

```text
Stage 3A–3N
Kernel Nucleus | Capabilities (0x0001–0x0045) | IPC | Processes | Filesystem | Networking | SMP

Stage 4A–4F
Compute Fabric | workloadd | agentd | resourced | intentd (Execution Plan Compiler)

Stage 5
User Interaction Substrate | compositord | surfaced | Visual HMAC Badging | ModalLock

Stage 6A–6E
shelld (Session Substrate) | Spatial Protocol (72B Header) | uids (Input Routing) | 
intentd (Intent Resolution) | observed (Telemetry, Feedback & Action Recording)
```

---

## 3. Discovery Audit Topics & Architectural Analysis

### Topic 1: Agent Spatial Semantic Grounding (`groundd`)

#### Problem Definition
For an agent to assist a human interactively (or analyze recorded demonstrations from Stage 6E), the agent must understand **where** visual elements are located on screen and **what** semantic role they serve (e.g., "Submit button at coordinates (420, 310) in Workspace 2"). 

Currently, agents have **zero** access to:
- Raw framebuffer memory in `compositord` (protected by kernel memory isolation).
- Hardware input devices in `uids` (protected by `InputPolicyCap`).
- Workspace surface trees in `shelld` (protected by `WorkspaceAccessCap`).

If agents were granted direct framebuffer access, key privacy guarantees (scrummed sensitive text, password fields, cross-workspace isolation) would be violated.

#### Proposed Solution: `groundd` (Spatial Grounding Authority)
`groundd` is an unprivileged daemon that exposes a **read-only Spatial Semantic Tree Query IPC interface**:

```text
compositord (Surface Layout) ──┐
                               ├──► groundd ──► SpatialQuery ──► agentd / intentd
shelld (Accessibility Tree) ───┘      (Read-Only, Sanitized)
```

**Key Constraints for `groundd`**:
1. **Workspace Containment**: `groundd` only returns spatial bounding boxes for surfaces matching the caller's `WorkspaceAccessCap`.
2. **Sensitive Element Scrubbing**: Password fields, credit card inputs, and elements marked `FLAG_SENSITIVE` return zeroed text and masked bounding boxes.
3. **No Direct Input Authority**: `groundd` returns spatial coordinates only; it cannot synthesize input events or invoke UI callbacks.

---

### Topic 2: Multi-Node Session Continuity & Handoff (`sessiond` Integration)

#### Problem Definition
When a user moves between physical hardware devices (e.g., from desktop node A to laptop node B), or when a node recovers from a power event:
- Workspace visual topologies (`shelld`) must re-bind to the local `compositord`.
- Interactive feedback prompts (`observed`) must re-route to the active user focus.
- Ongoing demonstration recordings (`observed`) must suspend safely or flush sanitized traces.

#### Architectural Guarantee
Session handoff is governed by `WorkspaceAccessCap` lineage and ZeroOS security policy:
- Session migration **never** grants new kernel capabilities.
- Remote session handoffs must present a valid `CSDTToken` (Stage 4F) and pass `authui` re-authentication if Class-3 privileges are active.

---

### Topic 3: Stage 6 Subsystem Closure & Stage 7 Boundary

With the addition of `groundd` and session continuity, the Stage 6 Human-Agent Interaction Subsystem achieves **100% operational closure**:

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

---

## 4. Comprehensive Authority & Responsibility Matrix

| Subsystem / Daemon | Operational Role | Execution Authority | Capability Creation | Input Routing | Intent Interpretation |
|---|---|---|---|---|---|
| `shelld` | Session & Layout | ❌ No | ❌ No | ❌ No | ❌ No |
| `compositord` | Display Composition | ❌ No | ❌ No | ❌ No | ❌ No |
| `uids` | Input Routing | ❌ No | ❌ No | ✅ Input Only | ❌ No |
| `intentd` | Intent Resolution | ❌ No | ❌ No | ❌ No | ✅ Validated Intent |
| `observed` | Telemetry & Recording | ❌ No | ❌ No | ❌ No | ❌ No |
| `groundd` (6F) | Spatial Semantic Tree | ❌ No | ❌ No | ❌ No | ❌ No |
| `authui` (Stage 5) | Trusted Security Prompts | ❌ No | ❌ No | ❌ No | ❌ No |
| `agentd` / `workloadd` | Task Execution | ✅ Workloads | ❌ No | ❌ No | ❌ No |

---

## 5. Proposed Machine Acceptance Gates (`6F-1` through `6F-14`)

To ensure absolute quality, the Stage 6F implementation must pass 14 machine-verifiable gates:

1. **Gate 6F-1: Spatial Query ABI Alignment**  
   `SpatialNodeQuery` (64 bytes) and `SpatialNodeResponse` (128 bytes) verified via compile-time `core::mem::size_of` assertions.
2. **Gate 6F-2: Spatial Element Grounding**  
   `groundd` successfully maps surface bounding boxes and returns sanitized UI element hierarchies.
3. **Gate 6F-3: Sensitive Element Masking**  
   UI elements marked `FLAG_SENSITIVE` return masked bounding boxes and zeroed text content.
4. **Gate 6F-4: Workspace Containment Isolation**  
   Spatial queries for unowned workspaces fail with `PermissionDenied`.
5. **Gate 6F-5: Read-Only Query Bounds**  
   `groundd` IPC opcodes reject all write/mutation requests with `InvalidRequest`.
6. **Gate 6F-6: Session State Persistence & Recovery**  
   `shelld` and `groundd` state trees persist and recover correctly across daemon restarts.
7. **Gate 6F-7: Multi-Node CSDT Session Migration**  
   Remote spatial session handoff requires valid `CSDTToken` authentication.
8. **Gate 6F-8: Offline Autonomy Verification**  
   `groundd` spatial queries function 100% offline without remote network access.
9. **Gate 6F-9: Memory-Bounded Spatial Tree Accounting**  
   Spatial tree nodes are preallocated in fixed-capacity ring buffers without dynamic heap memory leaks.
10. **Gate 6F-10: `groundd` Crash Fail-Closed Recovery**  
    Daemon crashes clean up spatial state handles and fail-closed to caller error responses.
11. **Gate 6F-11: Zero Model Authority Enforcement**  
    Agent models cannot grant themselves spatial query rights or bypass workspace bounds.
12. **Gate 6F-12: Zero Capability Creation Audit**  
    `groundd` holds zero capability creation rights and cannot issue Stage 3H grants.
13. **Gate 6F-13: Physical Memory Manager (PMM) Neutrality**  
    PMM frame allocation baseline matches final frame count exactly after Stage 6F verification.
14. **Gate 6F-14: Stage 3A–3N Kernel Preservation**  
    Kernel nucleus files under `kernel/src/stage3/` retain zero modified bytes.

---

## 6. Summary of Architectural Findings

1. **Legitimate Stage 6 Boundary**: Stage 6F provides essential agent spatial grounding (`groundd`) and multi-node session continuity, closing the Stage 6 Human-Agent Interaction Subsystem.
2. **Zero Capability Escalation**: `groundd` is strictly read-only and unprivileged. It cannot bypass `uids` or execute tasks.
3. **Stage 6 Complete**: Upon Stage 6F completion, Stage 6 will be 100% frozen, paving the way for Stage 7.

---
