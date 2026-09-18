//! ZeroOS - libzero System Service Supervisor & Lifecycle State Machine

use crate::error::ZeroError;

pub const MAX_SUPERVISED_SERVICES: usize = 8;
pub const DEFAULT_MAX_RETRIES: u32 = 3;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceLifecycleState {
    Unused = 0,
    Declared = 1,
    Starting = 2,
    Running = 3,
    Failed = 4,
    Restarting = 5,
    Stopping = 6,
    Stopped = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisedService {
    pub occupied: bool,
    pub service_id: u32,
    pub name: [u8; 32],
    pub state: ServiceLifecycleState,
    pub restart_count: u32,
    pub max_retries: u32,
}

impl SupervisedService {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            service_id: 0,
            name: [0u8; 32],
            state: ServiceLifecycleState::Unused,
            restart_count: 0,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

pub struct Supervisor {
    pub services: [SupervisedService; MAX_SUPERVISED_SERVICES],
    pub next_service_id: u32,
}

impl Supervisor {
    pub const fn new() -> Self {
        Self {
            services: [SupervisedService::empty(); MAX_SUPERVISED_SERVICES],
            next_service_id: 1,
        }
    }

    /// Declares a service to be supervised.
    pub fn declare_service(&mut self, name: &[u8; 32], max_retries: u32) -> Result<usize, ZeroError> {
        for (i, svc) in self.services.iter_mut().enumerate() {
            if !svc.occupied {
                svc.occupied = true;
                svc.service_id = self.next_service_id;
                self.next_service_id = self.next_service_id.wrapping_add(1);
                svc.name = *name;
                svc.state = ServiceLifecycleState::Declared;
                svc.restart_count = 0;
                svc.max_retries = max_retries;
                return Ok(i);
            }
        }
        Err(ZeroError::ObjectTableFull)
    }

    /// Transitions a declared or restarting service to Starting.
    pub fn transition_starting(&mut self, index: usize) -> Result<(), ZeroError> {
        if index >= MAX_SUPERVISED_SERVICES || !self.services[index].occupied {
            return Err(ZeroError::NotFound);
        }
        let svc = &mut self.services[index];
        match svc.state {
            ServiceLifecycleState::Declared | ServiceLifecycleState::Restarting => {
                svc.state = ServiceLifecycleState::Starting;
                Ok(())
            }
            _ => Err(ZeroError::InvalidRequest),
        }
    }

    /// Transitions a starting service to Running.
    pub fn transition_running(&mut self, index: usize) -> Result<(), ZeroError> {
        if index >= MAX_SUPERVISED_SERVICES || !self.services[index].occupied {
            return Err(ZeroError::NotFound);
        }
        let svc = &mut self.services[index];
        match svc.state {
            ServiceLifecycleState::Starting => {
                svc.state = ServiceLifecycleState::Running;
                Ok(())
            }
            _ => Err(ZeroError::InvalidRequest),
        }
    }

    /// Handles service failure with bounded restart backoff policy.
    ///
    /// Returns `Ok(true)` if restarted, `Ok(false)` if max retries exceeded (fail-closed).
    pub fn handle_failure(&mut self, index: usize) -> Result<bool, ZeroError> {
        if index >= MAX_SUPERVISED_SERVICES || !self.services[index].occupied {
            return Err(ZeroError::NotFound);
        }
        let svc = &mut self.services[index];
        svc.state = ServiceLifecycleState::Failed;

        if svc.restart_count < svc.max_retries {
            svc.restart_count += 1;
            svc.state = ServiceLifecycleState::Restarting;
            Ok(true)
        } else {
            // Terminal failure - prevent infinite restart loop
            Ok(false)
        }
    }

    /// Coordinates orderly shutdown of a service.
    pub fn stop_service(&mut self, index: usize) -> Result<(), ZeroError> {
        if index >= MAX_SUPERVISED_SERVICES || !self.services[index].occupied {
            return Err(ZeroError::NotFound);
        }
        let svc = &mut self.services[index];
        match svc.state {
            ServiceLifecycleState::Running | ServiceLifecycleState::Starting | ServiceLifecycleState::Restarting => {
                svc.state = ServiceLifecycleState::Stopping;
                svc.state = ServiceLifecycleState::Stopped;
                Ok(())
            }
            _ => Err(ZeroError::InvalidRequest),
        }
    }

    /// Shuts down all supervised services cleanly.
    pub fn shutdown_all(&mut self) {
        for svc in self.services.iter_mut() {
            if svc.occupied && svc.state != ServiceLifecycleState::Stopped {
                svc.state = ServiceLifecycleState::Stopping;
                svc.state = ServiceLifecycleState::Stopped;
            }
        }
    }
}
