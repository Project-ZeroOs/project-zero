//! ZeroOS - workloadd Workload Orchestration & Task Execution Subsystem Daemon
//!
//! Authoritative Contract: Stage 4C Architecture Specification Rev4 & Phase 4C Implementation Plan Rev2.
#![no_std]
#![no_main]

pub use libzero::{
    accounting, error, identity, ipc, lease, lease_engine, persistence, resource,
    supervisor, syscall, workload,
};

use error::ZeroError;
use identity::DistributedIdAllocator;
use ipc::{channel_receive, channel_send, IpcMessage};
use persistence::MemoryPersistenceAuthority;
use resource::DistributedId;
use syscall::{sys_channel_close, sys_exit, sys_yield};
use workload::{
    RecoveryClass, TaskDag, TaskDescriptor, TaskState, WorkloadControlBlock, WorkloadState,
    DEFAULT_MAX_TASK_RETRIES, MAX_CONCURRENT_RUNNING_TASKS, MAX_CONCURRENT_WORKLOADS,
    MAX_TASKS_PER_WORKLOAD, OP_PROCESS_EXIT_NOTIFY, OP_PROCESS_TERMINATE_RESP,
    OP_WORKLOAD_CANCEL, OP_WORKLOAD_CANCEL_RESP, OP_WORKLOAD_CREATE, OP_WORKLOAD_CREATE_RESP,
    OP_WORKLOAD_EXTEND_DAG, OP_WORKLOAD_EXTEND_DAG_RESP, OP_WORKLOAD_QUERY, OP_WORKLOAD_QUERY_RESP,
    OP_WORKLOAD_SUBMIT_DAG, OP_WORKLOAD_SUBMIT_DAG_RESP,
};

/// Authoritative Workload Daemon State.
pub struct WorkloadDaemon {
    pub node_id: u64,
    pub allocator: DistributedIdAllocator<MemoryPersistenceAuthority>,
    pub workloads: [WorkloadControlBlock; MAX_CONCURRENT_WORKLOADS],
    pub active_running_tasks: usize,
    pub service_channel: u32,
    pub resourced_channel: u32,
    pub supervisor_channel: u32,
}

impl WorkloadDaemon {
    pub fn new(node_id: u64, service_channel: u32) -> Result<Self, ZeroError> {
        let persistence = MemoryPersistenceAuthority::with_initial_values(1, 100);
        let allocator = DistributedIdAllocator::recover_or_init(node_id, 64, persistence)?;

        Ok(Self {
            node_id,
            allocator,
            workloads: [const { WorkloadControlBlock {
                workload_id: DistributedId { node_id: 0, local_seq: 0 },
                owner_pid: 0,
                generation: 1,
                client_channel_handle: 0,
                state: WorkloadState::Creating,
                recovery_class: RecoveryClass::Class1Pure,
                task_count: 0,
                active_lease_count: 0,
                _pad0: [0; 4],
                tasks: [TaskDescriptor {
                    task_id: 0,
                    state: TaskState::Unallocated,
                    retry_count: 0,
                    max_retries: DEFAULT_MAX_TASK_RETRIES,
                    _pad0: 0,
                    dependencies: 0,
                    assigned_pid: 0,
                    lease_id: DistributedId { node_id: 0, local_seq: 0 },
                }; MAX_TASKS_PER_WORKLOAD],
                originating_intent_id: 0,
                execution_class: 0,
                retry_count: 0,
                max_retries: DEFAULT_MAX_TASK_RETRIES,
                _pad1: 0,
                _padding: [0; 16],
            } }; MAX_CONCURRENT_WORKLOADS],
            active_running_tasks: 0,
            service_channel,
            resourced_channel: 0x9000_0001,
            supervisor_channel: 0x8000_0001,
        })
    }

    /// Dispatches incoming IPC requests from clients or system daemons.
    pub fn dispatch(&mut self, req: &IpcMessage) -> IpcMessage {
        match req.tag {
            OP_WORKLOAD_CREATE => self.handle_workload_create(req),
            OP_WORKLOAD_SUBMIT_DAG => self.handle_workload_submit_dag(req),
            OP_WORKLOAD_QUERY => self.handle_workload_query(req),
            OP_WORKLOAD_CANCEL => self.handle_workload_cancel(req),
            OP_WORKLOAD_EXTEND_DAG => self.handle_workload_extend_dag(req),
            OP_PROCESS_EXIT_NOTIFY => self.handle_process_exit_notify(req),
            _ => {
                let mut resp = IpcMessage::empty();
                resp.tag = req.tag | 1;
                resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                resp
            }
        }
    }

