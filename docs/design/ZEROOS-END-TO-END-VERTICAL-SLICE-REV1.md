# ZEROOS END-TO-END VERTICAL SLICE REV1

**Authoritative Design & Integration Demonstration Specification**  
**Target Document**: `docs/design/ZEROOS-END-TO-END-VERTICAL-SLICE-REV1.md`  
**Status**: 🟢 PROPOSED VERTICAL SLICE SPECIFICATION  
**Kernel Code Changes**: 0  
**New Syscalls**: 0  
**ABI Modifications**: 0  

---

## 1. OBJECTIVE

The objective of **ZEROOS END-TO-END VERTICAL SLICE REV1** is to provide a single, deterministic, end-to-end integration demonstration proving that all 10 frozen ZeroOS layers compose into **one coherent, human-intent-first operating system**.

The demonstration traces a single logical workload from user human intent down through workspace isolation, plan DAG construction, workload materialization, resource admission, process execution, telemetry observation, deliberate resource failure, closed-loop replanning (Plan v2), state snapshotting, atomic migration to a target node, destination execution, and final completion.

```text
USER INTENT ──► WORKSPACE ──► PLAN v1 ──► WORKLOAD ──► ADMISSION ──► EXECUTION (Node A)
                                                                           │
                                                                  (Resource Failure)
                                                                           │
                                                                           ▼
COMPLETION (Node B) ◄── EXECUTION ◄── COMMIT ◄── MIGRATION ◄── PLAN v2 ◄── OBSERVATION
```

---

## 2. CONTROLLED DEMONSTRATION WORKLOAD

The demonstration uses a ZeroOS-native deterministic workload: **`DatasetAnalyticsPipeline`**.

### Workload Characteristics
- **Human Goal**: Process a dataset stored in a workspace, evaluate statistical aggregations, survive a thermal/resource node fault, migrate execution to a secondary node, and produce a final summary object.
- **Components**:
  1. `PrepareDatasetTask`: Validates input object membership in workspace.
  2. `ComputeAnalyticsTask`: State-intensive processing task that encounters a deliberate node resource fault.
  3. `FinalizeReportTask`: Writes the final output summary object to the workspace filesystem.

---

## 3. HUMAN INTENT

The user submits a human intent through the `intentd` gateway within the target workspace `WS_DEMO_01`:

```text
IntentId:         DistributedId { node_id: 1, local_seq: 1001 }
WorkspaceId:      DistributedId { node_id: 1, local_seq: 500 }
Intent Text:      "Process dataset alpha and generate statistical analytics report."
Input Object:     ObjectId { node_id: 1, local_seq: 8080 } ("dataset_alpha.dat")
Output Object:    ObjectId { node_id: 1, local_seq: 8081 } ("analytics_report.json")
Creation Epoch:   TSC 10000000
Approval Policy:  Automatic (Policy-approved within WS_DEMO_01 quota)
```

---

## 4. WORKSPACE CONTEXT

The workload operates strictly within workspace `WS_DEMO_01`:

```text
WorkspaceId:                     DistributedId { node_id: 1, local_seq: 500 }
State:                           Active
Capability Envelope Handle:      0x00F1
Root Directory Handle:           0x00A0
Associated Membership Edges:     [ MEMBER_OF(WS_DEMO_01, dataset_alpha.dat) ]
Isolation Boundary:              Absolute (No cross-workspace leakage)
```

---

## 5. INTENT DAG

`intentd` constructs the declarative Intent DAG:

```text
Intent Node 1: Prepare (TaskType: 1, Priority: 1)
     │
     ▼
Intent Node 2: Compute (TaskType: 2, Priority: 2)
     │
     ▼
Intent Node 3: Finalize (TaskType: 3, Priority: 1)
```

### Graph Separation
- `Intent DAG`: Represents human goals (`IntentNodeId`).
- `Plan DAG`: Represents versioned orchestration steps (`PlanStepId`).
- `Workload Execution DAG`: Materialized task descriptors (`TaskDescriptor`).
- `Resource Graph`: Physical node capacities (`NodeId`, `ResourceId`).

---

## 6. PLAN v1 CONSTRUCTION

`pland` evaluates the Intent DAG and produces **Plan v1** (`PlanId_v1`):

