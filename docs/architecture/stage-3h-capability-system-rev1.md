# Project Zero — Stage 3H Architecture Rev1
## Capability System & Kernel Authority Model

- **Status**: `READY FOR REVIEW`
- **Target Subsystem**: `kernel/src/cap/` & `kernel/src/ipc/`
- **Author**: Antigravity Architecture & Kernel Engineering Team
- **Date**: 2026-09-17
- **Base Milestone**: Stage 3G (IPC & Kernel Object Subsystem 🟢 FROZEN)

---

## 1. Executive Summary

Stage 3G established the minimal, zero-heap communication substrate (bidirectional `Channel` pairs and page-backed `ShmObject` shared memory) under an authoritative 256-slot `KERNEL_OBJECT_TABLE` and 16 external `PROCESS_HANDLE_TABLES`. While Stage 3G provides basic generation-checked handles and rights masks, possession of a handle is currently coupled to direct process slot allocation, and the kernel lacks formal primitives for:
1. **Delegation**: Deriving child authority from parent authority.
2. **Monotonic Rights Attenuation**: Guaranteeing that derived authority is strictly a subset of source authority ($R_{derived} \subseteq R_{source}$).
3. **Revocation**: Invalidating delegated authority across process boundaries without destroying the underlying kernel object.
4. **Lineage Tracking**: Maintaining an auditable derivation tree across zero-heap static tables.

Stage 3H designs the **native ZeroOS capability system**. In ZeroOS:
> **"A capability is authority, not merely a reference."**
> **"An agent, application, or process can only act within the authority explicitly granted to it."**

Stage 3H introduces an external, static **Capability Derivation Tree (CDT)** (`CAPABILITY_NODE_TABLE`) mapped 1-to-1 to the frozen Stage 3G handle slots ($16 \times 32 = 512$ nodes occupying exactly **12 KiB** in `.bss`). It guarantees:
- **Frozen Stage 3F ABI Preservation**: `Process` descriptor remains exactly 128 bytes.
- **Frozen Stage 3G Handle ABI Preservation**: `HandleTable` remains exactly 520 bytes, `HandleEntry` remains exactly 16 bytes.
- **Strict Monotonic Derivation**: No authority amplification is mathematically possible.
- **Bounded Descendant Revocation**: Revoking a capability cascades down its derivation subtree in bounded constant-time $O(k)$ ($k \le 512$).
- **Linearized In-Flight Safety (`I-CHAN-5` Integration)**: Revocation respects in-flight operation pins without use-after-free.
- **Zero Dynamic Allocation**: 100% static, deterministic memory layout.

---

## 2. Design Goals

1. **Kernel-Native Authority Primitive**: A minimal, robust object-capability model enforced directly by the kernel boundary.
2. **Strict Principle of Least Privilege**: Ambient authority is prohibited. Processes possess only the specific rights explicitly granted by their held capabilities.
3. **Monotonic Rights Attenuation**: A capability may be duplicated or delegated with equal or fewer rights, never greater rights.
4. **Deterministic Revocation**: The holder of a capability with `RIGHT_REVOKE` can revoke all descendant capabilities derived from it across all processes in bounded time.
5. **Zero-Heap Determinism**: All capability nodes, derivation links, and metadata reside in fixed `.bss` static arrays.
6. **Backward Compatibility**: Fully preserves frozen Stages 3A–3G without reopening code or modifying data structures.
7. **Future-Proof Extensibility**: Expresses authority over channels, shared memory, processes, threads, synchronization primitives, devices, filesystems, network endpoints, workspaces, agents, and Compute Fabric accelerators.

---

## 3. Non-Goals

1. **No Application-Level RBAC or ACLs**: User/group identities, access control lists (DACLs), and role-based access frameworks belong to user space services in Stage 4+.
2. **No Ambient Privileges or Superuser (`root`)**: There is no "root" or superuser in ZeroOS. All authority derives from the initial bootstrap capabilities granted to PID 0.
3. **No User-Mode Syscall Trampoline (Stage 3I)**: Syscall instructions (`syscall`/`sysret`), Ring 3 GDT/TSS configuration, and user stack pivots belong to Stage 3I.
4. **No Hardware Capability Assistance**: CHERI-style fat pointers and tagged hardware memory are not assumed; the design is optimized for standard x86-64 long mode.
5. **No Dynamic Capability Trees**: No heap-allocated linked lists. The derivation graph is implemented as an intrusive index-linked tree in static memory.

---

## 4. Terminology

| Term | Formal Definition in ZeroOS |
| :--- | :--- |
| **Kernel Object** | An authoritative resource descriptor residing in `KERNEL_OBJECT_TABLE` (e.g., `Channel`, `ShmObject`, `Process`, `Event`). |
| **Handle** | An opaque 32-bit user-facing token `(slot_index: u16, generation: u16)` used by a process to address a specific capability slot. |
| **Capability** | The protected combination of an authorized reference to a Kernel Object, a specific **Rights Bitmask**, and derivation metadata. |
| **Capability Slot** | The physical entry in `PROCESS_HANDLE_TABLES` and its corresponding `CAPABILITY_NODE_TABLE` entry. |
| **Rights Mask** | A 16-bit bitmask defining the permitted operations on the referenced Kernel Object. |
| **Attenuation** | The process of creating a new capability from an existing one with a strictly reduced rights mask ($R_{child} \subset R_{parent}$). |
| **Delegation** | Transferring or duplicating a capability to another process. |
| **Revocation** | Invalidating a capability or its derivation subtree so subsequent accesses are rejected. |
| **In-Flight Pin** | An active operation reference (`in_flight_op_refs > 0`) preventing object destruction during concurrent execution. |

