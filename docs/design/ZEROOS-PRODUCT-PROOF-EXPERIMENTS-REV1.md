# ZEROOS PRODUCT PROOF — EXPERIMENTS REV1

## Status: 🟢 AUTHORIZED EXPERIMENT DESIGN
**Document ID:** `ZEROOS-PRODUCT-PROOF-EXPERIMENTS-REV1`  
**Authoritative Basis:** `docs/design/ZEROOS-PRODUCT-PROOF.md`  
**Execution Boundary:** **DO NOT IMPLEMENT STAGE 7. DO NOT WRITE CODE. DO NOT CREATE DAEMONS OR SYSCALLS.** This is an experiment design specification establishing concrete, reproducible tests to evaluate the ZeroOS complexity-transfer hypothesis.

---

## 1. Purpose

This document turns the frozen ZeroOS Product Proof framework (`docs/design/ZEROOS-PRODUCT-PROOF.md`) into a concrete, reproducible suite of product validation experiments. 

The purpose is to evaluate whether the existing ZeroOS architecture (Stages 3A–6F) successfully transfers computing complexity from the human user to the operating system, while preserving the human user's complete authority over intent and outcome.

---

## 2. Relationship to `ZEROOS-PRODUCT-PROOF.md`

`ZEROOS-PRODUCT-PROOF.md` defines the overarching product hypothesis, evaluation dimensions ($H, C, K, R, T$), 8 validation scenario families, 4 cross-cutting constraints, and the 5-category failure taxonomy.

`ZEROOS-PRODUCT-PROOF-EXPERIMENTS-REV1.md` operationalizes that framework into 7 concrete, end-to-end experiments. Every experiment defined herein directly maps to one or more scenario families and cross-cutting constraints, and measures the exact same $H, C, K, R, T$ metrics.

---

## 3. Validation Principles

1. **Test the Thesis, Not the Subsystems:** Experiments evaluate whether human computing complexity is reduced, not whether an individual daemon (`groundd`, `shelld`, `fabricd`) executes an isolated internal function.
2. **Real Computing Problems:** Experiments test authentic computing workflows that real users perform daily, rather than synthetic OS feature demos.
3. **No AI Magic Assumption:** Experiments cleanly isolate model intelligence from OS architectural capabilities. A failure caused by LLM reasoning must not be misclassified as an OS failure.
4. **Realistic Conventional Baselines:** Baseline comparisons reflect how a skilled user actually accomplishes tasks on modern conventional operating systems (Linux/macOS/Windows) without artificially inflating baseline difficulty.
5. **No Synthetic Metric Precision:** Metrics ($H, C, K, R, T$) represent observable operational counts and cognitive reconstruction steps, not arbitrary benchmark points.
6. **Zero Architectural Expansion:** If an experiment cannot execute on the frozen Stage 3A–6F architecture, the blocker must be classified under the failure taxonomy (A/B/C/D/E). No new subsystems, daemons, capability types, or syscalls may be introduced.

---

## 4. Experiment Selection Rationale

The 7 experiments were selected to achieve maximum architectural and scenario coverage across the 8 scenario families and 4 cross-cutting constraints with the minimum number of reproducible tests:

```text
                               SCENARIO & CONSTRAINT COVERAGE

  EXP-01: Artifact Creation ───────────► Family 1 (Creation), Traditional Apps
  EXP-02: Multi-Source Context ────────► Family 2 (Information), Grounding / Privacy
  EXP-03: Zero-Reconstruction Session ─► Family 3 (Continuity), Failure & Recovery
  EXP-04: Multi-Tool Pipeline ────────► Family 4 (Coordination), App IPC
  EXP-05: Unattended Workload Survival ─► Family 5 (Long-Running), Offline Operation
  EXP-06: Mid-Execution Intent Pivot ──► Family 6 (Intent Changes), Human-Agent Collab
  EXP-07: Transparent Compute Offload ──► Family 7 & 8 (Resource & Distributed)
```

---

## 5. Experiment Matrix

