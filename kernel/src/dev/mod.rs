//! Project Zero - Stage 3L Device Subsystem
//!
//! Authoritative Contract: Stage 3L Architecture Rev3 (Approved & Frozen).

pub mod types;
pub mod registry;
pub mod interrupt;
pub mod dma;
pub mod mmio;
pub mod tests;

/// Global initialization of the Stage 3L Device Subsystem.
pub fn init() {
    registry::init();
    interrupt::init();
    dma::init();
}