---

## 5. Capability Model

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        ZeroOS Capability Model                         │
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│   Process A (Slot Pa)                      Process B (Slot Pb)         │
│   ┌───────────────────────────┐            ┌───────────────────────────┐
│   │ Handle (Slot Ha, Gen Ga)  │            │ Handle (Slot Hb, Gen Gb)  │
│   └─────────────┬─────────────┘            └─────────────┬─────────────┘
│                 │ (Process-Local Index)                  │             │
│                 ▼                                        ▼             │
│   ┌───────────────────────────┐            ┌───────────────────────────┐
│   │ Capability Node (Pa, Ha)  │──delegates─► Capability Node (Pb, Hb)  │
│   │ - ID: 101                 │            │ - ID: 102                 │
│   │ - Rights: SEND|RECV|REVOKE│            │ - Rights: RECV only       │
│   │ - Depth: 0 (Root)         │            │ - Depth: 1 (Child)        │
│   └─────────────┬─────────────┘            └─────────────┬─────────────┘
│                 │                                        │             │
│                 └───────────────────┬────────────────────┘             │
│                                     │ Validates & Bounds               │
│                                     ▼                                  │
│                       ┌───────────────────────────┐                    │
│                       │ KERNEL_OBJECT_TABLE Slot  │                    │
│                       │ - Object: Channel #3      │                    │
│                       │ - Generation: G_obj       │                    │
│                       │ - Ref Count: 2            │                    │
│                       └───────────────────────────┘                    │
└────────────────────────────────────────────────────────────────────────┘
```

A capability in ZeroOS is defined by the tuple:
$$\mathbf{Capability} = \langle \mathbf{ID}, \mathbf{ObjectRef}, \mathbf{Rights}, \mathbf{Owner}, \mathbf{DerivationNode} \rangle$$

1. **Identity (`capability_id`)**: A unique, monotonic 64-bit identifier allocated from `NEXT_CAPABILITY_ID: AtomicU64`. Never reused across kernel history.
2. **Referenced Object (`object_index`, `object_generation`)**: Identifies the target slot in `KERNEL_OBJECT_TABLE` and verifies its generation.
3. **Rights Mask (`rights: u16`)**: Defines permitted operations.
4. **Owner (`process_slot`)**: Binds the capability to a specific process slot in `PROCESS_TABLE`.
5. **Derivation Node (`CapabilityNode`)**: Tracks parent linkage, first-child linkage, and sibling linkage to form the global Capability Derivation Tree (CDT).

---

## 6. Handle vs Capability Relationship

A critical architectural distinction must be maintained:

$$\mathbf{Handle} \neq \mathbf{Capability}$$

1. **`Handle` (32-bit Token)**:
   - What user space holds: `Handle(slot_index: u16, handle_generation: u16)`.
   - Process-local integer token. Does not contain raw rights or object pointers.
   - Forged or fabricated integers are detected in $O(1)$ by verifying slot bounds and generation match.
2. **`HandleEntry` (Frozen Stage 3G ABI, 16 bytes)**:
   - Resides in `PROCESS_HANDLE_TABLES[p_slot].entries[h_slot]`.
   - Contains: `object_index: u16`, `handle_generation: u16`, `object_generation: u16`, `rights: u16`, `endpoint: u8`, `occupied: bool`.
3. **`CapabilityNode` (Stage 3H Subsystem, 24 bytes)**:
   - Resides in external static array `CAPABILITY_NODE_TABLE[p_slot][h_slot]`.
   - Contains lineage, delegation, and revocation metadata.
   - Indexed 1-to-1 with `HandleEntry`.

```text
User Space:       Handle(slot=3, gen=5)
                      │
Kernel Boundary:      ▼
PROCESS_HANDLE_TABLES[p_slot].entries[3]  <── 1-to-1 ──>  CAPABILITY_NODE_TABLE[p_slot][3]
  ├── object_index: 12                                      ├── capability_id: 88412
  ├── handle_generation: 5                                  ├── parent: (p_slot=0, h_slot=1)
  ├── rights: SEND | TRANSFER                               ├── derivation_depth: 1
  └── occupied: true                                        └── is_revoked: false
```

### Architectural Contract
- **No handle can exist without authority**: If `occupied == false` or `is_revoked == true`, handle validation fails immediately with `Err(IpcError::InvalidHandle)` or `Err(IpcError::CapabilityRevoked)`.
- **Multiple capabilities can reference the same object**: Process A may hold a Read-Write capability to `Channel #1` while Process B holds a Read-Only capability to `Channel #1`.
- **Zero ABI Contradiction**: `Process` stays 128 bytes; `HandleTable` stays 520 bytes.

---

## 7. Kernel Object Association & Unified Lifetime

Stage 3G established the frozen reference accounting invariant (`I-OBJ-3`):
$$\text{ref\_count}() = \text{handle\_refs} + \text{mapping\_refs} + \text{in\_flight\_op\_refs}$$

