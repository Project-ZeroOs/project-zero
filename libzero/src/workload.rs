//! ZeroOS - libzero Workload Orchestration Data Structures & Protocol
//!
//! Authoritative Contract: Stage 4C Architecture Rev4 & Phase 4C Implementation Plan Rev2.

use crate::error::ZeroError;
use crate::resource::{DimensionCapacityVector, DistributedId, LocalityDomain, ResourceType};

// Protocol OpCodes for workloadd (registered with brokerd as "workload.service")
pub const OP_WORKLOAD_CREATE:         u64 = 0x4C01;
pub const OP_WORKLOAD_CREATE_RESP:    u64 = 0x4C02;
pub const OP_WORKLOAD_SUBMIT_DAG:     u64 = 0x4C03;
pub const OP_WORKLOAD_SUBMIT_DAG_RESP:u64 = 0x4C04;
pub const OP_WORKLOAD_QUERY:          u64 = 0x4C05;
pub const OP_WORKLOAD_QUERY_RESP:     u64 = 0x4C06;
pub const OP_WORKLOAD_CANCEL:         u64 = 0x4C07;
pub const OP_WORKLOAD_CANCEL_RESP:    u64 = 0x4C08;
pub const OP_WORKLOAD_EXTEND_DAG:     u64 = 0x4C09;
pub const OP_WORKLOAD_EXTEND_DAG_RESP:u64 = 0x4C0A;

// Supervisor Process Spawner OpCodes (registered with brokerd as "supervisor.service")
pub const OP_PROCESS_SPAWN:           u64 = 0x4A05;
pub const OP_PROCESS_SPAWN_RESP:      u64 = 0x4A06;
pub const OP_PROCESS_TERMINATE:       u64 = 0x4A07;
pub const OP_PROCESS_TERMINATE_RESP:  u64 = 0x4A08;
pub const OP_PROCESS_EXIT_NOTIFY:     u64 = 0x4A09;

pub const MAX_TASKS_PER_WORKLOAD: usize = 16;
pub const MAX_CONCURRENT_WORKLOADS: usize = 8;
pub const MAX_CONCURRENT_RUNNING_TASKS: usize = 8;
pub const DEFAULT_MAX_TASK_RETRIES: u8 = 3;
pub const MAX_LEASES_PER_WORKLOAD: usize = 16;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WorkloadState {
    Creating = 0,
    Ready = 1,
    Running = 2,
    Waiting = 3,
    Suspended = 4,
    Recovering = 5,
    Cancelling = 6,
    Completed = 7,
    Failed = 8,
    Cancelled = 9,
    Reclaimed = 10,
}

impl Default for WorkloadState {
    fn default() -> Self {
        Self::Creating
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TaskState {
    Unallocated = 0,
    Blocked = 1,
    Ready = 2,
    LeaseAcquired = 3,
    Running = 4,
    Completed = 5,
    Failed = 6,
    Cancelled = 7,
    Stopped = 8,
}

impl Default for TaskState {
    fn default() -> Self {
        Self::Unallocated
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RecoveryClass {
    Class1Pure = 0,
    Class2Stateful = 1,
    Class3Irreversible = 2,
}

impl Default for RecoveryClass {
    fn default() -> Self {
        Self::Class1Pure
    }
}

/// 32-byte task descriptor.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskDescriptor {
    pub task_id: u16,
    pub state: TaskState,
    pub retry_count: u8,
    pub max_retries: u8,
    pub _pad0: u8,
    pub dependencies: u16,
    pub assigned_pid: u64,
    pub lease_id: DistributedId,
}

const _: () = assert!(core::mem::size_of::<TaskDescriptor>() == 32);

/// 96-byte task resource demand descriptor.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskResourceDemand {
    pub resource_type: ResourceType,
    pub locality_domain: LocalityDomain,
    pub _pad0: [u8; 6],
    pub required_capacity: DimensionCapacityVector,
    pub min_duration_ticks: u64,
    pub required_cap_rights: u16,
    pub _padding: [u8; 6],
}

const _: () = assert!(core::mem::size_of::<TaskResourceDemand>() == 96);

/// 64-byte process spawn request structure (OP_PROCESS_SPAWN).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessSpawnRequest {
    pub workload_id: DistributedId,
    pub task_id: u16,
    pub _pad0: [u8; 2],
    pub task_cap_handle: u32,
    pub binary_name: [u8; 32],
    pub _padding: [u8; 8],
}

const _: () = assert!(core::mem::size_of::<ProcessSpawnRequest>() == 64);

/// 32-byte process spawn response structure (OP_PROCESS_SPAWN_RESP).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessSpawnResponse {
    pub status: i32,
    pub control_channel_handle: u32,
    pub process_id: u64,
    pub _padding: [u8; 16],
}

