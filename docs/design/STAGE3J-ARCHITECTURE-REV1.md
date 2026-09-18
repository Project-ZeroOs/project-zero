# Stage 3J — ELF & Program Execution Architecture Specification

**Status**: 🟡 PENDING ARCHITECTURAL REVIEW (Rev1)  
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

## 2. Subsystem Investigation & Inventory

The existing frozen contracts interface with Stage 3J as follows:

| Subsystem | Existing API & ABI | Ownership & Locking | Failure & Lifetime Rules | Stage 3J Support Readiness |
| :--- | :--- | :--- | :--- | :--- |
| **Process** ([`process.rs`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/task/process.rs)) | `Process` descriptor (128 B, 8-B aligned), `create_process(parent_pid, pmm)` returns `ProcessHandle`. `PROCESS_TABLE[16]`. | Guarded by `SCHEDULER.lock` (`IF=0`). Caller owns handle. | Failed creation reclaims PML4. Lifecycles: `Creating -> Active -> Terminating -> Zombie -> Reclaiming -> Free`. | 🟢 Ready. Loader populates newly created process before scheduling. |
| **AddressSpace** ([`vmm.rs`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/mm/vmm.rs)) | `AddressSpace::new_user(pmm)` clones higher-half kernel PML4 [256..512] with `USER=0`, lower half [0..256] clean. | `AddressSpace` owned by `Process.address_space`. `destroy()` frees intermediate tables and PML4 root. | Catastrophic failure if `MASTER_KERNEL_PML4` destroyed. Checks active CR3 hazard. | 🟢 Ready. Leaf data frames must be unmapped/freed before or during `destroy()`. |
| **VMM Mapping** ([`vmm.rs`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/mm/vmm.rs)) | `ActivePageTable::from_root(pml4_root).map_page(page, frame, flags, domain, pmm)` and `unmap_page(page, pmm)`. | Page tables walked via HHDM (`phys_to_virt_table`). Empty tables auto-reclaimed on unmap. | `map_page` atomic rollback on OOM. `unmap_page` returns mapped `PhysFrame` to caller. | 🟢 Ready. Supports off-CR3 address space manipulation. |
| **PMM** ([`pmm.rs`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/mm/pmm.rs)) | `alloc_frame() -> Option<PhysFrame>`, `free_frame(PhysFrame) -> Result<(), PmmError>`. | Bitmap allocator (2 bits/frame). Rejects double-free, freeing reserved/unusable memory. | Exhaustion returns `None`. Neutrality: `baseline_free == post_test_free`. | 🟢 Ready. Transaction rollback list ensures zero frame leaks on error. |
| **User VA Policy** ([`numbers.rs`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/syscall/numbers.rs)) | `USER_VA_MIN = 0x0000_0000_0020_0000`, `USER_VA_MAX_EXCLUSIVE = 0x0000_7F80_0000_0000`. | Strictly checked by `validate_user_range`. | Out-of-bounds or non-canonical returns `SyscallError::BadAddress`. | 🟢 Frozen & Authoritative. ELF loader must enforce identical bounds. |
| **KernelThread** ([`thread.rs`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/task/thread.rs)) | `KernelThread` descriptor (176 B), `create_user_thread(proc, rip, rsp, prio, pmm, vmm)`. | Dedicated 16 KiB kernel stack. Descriptor in static `THREAD_TABLE[16]`. | Initial frame: `CooperativeFrame` returning to `user_thread_bootstrap_trampoline`. | 🟢 Ready. No `SavedFrameType::User` needed. |
| **Privilege Transition** ([`context.asm`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/boot/context.asm)) | `user_thread_bootstrap_trampoline`: pushes `SS=0x1B`, `RSP=user_rsp`, `RFLAGS=0x0202`, `CS=0x23`, `RIP=user_rip`, executes `iretq`. | Ring 3 transition. Kernel GS restored via `swapgs` on syscall/interrupt. | Fails closed. | 🟢 Ready. |
| **Capability Model** ([`cap/`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/cap/)) | `CAPABILITY_NODE_TABLE[16]`, `PROCESS_HANDLE_TABLES[16]`. Root caps, monotonic derivation, Option B reparenting. | Guarded by `KERNEL_OBJECT_TABLE_LOCK`. Scoped per process slot. | Initialized/scrubbed during `create_process`. Reclaimed on `process_exit`. | 🟢 Ready. Fresh loaded process begins with clean capability table. |
| **Teardown & Reclamation** ([`process.rs`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/kernel/src/task/process.rs)) | `reclaim_process_resources_locked`: reclaims thread stacks, resets CR3 if active, destroys address space. | `SCHEDULER.lock` held (`IF=0`). | Must free leaf frames before PML4 destruction to ensure frame neutrality. | 🟡 Requires leaf frame tracking in `Process` or tracked unmapping. |

