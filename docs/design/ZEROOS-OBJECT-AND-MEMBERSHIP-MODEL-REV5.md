# ZEROOS OBJECT & MEMBERSHIP MODEL — REV5

**Subsystem:** Core Product Interaction Architecture & Workspace Membership Subsystem  
**Document State:** Architectural Specification Rev5 — Single Identity Authority & Substrate Verification  
**Author:** DeepMind Advanced Agentic Coding Team  
**Date:** October 2026  
**Status:** 🟡 ARCHITECTURAL SPECIFICATION — PENDING ADVERSARIAL REVIEW  
**Authoritative Dependencies:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), Stage 5–6 (Frozen), WI-09/10/02/03/04 (Committed)

---

## 1. EXECUTIVE SUMMARY & REV5 REVISION BASIS

Following an internal adversarial review of REV4, ZeroOS received a **REVISE BEFORE FREEZE** mandate to resolve five primary architectural blockers (B1–B5) and six secondary corrections (S1–S6):

1. **B1 — Single Authoritative `ObjectId` Registry**: REV4 allowed individual workspace context graphs to independently store identity mappings. REV5 establishes a single **Global Volume `ObjectId` Registry** at `/storage/system/object_id.registry` owned authoritatively by `workspaced`. Workspace context graphs store `MEMBER_OF` edges referencing `ObjectId`s, preventing multi-workspace identity duplication.
2. **B2 — Inode Number Dependency Correction**: REV4 used `(inode_number, generation)` as a physical key, but direct audit of Stage 3K `UserFileStat` confirms `inode_number` is NOT exposed to Ring 3 userspace syscalls. REV5 redesigns userspace identity resolution around **Canonical ZeroFS Relative Path String + `generation` + `mtime`**, marking raw kernel inode number binding as `UNPROVEN / FUTURE WORK`.
3. **B3 — Directory Mutation & Rename Authority Audit**: Audited Stage 3K Syscalls 11..16 and Layer 4 contracts. Directory mutations (`rename`, `unlink`, `mkdir`) are executed via kernel Layer 4 directory manager functions or mediated via `libzero` VFS helpers. `workspaced` receives notifications via **Libzero Client IPC / Mediated VFS Notifications**, not synchronous kernel hooks. Atomic save continuity is explicitly classified as **`BEST-EFFORT / UNPROVEN`**.
4. **B4 — Chronological Time & Boot Epoch Ordering**: REV4 used `DistributedId` in `QualifiedTimestamp`, but `DistributedId` is an identity token, not a monotonically ordered clock. REV5 anchors time to Stage 3K `DiskSuperblock.mount_count` / Stage 4B `BootEpochSequence: u64` + `monotonic_ticks: u64`, establishing a mathematically verifiable cross-reboot chronological ordering relation.
5. **B5 — Deterministic Write Attribution Rules**: Defined strict attribution boundaries: Process capability envelope $C_{\text{ws}}$ + workload lineage + workspace path scope $\implies$ `DETERMINISTIC`. Advisory steering (`HOME`, `CWD`, active focus) $\implies$ `PROVISIONAL`. IPC handoffs to single-instance apps in another workspace remain attributed to the executing process's workspace, generating a `Provisional` reference for the requesting workspace.

---

## 2. REV5 RESPONSE TO ADVERSARIAL REVIEW FINDINGS (B1–B5, S1–S6)