```text
PlanId:           DistributedId { node_id: 1, local_seq: 2001 }
Version:          1
PlanStep 1:       StepPrepare (Dependencies: None, CapRights: READ, Memory: 1GB)
PlanStep 2:       StepCompute (Dependencies: StepPrepare, CapRights: READ|WRITE, Memory: 4GB, CPU: 4 cores)
PlanStep 3:       StepFinalize (Dependencies: StepCompute, CapRights: MUTATE, Memory: 1GB)
Failure Policy:   ReplanOnResourceFault
```

---

## 7. WORKLOAD MATERIALIZATION

`workloadd` receives `PlanId_v1` and materializes the Workload Control Block:

```text
WorkloadId:               DistributedId { node_id: 1, local_seq: 3001 }
Originating IntentId:     DistributedId { node_id: 1, local_seq: 1001 }
PlanId:                   DistributedId { node_id: 1, local_seq: 2001 }
State:                    Creating ──► Queued ──► Admitted
Tasks:                    [ Task 0 (Prepare), Task 1 (Compute), Task 2 (Finalize) ]
Materialization Rule:     Deterministic & Idempotent (Same PlanStepId -> Same TaskId)
```

---

## 8. RESOURCE ADMISSION & LEASE GRANT

`workloadd` submits `TaskResourceDemand` to `resourced`:

```text
NodeId (Source):          NodeId { node_id: 1, boot_epoch: 100 }
Demand:                   4 CPU Cores, 4GB RAM, Locality: LocalNode
Admission Status:         ADMITTED
ResourceId:               DistributedId { node_id: 1, local_seq: 4001 }
ResourceLease:            ResourceLease { lease_id: 9001, expire_tsc: TSC + 500000 }
Placement:                Node 1 (Source Host)
```

---

## 9. INITIAL EXECUTION

`workloadd` spawns local host processes for Node 1:

```text
ExecutionId:              DistributedId { node_id: 1, local_seq: 5001 }
ProcessId (Source):       1024 (pid_t on Node 1)
CapabilityHandle (Local): 0x0010 (READ right on dataset_alpha.dat)
Execution State:          Running
Task 0 (Prepare):         Completed
Task 1 (Compute):         Running (Process 1024 actively computing)
```

$$\text{ActiveExecutions}(\text{WorkloadId}_{3001}) = 1$$

---

## 10. OBSERVATION TELEMETRY

`workloadd` logs execution events; `observed` projects monotonic telemetry:

```text
ExecutionEventId:         Event 7001 (EVENT_TASK_STARTED, Task 1, Process 1024)
ObservationId:            Obs 8001
Sequence Number:          101 (Monotonic)
Source Node:              NodeId { node_id: 1, boot_epoch: 100 }
Telemetry Status:         Process 1024 healthy, memory 2.1GB, CPU 98%
```

---

## 11. DELIBERATE FAILURE (RESOURCE LOSS)

To exercise the fault-handling pipeline, Node 1 encounters a thermal/resource fault:

```text
Trigger:                  Thermal Overheat on Node 1 (Temp > 85°C)
Event:                    EVENT_RESOURCE_DEGRADED (Node 1 capacity drop)
ObservationId:            Obs 8002 (OBSERVATION_THERMAL_CRITICAL)
Execution State:          Task 1 Quiesced (Process 1024 frozen)
Telemetry Emitted:        Node 1 thermal pressure -> Replanning Engine notified
```

---

## 12. REPLANNING & PLAN v2 GENERATION

`pland` receives `OBSERVATION_THERMAL_CRITICAL` and triggers closed-loop replanning:

```text
Action:                   EXECUTE_WORKLOAD_MIGRATION
PlanId (New):             PlanId_v2 (DistributedId { node_id: 1, local_seq: 2002 })
Version:                  2
Historical PlanId_v1:     IMMUTABLE (Retained in plan history)
PlanStep (New):           StepMigrateAndResume (Target: Node 2)
```

---

## 13. MIGRATION INITIATION

`workloadd` initiates migration transaction:

```text
MigrationId:              MigrationId { transaction_id: 7701, sequence: 1 }
MigrationSessionId:       MigrationSessionId { session_id: 9901, sequence: 1 }
WorkloadId:               DistributedId { node_id: 1, local_seq: 3001 } (CONSTANT)
ExecutionId:              DistributedId { node_id: 1, local_seq: 5001 } (SURVIVES)
MigrationType:            Cold Migration (Base primitive)
State:                    Requested ──► EligibilityCheck
```

