# Project Zero — Stage 3H Architecture Rev2
## Capability System & Kernel Authority Model

- **Status**: `READY FOR REVIEW`
- **Revision**: Rev2 (Surgical Reconciliation Pass)
- **Target Subsystem**: `kernel/src/cap/` & `kernel/src/ipc/`
- **Author**: Antigravity Architecture & Kernel Engineering Team
- **Date**: 2026-09-17
- **Base Milestone**: Stage 3G (IPC & Kernel Object Subsystem 🟢 FROZEN)

---

## 1. Executive Summary & Rev2 Architectural Reconciliation

Stage 3H establishes the native, zero-heap capability system for Project Zero. Following the architectural review of Rev1, Architecture Rev2 delivers precise, machine-level closure on the core authority mechanics:

1. **Strict Handle vs Capability Separation (Item 1 & 2)**:
   - A **`Handle`** is an opaque, process-local 32-bit token `(slot_index: u16, generation: u16)`.
   - A **`Capability`** is a kernel-authoritative record of authority represented by a unique 64-bit `capability_id`, an object reference, a rights bitmask, and a node in the Capability Derivation Tree (CDT).
   - **1-to-1 Handle-to-CDT Association**: Every occupied slot $(p, h)$ in `PROCESS_HANDLE_TABLES[p].entries[h]` is bound 1-to-1 to a unique `CapabilityNode` in `CAPABILITY_NODE_TABLE[p][h]`. There is zero handle aliasing; every valid handle addresses a distinct point of authority.
2. **Duplicate & Attenuation Semantics (Item 3)**:
   - When a handle is duplicated or attenuated, a **new distinct CDT node** is allocated with its own monotonic `capability_id`.
   - The duplicated capability is installed as a **direct child (descendant)** of the source capability in the CDT, enforcing $R(child) \subseteq R(parent)$.
3. **Exact `TRANSFER_MOVE` Semantics (Item 4)**:
   - Moving a capability transfers authority without duplicating reference counts.
   - The sender's slot is tombstoned/cleared (`occupied = false`, generation advanced).
   - A new CDT node is allocated in the receiver holding the same rights, inheriting the original parent linkage and derivation depth.
   - Total `handle_refs` on the underlying object remains strictly unchanged (+1 receiver, -1 sender = 0 delta).
4. **Exact `TRANSFER_DELEGATE` Semantics (Item 5)**:
   - Requires both `cap_rights::TRANSFER` and `cap_rights::DUPLICATE`.
   - Sender retains its capability intact.
   - A new child CDT node is allocated in the receiver, linked as a child of the sender's node. Total `handle_refs` increments (+1).
5. **Scoped Revocation & Invariant `I-CAP-Y` (Item 6 & 7)**:
   - Formally establishes **`I-CAP-Y` (Scoped Revocation)**: Revocation authority is scoped strictly to the capability's derivation subtree:
     $$\text{RevocationTarget}(C) \subseteq \text{Descendants}(C)$$
   - A capability holder with `RIGHT_REVOKE` can revoke its descendants, but **never** its ancestors, and **never** unrelated capabilities to the same object.
6. **No Authority Amplification & Invariant `I-CAP-X` (Item 8)**:
   - Formally establishes **`I-CAP-X` (No Authority Amplification)**: Derived capabilities are strictly bounded: $R(C_{child}) \subseteq R(C_{source})$ and $Obj(C_{child}) == Obj(C_{source})$.
7. **In-Flight Linearization & Pin Safety (Item 9)**:
   - Revoking a capability marks its slot revoked and invalidates future accesses, but leaves the underlying object alive while `in_flight_op_refs > 0` (`I-CHAN-4`, `I-CHAN-5`). Blocked threads wake safely and return `Err(IpcError::CapabilityRevoked)`.
8. **Unified Lifetime Accounting (Item 10)**:
   - No competing reference counters. Every active capability corresponds to exactly one `handle_refs` count in `KERNEL_OBJECT_TABLE`.
9. **Authoritative Dimension Reconciliation (Item 11)**:
   - Reconciles the process dimension: `MAX_PROCESSES = 16` is the authoritative static bound in frozen Stage 3F (`kernel/src/task/process.rs`) and Stage 3G (`PROCESS_HANDLE_TABLES`). `CAPABILITY_NODE_TABLE` dimension $[16][32]$ ($512$ nodes, 12 KiB) matches the codebase 1-to-1.

---

## 2. Design Goals

