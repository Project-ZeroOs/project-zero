//! Project Zero - 16550 UART Serial Driver
//!
//! Provides deterministic serial output over COM1 (0x3F8) for kernel diagnostics.
//! Implements `core::fmt::Write` to enable formatted output via `kprint!` and `kprintln!`.

use core::fmt;
use super::cpu::{inb, outb};

pub const COM1_BASE: u16 = 0x3F8;

pub struct SerialPort {
    base_port: u16,
}

impl SerialPort {
    pub const fn new(base_port: u16) -> Self {
        Self { base_port }
    }

    /// Initializes the 16550 UART to 115,200 baud, 8 data bits, no parity, 1 stop bit (8N1).
    pub fn init(&self) {
        unsafe {
            // Disable all interrupts
            outb(self.base_port + 1, 0x00);

            // Enable DLAB (Divisor Latch Access Bit) to set baud rate divisor
            outb(self.base_port + 3, 0x80);

            // Set divisor to 1 (115200 baud = 115200 / 1)
            outb(self.base_port + 0, 0x01); // Divisor LSB
            outb(self.base_port + 1, 0x00); // Divisor MSB

            // Configure 8 bits, no parity, 1 stop bit (8N1), clear DLAB
            outb(self.base_port + 3, 0x03);

            // Enable FIFO, clear TX/RX queues, 14-byte threshold
            outb(self.base_port + 2, 0xC7);

            // Set RTS/DSR, enable Aux Output 2 (IRQs enabled in hardware)
            outb(self.base_port + 4, 0x0B);
        }
    }

    /// Returns true if the transmitter holding register is empty and ready for a byte.
    #[inline(always)]
    fn is_transmit_empty(&self) -> bool {
        unsafe { (inb(self.base_port + 5) & 0x20) != 0 }
    }

    /// Transmits a single byte to the serial line.
    pub fn send_byte(&self, byte: u8) {
        // Wait until transmitter is ready
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(self.base_port + 0, byte);
        }
    }

    /// Transmits a byte string to the serial line.
    pub fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                // Emit carriage return before newline for proper terminal formatting
                self.send_byte(b'\r');
            }
            self.send_byte(byte);
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        SerialPort::write_str(self, s);
        Ok(())
    }
}

/// Global COM1 serial port instance for kernel output.
pub static COM1: SerialPort = SerialPort::new(COM1_BASE);

/// Internal formatting dispatch
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let mut port = SerialPort::new(COM1_BASE);
    let _ = port.write_fmt(args);
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {
        $crate::hal::arch::x86_64::serial::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! kprintln {
    () => {
        $crate::kprint!("\n")
    };
    ($($arg:tt)*) => {
        $crate::kprint!("{}\n", format_args!($($arg)*))
    };
}