Stage 3H **does NOT create a competing reference counter**. 
- Because each capability is authoritatively anchored by a `HandleEntry` slot, **every valid capability corresponds to exactly one `handle_refs` count** on the underlying `KernelObjectSlot`.
- When a capability is derived/duplicated, `object.header.handle_refs += 1`.
- When a capability is closed or revoked, `object.header.handle_refs -= 1`.
- If an object has active capabilities held by live processes, `handle_refs > 0`, preventing object reclamation (`I-CAP-5`).

---

## 8. Rights Model

The rights mask is encoded as a strongly-typed 16-bit bitfield (`CapRights`), divided into **Generic Rights** (high byte) and **Type-Specific Rights** (low byte):

```text
 15  14  13  12  11  10   9   8   7   6   5   4   3   2   1   0
┌───┬───┬───┬───┬───┬───┬───┬───┼───┬───┬───┬───┬───┬───┬───┬───┐
│   Generic Management Rights   │      Type-Specific Rights     │
└───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┴───┘
```

### Generic Rights (Bits 8..15)

```rust
pub mod cap_rights {
    // Generic Management Rights
    pub const DUPLICATE: u16 = 1 << 8;  // 0x0100: Derive/duplicate a child capability
    pub const TRANSFER:  u16 = 1 << 9;  // 0x0200: Transfer capability via IPC channel
    pub const REVOKE:    u16 = 1 << 10; // 0x0400: Revoke descendant capabilities
    pub const CLOSE:     u16 = 1 << 11; // 0x0800: Release/close this capability
    pub const INSPECT:   u16 = 1 << 12; // 0x1000: Query object status/signals
    pub const AUDIT:     u16 = 1 << 13; // 0x2000: Inspect capability derivation tree
}
```

### Type-Specific Rights (Bits 0..7)

#### 1. `Channel` (Control Plane)
```rust
pub mod channel_rights {
    pub const SEND:    u16 = 1 << 0; // 0x0001: Transmit message to ring buffer
    pub const RECEIVE: u16 = 1 << 1; // 0x0002: Receive message from ring buffer
}
```

#### 2. `ShmObject` (Data Plane)
```rust
pub mod shm_rights {
    pub const MAP_READ:  u16 = 1 << 0; // 0x0001: Map with PAGE_USER | PAGE_PRESENT | PAGE_NX
    pub const MAP_WRITE: u16 = 1 << 1; // 0x0002: Map with PAGE_WRITABLE (requires MAP_READ)
    pub const UNMAP:     u16 = 1 << 2; // 0x0004: Remove mapping from active address space
}
```

#### 3. Future Object Rights (Architectural Extensibility)
- **`Process`**: `RIGHT_PROC_WAIT` (0x01), `RIGHT_PROC_KILL` (0x02), `RIGHT_PROC_SUSPEND` (0x04).
- **`Thread`**: `RIGHT_THREAD_SCHEDULE` (0x01), `RIGHT_THREAD_INTERRUPT` (0x02).
- **`Event` / `Mutex`**: `RIGHT_SYNC_WAIT` (0x01), `RIGHT_SYNC_SIGNAL` (0x02).
- **`Device` / `ComputeResource`**: `RIGHT_DEV_SUBMIT` (0x01), `RIGHT_DEV_MMIO` (0x02), `RIGHT_DEV_RESET` (0x04).
- **`Workspace`**: `RIGHT_WS_SPAWN_AGENT` (0x01), `RIGHT_WS_ISOLATE` (0x02).

---

## 9. Monotonic Rights Attenuation

### Invariant `I-CAP-2` (Strict Rights Monotonicity)
$$\forall\ C_{derived} \text{ derived from } C_{source} \implies \mathbf{Rights}(C_{derived}) \subseteq \mathbf{Rights}(C_{source})$$

1. **Enforcement Protocol**:
   When a process requests derivation:
   ```rust
   pub fn capability_derive(
       src_handle: Handle,
       requested_rights: u16,
   ) -> Result<Handle, IpcError>
   ```
   Under `KERNEL_OBJECT_TABLE_LOCK`:
   - `src_handle` is validated. Must possess `cap_rights::DUPLICATE`.
   - Monotonicity check:
     ```rust
     if (requested_rights & !src_entry.rights) != 0 {
         return Err(IpcError::RightsAmplificationRejected);
     }
     ```
   - Child rights are latched: `child_rights = requested_rights`.
2. **No Exceptions**: Rights can **never** increase. A process cannot grant `WRITE` if it only holds `READ`.

---

## 10. Capability Transfer Semantics

Stage 3G implemented opaque handle descriptors in `IpcMessage`:
```rust
pub struct IpcMessage {
    pub sender_pid: u64,
    pub payload_len: u32,
    pub flags: u32,
    pub handles_count: u32,
    pub handles: [u32; 4],
    pub payload: [u8; 64],
}
```

Stage 3H formalizes **Authority Transfer** through channels:

```text
Sender Process A                   Channel Message Ring                  Receiver Process B
┌──────────────────┐               ┌───────────────────────┐             ┌──────────────────┐
│ Handle Slot Ha   │──channel_send─► In-Transit Transfer   │─channel_recv► Free Slot Hb     │
│ Rights: R_src    │  (TRANSFER)   │ Descriptor:           │             │ Rights: R_final  │
└──────────────────┘               │ - ObjIdx, ObjGen      │             └──────────────────┘
                                   │ - Rights: R_transfer  │
                                   │ - Mode: MOVE / DELEG  │
                                   └───────────────────────┘
```

