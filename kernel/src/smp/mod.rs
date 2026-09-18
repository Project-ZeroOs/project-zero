//! Stage 3N — SMP / Multi-Core Architecture
//!
//! Module root. Exports all SMP subsystems:
//!  - `types`:      static tables, constants, lifecycle enum, ASpaceGeneration
//!  - `discovery`:  MADT/CPUID topology discovery and LAPIC→CpuId mapping
//!  - `ipi`:        ICR dispatch, LAPIC SVR configuration, IPI ISR handlers
//!  - `tlb`:        synchronous TLB shootdown engine (`I-SMP-TLB-1`)
//!  - `bootstrap`:  INIT-SIPI-SIPI AP startup protocol
//!  - `scheduler`:  per-CPU runqueue selection and preemption policy
//!  - `tests`:      46-test bare-metal verification harness

pub mod types;
pub mod discovery;
pub mod ipi;
pub mod tlb;
pub mod bootstrap;
pub mod scheduler;
pub mod tests;