| Experiment ID | Experiment Name | Primary Family | Cross-Cutting Constraints Tested | Primary Metrics Evaluated | Existing Subsystems Exercised |
|---|---|---|---|---|---|
| **EXP-01** | Technical Report & Asset Assembly | Family 1 (Creation) | Traditional Apps, Human-Agent Collab | $H, K, T$ | `intentd`, `workspaced`, `shelld`, App IPC |
| **EXP-02** | Multi-Source Context Synthesis | Family 2 (Information) | Privacy Policies, Offline Operation | $H, C, K$ | `groundd`, `intentd`, `WorkspaceAccessCap` |
| **EXP-03** | Zero-Reconstruction Session Recovery | Family 3 (Continuity) | Failure & Recovery, Offline Operation | $C, R, T$ | `workspaced`, `shelld`, Stage 6F Session Headers |
| **EXP-04** | Coordinated Data Processing Pipeline | Family 4 (Coordination) | Traditional Apps, Human-Agent Collab | $H, K, T$ | Workload DAG, Capability IPC, `shelld` |
| **EXP-05** | Surface-Independent Workload Survival | Family 5 (Long-Running) | Failure & Recovery, Offline Operation | $R, T$ | Resource Leases, Stage 6F Fencing, `workspaced` |
| **EXP-06** | Dynamic Intent Pivot & Resource Release | Family 6 (Intent Changes) | Human-Agent Collab | $H, R, T$ | `intentd`, DAG Re-planning, Lease Revocation |
| **EXP-07** | Transparent Remote Compute Acceleration | Family 7 & 8 (Resource / Dist.) | Failure & Recovery, Offline Deferral | $K, T$ | `fabricd`, Stage 4F Identity, Fencing Proofs |

---

## 6. Detailed Experiment Specifications

---

### EXP-01: Technical Report & Asset Assembly

#### 1. Experiment ID and Name
`EXP-01`: Technical Report & Asset Assembly (Creation Family)

#### 2. Real Human Problem
A user needs to compile a multi-section status report containing benchmark metrics, architectural notes, and visual system diagrams based on raw data located in their workspace.

#### 3. Why This Problem Is Representative
Turning fragmented raw data into a structured output artifact is one of the most common productivity tasks on desktop computers.

#### 4. Conventional-Computer Baseline
On a conventional OS, a user must:
1. Open file manager and navigate to metrics directory.
2. Open terminal and run a log parsing script.
3. Copy raw parsed metrics to clipboard.
4. Open text editor / word processor and paste text.
5. Open diagramming tool, export image file to disk.
6. Open word processor, select "Insert Image", navigate to file path.
7. Save final document to a specific directory path.
*(Baseline: 7 explicit user steps, 3 manual app switches, 2 manual copy/paste transfers).*

#### 5. Exact User Goal
"Generate a formatted system status report summarizing Q3 benchmark logs and attach the network topology diagram."

#### 6. Starting State
Workspace contains raw log files (`/logs/q3-bench.log`), diagram file (`/assets/topology.png`), and an active spatial desktop session with editor and terminal open.

#### 7. User Actions
1. User types or speaks goal: "Generate a formatted system status report summarizing Q3 benchmark logs and attach the network topology diagram."
2. User reviews proposed execution plan from `intentd`.
3. User confirms plan with 1 click/keystroke.

#### 8. Expected ZeroOS Behavior
- `intentd` parses user intent and generates a Workload DAG plan.
- `workspaced` executes DAG nodes to parse `/logs/q3-bench.log` using standard user-space tools via IPC.
- `shelld` embeds `/assets/topology.png` and rendered markdown inside the workspace surface.
- The user is never prompted for destination file paths or application launch commands.

#### 9. Existing ZeroOS Components Exercised
`intentd`, `workspaced`, `shelld`, Workload DAG, Capability Tokens (`Stage 3H`), Persistent Workspace.

#### 10. What Must Already Work
- User-space log parser utility and markdown renderer binary in `/bin`.
- `intentd` plan generation and `workspaced` DAG dispatch.
- Capability delegation for file read access.
- *Application Integration Boundary:* Legacy/traditional document applications may require a ZeroOS IPC adapter/wrapper. The experiment evaluates the ZeroOS orchestration boundary (intent → Workload DAG → capability delegation) independently from application-specific integration.

#### 11. Evidence to Collect
- Number of human interventions ($H$).
- Number of manual copy-paste transfers ($K$).
- Technical terms exposed to user ($T$).
- Verification that PMM memory leaks $= 0$.

#### 12. Evaluation Dimensions Involved
- $H$: Target $= 1$ (confirmation action).
- $K$: Target $= 0$ (zero manual data copy/paste).
- $T$: Target $= 0$ (no process IDs, IPC handles, or file path flags exposed).

#### 13. Success Criteria
- Formatted report created and rendered in workspace within 10 seconds.
- $H \le 1, K = 0, T = 0$.
- Zero kernel memory leaks.

#### 14. Failure Criteria
- User is forced to select application binaries or manually pipe log output ($H > 2, K > 0$).
- `workspaced` or `shelld` crashes during execution.

#### 15. Failure Classification
- **Cat A:** Plan confirmation failed due to UI input focus timing.
- **Cat B:** Missing Application Integration — Markdown renderer app wrapper or IPC protocol adapter missing/failed. (Failure of an application to ingest capability-mediated input is Category B and must NOT be treated as a ZeroOS architectural defect).
- **Cat C:** `intentd` DAG builder failed to parse log filename.
- **Cat D:** Capability engine failed to grant file read access token.
- **Cat E:** Automated report generation proved less useful than direct manual editing.