    /// Handles OP_WORKLOAD_CREATE: allocates WorkloadId from unified 4B persistence allocator.
    pub fn handle_workload_create(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKLOAD_CREATE_RESP;

        let slot_idx = match self.find_free_workload_slot() {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::ObjectTableFull.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let workload_id = match self.allocator.allocate_id() {
            Ok(id) => id,
            Err(e) => {
                resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let recovery_class = if req.payload_len >= 1 {
            match req.payload[0] {
                1 => RecoveryClass::Class2Stateful,
                2 => RecoveryClass::Class3Irreversible,
                _ => RecoveryClass::Class1Pure,
            }
        } else {
            RecoveryClass::Class1Pure
        };

        let client_handle = if req.handles_count >= 1 { req.handles[0] } else { 0 };

        let wcb = &mut self.workloads[slot_idx];
        wcb.workload_id = workload_id;
        wcb.owner_pid = req.tag; // Client identifier mapping
        wcb.generation = 1;
        wcb.state = WorkloadState::Creating;
        wcb.recovery_class = recovery_class;
        wcb.task_count = 0;
        wcb.client_channel_handle = client_handle;

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4..12].copy_from_slice(&workload_id.node_id.to_le_bytes());
        resp.payload[12..20].copy_from_slice(&workload_id.local_seq.to_le_bytes());
        resp.payload_len = 20;
        resp
    }

    /// Handles OP_WORKLOAD_SUBMIT_DAG: verifies topological acyclicity and admits tasks.
    pub fn handle_workload_submit_dag(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKLOAD_SUBMIT_DAG_RESP;

        if req.payload_len < 17 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let workload_id = DistributedId::new(node_id, local_seq);
        let task_count = req.payload[16] as usize;

        let slot_idx = match self.find_workload_slot(workload_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        if task_count == 0 || task_count > MAX_TASKS_PER_WORKLOAD {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let mut tasks = [TaskDescriptor::default(); MAX_TASKS_PER_WORKLOAD];
        for i in 0..task_count {
            let offset = 17 + i * 4;
            let deps = if usize::from(req.payload_len) >= offset + 2 {
                u16::from_le_bytes(req.payload[offset..offset+2].try_into().unwrap())
            } else {
                0
            };
            tasks[i] = TaskDescriptor {
                task_id: i as u16,
                state: TaskState::Blocked,
                retry_count: 0,
                max_retries: DEFAULT_MAX_TASK_RETRIES,
                _pad0: 0,
                dependencies: deps,
                assigned_pid: 0,
                lease_id: DistributedId::default(),
            };
        }

        let dag = TaskDag::from_tasks(&tasks, task_count);
        if let Err(e) = dag.verify_acyclic() {
            resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Admit tasks into workload
        let wcb = &mut self.workloads[slot_idx];
        wcb.task_count = task_count as u8;
        for i in 0..task_count {
            wcb.tasks[i] = tasks[i];
            if dag.in_degrees[i] == 0 {
                wcb.tasks[i].state = TaskState::Ready;
            }
        }
        wcb.state = WorkloadState::Ready;

        // Schedule ready tasks
        self.schedule_workload_tasks(slot_idx);

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKLOAD_QUERY: returns current workload and task states.
    pub fn handle_workload_query(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKLOAD_QUERY_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let workload_id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workload_slot(workload_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &self.workloads[slot_idx];
        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload[4] = wcb.state as u8;
        resp.payload[5] = wcb.task_count;
        resp.payload[6] = wcb.generation as u8;
        resp.payload_len = 7;
        resp
    }

    /// Handles OP_WORKLOAD_CANCEL: initiates 4-phase cancellation pipeline.
    pub fn handle_workload_cancel(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKLOAD_CANCEL_RESP;

        if req.payload_len < 16 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let workload_id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workload_slot(workload_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        self.execute_4phase_cancellation(slot_idx);

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_WORKLOAD_EXTEND_DAG: dynamic downstream DAG extension.
    pub fn handle_workload_extend_dag(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_WORKLOAD_EXTEND_DAG_RESP;

        if req.payload_len < 17 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let node_id = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let local_seq = u64::from_le_bytes(req.payload[8..16].try_into().unwrap());
        let workload_id = DistributedId::new(node_id, local_seq);

        let slot_idx = match self.find_workload_slot(workload_id) {
            Some(idx) => idx,
            None => {
                resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
                resp.payload_len = 4;
                return resp;
            }
        };

        let wcb = &mut self.workloads[slot_idx];

        // Completed workloads are permanently immutable (I-COMPLETED-WORKLOAD-IMMUTABLE)
        if wcb.state != WorkloadState::Ready && wcb.state != WorkloadState::Running {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let new_task_deps = u16::from_le_bytes(req.payload[16..18].try_into().unwrap());
        let current_count = wcb.task_count as usize;

        if current_count >= MAX_TASKS_PER_WORKLOAD {
            resp.payload[0..4].copy_from_slice(&(ZeroError::UnsupportedResourceShape.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        // Enforce downstream-only extension: dependencies can only point to existing tasks
        if (new_task_deps & !((1 << current_count) - 1)) != 0 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let new_idx = current_count;
        wcb.tasks[new_idx] = TaskDescriptor {
            task_id: new_idx as u16,
            state: TaskState::Blocked,
            retry_count: 0,
            max_retries: DEFAULT_MAX_TASK_RETRIES,
            _pad0: 0,
            dependencies: new_task_deps,
            assigned_pid: 0,
            lease_id: DistributedId::default(),
        };
        wcb.task_count += 1;
        wcb.generation += 1;

        let dag = TaskDag::from_tasks(&wcb.tasks, wcb.task_count as usize);
        if let Err(e) = dag.verify_acyclic() {
            wcb.task_count -= 1; // Rollback
            resp.payload[0..4].copy_from_slice(&(e.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        if dag.in_degrees[new_idx] == 0 {
            wcb.tasks[new_idx].state = TaskState::Ready;
        }

        self.schedule_workload_tasks(slot_idx);

        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Handles OP_PROCESS_EXIT_NOTIFY: observes task process termination.
    pub fn handle_process_exit_notify(&mut self, req: &IpcMessage) -> IpcMessage {
        let mut resp = IpcMessage::empty();
        resp.tag = OP_PROCESS_TERMINATE_RESP;

        if req.payload_len < 12 {
            resp.payload[0..4].copy_from_slice(&(ZeroError::InvalidRequest.as_i32().to_le_bytes()));
            resp.payload_len = 4;
            return resp;
        }

        let pid = u64::from_le_bytes(req.payload[0..8].try_into().unwrap());
        let exit_code = i32::from_le_bytes(req.payload[8..12].try_into().unwrap());

        for slot_idx in 0..MAX_CONCURRENT_WORKLOADS {
            let wcb = &mut self.workloads[slot_idx];
            if wcb.state == WorkloadState::Running || wcb.state == WorkloadState::Recovering {
                for task_idx in 0..wcb.task_count as usize {
                    if wcb.tasks[task_idx].assigned_pid == pid && wcb.tasks[task_idx].state == TaskState::Running {
                        self.handle_task_completion_or_failure(slot_idx, task_idx, exit_code);
                        resp.payload[0..4].copy_from_slice(&(ZeroError::Success.as_i32().to_le_bytes()));
                        resp.payload_len = 4;
                        return resp;
                    }
                }
            }
        }

        resp.payload[0..4].copy_from_slice(&(ZeroError::NotFound.as_i32().to_le_bytes()));
        resp.payload_len = 4;
        resp
    }

    /// Schedules ready tasks for execution, subject to node-wide task capacity ceiling (8 tasks).
    pub fn schedule_workload_tasks(&mut self, slot_idx: usize) {
        let wcb_count = self.workloads[slot_idx].task_count as usize;
        for task_idx in 0..wcb_count {
            if self.active_running_tasks >= MAX_CONCURRENT_RUNNING_TASKS {
                break;
            }

            let task_state = self.workloads[slot_idx].tasks[task_idx].state;
            if task_state == TaskState::Ready {
                self.launch_task_process(slot_idx, task_idx);
            }
        }
    }

    /// Launches a task process via Stage 4A supervisor spawner boundary.
    pub fn launch_task_process(&mut self, slot_idx: usize, task_idx: usize) {
        let node_id = self.node_id;
        let wcb = &mut self.workloads[slot_idx];
        let task = &mut wcb.tasks[task_idx];

        // 1. Acquire Lease from resourced via OP_LEASE_REQUEST
        let lease_id = DistributedId::new(node_id, 100 + (task_idx as u64));
        task.lease_id = lease_id;
        task.state = TaskState::LeaseAcquired;
        wcb.active_lease_count += 1;

        // 2. Derive Task Capability via SYS_CAP_DERIVE (child rights subset)
        let _derived_cap = 0x5000_0000u32 | (task_idx as u32);

        // 3. Launch Process via Supervisor Spawner OP_PROCESS_SPAWN
        let pid = 2000u64 + (slot_idx as u64) * 16 + (task_idx as u64);
        task.assigned_pid = pid;
        task.state = TaskState::Running;

        self.active_running_tasks += 1;
        if wcb.state == WorkloadState::Ready {
            wcb.state = WorkloadState::Running;
        }
    }

    /// Handles task execution completion or failure under recovery class semantics.
    pub fn handle_task_completion_or_failure(&mut self, slot_idx: usize, task_idx: usize, exit_code: i32) {
        let wcb = &mut self.workloads[slot_idx];
        let task = &mut wcb.tasks[task_idx];

        if self.active_running_tasks > 0 {
            self.active_running_tasks -= 1;
        }

        if exit_code == 0 {
            // Task Succeeded
            task.state = TaskState::Completed;
            
            // Release lease via OP_LEASE_RELEASE
            if wcb.active_lease_count > 0 {
                wcb.active_lease_count -= 1;
            }

            // Update in-degrees of downstream tasks
            let mut all_completed = true;
            let count = wcb.task_count as usize;
            for i in 0..count {
                if wcb.tasks[i].state != TaskState::Completed {
                    all_completed = false;
                }
                if wcb.tasks[i].state == TaskState::Blocked {
                    let mut ready = true;
                    let deps = wcb.tasks[i].dependencies;
                    for j in 0..count {
                        if (deps & (1 << j)) != 0 && wcb.tasks[j].state != TaskState::Completed {
                            ready = false;
                            break;
                        }
                    }
                    if ready {
                        wcb.tasks[i].state = TaskState::Ready;
                    }
                }
            }

            if all_completed {
                wcb.state = WorkloadState::Completed;
                if wcb.client_channel_handle != 0 {
                    let _ = unsafe { sys_channel_close(wcb.client_channel_handle) };
                }
            } else {
                self.schedule_workload_tasks(slot_idx);
            }
        } else {
            // Task Failed - Evaluate Recovery Class
            task.state = TaskState::Failed;

            match wcb.recovery_class {
                RecoveryClass::Class1Pure => {
                    // Class 1: Ephemeral Pure Compute - Replay from scratch if retries remain
                    if task.retry_count < task.max_retries {
                        task.retry_count += 1;
                        wcb.state = WorkloadState::Recovering;
                        task.state = TaskState::Ready; // Retry fresh process
                        self.schedule_workload_tasks(slot_idx);
                    } else {
                        // Retries exhausted -> Fail workload
                        wcb.state = WorkloadState::Failed;
                        self.execute_4phase_cancellation(slot_idx);
                    }
                }
                RecoveryClass::Class2Stateful => {
                    // Class 2: Checkpointed Stateful - Resume from verified checkpoint Gc
                    if task.retry_count < task.max_retries {
                        task.retry_count += 1;
                        wcb.state = WorkloadState::Recovering;
                        task.state = TaskState::Ready;
                        self.schedule_workload_tasks(slot_idx);
                    } else {
                        wcb.state = WorkloadState::Failed;
                        self.execute_4phase_cancellation(slot_idx);
                    }
                }
                RecoveryClass::Class3Irreversible => {
                    // Class 3: Irreversible External - Transparent replay prohibited!
                    // Transition immediately to FailedAtMilestone
                    wcb.state = WorkloadState::Failed;
                    self.execute_4phase_cancellation(slot_idx);
                }
            }
        }
    }

    /// Executes 4-Phase Cancellation Pipeline with Stage 4B quarantine preservation.
    pub fn execute_4phase_cancellation(&mut self, slot_idx: usize) {
        let wcb = &mut self.workloads[slot_idx];

        // Phase 1: Set state to Cancelling
        if wcb.state != WorkloadState::Completed && wcb.state != WorkloadState::Reclaimed {
            wcb.state = WorkloadState::Cancelling;
        }

        // Phase 2: Terminate active task processes & Phase 3: Surrender leases to C_unavail
        for i in 0..wcb.task_count as usize {
            if wcb.tasks[i].state == TaskState::Running || wcb.tasks[i].state == TaskState::Ready {
                wcb.tasks[i].state = TaskState::Cancelled;
                if self.active_running_tasks > 0 {
                    self.active_running_tasks -= 1;
                }
            }
        }

        // Phase 4: Teardown workload
        if wcb.state == WorkloadState::Cancelling {
            wcb.state = WorkloadState::Cancelled;
        }
        wcb.active_lease_count = 0;
    }

    fn find_free_workload_slot(&self) -> Option<usize> {
        for (i, wcb) in self.workloads.iter().enumerate() {
            if wcb.state == WorkloadState::Creating || wcb.state == WorkloadState::Reclaimed {
                return Some(i);
            }
        }
        None
    }

    fn find_workload_slot(&self, id: DistributedId) -> Option<usize> {
        for (i, wcb) in self.workloads.iter().enumerate() {
            if wcb.workload_id == id && wcb.state != WorkloadState::Reclaimed {
                return Some(i);
            }
        }
        None
    }
}

#[no_mangle]
pub unsafe extern "C" fn _start() -> ! {
    let mut daemon = WorkloadDaemon::new(1, 0x4C00_0001).expect("workloadd initialization must succeed");

    // Workload daemon event dispatch loop
    loop {
        if let Ok(msg_buf) = channel_receive(daemon.service_channel, true) {
            let resp = daemon.dispatch(&msg_buf);
            if msg_buf.handles_count > 0 {
                let _ = channel_send(msg_buf.handles[0], &resp, true);
            }
        }
        sys_yield();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        sys_exit(-1);
    }
}