---

## 14. MIGRATION TARGET SELECTION & ADMISSION

`workloadd` requests target placement from `schedulerd` / `resourced`:

```text
Target NodeId:            NodeId { node_id: 2, boot_epoch: 105 }
Target DeviceId:          DeviceId { node_id: 2, hardware_signature: 0x99AA }
Target Compatibility:     COMPATIBLE (Same ISA x86_64, 8 CPU cores available)
Target Admission:         ADMITTED (resourced grants Lease 9002 on Node 2)
ResourceId (Target):      DistributedId { node_id: 2, local_seq: 4002 }
State:                    TargetSelected ──► Preparing
```

---

## 15. CHECKPOINT PAYLOAD CREATION

Source `workloadd` captures process state into a versioned `CheckpointRecord`:

```text
CheckpointId:             CheckpointId { payload_hash_hi: 0x1234, payload_hash_lo: 0x5678 }
State Payload Tiers:
  1. Checkpointable:      Memory pages, compute stack, register context (2.1MB)
  2. Reconstructible:     Workspace file offsets, environment variables
  3. Non-Transferable:    Physical PCI register handles EXCLUDED
  4. External:            Workspace storage object references PERSISTED
Integrity:                SHA-256 Digest = 0x8899AABB
Authentication:           HMAC-SHA256 Signature = 0x8899AABB ^ 0xA5A5A5A5A5A5A5A5
State:                    Checkpointing ──► Transferring
```

---

## 16. STATE TRANSFER

`fabricd` transfers payload package from Node 1 to Node 2:

```text
Channel:                  P2P Fabric Stream (Encrypted via Fabric TLS 1.3)
Envelope:                 StateTransferEnvelope { MigrationId: 7701, ReplayToken: 101 }
Replay Floor Check:       ReplayToken 101 > Floor 100 -> PASS
Destination Verification: SHA-256 Digest & HMAC verified by Node 2 workloadd -> PASS
State:                    Transferring ──► Restoring
```

---

## 17. CAPABILITY REBINDING

Target kernel/supervisor re-authorizes capability requirements:

```text
Source Handle (0x0010):   INVALIDATED on Node 2
CapabilityEnvelope:       Envelope { WorkspaceId: WS_DEMO_01, Object: dataset_alpha.dat }
Target Kernel Check:      WS_DEMO_01 Policy authorizes READ right on dataset_alpha.dat -> PASS
Target Handle (Re-issued): 0x0089 (Process-local table index on Node 2)
No Authority Leak:        Capability handles are NOT portable integers
```

---

## 18. RESOURCE REBINDING

```text
Source Lease (9001):      Held in quiesced state until commit
Target Lease (9002):      Active on Node 2 (4 CPU cores, 4GB RAM)
State:                    Restoring ──► Rebinding ──► Validating
```

---

## 19. NETWORK & I/O CONTINUITY

```text
EndpointId:               EndpointId { proxy_id: 5501, stream_id: 1 }
Network Socket:           Fabric Proxy buffers inbound packets (500ms queue window)
Target Socket Rebind:     EndpointId 5501 rebound from 10.0.0.1:8080 to 10.0.0.2:9090
Logical I/O Stream:       LogicalDisplayStream re-negotiates target format
Physical Device Stealing: BANNED (No raw display hardware stolen)
```

---

## 20. ATOMIC HANDOFF ($\text{ActiveExecutions}(WL) \le 1$)

`workloadd` Migration Manager executes the atomic handoff handshake:

```text
Source Node (Node 1)                  Migration Manager                   Target Node (Node 2)
[ PROCESS QUIESCED ] ──────────────────────────────────────────────────► [ PROCESS PAUSED ]
         │                                                                     │
         │                                                                     │ (Self-Check PASS)
         │                                                                     ▼
         │                                                            [ RESTORE_VALIDATED ]
         │                                                                     │
         │                        1. Issue COMMITTING                          │
         │◄────────────────────────────────────────────────────────────────────┘
         │
         │ 2. Invalidate Caps & Kill Process
         ▼
[ PROCESS TERMINATED ]
         │
         │ 3. Send SOURCE_TERMINATED Token
         └────────────────────────────────────────────────────────────────────►│
                                                                               │ 4. Un-pause Process
                                                                               ▼
                                                                      [ PROCESS RUNNING ]
```

