# STAGE 4F IMPLEMENTATION PLAN (REV 2)

## Intent Resolution, Personal Compute Fabric Foundation, & Multi-Node Placement Engine

**Status:** 🟢 FROZEN IMPLEMENTATION PLAN — READY FOR AUTHORIZATION  
**Author:** ZeroOS Architecture Team  
**Date:** 2026-10-06  
**Target Milestone:** Stage 4F (Intent Resolution & Compute Fabric Subsystem)  

---

## 1. Executive Summary & Authoritative Directives

This document details the concrete, machine-verifiable **Implementation Plan** for **Stage 4F (Intent Resolution & Compute Fabric Foundation)**, adhering strictly to the frozen **Stage 4F Architecture Specification Rev3** and **ADR-0030**.

### Substrate Preservation Rules
1. **Stage 3A–3N Production Kernel Nucleus**: **MUST remain 100% byte-identical** (0 bytes modified in kernel production source files).
2. **Stage 4A–4E Production Interfaces**: **MUST remain 100% architecturally compatible**. Existing IPC opcodes (`0x0100`..`0x0508`) and service contracts (`brokerd`, `resourced`, `workloadd`, `workspaced`, `agentd`) are preserved without breaking modifications.
3. **Stage 4 Verification Harness**: `run_stage4f_verification` added to `kernel/src/stage4/tests.rs`, `kernel/src/stage4/mod.rs`, and `kernel/src/lib.rs` as an authorized verification entry point.

### Architectural Rules
- **CSDT Derivation Path**: `fabricd` validates the CSDT container signature and distributed policy, then presents it to the workspace root capability handle (`C_ws`). Stage 3H `sys_cap_derive` executes attenuation. CSDT alone confers **0 kernel authority**.
- **Two-Stage Placement**: Phase 1 Hard Constraints Filter executes first; Phase 2 dimensionless cost function $J$ evaluates only surviving eligible nodes. Dynamic weights default to $w_{\text{lat}}=0.5, w_{\text{eng}}=0.3, w_{\text{cost}}=0.2$ via configurable policy.
- **`intentd` Safety Boundary**: `intentd` possesses **0 system authority**. It cannot issue capability handles, allocate resource leases, or spawn processes directly.
- **Class 3 Deduplication**: ZeroOS provider guarantees duplicate suppression for identical `OperationId = BLAKE2s(IntentId, TaskId, Sequence)` via a 2-phase `Prepared` → `Committed` ZeroFS journal latch. External idempotency requires the external system to accept `OperationId` as a transaction key.

---

## 2. Interface Audit & Component Topology

```text
                               +-----------------------------+
                               |     user / intentd.srv      |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |           intentd           |
                               |  (Intent Resolution Engine) |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |           agentd            |
                               |  (Agent Execution Context)  |
                               +--------------+--------------+
                                              |
                        +---------------------+---------------------+
                        |                                           |
           +------------v------------+                 +------------v------------+
           |        workloadd        |                 |         fabricd         |
           |  (Workload Task DAGs)   | ◄── Placement ──| (2-Stage Placement Plan)|
           +------------+------------+     Proposal    +------------+------------+
                        |                                           |
                        +---------------------+---------------------+
                                              |
                        +---------------------+---------------------+
                        |                                           |
           +------------v------------+                 +------------v------------+
           |    LOCAL NODE EXECUTION |                 |    REMOTE FABRIC NODE   |
           |   (resourced Leases)    |                 |  (CSDT Transport Mesh)  |
           +-------------------------+                 +-------------------------+
```

### Component Breakdown

| Component | Package Path | Subsystem Role | Memory Footprint |
|---|---|---|---|
| **`libzero/src/fabric.rs`** | `libzero/` | IPC OpCodes, Data Structures, Placement Types, CSDT Containers | Shared Library |
| **`intentd/`** | `intentd/` | Freestanding Intent Resolution Daemon | ~36 KiB BSS |
| **`fabricd/`** | `fabricd/` | Freestanding P2P Mesh & Placement Planner Daemon | ~72 KiB BSS |
| **`kernel/src/stage4/tests.rs`** | `kernel/` | Stage 4F Verification Entry Point (`run_stage4f_verification`) | Test Harness |

---