#### 16. What The Experiment Must NOT Assume
Does not assume arbitrary legacy applications automatically support ZeroOS IPC without wrappers; does not assume LLM model generates perfect prose. Evaluates structural document assembly and OS orchestration.

#### 17. How To Keep Small & Reproducible
Use fixed 50KB test log file and static PNG image asset inside deterministic QEMU test harness script.

---

### EXP-02: Multi-Source Context Synthesis

#### 1. Experiment ID and Name
`EXP-02`: Multi-Source Context Synthesis (Information Family)

#### 2. Real Human Problem
A user is debugging an error and needs to synthesize context from three visible surface windows: an active log viewer, a terminal error traceback, and an open text configuration file.

#### 3. Why This Problem Is Representative
Debugging and information synthesis require integrating state scattered across multiple active applications on screen.

#### 4. Conventional-Computer Baseline
User must switch focus to Terminal, select error text, copy it, switch to Text Editor, find relevant config line, switch to Log Viewer, correlate timestamps, and mentally synthesize the root cause.
*(Baseline: 5 app focus switches, 3 copy-paste operations, high mental context tracking).*

#### 5. Exact User Goal
"Based on the visible error traceback and config file, why is the network service rejecting connections?"

#### 6. Starting State
Spatial desktop displaying Terminal window with error traceback, Editor window with `/etc/net.conf`, and Log Viewer with `/var/log/net.log`.

#### 7. User Actions
1. User inputs query: "Based on the visible error traceback and config file, why is the network service rejecting connections?"

#### 8. Expected ZeroOS Behavior
- `groundd` captures spatial workspace context under `WorkspaceAccessCap` masking sensitive fields.
- `intentd` correlates active terminal error string (`port 8080 bound`) with configuration entry (`listen_port = 8080`).
- System presents clear answer: "Connection rejected because `net.conf` specifies port 8080, which is already bound by process PID 402 shown in Log Viewer."
- User performs 0 window switches or copy-paste transfers.

#### 9. Existing ZeroOS Components Exercised
`groundd`, `intentd`, `WorkspaceAccessCap`, `shelld` spatial layout broker.

#### 10. What Must Already Work
- `groundd` spatial read-only observation interface (`Stage 6F`).
- Redaction policy for auth/sensitive fields.
- Workspace capability check.

#### 11. Evidence to Collect
- Count of user window focus switches ($K$).
- Count of manual text selections/copies ($C$).
- Audit log verifying `groundd` obeyed read-only observation boundary.

#### 12. Evaluation Dimensions Involved
- $H$: Target $= 1$ (query input).
- $C$: Target $= 0$ (user does not need to memorize or re-type error tracebacks).
- $K$: Target $= 0$ (zero focus switches).

#### 13. Success Criteria
- Correct root cause identified using spatial context.
- $H = 1, C = 0, K = 0$.
- `groundd` performed 0 workspace state mutations.

#### 14. Failure Criteria
- System requires user to select text or specify window IDs ($K > 0, T > 0$).
- Sensitive redacted data leaked in context snapshot.

#### 15. Failure Classification
- **Cat A:** Spatial coordinate lookup timing out in test harness.
- **Cat B:** Terminal app text buffer export adapter missing.
- **Cat C:** `groundd` failed to filter out background window text.
- **Cat D:** `WorkspaceAccessCap` token validation defect.
- **Cat E:** Context lookup slower than manual inspection by human.

#### 16. What The Experiment Must NOT Assume
Does not assume LLM has infinite context window; relies on targeted spatial grounding extracts from `groundd`.

#### 17. How To Keep Small & Reproducible
Run in headless QEMU with mock framebuffer layout containing fixed text snippets for terminal, editor, and log viewer.

---

### EXP-03: Zero-Reconstruction Session Recovery

#### 1. Experiment ID and Name
`EXP-03`: Zero-Reconstruction Session Recovery (Context Continuity Family)

#### 2. Real Human Problem
A user is in the middle of a complex multi-tool analysis task when system power is abruptly cut or the OS kernel crashes.

#### 3. Why This Problem Is Representative
Unplanned interruptions are a major cause of productivity loss, requiring users to spend significant time reconstructing task context.

#### 4. Conventional-Computer Baseline
Upon reboot, modern conventional OSes may reopen closed application windows, but they do not restore active execution graphs, command parameters, background workload state, or in-flight task goals. User must re-navigate directories, re-run scripts, and re-establish mental state.
*(Baseline: 10-15 manual reconstruction steps, high recovery effort $R$).*

#### 5. Exact User Goal
Re-establish full productive working context after sudden system power loss during an active workload.

#### 6. Starting State
ZeroOS running active Workspace with 2 open application surfaces and 1 running workload DAG node.

#### 7. User Actions
1. Test harness triggers hard system reset (`QEMU system_reset`).
2. System reboots into ZeroOS.
3. User logs in.