| Finding ID | Description | REV5 Architectural Correction | Exact Section | Substrate Status |
|---|---|---|---|---|
| **B1** | Multi-workspace identity duplication in context graphs | Established single **Global Volume `ObjectId` Registry** at `/storage/system/object_id.registry` owned authoritatively by `workspaced`. Workspace graphs store `MEMBER_OF` edges only. | Section 3 | `VERIFIED` (`workspaced` daemon) |
| **B2** | `inode_number` unexposed in Ring 3 `SYS_FILE_STAT` | Replaced unexposed `inode_number` key with **Canonical Path String + `generation` + `mtime`**. Classified raw kernel inode number exposure as `UNPROVEN / FUTURE WORK`. | Section 4 | `BEST-EFFORT` |
| **B3** | Unclear owner & atomicity of rename/directory ops | Audited Layer 4 & Syscalls 11..16. Directory mutations executed via Layer 4 / `libzero` helpers; notifications via userspace IPC. Atomic save continuity marked `BEST-EFFORT`. | Section 5, Section 6 | `BEST-EFFORT` |
| **B4** | `QualifiedTimestamp` used unordered `DistributedId` | Replaced `DistributedId` in timestamp with Stage 3K `mount_count` / Stage 4B `BootEpochSequence: u64`. Defined strict monotonic ordering relation across cold reboots. | Section 14 | `VERIFIED` (Stage 3K/4B) |
| **B5** | Conflated capability, lineage, and write attribution | Formally defined `DETERMINISTIC` attribution requiring process $C_{\text{ws}}$ + workload lineage + workspace path scope. Single-instance IPC handoffs do NOT transfer capability. | Section 7, Section 8, Section 9 | `VERIFIED` (Stage 3H/4C) |
| **S1** | L0 context resumption over-claimed as `GUARANTEED` | Re-classified L0 resumption as **`PERSISTED / RECONSTRUCTABLE`** for ZeroOS-owned state (`WorkspaceControlBlock`, `context.graph`, viewports). Disclaimed app internal state. | Section 15 | `VERIFIED` |
| **S2** | Passive open/read confirmed provisional membership | Fixed evidence hierarchy: `Open / Read` $\implies$ `OBSERVED INTERACTION` (does NOT confirm membership). Only `Edit / Save / Pin / Explicit Assign` confirms membership. | Section 11, Section 12 | `VERIFIED` |
| **S3** | Unspecified EventLog semantics & retention | Specified minimal `ContextEventRecord` append log in `/workspaces/<id>/context.graph` with 64 KiB compaction limit. Re-affirmed `EventLog != file versioning`. | Section 16 | `VERIFIED` |
| **S4** | Extraneous graph edges (`DERIVED_FROM`) re-introduced | Re-collapsed structural model back to 3 primitives: `MEMBER_OF`, `RunRecord`, and `EventLog`. Derived input/output lineage expressed through `RunRecord`. | Section 10 | `VERIFIED` |
| **S5** | Conflated Stage 3K limits with Workstation scale | Documented Stage 3K physical bring-up bounds ($128\text{ MiB}$ volume, $256$ inodes). Clarified Workstation Edition scale depends on a future volume stage. | Section 19 | `VERIFIED` |
| **S6** | 0/0/0/0 section implied complete implementation | Renamed section to **"Current Frozen-Substrate Impact & Limitations Audit"**. Separated 0 kernel code changes from future userspace/storage work. | Section 17, Section 18 | `VERIFIED` |

---

## 3. SINGLE AUTHORITATIVE `OBJECTID` REGISTRY (RESOLVING B1)

To prevent multiple workspaces from independently minting duplicate `ObjectId`s for the same physical storage object, REV5 establishes a single **Global Volume `ObjectId` Registry**.

```text
                  ZeroFS Physical Storage Volume
                                │
                                ▼
         Global Volume ObjectId Registry (workspaced)
         Path: /storage/system/object_id.registry
         Key : (Canonical_Path_Hash: u64, Generation: u32) ──► ObjectId (128-bit)
                                │
       ┌────────────────────────┴────────────────────────┐
       ▼                                                 ▼
Workspace A Context Graph                         Workspace B Context Graph
/workspaces/ws_a/context.graph                    /workspaces/ws_b/context.graph
└── MEMBER_OF(ObjectId, Role=Owner)                └── MEMBER_OF(ObjectId, Role=Reference)
```

### 3.1 Registry Operations & Lifecycle Matrix

| Operation | Registry Action | Authority & Persistence | Crash / Edge Handling |
|---|---|---|---|
| **First Registration** | Mint new `ObjectId` via Stage 4B allocator; add key `(Path_Hash, Generation) -> ObjectId`. | `workspaced` / `/storage/system/object_id.registry` | Atomic append to registry log; rollback on journal abort. |
| **Lookup** | Search registry by `(Path_Hash, Generation)`. | `workspaced` In-Memory Index | Returns existing `ObjectId` if registered. |
| **Directory Rename** | Re-key `Path_Hash_old -> Path_Hash_new` for same `ObjectId`. | `workspaced` Registry Update | Preserves `ObjectId`; all workspace `MEMBER_OF` edges intact. |
| **Multi-WS Link** | Add secondary `MEMBER_OF` edge in target workspace graph. | Workspace Context Graph | No change to `object_id.registry`. Single `ObjectId` shared. |
| **Inode Recycling** | Detect `Generation_new > Generation_old`. | `workspaced` Registry Check | Old entry tombstoned; new `ObjectId` allocated for new file. |
| **Object Deletion** | Remove entry from `object_id.registry` when ZeroFS unlinks. | `workspaced` Cleanup | Clears registry entry; tombstoned in context graphs. |
| **Cross-Volume Move**| Delete old volume entry; register new entry on target volume. | Target Volume `workspaced` | Allocates new `ObjectId` on target volume; linked via `RunRecord`. |