---

## 3. Invariant Specifications (I-ELF-1 through I-ELF-18)

### I-ELF-1: Supported Executable Format & Magic Validation
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

### I-ELF-2: Supported Program Header Types
Only the following program header types are recognized:
- `PT_LOAD (1)`: Loadable segment (mapped into user address space).
- `PT_NOTE (4)`: Metadata (ignored, skipped).
- `PT_PHDR (6)`: Program header table reference (ignored, skipped).
- `PT_GNU_STACK (0x6474E551)`: Stack executable permissions note (must **not** have `PF_X`; ignored).

All other types (`PT_DYNAMIC`, `PT_INTERP`, `PT_SHLIB`, `PT_TLS`, `PT_GNU_RELRO`) are strictly **rejected** with `Err(ElfError::UnsupportedProgramHeader)`.

---

### I-ELF-3: Strict PT_LOAD Segment Validation
For every `PT_LOAD` segment:
1. **File Bounds**:
   - `p_offset.checked_add(p_filesz) <= elf_image.len()` (segment data must be fully within ELF buffer).
2. **Memory Bounds**:
   - `p_filesz <= p_memsz` (BSS portion is non-negative).
   - `p_vaddr.checked_add(p_memsz)` must not overflow `u64`.
3. **User Virtual Address Range**:
   - `p_vaddr >= USER_VA_MIN` ($0x0000\_0000\_0020\_0000$).
   - `p_vaddr + p_memsz <= USER_VA_MAX_EXCLUSIVE` ($0x0000\_7F80\_0000\_0000$).
   - `p_vaddr` must be canonical x86-64 user address.
4. **Congruence & Alignment**:
   - Page offset congruence: `(p_vaddr % 4096) == (p_offset % 4096)`.
5. **Non-Intersection with Reserved User Stack**:
   - Loadable segments must **not** intersect the user stack window:
     $$\big[p\_vaddr,\; p\_vaddr + p\_memsz\big) \cap \big[\mathtt{USER\_STACK\_BASE},\; \mathtt{USER\_STACK\_TOP}\big) = \emptyset$$
6. **Segment Non-Overlap**:
   - All `PT_LOAD` segments must be strictly disjoint in virtual address space.
   - If Segment $A$ and Segment $B$ have overlapping virtual page spans, the loader immediately rejects with `Err(ElfError::OverlappingSegments)`.

---

### I-ELF-4: Strict W^X Mapping Policy
ELF program header permissions (`p_flags`):
- `PF_X = 0x1` (Execute)
- `PF_W = 0x2` (Write)
- `PF_R = 0x4` (Read)

ZeroOS enforces the following invariant:
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
3. Must fall within the virtual address interval of a valid `PT_LOAD` segment having `p_flags & PF_X != 0`.
4. Must **not** point into an unmapped hole, stack, guard page, or data segment.

Violation results in `Err(ElfError::InvalidEntryPoint)`.

---

