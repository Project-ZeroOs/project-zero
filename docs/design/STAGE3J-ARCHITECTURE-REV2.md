# Stage 3J — ELF & Program Execution Architecture Specification

**Status**: 🟡 PENDING ARCHITECTURAL REVIEW (Rev2)  
**Contract Author**: Project Zero Kernel Nucleus  
**Target Milestone**: Stage 3J  
**Dependencies**: Stage 2 (PMM, VMM), Stage 3A–3E (Kernel Threads & Scheduler), Stage 3F (Processes & AddressSpace), Stage 3G (IPC & Objects), Stage 3H (Capabilities & Handles), Stage 3I (User Space & Syscall Interface).

---

## 1. Architectural Scope & Mission

Stage 3I established the first real ZeroOS user-space execution environment with fast system calls (`syscall`/`sysretq`) and freestanding user code.

Stage 3J introduces the **native ZeroOS ELF program-execution primitive**, transitioning from hard-coded payloads to validated, isolated user executables.

The execution pipeline is:

```text
ELF64 Binary Image (in-memory buffer)
           ↓
[Stage 1: ELF Header & Identity Validation]
           ↓
[Stage 2: Program Header & PT_LOAD Validation]
           ↓
[Stage 3: Process & AddressSpace Creation]
           ↓
[Stage 4: Segment Allocation, Copy & BSS Zeroing]
           ↓
[Stage 5: User Stack Allocation & Mapping]
           ↓
[Stage 6: Initial Thread & Context Construction]
           ↓
[Stage 7: Transaction Commit / Atomic Activation]
           ↓
Ring 3 User Execution
           ↓
Syscall Activity
           ↓
Process Termination (`sys_exit`) & Full Reclamation (PMM Neutral)
```

Stage 3J does **not** introduce Unix compatibility, dynamic linking, or generic agent abstractions. It establishes a deterministic, fail-closed, memory-neutral kernel program loader.

---

## 2. Subsystem Investigation & Ownership Architecture

Inspection of the frozen Stage 2 and Stage 3F contracts confirms:

1. **VMM / AddressSpace Ownership**:
   - `AddressSpace::destroy()` reclaims intermediate page tables (`PT`, `PD`, `PDPT`) and the PML4 root frame.
   - `AddressSpace::destroy()` **never** frees leaf data frames.
   - `unmap_page()` extracts the mapped leaf `PhysFrame` from the PTE, clears the entry, and returns the frame to the caller without freeing it.
2. **Leaf Frame Ownership**:
   - The entity that allocates a leaf frame retains ownership of that frame.
   - In Stage 3J, `PROCESS_MEMORY_MAPS[slot]` is the authoritative owner of all process-owned user leaf frames.
   - Process teardown (`reclaim_process_resources_locked`) unmaps each tracked leaf page, frees the leaf `PhysFrame` to PMM, and then invokes `AddressSpace::destroy()`.
   - **Zero double-free, zero leaks**: `AddressSpace` never touches leaf frames; `PROCESS_MEMORY_MAPS` never touches page-table frames.

---

## 3. Invariant Specifications (I-ELF-1 through I-ELF-18)

### I-ELF-1: Supported Executable Format & Header Validation
Stage 3J strictly and exclusively supports **64-bit x86-64 Little-Endian Executables**:
- `e_ident[EI_MAG0..=EI_MAG3] == [0x7F, b'E', b'L', b'F']` (0x7F 0x45 0x4C 0x46)
- `e_ident[EI_CLASS] == 2` (`ELFCLASS64`)
- `e_ident[EI_DATA] == 1` (`ELFDATA2LSB` — little endian)
- `e_ident[EI_VERSION] == 1` (`EV_CURRENT`)
- `e_ident[EI_OSABI] == 0` (`ELFOSABI_NONE` / `ELFOSABI_SYSV`) or `0xFF` (Standalone)
- `e_type == 2` (`ET_EXEC` — position-dependent executable image). Relocatable objects (`ET_REL`), shared objects / PIE (`ET_DYN`), and core dumps (`ET_CORE`) are **explicitly rejected**.
- `e_machine == 0x3E` (`EM_X86_64` — AMD x86-64 architecture).
- `e_version == 1` (`EV_CURRENT`).
- `e_ehsize == 64` (`sizeof(Elf64Ehdr)`).
- `e_phentsize == 56` (`sizeof(Elf64Phdr)`).
- `e_phnum >= 1 && e_phnum <= MAX_PROGRAM_HEADERS (16)`.
- `e_phoff.checked_add(e_phnum * 56) <= elf_image.len()`.

