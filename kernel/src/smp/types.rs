//! Stage 3N — SMP / Multi-Core Architecture: Core Types
//!
//! Defines all SMP-specific types, constants, and static tables.
//! The 48-byte `PerCpu` structure is UNCHANGED (frozen Stage 3A).
//! All per-CPU multi-core state lives in `PER_CPU_STATE: [CpuState; MAX_CPUS]`.
//!
//! Invariants frozen in Stage 3N Rev4:
//! - `I-SMP-TLB-1`:  Activation and mutation mutually serialized by `aspace.lock`.
//! - `I-SMP-ASPACE-1`: AddressSpace frames freed only after full rendezvous.
//! - `I-SMP-ASPACE-2`: Terminating/Zombie/Reclaiming/Free processes cannot be activated.
//! - `I-SMP-ASPACE-3`: Fail-closed panic if rendezvous timeout on unquiesced CPU.
//! - `I-SMP-SCHED-1`:  Deterministic all-cores-busy preemption policy.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Maximum number of CPUs supported.
pub const MAX_CPUS: usize = 4;

/// IPI Timeout Iteration Bounds
pub const MAX_IPI_POLL_ITERATIONS: u32  = 100_000;
pub const MAX_TLB_POLL_ITERATIONS: u32  = 1_000_000;
pub const MAX_RENDEZVOUS_POLL_TICKS: u32 = 10;   // 100 ms at 100 Hz
pub const MAX_AP_STARTUP_TICKS: u32      = 10;   // 100 ms at 100 Hz

/// IPI Vector Assignments (Stage 3N Rev4, Frozen)
pub const IPI_VECTOR_STOP: u8          = 0xFB;  // 251
pub const IPI_VECTOR_RESCHEDULE: u8    = 0xFC;  // 252
pub const IPI_VECTOR_TLB_SHOOTDOWN: u8 = 0xFD;  // 253
pub const LAPIC_SPURIOUS_VECTOR: u8    = 0xFF;  // 255

/// Stable logical CPU identifier (0..MAX_CPUS-1).
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpuId(pub u32);

impl CpuId {
    pub const BSP: CpuId = CpuId(0);

    #[inline(always)]
    pub fn as_usize(self) -> usize { self.0 as usize }

    #[inline(always)]
    pub fn is_valid(self) -> bool { (self.0 as usize) < MAX_CPUS }
}

/// CPU Lifecycle States.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuLifecycleState {
    /// Slot unused (pre-discovery)
    Absent   = 0,
    /// BSP initiated INIT-SIPI-SIPI; waiting for AP to signal Online
    Starting = 1,
    /// AP finished long-mode transition; handshake complete
    Online   = 2,
    /// AP entered scheduler and is executing threads
    Active   = 3,
    /// AP failed to boot within MAX_AP_STARTUP_TICKS
    Failed   = 4,
    /// AP is offline (evacuated)
    Offline  = 5,
}

/// Per-CPU slot descriptor (32 bytes, 8-byte aligned).
#[repr(C)]
pub struct CpuSlot {
    pub cpu_id:    CpuId,
    pub lapic_id:  u32,
    pub state:     CpuLifecycleState,
    pub is_bsp:    bool,
    pub _pad:      [u8; 14],
    pub generation: u32,
    pub _pad2:     u32,
}

const _: () = assert!(core::mem::size_of::<CpuSlot>() == 32);
const _: () = assert!(core::mem::align_of::<CpuSlot>() == 4);

impl CpuSlot {
    pub const fn empty() -> Self {
        Self {
            cpu_id:     CpuId(0),
            lapic_id:   0,
            state:      CpuLifecycleState::Absent,
            is_bsp:     false,
            _pad:       [0; 14],
            generation: 0,
            _pad2:      0,
        }
    }
}

/// Unified monotonic generation counter for an AddressSpace
/// (covers both page-table mutations and exit rendezvous).
/// Wraparound is forbidden: u64::MAX panics fail-closed.
pub struct ASpaceGeneration(AtomicU64);

impl ASpaceGeneration {
    pub const fn new() -> Self { Self(AtomicU64::new(1)) }

    /// Increment monotonically and return new generation.
    pub fn inc(&self) -> u64 {
        let prev = self.0.fetch_add(1, Ordering::SeqCst);
        let gen = prev.wrapping_add(1);
        assert!(gen != u64::MAX, "SMP: AddressSpace generation counter exhausted (u64::MAX)");
        gen
    }

    pub fn load(&self) -> u64 { self.0.load(Ordering::SeqCst) }
    pub fn load_relaxed(&self) -> u64 { self.0.load(Ordering::Relaxed) }
}

/// Per-core runqueue (simple counter + spinlock token for host tests)
#[repr(C)]
pub struct CpuRunQueues {
    pub critical_count: AtomicU32,
    pub high_count:     AtomicU32,
    pub normal_count:   AtomicU32,
    pub _pad:           u32,
}

impl CpuRunQueues {
    pub const fn empty() -> Self {
        Self {
            critical_count: AtomicU32::new(0),
            high_count:     AtomicU32::new(0),
            normal_count:   AtomicU32::new(0),
            _pad:           0,
        }
    }
}