1. **Kernel-Native Authority Primitive**: Object-capability model enforced directly by the kernel boundary on bare-metal x86-64.
2. **Strict Principle of Least Privilege**: Ambient authority is prohibited. A process possesses only the specific rights explicitly granted by its capabilities.
3. **Monotonic Rights Attenuation**: Authority can only decrease through derivation, never increase.
4. **Deterministic Scoped Revocation**: Bounded descendant revocation across process boundaries without destroying underlying kernel objects.
5. **Zero-Heap Determinism**: All capability nodes, derivation links, and metadata reside in fixed `.bss` static arrays.
6. **Frozen ABI Preservation**: Zero modifications to `Process` (128 bytes) or `HandleTable` (520 bytes).

---

## 3. Non-Goals

1. **No Application-Level RBAC or ACLs**: User/group identities and file ACLs belong to user space in Stage 4+.
2. **No Ambient Privileges (`root`)**: There is no superuser.
3. **No Syscall Trampoline (Stage 3I)**: Syscall instructions (`syscall`/`sysret`) and user stack pivots belong to Stage 3I.
4. **No Dynamic Heap Allocation**: No `kmalloc`, `Box`, or dynamic lists.

---

## 4. Terminology

- **Kernel Object**: Authoritative resource descriptor in `KERNEL_OBJECT_TABLE`.
- **Handle**: Opaque 32-bit user-facing token `(slot_index: u16, generation: u16)`.
- **Capability**: Protected kernel-internal record of authority: unique ID, object reference, rights mask, and derivation lineage.
- **Capability Derivation Tree (CDT)**: The global intrusive tree tracking parent-child authority relationships across processes.
- **Attenuation**: Deriving a child capability with a strictly reduced rights mask ($R_{child} \subset R_{parent}$).
- **Revocation**: Invalidating a capability or its derivation subtree.

---

## 5. Capability Model & Architecture

```text
┌────────────────────────────────────────────────────────────────────────┐
│                   ZeroOS Capability Architecture Rev2                  │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│   Process A (Slot Pa)                      Process B (Slot Pb)         │
│   ┌───────────────────────────┐            ┌───────────────────────────┐
│   │ Handle (Slot Ha, Gen Ga)  │            │ Handle (Slot Hb, Gen Gb)  │
│   └─────────────┬─────────────┘            └─────────────┬─────────────┘
│                 │ (Process-Local Index)                  │             │
│                 ▼                                        ▼             │
│   ┌───────────────────────────┐            ┌───────────────────────────┐
│   │ HandleEntry (Pa, Ha)      │            │ HandleEntry (Pb, Hb)      │
│   │ - ObjIdx: 10, ObjGen: G0  │            │ - ObjIdx: 10, ObjGen: G0  │
│   │ - Rights: SEND|RECV|REVOKE│            │ - Rights: RECV only       │
│   └─────────────┬─────────────┘            └─────────────┬─────────────┘
│                 │ (1-to-1 Binding)                       │             │
│                 ▼                                        ▼             │
│   ┌───────────────────────────┐            ┌───────────────────────────┐
│   │ CapabilityNode (Pa, Ha)   │──delegates─► CapabilityNode (Pb, Hb)   │
│   │ - CapID: 101              │            │ - CapID: 102              │
│   │ - Depth: 0 (Root)         │            │ - Depth: 1 (Child)        │
│   │ - Parent: None            │            │ - Parent: (Pa, Ha, Ga)    │
│   └─────────────┬─────────────┘            └─────────────┬─────────────┘
│                 │                                        │             │
│                 └───────────────────┬────────────────────┘             │
│                                     │ Validates Authority              │
│                                     ▼                                  │
│                       ┌───────────────────────────┐                    │
│                       │ KERNEL_OBJECT_TABLE Slot  │                    │
│                       │ - Object: Channel #10     │                    │
│                       │ - handle_refs: 2          │                    │
│                       └───────────────────────────┘                    │
└────────────────────────────────────────────────────────────────────────┘
```

A capability in ZeroOS is defined by:
$$\mathbf{Capability} = \langle \mathbf{ID}, \mathbf{ObjectRef}, \mathbf{Rights}, \mathbf{Owner}, \mathbf{DerivationNode} \rangle$$

---

## 6. Handle vs Capability Identity & 1-to-1 Association

### Precise Distinction
1. **`Handle` (32-bit User Token)**:
   - What user space holds: `Handle(slot_index: u16, handle_generation: u16)`.
   - Process-local integer index.
