# ZEROOS OBJECT & MEMBERSHIP MODEL — REV6

**Subsystem:** Core Product Interaction Architecture & Workspace Membership Subsystem  
**Document State:** Architectural Specification Rev6 — 3-Way Authority & Causality Realignment  
**Author:** DeepMind Advanced Agentic Coding Team  
**Date:** October 2026  
**Status:** 🟡 ARCHITECTURAL SPECIFICATION — PENDING ADVERSARIAL REVIEW  
**Authoritative Dependencies:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), Stage 5–6 (Frozen), WI-09/10/02/03/04 (Committed)

---

## 1. EXECUTIVE SUMMARY & REV6 REVISION BASIS

Following an external adversarial review of REV5, ZeroOS received a **REVISE BEFORE FREEZE** verdict targeting five specific architectural areas:

1. **🔴 B2 — Observation Key vs Physical Identity**: REV5 claimed `(Path_Hash, generation, mtime)` as a "verified physical identity key". Because path strings change on rename (`hash(a.txt) != hash(b.txt)`), a path hash cannot physically derive identity. REV6 reclassifies path keying as a **Best-Effort Userspace Observation Key** and establishes that `ObjectId` continuity across rename is an observed filesystem-history property rather than a physical stat invariant.
2. **🔴 B5 — Authorization Attribution vs Causal Attribution**: REV5 equated capability possession ($C_{\text{ws}}$) with "deterministic writer causality". REV6 explicitly decouples **Writer Authority** (process capability envelope), **Writer Process** (PID executing the file write), **Workload Causality** (Stage 4C task DAG trigger), and **Workspace Attribution** (final assignment tier). Deterministic attribution requires BOTH writer authority AND direct workload causality.
3. **🟡 B1 — Three-Way Authority Boundaries**: REV6 formally specifies the strict 3-way system authority protocol: ZeroFS is authoritative for physical file existence and paths; `object_id.registry` is authoritative for logical `ObjectId` assignment; `context.graph` is authoritative ONLY for workspace membership edges (`MEMBER_OF`).
4. **🟡 B3 / Rename-Over Semantics**: REV6 eliminates semantic ambiguity in rename-over operations (`rename(A, B)` where B exists) by explicitly adopting **Physical Source Semantics**: the surviving `/bar` file retains source $ID_A$, while target $ID_B$ is unlinked/tombstoned in the registry.
5. **🟡 B4 / Boot Epoch Ordering & EventLog Compaction**: REV6 proves cross-reboot timestamp ordering by anchoring boot epochs to Stage 3K `DiskSuperblock.mount_count` (persisted to disk prior to timestamp issuance). It also specifies crash-atomic EventLog compaction using temporary file write-replace staging.

---

## 2. REV6 RESPONSE TO ADVERSARIAL REVIEW FINDINGS

| Finding ID | Description | REV6 Architectural Correction | Exact Section | Substrate Status |
|---|---|---|---|---|
| **🔴 B2** | `Path_Hash + gen + mtime` claimed as physical identity | Re-classified as **Best-Effort Userspace Observation Key**. Specified that `ObjectId` continuity across rename is an observed operation history property. | Section 4 | `BEST-EFFORT` |
| **🔴 B5** | Capability possession ($C_{\text{ws}}$) equated with write causality | Decoupled **Writer Authority**, **Writer Process**, **Workload Causality**, and **Workspace Attribution**. Deterministic status requires direct causal proof. | Section 6, Section 7 | `VERIFIED` (Stage 3H/4C) |
| **🟡 B1** | Underspecified authority between ZeroFS, Registry, and Graph | Established explicit **3-Way Authority Protocol**: ZeroFS = Physical Truth; Registry = Logical Identity Truth; ContextGraph = Membership Truth. | Section 3 | `VERIFIED` |
| **🟡 B3** | Rename-over semantics contained logical contradiction | Adopted **Physical Source Semantics**: `rename(A, B)` assigns $ID_A$ to target path $B$; target $ID_B$ is unlinked and logged as superseded. | Section 4.1 | `BEST-EFFORT` |
| **🟡 B4** | Boot epoch ordering lacked persistence proof | Proved ordering by anchoring epochs to Stage 3K `DiskSuperblock.mount_count`, persisted in Block 1 prior to issuing timestamps. | Section 13 | `VERIFIED` (Stage 3K/4B) |
| **🟡 S3** | EventLog compaction lacked crash atomicity | Specified crash-atomic compaction using `/workspaces/<id>/context.graph.tmp` write-replace staging and ZeroFS 6-step commit. | Section 15 | `VERIFIED` |

