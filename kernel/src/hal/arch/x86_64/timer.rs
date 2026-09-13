//! Project Zero - Timer & Interrupt Controller (PIC / PIT)
//!
//! Remaps the legacy 8259 PIC to vectors 32..47 and initializes PIT Channel 0
//! for periodic 100 Hz (10 ms) clock ticks to drive timekeeping and preemption.

use core::sync::atomic::{AtomicU64, Ordering};
use super::cpu::{outb, io_wait};

const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16    = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16    = 0xA1;

const PIT_CHANNEL_0: u16 = 0x40;
const PIT_COMMAND: u16   = 0x43;
const PIT_BASE_FREQUENCY: u32 = 1193182;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Remaps 8259 PIC interrupt vectors to avoid colliding with CPU exceptions (0..31).
/// IRQs 0..7 mapped to Vectors 32..39, IRQs 8..15 mapped to Vectors 40..47.
pub fn init_pic() {
    unsafe {
        // ICW1: Start initialization in cascade mode
        outb(PIC1_COMMAND, 0x11);
        io_wait();
        outb(PIC2_COMMAND, 0x11);
        io_wait();

        // ICW2: Vector offsets (Master = 32, Slave = 40)
        outb(PIC1_DATA, 32);
        io_wait();
        outb(PIC2_DATA, 40);
        io_wait();

        // ICW3: Tell Master about Slave at IRQ2, and Slave its cascade identity (2)
        outb(PIC1_DATA, 0x04);
        io_wait();
        outb(PIC2_DATA, 0x02);
        io_wait();

        // ICW4: 8086 mode
        outb(PIC1_DATA, 0x01);
        io_wait();
        outb(PIC2_DATA, 0x01);
        io_wait();

        // Unmask IRQ0 (Timer) only; mask all other IRQs initially
        outb(PIC1_DATA, 0xFE); // Bit 0 = 0 (unmask Timer IRQ0)
        outb(PIC2_DATA, 0xFF); // Mask all slave IRQs
    }
}

/// Sends End-of-Interrupt (EOI) signal to the PIC.
#[inline(always)]
pub fn send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_COMMAND, 0x20);
        }
        outb(PIC1_COMMAND, 0x20);
    }
}

/// Initializes PIT Channel 0 to generate periodic ticks at `freq_hz` (e.g. 100 Hz).
pub fn init_pit(freq_hz: u32) {
    let divisor = (PIT_BASE_FREQUENCY / freq_hz) as u16;
    unsafe {
        // Channel 0, lobyte/hibyte, Mode 2 (rate generator), binary 16-bit
        outb(PIT_COMMAND, 0x34);
        io_wait();
        outb(PIT_CHANNEL_0, (divisor & 0xFF) as u8);
        io_wait();
        outb(PIT_CHANNEL_0, ((divisor >> 8) & 0xFF) as u8);
    }
}

/// Monotonic tick counter
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Uptime in milliseconds based on 100 Hz frequency (10 ms / tick)
pub fn uptime_ms() -> u64 {
    ticks() * 10
}

/// Timer interrupt handler invoked by `isr_32` in `boot/isr.asm`.
#[no_mangle]
pub extern "C" fn timer_interrupt_handler() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    send_eoi(0);
}