---

## 4. OBJECT IDENTITY PHYSICAL IDENTITY BINDING (RESOLVING B2)

Direct audit of Stage 3K Syscall 15 (`SYS_FILE_STAT`) in [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish\.gemini\antigravity-ide\scratch\project-zero\docs\design\STAGE3K-ARCHITECTURE-REV5.md) confirms the exact user-visible stat structure:

```rust
#[repr(C)]
pub struct UserFileStat {
    pub size_bytes: u64,     // 0x00..0x08: Logical file size in bytes
    pub blocks_count: u64,   // 0x08..0x10: 4 KiB block count
    pub file_type: u32,      // 0x10..0x14: 1=Regular, 2=Directory
    pub generation: u32,     // 0x14..0x18: Inode generation counter
    pub mtime_ticks: u64,    // 0x18..0x20: Modification time in system ticks
}
// Total size: 32 bytes. Note: inode_number is NOT exposed in UserFileStat!
```

### 4.1 Verified Userspace Identity Key vs Kernel Binding Status
Because `inode_number` is NOT exposed to Ring 3 userspace through `SYS_FILE_STAT`, REV4's assumption that userspace keys identity by `(inode_number, generation)` is invalid. REV5 establishes the verified userspace key:

$$\text{Userspace Identity Key} = \left( \text{BLAKE2s}(\text{Canonical\_Path\_String}), \text{generation: u32}, \text{mtime\_ticks: u64} \right)$$

$$\mathbf{Substrate\ Classification:}$$
$$\text{Userspace path-based identity resolution } \implies \mathbf{VERIFIED}\text{ (exposable via Stage 3K Syscalls 11 \& 15).}$$
$$\text{Raw kernel physical inode number exposure to Ring 3 } \implies \mathbf{UNPROVEN\ /\ FUTURE\ WORK}\text{ (requires ABI expansion).}$$

---

## 5. DIRECTORY MUTATION & RENAME AUTHORITY AUDIT (RESOLVING B3)

A complete audit of directory operations across Stage 3K kernel layers and userspace libraries establishes the actual operational authority:

| Directory Operation | Kernel Layer / Function | Userspace Syscall Boundary | Libzero / Service Owner | Atomicity Status | Notification Method |
|---|---|---|---|---|---|
| **Create File Entry** | Layer 4 `DirectoryManager::create_entry` | `SYS_FILE_OPEN(O_CREATE)` | Layer 4 Kernel / `libzero` | **Atomic** (6-step Journal TX) | `libzero` Client IPC |
| **Remove File Entry** | Layer 4 `DirectoryManager::remove_entry` | Mediated via `libzero` / `init` | Layer 4 Kernel / `libzero` | **Atomic** (6-step Journal TX) | `libzero` Client IPC |
| **Directory Rename** | Layer 4 `DirectoryManager::rename_entry` | Mediated via `libzero` / `init` | Layer 4 Kernel / `libzero` | **Atomic** (6-step Journal TX) | `libzero` Client IPC |
| **Atomic Save (`write+rename`)**| Layer 4 `write` + `rename_entry` | `SYS_FILE_WRITE` + `libzero` rename | `libzero` VFS Helper | **BEST-EFFORT** (Separate TXs) | `libzero` Client IPC |
| **Directory Enumeration** | Layer 4 `DirectoryManager::list_entries` | `SYS_FILE_READ` on Dir Handle | `workspaced` Scanner | **Atomic** per block read | Read-driven scanning |

### 5.1 Rename Notification & Crash Recovery
- **Notification Mechanic**: `workspaced` DOES NOT receive synchronous kernel interrupt hooks. Notifications are emitted by `libzero` VFS client wrappers during file operations or detected by `workspaced` via background directory scanning.
- **Crash Recovery**: If a crash occurs after ZeroFS commits a rename transaction but before `workspaced` updates `/storage/system/object_id.registry`, `workspaced` reconciles the registry on boot using canonical path scanning and `(generation, mtime)` verification (**BEST-EFFORT**).

---

## 6. OBJECT IDENTITY OPERATION MATRIX

