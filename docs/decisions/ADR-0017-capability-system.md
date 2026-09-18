# ADR-0017: Capability System and Kernel Authority Model

## Status
**Accepted / Frozen** (2026-09-17)

## Context
Stages 3A through 3G established bare-metal scheduling, memory virtualization, synchronization, and an IPC subsystem operating with untyped handle integers. However, Project Zero requires a mathematically rigorous authority model prior to user-space task execution (Stage 3I):
1. **Separation of Authority from Identity**: Process IDs and address pointers cannot confer execution rights. All kernel resource access must be mediated by unforgeable capabilities.
2. **Capability Derivation & Delegation**: Processes must be able to derive attenuated capabilities (e.g. read-only views of shared memory or channels) and delegate them to peer processes.
3. **Decoupled User Revocation vs. Kernel Teardown Authority**: Revocation initiated by a user process requires explicit `cap_rights::REVOKE`. Conversely, kernel process exit must reclaim resources owned by the terminating process unconditionally without requiring `REVOKE`.
4. **Ownership-Driven Teardown & Preservation of Peer Delegations (`I-CAP-TEARDOWN-2`, `I-CAP-TEARDOWN-3`)**: Process teardown is ownership-driven, not lineage-driven. Surviving peer processes holding delegated capabilities must not have their capabilities invalidated merely because the delegating process terminates.
5. **Frozen Process ABI & Zero Kernel Dynamic Memory**: Process ABI (128 bytes) and HandleTable ABI (520 bytes) remain frozen. Static `.bss` tables only.

## Decision

We establish the **Stage 3H Capability System and Kernel Authority Model**:

### 1. Capability Node Structure & Static Sizing
- **`CapabilityNode`**: 24 bytes (8-byte aligned) intrusive tree node:
  ```rust
  #[repr(C)]
  pub struct CapabilityNode {
      pub capability_id: u64,       // 8 B: Monotonic non-wrapping capability ID
      pub parent_pslot: u8,         // 1 B: Parent process slot (0xFF = root)
      pub parent_hslot: u8,         // 1 B: Parent handle slot (0xFF = root)
      pub parent_gen: u16,          // 2 B: Parent generation tag
      pub first_child_pslot: u8,    // 1 B: Head of child list
      pub first_child_hslot: u8,    // 1 B: Head of child list
      pub next_sibling_pslot: u8,   // 1 B: Intrusive sibling link
      pub next_sibling_hslot: u8,   // 1 B: Intrusive sibling link
      pub derivation_depth: u8,     // 1 B: Depth bound in [0..MAX_DERIVATION_DEPTH]
      pub flags: u8,                // 1 B: Audit/node flags
      pub is_revoked: bool,         // 1 B: Revocation marker
      pub _reserved: [u8; 5],       // 5 B: Padding to 24 B
  }
  ```
- **Static Footprint**: `CAPABILITY_NODE_TABLE: [[CapabilityNode; 32]; 16]` residing in static `.bss` ($16 \times 32 \times 24 = 12\text{ KiB}$).
- **1:1 Slot Pairing (`I-CAP-1`)**: Slot index `[p][h]` in `CAPABILITY_NODE_TABLE` is paired 1:1 with `PROCESS_HANDLE_TABLES[p].entries[h]`.

### 2. Derivation, Delegation & Depth Bounds
- **Monotonic Rights Attenuation (`I-CAP-X`)**: Any derived or delegated capability must satisfy `(requested_rights & !parent_rights) == 0`. Amplification attempts return `Err(IpcError::RightsAmplificationRejected)`.
- **Bounded Derivation Depth (`I-CAP-4`)**: `MAX_DERIVATION_DEPTH = 8`. Derivation attempts beyond depth 8 return `Err(IpcError::MaxDerivationDepthExceeded)`.
- **Monotonic 64-Bit IDs (`I-CAP-ID-1`, `I-CAP-ID-2`)**: `NEXT_CAPABILITY_ID: AtomicU64` increments monotonically from 1. At `u64::MAX`, it halts with `Err(IpcError::CapabilityIdExhausted)` without wrapping.

### 3. Option B Grandparent Reparenting (`I-CAP-CLOSE-1`, `I-CAP-TEARDOWN-2`)
- When capability $C$ is closed (voluntarily or via process exit), surviving child capabilities of $C$ are not revoked:
  - If $C$ had a parent $P$, children of $C$ are reparented directly to $P$ (`child.parent = P`, spliced into $P$'s child list).
  - If $C$ was a root capability, children of $C$ are promoted to independent root capabilities (`child.parent = None`, `derivation_depth = 0`).

### 4. Single-ID Transfer Move (`I-CAP-MOVE-3`)
- `transfer_move_locked(src_pslot, src_hslot, dst_pslot)`:
  1. **Preflight**: Verifies caller holds `cap_rights::TRANSFER`, validates destination slot availability, reserves exactly one `reserved_capability_id` via `allocate_capability_id()`.
  2. **Commit**: Installs capability in `dst_pslot` with `reserved_capability_id`, reparents all descendant subtrees to the new location (`I-CAP-MOVE-2`), and closes/clears the source handle and node without allocating a second ID.

### 5. Decoupled User Revocation vs. Process Exit Teardown
- **User Revocation (`capability_revoke_descendants_locked`)**: Caller MUST hold `cap_rights::REVOKE`. Only descendants are recursively revoked, while the target capability remains valid (`I-CAP-REVOKE-1`).
- **Kernel Process Teardown (`process_exit_capability_cleanup_locked`)**: Kernel executive authority unmaps all SHM mappings and reclaims every capability owned by the terminating process slot. Operates unconditionally without requiring `REVOKE` (`I-CAP-TEARDOWN-1`). Peer delegated capabilities survive under Option B reparenting (`I-CAP-TEARDOWN-2`, `I-CAP-TEARDOWN-3`).

### 6. Lock Hierarchy
$$\mathbf{KERNEL\_OBJECT\_TABLE\_LOCK} \prec \mathbf{SCHEDULER.lock} \prec \mathbf{CPU\ (IF=0)}$$
All capability operations execute with deterministic $O(1)$ or bounded $O(K)$ latency under `KERNEL_OBJECT_TABLE_LOCK`.

## Invariant Catalog
- `I-CAP-1`: 1:1 pairing between `HandleEntry` and `CapabilityNode`.
- `I-CAP-4`: Bounded derivation depth ($D \le 8$).
- `I-CAP-10`: Slot reuse isolation (scrubbed node on slot reclaim).
- `I-CAP-11`: Monotonic lock hierarchy compliance.
- `I-CAP-X`: Monotonic rights attenuation; amplification rejected.
- `I-CAP-Y`: Scoped revocation isolation.
- `I-CAP-ID-1`: Monotonic atomic ID advance with terminal exhaustion state.
- `I-CAP-ID-2`: Bounded-scan lookup by capability ID.
- `I-CAP-MOVE-1`: MOVE direct parent inheritance.
- `I-CAP-MOVE-2`: MOVE descendant subtree preservation.
- `I-CAP-MOVE-3`: Single ID consumption in MOVE.
- `I-CAP-CLOSE-1`: Grandparent reparenting on capability close.
- `I-CAP-TEARDOWN-1`: Exiting process capability and handle tables completely emptied.
- `I-CAP-TEARDOWN-2`: Peer delegated capabilities preserved across process teardown.
- `I-CAP-TEARDOWN-3`: Ownership-driven teardown (not lineage-driven).
- `I-CAP-HANDLE-1`: Derived/moved handles never create unsolicited roots.
- `I-CAP-HANDLE-2`: HandleTable count parity maintained across all operations.