2. **`HandleEntry` (Frozen Stage 3G ABI, 16 bytes)**:
   - Resides in `PROCESS_HANDLE_TABLES[pslot].entries[hslot]`.
   - Stores object index, generations, rights mask, endpoint discriminator, and occupancy flag.
3. **`CapabilityNode` (Stage 3H Authority Record, 24 bytes)**:
   - Resides in `CAPABILITY_NODE_TABLE[pslot][hslot]`.
   - Stores monotonic `capability_id`, parent linkage, derivation depth, revocation state, and child/sibling tree links.

### The 1-to-1 Binding Rule
> **Every occupied handle slot $(p, h)$ is bound 1-to-1 to a unique `CapabilityNode` $(p, h)$.**
> **There is zero handle aliasing.** A handle does not point to a shared capability node; it owns its dedicated node.
> Every valid handle represents a distinct, auditable, and revocable point of authority with its own monotonic `capability_id`.

---

## 7. Duplicate Semantics & CDT Behavior

When process $P$ duplicates or attenuates handle $H_1$ (slot $h_1$) to create handle $H_2$ (slot $h_2$):
1. **New Unique Node**: A new `CapabilityNode` is allocated at slot $(P, h_2)$ with a **new unique `capability_id`** from `NEXT_CAPABILITY_ID`.
2. **Tree Position**: $H_2$ is installed as a **direct child (descendant)** of $H_1$ in the CDT:
   - `H2.parent_pslot = P`, `H2.parent_hslot = h1`, `H2.parent_gen = H1.generation`.
   - `H2.derivation_depth = H1.derivation_depth + 1`.
   - $H_2$ is prepended to $H_1$'s `first_child` list.
3. **Rights Attenuation**: $R(H_2) = R(H_1) \ \& \ requested\_rights$.
4. **Reference Accounting**: Underlying object `handle_refs` increments by 1.
5. **Revocation Semantics**: Because $H_2$ is a child of $H_1$, revoking $H_1$'s descendants revokes $H_2$. However, $H_2$ cannot revoke $H_1$.

---

## 8. Exact `TRANSFER_MOVE` Semantics

When process A transfers handle $H_A$ at $(p_A, h_A)$ to process B via `TRANSFER_MOVE`:
1. **Sender Authority Deletion**:
   - Process A's slot $(p_A, h_A)$ is marked `occupied = false`.
   - Handle generation is incremented: `handle_generation += 1` (rejects stale handles).
   - In `CAPABILITY_NODE_TABLE[p_A][h_A]`, the node is cleared.
2. **Receiver Authority Allocation**:
   - A free slot $(p_B, h_B)$ is allocated in process B.
   - Process B receives a **fresh `CapabilityNode`** with a new monotonic `capability_id`.
   - Rights are preserved: $R(H_B) = R(H_A)$.
   - Derivation depth and parent linkage are inherited from $H_A$:
     `H_B.derivation_depth = H_A.derivation_depth`, `H_B.parent = H_A.parent`.
   - In the parent's child list, $(p_A, h_A)$ is replaced by $(p_B, h_B)$.
3. **Reference Count Invariance**:
   - Underlying object `handle_refs` remains **strictly unchanged** (+1 on receiver, -1 on sender = 0 delta).
   - Sender retains zero access to the capability.

---

## 9. Exact `TRANSFER_DELEGATE` Semantics

When process A delegates authority from handle $H_A$ at $(p_A, h_A)$ to process B:
1. **Prerequisites**: $H_A$ must possess BOTH `cap_rights::TRANSFER` and `cap_rights::DUPLICATE`.
2. **Sender Unchanged**: Process A retains $H_A$ completely intact.
3. **Receiver Child Allocation**:
   - A free slot $(p_B, h_B)$ is allocated in process B.
   - A new `CapabilityNode` is created with a new monotonic `capability_id`.
   - Rights are attenuated: $R(H_B) \subseteq R(H_A)$.
   - Linked as a **direct child** of $H_A$ in the CDT:
     `H_B.parent = (p_A, h_A, gen_A)`, `H_B.derivation_depth = H_A.derivation_depth + 1`.
4. **Reference Count Increment**:
   - Underlying object `handle_refs` increments by 1.
5. **Revocation Relationship**:
   - Process A holds full revocation authority over $H_B$ via `capability_revoke_descendants(H_A)`.

---

## 10. Scope of Revocation & Invariant `I-CAP-Y`