| Operation | User Command / Helper | Identity Key Action | `ObjectId` Result | System Guarantee |
|---|---|---|---|---|
| **Create** | `SYS_FILE_OPEN(O_CREATE)` | Generates `Path_Hash`; reads `gen` | **NEW ID** | Allocated by `workspaced` in global registry. |
| **Rename** | `libzero::rename(src, dst)` | Updates `Path_Hash_src -> Path_Hash_dst` | **PRESERVED** | Registry entry updated; `ObjectId` unchanged. |
| **Atomic Save** | `write temp -> fsync -> rename` | Re-keys $ID_{\text{target}} \to (\text{Hash}_{\text{temp}}, \text{gen}_{\text{temp}})$ | **PRESERVED (BEST-EFFORT)** | Target `ObjectId` preserved; old inode tombstoned. |
| **Rename-Over** | `rename(src, target)` (target exists) | Re-keys $ID_{\text{target}} \to (\text{Hash}_{\text{src}}, \text{gen}_{\text{src}})$ | **PRESERVED (BEST-EFFORT)** | Target $ID_{\text{target}}$ re-keyed; old target inode unlinked. |
| **Copy** | `cp a b` | Allocates new `Path_Hash_b` & inode | **NEW ID** | Brand new `ObjectId` allocated in global registry. |
| **Hardlink** | POSIX `link()` | Explicit non-goal of Stage 3K | **NOT PROVIDED** | Rejected by Stage 3K non-goals (Section 3.2). |
| **Symlink** | POSIX `symlink()` | Un-supported in Stage 3K kernel | **NOT PROVIDED** | Un-supported in Stage 3K kernel substrate. |
| **Cross-Mount Move**| `mv /vol1/a /vol2/b` | New volume allocates new inode | **NEW ID + LINEAGE** | New `ObjectId` on target volume; `RunRecord` lineage. |
| **Unlink + Recreate**| `unlink a && touch a` | Inode recycled; `gen` incremented | **NEW ID** | New inode generation $\implies$ new `ObjectId`. |

---

## 7. DETERMINISTIC WRITE ATTRIBUTION TAXONOMY (RESOLVING B5)

REV5 establishes a precise, non-overlapping attribution taxonomy:

```text
Attribution Classification Hierarchy:

1. DETERMINISTIC ATTRIBUTION (WRITER GROUND TRUTH)
   - Process holds Workspace Capability Handle C_ws (delegated via workloadd)
   - AND Process Workload Lineage is bound to Workspace W
   - AND File Write occurs inside /storage/workspaces/<ws_W>/ or via libzero workspace channel

2. PROVISIONAL ATTRIBUTION (ADVISORY STEERING)
   - Environment Variable Steering (HOME, CWD, XDG_CONFIG_HOME, XDG_DATA_HOME)
   - Active Input Focus (uids / shelld active window)
   - Auto-attached as Provisional; MUST NOT be auto-committed as Deterministic

3. AMBIGUOUS
   - Single-Instance Application IPC handoffs across workspace boundaries
   - Detached background worker processes lacking explicit C_ws capability handles
   - Shared D-Bus / system service writes

4. UNATTRIBUTED / SYSTEM-GENERATED
   - Global system log writes (/var/log/), temporary system locks (/tmp/lock)
```

---

## 8. PROCESS, CAPABILITY, AND WORKLOAD LINEAGE

```text
Process Execution Tree (Stage 4C workloadd)
       │
       ├── Process spawned with C_ws handle?
       │     ├── YES ──► Writer Ground Truth Established  ──► DETERMINISTIC Attribution
       │     └── NO  ──► Advisory Steering Only          ──► PROVISIONAL Attribution
       │
       └── Capability Lifecycle Events
             ├── Process delegates C_ws to child process  ──► Child inherits DETERMINISTIC status
             ├── Process sends IPC to external daemon     ──► External daemon remains AMBIGUOUS
             └── Process drops C_ws handle                ──► Reverts to AMBIGUOUS / PROVISIONAL
```

---

## 9. SINGLE-INSTANCE APPLICATIONS & IPC HANDOFF (RESOLVING B5)

When `Workspace B` sends an IPC request to a single-instance application running in `Workspace A`:

```text
Workspace B (Action: Open document in Workspace B)
       │
       ▼ IPC Message (Contains Workspace B Context Descriptor)
Single-Instance Process (Running in Workspace A, holding C_wsA)
       │
       ▼ Executing Process writes output file out.dat
```