## 3. Concrete Protocol OpCodes & IPC Data Structures

### 3.1 IPC OpCodes (`libzero/src/fabric.rs`)

```rust
// intentd Protocol OpCodes (Service Name: "intentd.srv")
pub const OP_INTENT_SUBMIT:              u64 = 0x0601;
pub const OP_INTENT_SUBMIT_RESP:         u64 = 0x0681;
pub const OP_INTENT_RESOLVE:             u64 = 0x0602;
pub const OP_INTENT_RESOLVE_RESP:        u64 = 0x0682;
pub const OP_INTENT_QUERY_STATE:         u64 = 0x0603;
pub const OP_INTENT_QUERY_STATE_RESP:    u64 = 0x0683;
pub const OP_INTENT_CANCEL:              u64 = 0x0604;
pub const OP_INTENT_CANCEL_RESP:         u64 = 0x0684;

// fabricd Protocol OpCodes (Service Name: "fabricd.srv")
pub const OP_FABRIC_NODE_REGISTER:       u64 = 0x0610;
pub const OP_FABRIC_NODE_REGISTER_RESP:  u64 = 0x0690;
pub const OP_FABRIC_NODE_HEARTBEAT:      u64 = 0x0611;
pub const OP_FABRIC_NODE_HEARTBEAT_RESP: u64 = 0x0691;
pub const OP_FABRIC_CSDT_DELEGATE:       u64 = 0x0612;
pub const OP_FABRIC_CSDT_DELEGATE_RESP:  u64 = 0x0692;
pub const OP_FABRIC_CSDT_REVOKE:         u64 = 0x0613;
pub const OP_FABRIC_CSDT_REVOKE_RESP:    u64 = 0x0693;
pub const OP_FABRIC_QUERY_TOPOLOGY:      u64 = 0x0614;
pub const OP_FABRIC_QUERY_TOPOLOGY_RESP: u64 = 0x0694;
```

### 3.2 Fixed Data Structure Layouts

```rust
use crate::resource::DistributedId;

/// 1,024-byte Intent Descriptor Header & Buffer
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct IntentDescriptor {
    pub intent_id: DistributedId,        // 16 B
    pub principal_id: DistributedId,     // 16 B
    pub workspace_id: DistributedId,     // 16 B
    pub state: u8,                       // 1 B (IntentState)
    pub ambiguity_flag: u8,              // 1 B
    pub min_ial_requirement: u8,        // 1 B (IAL-1..3)
    pub privacy_class: u8,               // 1 B (0=LocalOnly, 1=FabricPrivate, 2=Public)
    pub _pad0: [u8; 4],                  // 4 B
    pub submission_tsc: u64,             // 8 B
    pub deadline_tsc: u64,               // 8 B
    pub energy_limit_mwh: u32,           // 4 B
    pub raw_intent_len: u32,             // 4 B
    pub raw_intent_payload: [u8; 512],   // 512 B UTF-8 text buffer
    pub _padding: [u8; 432],             // 432 B padding to 1,024 B
}
const _: () = assert!(core::mem::size_of::<IntentDescriptor>() == 1024);

/// 128-byte Capability-Scoped Delegation Token Container
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CsdtToken {
    pub csdt_id: DistributedId,          // 16 B: Monotonic CSDT Identifier
    pub issuer_node_id: u64,             // 8 B: Source NodeId
    pub target_node_id: u64,             // 8 B: Destination NodeId
    pub workspace_id: DistributedId,     // 16 B: Target Workspace Identity
    pub capability_rights_mask: u64,     // 8 B: Attenuated Rights Bitmask
    pub valid_from_monotonic_tick: u64,  // 8 B: Start tick on receiver time authority
    pub expire_monotonic_tick: u64,      // 8 B: Expiration tick on receiver time authority
    pub signature: [u8; 48],             // 48 B: Ed25519 signature by issuer K_node
}
const _: () = assert!(core::mem::size_of::<CsdtToken>() == 128);

/// 256-byte Compute Fabric Node Descriptor
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct FabricNodeDescriptor {
    pub node_id: u64,                    // 8 B
    pub ial_level: u8,                   // 1 B (1=Software, 2=Hardware, 3=Attested)
    pub state: u8,                       // 1 B (0=Offline, 1=Online, 2=Degraded)
    pub cpu_cores: u16,                  // 2 B
    pub _pad0: [u8; 4],                  // 4 B
    pub total_ram_mb: u64,               // 8 B
    pub available_ram_mb: u64,           // 8 B
    pub gpu_compute_mflops: u64,         // 8 B
    pub npu_tops: u32,                   // 4 B
    pub rtt_latency_us: u32,             // 4 B
    pub battery_level_pct: u8,           // 1 B
    pub power_source: u8,                // 1 B (0=Battery, 1=AC)
    pub _pad1: [u8; 6],                  // 6 B
    pub last_heartbeat_tsc: u64,         // 8 B
    pub node_pubkey: [u8; 32],           // 32 B Ed25519 public key
    pub _padding: [u8; 160],             // 160 B padding to 256 B
}
const _: () = assert!(core::mem::size_of::<FabricNodeDescriptor>() == 256);
```