#### 8. Expected ZeroOS Behavior
- System initializes Stage 6F session recovery.
- `workspaced` validates `LogicalSessionSnapshotHeader` and fencing proof.
- Workspace spatial layout, active surfaces, open documents, and running workload graph state are restored automatically.
- User receives notification: "Workspace restored. Workload task 4 resumed at step 2/4."

#### 9. Existing ZeroOS Components Exercised
`workspaced`, `shelld`, Stage 6F `LogicalSessionSnapshotHeader`, Stage 6F Fencing Proofs, PMM frame accounting.

#### 10. What Must Already Work
- Stage 6F session snapshot persistence.
- Fencing proof verification (`receive_fencing_proof`).
- Workspace recovery logic in `workspaced`.
- *Recovery Boundary Distinction:* Explicitly separate ZeroOS-managed recovery (workspace/session metadata, spatial window layout, Workload DAG state, ZeroOS-owned persistent context) from Application-owned state (unsaved editor buffers, internal application memory, transient state for apps without a snapshot protocol). Application-internal state loss is Category B (Missing Application Integration) unless demonstrably caused by a ZeroOS recovery primitive defect.

#### 11. Evidence to Collect
- Time to restore workspace state (seconds).
- User actions required to resume work ($R$).
- Verification that PMM frame leak $= 0$.

#### 12. Evaluation Dimensions Involved
- $C$: Target $= 0$ (zero context state forgotten).
- $R$: Target $= 0$ (zero manual recovery actions).
- $T$: Target $= 0$ (no fencing nonces or snapshot offsets exposed).

#### 13. Success Criteria
- 100% spatial layout, workspace context, and workload DAG state restored without data loss.
- $C = 0, R = 0, T = 0$.
- Memory neutral post-restoration.

#### 14. Failure Criteria
- Restored workspace loses workload state or drops active capability tokens.
- Fencing proof validation fails on legitimate recovery, causing split-brain state.

#### 15. Failure Classification
- **Cat A:** QEMU disk sync failed before hard reset.
- **Cat B:** Missing Application Integration — Legacy app internal buffer loss or surface compositor failed to restore window dimensions. (Application-internal state loss is Category B and must NOT be treated as a ZeroOS architectural defect).
- **Cat C:** `workspaced` recovery parser rejected valid snapshot header.
- **Cat D:** Fencing proof nonce replay detection flaw.
- **Cat E:** Session restoration deemed unwanted by users who prefer clean reboots.

#### 16. What The Experiment Must NOT Assume
Does not assume ZeroOS automatically restores arbitrary application internal memory buffers; does not assume hardware nvram persistence. Evaluates whether ZeroOS restores overall computing CONTEXT and ongoing work without forcing human manual reconstruction ($C = 0, R = 0$).

#### 17. How To Keep Small & Reproducible
Automate QEMU power reset via test script after writing 1MB dummy workload state snapshot to disk image.

---

### EXP-04: Coordinated Data Processing Pipeline

#### 1. Experiment ID and Name
`EXP-04`: Coordinated Data Processing Pipeline (Multi-App Coordination Family)

#### 2. Real Human Problem
A user needs to filter a raw telemetry dataset, transform records, generate a statistical summary, and plot a visual chart.

#### 3. Why This Problem Is Representative
Multi-stage data transformation requires orchestrating multiple distinct applications and passing intermediate data between them.

#### 4. Conventional-Computer Baseline
User runs command line filter tool, outputs `data_filtered.csv`, opens spreadsheet app, imports CSV, calculates summary statistics, exports `chart.png`, opens image viewer.
*(Baseline: 6 manual steps, 3 intermediate files created on disk, high manual coordination $K$).*

#### 5. Exact User Goal
"Filter `/data/telemetry.csv` for error code 500, compute error frequency by hour, and generate a plot on screen."

#### 6. Starting State
Workspace contains raw dataset `/data/telemetry.csv`. Filter tool, statistics tool, and plotting tool binaries are available in `/bin`.

#### 7. User Actions
1. User inputs goal: "Filter `/data/telemetry.csv` for error code 500, compute error frequency by hour, and generate a plot on screen."
2. User clicks "Confirm Plan".

#### 8. Expected ZeroOS Behavior
- `intentd` constructs a 3-node Workload DAG:
  `FilterNode (telemetry.csv) -> StatsNode -> PlotNode`.
- `workspaced` executes nodes sequentially passing data via capability-gated IPC handles without writing temporary files to public paths.
- `shelld` renders the final plot inside an interactive surface view.

#### 9. Existing ZeroOS Components Exercised
`intentd`, `workspaced`, `shelld`, Workload DAG execution, Capability IPC pipes (`Stage 3H`).

#### 10. What Must Already Work
- Capability-gated IPC pipe creation and transfer.
- Sequential DAG node execution in `workspaced`.
- Surface image rendering in `shelld`.