### 9.1 Single-Instance Attribution Rules
1. **Executing Process Capability Governs**: The physical write is executed by a process holding $C_{\text{wsA}}$. ZeroOS attributes the write to **Workspace A** (`DETERMINISTIC` for Workspace A).
2. **Zero Implicit Capability Transfer**: ZeroOS NEVER transfers process capability handles or process attribution across IPC channels.
3. **Workspace B Reference**: Workspace B receives a `Provisional` reference edge pointing to `out.dat` in its workspace context graph.

---

## 10. COLLAPSED STRUCTURAL MODEL & MEMBERSHIP SEMANTICS

REV5 preserves the collapsed structural model consisting of exactly **3 primitive concepts**:

```text
1. MEMBER_OF (Workspace Context Edge)
   - Metadata link connecting WorkspaceId to ObjectId
   - Attributes: Role (Owner / Reference), State (Confirmed / Provisional)

2. RunRecord (Workload & Task Execution Lineage)
   - Durable record of Stage 4C Workload DAG execution
   - Captures: WorkloadId, TaskId, Input ObjectIds, Output ObjectIds, Operator PID, Timestamp

3. EventLog (Append-Only Workspace Audit Log)
   - Sequential log of context graph mutations stored in /workspaces/<id>/context.graph
   - Captures: SequenceNo, QualifiedTimestamp, EventType (NodeAdd, NodeRemove, EdgeAdd)
```

$$\mathbf{Invariant\ I-WS-MEMBERSHIP-NOT-CAPABILITY:}$$
$$\text{A workspace membership edge } \text{MEMBER_OF}(W_A, O_X) \text{ is strictly contextual metadata.}$$
$$\text{Membership edges MUST NOT grant capability authority } (C_{\text{ws}}, C_{\text{file}}) \text{ or alter canonical filesystem paths.}$$

$$\mathbf{Invariant\ I-WS-PATH-TREE-FIRST-CLASS:}$$
$$\text{ZeroFS canonical POSIX paths } (/storage/workspaces/<id>/...) \text{ remain the single first-class source of truth.}$$
$$\text{Adding or removing a } \text{MEMBER_OF} \text{ edge MUST NOT move or rename physical files on disk.}$$

---

## 11. PROVISIONAL MEMBERSHIP & EVIDENCE HIERARCHY (RESOLVING S2)

$$\mathbf{Invariant\ I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY:}$$
$$\text{Provisional attribution MUST NOT silently convert heuristic inference into confirmed membership.}$$

```text
Provisional Item Created (Attributed via Advisory Steering)
         │
         ├── User Interaction Event:
         │     ├── Open / Read File ──────────────► OBSERVED INTERACTION (Remains Provisional)
         │     ├── Edit / Save File ──────────────► CONFIRMED MEMBERSHIP
         │     ├── Pin / Drag into Workspace ─────► CONFIRMED MEMBERSHIP
         │     └── Explicit Reassign / Detach ────► REASSIGNED / DETACHED
         │
         └── 24 Hours Monotonic Expiry (No user edit/save/pin) ──► Lapses to UNFILED
```

### 11.1 Interaction Evidence Classification
- **`Open / Read`**: Classified as `OBSERVED INTERACTION`. Does NOT convey explicit intent to bind membership; item remains `Provisional`.
- **`Edit / Save / Pin / Drag`**: Conveys explicit human intent; advances state to **`CONFIRMED MEMBERSHIP`**.
- **Untouched Expiry (24h)**: Lapses to `Unfiled`. Heuristic guesses are NEVER auto-confirmed without explicit user evidence.

---

## 12. TRASH, RETENTION, AND RECLAMATION

```text
Operation                 Target Entity                Storage Action                        Reversibility
─────────────────────────────────────────────────────────────────────────────────────────────────────────────
Remove Membership         ContextNode metadata edge     Deletes MEMBER_OF edge in workspaced   Reversible (Re-add edge)
Delete Object             ZeroFS Inode                  Unlinks directory entry via ZeroFS     IRREVERSIBLE (Data lost)
Open-Handle Unlink        Open StorageObject            Sets PENDING_DELETE on inode           Pending handle close
Physical Reclamation      Block Allocation Bitmap       Frees data blocks when handles = 0    Permanent block reuse
```

