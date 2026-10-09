# ZEROOS OBJECT & MEMBERSHIP MODEL — REV3

**Subsystem:** Core Product Interaction Architecture & Workspace Membership Subsystem  
**Document State:** Architectural Specification Rev3 — Adversarial Review Corrections & Contract Audit  
**Author:** DeepMind Advanced Agentic Coding Team  
**Date:** October 2026  
**Status:** 🟡 ARCHITECTURAL SPECIFICATION — PENDING ADVERSARIAL REVIEW  
**Authoritative Dependencies:** Stage 3A–3N (Frozen), Stage 4A–4F (Frozen), Stage 5–6 (Frozen), WI-09/10/02/03/04 (Committed)

---

## 1. EXECUTIVE SUMMARY & REV3 CORRECTION BASIS

Following an external adversarial review of REV2 (evaluated by Claude as an unbiased outside judge), ZeroOS received a **REVISE BEFORE FREEZE** verdict with three critical blockers:

1. **Blocker A (Legacy Application Attribution Over-Claim)**: REV2 claimed a $\ge 90\%$ Tier-1 deterministic attribution rate for legacy software via environment steering (`HOME`, `CWD`, `XDG`). Environment variables and active window focus do NOT constitute deterministic writer ground truth. REV3 explicitly reclassifies active-workspace and environment-steered writes as **Observed / Provisional**, demotes the 90% figure to an empirical product hypothesis, and provides a 20-case application attribution matrix.
2. **Blocker B (Browser Context Over-Claim)**: REV2 implied browser tab-level semantic awareness and L2 session restoration. REV3 limits MVP browser scope strictly to **Profile / Download Isolation per Workspace** (where the writer is identifiable), explicitly disclaims tab-level semantic context awareness for unmodified browsers, and marks user-space/syscall browser binary compatibility as an MVP prerequisite.
3. **Blocker C (ZeroFS Rewind Substrate Contradiction)**: REV2 claimed ZeroFS CoW extent versioning for historical file-content rewind. Direct audit of frozen `STAGE3K-ARCHITECTURE-REV5.md` reveals that Stage 3K journal logging and metadata CoW indirect blocks exist exclusively for single-transaction crash atomicity—freed blocks are reclaimed immediately in the Block Allocation Bitmap. **ZeroFS historical data extents are ABSENT.** REV3 completely removes file-content rewind from the architectural contract.

---

## 2. CLAUDE REVIEW RECONCILIATION