/// Extended per-CPU state for SMP (lives outside frozen 48-byte PerCpu).
/// Size <= 128 bytes.
#[repr(C, align(8))]
pub struct CpuState {
    /// CPU lifecycle state (Stage 3N)
    pub state:               CpuLifecycleState,
    pub _pad0:               [u8; 3],
    /// Hardware LAPIC ID (for ICR targeting)
    pub lapic_id:            u32,
    /// Logical CPU ID
    pub cpu_id:              CpuId,
    /// IPI pending bitmask (coalesced dispatch)
    pub ipi_pending_mask:    AtomicU32,
    /// PID of currently active address space (under aspace.lock)
    pub active_pid:          AtomicU64,
    /// TLB acknowledgement generation (written by ISR 253, zero-lock)
    pub tlb_ack_gen:         AtomicU64,
    /// TLB acknowledgement AddressSpace ID (I-SMP-TLB-2)
    pub tlb_ack_aspace_id:   AtomicU64,
    /// Exit rendezvous acknowledgement generation (written on cooperative exit)
    pub rendezvous_ack_gen:  AtomicU64,
    /// Interrupt count telemetry
    pub interrupt_count:     AtomicU64,
    /// Per-core kernel stack top (for TSS RSP0 and IST1)
    pub kernel_stack_top:    u64,
    /// IST1 stack top for #DF
    pub ist1_stack_top:      u64,
    /// Runqueue counters
    pub run_queues:          CpuRunQueues,
    /// Scheduler lock (Level 10; acquired ascending by CpuId)
    pub lock_held:           AtomicU32,
    pub _pad1:               [u8; 12],
}

const _: () = assert!(core::mem::size_of::<CpuState>() <= 128);

impl CpuState {
    pub const fn empty() -> Self {
        Self {
            state:              CpuLifecycleState::Absent,
            _pad0:              [0; 3],
            lapic_id:           0,
            cpu_id:             CpuId(0),
            ipi_pending_mask:   AtomicU32::new(0),
            active_pid:         AtomicU64::new(0),
            tlb_ack_gen:        AtomicU64::new(0),
            tlb_ack_aspace_id:  AtomicU64::new(0),
            rendezvous_ack_gen: AtomicU64::new(0),
            interrupt_count:    AtomicU64::new(0),
            kernel_stack_top:   0,
            ist1_stack_top:     0,
            run_queues:         CpuRunQueues::empty(),
            lock_held:          AtomicU32::new(0),
            _pad1:              [0; 12],
        }
    }
}

/// LAPIC ID → CpuId translation table (256 entries, 1 byte each).
/// 0xFF = unmapped.
pub const LAPIC_UNMAPPED: u8 = 0xFF;

/// Per-target TLB shootdown request slot (64 bytes, cache-line aligned).
/// Invariants:
/// - `I-SMP-TLB-2`: Evaluated with (aspace_id, generation) tuple.
/// - `I-SMP-TLB-3`: Published via active.store(true, Release); consumed via active.load(Acquire).
#[repr(C, align(64))]
pub struct TlbTargetSlot {
    pub aspace_id:  AtomicU64,
    pub generation: AtomicU64,
    pub vaddr:      AtomicU64,
    pub active:     core::sync::atomic::AtomicBool,
    pub _pad:       [u8; 39],
}

const _: () = assert!(core::mem::size_of::<TlbTargetSlot>() == 64);

impl TlbTargetSlot {
    pub const fn empty() -> Self {
        Self {
            aspace_id:  AtomicU64::new(0),
            generation: AtomicU64::new(0),
            vaddr:      AtomicU64::new(0),
            active:     core::sync::atomic::AtomicBool::new(false),
            _pad:       [0; 39],
        }
    }
}

/// Global monotonic AddressSpace ID counter (I-SMP-TLB-4).
/// Never wraps, never reused.
pub static NEXT_ASPACE_ID: AtomicU64 = AtomicU64::new(1);

/// Global TLB shootdown lock (Level 10.5 in lock hierarchy).
/// Acquired under aspace.lock to serialize broadcasts across AddressSpaces.
pub static TLB_SHOOTDOWN_LOCK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Per-target TLB shootdown request slots.
pub static TLB_SHOOTDOWN_REQUEST: [TlbTargetSlot; MAX_CPUS] = [
    TlbTargetSlot::empty(),
    TlbTargetSlot::empty(),
    TlbTargetSlot::empty(),
    TlbTargetSlot::empty(),
];

/// Legacy/global TLB shootdown descriptor (maintained for compatibility/diagnostics).
#[repr(C, align(64))]
pub struct TlbShootdownState {
    pub aspace_pid:   AtomicU64,
    pub gen:          AtomicU64,
    pub target_mask:  AtomicU32,
    pub vaddr:        AtomicU64,
    pub _pad:         [u8; 28],
}

const _: () = assert!(core::mem::size_of::<TlbShootdownState>() == 64);

impl TlbShootdownState {
    pub const fn empty() -> Self {
        Self {
            aspace_pid:  AtomicU64::new(0),
            gen:         AtomicU64::new(0),
            target_mask: AtomicU32::new(0),
            vaddr:       AtomicU64::new(0),
            _pad:        [0; 28],
        }
    }
}

// ====================================================================
// Global Static SMP Tables
// ====================================================================

/// Authoritative CPU slot table.
pub static mut CPU_SLOT_TABLE: [CpuSlot; MAX_CPUS] = [
    CpuSlot::empty(), CpuSlot::empty(), CpuSlot::empty(), CpuSlot::empty(),
];

/// Extended per-CPU state (not in frozen 48-byte PerCpu).
pub static mut PER_CPU_STATE: [CpuState; MAX_CPUS] = [
    CpuState::empty(), CpuState::empty(), CpuState::empty(), CpuState::empty(),
];

/// LAPIC ID → Logical CpuId mapping.
pub static mut LAPIC_TO_CPUID: [u8; 256] = [LAPIC_UNMAPPED; 256];

/// Global TLB shootdown descriptor.
pub static TLB_SHOOTDOWN_STATE: TlbShootdownState = TlbShootdownState::empty();

/// Number of discovered, online CPUs.
pub static ONLINE_CPU_COUNT: AtomicU32 = AtomicU32::new(1); // BSP always online