### Formal Invariant `I-CAP-Y` (Scoped Revocation)
> **Revocation of capability $C$ is strictly scoped to $C$'s derivation subtree:**
> $$\text{RevocationTarget}(C) \subseteq \text{Descendants}(C)$$
> **Revoking capability $C$ cannot invalidate any capability that is not a derived descendant of $C$, even if they reference the exact same underlying kernel object.**

```text
               Capability Derivation Subtree Scoping
                             [Root C0]
                                 │
                    ┌────────────┴────────────┐
                    ▼                         ▼
            [Child C1] (Proc 1)       [Child C2] (Proc 2)
                    │
            ┌───────┴───────┐
            ▼               ▼
      [Grandchild C3] [Grandchild C4]
         (Proc 1)        (Proc 3)
```

1. **Downstream Only**: If holder of $C_1$ invokes `capability_revoke_descendants(C1)`:
   - Descendants $C_3$ and $C_4$ are revoked immediately.
   - $C_1$ itself REMAINS VALID.
   - Root $C_0$ and sibling branch $C_2$ are **completely untouched**.
2. **No Upstream Revocation**: $C_1$ can **never** revoke $C_0$.
3. **No Peer Revocation**: $C_1$ can **never** revoke $C_2$, even though both point to the same object.

---

## 11. No Authority Amplification & Invariant `I-CAP-X`

### Formal Invariant `I-CAP-X` (No Authority Amplification)
> **For every capability $C$ derived from source capability $S$:**
> 1. $\mathbf{Rights}(C) \subseteq \mathbf{Rights}(S)$.
> 2. $\mathbf{Object}(C) == \mathbf{Object}(S)$ and $\mathbf{Endpoint}(C) == \mathbf{Endpoint}(S)$.
> 3. No derivation, transfer, or kernel operation can expand the rights mask beyond $S$ or alter the referenced object.

Any derivation request specifying `(requested_rights & !src.rights) != 0` is deterministically rejected with `Err(IpcError::RightsAmplificationRejected)`.

---

## 12. In-Flight Operations & Revocation Linearization

Stage 3H integrates with frozen Stage 3G invariants `I-CHAN-4`, `I-CHAN-5`, and `I-IPC-1`:

### Formal Invariant `I-CAP-10` (Revocation Pin Invariance)
> **Revoking a capability invalidates future operations immediately, but cannot invalidate an already-pinned in-flight operation or cause use-after-free.**

1. **Linearization Sequence**:
   - An IPC operation validates handle $H$, verifies `is_revoked == false`, increments `in_flight_op_refs += 1`, and registers `THREAD_IPC_STATE[tid].occupied = true`.
   - If `capability_revoke_descendants` executes concurrently under `KERNEL_OBJECT_TABLE_LOCK`:
     - The capability slot is marked `is_revoked = true` and cleared (`occupied = false`).
     - The underlying object is **NOT freed** because `in_flight_op_refs > 0`.
     - Any thread blocked in `waiters_rx` or `waiters_tx` on that endpoint is woken under `SCHEDULER.lock`.
     - The woken thread observes `is_revoked == true`, decrements `in_flight_op_refs`, and returns `Err(IpcError::CapabilityRevoked)`.
     - An operation already performing memory copy finishes its atomic copy, decrements `in_flight_op_refs`, and exits.
2. **Deterministic Reclamation**: Object reclamation occurs only after all in-flight pins, mapping refs, and handle refs reach zero.

---

## 13. Unified Object Lifetime Integration

Stage 3H **does NOT create a competing reference counter**:
- The frozen Stage 3G `object.header.handle_refs` counts the **exact number of live, active, non-revoked capabilities referencing that object across all processes**.
- $$\text{ref\_count}() = \text{handle\_refs} + \text{mapping\_refs} + \text{in\_flight\_op\_refs}$$
- Derivation, duplication, and delegation increment `handle_refs += 1`.
- Close and revocation decrement `handle_refs -= 1`.
- Single, authoritative, frozen truth.

---

## 14. Authoritative Static Dimension Reconciliation

### Reconciliation of Process Bounds
- Frozen Stage 3F `kernel/src/task/process.rs` defines:
  ```rust
  pub const MAX_PROCESSES: usize = 16;
  pub static mut PROCESS_TABLE: [ProcessSlot; MAX_PROCESSES] = ...;
  ```
- Frozen Stage 3G `kernel/src/ipc/handle.rs` defines:
  ```rust
  pub const MAX_HANDLES_PER_PROCESS: usize = 32;
  pub static mut PROCESS_HANDLE_TABLES: [HandleTable; MAX_PROCESSES] = ...;
  ```
  Verified ELF footprint: $16 \times 520 = 8,320$ bytes.