$$\text{ActiveExecutions}(\text{WorkloadId}_{3001}) \le 1 \quad \text{at all times}$$

```text
State:                    Validating ──► Committing ──► Completed
Source Lease (9001):      RELEASED to resourced back-pool
```

---

## 21. DESTINATION EXECUTION

```text
WorkloadId:               DistributedId { node_id: 1, local_seq: 3001 } (CONSTANT)
ExecutionId:              DistributedId { node_id: 1, local_seq: 5001 } (SURVIVES)
ProcessId (Destination):  2048 (New pid_t on Node 2)
Task 1 (Compute):         Resumes from snapshot checkpoint offset -> Finishes successfully
Task 2 (Finalize):        Spawns on Node 2 -> Writes analytics_report.json to WS_DEMO_01
```

---

## 22. POST-MIGRATION OBSERVATION

```text
ExecutionEventId:         Event 7002 (EVENT_TASK_COMPLETED, Task 2, Process 2048)
ObservationId:            Obs 8003
Sequence Number:          102 (Monotonic)
Source Node:              NodeId { node_id: 2, boot_epoch: 105 }
Provenance:               Node 2 Execution Event -> Aggregated by observed
```

---

## 23. FINAL COMPLETION

```text
WorkloadState:            Completed (Terminal State)
Output Object:            ObjectId { node_id: 1, local_seq: 8081 } (analytics_report.json)
Workspace Membership:     MEMBER_OF(WS_DEMO_01, analytics_report.json)
Target Lease (9002):      RELEASED to resourced back-pool
MigrationId (7701):       Completed (Immutable transaction record)
PlanId_v2:                Step Completed
```

---

## 24. COMPLETE IDENTITY TRACE TABLE

| Milestone Stage | Active Identity Type | Exact Value / Handle | Authority Owner |
| :--- | :--- | :--- | :--- |
| Human Intent | `IntentId` | `DistributedId { 1, 1001 }` | `intentd` |
| Workspace Isolation | `WorkspaceId` | `DistributedId { 1, 500 }` | `workspaced` |
| Intent Graph Node | `IntentNodeId` | `1`, `2`, `3` | `intentd` |
| Plan Version 1 | `PlanId` / `PlanStepId` | `DistributedId { 1, 2001 }` / Step 1..3 | `pland` |
| Plan Version 2 | `PlanId` (v2) | `DistributedId { 1, 2002 }` | `pland` |
| Workload DAG | `WorkloadId` | `DistributedId { 1, 3001 }` | `workloadd` |
| Cognitive Actor | `AgentId` | `DistributedId { 1, 6001 }` | `agentd` |
| Execution Attempt | `ExecutionId` | `DistributedId { 1, 5001 }` | `workloadd` |
| Source OS Process | `ProcessId` (Source) | `1024` (`pid_t` on Node 1) | Node 1 Kernel |
| Destination OS Process | `ProcessId` (Dest) | `2048` (`pid_t` on Node 2) | Node 2 Kernel |
| Source Resource Lease | `ResourceId` / `LeaseId` | `DistributedId { 1, 4001 }` / Lease 9001 | `resourced` (Node 1) |
| Target Resource Lease | `ResourceId` / `LeaseId` | `DistributedId { 2, 4002 }` / Lease 9002 | `resourced` (Node 2) |
| Source Node | `NodeId` (Source) | `NodeId { 1, 100 }` | Node 1 `fabricd` |
| Target Node | `NodeId` (Target) | `NodeId { 2, 105 }` | Node 2 `fabricd` |
| Target Physical Device | `DeviceId` | `DeviceId { 2, 0x99AA }` | Node 2 Platform |
| Migration Transaction | `MigrationId` | `MigrationId { 7701, 1 }` | `workloadd` |
| State Checkpoint | `CheckpointId` | `CheckpointId { 0x1234, 0x5678 }` | Workspace Storage |
| Continuity Session | `MigrationSessionId` | `MigrationSessionId { 9901, 1 }` | `workloadd` |
| Network Socket Proxy | `EndpointId` | `EndpointId { 5501, 1 }` | Fabric Network Proxy |
| Execution Event | `ExecutionEventId` | Event 7001 / 7002 | `workloadd` Logger |
| Telemetry Observation | `ObservationId` | Obs 8001 / 8002 / 8003 | `observed` |