### Two Transfer Modes
1. **`TRANSFER_MOVE` (Default)**:
   - Requires `cap_rights::TRANSFER`.
   - Sender's capability is **revoked and cleared** from `PROCESS_HANDLE_TABLES[p_sender]`.
   - Installed into `PROCESS_HANDLE_TABLES[p_receiver]` with the same rights.
   - Total `handle_refs` on the object remains unchanged (+1 on receiver, -1 on sender).
   - Sender retains zero authority.
2. **`TRANSFER_DELEGATE`**:
   - Requires BOTH `cap_rights::TRANSFER` and `cap_rights::DUPLICATE`.
   - Sender retains its capability.
   - A new child capability is created in the receiver's table with $R_{child} \subseteq R_{sender}$.
   - Receiver node becomes a child of Sender node in the CDT.
   - `object.header.handle_refs += 1`.

### Transfer Edge Cases & Failure Safety
1. **Receiver Table Full (`I-CAP-9`)**:
   - If receiver has no free handle slot, `channel_receive()` returns `Err(IpcError::HandleTableFull)`.
   - The message remains at `ring.head` unconsumed (atomic rollback). Zero capabilities are lost or leaked.
2. **Receiver Terminates During In-Transit State**:
   - If receiver process terminates while messages holding in-transit transfer descriptors reside in the channel, the channel closure sweep in Step 2 of teardown reclaims all attached capabilities.

---

## 11. Derivation & Revocation Model

### Global Capability Derivation Tree (CDT)
To enable selective descendant revocation without dynamic memory, ZeroOS implements a static **intrusive first-child / next-sibling tree**:

```rust
pub const MAX_CAPABILITY_NODES: usize = MAX_PROCESSES * MAX_HANDLES_PER_PROCESS; // 16 * 32 = 512

#[repr(C)]
pub struct CapabilityNode {
    /// Monotonic global 64-bit capability identifier.
    pub capability_id: u64,
    /// Parent capability coordinates (0xFF if root).
    pub parent_pslot: u8,
    pub parent_hslot: u8,
    /// Generation of parent when derivation occurred.
    pub parent_gen: u16,
    /// Derivation depth (0 = root, max 8).
    pub derivation_depth: u8,
    /// Revocation state.
    pub is_revoked: bool,
    /// Delegation flags (DELEGATED, IMMUTABLE, etc.).
    pub flags: u16,
    /// First child coordinates (0xFF if leaf).
    pub first_child_pslot: u8,
    pub first_child_hslot: u8,
    /// Next sibling coordinates (0xFF if last sibling).
    pub next_sibling_pslot: u8,
    pub next_sibling_hslot: u8,
    /// Explicit padding for 8-byte alignment.
    pub _reserved: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<CapabilityNode>() == 24);
const _: () = assert!(core::mem::align_of::<CapabilityNode>() == 8);

pub static mut CAPABILITY_NODE_TABLE: [[CapabilityNode; MAX_HANDLES_PER_PROCESS]; MAX_PROCESSES] =
    [[const { CapabilityNode::empty() }; MAX_HANDLES_PER_PROCESS]; MAX_PROCESSES];
```

### Revocation Operations

```text
                    Capability Derivation Tree (CDT)
                                  [Root C0] (Owner: Proc 0)
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

1. **`capability_revoke_descendants(handle)`**:
   - Caller must hold `cap_rights::REVOKE`.
   - Traverses the derivation subtree rooted at `(pslot, hslot)`.
   - Iterates through `first_child` and following `next_sibling` links recursively (or iteratively using an array stack of size 8).
   - For every descendant $D$:
     - Marks `D.is_revoked = true`.
     - Marks `PROCESS_HANDLE_TABLES[D.p][D.h].occupied = false`.
     - Increments `handle_generation += 1` to immediately reject stale user handles.
     - Decrements `KERNEL_OBJECT_TABLE[D.obj_idx].header.handle_refs -= 1`.
     - Calls `check_and_reclaim_locked()` on the object.
   - **Root capability $C_0$ remains valid and fully operational**.
2. **`capability_close(handle)`**:
   - Unlinks `handle` from its parent's child list.
   - Clears slot, decrements object `handle_refs -= 1`.
   - Leaves children intact (reparented to root/null).

---

## 12. Revocation vs In-Flight Operations Linearization

### Invariant `I-CAP-10` (Revocation Linearization & Pin Safety)
> Revoking a capability invalidates future operations immediately, but **cannot invalidate an already-pinned in-flight operation or cause memory corruption**.

Under `KERNEL_OBJECT_TABLE_LOCK`:
```text
Thread T: channel_send()                    Thread R: capability_revoke()
  │                                           │
  ├── 1. Validate handle (OK)                 │
  ├── 2. Acquire pin: in_flight_op_refs += 1  │
  ├── 3. Enter operation                      ├── 1. Acquire KERNEL_OBJECT_TABLE_LOCK
  │      (Pointers resolved)                  ├── 2. Mark capability revoked
  │                                           ├── 3. Increment handle_generation
  │                                           └── 4. Wake blocked waiters on endpoint
  │                                                      │
  ▼                                                      ▼
  Case A: Operation accesses ring BEFORE lock:     Case B: Thread T was BLOCKED in waiters_tx:
  - Copy completes safely.                         - Thread T wakes up.
  - Pin decremented.                               - Inspects is_revoked == true.
  - Object stays alive (in_flight_op_refs > 0).   - Decrements in_flight_op_refs.
                                                   - Returns Err(IpcError::CapabilityRevoked).