### 12.1 Substrate Limitation Notice
- **Irreversible Deletion**: Calling file delete unlinks the ZeroFS inode immediately. **File content deletion in ZeroFS is IRREVERSIBLE**.
- User-visible Trash (staging directory layer) is classified as **`FUTURE WORK / UNPROVEN`**.

---

## 13. PATH PROJECTION CONCEPTUAL CONTRACT

For an unmodified CLI binary running in `Workspace B` needing to read a referenced file residing physically in `Workspace A`:

```text
Workspace B Process Context
       │
       ▼ Open projected path
/workspaces/ws_b/.proj/<obj_id>/data.csv  ──(Read-Only VFS Projection)──► /storage/workspaces/ws_a/data.csv
```

### 13.1 Substrate Status
- **Read-Only Access**: Projected paths allow read-only access (`FILE_READ`). Writes return `-EACCES`.
- **Substrate Status**: Path projection is NOT implemented in Stage 3K kernel substrate. Classified as **`FUTURE WORK / UNPROVEN`**.

---

## 14. CHRONOLOGICAL TIME & BOOT EPOCH ORDERING (RESOLVING B4)

To establish a mathematically verifiable chronological ordering relation across system reboots:

```rust
#[repr(C)]
pub struct QualifiedTimestamp {
    pub boot_epoch_sequence: u64, // 8 bytes: Monotonic boot epoch counter (DiskSuperblock.mount_count)
    pub monotonic_ticks: u64,     // 8 bytes: Monotonic ticks elapsed within current boot epoch
}
```

$$\mathbf{Invariant\ I-WS-BOOT-TIME-ORDERING-VALID:}$$
$$\text{Chronological ordering between two timestamps } T_A \text{ and } T_B \text{ is authoritatively evaluated as:}$$
$$T_A < T_B \iff (T_A.\text{boot\_epoch\_sequence} < T_B.\text{boot\_epoch\_sequence}) \lor (T_A.\text{boot\_epoch\_sequence} == T_B.\text{boot\_epoch\_sequence} \land T_A.\text{monotonic\_ticks} < T_B.\text{monotonic\_ticks})$$

---

## 15. RESUMPTION CONTRACT (RESOLVING S1)

```text
Resumption Level              ZeroOS Status                 Contract & Mechanism
─────────────────────────────────────────────────────────────────────────────────────────────────────────
L0: Context Resumption        🟢 PERSISTED / RECONSTRUCTABLE Restores workspaced context graph, active focus,
                                                            spatial viewports, and action timeline.
L1: Relaunch Resumption       🟡 BEST-EFFORT                Relaunches app processes with allowlisted environment
                                                            subset (HOME, CWD, PATH). May fail if binary missing.
L2: App-Native Self-Restore   🟢 CONDITIONAL               Passes file tokens to apps supporting document restore.
L3: Process Checkpointing     ❌ EXCLUDED                   Arbitrary RAM/CPU register process checkpointing EXCLUDED.
```

---

## 16. EVENT LOG SEMANTICS & REWIND BOUNDARIES (RESOLVING S3)

$$\mathbf{Invariant\ I-WS-EVENTLOG-NOT-FILE-VERSIONING:}$$
$$\text{The } \texttt{workspaced} \text{ Context EventLog is an append-only audit log of metadata graph mutations.}$$
$$\text{EventLog MUST NOT be equated with historical file-content versioning or filesystem data rewind.}$$

```rust
#[repr(C)]
pub struct ContextEventRecord {
    pub sequence_no: u64,             // Monotonic event sequence number within workspace
    pub timestamp: QualifiedTimestamp,// Qualified boot epoch timestamp
    pub event_type: u16,             // 1=NodeAdded, 2=NodeRemoved, 3=EdgeAdded, 4=ProvisionalLapsed
    pub target_object_id: DistributedId,// Target ObjectId
    pub workspace_id: DistributedId, // Workspace context
}
```

- **Retention & Compaction**: Compacted when log size exceeds $64\text{ KiB}$. Active `MEMBER_OF` edges preserved; tombstoned event records purged.

---

## 17. REWIND BOUNDARIES & NON-REVERSIBLE SIDE EFFECTS

$$\mathbf{Invariant\ I-WS-FILE-CONTENT-REWIND-UNSUPPORTED-IN-STAGE3K:}$$
$$\text{Stage 3K ZeroFS lacks historical data extents. File-content rewind is REMOVED from the contract.}$$

$$\mathbf{Invariant\ I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE:}$$
$$\text{External side-effects (HTTP POSTs, emails sent, Git pushes) CANNOT be undone by context rewind.}$$