---

## 4. CSDT Capability Path & Placement Engine Detail

### 4.1 CSDT Derivation Sequence (Zero Capability Authority in `fabricd`)

```text
Node B (Issuer)
  │
  ├── Signed CsdtToken container sent over Noise network mesh
  │
Node A (fabricd)
  │
  ├── 1. Validates Ed25519 signature & distributed policy
  ├── 2. Validates monotonic expiration tick against Stage 4B Qualified Time Authority
  ├── 3. Presents CsdtToken to Target Workspace Owner process
  │
Node A (Workspace Process / Stage 3H Kernel)
  │
  ├── 4. Workspace process verifies requested rights <= C_ws rights
  ├── 5. Executes sys_cap_derive(C_ws, csdt.rights_mask, &mut C_local) via Stage 3H
  └── 6. Attenuated Stage 3H capability (C_local) created in workspace handle table
```

### 4.2 Placement Planner Filter & Cost Evaluation Engine

```rust
pub struct FabricPlannerPolicy {
    pub w_lat: f32, // Default: 0.5
    pub w_eng: f32, // Default: 0.3
    pub w_cost: f32, // Default: 0.2
}

pub fn evaluate_placement(task_req: &TaskDemandSpec, policy: &FabricPlannerPolicy) -> Result<u64, ZeroError> {
    let mut best_node_id: u64 = 0;
    let mut best_cost: f32 = f32::MAX;

    unsafe {
        for node in FABRIC_NODE_TABLE.iter() {
            if node.state != 1 { continue; } // Must be Online

            // PHASE 1: HARD BOOLEAN CONSTRAINTS FILTER (EXPLICIT PRECEDENCE)
            if task_req.min_ial > node.ial_level { continue; }
            if task_req.required_ram_mb > node.available_ram_mb { continue; }
            if task_req.requires_npu && node.npu_tops == 0 { continue; }
            if task_req.requires_gpu && node.gpu_compute_mflops == 0 { continue; }

            // PHASE 2: DIMENSIONLESS SOFT OPTIMIZATION (ONLY FOR SURVIVING ELIGIBLE NODES)
            let n_lat = (node.rtt_latency_us as f32) / 100_000.0;
            let n_eng = if node.power_source == 0 { (100 - node.battery_level_pct) as f32 / 100.0 } else { 0.0 };
            let n_cost = 0.1;

            let cost = policy.w_lat * n_lat + policy.w_eng * n_eng + policy.w_cost * n_cost;
            if cost < best_cost {
                best_cost = cost;
                best_node_id = node.node_id;
            }
        }
    }

    if best_node_id != 0 {
        Ok(best_node_id)
    } else {
        Err(ZeroError::ResourceExhausted)
    }
}
```

---

## 5. QEMU Machine Acceptance Matrix (26 Machine Tests: 4F-A to 4F-Z)