#### 11. Evidence to Collect
- Count of temporary files left in user workspace.
- Count of manual data piping steps ($K$).
- Total end-to-end execution time.

#### 12. Evaluation Dimensions Involved
- $H$: Target $= 1$ (plan confirmation).
- $K$: Target $= 0$ (zero manual file piping or IPC setup).
- $T$: Target $= 0$ (no IPC handle IDs exposed to user).

#### 13. Success Criteria
- Plot generated and displayed cleanly in workspace.
- $H = 1, K = 0, T = 0$.
- Zero orphan processes or leak of intermediate files.

#### 14. Failure Criteria
- System requires user to specify intermediate file names or shell pipe syntax (`|`).
- Capability check fails on inter-node IPC transfer.

#### 15. Failure Classification
- **Cat A:** Pipe buffer overflow due to small test dataset buffer.
- **Cat B:** Plotting app binary IPC pipe reader adapter missing.
- **Cat C:** `workspaced` DAG engine failed to pass IPC capability token to child node.
- **Cat D:** Capability engine lacks support for anonymous IPC pipe caps.
- **Cat E:** User prefers manual command line piping over automated DAG.

#### 16. What The Experiment Must NOT Assume
Does not assume custom non-standard Unix utilities; uses simple stdin/stdout stream interfaces wrapped in capability handles.

#### 17. How To Keep Small & Reproducible
Use synthetic 1000-line CSV telemetry file and standard lightweight C data processing binaries in QEMU build.

---

### EXP-05: Surface-Independent Workload Survival

#### 1. Experiment ID and Name
`EXP-05`: Surface-Independent Workload Survival (Long-Running Work Family)

#### 2. Real Human Problem
A user starts a long-running 20-minute data compilation task, then closes their laptop lid, disconnects the surface display, or loses network connection while switching work locations.

#### 3. Why This Problem Is Representative
Users frequently move between environments or close display surfaces while compute jobs run in the background.

#### 4. Conventional-Computer Baseline
Closing a laptop lid or disconnecting an SSH/GUI session on conventional systems often suspends or kills active foreground processes, unless the user manually configured background daemons (`tmux`, `nohup`, `disown`).
*(Baseline: 4 pre-planning steps, risk of task loss if forgotten).*

#### 5. Exact User Goal
Launch a long-running data compilation job, disconnect the display surface, and verify the job completes safely in the background.

#### 6. Starting State
Active Workspace running a long-running multi-node compilation task managed by `workspaced` with active Resource Lease (`Stage 4F`).

#### 7. User Actions
1. User starts compilation task.
2. Test harness terminates `shelld` / surface display connection to simulate closing laptop lid / display disconnect.
3. 30 seconds later, test harness reconnects `shelld` surface display.

#### 8. Expected ZeroOS Behavior
- `workspaced` detects surface disconnect but maintains workload execution under valid Resource Lease.
- Compilation task proceeds unhindered in background.
- Upon surface reconnection, `shelld` attaches to active workspace session and displays progress (e.g., "Compilation complete: 100%").

#### 9. Existing ZeroOS Components Exercised
`workspaced`, Resource Leases (`Stage 4F`), `shelld` surface lifecycle, Stage 6F session continuity.

#### 10. What Must Already Work
- Decoupled architecture where `workspaced` is independent of `shelld` process lifecycle.
- Resource lease renewal in `workspaced`.

#### 11. Evidence to Collect
- Workload status telemetry during surface disconnect.
- Count of user steps required to resume monitoring ($R$).
- Memory neutral verification.

#### 12. Evaluation Dimensions Involved
- $R$: Target $= 0$ (zero manual process re-attachment steps).
- $T$: Target $= 0$ (no lease IDs or daemon PIDs exposed).

#### 13. Success Criteria
- Workload completes successfully during surface disconnect.
- Surface reconnection immediately displays updated task state.
- $R = 0, T = 0$.

#### 14. Failure Criteria
- Surface termination causes `workspaced` to abort active workloads.
- Reconnecting surface results in blank screen or lost workspace context.

#### 15. Failure Classification
- **Cat A:** Test harness socket disconnect timeout too short.
- **Cat B:** Display compositor failed to re-bind framebuffer.
- **Cat C:** `workspaced` tied workload lifecycle to surface socket connection.
- **Cat D:** Resource lease engine automatically revokes lease on surface disconnect.
- **Cat E:** Users prefer workloads to pause automatically when display is closed.

#### 16. What The Experiment Must NOT Assume
Does not require remote cloud infrastructure; operates entirely on local node resource lease architecture.

#### 17. How To Keep Small & Reproducible
Simulate long-running job with 10-second sleep loop inside QEMU harness.

---

### EXP-06: Dynamic Intent Pivot & Resource Release