Any failure returns `Err(ElfError::InvalidHeader)`.

---

### I-ELF-2: Program Header Type Semantics
Every program header is processed according to strict rules:
- `PT_LOAD (1)`: Loadable segment (validated and mapped into user address space).
- `PT_PHDR (6)`: Program header table self-reference (validated: must lie within file bounds; ignored for mapping).
- `PT_NOTE (4)`: Metadata note (validated: file bounds check; ignored for mapping).
- `PT_GNU_STACK (0x6474E551)`: Stack execution flag. **Strictly enforced non-executable**: if `(p_flags & PF_X) != 0`, the ELF binary is **rejected** with `Err(ElfError::ExecutableStackRejected)`. ZeroOS unconditionally mandates non-executable stacks (`NX=1`).
- All other types (`PT_DYNAMIC`, `PT_INTERP`, `PT_SHLIB`, `PT_TLS`, `PT_GNU_RELRO`) are **rejected** with `Err(ElfError::UnsupportedProgramHeader)`.

---

### I-ELF-3: Strict PT_LOAD Validation & p_align Contract
For every `PT_LOAD` segment:
1. **File Bounds & Overflow**:
   - `p_offset.checked_add(p_filesz)` must not overflow `u64` and must be `<= elf_image.len()`.
2. **Memory Bounds & Overflow**:
   - `p_filesz <= p_memsz`.
   - `p_vaddr.checked_add(p_memsz)` must not overflow `u64`.
   - Validated end address: `p_vaddr + p_memsz <= USER_VA_MAX_EXCLUSIVE`.
   - Start address: `p_vaddr >= USER_VA_MIN`.
   - `p_vaddr` and `p_vaddr + p_memsz` must be canonical x86-64 user addresses.
3. **Alignment Contract (`p_align`)**:
   - `p_align` must be `0`, `1`, `4096`, or a power of two $\ge 4096$. Any non-power-of-two or unsupported alignment returns `Err(ElfError::InvalidAlignment)`.
   - If `p_align >= 4096`, congruence is enforced: `(p_vaddr % p_align) == (p_offset % p_align)`.
   - In all cases, 4 KiB page-offset congruence is enforced: `(p_vaddr % 4096) == (p_offset % 4096)`.
4. **Checked Page Rounding**:
   - Virtual page bounds are computed strictly using checked arithmetic:
     $$\mathtt{page\_start} = p\_vaddr \ \& \ !\mathtt{0xFFF}$$
     $$\mathtt{page\_end} = (p\_vaddr.checked\_add(p\_memsz)?.checked\_add(\mathtt{0xFFF})?) \ \& \ !\mathtt{0xFFF}$$
5. **Non-Intersection with User Stack Window**:
   - Loadable segments must **not** intersect the user stack and guard window:
     $$\big[\mathtt{page\_start},\; \mathtt{page\_end}\big) \cap \big[\mathtt{USER\_STACK\_GUARD},\; \mathtt{USER\_STACK\_TOP}\big) = \emptyset$$
6. **Segment Non-Overlap & Page Permission Conflict**:
   - All `PT_LOAD` segments must be strictly disjoint in virtual page spans:
     $$\big[\mathtt{page\_start}_A,\; \mathtt{page\_end}_A\big) \cap \big[\mathtt{page\_start}_B,\; \mathtt{page\_end}_B\big) = \emptyset$$
   - If two segments share any 4 KiB page or overlap in virtual space, the loader immediately rejects with `Err(ElfError::OverlappingSegments)` (preventing page-level permission conflicts).

---

### I-ELF-4: Strict W^X Mapping Policy
ELF program header permissions (`p_flags`):
- `PF_X = 0x1` (Execute)
- `PF_W = 0x2` (Write)
- `PF_R = 0x4` (Read)

ZeroOS enforces:
```text
I-ELF-WX-1:
No user page mapping may be simultaneously Writable and Executable.
If (p_flags & PF_W != 0) && (p_flags & PF_X != 0), the ELF binary is REJECTED.
```

Conversion table to `PageTableFlags`:

