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

pub static TICKS: AtomicU64 = AtomicU64::new(0);

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

use core::sync::atomic::AtomicBool;

pub static GPR_TEST_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static GPR_TEST_TICKS: AtomicU64 = AtomicU64::new(0);
pub static GPR_TEST_TARGET: AtomicU64 = AtomicU64::new(10);

extern "C" {
    pub static mut gpr_sentinel_flag: u64;
}

/// Timer interrupt handler invoked by `isr_32` in `boot/isr.asm`.
#[no_mangle]
pub extern "C" fn timer_interrupt_handler(frame: *mut crate::hal::arch::x86_64::lapic::InterruptFrame) {
    if GPR_TEST_ACTIVE.load(Ordering::Relaxed) {
        TICKS.fetch_add(1, Ordering::Relaxed);
        unsafe {
            crate::hal::arch::x86_64::lapic::lapic_eoi();
            let f = &*frame;
            assert_eq!(f.rax, 0x1111_1111_1111_1111, "In-flight RAX corrupted in ISR frame");
            assert_eq!(f.rcx, 0x2222_2222_2222_2222, "In-flight RCX corrupted in ISR frame");
            assert_eq!(f.rdx, 0x3333_3333_3333_3333, "In-flight RDX corrupted in ISR frame");
            assert_eq!(f.rbx, 0x4444_4444_4444_4444, "In-flight RBX corrupted in ISR frame");
            assert_eq!(f.rbp, 0x5555_5555_5555_5555, "In-flight RBP corrupted in ISR frame");
            assert_eq!(f.rsi, 0x6666_6666_6666_6666, "In-flight RSI corrupted in ISR frame");
            assert_eq!(f.rdi, 0x7777_7777_7777_7777, "In-flight RDI corrupted in ISR frame");
            assert_eq!(f.r8,  0x8888_8888_8888_8888, "In-flight R8 corrupted in ISR frame");
            assert_eq!(f.r9,  0x9999_9999_9999_9999, "In-flight R9 corrupted in ISR frame");
            assert_eq!(f.r10, 0xAAAA_AAAA_AAAA_AAAA, "In-flight R10 corrupted in ISR frame");
            assert_eq!(f.r11, 0xBBBB_BBBB_BBBB_BBBB, "In-flight R11 corrupted in ISR frame");
            assert_eq!(f.r12, 0xCCCC_CCCC_CCCC_CCCC, "In-flight R12 corrupted in ISR frame");
            assert_eq!(f.r13, 0xDDDD_DDDD_DDDD_DDDD, "In-flight R13 corrupted in ISR frame");
            assert_eq!(f.r14, 0xEEEE_EEEE_EEEE_EEEE, "In-flight R14 corrupted in ISR frame");
            assert_eq!(f.r15, 0xFFFF_FFFF_FFFF_FFFF, "In-flight R15 corrupted in ISR frame");
        }

        let observed = GPR_TEST_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        if observed >= GPR_TEST_TARGET.load(Ordering::Relaxed) {
            unsafe {
                gpr_sentinel_flag = 1;
            }
        }
        return;
    }

    crate::dev::interrupt::on_timer_tick();
    crate::task::scheduler::schedule_preemption_from_irq(frame);
}