---

## 3. THREE-WAY SYSTEM AUTHORITY BOUNDARIES (RESOLVING B1)

To eliminate authority ambiguity across kernel storage, identity management, and workspace context, REV6 establishes a strict **3-Way System Authority Protocol**:

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                          THREE-WAY AUTHORITY PROTOCOL                           │
├──────────────────────────────────┬──────────────────────────────┬───────────────┤
│          ZeroFS Storage          │ Global Volume Registry       │ Workspace     │
│       (Stage 3K Kernel)          │ (workspaced /system/)        │ Context Graph │
├──────────────────────────────────┼──────────────────────────────┼───────────────┤
│ Authoritative for:               │ Authoritative for:           │ Authoritative │
│ - Physical file existence        │ - Logical ObjectId           │ ONLY for:     │
│ - Physical block allocation      │   assignment & resolution    │ - Workspace   │
│ - Canonical POSIX paths          │ - Inode-to-ObjectId mapping  │   membership  │
│ - Raw inode generation & size    │ - Volume-wide identity index │   edges       │
└──────────────────────────────────┴──────────────────────────────┴───────────────┘
```

### 3.1 Authority Hierarchy & Conflict Resolution
1. **Physical Existence Sovereignty**: ZeroFS is the absolute source of truth for physical file existence. If ZeroFS reports a file does not exist, no registry or context graph record can claim physical existence.
2. **Logical Identity Sovereignty**: `object_id.registry` (stored at `/storage/system/object_id.registry` and owned authoritatively by `workspaced`) is sovereign for assigning 128-bit `ObjectId`s to physical ZeroFS files. Workspace context graphs NEVER independently mint `ObjectId`s.
3. **Context Membership Sovereignty**: `/workspaces/<ws_id>/context.graph` is sovereign ONLY for metadata membership edges (`MEMBER_OF`). A membership edge confers context association, NEVER physical existence or capability authority.

### 3.2 Boot Recovery & Crash Hierarchy
If an un-graceful system crash occurs while a file operation is committing:
- **ZeroFS Block Device Commitment** (6-step journal protocol) executes first.
- On reboot, `workspaced` scans ZeroFS filesystem state. If ZeroFS shows a file was created or unlinked, `workspaced` reconciles `object_id.registry` to match ZeroFS physical reality (**ZeroFS physical state is sovereign**).

---

## 4. OBJECT IDENTITY & USERSPACE OBSERVATION KEY (RESOLVING B2)

$$\mathbf{Invariant\ I-WS-OBJECT-ID-USERSPACE-OBSERVATION-KEY:}$$
$$\text{Ring 3 userspace identity resolution } (\text{BLAKE2s}(\text{Path}), \text{generation}, \text{mtime}) \text{ is a BEST-EFFORT OBSERVATION KEY.}$$
$$\text{ObjectId continuity across file rename is an observed operation-history property,}$$
$$\text{NOT a physically derivable identity from the frozen Stage 3K stat ABI (which omits inode numbers).}$$

```text
ZeroFS Physical File (/storage/workspaces/ws_a/data.csv)
  ├── DiskInode.generation: u32 (Recycling counter)
  └── UserFileStat.mtime_ticks: u64
             ▲
             │ (Best-Effort Observation Key: Path_Hash + generation + mtime)