const _: () = assert!(core::mem::size_of::<ProcessSpawnResponse>() == 32);

/// 576-byte workload control block.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct WorkloadControlBlock {
    pub workload_id: DistributedId,
    pub owner_pid: u64,
    pub generation: u32,
    pub client_channel_handle: u32,
    pub state: WorkloadState,
    pub recovery_class: RecoveryClass,
    pub task_count: u8,
    pub active_lease_count: u8,
    pub _pad0: [u8; 4],
    pub tasks: [TaskDescriptor; MAX_TASKS_PER_WORKLOAD],
    pub _padding: [u8; 24],
}

impl Default for WorkloadControlBlock {
    fn default() -> Self {
        Self {
            workload_id: DistributedId::default(),
            owner_pid: 0,
            generation: 1,
            client_channel_handle: 0,
            state: WorkloadState::Creating,
            recovery_class: RecoveryClass::Class1Pure,
            task_count: 0,
            active_lease_count: 0,
            _pad0: [0; 4],
            tasks: [TaskDescriptor::default(); MAX_TASKS_PER_WORKLOAD],
            _padding: [0; 24],
        }
    }
}

const _: () = assert!(core::mem::size_of::<WorkloadControlBlock>() == 576);

/// Task DAG representation and topological sort engine.
#[derive(Copy, Clone, Debug)]
pub struct TaskDag {
    pub in_degrees: [u8; MAX_TASKS_PER_WORKLOAD],
    pub dependency_matrix: [u16; MAX_TASKS_PER_WORKLOAD],
    pub task_count: usize,
}

impl Default for TaskDag {
    fn default() -> Self {
        Self {
            in_degrees: [0; MAX_TASKS_PER_WORKLOAD],
            dependency_matrix: [0; MAX_TASKS_PER_WORKLOAD],
            task_count: 0,
        }
    }
}

impl TaskDag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_tasks(tasks: &[TaskDescriptor], count: usize) -> Self {
        let mut dag = Self::new();
        dag.task_count = count;

        for i in 0..count {
            let deps = tasks[i].dependencies;
            dag.dependency_matrix[i] = deps;
            let mut degree = 0u8;
            for j in 0..count {
                if (deps & (1 << j)) != 0 {
                    degree += 1;
                }
            }
            dag.in_degrees[i] = degree;
        }
        dag
    }

    /// Verifies that the task DAG is non-empty, within size limits, and strictly acyclic.
    pub fn verify_acyclic(&self) -> Result<(), ZeroError> {
        if self.task_count == 0 || self.task_count > MAX_TASKS_PER_WORKLOAD {
            return Err(ZeroError::InvalidRequest);
        }

        let mut in_degrees = self.in_degrees;
        let mut queue = [0u8; MAX_TASKS_PER_WORKLOAD];
        let mut head = 0usize;
        let mut tail = 0usize;
        let mut processed = 0usize;

        // Enqueue all tasks with in-degree 0
        for i in 0..self.task_count {
            if in_degrees[i] == 0 {
                queue[tail] = i as u8;
                tail += 1;
            }
        }

        // Topological sort loop
        while head < tail {
            let u = queue[head] as usize;
            head += 1;
            processed += 1;

            for v in 0..self.task_count {
                if (self.dependency_matrix[v] & (1 << u)) != 0 {
                    if in_degrees[v] > 0 {
                        in_degrees[v] -= 1;
                        if in_degrees[v] == 0 {
                            queue[tail] = v as u8;
                            tail += 1;
                        }
                    }
                }
            }
        }

        if processed == self.task_count {
            Ok(())
        } else {
            Err(ZeroError::CyclicDependency)
        }
    }
}