| `p_flags` | Permissions | ZeroOS `PageTableFlags` |
| :--- | :--- | :--- |
| `PF_R` | Read-Only | `PRESENT \| USER_ACCESSIBLE \| NO_EXECUTE` |
| `PF_R \| PF_W` | Read-Write | `PRESENT \| USER_ACCESSIBLE \| WRITABLE \| NO_EXECUTE` |
| `PF_R \| PF_X` | Read-Execute | `PRESENT \| USER_ACCESSIBLE` (NX=0, W=0) |
| `PF_W \| PF_X` | W+X | **REJECTED** (`ElfError::WwxViolation`) |
| `PF_R \| PF_W \| PF_X` | RWX | **REJECTED** (`ElfError::WwxViolation`) |
| Any without `PF_R` | No Read | **REJECTED** (`ElfError::InvalidPermissions`) |

---

### I-ELF-5: Entry Point Validation
The entry point `e_entry`:
1. Must be a canonical 64-bit virtual address.
2. Must satisfy: $\mathtt{USER\_VA\_MIN} \le e\_entry < \mathtt{USER\_VA\_MAX\_EXCLUSIVE}$.
3. Must fall within the virtual address span $[p\_vaddr, p\_vaddr + p\_memsz)$ of a valid `PT_LOAD` segment having `p_flags & PF_X != 0`.
4. After segment mapping, the page containing `e_entry` must be verified via `ActivePageTable::is_mapped()` and its flags must **not** contain `NO_EXECUTE`.
5. Violation results in `Err(ElfError::InvalidEntryPoint)`.

---

### I-ELF-6: Segment Memory Allocation, Copy & BSS Zeroing
For each valid `PT_LOAD` segment:
1. Spanned virtual page range: `page_start .. page_end` (4 KiB steps).
2. For each page:
   - Allocate 1 physical frame from PMM.
   - Record in temporary `AllocatedFrameTracker`.
   - Map into `AddressSpace` via `ActivePageTable::from_root(pml4_root).map_page(...)` with `MappingDomain::User` and flags from `I-ELF-4`.
   - Access frame via HHDM (`PhysicalAddress::new(frame.address()).to_hhdm()`).
   - Zero out the 4096-byte frame memory completely.
   - If the page overlaps with file-backed data $[p\_vaddr, p\_vaddr + p\_filesz)$:
     - Compute page-relative copy offsets and copy file data from the ELF buffer.
   - Any remaining bytes in the page beyond `p_filesz` up to `p_memsz` remain cleanly zeroed (BSS).

---

### I-ELF-7: Exact Frozen User Stack Contract
The user stack is defined by frozen architectural constants:
```rust
pub const USER_STACK_SIZE: u64        = 16 * 1024;             // 16 KiB = 4 pages
pub const USER_STACK_PAGES: usize     = 4;
pub const USER_STACK_TOP: u64         = 0x0000_7F7F_FFFF_0000; // 16-byte aligned
pub const USER_STACK_BASE: u64        = USER_STACK_TOP - USER_STACK_SIZE; // 0x0000_7F7F_FFFC_0000
pub const USER_STACK_GUARD: u64       = USER_STACK_BASE - 4096;           // 0x0000_7F7F_FFFB_F000
pub const USER_INITIAL_RSP: u64       = USER_STACK_TOP;
```

Stack layout:
```text
USER_STACK_GUARD (0x0000_7F7F_FFFB_F000)
  [4 KiB UNMAPPED GUARD PAGE]
USER_STACK_BASE  (0x0000_7F7F_FFFC_0000)
  [Page 0: 4 KiB RW+NX]
  [Page 1: 4 KiB RW+NX]
  [Page 2: 4 KiB RW+NX]
  [Page 3: 4 KiB RW+NX]
USER_STACK_TOP   (0x0000_7F7F_FFFF_0000) == USER_INITIAL_RSP
```
- Stack flags: `PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE`.
- Initial stack content: zeroed. No `argv`/`envp` ABI in Stage 3J (`argc = 0`, `argv = NULL`).
- `USER_INITIAL_RSP` is 16-byte aligned.

---

### I-ELF-8: Complete Transactional Rollback
```text
I-ELF-LOAD-TX-1:
A failed ELF load leaves:
- no process-owned leaf frames allocated,
- no process-owned mappings,
- no newly allocated page-table resources,
- no scheduler-visible user thread,
- no leaked process slot,
- no leaked capabilities/handles,
- no partially initialized execution image.
```

Rollback protocol:
If ANY error occurs prior to atomic activation:
1. For every leaf frame mapped into the new `AddressSpace`:
   - Call `unmap_page(page, pmm)`, clearing the PTE and reclaiming intermediate tables.
   - Call `pmm.free_frame(frame)`.