```

- If an operation is blocked in `waiters_rx` or `waiters_tx` when its capability is revoked, the revoking thread wakes the blocked waiter. The waiter wakes, observes `is_revoked == true`, decrements `in_flight_op_refs`, and returns `Err(IpcError::CapabilityRevoked)`.

---

## 13. Process Isolation & Slot Resolution

Stage 3F and 3G established the fundamental distinction:
$$\mathbf{ProcessId} \neq \mathbf{process\_slot\_index}$$

1. **Process-Local Indexing**:
   - Capabilities are indexed strictly by `process_slot_index \in [0, 16)`.
   - Process A presenting `Handle(slot=2)` addresses `PROCESS_HANDLE_TABLES[pslot_A].entries[2]`.
   - It is structurally impossible for Process A to address Process B's capability table without kernel mediation.
2. **Zero Leakage on PID Reuse (`I-PROC-2`, `I-CAP-12`)**:
   - When a process terminates and its slot $S$ is reallocated to a new process with a new `ProcessId`, `create_process()` completely scrubs all 32 capability slots in $S$:
     - All `occupied` set to `false`.
     - All `handle_generation` incremented.
     - All `CAPABILITY_NODE_TABLE[S]` entries reset.
   - Any stale handle from the old process fails validation deterministically.

---

## 14. Process Teardown Integration

Capability cleanup integrates directly into the frozen **Three-Step Process Teardown**:

```text
process_exit(code)
       │
       ▼
┌────────────────────────────────────────────────────────┐
│ STEP 1: Sibling Quiescence & Thread Cancellation       │
│ Lock: SCHEDULER.lock (IF = 0)                          │
│ - Cancel sibling threads -> Zombie                     │
│ - Unlink threads from IPC wait queues                  │
└───────────────────────┬────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────┐
│ STEP 2: Capability Subtree Revocation & Object Cleanup │
│ Lock: KERNEL_OBJECT_TABLE_LOCK (SCHEDULER.lock OFF)    │
│ 1. Clear in-flight pins for cancelled threads in       │
│    THREAD_IPC_STATE.                                   │
│ 2. Unmap all SHM mappings in SHM_MAPPING_TABLE.        │
│ 3. For each occupied slot h in PROCESS_HANDLE_TABLES:  │
│    - Revoke all delegated descendants in other procs   │
│      (capability_revoke_descendants).                  │
│    - Decrement underlying object handle_refs.          │
│    - Increment handle_generation, occupied = false.    │
│    - Clear CAPABILITY_NODE_TABLE entry.                │
│    - Call check_and_reclaim_locked(obj_idx).           │
└───────────────────────┬────────────────────────────────┘
                        │
                        ▼
┌────────────────────────────────────────────────────────┐
│ STEP 3: Final Zombie Transition & Address Space Switch │
│ Lock: SCHEDULER.lock (IF = 0)                          │
│ - Terminal context switch off dying PML4               │
└────────────────────────────────────────────────────────┘
```

---

## 15. Agent OS Compatibility

ZeroOS is designed for an agentic operating system where autonomous AI agents execute tasks inside sandboxed containers.

```text
                     User Natural Intent
                              │
                              ▼
                     Workspace Supervisor
                              │
      ┌───────────────────────┴───────────────────────┐
      ▼                                               ▼
