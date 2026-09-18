//! Project Zero - Stage 3K Block Device Layer
//!
//! Authoritative Contract: Stage 3K Architecture Rev5 (Approved & Frozen).

use core::sync::atomic::{AtomicBool, Ordering};
use crate::fs::types::{BLOCK_SIZE, SECTOR_SIZE, SECTORS_PER_BLOCK, FsError};
use crate::hal::arch::x86_64::cpu::{inb, outb, inw, outw, io_wait};

pub const MAX_ATA_POLL_ITERATIONS: u32 = 100_000;
pub const MAX_BLOCK_DEVICES: usize = 2;

// Primary ATA Port Map
const ATA_PORT_DATA: u16         = 0x1F0;
const ATA_PORT_ERROR: u16        = 0x1F1;
const ATA_PORT_SECT_COUNT: u16   = 0x1F2;
const ATA_PORT_LBA_LOW: u16      = 0x1F3;
const ATA_PORT_LBA_MID: u16      = 0x1F4;
const ATA_PORT_LBA_HIGH: u16     = 0x1F5;
const ATA_PORT_DRIVE_HEAD: u16   = 0x1F6;
const ATA_PORT_STATUS_CMD: u16   = 0x1F7;
const ATA_PORT_ALT_STATUS: u16   = 0x3F6;

// ATA Commands
const ATA_CMD_READ_SECTORS: u8   = 0x20;
const ATA_CMD_WRITE_SECTORS: u8  = 0x30;
const ATA_CMD_IDENTIFY: u8       = 0xEC;
const ATA_CMD_FLUSH_CACHE: u8    = 0xE7;

// ATA Status Bits
const ATA_STATUS_ERR: u8 = 0x01;
const ATA_STATUS_DRQ: u8 = 0x08;
const ATA_STATUS_DF: u8  = 0x20;
const ATA_STATUS_DRDY: u8 = 0x40;
const ATA_STATUS_BSY: u8 = 0x80;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Offline = 0,
    Probing = 1,
    Ready = 2,
    Faulted = 3,
}