workspaced Global Volume Registry (/storage/system/object_id.registry)
  └── Maps Observation Key ──► ObjectId (128-bit DistributedId)
```

### 4.1 Rename-Over Semantics (Physical Source Semantics)
When `rename(src, target)` replaces an existing `target` file ($A \to B$ where $B$ exists):

```text
Before Rename:
  Source File A (/foo) ──► ObjectId_A
  Target File B (/bar) ──► ObjectId_B

Execution (Physical Source Semantics):
1. ZeroFS unlinks Target File B's inode; replaces directory entry with Source File A's inode.
2. workspaced updates registry: Target Path /bar is now mapped to ObjectId_A.
3. ObjectId_B is tombstoned in object_id.registry.
4. RunRecord logs Event: "ObjectId_B superseded by ObjectId_A via rename-over".
```

Result: The surviving file content at `/bar` retains $ID_A$. Target $ID_B$ is cleanly tombstoned without semantic contradiction.

---

## 5. DIRECTORY MUTATION & RENAME AUTHORITY AUDIT (RESOLVING B3)

A complete audit of directory operations across Stage 3K kernel layers and userspace libraries establishes the actual operational authority:

| Directory Operation | Kernel Layer / Function | Userspace Syscall Boundary | Service / Library Owner | Atomicity Guarantee | Notification Method |
|---|---|---|---|---|---|
| **Create Entry** | Layer 4 `DirectoryManager::create_entry` | `SYS_FILE_OPEN(O_CREATE)` | Kernel Layer 4 / `libzero` | **Atomic** (6-step Journal TX) | `libzero` Client IPC |
| **Remove Entry** | Layer 4 `DirectoryManager::remove_entry` | Mediated via `libzero` / `init` | Kernel Layer 4 / `libzero` | **Atomic** (6-step Journal TX) | `libzero` Client IPC |
| **Rename Entry** | Layer 4 `DirectoryManager::rename_entry` | Mediated via `libzero` / `init` | Kernel Layer 4 / `libzero` | **Atomic** (6-step Journal TX) | `libzero` Client IPC |
| **Atomic Save (`write+rename`)**| Layer 4 `write` + `rename_entry` | `SYS_FILE_WRITE` + `libzero` rename | `libzero` VFS Helper | **BEST-EFFORT** (Separate TXs) | `libzero` Client IPC |
| **Directory Scan** | Layer 4 `DirectoryManager::list_entries` | `SYS_FILE_READ` on Dir Handle | `workspaced` Scanner | **Atomic** per block read | Read-driven scanning |

### 5.1 Substrate Distinction
- **Frozen Kernel Substrate**: Stage 3K Syscalls 11..16 provide `SYS_FILE_OPEN`, `READ`, `WRITE`, `CLOSE`, `STAT`, `SYNC`. Raw `rename` and `unlink` syscalls are NOT provided in Syscalls 11..16.
- **Designated Ring 3 Helper**: `libzero` VFS helpers execute directory mutations via directory handles. `workspaced` observes these mutations via `libzero` IPC notifications or background directory scanning (**BEST-EFFORT**).

---

## 6. WRITE ATTRIBUTION TAXONOMY & CAUSAL LINEAGE (RESOLVING B5)

REV6 explicitly decouples authorization, process execution, and causal workload triggers:

```text
1. WRITER_AUTHORITY   ──► Does process hold Workspace Capability Handle C_ws? (Security Envelope)
2. WRITER_PROCESS     ──► PID executing the physical SYS_FILE_WRITE syscall in Stage 3K.
3. WORKLOAD_CAUSALITY ──► Specific Stage 4C WorkloadId/TaskId that triggered the execution DAG.
4. WORKSPACE_ATTR     ──► Final workspace assignment tier (DETERMINISTIC, PROVISIONAL, AMBIGUOUS).
```

$$\mathbf{Invariant\ I-WS-WRITE-ATTRIBUTION-DETERMINISTIC:}$$
$$\text{Workspace Attribution is DETERMINISTIC iff:}$$
$$\text{1. WRITER\_AUTHORITY is valid } (C_{\text{ws}} \text{ handle held by writing process}), \mathbf{AND}$$
$$\text{2. WORKLOAD\_CAUSALITY is directly proven } (\text{Task DAG spawned by } \texttt{workloadd} \text{ without untracked IPC hops}), \mathbf{AND}$$
$$\text{3. Target path resides inside } \text{/storage/workspaces/<ws\_id>/} \text{ or explicit } \texttt{libzero} \text{ stream.}$$

### 6.1 Attribution Classification Hierarchy

| Attribution Tier | Required Evidence | System Behavior |
|---|---|---|
| **`DETERMINISTIC`** | `WRITER_AUTHORITY` + Direct `WORKLOAD_CAUSALITY` + Workspace Path Scope | Auto-committed to workspace context graph. |
| **`PROVISIONAL`** | Advisory Steering (`HOME`, `CWD`, active focus) without direct `WORKLOAD_CAUSALITY` | Auto-attached with Provisional badge; lapses to `Unfiled` if untouched for 24h. |
| **`AMBIGUOUS`** | IPC handoffs to single-instance apps, detached daemons, untracked IPC hops | Routed to Workspace Inbox / Review Queue. |
| **`SYSTEM_GENERATED`** | Global system logs (`/var/log/`), temporary lock files (`/tmp/lock`) | Excluded from workspace context graphs. |

---

## 7. PROCESS, CAPABILITY, AND WORKLOAD LINEAGE

```text
Process Execution Tree (Stage 4C workloadd)
       │
       ├── Process spawned with C_ws handle + direct TaskId binding?
       │     ├── YES ──► Writer Authority + Workload Causality Proven ──► DETERMINISTIC
       │     └── NO  ──► Lacks direct TaskId binding                  ──► PROVISIONAL
       │
       └── Multi-Process & IPC Scenarios
             ├── Child process created via sys_process_create          ──► Inherits C_ws & Causality
             ├── Shared Daemon write (un-tracked IPC caller token)    ──► AMBIGUOUS
             └── Process drops C_ws handle                             ──► AMBIGUOUS / PROVISIONAL
