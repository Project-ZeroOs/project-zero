# ADR-0009: Physical Frame Manager (PMM) Architecture

## Status
Accepted (Stage 2D Baseline)

## Context
Stage 2C established an authoritative, read-only memory inventory by discovering firmware memory maps through the Multiboot 1 ABI, subtracting critical Project Zero kernel reservations, and deriving page-aligned 4 KiB frame-candidate intervals.

Stage 2D introduces the **Physical Frame Manager (PMM)** responsible for physical frame ownership, state tracking, allocation, and deallocation. 

### Architectural Challenges & Constraints
1. **No Circular Bootstrap Dependency**: A dynamic memory allocator cannot allocate the storage required for its own metadata from memory it does not yet manage. The PMM metadata must have an explicitly known physical address and extent *before* the allocator becomes operational.
2. **Elimination of the 128-MiB Hardcoded Memory Model**: While initial QEMU environments provision 128 MiB of RAM, the PMM architecture must not assume 128 MiB is an architectural constant. It must preserve 64-bit physical addresses, derive tracked frame counts dynamically from the firmware inventory, support compile-time bounded capacity, and fail closed with explicit capacity errors if memory exceeds that capacity.
3. **Strict Privilege & Subsystem Separation**: The PMM must consume the Stage 2C page-aligned frame candidates directly and must not re-parse raw Multiboot headers. Furthermore, low-level architecture layers (such as GDT) must not depend on PMM, preventing cyclic module dependencies (`gdt -> pmm -> inventory -> gdt`).
4. **Fail-Closed State Model**: Unknown, unclassified, or accidentally omitted physical memory must never become allocatable. All physical frames must start in a non-allocatable state (`RESERVED` or `UNUSABLE`), and only verified Stage 2C page-aligned candidate frames may be promoted to `FREE`.

---

## Decision

### 1. Explicit 4-State 2-Bit Authoritative Bitmap
Every tracked 4 KiB physical frame is tracked in an authoritative 2-bit state model:
* `0b00 = FrameState::Free`: Unallocated candidate frame available for allocation.
* `0b01 = FrameState::Allocated`: Granted by `alloc_frame()`; owned by caller.
* `0b10 = FrameState::Reserved`: Protected kernel image, stacks, page tables, GDT, TSS, IDT, or Multiboot boot metadata.
* `0b11 = FrameState::Unusable`: Firmware non-RAM (MMIO, ACPI, BIOS ROM, or out-of-pool memory).

This representation mathematically guarantees mutual exclusivity by construction:
$$\text{Free} + \text{Allocated} + \text{Reserved} + \text{Unusable} = \text{Tracked Frames}$$
$$\text{Allocated} \cap \text{Free} = \emptyset, \quad \text{Allocated} \cap \text{Reserved} = \emptyset, \quad \text{Free} \cap \text{Reserved} = \emptyset$$

### 2. Statically Reserved Metadata Bootstrap
To eliminate circular bootstrap dependencies:
1. PMM metadata storage (`PmmMetadataStorage`) is statically allocated in `.bss` with 4096-byte alignment.
2. Capacity is set to `PMM_MAX_TRACKED_FRAMES = 65,536` frames (256 MiB physical address space), requiring $65,536 \times 2 \text{ bits} = 16,384 \text{ bytes}$ (4 frames / 16 KiB).
3. The physical range `[start, end)` of this metadata is queried via `get_pmm_metadata_range()` and registered as an explicit Project Zero reservation (`"PMM Bitmap Metadata"`) in `inventory.rs` *before* Stage 2D candidate pools are finalized.
4. Non-overlap with all other critical reservations is validated with an assertion before insertion.
5. The Stage 2D candidate pool is derived after this subtraction, cleanly carving out the 4 metadata frames with zero circular dependencies.

### 3. Strongly Typed 64-Bit Frame Identities
We distinguish physical byte addresses from frame indices without 32-bit truncation:
* `FrameNumber(pub u64)`: Explicit 64-bit frame index.
* `PhysFrame(pub u64)`: Explicit 64-bit physical address, guaranteed to satisfy `address % 4096 == 0`.
* Conversions between `FrameNumber` and `PhysFrame` enforce checked multiplication and alignment checks, rejecting arithmetic overflow and misaligned addresses.

### 4. Fail-Closed Initialization Pipeline
PMM initialization strictly follows the sequence:
```
All Tracked Frames (0..N)
    ↓
Set to UNUSABLE (or RESERVED if within firmware Type 1 RAM)
    ↓
Mark all Stage 2C Reservations as RESERVED
    ↓
Promote ONLY Stage 2C page-aligned candidate pools to FREE
```
If the physical memory requires tracking more than `PMM_MAX_TRACKED_FRAMES`, initialization returns `Err(PmmInitError::CapacityExceeded { required, max })` rather than silently truncating or corrupting memory.

### 5. Allocation & Free API
* `alloc_frame() -> Option<PhysFrame>`:
  - Scans bitmap using deterministic first-free word skipping.
  - Transitions frame from `FREE` $\to$ `ALLOCATED`.
  - Asserts 4 KiB alignment and inclusion within candidate ranges.
  - Returns `None` on exhaustion without corrupting state.
* `free_frame(frame: PhysFrame) -> Result<(), PmmError>`:
  - Validates 4 KiB alignment and range bounds.
  - Rejects double-free if already `FREE` (`PmmError::DoubleFree`).
  - Rejects freeing `RESERVED` frames (`PmmError::FrameReserved`).
  - Rejects freeing `UNUSABLE` frames (`PmmError::FrameUnusable`).
  - Rejects misaligned addresses (`PmmError::FrameMisaligned`).
  - Transitions valid allocated frame from `ALLOCATED` $\to$ `FREE`.

---

## Verification & Invariants
The implementation is validated by runtime debug assertions and an automated test suite (`tests/test_pmm.py`):
1. **Accounting Invariant**:
   $$\text{free\_count} + \text{allocated\_count} + \text{reserved\_count} + \text{unusable\_count} == \text{tracked\_frames}$$
2. **Metadata Candidate Delta**:
   Boot telemetry verifies that PMM metadata is excluded from candidate frames:
   - Candidate frames before PMM metadata: 32,385 frames
   - PMM metadata frames: 4 frames (16 KiB)
   - Candidate frames after PMM metadata: 32,381 frames (Delta: -4 frames)
3. **Runtime Smoke Test**:
   Every boot executes a deterministic smoke test:
   - Allocate Frame A, Allocate Frame B ($A \neq B$, 4 KiB aligned)
   - Free Frame A
   - Allocate Frame C (verifies deterministic first-free reuse of Frame A)
   - Free Frames B and C
   - Rejection tests for double free, freeing reserved memory, and misaligned addresses
   - Complete restoration of initial invariant state

---

## Consequences
* **Positive**:
  - Authoritative, robust physical frame allocator operating with $O(1)$ word-skipping deterministic scans.
  - No circular bootstrap dependencies.
  - Security invariant preserved: unknown physical memory is never allocatable.
  - Clean modularity: GDT and CPU layers remain uncoupled from PMM.
* **Limitations**:
  - Linear bitmap scan is prioritized for correctness over multi-core NUMA or buddy performance (acceptable for Stage 2D microkernel nucleus).
  - Virtual memory management (VMM), page tables, and heap remain strictly out of scope until Stage 2E.