- **Stage 3H Authoritative Dimension**:
  ```rust
  pub const MAX_CAPABILITY_NODES: usize = MAX_PROCESSES * MAX_HANDLES_PER_PROCESS; // 16 * 32 = 512
  pub static mut CAPABILITY_NODE_TABLE: [[CapabilityNode; MAX_HANDLES_PER_PROCESS]; MAX_PROCESSES] = ...;
  ```
  Footprint: $512 \times 24\ \text{bytes} = \mathbf{12,288\ \text{bytes}\ (12\ \text{KiB})}$ in `.bss`.
- **Resolution**: Earlier documentation drafts mentioning "8" were historical Stage 2 notes. The actual frozen code across Stages 3F and 3G is **16 process slots**. There is zero contradiction; the capability table matches the running kernel exactly.

---

## 15. Rights Model Specification

```rust
pub mod cap_rights {
    // Generic Management Rights (Bits 8..15)
    pub const DUPLICATE: u16 = 1 << 8;  // 0x0100: Derive a child capability
    pub const TRANSFER:  u16 = 1 << 9;  // 0x0200: Transfer capability via IPC
    pub const REVOKE:    u16 = 1 << 10; // 0x0400: Revoke descendant capabilities
    pub const CLOSE:     u16 = 1 << 11; // 0x0800: Release/close this capability
    pub const INSPECT:   u16 = 1 << 12; // 0x1000: Query object status/signals
    pub const AUDIT:     u16 = 1 << 13; // 0x2000: Inspect capability derivation tree

    // Type-Specific Rights (Bits 0..7) - Channel
    pub const CHANNEL_SEND:    u16 = 1 << 0; // 0x0001: Transmit message to ring buffer
    pub const CHANNEL_RECEIVE: u16 = 1 << 1; // 0x0002: Receive message from ring buffer

    // Type-Specific Rights (Bits 0..7) - ShmObject
    pub const SHM_MAP_READ:  u16 = 1 << 0; // 0x0001: Map with PAGE_NX (read-only)
    pub const SHM_MAP_WRITE: u16 = 1 << 1; // 0x0002: Map with PAGE_WRITABLE
    pub const SHM_UNMAP:     u16 = 1 << 2; // 0x0004: Remove mapping from active space
}
```

---

## 16. Capability Derivation Tree (CDT) Node Layout

```rust
#[repr(C)]
pub struct CapabilityNode {
    pub capability_id: u64,     // Offset 0x00..0x08: Monotonic unique 64-bit ID
    pub parent_pslot: u8,       // Offset 0x08: Parent process slot (0xFF if root)
    pub parent_hslot: u8,       // Offset 0x09: Parent handle slot
    pub parent_gen: u16,        // Offset 0x0A..0x0C: Generation of parent when derived
    pub derivation_depth: u8,   // Offset 0x0C: Derivation depth (0 = root, max 8)
    pub is_revoked: bool,       // Offset 0x0D: Explicit revocation latch
    pub flags: u16,             // Offset 0x0E..0x10: Delegation flags (DELEGATED, IMMUTABLE)
    pub first_child_pslot: u8,  // Offset 0x10: First child process slot (0xFF if none)
    pub first_child_hslot: u8,  // Offset 0x11: First child handle slot
    pub next_sibling_pslot: u8, // Offset 0x12: Sibling process slot (0xFF if none)
    pub next_sibling_hslot: u8, // Offset 0x13: Sibling handle slot
    pub _reserved: [u8; 4],     // Offset 0x14..0x18: Explicit padding for 8-byte alignment
}

const _: () = assert!(core::mem::size_of::<CapabilityNode>() == 24);
const _: () = assert!(core::mem::align_of::<CapabilityNode>() == 8);
```

---

## 17. Monotonic Lock Hierarchy

$$\mathbf{KERNEL\_OBJECT\_TABLE\_LOCK} \prec \mathbf{SCHEDULER.lock} \prec \mathbf{CPU\ (IF=0)}$$

- `CAPABILITY_NODE_TABLE` mutations occur strictly under `KERNEL_OBJECT_TABLE_LOCK` with interrupts disabled (`IF = 0`).
- Waking blocked waiters during revocation acquires nested `SCHEDULER.lock`, releasing it before releasing `KERNEL_OBJECT_TABLE_LOCK`.
- No locks are held across thread context switches.

---

## 18. Process Teardown Integration