#### 1. Experiment ID and Name
`EXP-06`: Dynamic Intent Pivot & Resource Release (Mid-Task Intent Changes Family)

#### 2. Real Human Problem
Five minutes into running a CPU-intensive data indexing job, the user realizes they passed the wrong parameter and wants to cancel the job, release system resources, and start a different query.

#### 3. Why This Problem Is Representative
Human users constantly change their minds or correct mistakes mid-execution.

#### 4. Conventional-Computer Baseline
User must open Process Monitor or Terminal, find process name/PID, issue `kill -9`, check disk for leftover temporary files, manually delete them, and type new command.
*(Baseline: 5 manual steps, technical PID manipulation $T > 0$).*

#### 5. Exact User Goal
"Stop the current indexing job immediately and run a fast summary instead."

#### 6. Starting State
Active Workspace executing a 4-node CPU-intensive indexing DAG in `workspaced` holding active resource leases.

#### 7. User Actions
1. User issues voice/text command: "Stop the current indexing job immediately and run a fast summary instead."
2. User confirms re-plan.

#### 8. Expected ZeroOS Behavior
- `intentd` intercepts intent change and instructs `workspaced` to cancel running DAG nodes.
- `workspaced` revokes sub-node capability tokens and releases Resource Leases back to system pool.
- `intentd` constructs new summary DAG and launches execution.
- No orphan processes, stale lock files, or leaked memory frames remain.

#### 9. Existing ZeroOS Components Exercised
`intentd`, `workspaced`, Workload DAG cancellation, Capability revocation (`Stage 3H`), Resource Lease release (`Stage 4F`).

#### 10. What Must Already Work
- DAG node cancellation protocol in `workspaced`.
- Capability token revocation syscall / interface.
- Resource lease return in `resourced`.

#### 11. Evidence to Collect
- Resource lease pool count before, during, and after cancellation.
- PMM frame accounting (verify 0 frame leaks after cancellation).
- Count of user operational steps ($H$).

#### 12. Evaluation Dimensions Involved
- $H$: Target $= 1$ (re-plan confirmation).
- $R$: Target $= 0$ (zero manual cleanup steps).
- $T$: Target $= 0$ (no PIDs or kill signals exposed).

#### 13. Success Criteria
- Active DAG cleanly canceled within 500ms; new DAG launched.
- Resource leases released completely.
- $H = 1, R = 0, T = 0$, zero memory leaks.

#### 14. Failure Criteria
- Canceled DAG nodes continue running as orphan background processes.
- Memory or resource leases remain leaked after cancellation.

#### 15. Failure Classification
- **Cat A:** Timing race in test harness output check.
- **Cat B:** User-space tool app wrapper ignores SIGTERM/cancellation IPC.
- **Cat C:** `workspaced` cancellation handler missing lease release call.
- **Cat D:** Capability engine lacks capability revocation primitive.
- **Cat E:** Users prefer letting jobs finish rather than canceling midway.

#### 16. What The Experiment Must NOT Assume
Does not assume instant kill without state cleanup; verifies structured DAG node cancellation and lease release.

#### 17. How To Keep Small & Reproducible
Use dummy loop process that logs cancellation events to serial console in QEMU.

---

### EXP-07: Transparent Remote Compute Acceleration

#### 1. Experiment ID and Name
`EXP-07`: Transparent Remote Compute Acceleration (Resource Complexity & Distributed Computing Families)

#### 2. Real Human Problem
A user needs to execute a heavy computational analysis (e.g., dataset build or model compilation) that exceeds local machine CPU capacity, but secondary compute nodes are available on the local network.

#### 3. Why This Problem Is Representative
Leveraging multi-machine compute resources without complex cluster management is a major barrier in personal and enterprise computing.

#### 4. Conventional-Computer Baseline
User must configure SSH keys, set up remote directory paths, scrip sync files (`rsync`), log into remote machine, execute command, wait, and scp output back.
*(Baseline: 8 manual steps, complex network/SSH setup, exposed node IPs $T > 0, K > 0$).*

#### 5. Exact User Goal
"Run a full system benchmark compilation using all available compute power."

#### 6. Starting State
Dual-node ZeroOS setup (Node A local desktop, Node B headless compute node linked via local fabric network). Node A holds active workspace.

#### 7. User Actions
1. User inputs goal: "Run a full system benchmark compilation using all available compute power."
2. User confirms execution.

#### 8. Expected ZeroOS Behavior
- `workspaced` on Node A evaluates resource requirements and queries `fabricd` for available remote node capacity.
- Node A negotiates remote resource lease on Node B using Stage 4F node identity and Stage 6F fencing proofs.
- Heavy compilation DAG nodes are transparently routed to Node B over `fabricd`.
- Output artifacts stream back to Node A workspace seamlessly.
- User never enters IP addresses, SSH commands, or node identifiers ($T = 0, K = 0$).