2. Destroy the `AddressSpace` via `address_space.destroy(pmm)` (frees intermediate tables and PML4 root).
3. If thread descriptor was allocated, free it and destroy its kernel stack.
4. Release the process slot in `PROCESS_TABLE`.
5. Release the memory map slot in `PROCESS_MEMORY_MAPS`.
6. **PMM Neutrality**: `baseline_free == post_failure_free` is strictly guaranteed.

---

### I-ELF-9: Bounded Memory Map Companion Table & Lifecycle
To preserve the frozen 128-byte `Process` ABI without dynamic allocation:
```rust
pub const MAX_PROCESS_MAPPED_PAGES: usize = 64; // Up to 256 KiB user memory per process

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapState {
    Free = 0,
    Allocating = 1,
    Active = 2,
    Reclaiming = 3,
}

pub struct ProcessMemoryMap {
    pub state: MemoryMapState,
    pub page_count: usize,
    pub pages: [(Page, PhysFrame); MAX_PROCESS_MAPPED_PAGES],
}

pub static mut PROCESS_MEMORY_MAPS: [ProcessMemoryMap; MAX_PROCESSES] =
    [const { ProcessMemoryMap::empty() }; MAX_PROCESSES];
```

Lifecycle Invariants:
- `I-ELF-MEMMAP-1`: `PROCESS_MEMORY_MAPS[slot]` is authoritative for all process-owned user leaf frames created by the ELF loader.
- `I-ELF-MEMMAP-2`: An entry exists in `Active` state if and only if the corresponding process slot is `Active`.
- `I-ELF-MEMMAP-3`: Every allocated leaf frame is recorded exactly once.
- `I-ELF-MEMMAP-4`: A leaf frame cannot be freed while its ownership record remains active.
- `I-ELF-MEMMAP-5`: Memory-map metadata is transitioned to `Reclaiming` during teardown, cleared, and set to `Free` only after all owned leaf frames and mappings are reclaimed.
- `I-ELF-MEMMAP-6`: Every page mapped into the process's user address space that is owned by the ELF loader is represented exactly once in `PROCESS_MEMORY_MAPS[slot]`. Every entry in `PROCESS_MEMORY_MAPS[slot]` corresponds to exactly one currently mapped process-owned user leaf frame.

```text
I-ELF-LOAD-TX-2:
Before commit, PROCESS_MEMORY_MAPS is in Allocating state.
After successful commit, it is Active.
Any failure before commit returns it to Free with zero retained
process-owned leaf frames and no retained user mappings.
```

---

### I-ELF-10: Leaf Frame Ownership & No-Double-Free Invariant
```text
I-ELF-MEM-OWNERSHIP-1:
After successful load commit:
1. PROCESS_MEMORY_MAPS owns every process-owned user leaf frame.
2. AddressSpace/VMM owns page-table structures and virtual mappings,
   but does NOT independently free those leaf frames.
3. Process teardown reclaims mapping ownership exactly once:
   unmap_page() -> pmm.free_frame(leaf) -> AddressSpace::destroy().
```

---

### I-ELF-11: Initial Execution Image & Ring 3 Entry
When the user thread begins execution in Ring 3:
- `RIP = e_entry`
- `RSP = USER_INITIAL_RSP` (`USER_STACK_TOP`)
- `CS = 0x23` (GDT entry 4, RPL 3)
- `SS = 0x1B` (GDT entry 3, RPL 3)
- `RFLAGS = 0x0202` (`IF=1`, reserved bit 1 = 1)
- All general-purpose registers (`RAX, RBX, RCX, RDX, RSI, RDI, RBP, R8..R15`) are cleared to 0.
- `GS_BASE` is set to 0; `IA32_KERNEL_GS_BASE` contains kernel `PerCpu`.

The transition executes via `create_user_thread()`, entering through `user_thread_bootstrap_trampoline` and `iretq`.

---

### I-ELF-12: Capability & Handle Authority
A loaded ELF executable begins with:
- A clean, initialized handle table (`PROCESS_HANDLE_TABLES[slot].count == 0`).
- A clean, empty capability node set (`CAPABILITY_NODE_TABLE[slot]`).
- No ambient authority. If initial capabilities (e.g. IPC channels) are provided by a parent process, they must be transferred explicitly via `TRANSFER_DELEGATE` or `TRANSFER_MOVE`.

---

