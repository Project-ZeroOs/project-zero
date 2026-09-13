# ADR-0010: Virtual Memory Manager (VMM) Architecture

## Status
Accepted (Stage 2E Baseline)

## Context
Following the implementation of the physical memory inventory (Stage 2C) and the Physical Frame Manager (Stage 2D), Project Zero requires an authoritative **Virtual Memory Manager (VMM)** to manage x86-64 4-level paging hierarchies, virtual-to-physical address translations, and memory protection attributes.

### Architectural Challenges & Invariants
1. **Dynamic Hardware Address Geometry**: x86-64 processors implement differing physical address widths (e.g., 36, 40, 48, or 52 bits). Hardcoding a generic 52-bit mask allows reserved bits to be set, triggering CPU General Protection faults (#GP) on entry reload. Address validation must derive from authoritative CPUID discovery.
2. **Execution Protection & Hardware Enablement**: The No-Execute (`NX`) bit (bit 63) is invalid unless hardware support is verified via CPUID and explicitly enabled via `IA32_EFER.NXE` (bit 11). Setting NX when `EFER.NXE = 0` triggers immediate page faults (#PF) or reserved-bit exceptions.
3. **PMM vs. VMM Separation of Concerns**:
   - **PMM**: Owns physical frames, tracks frame allocation states (Free, Allocated, Reserved, Unusable).
   - **VMM**: Owns page-table structures (`PML4`, `PDPT`, `PD`, `PT`) and creates/destroys translation mappings.
   - **Caller**: Retains authoritative ownership of mapped physical data frames. Unmapping a virtual page reclaims page tables, but **never** frees the mapped data frame to PMM.
4. **Intermediate Table Security Domain (`USER=0`)**: On x86-64, privilege checks propagate through the entire translation hierarchy. For kernel supervisor mappings, intermediate entries (`PML4E`, `PDPTE`, `PDE`) and leaf `PTE` must strictly enforce `USER = 0`. Future user mappings will utilize a distinct domain without rewriting page-table walkers.
5. **Atomic Allocation Rollback**: When mapping a virtual address, new intermediate tables (`PDPT`, `PD`, `PT`) are allocated on demand from PMM. If an allocation fails partway through, or a duplicate mapping is detected at the leaf, all newly allocated tables in that call must be freed back to PMM and parent entries cleared, preventing orphaned tables.
6. **Intermediate Table Reclamation & Accounting Restoration**: When `unmap_page()` removes a leaf translation, any intermediate table that becomes completely empty (0 present entries) must be reclaimed back to PMM, preserving PMM free-frame accounting across complete map/unmap cycles.
7. **Transparent Huge-Page Translation Compatibility**: The early bootstrap identity-maps 0..1 GiB using 2 MiB huge pages at the PD level. The VMM must transparently translate both 4 KiB pages and huge pages, but must **only** allocate 4 KiB mappings during Stage 2E.

---

## Decision

### 1. Authoritative Hardware Geometry Discovery
Hardware address dimensions are discovered dynamically during boot via CPUID leaf `0x8000_0008`:
* `physical_bits`: Number of physical address bits (e.g. 40 on QEMU x86-64).
* `virtual_bits`: Number of linear virtual address bits (48 bits in standard 4-level paging).
* `physical_mask`: Authoritative bitmask derived as `((1 << physical_bits) - 1) & !0xFFF`.

Any physical address or frame exceeding `physical_mask` is strictly rejected (`InvalidPhysicalAddress`). Any virtual address failing canonical 48-bit sign extension (bits 47..63 identical) is rejected (`NonCanonicalAddress`).

### 2. Execution Protection (NX) Detection & Enablement
Before any paging structures are manipulated:
1. CPUID leaf `0x8000_0001` EDX bit 20 is queried to confirm NX support.
2. If supported, bit 11 (`NXE`) of `IA32_EFER` (MSR `0xC000_0080`) is set via `wrmsr`.
3. Setting `PageTableFlags::NO_EXECUTE` without `geometry.nx_enabled` is rejected (`NxDisabled`).

### 3. Strongly Typed CR3 & Explicit PCID Policy
CR3 is modeled via a typed struct `Cr3`:
* `root`: Physical root frame (`PhysFrame`), validated against `physical_mask`.
* `flags`: Control flags (`Cr3Flags`: `PAGE_LEVEL_WRITETHROUGH`, `PAGE_LEVEL_CACHE_DISABLE`).
* **PCID Policy**: Single-core bring-up explicitly keeps PCID disabled (`CR4.PCIDE = 0`). Bits `[11:0]` are masked/zeroed on CR3 reload.

### 4. Simple, Non-Parameterized Paging Structures
To prevent generic abstraction sprawl, paging types remain concrete:
* `PageTable`: 4096-byte aligned (`#[repr(C, align(4096))]`) array of 512 `PageTableEntry` primitives.
* `PageTableEntry`: Transparent 64-bit value with checked frame and flag extraction against `AddressSpaceGeometry`.
* `PageTableFlags`: Type-safe bitflags representing architectural attributes (`PRESENT`, `WRITABLE`, `USER_ACCESSIBLE`, `NO_EXECUTE`, etc.).

### 5. Security Domain Model (`MappingDomain`)
Translations are classified by security domain:
* `MappingDomain::Kernel`: Intermediate tables (`PML4E`, `PDPTE`, `PDE`) enforce `USER = 0`. Leaf entries enforce `USER = 0`.
* `MappingDomain::User`: Intermediate tables enforce `USER = 1` along the user path (reserved for future Stage 3).

### 6. PMM-Backed Intermediate Table Allocation & Atomic Rollback
When `ActivePageTable::map_page()` walks the 4-level hierarchy:
1. If an intermediate table entry (`PML4E`, `PDPTE`, `PDE`) is absent, a new physical frame is allocated from `PMM.alloc_frame()`.
2. The newly allocated table frame is immediately zeroed (`table.zero()`).
3. Allocations made during that specific call are tracked (`allocated_pdpt`, `allocated_pd`, `allocated_pt`).
4. If allocation fails at any step, or if the leaf `PTE` is already present (`AlreadyMapped`), an atomic rollback occurs:
   - Newly allocated tables are freed back to PMM in reverse order.
   - Parent entries in pre-existing tables are cleared.
   - Zero leaked frames or corrupted pointers remain.

### 7. Empty Intermediate Table Reclamation & Ownership Preservation
When `ActivePageTable::unmap_page()` executes:
1. The leaf `PTE` is cleared, and `cpu::invlpg` flushes the linear translation from the TLB.
2. The mapped `PhysFrame` is returned to the caller (`Ok(mapped_frame)`). **It is not freed to PMM.**
3. If the leaf PT has 0 present entries (`present_count() == 0`), its frame is freed to PMM and the parent PDE cleared.
4. Reclamation cascades upward to the PD and PDPT, stopping at the root PML4 or when a table still contains other active mappings (such as the early boot 1 GiB mapping).
5. A full map/unmap lifecycle followed by caller data frame deallocation restores PMM free-frame counts to the exact initial baseline.

---

## Consequences

### Positive
* **Deterministic Memory Accounting**: Zero intermediate table leaks on failure, and complete restoration of free memory on unmapping.
* **Privilege Enforcement**: Strict `USER=0` policy prevents intermediate table privilege escalation.
* **Hardware Robustness**: CPUID geometry discovery prevents reserved-bit #GP faults across diverse x86-64 hardware.
* **Capability Cleanliness**: Caller retention of mapped data frames establishes the necessary abstraction for future memory capabilities and shared mappings.

### Out of Scope for Stage 2E
* Higher-half kernel relocation (`0xFFFF_8000_0000_0000`).
* Kernel heap allocator (`GlobalAlloc`) modifications.
* Ring 3 user address space management.
* Dynamic 2 MiB / 1 GiB huge-page allocation (read-only translation supported).
* Multi-core / SMP TLB shootdowns.