#### 9. Existing ZeroOS Components Exercised
`fabricd`, `workspaced`, Stage 4F authenticated node identity, Stage 6F fencing proofs (`receive_fencing_proof`), Resource Graph.

#### 10. What Must Already Work
- `fabricd` node discovery and authenticated transport (`Stage 4F`).
- Cross-node fencing proof validation (`Stage 6F`).
- Remote DAG node dispatch in `workspaced`.
- *Remote DAG Routing Boundary:* User-space remote DAG scheduling and routing in `workspaced` is a prerequisite for EXP-07. Subsystem roles are strictly divided: `fabricd` is transport/discovery/identity infrastructure, while `workspaced` is workload/DAG execution authority. ZeroOS hides node IDs, IP addresses, SSH keys, credentials, and topology details from the human user. If `workspaced` lacks remote scheduling logic, it must be classified as Category C (Missing User-Space Implementation) and NOT as a Stage 4F/6F architectural defect. The experiment evaluates the product experience once frozen distributed-fabric contracts are exercised end-to-end.

#### 11. Evidence to Collect
- Serial console logs on Node A and Node B confirming cross-node execution.
- Total wall-clock time compared to local-only execution.
- Technical terms exposed to user ($T$).

#### 12. Evaluation Dimensions Involved
- $K$: Target $= 0$ (zero manual file sync or SSH setup).
- $T$: Target $= 0$ (zero node IDs or IP addresses exposed to user).

#### 13. Success Criteria
- Computation completes successfully offloading tasks to Node B.
- Wall-clock time reduced compared to local execution.
- $K = 0, T = 0$.

#### 14. Failure Criteria
- System prompts user for remote IP address, password, or SSH key.
- Fencing proof verification fails, rejecting legitimate remote execution.

#### 15. Failure Classification
- **Cat A:** QEMU tap network interface link down.
- **Cat B:** Remote binary tool version mismatch between nodes.
- **Cat C:** Missing User-Space Implementation — `workspaced` remote DAG node scheduling/dispatch logic incomplete or `fabricd` message serializing defect in remote DAG payload. (Missing remote DAG routing in user-space `workspaced` is Category C and must NOT be treated as a Stage 4F/6F architectural defect).
- **Cat D:** Fencing proof validation defect in multi-node epoch handshake.
- **Cat E:** Distributed offloading overhead exceeds local computation speedup.

#### 16. What The Experiment Must NOT Assume
Does not assume public internet connectivity; does not invent new routing architecture. Operates over local authenticated fabric network contracts.

#### 17. How To Keep Small & Reproducible
Run dual QEMU instances (`node_a` and `node_b`) connected via virtual socket network script.

---

## 7. Cross-Cutting Constraints Matrix

Every experiment must adhere to the 4 cross-cutting constraints:

| Experiment ID | Offline Operation | Traditional Apps | Failure & Recovery | Human-Agent Collab |
|---|---|---|---|---|
| **EXP-01** | Local assets only (100% offline) | Native CLI/MD apps | Clean abort on error | Plan review & confirmation |
| **EXP-02** | Local spatial layout (100% offline) | Legacy window buffers | Read-only isolation | Natural query interface |
| **EXP-03** | Local snapshot (100% offline) | Surface session restore | Hard reset recovery | Auto-resume with status |
| **EXP-04** | Local data files (100% offline) | Std pipe binaries | No orphan files left | Single plan confirmation |
| **EXP-05** | Local workload (100% offline) | Background process | Surface drop survival | Status view on reconnect |
| **EXP-06** | Local execution (100% offline) | Canceled CLI tool | Resource lease release | Dynamic mid-task pivot |
| **EXP-07** | Local fabric (degrades if offline) | Distributed binaries | Fencing proof safety | Transparent compute offer |

---

## 8. Evidence Collection Methodology

For each experiment execution, the automated test harness generates a structured log: `ZEROOS-VALIDATION-LOG-<EXP_ID>.json`:

```json
{
  "experiment_id": "EXP-01",
  "timestamp": "2026-10-07T01:30:00Z",
  "metrics": {
    "H_human_actions": 1,
    "C_context_memory_steps": 0,
    "K_coordination_actions": 0,
    "R_recovery_steps": 0,
    "T_exposed_technical_terms": 0
  },
  "subsystem_verification": {
    "pmm_leaked_frames": 0,
    "stage3_nucleus_modified_bytes": 0,
    "groundd_mutations_attempted": 0,
    "fabricd_unauthenticated_messages": 0
  },
  "verdict": "PASS",
  "failure_category": null
}
```

---

## 9. Failure Classification Methodology

When an experiment fails, the failure MUST be categorized using the decision framework:

```text
                        FAILURE DECISION TREE

                  Did the experiment fail?
                             │
                             ▼
             Is the failure due to test harness setup?
                           ╱   ╲
                         YES    NO
                         ╱       ╲
                        ▼         ▼
                    Category A   Is it an app wrapper issue?
                   (Test Setup)          ╱   ╲
                                       YES    NO
                                       ╱       ╲
                                      ▼         ▼
                                 Category B   Can existing architecture
                               (Integration)  support it via daemon logic?
                                                   ╱   ╲
                                                 YES    NO
                                                 ╱       ╲
                                                ▼         ▼
                                           Category C   Category D
                                          (User-space)  (Architecture)
```

- **Category A:** Test harness script timing, missing test file, QEMU configuration error.
- **Category B:** Application wrapper missing or IPC protocol mismatch with traditional app.
- **Category C:** Daemon logic missing (e.g., `intentd` DAG parser edge case) using existing primitives.
- **Category D:** Genuine architectural gap requiring kernel or capability primitive revision.
- **Category E:** The feature works, but provides zero user value or increases friction.

---

## 10. Confounders

To ensure valid results, experiments must explicitly isolate and control for three major confounders:

1. **LLM Model Intelligence:** An error in LLM reasoning must NOT be counted as an OS architecture failure. Mock/deterministic plan generators must be available to test OS primitives independently of model variance.
2. **Application Wrapper Quality:** A bug in a third-party application integration bridge must be classified as **Category B**, not an OS architecture failure.
3. **Hardware / QEMU Performance:** Slow disk IO or emulation lag in QEMU must NOT be confused with architectural overhead. Metrics focus on action counts ($H, K, R$) and state integrity rather than raw wall-clock milliseconds.

---

## 11. What Constitutes Convincing Evidence

Evidence is convincing ONLY if:
1. **$H, C, K, R, T$ Metrics Dramatically Outperform Baseline:** $H \le 1$, $C = 0$, $K = 0$, $R = 0$, $T = 0$ across diverse experiments.
2. **Multi-Category Repeatability:** The complexity transfer is demonstrated across Creation, Information, Continuity, Coordination, Long-Running, Intent Changes, and Distributed computing.
3. **Subsystem Neutrality Preserved:** Zero kernel memory leaks, zero `groundd` mutations, zero `fabricd` capability derivations.

---

## 12. What Would Invalidate the ZeroOS Hypothesis

The ZeroOS hypothesis is **INVALIDATED** if validation demonstrates:
1. **Complexity Shift, Not Reduction:** The user simply exchanges manual file management for tedious prompt engineering and plan debugging ($H \ge 4$).
2. **Category D Dominance:** Validation encounters repeated Category D architectural failures that cannot be resolved without reinventing conventional OS abstractions.
3. **Brittleness:** Minor interruptions or application failures corrupt the entire workspace context ($R \ge 5$).

---

## 13. Stop Conditions & Anti-Scope-Creep Rules

1. **NO CODE IMPLEMENTATION IN THIS PHASE:** This document specifies validation tests ONLY.
2. **NO NEW SUBSYSTEMS:** No new daemons (`daemond`, `serviced`) may be proposed to fix a test failure.
3. **MAX 7 EXPERIMENTS:** Do not expand the test suite beyond these 7 core representative experiments.

---

## 14. Final Decision Table & Verdict

### 14.1 Decision Table

| Validation Result | Meaning | Required Next Action |
|---|---|---|
| **EXP-01 through EXP-07 PASS** | Existing architecture successfully absorbs computing complexity. | Proceed to execute Product Proof validation suite. |
| **Failures are mostly Category B** | Application integration is the main bottleneck. | Build lightweight application wrappers; do NOT touch OS architecture. |
| **Failures are mostly Category C** | Existing architecture is sufficient but user-space daemon logic is incomplete. | Complete missing daemon logic within frozen Stage 3A–6F contracts. |
| **Any strong Category D evidence** | Core OS architectural primitive is defective or missing. | Stop immediately and conduct architectural review. |
| **Failures are mostly Category E** | The product complexity-transfer hypothesis does not provide user value. | Re-evaluate product direction. |

---

### 14.2 Final Verdict

```text
================================================================================
                    ZEROOS PRODUCT PROOF EXPERIMENT DESIGN

  Architecture Readiness (Stages 3A–6F):    🟢 COMPLETE & FROZEN
  Scenario Coverage (7 Core Experiments):   🟢 100% COVERAGE
  Subsystem Boundaries Preserved:            🟢 100% PRESERVED
  No New Architectural Expansion Required:  🟢 ENFORCED
================================================================================
```

## 🟢 READY FOR EXECUTION

The experiment design specification is complete, rigorous, and fully grounded in the frozen ZeroOS architecture (`docs/design/ZEROOS-PRODUCT-PROOF.md`). No implementation work, new primitives, or Stage 7 abstractions are authorized. The system is ready to execute validation testing.