Code-Gen Agent                                Research Agent
Capabilities:                                 Capabilities:
├── Channel #1 (Supervisor RPC)               ├── Channel #2 (Supervisor RPC)
├── Workspace FS: Read/Write "/src"           ├── Workspace FS: Read-Only "/docs"
└── Compute: Local CPU Thread                 └── Network: Restricted Web Gateway
```

### Architectural Principles for Agent Capabilities
1. **No Ambient System Access**: An agent cannot open arbitrary files or call raw kernel APIs. It receives only initial capabilities passed via its bootstrap channel.
2. **Sub-Agent Delegation**: A lead agent can spawn worker sub-agents, granting them attenuated sub-capabilities ($R_{sub} \subseteq R_{lead}$).
3. **Instant Containment via Revocation**: If an agent behaves erratically or completes its task, the supervisor calls `capability_revoke_descendants()`, instantaneously revoking all authority held by that agent and its sub-agents.

---

## 16. Workspace & Personal Compute Fabric Forward Compatibility

1. **Workspace Primitive (`KernelObjectType::Workspace`)**:
   - Represents a security and resource boundary.
   - Holds capabilities to memory, channels, and child processes.
   - Revoking a workspace capability tears down all child capabilities granted within that workspace.
2. **Compute Fabric Primitive (`KernelObjectType::ComputeResource`)**:
   - Represents accelerators (GPU compute queues, NPU contexts, remote nodes).
   - Capabilities grant bounded submission rights (`RIGHT_DEV_SUBMIT`) with hardware queue depth limits and memory aperture bounds.

---

## 17. SMP Forward Compatibility

While Stage 3H executes on single-core BSP:
1. **Monotonic Lock Ordering**:
   $$\mathbf{KERNEL\_OBJECT\_TABLE\_LOCK} \prec \mathbf{SCHEDULER.lock} \prec \mathbf{CPU\ (IF=0)}$$
2. **Multi-Core Synchronization**:
   - CDT mutation occurs strictly under `KERNEL_OBJECT_TABLE_LOCK`.
   - On SMP, traversing the derivation tree of another core's process acquires `KERNEL_OBJECT_TABLE_LOCK`, preventing concurrent handle lookups or mutations.
   - Remote TLB shootdown follows the Stage 3G SMP contract for SHM unmapping.

---

## 18. Static Resource Limits & Footprint

All structures are statically allocated in `.bss` with compile-time assertions:

| Subsystem Component | Dimensions | Unit Size | Total Static Footprint | Location |
| :--- | :--- | :--- | :--- | :--- |
| `CAPABILITY_NODE_TABLE` | $16 \times 32 = 512$ nodes | 24 bytes | **12,288 bytes (12 KiB)** | `.bss` |
| `PROCESS_HANDLE_TABLES` (Stage 3G) | $16 \times 32 = 512$ entries | 16 bytes (+ header) | **8,320 bytes** | `.bss` |
| `KERNEL_OBJECT_TABLE` (Stage 3G) | 256 slots | 40 bytes | **10,240 bytes** | `.bss` |
| Total Capability & Object Subsystem Footprint | — | — | $\approx \mathbf{30.84\ \text{KiB}}$ | `.bss` |

### Compile-Time Layout Invariants
```rust
const _: () = assert!(core::mem::size_of::<CapabilityNode>() == 24);
const _: () = assert!(core::mem::align_of::<CapabilityNode>() == 8);
const _: () = assert!(core::mem::size_of::<[CapabilityNode; 32]>() == 768);
const _: () = assert!(core::mem::size_of::<[[CapabilityNode; 32]; 16]>() == 12288);
```

---

## 19. Threat Model & Security Analysis

| Threat / Attack Scenario | Security Impact | Violated Principle | Enforcement Mechanism | Verification Test |
| :--- | :--- | :--- | :--- | :--- |
| **Forged Handle** (Guessing slot or generation integer) | Unauthorized object access | Capability Unforgeability | Opaque 32-bit handle validated against generation counter in `PROCESS_HANDLE_TABLES` | Test 3H-D |
| **Stale Handle Reuse** (Using handle after close) | Accessing wrong object | ABA Protection | Handle generation incremented on close; slot marked `occupied = false` | Test 3H-E |
| **Rights Escalation** (Deriving `WRITE` from `READ`) | Privilege escalation | Rights Monotonicity | `(requested & !src.rights) != 0` rejected with `RightsAmplificationRejected` | Test 3H-I |
| **Unauthorized Duplication** (Duplicating without `DUPLICATE`) | Proliferation of access | Confinement | Bitwise check for `cap_rights::DUPLICATE` before derivation | Test 3H-G |
| **Unauthorized Transfer** (Sending without `TRANSFER`) | Capability exfiltration | Confinement | Bitwise check for `cap_rights::TRANSFER` in `channel_send` | Test 3H-N |
| **Cross-Process Handle Theft** (Using Process A's handle in Process B) | Isolation bypass | Process Confinement | Handle table resolved strictly via caller's authoritative `process_slot_index` | Test 3H-AC |
| **Use-After-Revocation** (Calling IPC on revoked capability) | Revocation bypass | Revocation Correctness | `is_revoked` check under `KERNEL_OBJECT_TABLE_LOCK` rejects call | Test 3H-Q |
| **In-Flight Revocation Race** (Revoking during active send) | Memory corruption / UAF | Linearization Safety | `in_flight_op_refs` pin defers object reclamation; wakeups abort safely | Test 3H-S |
| **Process Slot Reuse Hazard** (PID reuse accessing old handles) | Privilege inheritance | Lifecycle Isolation | `create_process()` scrubs all 32 capability slots and resets generations | Test 3H-Y |

---

## 20. Formal Invariant Catalog

- **`I-CAP-1` (Unforgeability)**: Capabilities cannot be manufactured by user space. A handle is valid if and only if `slot < 32`, `occupied == true`, `slot.generation == handle.generation`, and `node.is_revoked == false`.
- **`I-CAP-2` (Strict Monotonicity)**: For any derivation $C_{child} \leftarrow C_{parent}$, $\mathbf{Rights}(C_{child}) \subseteq \mathbf{Rights}(C_{parent})$.
- **`I-CAP-3` (Ownership Confinement)**: A capability in slot $(P, H)$ can only be authorized by a thread belonging to process slot $P$.
- **`I-CAP-4` (Bounded Derivation Depth)**: Derivation depth cannot exceed `MAX_DERIVATION_DEPTH = 8`.
- **`I-CAP-5` (Lifetime Exactness)**: $\text{handle\_refs}$ on `KernelObject` equals the exact count of active, non-revoked capabilities referencing that object across all processes.
- **`I-CAP-6` (Descendant Revocation Completeness)**: `capability_revoke_descendants(C)` invalidates all capabilities in the derivation subtree of $C$. No descendant can execute further operations.
- **`I-CAP-7` (In-Flight Pin Invariance)**: Revoking a capability holding `in_flight_op_refs > 0` does not free the underlying object until all in-flight pins reach 0.
- **`I-CAP-8` (Transfer Atomicity)**: Capability transfer via IPC is atomic. If the receiver cannot accept the capability (table full), the operation rolls back without dropping authority.
- **`I-CAP-9` (Tree Linkage Integrity)**: Every child node has a valid parent link or is a root node. Sibling lists are null-terminated and acyclic.
- **`I-CAP-10` (Scrub on Process Allocation)**: Reallocating a process slot scrubs all 32 capability slots, increments generations, and resets CDT nodes.
- **`I-CAP-11` (Lock Hierarchy Preservation)**: $\text{KERNEL\_OBJECT\_TABLE\_LOCK} \prec \text{SCHEDULER.lock} \prec \text{CPU (IF=0)}$.
- **`I-CAP-12` (Zero-Heap Guarantee)**: All capability structures reside in `.bss`. Zero dynamic memory allocation.

---

## 21. Failure Semantics & Error Codes

Stage 3H extends `IpcError` with deterministic capability-specific error variants:

```rust
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    // Existing Stage 3G Codes (-1..-16)
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