Integrated into Step 2 of the frozen Stage 3F Three-Step Teardown:
```text
Step 1: SCHEDULER.lock held
  - Sibling threads cancelled -> Zombie.
  - Sibling threads unlinked from IPC wait queues.
Step 2: KERNEL_OBJECT_TABLE_LOCK held (SCHEDULER.lock released)
  - Clear in-flight pins for cancelled threads in THREAD_IPC_STATE.
  - Sweep and unmap SHM mappings in SHM_MAPPING_TABLE.
  - For each occupied capability slot h in terminating process:
      - Call capability_revoke_descendants(h) (revokes all delegated children).
      - Decrement underlying object handle_refs.
      - Increment handle_generation, clear slot.
      - Call check_and_reclaim_locked(obj_idx).
Step 3: SCHEDULER.lock held
  - Switch address space and terminal context switch.
```

---

## 19. Complete Invariant Catalog

- **`I-CAP-1` (Unforgeability)**: Capabilities cannot be manufactured by user space. A handle is valid if and only if `slot < 32`, `occupied == true`, `slot.generation == handle.generation`, and `node.is_revoked == false`.
- **`I-CAP-2` (Strict Monotonicity)**: For any derivation $C_{child} \leftarrow C_{parent}$, $\mathbf{Rights}(C_{child}) \subseteq \mathbf{Rights}(C_{parent})$.
- **`I-CAP-3` (Ownership Confinement)**: A capability in slot $(P, H)$ can only be authorized by a thread belonging to process slot $P$.
- **`I-CAP-4` (Bounded Derivation Depth)**: Derivation depth cannot exceed `MAX_DERIVATION_DEPTH = 8`.
- **`I-CAP-5` (Lifetime Exactness)**: $\text{handle\_refs}$ on `KernelObject` equals the exact count of active, non-revoked capabilities referencing that object across all processes.
- **`I-CAP-6` (Descendant Revocation Completeness)**: `capability_revoke_descendants(C)` invalidates all capabilities in the derivation subtree of $C$. No descendant can execute further operations.
- **`I-CAP-7` (In-Flight Pin Invariance)**: Revoking a capability holding `in_flight_op_refs > 0` does not free the underlying object until all in-flight pins reach 0.
- **`I-CAP-8` (Transfer Atomicity)**: Capability transfer via IPC is atomic. If receiver table is full, the operation rolls back without dropping authority.
- **`I-CAP-9` (Tree Linkage Integrity)**: Every child node has a valid parent link or is a root node. Sibling lists are null-terminated and acyclic.
- **`I-CAP-10` (Scrub on Process Allocation)**: Reallocating a process slot scrubs all 32 capability slots, increments generations, and resets CDT nodes.
- **`I-CAP-11` (Lock Hierarchy Preservation)**: $\text{KERNEL\_OBJECT\_TABLE\_LOCK} \prec \text{SCHEDULER.lock} \prec \text{CPU (IF=0)}$.
- **`I-CAP-12` (Zero-Heap Guarantee)**: All capability structures reside in `.bss`. Zero dynamic memory allocation.
- **`I-CAP-X` (No Authority Amplification)**: For every capability $C$ derived from source $S$, $\mathbf{Rights}(C) \subseteq \mathbf{Rights}(S)$ and $\mathbf{Object}(C) == \mathbf{Object}(S)$.
- **`I-CAP-Y` (Scoped Revocation)**: Revocation of capability $C$ may invalidate $C$ and capabilities in $C$'s derivation subtree, but cannot invalidate an unrelated capability to the same underlying object.

---

## 20. Threat Model & Security Analysis

