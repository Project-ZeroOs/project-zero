# ADR-0028: Agent Runtime Subsystem and Agent Security Model (Rev 2)

## Context & Problem Statement

In traditional AI application stacks, an "Agent" is typically built as a user-space application SDK, a chatbot loop, or a framework wrapping LLM API calls with unstructured Python functions. These implementations lack OS-level security boundaries, capability scoping, deterministic failure containment, resource lease accounting, and persistent context graph integration.

Conversely, embedding an AI model directly into an operating system kernel violates memory safety, subsystem isolation, and microkernel simplicity principles.

ZeroOS requires a first-class **Agent Runtime Subsystem (Stage 4E)** that establishes Agents as OS-managed control entities operating above the frozen Stage 3A–3N kernel nucleus, Stage 4A system services, Stage 4B Resource Graph (`resourced`), Stage 4C Workload Orchestration (`workloadd`), and Stage 4D Workspace subsystem (`workspaced`), without duplicating execution scheduling, resource accounting, or context graph management.

---

## Decision Drivers

1. **Kernel Capability Authority**: All capability attenuation ($C_{agent} \subseteq C_{ws}$), derivation, validation, and revocation must be enforced strictly by the Stage 3H kernel capability system. `agentd` holds process-local handle indices only.
2. **Untrusted Model Cognition**: Model output is data, not authority ($\text{Model Output} \neq \text{Authority}$). AI inference engines exist in user space as resource-backed services.
3. **Non-Duplication**: `workloadd` remains the sole execution authority for Workloads/Processes, `resourced` the sole resource accounting manager, and `workspaced` the sole Context Graph manager.
4. **Generic Human Authorization Boundary**: High-impact actions trigger mandatory out-of-band human authorization via a trusted authorization path without introducing ad-hoc magic capability primitives.
5. **Separated State Machines**: Agent administrative lifecycle (`Creating` $\rightarrow$ `Active` $\rightarrow$ `Stopping` $\rightarrow$ `Terminated` $\rightarrow$ `Reclaimed`) is separated from runtime execution status (`Idle`, `Waiting`, `Executing`, `Suspended`, `Recovering`).
6. **Durable vs. Ephemeral State**: Capability handles (`u32`) are NEVER persisted to disk. Recovery re-derives handles from the kernel.

---

## Decision Statement

We decide to adopt the following architectural architecture for Stage 4E:

1. **Agent Definition**: An Agent is defined as a persistent computational control entity identified by a 64-bit `AgentId` (allocated via 4B `DistributedIdAllocator`), belonging strictly to one `WorkspaceId`, with state managed by `agentd`.
2. **Layered Execution Hierarchy**:
   $$\text{Workspace} \longrightarrow \text{Agent} \longrightarrow \text{Workload(s)} \longrightarrow \text{Task(s)} \longrightarrow \text{Process(es)}$$
3. **Execution Delegation**: `agentd` does not schedule processes or manage threads. It formulates goals into Workloads executed via `workloadd` IPC (`OP_WORKLOAD_CREATE`).
4. **Kernel Capability Attenuation**: $C_{agent}$ is derived from $C_{ws}$ via kernel `sys_cap_derive`. Revoking $C_{ws}$ invalidates all child Agent capabilities.
5. **Cognition Contract**: Model output is untrusted data. Model inference occurs in user space via standard stream-oriented IPC channels connected to inference provider services backed by `resourced` GPU leases.
6. **Human Authorization Boundary**: High-impact actions transition the Agent runtime state to `Suspended` until cleared by an out-of-band trusted authorization service.
7. **IPC Boundary**: `libzero/src/agent.rs` defines the low-level 2048-byte fixed IPC message format and opcodes for `agentd`.

---

## Primary Security Invariants

1. **`I-AGENT-KERNEL-AUTHORITY`**: Capability attenuation, derivation, validation, and revocation are enforced strictly by the Stage 3H kernel capability system.
2. **`I-AGENT-MODEL-NO-AUTHORITY`**: Model output is untrusted data and carries zero security authority.
3. **`I-AGENT-MODEL-NO-CAPABILITY-AMPLIFICATION`**: Model output cannot increase capability rights or widen $C_{agent}$.
4. **`I-AGENT-MODEL-NO-HUMAN-AUTHORIZATION`**: Model output cannot satisfy, grant, or bypass human authorization policies.
5. **`I-AGENT-ID-UNIQUE`**: Globally unique 64-bit `DistributedId` allocated via `resourced`.
6. **`I-AGENT-NO-CAP-AMPLIFICATION`**: $C_{agent} \subseteq C_{ws}$ strictly enforced by Stage 3 kernel capability handles.
7. **`I-AGENT-WORKSPACE-CONTAINMENT`**: Agent is strictly bound to its assigned `WorkspaceId`.
8. **`I-AGENT-WORKLOAD-BOUNDARY`**: Execution requests route strictly through `workloadd`.
9. **`I-AGENT-NO-RESOURCE-BYPASS`**: Resource demands pass inside Workload specs to `resourced`.
10. **`I-AGENT-NO-AMBIENT-AUTHORITY`**: All IPC calls require valid capability handles.
11. **`I-AGENT-DELEGATION-ATTENUATION`**: Child Agent capability $C_{child} \subseteq C_{parent} \subseteq C_{ws}$.
12. **`I-AGENT-PERSISTENCE-CONSISTENCY`**: Atomic write-fsync-rename transaction on ZeroFS `agent.meta`. Capability handles are never persisted.
13. **`I-AGENT-EVENT-AUTHENTICITY`**: Event perception routing validates producer signatures via `brokerd`.

---

## Consequences

### Positive
- Establishes a rigorous, capability-bounded Agent primitive natively supported by the OS.
- Enforces strict kernel capability derivation while keeping AI cognition isolated in user space.
- Preserves all frozen Stage 3 and Stage 4A–4D code and contracts without modification.
- Separates Agent lifecycle state from runtime execution status.

### Negative
- Requires multi-step IPC translation for goal dispatching (`Client -> agentd -> workloadd -> resourced -> Kernel`).
- Bounds Agent operation to single-Workspace membership.

---

## Compliance & Verification

- **Architecture Specification**: `docs/design/STAGE4E-ARCHITECTURE-REV2.md`
- **Stage 3–4D Code Integrity**: 0 modifications to kernel or frozen Stage 4 daemons.
- **Status**: `APPROVED — FROZEN`