### I-ELF-13: Strongly Typed Error Model
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    ImageTooSmall,
    InvalidMagic,
    UnsupportedClass,
    UnsupportedEndian,
    UnsupportedVersion,
    UnsupportedType,
    UnsupportedMachine,
    InvalidHeader,
    UnsupportedProgramHeader,
    ExecutableStackRejected,
    MalformedSegment,
    FileBoundsExceeded,
    VirtualAddressOverflow,
    OutOfUserBounds,
    InvalidAlignment,
    MisalignedOffsetCongruence,
    OverlappingSegments,
    PagePermissionConflict,
    WwxViolation,
    InvalidPermissions,
    InvalidEntryPoint,
    CapacityExceeded,
    OutOfMemory,
    MappingFailed,
    StackCreationFailed,
    ProcessCreationFailed,
    ThreadCreationFailed,
}
```

---

### I-ELF-14: Concurrency & Lock Hierarchy
The loader operates under strict monotonic lock order:
1. `validate_elf()` runs without holding any kernel locks.
2. `create_process()` acquires `SCHEDULER.lock`, claims slot, and releases lock.
3. Page allocation and mapping into the new `AddressSpace` runs using `pmm` and off-CR3 page table manipulation via HHDM (no scheduler lock held).
4. `create_user_thread()` claims thread descriptor and enqueues to scheduler under `SCHEDULER.lock`.
5. No circular lock dependencies exist.

---

### I-ELF-15: Explicit Out-of-Scope Exclusions
Stage 3J explicitly excludes:
- Position-Independent Executables (PIE / `ET_DYN`)
- Dynamic linking, shared libraries, and ELF interpreters (`PT_INTERP`)
- Relocations (`.rel`, `.rela`)
- Thread Local Storage (TLS / `PT_TLS`)
- Demand paging, page faults as program loader, and copy-on-write
- Unix `mmap` / `execve` replacement semantics
- Dynamic user stack growth

---

## 4. Verification & Test Matrix

Stage 3J establishes **16 machine verification tests (`3J-A` through `3J-P`)**:

| Test ID | Name | Invariants Verified |
| :--- | :--- | :--- |
| **3J-A** | Minimal Valid ELF64 Header & Identity Parse | `I-ELF-1` |
| **3J-B** | Bad Magic, Wrong Class, & Unsupported Architecture Rejection | `I-ELF-1` |
| **3J-C** | Unsupported Program Header & Executable Stack Rejection | `I-ELF-2` |
| **3J-D** | PT_LOAD File Bounds Exceeded Rejection | `I-ELF-3` |
| **3J-E** | PT_LOAD Memory Underflow (`filesz > memsz`) Rejection | `I-ELF-3` |
| **3J-F** | PT_LOAD Virtual Address Out of User Bounds Rejection | `I-ELF-3` |
| **3J-G** | PT_LOAD Virtual Address Overflow Rejection | `I-ELF-3` |
| **3J-H** | Overlapping PT_LOAD Segments Rejection | `I-ELF-3` |
| **3J-I** | Strict W^X Violation Rejection (RWX & W+X) | `I-ELF-4` |
| **3J-J** | Invalid Entry Point Rejection (Out of Bounds / Non-Executable) | `I-ELF-5` |
| **3J-K** | Segment Copy & BSS Zero Initialization Fidelity | `I-ELF-6` |
| **3J-L** | Guarded User Stack Allocation, Alignment & Permissions | `I-ELF-7` |
| **3J-M** | Real ELF Program Loading & Execution in Ring 3 | `I-ELF-10`, `I-ELF-11` |
| **3J-N** | Loaded ELF Syscall Invocation & Clean `sys_exit` | `I-ELF-11`, Stage 3I |
| **3J-O** | Failed ELF Load Immediate PMM Neutrality (Rollback) | `I-ELF-8`, `I-ELF-10` |
| **3J-P** | Complete Process Lifecycle Teardown & Full PMM Neutrality | `I-ELF-8`, `I-ELF-9`, `I-ELF-10` |

### Target Verification Totals:
- In-Kernel Machine Tests: 199 (Stage 3I) + 16 (Stage 3J) = **215 tests**
- Host Pytest Files: 26 + 1 ([`test_stage3j.py`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/tests/test_stage3j.py)) = **27 files**
- Host Pytest Items: 98 + 6 = **104 items**
- PMM Neutrality: `baseline_free == post_test_free` strictly enforced across success and failure.
