//! ZeroOS - libzero Qualified Monotonic Time Authority Substrate
//!
//! Authoritative Contract: Stage 4B Architecture Rev12 & ADR-0025 Rev12.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::error::ZeroError;

pub const QUAL_FLAG_INVARIANT_TSC: u32 = 1 << 0;
pub const QUAL_FLAG_SMP_SYNCED: u32 = 1 << 1;
pub const QUAL_FLAG_CALIBRATED: u32 = 1 << 2;

pub const SEQUENCE_TERMINAL_LATCH: u64 = u64::MAX;
pub const SEQUENCE_MAX_VALID_EVEN: u64 = u64::MAX - 1;
pub const MAX_TIME_OBSERVATION_AGE_MS: u64 = 50;

/// 64-byte, cache-line aligned shared-memory time observation frame.
/// Backed by Stage 3G Shared Memory. init holds SHM_WRITE; consumers hold SHM_READ.
#[repr(C, align(64))]
pub struct TimeObservationFrame {
    pub sequence: AtomicU64,            // Seqlock counter; odd = write in progress; u64::MAX = terminal latch
    pub boot_epoch: AtomicU64,          // Durable boot incarnation identifier
    pub monotonic_ticks: AtomicU64,     // Monotonic hardware tick snapshot
    pub frequency_hz: AtomicU64,        // Qualified counter frequency (Hz)
    pub capture_tsc: AtomicU64,         // Raw TSC reading at observation capture
    pub max_drift_ns: AtomicU32,        // Maximum SMP phase divergence bound
    pub qualification_flags: AtomicU32, // Bit 0: Invariant, Bit 1: SMP synced, Bit 2: Calibrated
    pub _reserved: [u8; 16],            // Explicit padding to 64 bytes
}

const _: () = assert!(core::mem::size_of::<TimeObservationFrame>() == 64);
const _: () = assert!(core::mem::align_of::<TimeObservationFrame>() == 64);

/// Immutable snapshot of a consistent TimeObservationFrame read.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeObservation {
    pub sequence: u64,
    pub boot_epoch: u64,
    pub monotonic_ticks: u64,
    pub frequency_hz: u64,
    pub capture_tsc: u64,
    pub max_drift_ns: u32,
    pub qualification_flags: u32,
}

impl TimeObservationFrame {
    pub const fn new() -> Self {
        Self {
            sequence: AtomicU64::new(0),
            boot_epoch: AtomicU64::new(0),
            monotonic_ticks: AtomicU64::new(0),
            frequency_hz: AtomicU64::new(0),
            capture_tsc: AtomicU64::new(0),
            max_drift_ns: AtomicU32::new(0),
            qualification_flags: AtomicU32::new(0),
            _reserved: [0u8; 16],
        }
    }

    /// Lock-free seqlock reader with Acquire/Release memory ordering.
    /// Rejects odd sequence counters and u64::MAX terminal latches fail-closed.
    pub fn read_observation(&self) -> Result<TimeObservation, ZeroError> {
        let mut retries = 0usize;
        loop {
            let seq1 = self.sequence.load(Ordering::Acquire);
            if seq1 == SEQUENCE_TERMINAL_LATCH {
                return Err(ZeroError::TimeAuthorityUnavailable);
            }
            if seq1 & 1 != 0 {
                retries += 1;
                if retries > 10_000 {
                    return Err(ZeroError::TimeAuthorityUnavailable);
                }
                core::hint::spin_loop();
                continue;
            }

            // Read atomic payload fields
            let boot_epoch = self.boot_epoch.load(Ordering::Relaxed);
            let monotonic_ticks = self.monotonic_ticks.load(Ordering::Relaxed);
            let frequency_hz = self.frequency_hz.load(Ordering::Relaxed);
            let capture_tsc = self.capture_tsc.load(Ordering::Relaxed);
            let max_drift_ns = self.max_drift_ns.load(Ordering::Relaxed);
            let qualification_flags = self.qualification_flags.load(Ordering::Relaxed);

            let seq2 = self.sequence.load(Ordering::Acquire);
            if seq1 == seq2 {
                return Ok(TimeObservation {
                    sequence: seq1,
                    boot_epoch,
                    monotonic_ticks,
                    frequency_hz,
                    capture_tsc,
                    max_drift_ns,
                    qualification_flags,
                });
            }

            retries += 1;
            if retries > 10_000 {
                return Err(ZeroError::TimeAuthorityUnavailable);
            }
            core::hint::spin_loop();
        }
    }

    /// Writer update routine for init Time Authority Adapter.
    pub fn publish(
        &self,
        boot_epoch: u64,
        monotonic_ticks: u64,
        frequency_hz: u64,
        capture_tsc: u64,
        max_drift_ns: u32,
        qualification_flags: u32,
    ) -> Result<(), ZeroError> {
        let cur_seq = self.sequence.load(Ordering::Relaxed);
        if cur_seq == SEQUENCE_TERMINAL_LATCH {
            return Err(ZeroError::TimeAuthorityUnavailable);
        }

        let next_odd = cur_seq.checked_add(1).ok_or(ZeroError::TimeAuthorityUnavailable)?;
        if next_odd >= SEQUENCE_MAX_VALID_EVEN {
            // Latch to terminal odd state
            self.sequence.store(SEQUENCE_TERMINAL_LATCH, Ordering::Release);
            return Err(ZeroError::TimeAuthorityUnavailable);
        }

        self.sequence.store(next_odd, Ordering::Release);

        self.boot_epoch.store(boot_epoch, Ordering::Relaxed);
        self.monotonic_ticks.store(monotonic_ticks, Ordering::Relaxed);
        self.frequency_hz.store(frequency_hz, Ordering::Relaxed);
        self.capture_tsc.store(capture_tsc, Ordering::Relaxed);
        self.max_drift_ns.store(max_drift_ns, Ordering::Relaxed);
        self.qualification_flags.store(qualification_flags, Ordering::Relaxed);

        let next_even = next_odd + 1;
        self.sequence.store(next_even, Ordering::Release);
        Ok(())
    }

    /// Sets the terminal latch to lock out all readers permanently upon fatal regression.
    pub fn latch_terminal_regression(&self) {
        self.sequence.store(SEQUENCE_TERMINAL_LATCH, Ordering::Release);
    }
}

impl Default for TimeObservationFrame {
    fn default() -> Self {
        Self::new()
    }
}

/// Canonical hardware reader sequence: LFENCE; RDTSC.
/// Strict execution serialization across all active cores.
#[inline(always)]
pub fn read_canonical_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_mm_lfence();
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

/// Evaluates consumer freshness against independently captured hardware TSC.
/// Enforces I-TIME-OBSERVATION-FRESHNESS (delta T <= 50ms) and I-TIME-SOURCE-REGRESSION.
pub fn evaluate_freshness(obs: &TimeObservation, t_hardware_eval: u64) -> Result<(), ZeroError> {
    if t_hardware_eval < obs.capture_tsc {
        return Err(ZeroError::HardwareRegressionDetected);
    }
    if obs.frequency_hz == 0 {
        return Err(ZeroError::TimeAuthorityUnavailable);
    }

    let delta_tsc = t_hardware_eval - obs.capture_tsc;
    // delta_ms = (delta_tsc * 1000) / freq
    let delta_ms = delta_tsc
        .checked_mul(1000)
        .map(|v| v / obs.frequency_hz)
        .unwrap_or(u64::MAX);

    if delta_ms > MAX_TIME_OBSERVATION_AGE_MS {
        return Err(ZeroError::TimeObservationStale);
    }
    Ok(())
}