| Threat / Attack Scenario | Security Impact | Violated Principle | Enforcement Mechanism | Verification Test |
| :--- | :--- | :--- | :--- | :--- |
| **Forged Handle** | Unauthorized object access | Capability Unforgeability | Opaque 32-bit handle validated against generation counter | Test 3H-D |
| **Stale Handle Reuse** | Accessing wrong object | ABA Protection | Generation incremented on close; slot marked `occupied = false` | Test 3H-E |
| **Rights Escalation** | Privilege escalation | Rights Monotonicity | `(requested & !src.rights) != 0` rejected with `RightsAmplificationRejected` | Test 3H-I |
| **Unauthorized Duplication**| Proliferation of access | Confinement | Bitwise check for `cap_rights::DUPLICATE` before derivation | Test 3H-G |
| **Unauthorized Transfer** | Capability exfiltration | Confinement | Bitwise check for `cap_rights::TRANSFER` in `channel_send` | Test 3H-N |
| **Cross-Process Handle Theft**| Isolation bypass | Process Confinement | Handle table resolved strictly via caller's authoritative `process_slot_index` | Test 3H-AC |
| **Use-After-Revocation** | Revocation bypass | Revocation Correctness | `is_revoked` check under lock rejects call with `Err(CapabilityRevoked)` | Test 3H-Q |
| **Out-of-Scope Revocation** | Denial of service on peers | Scoped Revocation (`I-CAP-Y`)| Revocation algorithm restricted to traversing descendants in CDT | Test 3H-R2 |
| **In-Flight Revocation Race**| Memory corruption / UAF | Linearization Safety | `in_flight_op_refs` pin defers object reclamation; wakeups abort safely | Test 3H-U |
| **Process Slot Reuse Hazard**| Privilege inheritance | Lifecycle Isolation | `create_process()` scrubs all 32 capability slots and resets generations | Test 3H-Y |

---

## 21. Deterministic Failure Codes

```rust
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    // Frozen Stage 3G Codes (-1..-16)
    InvalidHandle               = -1,
    BadHandleGeneration         = -2,
    PermissionDenied            = -3,
    ObjectTableFull             = -4,
    HandleTableFull             = -5,
    EndpointClosed              = -6,
    PeerClosed                  = -7,
    WouldBlock                  = -8,
    InvalidMessage              = -9,
    InvalidProcess              = -10,
    InvalidAddress              = -11,
    MappingTableFull            = -12,
    VmmError                    = -13,
    PmmError                    = -14,
    ObjectIdExhausted           = -15,
    ConcurrentOperation         = -16,

    // Stage 3H Capability Specific Codes (-17..-22)
    CapabilityRevoked           = -17, // Handle was explicitly revoked
    RightsAmplificationRejected = -18, // Requested rights exceed source capability rights
    MaxDerivationDepthExceeded  = -19, // Derivation tree depth exceeded limit (8)
    NotCapabilityOwner          = -20, // Caller process does not own this capability
    CapabilityNotTransferable   = -21, // Capability lacks RIGHT_TRANSFER
    CapabilityNotDuplicable     = -22, // Capability lacks RIGHT_DUPLICATE
}
```

---

## 22. Machine-Level Verification Plan (38 Tests: 3H-A through 3H-AH)