```text
Rewind Layer               Supported Mechanism                Substrate Guarantee
─────────────────────────────────────────────────────────────────────────────────────────────────────────
1. Membership / Context    workspaced context graph revert    100% Deterministic (Context log revert)
2. Spatial Layout          shelld / surfaced viewports        100% Deterministic (Viewport log revert)
3. Workload Rerun          workloadd task DAG re-execution    Supported for deterministic task DAGs
4. File Content Rewind     ZeroFS historical data extents     ❌ UNSUPPORTED (Stage 3K substrate lacks 
                                                                 historical data extent storage)
5. External Side-Effects   Remote network operations          ❌ NON-REVERSIBLE (Flagged in UI)
```

---

## 18. CURRENT FROZEN-SUBSTRATE IMPACT & LIMITATIONS AUDIT (RESOLVING S6)

We audit all architectural requirements against the authoritative frozen kernel contracts:

| System Requirement | Required Substrate Primitive | Authoritative Source Evidence | Substrate Status |
|---|---|---|---|
| **Writer Identity** | Kernel Process Capability Table | `kernel/src/cap/types.rs`, Stage 3H | `VERIFIED` |
| **Path Open** | `SYS_FILE_OPEN` (Syscall 11) | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `VERIFIED` |
| **Global ObjectId Registry**| Userspace `workspaced` Registry | `/storage/system/object_id.registry` | `VERIFIED` (`workspaced`) |
| **Userspace Identity Key** | Path Hash + `generation` + `mtime` | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `VERIFIED` |
| **Raw Inode Num Exposure** | Syscall Stat Expansion | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | **`UNPROVEN / FUTURE WORK`** |
| **Atomic Save Identity** | Userspace `workspaced` Re-keying | `/storage/system/object_id.registry` | **`BEST-EFFORT`** (Non-atomic with disk commits) |
| **Path Projection** | Read-Only Virtual VFS Directory | `STAGE3K-ARCHITECTURE-REV5.md` Section 14 | **`FUTURE WORK`** |
| **User-Visible Trash** | Staging Directory Layer | `STAGE3K-ARCHITECTURE-REV5.md` Section 19 | **`FUTURE WORK`** |
| **Historical Data Versions**| ZeroFS Data Extent Versioning | `STAGE3K-ARCHITECTURE-REV5.md` Section 6, 7 | **`ABSENT`** (Removed from MVP contract) |
| **Per-Instance `/tmp`** | VFS Namespace Virtualization | `STAGE3K-ARCHITECTURE-REV5.md` Section 4 | **`ABSENT`** (Marked Future Work) |
| **Browser Syscall Surface**| Microkernel Userspace Execution | Stage 3I, Stage 3J | **`UNPROVEN`** (MVP Prerequisite) |

```text
Current Frozen-Substrate Impact:
- Kernel Syscalls Added         : 0
- New Capability Types Added    : 0
- New Daemons Required          : 0
- Stage 3A–3N Code Modifications: 0
```

---

## 19. STAGE 3K STORAGE SCALE vs WORKSTATION EDITION SCALE (RESOLVING S5)

### 19.1 Stage 3K Bring-Up Storage Bounds
Direct audit of [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish\.gemini\antigravity-ide\scratch\project-zero\docs\design\STAGE3K-ARCHITECTURE-REV5.md) confirms exact physical volume limits:

```text
Stage 3K Physical Storage Bounds:
- Maximum Volume Size   : 134,217,728 bytes (128 MiB max volume capacity)
- Maximum Inodes        : 256 inodes per volume
- Maximum File Size     : 4,268,032 bytes (~4.07 MiB max single file size)
- Maximum Filename Len  : 55 bytes per component
- StorageObject Slots   : 32 concurrent open file descriptors in kernel BSS
```

$$\mathbf{Statement\ of\ Scale\ Alignment:}$$
$$\text{Stage 3K is a microkernel bring-up storage substrate for early system boot and verification.}$$
$$\text{Workstation Edition intended scale (10,000+ files, multi-gigabyte volumes) DEPENDS ON A FUTURE STORAGE STAGE.}$$

---

## 20. CATALOG OF AUTHORITATIVE INVARIANTS