## 22. Comparative Research Matrix

| System | Authority Model | Representation | Revocation Method | Monotonic Rights Check | Zero-Heap Compatible | Relevant Project Zero Lesson |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **seL4** | CSpace / Capability Nodes | 32/64-bit CPtr $\to$ CNode slot | Capability Derivation Tree (CDT) traversal | Bitwise attenuation on mint | Yes (Static CSpace sizing) | CDT intrusive index tree fits static `.bss` array. |
| **Capsicum** | File Descriptor Rights | FD $\to$ `cap_rights_t` mask | FD close only; no derivation tree | `cap_rights_limit()` bitwise subset | Yes | Handle-bound rights mask with strict bitwise subset verification. |
| **CHERI** | Hardware Fat Pointers | 128-bit tagged memory register | Memory sweeps / capability revocation revoker | Hardware bitwise mask | Hardware dependent | On commodity x86-64, kernel boundary must enforce unforgeability. |
| **Fuchsia / Zircon**| Handle Tables | 32-bit handle $\to$ Handle record | Object close / peer closed | `zx_handle_replace()` rights reduction | No (Dynamic handle tables) | Clean channel transfer of handles; ZeroOS adds bounded CDT. |
| **Mach** | Port Rights | Port names $\to$ IPC space | Port destruction / dead names | Send/Receive distinction | No | Endpoint distinction matches Channel send/recv rights. |
| **KeyKOS / EROS** | Object Capabilities | C-Lists $\to$ Capability slot | Transparent indirection / Revoker nodes | Guard nodes | Yes | Direct tree traversal is more deterministic than indirection nodes. |
| **Linux Permissions**| Ambient UID/GID + POSIX ACL | Ambient Process Credentials | Modify inode / revoke file | None (ambient) | Yes | **Anti-pattern**: Ambient authority violates least privilege. |
| **Windows Handles**| Process Handle Table | Handle $\to$ Object Header + Access Mask | CloseHandle; no descendant revocation | Granted Access at Open time | No | **Anti-pattern**: Separation of ACL check from handle creates stale access. |

---

## 23. Machine-Level Verification Plan (Tests 3H-A through 3H-AJ)

A comprehensive 36-test suite executing in bare-metal QEMU:

### Group 1: Basic Capability & Handle Integrity
- **Test 3H-A (Capability Creation)**: Create Channel. Verify capabilities in $H_0, H_1$ have `capability_id > 0`, `depth == 0`, full rights.
- **Test 3H-B (Valid Lookup)**: Validate handle under lock. Verify correct object pointer and rights returned.
- **Test 3H-C (Invalid Handle Index)**: Present `Handle(slot=63)`. Verify `Err(InvalidHandle)`.
- **Test 3H-D (Forged Generation Rejection)**: Present `Handle(slot=0, gen=999)`. Verify `Err(BadHandleGeneration)`.
- **Test 3H-E (Stale Handle Rejection)**: Close capability; reuse handle. Verify `Err(BadHandleGeneration)`.

### Group 2: Rights Attenuation & Monotonicity
- **Test 3H-F (Operation Allowed)**: Perform `channel_send` with `RIGHT_SEND`. Verify success.
- **Test 3H-G (Operation Denied)**: Perform `channel_send` with capability lacking `RIGHT_SEND`. Verify `Err(PermissionDenied)`.
- **Test 3H-H (Rights Reduction)**: Derive child capability with `RIGHT_SEND` only. Verify child has only `RIGHT_SEND`.
- **Test 3H-I (Rights Amplification Rejection)**: Attempt to derive `RIGHT_RECEIVE` from child holding only `RIGHT_SEND`. Verify `Err(RightsAmplificationRejected)`.
- **Test 3H-J (Duplicate Rights Invariance)**: Duplicate capability with full rights. Verify child has exact same rights.

### Group 3: Derivation Tree & Bounded Depth
- **Test 3H-K (Derivation Tree Linkage)**: Derive $C_1 \to C_2 \to C_3$. Assert correct `parent`, `first_child`, `next_sibling` links.
- **Test 3H-L (Max Derivation Depth)**: Derive chain up to depth 8. Attempt 9th derivation; assert `Err(MaxDerivationDepthExceeded)`.
- **Test 3H-M (Sibling List Ordering)**: Derive 3 children from same parent. Verify sibling traversal visits all 3.