---

## 25. COMPLETE AUTHORITY TRACE TABLE

| Operation | Requester | Authoritative Component | Persistent Storage Owner | Recovery Authority |
| :--- | :--- | :--- | :--- | :--- |
| Submit Intent | User / Agent | `intentd` | Workspace DB | `intentd` Recovery |
| Create Workspace | Admin / System | `workspaced` | Workspace Control Block | `workspaced` DB |
| Construct Plan | Replanning Engine | `pland` | Plan Version Log | `pland` Recovery |
| Materialize Workload | Orchestration | `workloadd` | Workload Control Block | `workloadd` WAL |
| Admit Resource | Workload / Migration | `resourced` | Fabric Lease Table | `resourced` Engine |
| Execute Task | Workload Engine | Local OS Kernel | Execution Event Log | Kernel Task Table |
| Capture Telemetry | Execution Logger | `observed` | Telemetry Store | `observed` Service |
| Replan Execution | Telemetry Event | `pland` | Plan Version Log | `pland` Engine |
| Initiate Migration | Intent / Telemetry | `workloadd` Mig Manager | Migration Transaction Log | Migration Coordinator |
| Rebind Capability | Migration Manager | Target OS Kernel | Target Process Table | Target Kernel |
| Rebind Resource | Migration Manager | `resourced` | Fabric Lease Table | `resourced` Engine |
| Complete Workload | Execution Task | `workloadd` | Workload Control Block | `workloadd` Master |

---

## 26. PERSISTENCE CHECKPOINTS (A THROUGH J)

```text
Checkpoint A: After Intent Submission          -> Persisted in intentd DB
Checkpoint B: After Plan v1 Persistence          -> Persisted in pland Plan Log
Checkpoint C: After Workload Materialization    -> Persisted in workloadd WCB
Checkpoint D: After Execution Process Spawn      -> Persisted in workloadd Execution Log
Checkpoint E: After Thermal Fault Observation   -> Persisted in observed Telemetry Store
Checkpoint F: After Plan v2 Replanning           -> Persisted in pland Plan Log (Version 2)
Checkpoint G: After Checkpoint Payload Creation -> Written to /workspaces/WS_DEMO_01/checkpoints/
Checkpoint H: After Target Restore & Validation -> Persisted in Target workloadd Log
Checkpoint I: After Atomic Commit Handshake      -> Persisted in Migration Transaction Log
Checkpoint J: After Final Workload Completion   -> Persisted in Workspace Object Registry & WCB
```

---

## 27. RECOVERY MODEL & BOUNDARIES

If a node crash occurs at any checkpoint boundary A–J:
- **Crash before Checkpoint I (Uncommitted)**: Migration transaction aborted (`ROLLED_BACK`). `pland` triggers Restart Migration using durable Checkpoint G payload on an alive node.
- **Crash after Checkpoint I (Committed)**: Migration transaction is complete (`Completed`). Destination process on Node 2 resumes execution.
- In invariant: $\text{ActiveExecutions}(WL_{3001}) \le 1$ is preserved across crash recovery.

---

## 28. CONCURRENCY & LOCK ORDERING HIERARCHY

All components in the vertical slice acquire locks according to the global 6-tier hierarchy:

```text
Level 1: Kernel VFS & Storage Object Table Locks
  └── Level 2: Workspace Domain Lock (workspaced)
        └── Level 3: Fabric Domain & Resource Scheduler Lock (resourced / fabricd)
              └── Level 4: Workload Execution & Migration Transaction Lock (workloadd)
                    └── Level 5: Orchestration Plan Lock (pland / intentd)
                          └── Level 6: Observability Telemetry Lock (observed)
```

---

## 29. SECURITY TRUST GRAPH & NON-ESCALATION

```text
Ring 0 Kernel ──► workspaced / resourced / workloadd ──► intentd / pland / observed ──► Untrusted Workload Process
```
- **Non-Escalation Guarantee**: Schedulers (`resourced`) grant resource capacity slices, but cannot issue capability handles to workspace objects. Migration re-authorizes capability envelopes against target kernel and workspace policy.

---

## 30. INTEGRATION TEST MATRIX (20 TEST CASES)