```

---

## 8. SINGLE-INSTANCE APPLICATIONS & IPC HANDOFF

When `Workspace B` triggers an action that sends an IPC message to an existing single-instance application running in `Workspace A`:

```text
Workspace B (User Action: Open document in Workspace B)
       │
       ▼ IPC Message (Carries Workspace B Context Token)
Single-Instance Application Process (Running in Workspace A under C_wsA)
       │
       ▼ Executing Process writes output file out.dat
```

### 8.1 Single-Instance Attribution Rules
1. **`WRITER_AUTHORITY` Governs**: Physical write is executed by PID holding $C_{\text{wsA}}$. ZeroOS attributes the physical write to **Workspace A** (`DETERMINISTIC` for Workspace A).
2. **`WORKLOAD_CAUSALITY` Broken for Workspace B**: Because the write traversed an untracked IPC channel into an existing process in Workspace A, Workspace B lacks direct `WORKLOAD_CAUSALITY`.
3. **Workspace B Reference**: Workspace B receives a `Provisional` reference edge pointing to `out.dat`. ZeroOS NEVER transfers capability handles across IPC channels.

---

## 9. COLLAPSED STRUCTURAL MODEL & MEMBERSHIP SEMANTICS

REV6 maintains the collapsed structural model consisting of exactly **3 primitive concepts**:

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

## 10. PROVISIONAL MEMBERSHIP & EVIDENCE HIERARCHY

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

### 10.1 Interaction Evidence Classification
- **`Open / Read`**: Classified as `OBSERVED INTERACTION`. Does NOT convey explicit intent to bind membership; item remains `Provisional`.
- **`Edit / Save / Pin / Drag`**: Conveys explicit human intent; advances state to **`CONFIRMED MEMBERSHIP`**.
- **Untouched Expiry (24h)**: Lapses to `Unfiled`. Heuristic guesses are NEVER auto-confirmed without explicit user evidence.

---

## 11. TRASH, RETENTION, AND RECLAMATION

```text
Operation                 Target Entity                Storage Action                        Reversibility
─────────────────────────────────────────────────────────────────────────────────────────────────────────────
Remove Membership         ContextNode metadata edge     Deletes MEMBER_OF edge in workspaced   Reversible (Re-add edge)
Delete Object             ZeroFS Inode                  Unlinks directory entry via ZeroFS     IRREVERSIBLE (Data lost)
Open-Handle Unlink        Open StorageObject            Sets PENDING_DELETE on inode           Pending handle close
Physical Reclamation      Block Allocation Bitmap       Frees data blocks when handles = 0    Permanent block reuse
```

### 11.1 Substrate Limitation Notice
- **Irreversible Deletion**: Calling file delete unlinks the ZeroFS inode immediately. **File content deletion in ZeroFS is IRREVERSIBLE**.
- User-visible Trash (staging directory layer) is classified as **`FUTURE WORK / UNPROVEN`**.

---

## 12. PATH PROJECTION CONCEPTUAL CONTRACT

For an unmodified CLI binary running in `Workspace B` needing to read a referenced file residing physically in `Workspace A`:

```text
Workspace B Process Context
       │
       ▼ Open projected path