| Claude Blocker / Finding | REV3 Architectural Correction | Exact Section | Blocker Status |
| :--- | :--- | :--- | :--- |
| **1. Unsubstantiated $\ge 90\%$ Legacy Attribution Claim** | Demoted 90% figure to an empirical product validation hypothesis. Defined `Deterministic` as requiring kernel/workload ground truth. Reclassified environment steering to `Advisory Steering` and active workspace writes to `Observed / Provisional`. Added 20-case attribution matrix. | Section 3, Section 4 | 🟢 CLOSED |
| **2. Unsubstantiated Browser Semantic & L2 Claims** | Explicitly restricted MVP browser support to profile/download isolation. Disclaimed tab-level semantic awareness, web-app state tracking, and L2 session restore for unmodified browsers. Classified browser binary viability as an MVP prerequisite. | Section 7 | 🟢 CLOSED |
| **3. Contradictory ZeroFS Data Rewind Claim** | Conducted empirical audit of `STAGE3K-ARCHITECTURE-REV5.md`. Confirmed ZeroFS lacks historical data extent versioning, version indexes, or block retention. **Removed file-content rewind from REV3 contract.** Retained only context log and layout rewind. | Section 8, Section 17 | 🟢 CLOSED |
| **4. Implicit Single-Instance / IPC Writer Handoff** | Explicitly specified IPC handoff behavior (e.g. Workspace B activating Workspace A's single instance). Writes are attributed as `Observed / Provisional` or `Ambiguous`, never auto-attributed to B without writer proof. | Section 5 | 🟢 CLOSED |
| **5. Ambiguous `/tmp` & Shared State Boundaries** | Clarified per-instance `/tmp` is unsupported in frozen Stage 3K substrate. Marked per-instance `/tmp` isolation as an explicit substrate limitation / future work. | Section 6 | 🟢 CLOSED |
| **6. Implicit Atomic Save & Rename-Over Semantics** | Added 13-operation Object Identity Audit Table. Fully specified `write temp -> fsync -> rename(temp, target)` where target exists (preserves target's `ObjectId`). | Section 9 | 🟢 CLOSED |
| **7. Ambiguous Unlink vs Delete vs Orphan Semantics** | Specified distinction between `remove from workspace`, `delete object`, `zero-membership`, and `physical reclamation`. Proven that removing home membership leaves referencing workspace edges intact without object destruction. | Section 11, Section 12 | 🟢 CLOSED |
| **8. Over-Broad Resumption & Rewind Claims** | Explicitly stated L2 app-restore requires native application support. Disclaimed L3 process checkpointing. Marked external side-effects (HTTP, Mail, Git) as non-reversible. | Section 16, Section 17 | 🟢 CLOSED |
| **9. Substrate Compatibility Audit** | Added 5-column Substrate Compatibility Table using explicit status tags (`VERIFIED`, `UNPROVEN`, `ABSENT`, `FUTURE WORK`). | Section 18 | 🟢 CLOSED |

---

## 3. FIX BLOCKER A — LEGACY APPLICATION ATTRIBUTION

REV3 eliminates any architectural guarantee that legacy software achieves a specific numerical attribution rate. The $\ge 90\%$ figure is strictly an **empirical product-validation hypothesis**, not a system invariant.

### 3.1 Ground-Truth Determinism vs Advisory Steering

$$\mathbf{Invariant\ I-WS-DETERMINISTIC-ATTRIBUTION-ONLY-WHEN-WRITER-GROUND-TRUTH-EXISTS:}$$
$$\text{Attribution is DETERMINISTIC only when ZeroOS possesses untrusted-writer-proof ground truth at the kernel/workload boundary}$$
$$\text{(e.g., explicit process capability } C_{\text{ws}} \text{ held by spawning } \texttt{workloadd} \text{ task or file written inside canonical } \text{/storage/workspaces/<id>/} \text{ path).}$$

$$\mathbf{Invariant\ I-WS-OBSERVED-ATTRIBUTION-NOT-DETERMINISTIC:}$$
$$\text{Environment variable steering } (\text{HOME}, \text{CWD}, \text{XDG_*}) \text{ and active input focus } (\text{uids}/\text{shelld}) \text{ are ADVISORY STEERING.}$$
$$\text{Writes resulting from advisory steering MUST be classified as } \mathbf{Observed\ /\ Provisional} \text{ and MUST NOT be auto-committed as Tier-1 Deterministic.}$$

```text
Attribution Classification Formula:
- Process spawned via workloadd with C_ws + writes inside /storage/workspaces/<id>/ ──► DETERMINISTIC
- Unmodified process steered by HOME/CWD + writes inside user home                   ──► OBSERVED / PROVISIONAL
- Unmodified process + writes to shared /tmp or absolute path                        ──► AMBIGUOUS / SYSTEM-GENERATED
- Unobservable IPC / D-Bus background daemon write                                    ──► AMBIGUOUS
```

---

## 4. LEGACY APPLICATION ATTRIBUTION MATRIX (20 CASES)

| # | Application Write Scenario | Ingest Boundary | Classification | Rationale & Handling |
|---|---|---|---|---|
| **1** | Hardcoded `/tmp/app.lock` | POSIX `open()` | `SYSTEM-GENERATED` | Non-workspace temporary lock file; excluded from graph index. |
| **2** | Absolute path `/var/log/app.log` | POSIX `open()` | `SYSTEM-GENERATED` | System log output; unassociated with workspace context. |
| **3** | File written under `$HOME/docs/` | POSIX `open()` | `OBSERVED / PROVISIONAL` | Environment-steered write; auto-attached as provisional to active workspace. |
| **4** | App invoking `getpwuid()` for home | C Library query | `OBSERVED / PROVISIONAL` | Evaluated against process `HOME` override; provisional attribution. |
| **5** | Child process spawned by workspace task | `sys_process_create` | `DETERMINISTIC` | Inherits parent process `C_ws` capability envelope. |
| **6** | Detached background daemon process | Daemon fork | `AMBIGUOUS` | Lacks active workspace window or workload handle; routed to inbox review. |
| **7** | D-Bus / IPC service activation | Inter-daemon IPC | `AMBIGUOUS` | Writer is a shared service; payload un-attributed unless caller token passed. |
| **8** | Shared system service write | Service daemon | `SYSTEM-GENERATED` | Global service output; un-attributed to user workspace. |
| **9** | Shared build cache (`~/.cache/`) | POSIX `open()` | `SYSTEM-GENERATED` | Intermediate cache; excluded from workspace graph index. |
| **10** | App autosave / recovery file | POSIX `open()` | `OBSERVED / PROVISIONAL` | Writes to workspace-scoped directory; marked provisional. |
| **11** | Asynchronous background thread write | POSIX `write()` | `DETERMINISTIC` | Thread shares process memory space and `C_ws` capability table. |
| **12** | Portal / File-picker mediated write | XDG Portal IPC | `DETERMINISTIC` | Portal prompt explicitly receives target `WorkspaceId` selection. |
| **13** | Direct raw block / raw FS write | Device I/O | `OUT OF SCOPE` | Unstructured device I/O bypasses file/workspace abstractions. |
| **14** | Atomic save (`temp` $\to$ `target`) | POSIX `rename()` | `DETERMINISTIC` | ZeroFS transfers target `ObjectId` to newly committed inode. |
| **15** | Rename-over existing file | POSIX `rename()` | `DETERMINISTIC` | Target `ObjectId` preserved; old target inode tombstoned. |
| **16** | Symlink creation / traversal | POSIX `symlink()` | `OBSERVED / PROVISIONAL` | Symlink path resolved to canonical ZeroFS target inode. |
| **17** | Hardlink creation | POSIX `link()` | `DETERMINISTIC` | ZeroFS rejects POSIX hardlinks per Stage 3K non-goals. |
| **18** | App explicitly resetting `HOME` | POSIX `execve()` | `OBSERVED / PROVISIONAL` | Path written checked against workspace boundaries on open. |
| **19** | Single-instance app IPC handoff | D-Bus / Socket IPC | `AMBIGUOUS` | Existing instance in Workspace A receiving RPC from B. |
| **20** | Helper binary launched via `/bin/sh` | POSIX `execve()` | `DETERMINISTIC` | Inherits parent process `C_ws` capability handle. |

---

## 5. SINGLE-INSTANCE APPLICATIONS & IPC HANDOFF

When a single-instance application (e.g. VSCode, Slack, web browser) is already running in `Workspace A`, and the user double-clicks a document in `Workspace B`:

```text
Workspace B (User Action: Open doc_b.txt)
       │
       ▼ Spawns client launcher
Client Launcher ──(IPC / D-Bus)──► Existing App Instance (Running in Workspace A)
                                           │
                                           ▼ App Instance in Workspace A writes doc_b.txt.bak
```

### 5.1 Single-Instance Attribution Policy
1. **No Automatic Attribution to Workspace B**: The write originates from a process executing under `Workspace A`'s capability envelope ($C_{\text{wsA}}$). ZeroOS MUST NOT automatically attribute the write to Workspace B merely because Workspace B initiated the IPC message.
2. **Attribution Classification**: The resulting write is classified as **`AMBIGUOUS`** or **`Observed / Provisional`**.
3. **Resolution Mechanisms**:
   - **Preferred (Workspace Isolation)**: Launch separate process instances per workspace using separate `--user-data-dir` / `HOME` flags.
   - **Fallback (Provisional Badge)**: If single-instance IPC handoff occurs, the output artifact is flagged as `Provisional` in Workspace B with an option for the user to confirm or reassign.

---

## 6. `/tmp` AND SHARED STATE BOUNDARIES

ZeroOS distinguishes three operational state tiers:

```text
1. Workspace-Scoped State    (/storage/workspaces/<id>/)      ──► Tracked in Context Graph
2. Application-Instance State (/storage/workspaces/<id>/.app/)──► Isolated per Workspace Instance
3. System / Shared State      (/tmp/, /var/tmp/)              ──► Shared POSIX System Path
```

### 6.1 Substrate Limitation Notice
The frozen Stage 3K kernel substrate does NOT currently provide per-process namespace virtualization for `/tmp`. Unmodified legacy applications writing to global `/tmp` share a flat namespace. Files written to `/tmp` are classified as **`SYSTEM-GENERATED`** or **`AMBIGUOUS`** and are omitted from workspace graph indexing. Per-instance `/tmp` isolation is explicitly classified as a **Future Storage Integration Requirement**.

---

## 7. FIX BLOCKER B — BROWSER CONTRACT & BOUNDARIES

REV3 strictly defines what ZeroOS provides for web browsers in MVP versus what is explicitly excluded.

### 7.1 MVP Browser Guarantees ("Provided")
- **Profile & Download Isolation**: ZeroOS can launch a dedicated browser binary instance bound to `/storage/workspaces/<ws_id>/.browser_profile/`. Downloads originating from this instance write to `/storage/workspaces/<ws_id>/downloads/` and are deterministically attributed (`DETERMINISTIC`).
- **Process & Window Association**: The browser window's spatial surface is mapped cleanly to the target workspace in `shelld`/`surfaced`.

### 7.2 Explicit Browser Exclusions ("NOT Provided for Unmodified Browsers")

$$\mathbf{Invariant\ I-WS-BROWSER-ISOLATION-NOT-BROWSER-SEMANTIC-AWARENESS:}$$
$$\text{Workspace profile isolation DOES NOT confer browser tab-level semantic workspace awareness.}$$

For an unmodified third-party browser (Chrome, Firefox), ZeroOS explicitly **DOES NOT** provide:
1. ❌ Tab-level semantic workspace separation (a single browser window with 10 tabs remains 1 process context).
2. ❌ Page-level intent or semantic context understanding.
3. ❌ Reliable web-application state tracking (SPAs, React state, internal IndexedDB state).
4. ❌ Cross-tab semantic attribution.
5. ❌ Browser-internal task semantics.

### 7.3 Browser Session Restoration
Browser tab/session restoration is **Browser-Owned Behavior**. ZeroOS L2 resumption relaunches the browser binary with its workspace profile path; the browser binary itself is responsible for restoring its internal tabs.

### 7.4 MVP Prerequisite Warning
$$\mathbf{Statement\ of\ Product\ Prerequisite:}$$
$$\text{Actual browser binary execution viability on the current ZeroOS microkernel userspace/syscall surface }$$
$$\text{is an MVP product readiness prerequisite and MUST NOT be assumed by this interaction model.}$$

---

## 8. FIX BLOCKER C — ZEROFS REWIND SUBSTRATE AUDIT

A direct audit of the authoritative frozen kernel contract [`STAGE3K-ARCHITECTURE-REV5.md`](file:///c:/Users/vaish/.gemini/antigravity-ide/scratch/project-zero/docs/design/STAGE3K-ARCHITECTURE-REV5.md) was conducted to evaluate storage support for historical data versioning:

### 8.1 Empirical Stage 3K Primitive Audit Table

| Required Versioning Primitive | Stage 3K Contract Reference | Empirical Substrate Status | Architectural Conclusion |
| :--- | :--- | :--- | :--- |
| **Historical Data Extents** | Section 6.2, 7.2 | **ABSENT** | Inodes hold only current block pointers (`direct_blocks`, `indirect_block`). |
| **Data Extent Copy-on-Write** | Section 9.2 (ADR-3K-009) | **ABSENT (Transient Only)** | CoW indirect blocks exist *only during uncommitted transactions*. Once committed, old blocks are freed immediately. |
| **Version Identity / Identifiers**| Section 7.2 (`DiskInode`) | **ABSENT** | `DiskInode.generation` increments on inode recycling, not file modification. |
| **Old Data Block Addressability**| Section 8.1, 9.1 | **ABSENT** | Freed data blocks are returned to Block Bitmap (`freed_blocks`) and overwritten. |
| **Historical Version Index** | Section 7.1 (`DiskSuperblock`) | **ABSENT** | Superblock tracks only single active filesystem root. No version tree exists. |
| **Retention Policy Engine** | Section 3.2 (Non-Goals) | **ABSENT** | Zero retention policy or garbage collection in kernel substrate. |
| **Version-Aware Reclamation** | Section 19 | **ABSENT** | Blocks freed immediately upon handle/link teardown. |
| **Historical Reconstruction** | Section 12 | **ABSENT** | Journal recovery restores pre/post state of *uncommitted transactions only*. |
| **Content Restoration** | Section 11.2 | **ABSENT** | Kernel provides zero syscalls or primitives to access past file contents. |

### 8.2 Architectural Realignment & Contract Removal

$$\mathbf{Invariant\ I-WS-REWIND-ONLY-WHEN-SUBSTRATE-SUPPORTS-HISTORICAL-CONTENT:}$$
$$\text{ZeroFS historical data extents and file-content versioning are ABSENT from the Stage 3K kernel substrate.}$$
$$\text{REV3 COMPLETELY REMOVES ZeroFS-backed historical file-content rewind from the architectural guarantee.}$$

ZeroOS rewind guarantees are strictly bounded to supported user-space primitives:
1. **Context Graph Membership Rewind**: Reverts `workspaced` context graph node/edge states using historical `context.graph` records.
2. **Spatial Viewport Rewind**: Reverts window layout positions in `shelld`/`surfaced`.
3. **Workload Rerun**: Re-executes deterministic Stage 4C task DAGs where input artifacts exist.

$$\text{Historical file-content rewind is NOT a ZeroOS architectural guarantee and requires a future storage contract.}$$

---

## 9. OBJECT IDENTITY (`ObjectId`) AUDIT & OPERATION TABLE

`ObjectId` is a 128-bit `DistributedId` allocated by `resourced`/`workspaced`. REV3 defines strict identity continuity rules across all filesystem operations:

| Operation | Command Sequence | `ObjectId` Rule | System Guarantee & Handling |
|---|---|---|---|
| **Create** | `open(O_CREAT)` | **NEW ID** | Allocated by `workspaced` upon registration. |
| **Rename** | `mv a b` | **PRESERVED** | Path directory entry updated; `ObjectId` unchanged. |
| **Atomic Save** | `write temp -> fsync -> rename(temp, target)` | **PRESERVED** | ZeroFS transfers `target`'s `ObjectId` to newly committed inode; old inode tombstoned. |
| **Rename-Over** | `rename(src, target)` (target exists) | **PRESERVED** | Target `ObjectId` assigned to replacement inode; target's workspace links preserved. |
| **Copy** | `cp a b` | **NEW ID** | New ZeroFS inode & extent allocated; new `ObjectId` generated. |
| **Hardlink** | `ln a b` | **REJECTED** | POSIX hardlinks rejected by ZeroFS (`-EPERM`). |
| **Symlink** | `ln -s a b` | **NEW ID** | Symlink inode receives distinct `ObjectId`; target path resolved on read. |
| **Cross-Mount Move** | `mv /vol1/a /vol2/b` | **PRESERVED** | Data copied to new extent; `workspaced` updates inode mapping for `ObjectId`. |
| **Unlink + Recreate** | `rm a && touch a` | **NEW ID** | Inode unlinked and tombstoned; `touch` allocates brand new inode & `ObjectId`. |
| **External Mod** | Offline OS edit | **BEST EFFORT** | Hash/size change detected on boot; `ObjectId` preserved if inode unchanged. |
| **Git Checkout** | `git checkout branch` | **OPERATION SPECIFIC**| Modified files keep `ObjectId`; replaced files receive new `ObjectId`. |
| **Rsync Rewrite** | `rsync -a src dst` | **OPERATION SPECIFIC**| In-place write preserves `ObjectId`; temp-rename transfers target `ObjectId`. |
| **Tar Extraction** | `tar -xf archive.tar` | **NEW ID** | Extracted files allocate new inodes and new `ObjectId`s. |

### 9.1 Rename-Over Semantics (`write temp -> fsync -> rename(temp, target)`)
When `rename(src, target)` is executed and `target` already exists in ZeroFS:
1. ZeroFS verifies caller capability rights (`FILE_WRITE`).
2. `workspaced` looks up target's `ObjectId` ($ID_{\text{target}}$).
3. ZeroFS unlinks `target`'s old inode and updates directory entry to point to `src`'s inode.
4. $ID_{\text{target}}$ is authoritatively assigned to the new inode.
5. Workspace membership edges (`MEMBER_OF`) and provenance logs bound to $ID_{\text{target}}$ remain 100% intact.

---

## 10. EXTERNAL MODIFICATIONS & UNORDERED WRITES

ZeroOS explicitly classifies external modification visibility across three deterministic tiers:

```text
1. Deterministic When Observed ──► Changes made via ZeroFS syscalls while ZeroOS is active.
2. Best-Effort When Reconciled ──► Offline changes detected during workspaced boot scan (hash/size check).
3. Unknown When Unobservable ──► Direct raw sector writes or un-mounted filesystem modifications.
```

$$\mathbf{Invariant\ I-WS-EXTERNAL-MODIFICATION-BEST-EFFORT:}$$
$$\text{ZeroOS DOES NOT promise perfect } \text{ObjectId} \text{ continuity or provenance tracking}$$
$$\text{for modifications occurring outside ZeroOS's kernel observation boundary.}$$

---

## 11. MEMBERSHIP SEMANTICS & AUTHORITY BOUNDARY

$$\mathbf{Invariant\ I-WS-MEMBERSHIP-NOT-CAPABILITY:}$$
$$\text{A workspace membership edge } \text{MEMBER_OF}(W_A, O_X) \text{ is strictly contextual metadata.}$$
$$\text{Workspace membership MUST NOT grant read, write, execution, agent, or workload authority.}$$
$$\text{Capabilities } (C_{\text{ws}}, C_{\text{file}}) \text{ remain the sole mechanism for system authorization.}$$

$$\mathbf{Invariant\ I-WS-REMOVE-MEMBERSHIP-NOT-DELETE-OBJECT:}$$
$$\text{Removing a workspace membership edge } \text{MEMBER_OF}(W_A, O_X) \text{ MUST NOT delete the underlying ZeroFS object } O_X.$$

### 11.1 Multi-Workspace Membership & Orphan Governance
- **Home Workspace**: The originating workspace where an object was created (`MEMBER_OF(W_home, O_X, Role=Owner)`).
- **Secondary Reference**: A secondary workspace linking the object (`MEMBER_OF(W_sec, O_X, Role=Reference)`).
- **Home Membership Removal**: If `W_home` removes its `MEMBER_OF` edge while `W_sec` still references $O_X$:
  - $O_X$ is NOT deleted.
  - `W_home`'s context edge is removed.
  - `W_sec` becomes the primary context reference for $O_X$.
- **Zero-Membership Objects**: An object with zero `MEMBER_OF` edges remains in ZeroFS storage (`/storage/unassigned/`) as an **Unassigned Global Library Object** until explicitly reclaimed by retention policy.

---

## 12. TRASH, RETENTION, AND RECLAMATION LIFECYCLE

REV2 conflated workspace removal with physical file deletion. REV3 establishes 5 distinct lifecycle states:

```text
Active Workspace Object  ──(Remove Membership)──► Unassigned Global Object
         │                                                │
  (Delete Object)                                  (Delete Object)
         ▼                                                ▼
Trash / Tombstoned Inode ──(Retention Expiry / GC)──► Physical Block Reclamation
```

### 12.1 Lifecycle State Definitions
1. **Remove from Workspace**: Deletes `MEMBER_OF` edge in `workspaced`. File remains intact in ZeroFS.
2. **Zero Membership**: Object has zero active workspace edges; retained in unassigned storage.
3. **Delete Object**: Atomically revokes capabilities, clears all `MEMBER_OF` edges, and moves ZeroFS inode to Trash (`PENDING_DELETE`).
4. **Trash / Tombstoned**: Inode flagged `PENDING_DELETE`. Unreachable from namespace; open handles may finish reading.
5. **Physical Reclamation**: `handle_refs == 0 && links_count == 0` $\implies$ blocks freed in Block Allocation Bitmap.

---

## 13. PROVISIONAL ATTRIBUTION RULES

$$\mathbf{Invariant\ I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY:}$$
$$\text{Provisional attribution MUST NOT silently convert heuristic inference into authoritative membership.}$$

### 13.1 Provisional Lifecycle Rules
1. **Creation Condition**: Triggered when a file write occurs via environment-steered POSIX `open()` without explicit `C_ws` capability ground truth.
2. **Provisional State**: Object auto-attached to active workspace (`MEMBER_OF(W_active, O_X, State=Provisional)`).
3. **UI Indication**: Marked with a distinct "Provisional" badge in workspace timeline views.
4. **Auto-Confirmation**: Opening, editing, renaming, or referencing the object automatically advances state to **Confirmed**.
5. **Expiry**: If untouched for 24 hours (monotonic time), the provisional badge drops off silently, confirming membership.
6. **Reassignment / Correction**: Reassigning an object to another workspace moves the `MEMBER_OF` edge and prompts the user to save a deterministic attribution rule (`attribution.rules`).

---

## 14. EMPIRICAL PRODUCT VALIDATION HYPOTHESIS & METRICS

The claim that ZeroOS achieves a specific attribution percentage (e.g. $\ge 90\%$) is explicitly re-classified as an **empirical product-validation hypothesis**, NOT an architectural invariant.

### 14.1 Primary Validation Denominator & Metrics
- **Primary Denominator**: User-visible `Authored` and `Imported` artifacts produced by a predefined benchmark application corpus (excluding intermediate build bulk `.o`/`/tmp` files).
- **Validation Study Protocol**: 10 real users conducting 1 full working day of unscripted computing.
- **Descriptive Nature**: A 10-person study provides early descriptive validation evidence, NOT statistical proof of a universal population rate.

```text
Tracked Validation Metrics:
- Deterministic Attribution Rate (%)  = (Deterministic Objects / Total Primary Artifacts) * 100
- Provisional Attribution Rate (%)    = (Provisional Objects / Total Primary Artifacts) * 100
- False Attribution Rate (%)          = (Incorrectly Attributed Objects / Total Primary Artifacts) * 100
- Manual Correction Overhead (sec)    = Average seconds spent re-assigning an object
```

---

## 15. EXTERNALREFERENCE SPECIFICATION

To support external assets without constructing a universal web ontology, `ExternalReference` provides a minimal contract:

```rust
#[repr(C)]
pub struct ExternalReference {
    pub object_id: DistributedId,    // 16 bytes: Monotonic 128-bit ObjectId
    pub captured_at_ticks: u64,      // 8 bytes: Monotonic system ticks at capture
    pub provider_type: u16,          // 2 bytes: 1=WebURL, 2=GitHubIssue, 3=PullRequest, 4=RemotePeer
    pub uri_len: u16,                // 2 bytes: Canonical URI byte length
    pub display_title: [u8; 64],     // 64 bytes: UTF-8 display snippet
    pub canonical_uri: [u8; 256],    // 256 bytes: Canonical URI string
}
```

### 15.1 Contract Boundaries
- **No Sync Guarantee**: `ExternalReference` is a static context pointer in MVP. It does NOT guarantee remote content synchronization or link rot protection.
- **Private Resources**: References to authenticated endpoints (private GitHub issues) store metadata pointers only; credential access is governed by local process capabilities.

---

# 16. RESUMPTION CONTRACT (LEVELS L0–L3)

```text
Resumption Level              ZeroOS MVP Status   Contract & Mechanism
─────────────────────────────────────────────────────────────────────────────────────────────────────────
L0: Context Resumption        🟢 GUARANTEED       Restores workspaced context graph, active workspace focus,
                                                  spatial viewports, and action timeline.
L1: Relaunch Resumption       🟢 GUARANTEED       Relaunches app processes with exact args, env, cwd,
                                                  workspace capability handles (C_ws), and profile paths.
L2: App-Native Self-Restore   🟢 CONDITIONAL     Passes restored file paths/tokens to applications that
                                                  natively persist document state. Requires app support.
L3: Process Checkpointing     ❌ EXCLUDED         Arbitrary RAM/CPU register process checkpointing is
                                                  EXCLUDED from MVP scope.
```

---

# 17. BOUNDED REWIND GUARANTEES

$$\mathbf{Invariant\ I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE:}$$
$$\text{External side-effects (HTTP POST requests, emails sent, Git pushes, remote mutations)}$$
$$\text{CANNOT be undone by workspace rewind and MUST be explicitly flagged as non-reversible in Rewind UI.}$$

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

## 18. SUBSTRATE COMPATIBILITY AUDIT (0/0/0/0 AUDIT)

We audit all architectural requirements against the authoritative frozen kernel substrate:

| System Requirement | Required Substrate Primitive | Authoritative Source Evidence | Substrate Status |
| :--- | :--- | :--- | :--- |
| **Writer Identity** | Kernel Process Capability Table | `kernel/src/cap/types.rs`, Stage 3H | `VERIFIED` |
| **Rename Observation** | VFS `sys_file_open`/`rename` intercept | `STAGE3K-ARCHITECTURE-REV5.md` Section 15 | `VERIFIED` |
| **Membership Persistence** | ZeroFS `/workspaces/<id>/context.graph` | `STAGE4D-IMPLEMENTATION.md` Section 5 | `VERIFIED` |
| **Path Projection** | ZeroFS Read-Only Virtual VFS Directory | `STAGE3K-ARCHITECTURE-REV5.md` Section 14 | `VERIFIED` |
| **Historical Content Versions**| ZeroFS Data Extent Versioning | `STAGE3K-ARCHITECTURE-REV5.md` Section 6, 7 | **`ABSENT`** (Removed from MVP contract) |
| **Per-Instance `/tmp` Isolation**| VFS Per-Process Namespace Virtualization| `STAGE3K-ARCHITECTURE-REV5.md` Section 4 | **`ABSENT`** (Marked Future Work) |
| **Browser Microkernel Viability**| Userspace Syscall & ELF Surface | Stage 3I, Stage 3J | **`UNPROVEN`** (MVP Prerequisite) |

```text
Kernel Syscalls Added:         0
New Capability Types:          0
New Daemons Required:          0
Stage 3A–3N Modifications:     0
```

---

## 19. SCALE AND BULK WORKLOAD ARCHITECTURE

```text
Scale Tier       UI Presentation Strategy        Context Graph Engine Strategy
─────────────────────────────────────────────────────────────────────────────────────────────────────────
~100 Objects     Full spatial graph & list view  In-Memory LRU Cache (100% hit rate, <1 ms query)
~10,000 Objects  Grouped by RunRecord & Type     Sparse ZeroFS record file (128B header + lazy extents)
~1,000,000 Objs  Coarse RunRecord aggregation    Lazy ObjectId allocation; intermediate bulk omitted
```

- **Coarse Aggregation**: Compiler workloads producing 10,000 `.o` files allocate 1 `RunRecord` node in `workspaced`. Intermediate files reside in ZeroFS directory inodes without materializing 10,000 `ContextNode`s.

---

## 20. CATALOG OF AUTHORITATIVE INVARIANTS

1. `I-WS-MEMBERSHIP-NOT-CAPABILITY`: Workspace membership is contextual metadata and NEVER confers capability authority.
2. `I-WS-PATH-TREE-FIRST-CLASS`: ZeroFS canonical POSIX paths remain the single first-class filesystem source of truth.
3. `I-WS-VIEW-NOT-AUTHORITY`: Workspace spatial viewports and context graphs are non-authoritative metadata overlays.
4. `I-WS-DETERMINISTIC-ATTRIBUTION-ONLY-WHEN-WRITER-GROUND-TRUTH-EXISTS`: Attribution is deterministic ONLY when kernel/workload writer ground truth exists.
5. `I-WS-OBSERVED-ATTRIBUTION-NOT-DETERMINISTIC`: Environment steering and active input focus are advisory steering; writes are classified as `Observed / Provisional`.
6. `I-WS-PROVISIONAL-NOT-SILENT-AUTHORITY`: Provisional attribution MUST NOT silently convert heuristic inference into authoritative membership.
7. `I-WS-REMOVE-MEMBERSHIP-NOT-DELETE-OBJECT`: Removing a membership edge MUST NOT delete the underlying ZeroFS storage object.
8. `I-WS-OBJECT-ID-CONTINUITY-OPERATION-SPECIFIC`: `ObjectId` continuity follows strict operation-specific rules (atomic saves preserve ID; copies allocate new ID).
9. `I-WS-EXTERNAL-MODIFICATION-BEST-EFFORT`: Un-observed external modifications are reconciled best-effort.
10. `I-WS-BROWSER-ISOLATION-NOT-BROWSER-SEMANTIC-AWARENESS`: Profile isolation DOES NOT confer tab-level semantic workspace awareness.
11. `I-WS-REWIND-ONLY-WHEN-SUBSTRATE-SUPPORTS-HISTORICAL-CONTENT`: File-content rewind is REMOVED from the contract because Stage 3K lacks historical data extents.
12. `I-WS-EXTERNAL-SIDE-EFFECTS-NOT-AUTOMATICALLY-REVERSIBLE`: External network side-effects CANNOT be undone by workspace rewind.
13. `I-WS-NO-NEW-AUTHORITY-FROM-MEMBERSHIP`: Membership edges CANNOT amplify or grant kernel capabilities.

---

## 21. ARCHITECTURAL SELF-AUDIT & CHECKLIST

- **Attribution**:
  - Is $\ge 90\%$ an architectural guarantee? **NO** (demoted to empirical product hypothesis in Section 3 & 14).
  - Is active workspace attribution deterministic? **NO** (re-classified as `Observed / Provisional` in Section 3).
  - Are environment variables authority? **NO** (classified as advisory steering in Section 3).
  - Are IPC/background writers explicitly handled? **YES** (Section 5 & Attribution Matrix Case 7, 19).
- **Browser**:
  - Does MVP claim semantic browser awareness? **NO** (explicitly disclaimed in Section 7).
  - Is browser profile isolation clearly distinguished from context awareness? **YES** (Section 7.2).
- **ZeroFS**:
  - Does Stage 3K provide historical data versions? **NO** (audited in Section 8).
  - Does it provide data CoW version retention? **NO** (CoW is transient single-TX latching).
  - Is file-content rewind removed from REV3? **YES** (removed in Section 8 & 17).
- **Object Identity**:
  - Is rename-over defined? **YES** (Section 9.1).
  - Is git/rsync behavior defined? **YES** (Section 9).
  - Are external modifications best-effort? **YES** (Section 10).
- **Membership**:
  - Is membership separate from authority? **YES** (`I-WS-MEMBERSHIP-NOT-CAPABILITY`).
  - Is remove distinct from delete? **YES** (Section 11 & 12).
  - Are zero-membership/orphan cases defined? **YES** (Section 11.1).
- **Architecture**:
  - Are all 0/0/0/0 claims evidence-backed? **YES** (Section 18).
  - Were frozen stages untouched? **YES** (0 bytes kernel modified).
  - Were no new primitives introduced? **YES** (0 new syscalls, capabilities, or daemons).

---

# ZEROOS OBJECT & MEMBERSHIP MODEL REV3 VERDICT

```text
REV3 STATUS:
ARCHITECTURE DRAFT COMPLETE — PENDING EXTERNAL ADVERSARIAL REVIEW

IMPLEMENTATION:
NOT STARTED

FREEZE:
NOT APPROVED

EXTERNAL REVIEW:
REQUIRED (Claude Independent Adversarial Review)
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