### I-ELF-6: Segment Memory Allocation, Copy & BSS Zeroing
For each valid `PT_LOAD` segment:
1. Determine the spanned virtual page range:
   $$\mathtt{start\_page} = p\_vaddr \ \& \ !\mathtt{0xFFF}$$
   $$\mathtt{end\_page} = (p\_vaddr + p\_memsz + \mathtt{0xFFF}) \ \& \ !\mathtt{0xFFF}$$
2. For each 4 KiB page in `start_page .. end_page`:
   - Allocate 1 physical frame from PMM.
   - Map into `AddressSpace` via `ActivePageTable::from_root(pml4_root).map_page(...)` with `MappingDomain::User` and flags from `I-ELF-4`.
   - Access frame via HHDM (`PhysicalAddress::new(frame.address()).to_hhdm()`).
   - Zero out the 4096-byte frame memory completely.
   - If the page overlaps with file-backed data `[p_vaddr .. p_vaddr + p_filesz)`:
     - Copy the corresponding slice from the ELF image into the frame.
   - Any memory in the page beyond `p_filesz` up to `p_memsz` remains cleanly zeroed (BSS).

---

### I-ELF-7: User Stack Construction
The initial user stack is constructed deterministically:
- Virtual address range: `[USER_STACK_BASE .. USER_STACK_TOP)` (16 KiB = 4 pages).
  - `USER_STACK_BASE = 0x0000_7F7F_FFFC_0000`
  - `USER_STACK_TOP  = 0x0000_7F7F_FFFF_0000`
- Guard page: `USER_STACK_BASE - 4096` ($0x0000\_7F7F\_FFFB\_F000$) is left strictly unmapped.
- Flags: `PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE`.
- Initial stack content: zeroed. In Stage 3J, no `argv` / `envp` ABI is passed on user stack (`argc = 0`, `argv = NULL`). Initial `RSP = USER_STACK_TOP` (16-byte aligned).

---

### I-ELF-8: Transactional Loading & Zero Memory Leakage
```text
I-ELF-TRANSACTION-1:
ELF loading is strictly transactional. If ANY validation, frame allocation,
page mapping, or thread creation step fails, the loader reclaims EVERY allocated
physical frame and page table before returning the error.
```

The loader maintains an `AllocatedFrameTracker` recording all allocated leaf frames:
- On success: the frame list is attached to the process image.
- On error: every recorded frame is unmapped and freed to PMM, and the `AddressSpace` is destroyed.
- **PMM Neutrality**: Under both successful load and deliberately failed load (malformed ELF, OOM, W^X breach), `baseline_free == post_load_free`.

---

### I-ELF-9: Process Descriptor & Tracking Integration
To guarantee complete PMM neutrality on process exit without altering the frozen 128-byte `Process` ABI:
- A static process memory tracking table is introduced:
  ```rust
  pub const MAX_PROCESS_MAPPED_PAGES: usize = 64; // Up to 256 KiB per user process in Stage 3
  pub struct ProcessMemoryMap {
      pub occupied: bool,
      pub page_count: usize,
      pub pages: [(Page, PhysFrame); MAX_PROCESS_MAPPED_PAGES],
  }
  pub static mut PROCESS_MEMORY_MAPS: [ProcessMemoryMap; MAX_PROCESSES] = ...;
  ```
- When `reclaim_process_resources_locked` reclaims a terminating process, it unmaps and frees all leaf frames recorded in `PROCESS_MEMORY_MAPS[slot]`, then calls `AddressSpace::destroy()`.
- The 128-byte `Process` ABI remains **100% frozen and unmodified**.

---

### I-ELF-10: Initial Execution Image & Ring 3 Entry
When the user thread begins execution in Ring 3:
- `RIP = e_entry`
- `RSP = USER_STACK_TOP`
- `CS = 0x23` (GDT entry 4, RPL 3)
- `SS = 0x1B` (GDT entry 3, RPL 3)
- `RFLAGS = 0x0202` (`IF=1`, reserved bit 1 = 1)
- All general-purpose registers (`RAX, RBX, RCX, RDX, RSI, RDI, RBP, R8..R15`) are cleared to 0.
- `GS_BASE` is set to 0; `IA32_KERNEL_GS_BASE` contains kernel `PerCpu`.