- **Test 3H-A (Capability Creation)**: Channel creation allocates capabilities in $H_0, H_1$ with `capability_id > 0`, `depth == 0`.
- **Test 3H-B (Valid Lookup)**: Handle validation returns correct object pointer and rights under lock.
- **Test 3H-C (Invalid Handle Index)**: Present `Handle(slot=63)`; assert `Err(InvalidHandle)`.
- **Test 3H-D (Forged Generation Rejection)**: Present `Handle(slot=0, gen=999)`; assert `Err(BadHandleGeneration)`.
- **Test 3H-E (Stale Handle Rejection)**: Close capability; reuse handle; assert `Err(BadHandleGeneration)`.
- **Test 3H-F (Operation Allowed)**: Send on channel with `RIGHT_SEND`; assert success.
- **Test 3H-G (Operation Denied)**: Send on channel lacking `RIGHT_SEND`; assert `Err(PermissionDenied)`.
- **Test 3H-H (Rights Attenuation)**: Derive child capability with `RIGHT_SEND` only; assert child has only `RIGHT_SEND`.
- **Test 3H-I (No Rights Amplification)**: Attempt to derive `RIGHT_RECEIVE` from child holding only `RIGHT_SEND`; assert `Err(RightsAmplificationRejected)`.
- **Test 3H-J (Duplicate Rights Invariance)**: Duplicate capability with full rights; assert child matches parent rights.
- **Test 3H-K (Derivation Tree Linkage)**: Derive $C_1 \to C_2 \to C_3$; assert `parent`, `first_child`, `next_sibling` links.
- **Test 3H-L (Max Derivation Depth)**: Derive chain up to depth 8; attempt 9th derivation; assert `Err(MaxDerivationDepthExceeded)`.
- **Test 3H-M (Sibling List Ordering)**: Derive 3 children from same parent; assert sibling traversal visits all 3.
- **Test 3H-N (Transfer Move)**: Process A sends capability to Process B via `TRANSFER_MOVE`; assert A loses handle, B acquires handle with unchanged `handle_refs`.
- **Test 3H-O (Transfer Delegate)**: Process A sends capability to Process B via `TRANSFER_DELEGATE`; assert A retains handle, B holds child in CDT.
- **Test 3H-P (Transfer Lacking Rights)**: Attempt transfer on capability lacking `cap_rights::TRANSFER`; assert `Err(CapabilityNotTransferable)`.
- **Test 3H-Q (Receiver Table Full)**: Fill Process B's handle table; attempt transfer; assert `Err(HandleTableFull)` and message unconsumed.
- **Test 3H-R (Descendant Revocation)**: Process A delegates to B and C; A calls `capability_revoke_descendants`; assert B and C handles return `Err(CapabilityRevoked)`, A remains valid.
- **Test 3H-R2 (Scoped Revocation Isolation)**: Process A holds $C_1$ and $C_{unrelated}$ pointing to same channel; revoking $C_1$'s descendants leaves $C_{unrelated}$ fully operational.
- **Test 3H-S (Revocation Lacking Rights)**: Attempt revocation on capability lacking `cap_rights::REVOKE`; assert `Err(PermissionDenied)`.
- **Test 3H-T (Revocation of Revoked Capability)**: Revoking an already revoked capability is a deterministic no-op/error.
- **Test 3H-U (In-Flight Pin Revocation Safety)**: Process B blocks receiving; Process A revokes B's capability; assert B wakes with `Err(CapabilityRevoked)`, pin drops to 0, zero corruption.
- **Test 3H-V (Cascading Multi-Level Revocation)**: Revoke root of 3-level tree ($C_0 \to C_1 \to C_2$); assert all descendants across all processes revoked.
- **Test 3H-W (Process Exit Cleans Capabilities)**: Process with 10 capabilities exits; assert all 10 slots freed, object `handle_refs` decremented.
- **Test 3H-X (Process Exit Revokes Delegated Children)**: Process A exits; assert all capabilities delegated by A to Process B are revoked cleanly.
- **Test 3H-Y (Process Slot Reuse Isolation)**: Populate capabilities in slot $P$; terminate process; reallocate slot $P$; assert all 32 capability slots clear with advanced generations.
- **Test 3H-Z (SHM Read-Only Mapping)**: Map SHM with capability holding `MAP_READ`; assert read succeeds and `WRITABLE=0`.
- **Test 3H-AA (SHM Write Denial)**: Attempt to map SHM as writable using capability lacking `MAP_WRITE`; assert `Err(PermissionDenied)`.
- **Test 3H-AB (SHM Revocation Unmapping)**: Revoke active SHM capability; assert mapping unmapped, TLB invalidated, frames pinned until unmap commits.
- **Test 3H-AC (Cross-Process Isolation)**: Process A attempts to access Process B's capability handle; assert `Err(InvalidHandle)`.
- **Test 3H-AD (Object Reclamation After Final Capability)**: Close all capabilities to object; assert object reclaims cleanly.
- **Test 3H-AE (Monotonic ID Atomic Advance)**: Verify `capability_id` advances monotonically without collisions.
- **Test 3H-AF (Lock Hierarchy Compliance)**: Verify lock order `KERNEL_OBJECT_TABLE_LOCK` $\prec$ `SCHEDULER.lock` under debug flags.
- **Test 3H-AG (PMM Frame Neutrality)**: Assert `pmm.free_frame_count()` before and after verification suite matches exactly (0 frame leak).
- **Test 3H-AH (MOVE Audit Trail)**: Process A moves capability to B; assert A's slot reflects moved state while B holds new unique `capability_id`.

---

## 23. Stage 3I Syscall & Ring 3 Handoff

Stage 3I will implement:
1. `syscall`/`sysret` dispatchers translating user registers into kernel capability calls.
2. User Handle validation: Every syscall taking a resource argument takes a 32-bit `Handle`, validating it through `cap::validate_capability_locked()`.
3. Ring 3 Privilege Boundary: User space cannot inspect kernel page tables or `CAPABILITY_NODE_TABLE`.

---

## 24. Final Architecture Review Status

```text
===================================================================
Project Zero — Stage 3H Architecture Rev2: Capability System
Status: READY FOR REVIEW
===================================================================
All Stages 3A–3G invariants preserved.
Zero modifications to frozen Process (128 B) or HandleTable (520 B) ABIs.
Zero dynamic heap allocation (12 KiB static .bss footprint).
Strict 1-to-1 handle-to-node association with zero aliasing.
Formal invariants I-CAP-X (No Authority Amplification) and
I-CAP-Y (Scoped Revocation) established.
===================================================================
```