| ID | Test Scenario | Description | Expected Result |
| :--- | :--- | :--- | :--- |
| **IT-01** | Happy Path Intent $\to$ Completion | Single-node pipeline execution without faults | `WorkloadState::Completed`, output object created |
| **IT-02** | Resource Loss Replanning | Thermal fault during compute task triggers Plan v2 | Observation emitted, `pland` generates Plan v2 |
| **IT-03** | Cold Migration Execution | Migration state machine executes states Requested $\to$ Completed | Target process resumes execution on Node 2 |
| **IT-04** | Recovery at Checkpoint B | Host crash after Plan v1 persistence | Startup recovers Plan v1 cleanly from `pland` log |
| **IT-05** | Recovery at Checkpoint G | Host crash after snapshot creation | Restart Migration resumes from Checkpoint G payload |
| **IT-06** | Recovery at Checkpoint I | Host crash post-commit handshake | Workload continues on Node 2; Node 1 process killed |
| **IT-07** | Duplicate Materialization | Submit same PlanStepId twice | Idempotent materialization (same `WorkloadId`) |
| **IT-08** | Duplicate Migration Request | Issue concurrent migration commands for same WorkloadId | Second request rejected with `MIGRATION_BUSY` |
| **IT-09** | Concurrent Cancellation & Migration | Cancel workspace during `TRANSFERRING` | Migration state machine transitions to `CANCELLED` |
| **IT-10** | Concurrent Replanning & Migration | Update intent while migration is in `REBINDING` | Migration lock serializes replanning until complete |
| **IT-11** | Unauthorized Target Capability | Target node attempts capability escalation | Target kernel rejects re-authorization (`PermissionDenied`) |
| **IT-12** | Cross-Workspace Migration Attempt | Migrate workload into target allocated to another workspace | Re-authorization fails (`PermissionDenied`) |
| **IT-13** | Target Admission Failure | Target node lacks 4GB RAM capacity | Admission rejected (`RESOURCE_ADMISSION_DENIED`); rollback |
| **IT-14** | Lease Expiry Mid-Migration | Target lease expires during `RESTORING` | Migration state machine transitions to `ROLLED_BACK` |
| **IT-15** | Split-Brain Defense | Target attempts un-pause without `SOURCE_TERMINATED` | Target kernel aborts process instantiation |
| **IT-16** | WorkloadId Stability | Verify `WorkloadId` across device migration hops | `WorkloadId` remains identical (`DistributedId { 1, 3001 }`) |
| **IT-17** | ExecutionId Survival | Verify `ExecutionId` across cold migration | `ExecutionId` survives (`DistributedId { 1, 5001 }`) |
| **IT-18** | ProcessId Replacement | Verify host process ID across migration | `ProcessId_Source` (1024) $\neq$ `ProcessId_Dest` (2048) |
| **IT-19** | Stale Session Rejection | Submit migration message with stale replay token | Replay token floor check fails (`StaleEndpoint`) |
| **IT-20** | Resource Cleanup on Completion | Verify lease release post-completion | Lease 9002 released back to `resourced` pool |

---

## 31. CROSS-LAYER INVARIANT MAPPING

```text
CL-01 (Identity Separation)              ──► Demonstrated via Identity Trace Table (20 distinct identities)
CL-02 (Single Authority Per Decision)    ──► Demonstrated via Authority Trace Table
CL-03 (Capability Non-Escalation)        ──► Demonstrated in IT-11 & Target Kernel Envelope Re-authorization
CL-04 (Resource Non-Escalation)          ──► Demonstrated in IT-13 & resourced Admission Gates
CL-05 (Workspace Isolation)              ──► Demonstrated in IT-12 & WorkspaceId Invariant
CL-06 (Single Authoritative Execution)   ──► Demonstrated via AtomicHandoffController (ActiveExecutions <= 1)
CL-07 (Immutable Historical Plans)       ──► Demonstrated via Plan v1 & Plan v2 Versioning
CL-08 (Authoritative Event Ordering)     ──► Demonstrated via workloadd Event Logger & observed Telemetry
CL-09 (Migration Split-Brain Prevention) ──► Demonstrated in IT-15 & Atomic Handshake Protocol
CL-10 (Persistence Authority)            ──► Demonstrated via Persistence Checkpoints A–J
CL-11 (Recovery Determinism)             ──► Demonstrated in IT-04, IT-05, IT-06 & Recovery Sequence
CL-12 (Failure Observability)            ──► Demonstrated via OBSERVATION_THERMAL_CRITICAL Telemetry
CL-13 (Replanning Consistency)           ──► Demonstrated via Closed-Loop pland Replanning
CL-14 (Resource Lease Correctness)       ──► Demonstrated in IT-20 & Lease 9002 Release
CL-15 (Object Identity Stability)        ──► Demonstrated via ObjectId Registry & Workspace Membership
CL-16 (No Kernel Authority Bypass)       ──► Demonstrated via Ring 0 Kernel Capability Checks
CL-17 (No Syscall/ABI Drift)             ──► Demonstrated via 0 Kernel Changes & 0 New Syscalls
CL-18 (No Architectural Cycles)          ──► Demonstrated via Downward Dependency Tree & Lock Hierarchy
```