1. `I-WS-MEMBERSHIP-NOT-CAPABILITY`: Workspace membership is contextual metadata and NEVER confers capability authority.
2. `I-WS-PATH-TREE-FIRST-CLASS`: ZeroFS canonical POSIX paths remain the single first-class filesystem source of truth.
3. `I-WS-VIEW-NOT-AUTHORITY`: Workspace spatial viewports and context graphs are non-authoritative metadata overlays.
4. `I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY`: Provisional attribution MUST NOT silently convert heuristic inference into confirmed membership; untouched items lapse to `Unfiled`.
5. `I-WS-REMOVE-MEMBERSHIP-NOT-DELETE-OBJECT`: Removing a membership edge MUST NOT delete the underlying ZeroFS storage object or move canonical paths.
6. `I-WS-EXTERNAL-MODIFICATION-BEST-EFFORT`: Un-observed external modifications are reconciled best-effort.
7. `I-WS-BROWSER-ISOLATION-NOT-BROWSER-SEMANTIC-AWARENESS`: Profile isolation DOES NOT confer tab-level semantic workspace awareness.
8. `I-WS-REWIND-ONLY-WHEN-SUBSTRATE-SUPPORTS-HISTORICAL-CONTENT`: File-content rewind is REMOVED because Stage 3K lacks historical data extents.
9. `I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE`: External network side-effects CANNOT be undone by context rewind.
10. `I-WS-OBJECT-ID-SINGLE-AUTHORITY`: `workspaced` owns the single authoritative Global Volume `ObjectId` Registry at `/storage/system/object_id.registry`.
11. `I-WS-OBJECT-ID-PHYSICAL-KEY-VERIFIED`: Ring 3 userspace identity resolution is keyed by `(Path_Hash, generation, mtime)`.
12. `I-WS-WRITE-ATTRIBUTION-DETERMINISTIC`: Writes are `DETERMINISTIC` iff process $C_{\text{ws}}$ handle + workload lineage + workspace path scope match.
13. `I-WS-BOOT-TIME-ORDERING-VALID`: Chronological timestamp ordering across cold reboots uses `(boot_epoch_sequence, monotonic_ticks)`.
14. `I-WS-EVENTLOG-NOT-FILE-VERSIONING`: `ContextEventRecord` is an append-only metadata audit log, NOT file-content versioning.

---

## 21. ARCHITECTURAL SELF-AUDIT & CHECKLIST

- **Single `ObjectId` Authority**: Single global registry at `/storage/system/object_id.registry` owned by `workspaced` (Section 3).
- **Userspace Identity Key**: Keyed by `(Path_Hash, generation, mtime)`; unexposed `inode_number` marked `UNPROVEN / FUTURE WORK` (Section 4).
- **Rename & Directory Authority**: Mediated via Layer 4 / `libzero` helpers; notifications via userspace IPC; atomic save continuity marked `BEST-EFFORT` (Section 5, Section 6).
- **Time Ordering**: Anchored to Stage 3K `mount_count` / Stage 4B `BootEpochSequence: u64` + `monotonic_ticks` (Section 14).
- **Write Attribution**: Strict ground-truth rules defined; IPC handoffs do NOT transfer capability authority (Section 7, Section 9).
- **L0 Resumption**: Re-classified as `PERSISTED / RECONSTRUCTABLE` for ZeroOS-owned state (Section 15).
- **Provisional Confirmation**: Open/read classified as `OBSERVED INTERACTION` (remains provisional); only edit/save/pin confirms (Section 11).
- **Event Log**: Append log specified with 64 KiB compaction limit; explicitly decoupled from file versioning (Section 16).
- **Scale & Impact**: Stage 3K limits ($128\text{ MiB}$) documented; 0 kernel code changes verified (Section 18, Section 19).

---

# ZEROOS OBJECT & MEMBERSHIP MODEL REV5 VERDICT

```text
REV5 STATUS:
ARCHITECTURE DRAFT COMPLETE — PENDING ADVERSARIAL REVIEW

IMPLEMENTATION:
NOT STARTED

FREEZE:
NOT APPROVED

STAGE 3A–3N:
FROZEN / UNMODIFIED

EXTERNAL REVIEW:
REQUIRED
```

```text
STATUS: 🟡 REVISION COMPLETE — PENDING EXTERNAL ADVERSARIAL REVIEW
IMPLEMENTATION: NOT AUTHORIZED
STAGE 3A–3N: FROZEN (0 bytes modified)
STAGE 4A–4F: FROZEN (100% reused)
STAGE 5–6: FROZEN (100% reused)
NEW SYSCALLS: 0
NEW DAEMONS: 0
```
