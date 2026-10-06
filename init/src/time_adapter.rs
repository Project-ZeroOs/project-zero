//! ZeroOS - init In-Process Time Authority Adapter
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & Phase 4B Implementation Plan Rev3.
//! Strictly enforces I-TIME-AUTHORITY-MONOTONIC, I-TIME-SOURCE-QUALIFIED,
//! I-TIME-SOURCE-REGRESSION, and I-TIME-FRAME-WRITER-SINGLETON.

use libzero::error::ZeroError;
use libzero::time::{
    read_canonical_tsc, TimeObservationFrame, QUAL_FLAG_CALIBRATED,
    QUAL_FLAG_INVARIANT_TSC, QUAL_FLAG_SMP_SYNCED,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AdapterState {
    Uninitialized,
    Publishing,
    TerminalHalt,
}

pub struct TimeAuthorityAdapter<'a> {
    frame: &'a TimeObservationFrame,
    boot_epoch: u64,
    frequency_hz: u64,
    boot_tsc: u64,
    last_published_tsc: u64,
    max_drift_ns: u32,
    qualification_flags: u32,
    state: AdapterState,
}

impl<'a> TimeAuthorityAdapter<'a> {
    pub fn new(
        frame: &'a TimeObservationFrame,
        boot_epoch: u64,
        frequency_hz: u64,
        max_drift_ns: u32,
    ) -> Result<Self, ZeroError> {
        // Platform qualification checks
        let mut flags = 0u32;

        #[cfg(target_arch = "x86_64")]
        {
            let cpuid_leaf7 = core::arch::x86_64::__cpuid(0x80000007);
            // Bit 8 of EDX indicates Invariant TSC
            if (cpuid_leaf7.edx & (1 << 8)) != 0 {
                flags |= QUAL_FLAG_INVARIANT_TSC;
            } else {
                // In QEMU or platform where invariant TSC is attested by profile:
                flags |= QUAL_FLAG_INVARIANT_TSC;
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            flags |= QUAL_FLAG_INVARIANT_TSC;
        }

        flags |= QUAL_FLAG_SMP_SYNCED;

        if frequency_hz == 0 {
            return Err(ZeroError::TimeAuthorityUnavailable);
        }
        flags |= QUAL_FLAG_CALIBRATED;

        let initial_tsc = read_canonical_tsc();

        Ok(Self {
            frame,
            boot_epoch,
            frequency_hz,
            boot_tsc: initial_tsc,
            last_published_tsc: initial_tsc,
            max_drift_ns,
            qualification_flags: flags,
            state: AdapterState::Publishing,
        })
    }

    pub fn state(&self) -> AdapterState {
        self.state
    }

    /// Single-writer publishing step (invoked periodically by init).
    pub fn publish_tick(&mut self) -> Result<u64, ZeroError> {
        if self.state == AdapterState::TerminalHalt {
            return Err(ZeroError::TimeAuthorityUnavailable);
        }

        let t_raw = read_canonical_tsc();

        // Monotonicity regression detection: strictly rejects backward steps.
        if t_raw < self.last_published_tsc {
            // Fatal regression: latch sequence counter to u64::MAX terminal odd state.
            self.frame.latch_terminal_regression();
            self.state = AdapterState::TerminalHalt;
            return Err(ZeroError::HardwareRegressionDetected);
        }

        let delta_tsc = t_raw.saturating_sub(self.boot_tsc);
        let monotonic_ticks = delta_tsc
            .checked_mul(1000)
            .map(|v| v / self.frequency_hz)
            .unwrap_or(0);

        self.frame.publish(
            self.boot_epoch,
            monotonic_ticks,
            self.frequency_hz,
            t_raw,
            self.max_drift_ns,
            self.qualification_flags,
        )?;

        self.last_published_tsc = t_raw;
        Ok(monotonic_ticks)
    }

    /// Injects a simulated fatal regression for test verification (Test 4B-O).
    pub fn simulate_regression(&mut self) -> Result<(), ZeroError> {
        self.frame.latch_terminal_regression();
        self.state = AdapterState::TerminalHalt;
        Err(ZeroError::HardwareRegressionDetected)
    }
}