---

## 32. OBSERVABILITY REQUIREMENTS

The demonstration produces authoritative audit responses for every operational query:

```text
Q1: What happened?           ──► ObservationId Obs 8002 (OBSERVATION_THERMAL_CRITICAL on Node 1)
Q2: Why did it happen?       ──► Thermal overheat (Temp > 85°C) degraded Node 1 capacity
Q3: Which workload caused it? ──► WorkloadId DistributedId { 1, 3001 }
Q4: Which execution ran it?   ──► ExecutionId DistributedId { 1, 5001 }
Q5: Which process ran it?     ──► ProcessId 1024 (Node 1) ──► ProcessId 2048 (Node 2)
Q6: Which resource hosted it? ──► Lease 9001 (Node 1) ──► Lease 9002 (Node 2)
Q7: Which node hosted it?     ──► NodeId { 1, 100 } ──► NodeId { 2, 105 }
Q8: Which migration moved it? ──► MigrationId { 7701, 1 }
Q9: Which checkpoint held it? ──► CheckpointId { 0x1234, 0x5678 }
Q10: Which plan caused it?    ──► PlanId_v1 (Version 1) ──► PlanId_v2 (Version 2)
```

---

## 33. SUCCESS CRITERIA

1. Human intent reaches execution via workspace `WS_DEMO_01`.
2. Resource admission occurs through `resourced` scheduler authority.
3. Execution emits authoritative events logged by `workloadd`.
4. `observed` projects monotonic telemetry observations.
5. Deterministic thermal fault is observed.
6. `pland` closed-loop replanning constructs Plan v2.
7. Workload task is rematerialized idempotently.
8. Migration state machine executes states `Requested` $\to$ `Completed`.
9. Capability envelope is re-authorized by target kernel.
10. Resource lease is admitted on Node 2 by `resourced`.
11. Atomic handoff maintains $\text{ActiveExecutions}(WL_{3001}) \le 1$.
12. Destination execution resumes logical `ExecutionId`.
13. Post-migration observation reflects Node 2 execution.
14. Workload completes with `WorkloadState::Completed`.
15. All 20 identities remain strictly distinct.
16. Workspace isolation remains intact.
17. Zero kernel code changes, 0 new syscalls, 0 ABI modifications.

---

## 34. IMPLEMENTATION BOUNDARY

### Reused Frozen Primitives (Unchanged)
- Kernel Capability System & VFS
- Object & Membership Registry (`workspaced`)
- Workload Control Block & Process Spawner (`workloadd`)
- Resource Scheduler & Lease Engine (`resourced`)
- Telemetry Collector (`observed`)
- Intent & Plan Orchestration (`intentd` / `pland`)
- Migration Engine (`libzero/src/migration.rs`)

### Integration Glue (Ring3 Only)
- Integration test suite harness (`tests/vertical_slice_test.rs`).
- Telemetry event subscriber callback wiring.

### Architecture Changes Required
- **0**. (Zero architecture changes).

---

## 35. RISKS & LIMITATIONS

- **Host Test Execution Limitation**: Native host `cargo test` execution on Windows host environments is blocked by MSVC `link.exe` linker toolchain absence; freestanding target compilation (`cargo check --target x86_64-unknown-none`) is 100% verified.
- **ISA Binary Translation Boundary**: Native machine code migration across heterogeneous CPU ISAs (e.g. x86_64 to ARM64) is categorized as `NON_MIGRATABLE` per REV1 eligibility rules; WASM bytecode runtimes are used for cross-ISA demonstration steps.
