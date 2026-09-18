//! Project Zero - Stage 3L Device Subsystem Types & Descriptors
//!
//! Authoritative Contract: Stage 3L Architecture Rev3 (Approved & Frozen).

use core::sync::atomic::{AtomicU64, Ordering};

pub const MAX_DEVICES: usize = 16;
pub const MAX_DEVICE_RESOURCES: usize = 64;
pub const MAX_INTERRUPT_VECTORS: usize = 256;
pub const MAX_SHARED_IRQ_BINDINGS: usize = 4;
pub const MAX_DMA_BUFFERS: usize = 32;
pub const MAX_FRAMES_PER_BUFFER: usize = 16;   // Up to 64 KiB per DMA buffer
pub const MAX_PINNED_DMA_FRAMES: usize = 128;  // 128 * 4096 = 512 KiB max total DMA pool

pub const BOOTSTRAP_MEM_DEVICE_ID: DeviceId = DeviceId(0);
pub const BOOTSTRAP_ATA_DEVICE_ID: DeviceId = DeviceId(1);
pub static NEXT_DEVICE_ID: AtomicU64 = AtomicU64::new(2);

/// Globally unique 64-bit Device Identifier.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u64);

impl DeviceId {
    #[inline(always)]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    #[inline(always)]
    pub fn next_dynamic() -> Self {
        Self(NEXT_DEVICE_ID.fetch_add(1, Ordering::SeqCst))
    }

    #[inline(always)]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Unknown             = 0,
    Storage             = 1, // ATA, NVMe, VirtIO-Blk, MemBlock
    Network             = 2, // Ethernet NIC, VirtIO-Net
    Display             = 3, // VGA, Linear Framebuffer, VirtIO-GPU
    Input               = 4, // PS/2 Keyboard, Mouse, VirtIO-Input
    BusController       = 5, // PCI Host Bridge, PCIe Root Port, USB Controller
    Accelerator         = 6, // GPU Compute Engine, NPU Inference Engine
    Timer               = 7, // LAPIC Timer, PIT, HPET, RTC
    InterruptController = 8, // PIC, IOAPIC, LAPIC
    Platform            = 9, // Power Management, ACPI, System Reset
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceLifecycleState {
    Discovered = 0,
    Probed     = 1,
    Attached   = 2,
    Ready      = 3,
    Active     = 4,
    Quiescing  = 5,
    Detached   = 6,
    Released   = 7,
    Faulted    = 8,
    Resetting  = 9,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Unused    = 0,
    IoPort    = 1, // x86 I/O Port Range
    Mmio      = 2, // Physical Memory-Mapped I/O Window
    Irq       = 3, // Hardware IRQ line & mapped vector
    DmaBuffer = 4, // Pinned physical memory buffer
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceSharing {
    Exclusive = 0,
    Shared    = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceError {
    DeviceNotFound,
    ResourceConflict,
    DeviceFault,
    DeviceBusy,
    PermissionDenied,
    OutOfMemory,
    InvalidParameter,
    CapacityExceeded,
}

/// Static 48-byte Device Table Slot.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceSlot {
    pub occupied: bool,
    pub class: DeviceClass,
    pub state: DeviceLifecycleState,
    pub _pad0: u8,
    pub generation: u16,
    pub _pad1: u16,
    pub device_id: DeviceId,
    pub driver_pid: u64,
    pub kernel_object_id: u64,
    pub resource_mask: u64,
    pub bound_event_id: u64,
}

const _: () = assert!(core::mem::size_of::<DeviceSlot>() == 48);
const _: () = assert!(core::mem::align_of::<DeviceSlot>() == 8);

impl DeviceSlot {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            class: DeviceClass::Unknown,
            state: DeviceLifecycleState::Released,
            _pad0: 0,
            generation: 1,
            _pad1: 0,
            device_id: DeviceId(0),
            driver_pid: 0,
            kernel_object_id: 0,
            resource_mask: 0,
            bound_event_id: 0,
        }
    }
}

/// Static 32-byte Device Resource Slot.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceResourceSlot {
    pub occupied: bool,
    pub res_type: ResourceType,
    pub sharing: ResourceSharing,
    pub _pad: u8,
    pub _reserved: u32,
    pub device_id: DeviceId,
    pub base: u64,
    pub size: u64,
}

const _: () = assert!(core::mem::size_of::<DeviceResourceSlot>() == 32);
const _: () = assert!(core::mem::align_of::<DeviceResourceSlot>() == 8);

impl DeviceResourceSlot {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            res_type: ResourceType::Unused,
            sharing: ResourceSharing::Exclusive,
            _pad: 0,
            _reserved: 0,
            device_id: DeviceId(0),
            base: 0,
            size: 0,
        }
    }
}

/// User query structure for SYS_DEV_QUERY (Syscall 17).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceInfo {
    pub device_id: u64,
    pub class: u8,
    pub state: u8,
    pub driver_pid: u64,
    pub resource_count: u32,
    pub _reserved: u32,
}

/// Logical DMA Buffer Descriptor (Tracks multi-frame allocation).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DmaBufferDescriptor {
    pub occupied: bool,
    pub buffer_id: u32,
    pub frame_count: u16,
    pub sharing: ResourceSharing,
    pub device_id: DeviceId,
    pub owning_pid: u64,
    pub phys_base: u64,
    pub user_virt_addr: u64,
    pub frame_indices: [u16; MAX_FRAMES_PER_BUFFER],
}

impl DmaBufferDescriptor {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            buffer_id: 0,
            frame_count: 0,
            sharing: ResourceSharing::Exclusive,
            device_id: DeviceId(0),
            owning_pid: 0,
            phys_base: 0,
            user_virt_addr: 0,
            frame_indices: [0xFFFF; MAX_FRAMES_PER_BUFFER],
        }
    }
}

/// Physical Frame Pin Record.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhysicalFramePinRecord {
    pub occupied: bool,
    pub _pad: u8,
    pub pin_count: u16,
    pub owning_buffer_mask: u32,
    pub phys_addr: u64,
}

impl PhysicalFramePinRecord {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            _pad: 0,
            pin_count: 0,
            owning_buffer_mask: 0,
            phys_addr: 0,
        }
    }
}

/// Bounded Interrupt Binding Slot.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptBinding {
    pub occupied: bool,
    pub binding_id: u16,
    pub _pad: u32,
    pub device_id: DeviceId,
    pub driver_pid: u64,
    pub event_object_id: u64,
    pub interrupt_count: u64,
    pub top_half: Option<fn(vector: u8, device_id: DeviceId)>,
}

impl InterruptBinding {
    pub const fn empty() -> Self {
        Self {
            occupied: false,
            binding_id: 0,
            _pad: 0,
            device_id: DeviceId(0),
            driver_pid: 0,
            event_object_id: 0,
            interrupt_count: 0,
            top_half: None,
        }
    }
}

/// Global State per Interrupt Vector.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IrqLineState {
    pub active_bindings: u8,
    pub sharing: ResourceSharing,
    pub masked: bool,
    pub in_storm: bool,
    pub irq_count_current_tick: u32,
    pub cooldown_ticks_remaining: u32,
    pub total_line_interrupts: u64,
}

impl IrqLineState {
    pub const fn empty() -> Self {
        Self {
            active_bindings: 0,
            sharing: ResourceSharing::Exclusive,
            masked: false,
            in_storm: false,
            irq_count_current_tick: 0,
            cooldown_ticks_remaining: 0,
            total_line_interrupts: 0,
        }
    }
}