pub trait BlockDevice {
    fn device_id(&self) -> u8;
    fn block_count(&self) -> u64;
    fn state(&self) -> DeviceState;
    fn read_block(&mut self, block_idx: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError>;
    fn write_block(&mut self, block_idx: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError>;
    fn flush(&mut self) -> Result<(), FsError>;
}

// ---------------------------------------------------------------------------
// ATA PIO Primary Master Driver (Device ID = 1)
// ---------------------------------------------------------------------------

pub struct AtaPioBlockDevice {
    device_id: u8,
    state: DeviceState,
    total_blocks: u64,
}

impl AtaPioBlockDevice {
    pub const fn new() -> Self {
        Self {
            device_id: 1,
            state: DeviceState::Offline,
            total_blocks: 0,
        }
    }

    unsafe fn wait_ready(&mut self) -> Result<u8, FsError> {
        for _ in 0..MAX_ATA_POLL_ITERATIONS {
            let status = inb(ATA_PORT_STATUS_CMD);
            if (status & ATA_STATUS_BSY) == 0 {
                if (status & ATA_STATUS_ERR) != 0 || (status & ATA_STATUS_DF) != 0 {
                    self.state = DeviceState::Faulted;
                    return Err(FsError::DeviceError);
                }
                return Ok(status);
            }
        }
        self.state = DeviceState::Faulted;
        Err(FsError::DeviceTimeout)
    }

    unsafe fn wait_drq(&mut self) -> Result<(), FsError> {
        for _ in 0..MAX_ATA_POLL_ITERATIONS {
            let status = inb(ATA_PORT_STATUS_CMD);
            if (status & ATA_STATUS_BSY) == 0 && (status & ATA_STATUS_DRQ) != 0 {
                return Ok(());
            }
            if (status & ATA_STATUS_ERR) != 0 || (status & ATA_STATUS_DF) != 0 {
                self.state = DeviceState::Faulted;
                return Err(FsError::DeviceError);
            }
        }
        self.state = DeviceState::Faulted;
        Err(FsError::DeviceTimeout)
    }

    pub fn probe(&mut self) -> Result<(), FsError> {
        self.state = DeviceState::Probing;
        unsafe {
            // Select Master drive (0xA0)
            outb(ATA_PORT_DRIVE_HEAD, 0xA0);
            io_wait();

            // Clear sector count & LBA
            outb(ATA_PORT_SECT_COUNT, 0);
            outb(ATA_PORT_LBA_LOW, 0);
            outb(ATA_PORT_LBA_MID, 0);
            outb(ATA_PORT_LBA_HIGH, 0);

            // Send IDENTIFY
            outb(ATA_PORT_STATUS_CMD, ATA_CMD_IDENTIFY);
            io_wait();

            let status = inb(ATA_PORT_STATUS_CMD);
            if status == 0 || status == 0xFF {
                self.state = DeviceState::Offline;
                return Err(FsError::NotFound);
            }

            self.wait_ready()?;
            self.wait_drq()?;

            // Read 256 words (512 bytes) of IDENTIFY data
            let mut identify_buf = [0u16; 256];
            for word in identify_buf.iter_mut() {
                *word = inw(ATA_PORT_DATA);
            }

            // Sectors from words 60 and 61 (28-bit LBA capacity)
            let total_sectors = (identify_buf[60] as u64) | ((identify_buf[61] as u64) << 16);
            self.total_blocks = total_sectors / (SECTORS_PER_BLOCK as u64);
            self.state = DeviceState::Ready;
            Ok(())
        }
    }
}

impl BlockDevice for AtaPioBlockDevice {
    fn device_id(&self) -> u8 {
        self.device_id
    }

    fn block_count(&self) -> u64 {
        self.total_blocks
    }

    fn state(&self) -> DeviceState {
        self.state
    }

    fn read_block(&mut self, block_idx: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if self.state != DeviceState::Ready {
            return Err(FsError::DeviceError);
        }
        if block_idx >= self.total_blocks {
            return Err(FsError::InvalidArgument);
        }

        let base_lba = block_idx * (SECTORS_PER_BLOCK as u64);

        unsafe {
            for s in 0..SECTORS_PER_BLOCK {
                let lba = base_lba + (s as u64);
                self.wait_ready()?;

                outb(ATA_PORT_DRIVE_HEAD, 0xE0 | (((lba >> 24) & 0x0F) as u8));
                outb(ATA_PORT_SECT_COUNT, 1);
                outb(ATA_PORT_LBA_LOW, (lba & 0xFF) as u8);
                outb(ATA_PORT_LBA_MID, ((lba >> 8) & 0xFF) as u8);
                outb(ATA_PORT_LBA_HIGH, ((lba >> 16) & 0xFF) as u8);
                outb(ATA_PORT_STATUS_CMD, ATA_CMD_READ_SECTORS);

                self.wait_drq()?;

                let offset = s * SECTOR_SIZE;
                let sector_slice = &mut buf[offset..offset + SECTOR_SIZE];
                for chunk in sector_slice.chunks_exact_mut(2) {
                    let word = inw(ATA_PORT_DATA);
                    chunk[0] = (word & 0xFF) as u8;
                    chunk[1] = ((word >> 8) & 0xFF) as u8;
                }
            }
        }
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if self.state != DeviceState::Ready {
            return Err(FsError::DeviceError);
        }
        if block_idx >= self.total_blocks {
            return Err(FsError::InvalidArgument);
        }

        let base_lba = block_idx * (SECTORS_PER_BLOCK as u64);

        unsafe {
            for s in 0..SECTORS_PER_BLOCK {
                let lba = base_lba + (s as u64);
                self.wait_ready()?;

                outb(ATA_PORT_DRIVE_HEAD, 0xE0 | (((lba >> 24) & 0x0F) as u8));
                outb(ATA_PORT_SECT_COUNT, 1);
                outb(ATA_PORT_LBA_LOW, (lba & 0xFF) as u8);
                outb(ATA_PORT_LBA_MID, ((lba >> 8) & 0xFF) as u8);
                outb(ATA_PORT_LBA_HIGH, ((lba >> 16) & 0xFF) as u8);
                outb(ATA_PORT_STATUS_CMD, ATA_CMD_WRITE_SECTORS);

                self.wait_drq()?;

                let offset = s * SECTOR_SIZE;
                let sector_slice = &buf[offset..offset + SECTOR_SIZE];
                for chunk in sector_slice.chunks_exact(2) {
                    let word = (chunk[0] as u16) | ((chunk[1] as u16) << 8);
                    outw(ATA_PORT_DATA, word);
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), FsError> {
        if self.state != DeviceState::Ready {
            return Err(FsError::DeviceError);
        }
        unsafe {
            self.wait_ready()?;
            outb(ATA_PORT_DRIVE_HEAD, 0xE0);
            outb(ATA_PORT_STATUS_CMD, ATA_CMD_FLUSH_CACHE);
            self.wait_ready()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MemBlockDevice (In-Memory Mock Device, Device ID = 0)
// ---------------------------------------------------------------------------

pub const MEM_DEVICE_BLOCKS: usize = 256; // 1 MiB mock volume backed by 256 physical frames

pub struct MemBlockDevice;

// Track physical frame addresses allocated for MemBlockDevice
static mut MEM_DEVICE_FRAMES: [u64; MEM_DEVICE_BLOCKS] = [0; MEM_DEVICE_BLOCKS];

pub struct MemDeviceWrapper {
    device_id: u8,
    state: DeviceState,
    total_blocks: u64,
}

impl MemDeviceWrapper {
    pub const fn new() -> Self {
        Self {
            device_id: 0,
            state: DeviceState::Ready,
            total_blocks: MEM_DEVICE_BLOCKS as u64,
        }
    }

    pub fn init_storage(&mut self, pmm: &mut crate::mm::pmm::PhysicalMemoryManager) {
        unsafe {
            for i in 0..MEM_DEVICE_BLOCKS {
                if MEM_DEVICE_FRAMES[i] == 0 {
                    let frame = pmm.alloc_frame().expect("PMM frame allocation for MemDevice");
                    MEM_DEVICE_FRAMES[i] = frame.address();
                    let virt = (crate::mm::vmm::HHDM_BASE + frame.address()) as *mut u8;
                    core::ptr::write_bytes(virt, 0, BLOCK_SIZE);
                }
            }
        }
        let _ = crate::dev::registry::register_device(crate::dev::types::BOOTSTRAP_MEM_DEVICE_ID, crate::dev::types::DeviceClass::Storage);
        self.state = DeviceState::Ready;
    }

    pub fn reset(&mut self) {
        unsafe {
            for &phys in &MEM_DEVICE_FRAMES {
                if phys != 0 {
                    let virt = (crate::mm::vmm::HHDM_BASE + phys) as *mut u8;
                    core::ptr::write_bytes(virt, 0, BLOCK_SIZE);
                }
            }
        }
        self.state = DeviceState::Ready;
    }
}

impl BlockDevice for MemDeviceWrapper {
    fn device_id(&self) -> u8 {
        self.device_id
    }

    fn block_count(&self) -> u64 {
        self.total_blocks
    }

    fn state(&self) -> DeviceState {
        self.state
    }

    fn read_block(&mut self, block_idx: u64, buf: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if self.state != DeviceState::Ready {
            return Err(FsError::DeviceError);
        }
        if block_idx >= self.total_blocks {
            return Err(FsError::InvalidArgument);
        }
        unsafe {
            let phys = MEM_DEVICE_FRAMES[block_idx as usize];
            if phys == 0 {
                return Err(FsError::DeviceError);
            }
            let virt = (crate::mm::vmm::HHDM_BASE + phys) as *const u8;
            core::ptr::copy_nonoverlapping(virt, buf.as_mut_ptr(), BLOCK_SIZE);
        }
        Ok(())
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        if self.state != DeviceState::Ready {
            return Err(FsError::DeviceError);
        }
        if block_idx >= self.total_blocks {
            return Err(FsError::InvalidArgument);
        }
        unsafe {
            let phys = MEM_DEVICE_FRAMES[block_idx as usize];
            if phys == 0 {
                return Err(FsError::DeviceError);
            }
            let virt = (crate::mm::vmm::HHDM_BASE + phys) as *mut u8;
            core::ptr::copy_nonoverlapping(buf.as_ptr(), virt, BLOCK_SIZE);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), FsError> {
        if self.state != DeviceState::Ready {
            return Err(FsError::DeviceError);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Global Block Device Registry & Synchronization
// Lock Order: FILESYSTEM_LOCK (1) < STORAGE_OBJECT_TABLE_LOCK (2)
//             < BLOCK_CACHE_LOCK (3) < BLOCK_DEVICE_LOCK (4)
// ---------------------------------------------------------------------------

pub struct BlockDeviceLock {
    lock: AtomicBool,
}

impl BlockDeviceLock {
    pub const fn new() -> Self {
        Self {
            lock: AtomicBool::new(false),
        }
    }

    #[inline(always)]
    pub fn acquire(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    #[inline(always)]
    pub fn release(&self) {
        self.lock.store(false, Ordering::Release);
    }
}

pub static BLOCK_DEVICE_LOCK: BlockDeviceLock = BlockDeviceLock::new();
pub static mut MEM_DEVICE: MemDeviceWrapper = MemDeviceWrapper::new();
pub static mut ATA_DEVICE: AtaPioBlockDevice = AtaPioBlockDevice::new();

pub fn get_device<'a>(id: u8) -> Result<&'a mut dyn BlockDevice, FsError> {
    unsafe {
        match id {
            0 => Ok(&mut MEM_DEVICE),
            1 => Ok(&mut ATA_DEVICE),
            _ => Err(FsError::NotFound),
        }
    }
}