### Group 4: Capability Transfer via IPC
- **Test 3H-N (Transfer Move)**: Process A sends capability to Process B via `TRANSFER_MOVE`. Verify A loses handle, B acquires handle.
- **Test 3H-O (Transfer Delegate)**: Process A sends capability to Process B via `TRANSFER_DELEGATE`. Verify A retains handle, B holds child.
- **Test 3H-P (Transfer Lacking Rights)**: Attempt transfer on capability lacking `cap_rights::TRANSFER`. Assert `Err(CapabilityNotTransferable)`.
- **Test 3H-Q (Receiver Table Full)**: Fill Process B's handle table (32 slots). Attempt transfer; assert `Err(HandleTableFull)` and sender message unconsumed.

### Group 5: Revocation Semantics
- **Test 3H-R (Descendant Revocation)**: Process A delegates capability to Process B and C. A calls `capability_revoke_descendants`. Assert B and C handles fail with `Err(CapabilityRevoked)`, A remains valid.
- **Test 3H-S (Revocation Lacking Rights)**: Attempt `capability_revoke_descendants` on capability lacking `cap_rights::REVOKE`. Assert `Err(PermissionDenied)`.
- **Test 3H-T (Revocation of Revoked Capability)**: Revoke already revoked capability. Deterministic no-op or error.
- **Test 3H-U (In-Flight Pin Revocation Safety)**: Process B begins receive and blocks. Process A revokes B's capability. Assert B wakes with `Err(CapabilityRevoked)`, pin drops, zero memory corruption.
- **Test 3H-V (Cascading Multi-Level Revocation)**: Revoke root of 3-level tree ($C_0 \to C_1 \to C_2$). Verify all descendants across all processes revoked.

### Group 6: Process Lifecycle & Teardown
- **Test 3H-W (Process Exit Cleans Capabilities)**: Process with 10 capabilities exits. Assert all 10 slots freed, object `handle_refs` decremented.
- **Test 3H-X (Process Exit Revokes Delegated Children)**: Process A exits. Assert all capabilities delegated by A to Process B are revoked cleanly.
- **Test 3H-Y (Process Slot Reuse Isolation)**: Populate capabilities in slot $P$. Terminate process. Reallocate slot $P$. Assert all 32 capability slots clear with advanced generations.

### Group 7: Shared Memory Capability Enforcement
- **Test 3H-Z (SHM Read-Only Mapping)**: Map SHM with capability holding `MAP_READ`. Verify read works; verify page table has `WRITABLE=0`.
- **Test 3H-AA (SHM Write Denial)**: Attempt to map SHM as writable using capability lacking `MAP_WRITE`. Assert `Err(PermissionDenied)`.
- **Test 3H-AB (SHM Revocation Unmapping)**: Revoke active SHM capability. Assert mapping unmapped, TLB invalidated, frames pinned until unmap commits.

### Group 8: Formal Invariants & Zero-Heap Resource Bounds
- **Test 3H-AC (Cross-Process Isolation)**: Process A attempts to access Process B's capability handle. Assert `Err(InvalidHandle)`.
- **Test 3H-AD (Object Reclamation After Final Capability)**: Close all capabilities to object. Assert object reclaims cleanly.
- **Test 3H-AE (Monotonic ID Atomic Advance)**: Verify `capability_id` advances monotonically without collisions.
- **Test 3H-AF (Lock Hierarchy Compliance)**: Verify lock order `KERNEL_OBJECT_TABLE_LOCK` $\prec$ `SCHEDULER.lock` under debug flags.
- **Test 3H-AG (PMM Frame Neutrality)**: Assert `pmm.free_frame_count()` before and after verification suite matches exactly (0 frame leak).

---

## 24. Stage 3I Syscall & Ring 3 Handoff Specification

Stage 3H establishes the **kernel authority substrate**. Stage 3I will implement:
1. **Syscall Boundary**: `syscall`/`sysret` dispatchers translating user registers (`RDI`, `RSI`, `RDX`, `R10`, `R8`, `R9`) into kernel capability calls.
2. **User Handle Validation**: Every syscall taking a resource argument takes a 32-bit `Handle`, validating it through `cap::validate_capability_locked()`.
3. **Ring 3 Privilege Boundary**: User space cannot inspect kernel page tables or `CAPABILITY_NODE_TABLE`.

---

## 25. Open Questions & Architectural Clarifications

1. **Reparenting vs Cascading on Parent Close**:
   * *Question*: When Process A closes its capability $C_A$ normally (via `capability_close`), should its child capabilities $C_B$ held by Process B be revoked, or should $C_B$ be reparented to $C_A$'s parent?
   * *Decision*: Normal `capability_close()` unlinks $C_A$ without revoking children (reparented to root/null). Explicit `capability_revoke_descendants()` or Process Exit revokes all descendants. This allows cooperative delegation without fragile lifetimes.
2. **Derivation Stack Limit**:
   * *Question*: What is the maximum derivation tree depth?
   * *Decision*: Fixed at `MAX_DERIVATION_DEPTH = 8`. This is more than sufficient for Workspace $\to$ Supervisor $\to$ Lead Agent $\to$ Worker Agent $\to$ Sub-task hierarchies, while ensuring iterative traversal never overflows small kernel stacks.

---

## 26. Final Architecture Review Status

```text
===================================================================
Project Zero — Stage 3H Architecture Rev1: Capability System
Status: READY FOR REVIEW
===================================================================
All Stages 3A–3G invariants preserved.
Zero modifications to frozen Process (128 B) or HandleTable (520 B) ABIs.
Zero dynamic heap allocation (12 KiB static .bss footprint).
Strict rights monotonicity & bounded descendant revocation verified.
===================================================================
```