The transition executes via `create_user_thread()`, entering through `user_thread_bootstrap_trampoline` and `iretq`.

---

### I-ELF-11: Capability & Handle Authority
A loaded ELF executable begins with:
- A clean, initialized handle table (`PROCESS_HANDLE_TABLES[slot].count == 0`).
- A clean, empty capability node set (`CAPABILITY_NODE_TABLE[slot]`).
- No ambient authority. If initial capabilities (e.g. IPC channels) are provided by a parent process, they must be transferred explicitly via `TRANSFER_DELEGATE` or `TRANSFER_MOVE`.

---

### I-ELF-12: Error Model
The loader returns strongly typed, unambiguous errors:
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
    MalformedSegment,
    FileBoundsExceeded,
    VirtualAddressOverflow,
    OutOfUserBounds,
    UnalignedVirtualAddress,
    MisalignedOffsetCongruence,
    OverlappingSegments,
    WwxViolation,
    InvalidPermissions,
    InvalidEntryPoint,
    OutOfMemory,
    MappingFailed,
    StackCreationFailed,
    ProcessCreationFailed,
    ThreadCreationFailed,
}
```

---

### I-ELF-13: Concurrency & Lock Hierarchy
The loader operates under strict monotonic lock order:
1. `validate_elf()` runs without holding any kernel locks.
2. `create_process()` acquires `SCHEDULER.lock`, claims slot, and releases lock.
3. Page allocation and mapping into the new `AddressSpace` runs using `pmm` and off-CR3 page table manipulation via HHDM (no scheduler lock held).
4. `create_user_thread()` claims thread descriptor and enqueues to scheduler under `SCHEDULER.lock`.
5. No circular lock dependencies exist.

---

### I-ELF-14: Explicit Out-of-Scope Exclusions
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
| **3J-C** | Unsupported Program Header Type Rejection | `I-ELF-2` |
| **3J-D** | PT_LOAD File Bounds Exceeded Rejection | `I-ELF-3` |
| **3J-E** | PT_LOAD Memory Underflow (`filesz > memsz`) Rejection | `I-ELF-3` |
| **3J-F** | PT_LOAD Virtual Address Out of User Bounds Rejection | `I-ELF-3` |
| **3J-G** | PT_LOAD Virtual Address Overflow Rejection | `I-ELF-3` |
| **3J-H** | Overlapping PT_LOAD Segments Rejection | `I-ELF-3` |
| **3J-I** | Strict W^X Violation Rejection (RWX & W+X) | `I-ELF-4` |
| **3J-J** | Invalid Entry Point Rejection (Out of Bounds / Non-Executable) | `I-ELF-5` |
| **3J-K** | Segment Copy & BSS Zero Initialization Fidelity | `I-ELF-6` |
| **3J-L** | Guarded User Stack Allocation & Permissions | `I-ELF-7` |
| **3J-M** | Real ELF Program Loading & Execution in Ring 3 | `I-ELF-10` |
| **3J-N** | Loaded ELF Syscall Invocation & Clean `sys_exit` | `I-ELF-10`, Stage 3I |
| **3J-O** | Failed ELF Load Immediate PMM Neutrality | `I-ELF-8` |
| **3J-P** | Complete Process Lifecycle Teardown & Full PMM Neutrality | `I-ELF-8`, `I-ELF-9` |

### Target Verification Totals:
- In-Kernel Machine Tests: 199 (Stage 3I) + 16 (Stage 3J) = **215 tests**
- Host Pytest Files: 26 + 1 ([`test_stage3j.py`](file:///C:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/tests/test_stage3j.py)) = **27 files**
- Host Pytest Items: 98 + 6 = **104 items**
- PMM Neutrality: `baseline_free == post_test_free` strictly enforced.