| Test ID | Test Name & Boundary Scope | Validation Requirement |
|---|---|---|
| **4F-A** | `intentd Startup` | Verify `intentd` daemon startup and service registration in `brokerd`. |
| **4F-B** | `fabricd Startup` | Verify `fabricd` daemon startup and service registration in `brokerd`. |
| **4F-C** | `Intent Submission` | Parse `OP_INTENT_SUBMIT` into `IntentDescriptor`. |
| **4F-D** | `Ambiguity Execution Blocking` | Assert ambiguous intent in `Clarifying` state **cannot** execute workloads. |
| **4F-E** | `Intent Safety Boundary` | Assert `intentd` **cannot** directly issue capabilities or allocate leases. |
| **4F-F** | `Model Non-Authority Enforced` | Reject model attempts to inject capability handles into ExecutionPlans. |
| **4F-G** | `Plan Structural Validation` | Deterministic validator rejects cyclical or invalid Plan DAGs. |
| **4F-H** | `Ephemeral Agent Lifecycle` | Verify one-shot lifecycle: `Creating → Active → Stopping → Reclaimed`. |
| **4F-I** | `Workload DAG Handoff` | Handoff validated plan to `agentd` and `workloadd`. |
| **4F-J** | `Fabric Peer Discovery` | Authenticate P2P peer node discovery beacons. |
| **4F-K** | `IAL Peer Classification` | Verify IAL-1, IAL-2, and IAL-3 node registration. |
| **4F-L** | `Hard Constraints Filter` | Filter out candidate nodes failing RAM/NPU/IAL hard bounds. |
| **4F-M** | `Hard Filter Precedence` | Assert soft objective weights **cannot** override Phase 1 hard constraint filter. |
| **4F-N** | `Soft Cost Optimization` | Evaluate soft cost $J$ across surviving eligible nodes. |
| **4F-O** | `Advisory Remote Telemetry` | Assert remote telemetry observations **cannot** allocate capacity directly. |
| **4F-P** | `CSDT Token Generation` | Issue signed `CsdtToken` container. |
| **4F-Q** | `CSDT Expiration Monotonic` | Expire CSDT via Stage 4B Qualified Monotonic Time Authority. |
| **4F-R** | `CSDT Kernel Cap Derivation` | Target workspace derives Stage 3H capability via `sys_cap_derive` from `C_ws`. |
| **4F-S** | `CSDT Subtree Revocation` | Assert `sys_cap_revoke` invalidates derived Stage 3H capability subtree upon CSDT expiry. |
| **4F-T** | `Provider Authoritative Lease` | Provider `resourced` owns lease authority; consumer partition triggers provider quarantine & reclamation. |
| **4F-U** | `Class 3 Side-Effect Latch` | Latch 2-phase `Prepared → Committed` `OperationId` on ZeroFS. |
| **4F-V** | `Duplicate Operation Rejection` | Reject duplicate `OperationId` execution within provider boundary. |
| **4F-W** | `Ephemeral Agent Crash Recovery`| Reclaim ACB slot without resurrecting Ephemeral Agent. |
| **4F-X** | `Protocol Robustness` | Reject malformed IPC opcodes and response tags. |
| **4F-Y** | `PMM Neutrality` | Assert baseline == final PMM free frame count. |
| **4F-Z** | `Substrate Preservation` | Assert 0 bytes modified in Stage 3A–3N kernel production code. |

---

## 6. Implementation Milestones

1. **Step 1: Protocol & Types (`libzero/src/fabric.rs`)**: IPC OpCodes `0x0601`..`0x0614` and static structures.
2. **Step 2: `intentd` Service Package (`intentd/`)**: Freestanding daemon, intent parsing, deterministic plan validator.
3. **Step 3: `fabricd` Service Package (`fabricd/`)**: Freestanding daemon, P2P discovery mesh, CSDT transport, 2-stage placement engine.
4. **Step 4: Kernel Verification Harness (`kernel/src/stage4/tests.rs`)**: Implementation of `run_stage4f_verification` covering tests `4F-A` to `4F-Z`.
5. **Step 5: Python Test Suite (`tests/test_stage4f.py`)**: QEMU runner executing all 26 machine verification scenarios with ISA exit code 33.
6. **Step 6: System Regression**: Execute `python -m unittest discover tests` ensuring 100% pass across all 184 system tests.

---

## 7. Status Banner

```text
STATUS: FROZEN — APPROVED FOR IMPLEMENTATION
STAGE 3 PRODUCTION KERNEL MODIFICATIONS: NONE
STAGE 4A–4E PRODUCTION INTERFACE COMPATIBILITY: PRESERVED
```