/workspaces/ws_b/.proj/<obj_id>/data.csv  ──(Read-Only VFS Projection)──► /storage/workspaces/ws_a/data.csv
```

### 12.1 Substrate Status
- **Read-Only Access**: Projected paths allow read-only access (`FILE_READ`). Writes return `-EACCES`.
- **Substrate Status**: Path projection is NOT implemented in Stage 3K kernel substrate. Classified as **`FUTURE WORK / UNPROVEN`**.

---

## 13. CHRONOLOGICAL TIME & BOOT EPOCH ORDERING PROOF (RESOLVING B4)

To prove mathematically verifiable chronological time ordering across cold reboots:

```rust
#[repr(C)]
pub struct QualifiedTimestamp {
    pub boot_epoch_sequence: u64, // 8 bytes: Monotonic boot epoch counter (DiskSuperblock.mount_count)
    pub monotonic_ticks: u64,     // 8 bytes: Monotonic ticks elapsed within current boot epoch
}
```

### 13.1 Boot Epoch Persistence & Ordering Proof
1. **Persistence Protocol**: Stage 3K `DiskSuperblock.mount_count` (stored in Block 1) is incremented and flushed to disk during Layer 3 `mount_volume()` **prior to issuing any timestamps or accepting filesystem operations**.
2. **Crash Atomicity**: Superblock update uses the 6-step journal protocol (`I-STOR-JOURNAL-1`). If power fails during mount, journal recovery rolls forward or re-executes mount count increment. `mount_count` NEVER regresses.
3. **Ordering Relation**:

$$\mathbf{Invariant\ I-WS-BOOT-TIME-ORDERING-VALID:}$$
$$\text{Chronological ordering between two timestamps } T_A \text{ and } T_B \text{ is authoritatively evaluated as:}$$
$$T_A < T_B \iff (T_A.\text{boot\_epoch\_sequence} < T_B.\text{boot\_epoch\_sequence}) \lor (T_A.\text{boot\_epoch\_sequence} == T_B.\text{boot\_epoch\_sequence} \land T_A.\text{monotonic\_ticks} < T_B.\text{monotonic\_ticks})$$

$$\text{Substrate Classification } \implies \mathbf{VERIFIED\ FOR\ ORDERING\ VIA\ STAGE\ 3K\ SUPERBLOCK\ MOUNT\_COUNT.}$$

---

## 14. RESUMPTION CONTRACT (BEST-EFFORT L1)

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

## 15. EVENT LOG SEMANTICS & COMPACTION CRASH ATOMICITY (RESOLVING S3)

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

### 15.1 Compaction Protocol & Crash Atomicity
1. `workspaced` writes new compacted event log to `/workspaces/<id>/context.graph.tmp`.
2. Issues `SYS_FILE_SYNC` to flush data blocks to block device storage.
3. Replaces `/workspaces/<id>/context.graph` via ZeroFS directory entry update.
4. If power fails mid-compaction, `/workspaces/<id>/context.graph` remains 100% intact (**Crash-Atomic Compaction**).

---

## 16. REWIND BOUNDARIES & NON-REVERSIBLE SIDE EFFECTS

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

## 17. CURRENT FROZEN-SUBSTRATE IMPACT & LIMITATIONS AUDIT

We audit all architectural requirements against the authoritative frozen kernel contracts:

| System Requirement | Required Substrate Primitive | Authoritative Source Evidence | Substrate Status |
|---|---|---|---|
| **Writer Authority** | Kernel Process Capability Table | `kernel/src/cap/types.rs`, Stage 3H | `VERIFIED` |
| **Path Open** | `SYS_FILE_OPEN` (Syscall 11) | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `VERIFIED` |
| **Boot Epoch Persistence** | `DiskSuperblock.mount_count` | `STAGE3K-ARCHITECTURE-REV5.md` Section 7.1 | `VERIFIED` |
| **Global ObjectId Registry**| Userspace `workspaced` Registry | `/storage/system/object_id.registry` | `VERIFIED` (`workspaced`) |
| **Userspace Observation Key**| Path Hash + `generation` + `mtime` | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `BEST-EFFORT` |
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

## 18. STAGE 3K STORAGE SCALE vs WORKSTATION EDITION SCALE

### 18.1 Stage 3K Bring-Up Storage Bounds
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

## 19. CATALOG OF AUTHORITATIVE INVARIANTS

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
11. `I-WS-OBJECT-ID-USERSPACE-OBSERVATION-KEY`: Userspace identity keying `(Path_Hash, generation, mtime)` is a best-effort observation key, NOT a kernel physical stat invariant.
12. `I-WS-WRITE-ATTRIBUTION-DETERMINISTIC`: Workspace Attribution is `DETERMINISTIC` iff `WRITER_AUTHORITY` ($C_{\text{ws}}$) AND direct `WORKLOAD_CAUSALITY` match.
13. `I-WS-BOOT-TIME-ORDERING-VALID`: Chronological timestamp ordering across cold reboots uses `(boot_epoch_sequence, monotonic_ticks)` anchored to `DiskSuperblock.mount_count`.
14. `I-WS-EVENTLOG-NOT-FILE-VERSIONING`: `ContextEventRecord` is an append-only metadata audit log, NOT file-content versioning.

---

## 20. ARCHITECTURAL SELF-AUDIT & CHECKLIST

- **Observation Key vs Physical Identity**: Reclassified as Best-Effort Observation Key (Section 4).
- **Writer Authority vs Causal Attribution**: Decoupled into `WRITER_AUTHORITY`, `WRITER_PROCESS`, `WORKLOAD_CAUSALITY`, and `WORKSPACE_ATTRIBUTION` (Section 6).
- **Three-Way Authority Boundaries**: Defined ZeroFS = Physical Truth, Registry = Logical Identity Truth, ContextGraph = Membership Truth (Section 3).
- **Rename-Over Semantics**: Physical source semantics adopted ($ID_A$ survives at target path $B$, Section 4.1).
- **Boot Epoch Ordering**: Proved via Stage 3K `DiskSuperblock.mount_count` Block 1 persistence (Section 13).
- **EventLog Compaction**: Crash-atomic via temporary file write-replace staging (Section 15).

---

# ZEROOS OBJECT & MEMBERSHIP MODEL REV6 VERDICT

```text
REV6 STATUS:
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
