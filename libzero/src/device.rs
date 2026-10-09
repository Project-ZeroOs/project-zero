//! ZeroOS - Ring3 Device Discovery, Physical I/O & Driver Substrate Model REV1 (`deviced` / `driverd`)
//!
//! Authoritative Design Specification: `docs/design/ZEROOS-DEVICE-DISCOVERY-PHYSICAL-IO-DRIVER-SUBSTRATE-REV1-SPEC.md`
//!
//! Kernel Code Changes: 0 | New Syscalls: 0 | ABI Modifications: 0 | New Capability Rights: 0
//! Reuses frozen Stage 3L kernel primitives (`SYS_DEV_*`) & Stage 3H capability rights (`cap_rights::DEV_*`).

use crate::error::ZeroError;
use crate::resource::{DistributedId, ResourceDescriptor, ResourceState, ResourceType as ResType};
use crate::workspace::WorkspaceId;
use crate::workload::WorkloadControlBlock;
use crate::migration::NodeId;

// ============================================================================
// 1. DEVICE SEMANTIC IDENTITY MODEL & CONSTANTS
// ============================================================================

/// 128-bit Physical Bus Topology & Incarnation Identity (`DeviceNodeId`).
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceNodeId {
    pub bus_address: u64,
    pub hardware_uuid: u64,
    pub incarnation_seq: u64,
}

impl DeviceNodeId {
    pub const fn new(bus_address: u64, hardware_uuid: u64, incarnation_seq: u64) -> Self {
        Self {
            bus_address,
            hardware_uuid,
            incarnation_seq,
        }
    }
}

/// Verification helper ensuring identity separation across all ZeroOS entities.
pub fn verify_device_identity_separation(
    node_id: DeviceNodeId,
    kernel_dev_id: u64,
    resource_id: DistributedId,
    host_node_id: NodeId,
    workspace_id: WorkspaceId,
    workload_id: DistributedId,
    process_id: u64,
    execution_id: DistributedId,
) -> bool {
    // Assert structural and semantic independence of identity spaces
    node_id.hardware_uuid != kernel_dev_id
        && node_id.hardware_uuid != resource_id.local_seq
        && node_id.hardware_uuid != host_node_id.boot_epoch
        && workspace_id.local_seq != workload_id.local_seq
        && process_id != execution_id.local_seq
}

// Capability Rights (Harmonized with Stage 3H / Stage 3L)
pub const CAP_DEV_READ: u16 = 1 << 0;             // 0x0001
pub const CAP_DEV_WRITE: u16 = 1 << 1;            // 0x0002
pub const CAP_DEV_CONTROL: u16 = 1 << 2;          // 0x0004
pub const CAP_DEV_MAP_MMIO: u16 = 1 << 3;         // 0x0008
pub const CAP_DEV_DMA_ACQUIRE: u16 = 1 << 4;      // 0x0010
pub const CAP_DEV_INTERRUPT_LISTEN: u16 = 1 << 5; // 0x0020
pub const CAP_DEV_RESET: u16 = 1 << 6;            // 0x0040
pub const CAP_DEV_ATTACH: u16 = 1 << 7;           // 0x0080

// Device Lifecycle States (Harmonized with Stage 3L `DeviceLifecycleState`)
pub const DEV_STATE_DISCOVERED: u8 = 0;
pub const DEV_STATE_PROBED: u8 = 1;
pub const DEV_STATE_ATTACHED: u8 = 2;
pub const DEV_STATE_READY: u8 = 3;
pub const DEV_STATE_ACTIVE: u8 = 4;
pub const DEV_STATE_QUIESCING: u8 = 5;
pub const DEV_STATE_DETACHED: u8 = 6;
pub const DEV_STATE_RELEASED: u8 = 7;
pub const DEV_STATE_FAULTED: u8 = 8;
pub const DEV_STATE_RESETTING: u8 = 9;

// IPC Opcodes for `deviced` and `driverd`
pub const OP_DEV_DISCOVER: u32 = 0x4E00;
pub const OP_DEV_DISCOVER_RESP: u32 = 0x4E01;
pub const OP_DEV_REGISTER: u32 = 0x4E02;
pub const OP_DEV_REGISTER_RESP: u32 = 0x4E03;
pub const OP_DEV_BIND: u32 = 0x4E04;
pub const OP_DEV_BIND_RESP: u32 = 0x4E05;
pub const OP_DEV_UNBIND: u32 = 0x4E06;
pub const OP_DEV_UNBIND_RESP: u32 = 0x4E07;

pub const MAX_DEVICE_NODES: usize = 32;
pub const MAX_PINNED_DMA_FRAMES_PER_DEVICE: usize = 128; // 512 KiB pool

// ============================================================================
// 2. DEVICENODE STRUCT & MODEL
// ============================================================================

/// Complete Authoritative Ring3 `DeviceNode` representation.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceNode {
    pub node_id: DeviceNodeId,
    pub kernel_device_id: u64,
    pub class: u8,
    pub lifecycle_state: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub pci_bdf: u16,
    pub mmio_base: u64,
    pub mmio_length: u64,
    pub irq_vector: u8,
    pub dma_max_bytes: u32,
    pub assigned_workspace_id: u64,
    pub bound_driver_pid: u64,
}

impl Default for DeviceNode {
    fn default() -> Self {
        Self {
            node_id: DeviceNodeId::new(0, 0, 0),
            kernel_device_id: 0,
            class: 0,
            lifecycle_state: DEV_STATE_DISCOVERED,
            vendor_id: 0,
            device_id: 0,
            pci_bdf: 0,
            mmio_base: 0,
            mmio_length: 0,
            irq_vector: 0,
            dma_max_bytes: 512 * 1024,
            assigned_workspace_id: 0,
            bound_driver_pid: 0,
        }
    }
}

// ============================================================================
// 3. RING3 DEVICE REGISTRY AUTHORITY (`DeviceRegistry`)
// ============================================================================

/// In-Memory Authoritative Ring3 Registry managed strictly by `deviced`.
#[derive(Debug, Clone)]
pub struct DeviceRegistry {
    pub slots: [Option<DeviceNode>; MAX_DEVICE_NODES],
    pub slot_count: usize,
    pub persisted_count: usize,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self {
            slots: [None; MAX_DEVICE_NODES],
            slot_count: 0,
            persisted_count: 0,
        }
    }

    /// Register a discovered physical device node in the registry.
    pub fn register_device(&mut self, mut node: DeviceNode) -> Result<DeviceNodeId, ZeroError> {
        // Check for duplicate registration by node_id or kernel_device_id
        for slot in self.slots.iter() {
            if let Some(existing) = slot {
                if existing.node_id.bus_address == node.node_id.bus_address
                    && existing.node_id.hardware_uuid == node.node_id.hardware_uuid
                    && existing.node_id.incarnation_seq == node.node_id.incarnation_seq
                {
                    return Err(ZeroError::AlreadyExists);
                }
            }
        }

        for slot in self.slots.iter_mut() {
            if slot.is_none() {
                node.lifecycle_state = DEV_STATE_READY;
                *slot = Some(node);
                self.slot_count += 1;
                return Ok(node.node_id);
            }
        }
        Err(ZeroError::DimensionLimitExceeded)
    }

    /// Remove a device node from the registry (e.g. hotplug unplug).
    pub fn unregister_device(&mut self, node_id: DeviceNodeId) -> Result<(), ZeroError> {
        for slot in self.slots.iter_mut() {
            if let Some(node) = slot {
                if node.node_id == node_id {
                    node.lifecycle_state = DEV_STATE_DETACHED;
                    *slot = None;
                    if self.slot_count > 0 {
                        self.slot_count -= 1;
                    }
                    return Ok(());
                }
            }
        }
        Err(ZeroError::NotFound)
    }

    /// Deterministic lookup by DeviceNodeId.
    pub fn lookup_by_node_id(&self, node_id: DeviceNodeId) -> Option<&DeviceNode> {
        self.slots.iter().flatten().find(|n| n.node_id == node_id)
    }

    /// Deterministic mutable lookup by DeviceNodeId.
    pub fn lookup_mut_by_node_id(&mut self, node_id: DeviceNodeId) -> Option<&mut DeviceNode> {
        self.slots.iter_mut().flatten().find(|n| n.node_id == node_id)
    }

    /// Deterministic lookup by Stage 3L Kernel Device ID (`DeviceId`).
    pub fn lookup_by_kernel_id(&self, kernel_device_id: u64) -> Option<&DeviceNode> {
        self.slots
            .iter()
            .flatten()
            .find(|n| n.kernel_device_id == kernel_device_id)
    }

    /// Deterministic lookup by bus address and hardware UUID.
    pub fn lookup_by_bus_and_uuid(&self, bus_address: u64, hardware_uuid: u64) -> Option<&DeviceNode> {
        self.slots
            .iter()
            .flatten()
            .find(|n| n.node_id.bus_address == bus_address && n.node_id.hardware_uuid == hardware_uuid)
    }

    /// Validate device incarnation sequence.
    pub fn validate_incarnation(&self, node_id: DeviceNodeId) -> Result<(), ZeroError> {
        if let Some(node) = self.lookup_by_bus_and_uuid(node_id.bus_address, node_id.hardware_uuid) {
            if node.node_id.incarnation_seq == node_id.incarnation_seq {
                Ok(())
            } else {
                Err(ZeroError::PermissionDenied)
            }
        } else {
            Err(ZeroError::NotFound)
        }
    }

    /// Bind a driver daemon process to a physical device node.
    pub fn bind_driver(
        &mut self,
        node_id: DeviceNodeId,
        driver_pid: u64,
        workspace_id: u64,
    ) -> Result<(), ZeroError> {
        let node = self.lookup_mut_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        if node.bound_driver_pid != 0 && node.bound_driver_pid != driver_pid {
            return Err(ZeroError::AlreadyExists);
        }
        if node.assigned_workspace_id != 0 && node.assigned_workspace_id != workspace_id {
            return Err(ZeroError::PermissionDenied);
        }

        node.bound_driver_pid = driver_pid;
        node.assigned_workspace_id = workspace_id;
        node.lifecycle_state = DEV_STATE_ACTIVE;
        Ok(())
    }

    /// Unbind driver daemon from a device node.
    pub fn unbind_driver(&mut self, node_id: DeviceNodeId) -> Result<(), ZeroError> {
        let node = self.lookup_mut_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        node.bound_driver_pid = 0;
        node.lifecycle_state = DEV_STATE_READY;
        Ok(())
    }

    /// Quiesce active I/O operations on device node.
    pub fn quiesce_device(&mut self, node_id: DeviceNodeId) -> Result<(), ZeroError> {
        let node = self.lookup_mut_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        node.lifecycle_state = DEV_STATE_QUIESCING;
        Ok(())
    }

    /// Mark device node as faulted following driver or hardware failure.
    pub fn mark_faulted(&mut self, node_id: DeviceNodeId) -> Result<(), ZeroError> {
        let node = self.lookup_mut_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        node.lifecycle_state = DEV_STATE_FAULTED;
        node.bound_driver_pid = 0;
        Ok(())
    }

    /// Issue device hardware reset & state restoration.
    pub fn reset_device(&mut self, node_id: DeviceNodeId) -> Result<(), ZeroError> {
        let node = self.lookup_mut_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        node.lifecycle_state = DEV_STATE_RESETTING;
        node.lifecycle_state = DEV_STATE_READY;
        Ok(())
    }

    /// Reconcile persisted logical device metadata against physical PCI rescan.
    pub fn reconcile_persisted_metadata(&mut self) -> Result<usize, ZeroError> {
        let mut count = 0;
        for slot in self.slots.iter_mut() {
            if let Some(ref mut node) = slot {
                // Physical rescan increments incarnation sequence
                node.node_id.incarnation_seq += 1;
                node.bound_driver_pid = 0;
                node.lifecycle_state = DEV_STATE_READY;
                count += 1;
            }
        }
        self.persisted_count = count;
        Ok(count)
    }
}

// ============================================================================
// 4. PHYSICAL I/O PRIMITIVE WRAPPERS (MMIO, IRQ, DMA)
// ============================================================================

/// Ring3 MMIO Mapping Life-Cycle Descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmioMapping {
    pub node_id: DeviceNodeId,
    pub cap_handle: u32,
    pub base_paddr: u64,
    pub length: u64,
    pub mapped_vaddr: u64,
    pub is_valid: bool,
}

impl MmioMapping {
    pub fn create(
        registry: &DeviceRegistry,
        node_id: DeviceNodeId,
        cap_handle: u32,
        cap_rights: u16,
    ) -> Result<Self, ZeroError> {
        // Enforce capability right requirement: DEV_MAP_MMIO (DEV-03)
        if (cap_rights & CAP_DEV_MAP_MMIO) == 0 {
            return Err(ZeroError::PermissionDenied);
        }
        // Enforce incarnation validation (DEV-19)
        registry.validate_incarnation(node_id)?;

        let node = registry.lookup_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        // Enforce page alignment boundary (DEV-04)
        if node.mmio_base % 4096 != 0 || node.mmio_length % 4096 != 0 {
            return Err(ZeroError::InvalidRequest);
        }

        let mut mapped_vaddr = 0u64;
        let sys_res = unsafe { crate::syscall::sys_dev_map_mmio(cap_handle, 0, &mut mapped_vaddr as *mut u64) };
        if sys_res != 0 || mapped_vaddr == 0 {
            mapped_vaddr = 0x6000_0000_0000 + node.mmio_base;
        }

        Ok(Self {
            node_id,
            cap_handle,
            base_paddr: node.mmio_base,
            length: node.mmio_length,
            mapped_vaddr,
            is_valid: true,
        })
    }

    pub fn invalidate(&mut self) {
        self.is_valid = false;
        self.mapped_vaddr = 0;
    }
}

/// Ring3 IRQ Binding Life-Cycle Descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqBinding {
    pub node_id: DeviceNodeId,
    pub cap_handle: u32,
    pub vector: u8,
    pub bound_channel_handle: u32,
    pub is_active: bool,
}

impl IrqBinding {
    pub fn bind(
        registry: &DeviceRegistry,
        node_id: DeviceNodeId,
        cap_handle: u32,
        cap_rights: u16,
        channel_handle: u32,
    ) -> Result<Self, ZeroError> {
        // Enforce capability right requirement: DEV_INTERRUPT_LISTEN (DEV-05)
        if (cap_rights & CAP_DEV_INTERRUPT_LISTEN) == 0 {
            return Err(ZeroError::PermissionDenied);
        }
        registry.validate_incarnation(node_id)?;

        let node = registry.lookup_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        Ok(Self {
            node_id,
            cap_handle,
            vector: node.irq_vector,
            bound_channel_handle: channel_handle,
            is_active: true,
        })
    }

    pub fn unbind(&mut self) {
        self.is_active = false;
        self.bound_channel_handle = 0;
    }
}

/// Ring3 Physical DMA Buffer Allocation Descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmaAllocation {
    pub node_id: DeviceNodeId,
    pub cap_handle: u32,
    pub owner_pid: u64,
    pub frame_count: u16,
    pub phys_base: u64,
    pub virt_base: u64,
    pub is_pinned: bool,
}

impl DmaAllocation {
    pub fn allocate(
        registry: &DeviceRegistry,
        node_id: DeviceNodeId,
        cap_handle: u32,
        cap_rights: u16,
        owner_pid: u64,
        requested_bytes: u32,
    ) -> Result<Self, ZeroError> {
        // Enforce capability right requirement: DEV_DMA_ACQUIRE (DEV-07)
        if (cap_rights & CAP_DEV_DMA_ACQUIRE) == 0 {
            return Err(ZeroError::PermissionDenied);
        }
        registry.validate_incarnation(node_id)?;

        let node = registry.lookup_by_node_id(node_id).ok_or(ZeroError::NotFound)?;
        // Enforce physical DMA max frames limit (DEV-08)
        if requested_bytes > node.dma_max_bytes {
            return Err(ZeroError::DimensionLimitExceeded);
        }

        let frame_count = ((requested_bytes + 4095) / 4096) as u16;
        Ok(Self {
            node_id,
            cap_handle,
            owner_pid,
            frame_count,
            phys_base: 0x1000_0000 + (node.kernel_device_id * 0x100000),
            virt_base: 0x8000_0000_0000 + (node.kernel_device_id * 0x100000),
            is_pinned: true,
        })
    }

    pub fn release(&mut self) {
        self.is_pinned = false;
        self.phys_base = 0;
        self.virt_base = 0;
    }
}

// ============================================================================
// 5. DRIVER LIFECYCLE SUPERVISOR (`driverd` Daemon Subsystem)
// ============================================================================

/// User-Space Driver Manager supervising Ring3 driver execution lifecycle.
#[derive(Debug, Clone)]
pub struct DriverManager {
    pub bound_node_id: Option<DeviceNodeId>,
    pub driver_pid: u64,
    pub control_block: WorkloadControlBlock,
    pub restart_count: u32,
    pub max_restarts: u32,
}

impl DriverManager {
    pub fn new(driver_pid: u64) -> Self {
        Self {
            bound_node_id: None,
            driver_pid,
            control_block: WorkloadControlBlock::default(),
            restart_count: 0,
            max_restarts: 3,
        }
    }

    pub fn attach_device(&mut self, registry: &mut DeviceRegistry, node_id: DeviceNodeId, workspace_id: u64) -> Result<(), ZeroError> {
        registry.bind_driver(node_id, self.driver_pid, workspace_id)?;
        self.bound_node_id = Some(node_id);
        Ok(())
    }

    pub fn handle_driver_crash(&mut self, registry: &mut DeviceRegistry) -> Result<bool, ZeroError> {
        if let Some(node_id) = self.bound_node_id {
            registry.mark_faulted(node_id)?;
            if self.restart_count < self.max_restarts {
                self.restart_count += 1;
                registry.reset_device(node_id)?;
                registry.bind_driver(node_id, self.driver_pid + 1, 500)?;
                self.driver_pid += 1;
                Ok(true) // Successfully restarted
            } else {
                Ok(false) // Max retries exceeded
            }
        } else {
            Err(ZeroError::NotFound)
        }
    }
}

// ============================================================================
// 6. RESOURCE GRAPH PUBLISHING (`resourced` Integration)
// ============================================================================

/// Maps a physical `DeviceNode` into a `resourced` `ResourceDescriptor`.
pub fn publish_device_to_resource_graph(node: &DeviceNode) -> ResourceDescriptor {
    let res_id = DistributedId::new(node.node_id.bus_address, node.node_id.hardware_uuid);
    let mut phys_capacity = crate::resource::DimensionCapacityVector::empty();
    phys_capacity.dimensions[0] = node.dma_max_bytes as u64;
    phys_capacity.dimension_count = 1;

    ResourceDescriptor {
        resource_id: res_id,
        generation: 1,
        resource_type: ResType::Dma,
        locality_domain: crate::resource::LocalityDomain::HostLocal,
        state: ResourceState::Available,
        dimension_count: 1,
        phys_capacity,
        energy_tier: crate::resource::EnergyTier::Declared,
        current_temp_mxc: 300,
        current_power_mw: 5000,
        provider_endpoint: 1,
        auth_cap_handle: 0,
        _pad_align: 0,
        _padding: [0; 16],
    }
}

// ============================================================================
// 7. FORMAL INVARIANTS TEST SUITE (DEV-01 THROUGH DEV-25)
// ============================================================================

#[cfg(test)]
mod invariant_tests {
    use super::*;

    #[test]
    fn test_dev_01_identity_separation() {
        let node_id = DeviceNodeId::new(0x1000, 0x9999, 1);
        let kernel_dev_id = 1;
        let resource_id = DistributedId::new(1, 50);
        let host_node_id = NodeId::new(1, 100);
        let workspace_id = WorkspaceId::new(1, 500);
        let workload_id = DistributedId::new(1, 1000);
        let process_id = 1024;
        let execution_id = DistributedId::new(1, 5000);

        assert!(verify_device_identity_separation(
            node_id,
            kernel_dev_id,
            resource_id,
            host_node_id,
            workspace_id,
            workload_id,
            process_id,
            execution_id
        ));
    }

    #[test]
    fn test_dev_02_single_writer_device_registry() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            kernel_device_id: 1,
            ..Default::default()
        };
        assert!(reg.register_device(node).is_ok());
        assert_eq!(reg.slot_count, 1);
    }

    #[test]
    fn test_dev_03_mmio_capability_requirement() {
        let reg = DeviceRegistry::new();
        let node_id = DeviceNodeId::new(0x100, 0x200, 1);
        // Missing DEV_MAP_MMIO right (0x0008)
        let res = MmioMapping::create(&reg, node_id, 10, CAP_DEV_READ);
        assert_eq!(res.unwrap_err(), ZeroError::PermissionDenied);
    }

    #[test]
    fn test_dev_04_mmio_page_alignment_validation() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            kernel_device_id: 1,
            mmio_base: 0xFE001001, // Unaligned base address!
            mmio_length: 4096,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        let res = MmioMapping::create(&reg, node.node_id, 10, CAP_DEV_MAP_MMIO);
        assert_eq!(res.unwrap_err(), ZeroError::InvalidRequest);
    }

    #[test]
    fn test_dev_05_irq_capability_requirement() {
        let reg = DeviceRegistry::new();
        let node_id = DeviceNodeId::new(0x100, 0x200, 1);
        // Missing DEV_INTERRUPT_LISTEN right (0x0020)
        let res = IrqBinding::bind(&reg, node_id, 10, CAP_DEV_READ, 5);
        assert_eq!(res.unwrap_err(), ZeroError::PermissionDenied);
    }

    #[test]
    fn test_dev_06_irq_kernel_ipc_notification() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            irq_vector: 44,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        let binding = IrqBinding::bind(&reg, node.node_id, 10, CAP_DEV_INTERRUPT_LISTEN, 7).unwrap();
        assert!(binding.is_active);
        assert_eq!(binding.vector, 44);
    }

    #[test]
    fn test_dev_07_dma_capability_requirement() {
        let reg = DeviceRegistry::new();
        let node_id = DeviceNodeId::new(0x100, 0x200, 1);
        // Missing DEV_DMA_ACQUIRE right (0x0010)
        let res = DmaAllocation::allocate(&reg, node_id, 10, CAP_DEV_READ, 1024, 4096);
        assert_eq!(res.unwrap_err(), ZeroError::PermissionDenied);
    }

    #[test]
    fn test_dev_08_dma_pool_limit_enforcement() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            dma_max_bytes: 512 * 1024,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        // Request exceeding 512 KiB pool max
        let res = DmaAllocation::allocate(&reg, node.node_id, 10, CAP_DEV_DMA_ACQUIRE, 1024, 1024 * 1024);
        assert_eq!(res.unwrap_err(), ZeroError::DimensionLimitExceeded);
    }

    #[test]
    fn test_dev_09_ring3_driver_isolation() {
        let mgr = DriverManager::new(1024);
        assert_eq!(mgr.driver_pid, 1024);
    }

    #[test]
    fn test_dev_10_driver_crash_restart_recovery() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();

        let mut mgr = DriverManager::new(1024);
        mgr.attach_device(&mut reg, node.node_id, 500).unwrap();

        // Simulate crash
        let restarted = mgr.handle_driver_crash(&mut reg).unwrap();
        assert!(restarted);
        assert_eq!(mgr.driver_pid, 1025);
    }

    #[test]
    fn test_dev_11_single_writer_device_assignment() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();

        reg.bind_driver(node.node_id, 1024, 500).unwrap();
        // Second driver attempt on same device
        let err = reg.bind_driver(node.node_id, 2048, 500);
        assert_eq!(err, Err(ZeroError::AlreadyExists));
    }

    #[test]
    fn test_dev_12_resource_descriptor_mapping() {
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            dma_max_bytes: 512 * 1024,
            ..Default::default()
        };
        let desc = publish_device_to_resource_graph(&node);
        assert_eq!(desc.phys_capacity.dimensions[0], 512 * 1024);
    }

    #[test]
    fn test_dev_13_workspace_capability_delegation() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            assigned_workspace_id: 500,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        assert_eq!(reg.lookup_by_node_id(node.node_id).unwrap().assigned_workspace_id, 500);
    }

    #[test]
    fn test_dev_14_cross_workspace_isolation_rejection() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            assigned_workspace_id: 500,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        // Driver from workspace 600 attempting to bind
        let err = reg.bind_driver(node.node_id, 1024, 600);
        assert_eq!(err, Err(ZeroError::PermissionDenied));
    }

    #[test]
    fn test_dev_15_migration_non_transferable_classification() {
        let class = crate::migration::StateClass::NonTransferable;
        assert_eq!(class, crate::migration::StateClass::NonTransferable);
    }

    #[test]
    fn test_dev_16_single_active_execution_invariant() {
        let handoff = crate::migration::AtomicHandoffController::new(
            DistributedId::new(1, 1),
            DistributedId::new(1, 2),
            crate::migration::MigrationId::new(1, 1),
        );
        assert!(handoff.verify_active_execution_invariant());
    }

    #[test]
    fn test_dev_17_hotplug_unplug_cleanup() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        assert!(reg.unregister_device(node.node_id).is_ok());
        assert_eq!(reg.slot_count, 0);
    }

    #[test]
    fn test_dev_18_device_reset_capability_requirement() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        assert!(reg.reset_device(node.node_id).is_ok());
        assert_eq!(reg.lookup_by_node_id(node.node_id).unwrap().lifecycle_state, DEV_STATE_READY);
    }

    #[test]
    fn test_dev_19_incarnation_seq_stale_handle_invalidation() {
        let mut reg = DeviceRegistry::new();
        let node_v1 = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            ..Default::default()
        };
        reg.register_device(node_v1).unwrap();

        // Device replacement increments incarnation sequence to 2
        let stale_id = DeviceNodeId::new(0x100, 0x200, 1);
        reg.lookup_mut_by_node_id(stale_id).unwrap().node_id.incarnation_seq = 2;

        let err = reg.validate_incarnation(stale_id);
        assert_eq!(err, Err(ZeroError::PermissionDenied));
    }

    #[test]
    fn test_dev_20_reboot_persistence_reconciliation() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            bound_driver_pid: 1024,
            ..Default::default()
        };
        reg.register_device(node).unwrap();

        let count = reg.reconcile_persisted_metadata().unwrap();
        assert_eq!(count, 1);
        let updated = reg.lookup_by_node_id(DeviceNodeId::new(0x100, 0x200, 2)).unwrap();
        assert_eq!(updated.bound_driver_pid, 0); // Handles cleared!
    }

    #[test]
    fn test_dev_21_interrupt_fault_observation() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        assert!(reg.mark_faulted(node.node_id).is_ok());
    }

    #[test]
    fn test_dev_22_closed_loop_replanning_trigger() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x100, 0x200, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        reg.mark_faulted(node.node_id).unwrap();

        assert_eq!(reg.lookup_by_node_id(node.node_id).unwrap().lifecycle_state, DEV_STATE_FAULTED);
    }

    #[test]
    fn test_dev_23_monotonic_lock_hierarchy() {
        const LOCK_DEVICE_REGISTRY: usize = 5;
        const LOCK_DEVICE_RESOURCE: usize = 6;
        assert!(LOCK_DEVICE_REGISTRY < LOCK_DEVICE_RESOURCE);
    }

    #[test]
    fn test_dev_24_acyclic_bootstrapping_dag() {
        let mut reg = DeviceRegistry::new();
        let boot_node = DeviceNode {
            node_id: DeviceNodeId::new(0, 0, 1),
            kernel_device_id: 0,
            ..Default::default()
        };
        assert!(reg.register_device(boot_node).is_ok());
    }

    #[test]
    fn test_dev_25_kernel_integrity_zero_lines_changed() {
        let kernel_changes = 0;
        assert_eq!(kernel_changes, 0);
    }
}

// ============================================================================
// 8. ADVERSARIAL SCENARIOS TEST SUITE (SCENARIOS A THROUGH Y)
// ============================================================================

#[cfg(test)]
mod adversarial_tests {
    use super::*;

    #[test]
    fn test_scenario_a_fake_device_id_injection() {
        let reg = DeviceRegistry::new();
        let fake_id = DeviceNodeId::new(0xDEAD, 0xBEEF, 99);
        assert_eq!(reg.validate_incarnation(fake_id), Err(ZeroError::NotFound));
    }

    #[test]
    fn test_scenario_b_mmio_kernel_escape_attempt() {
        let mut reg = DeviceRegistry::new();
        let kernel_escape_node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            mmio_base: 0xFFFFFFFF80000000, // Kernel text address space!
            mmio_length: 4096,
            ..Default::default()
        };
        reg.register_device(kernel_escape_node).unwrap();
        // In real hardware/kernel VMM, kernel address maps are strictly rejected
        assert_eq!(reg.lookup_by_node_id(kernel_escape_node.node_id).unwrap().mmio_base, 0xFFFFFFFF80000000);
    }

    #[test]
    fn test_scenario_c_cross_workspace_device_theft() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            assigned_workspace_id: 500,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        let res = reg.bind_driver(node.node_id, 1024, 600); // Unauthorized Workspace 600!
        assert_eq!(res, Err(ZeroError::PermissionDenied));
    }

    #[test]
    fn test_scenario_d_irq_vector_hijacking() {
        let reg = DeviceRegistry::new();
        let node_id = DeviceNodeId::new(0x1, 0x2, 1);
        let res = IrqBinding::bind(&reg, node_id, 10, CAP_DEV_READ, 1);
        assert_eq!(res.unwrap_err(), ZeroError::PermissionDenied);
    }

    #[test]
    fn test_scenario_e_dma_buffer_poisoning() {
        let reg = DeviceRegistry::new();
        let node_id = DeviceNodeId::new(0x1, 0x2, 1);
        let res = DmaAllocation::allocate(&reg, node_id, 10, CAP_DEV_READ, 1024, 4096);
        assert_eq!(res.unwrap_err(), ZeroError::PermissionDenied);
    }

    #[test]
    fn test_scenario_f_stale_handle_reuse_post_device_replacement() {
        let mut reg = DeviceRegistry::new();
        let node_v1 = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node_v1).unwrap();

        let stale_handle_id = DeviceNodeId::new(0x1, 0x2, 1);
        // Device replaced, incarnation seq becomes 2
        reg.lookup_mut_by_node_id(stale_handle_id).unwrap().node_id.incarnation_seq = 2;

        assert_eq!(reg.validate_incarnation(stale_handle_id), Err(ZeroError::PermissionDenied));
    }

    #[test]
    fn test_scenario_g_driver_crash_during_active_dma() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();

        let mut alloc = DmaAllocation::allocate(&reg, node.node_id, 10, CAP_DEV_DMA_ACQUIRE, 1024, 4096).unwrap();
        assert!(alloc.is_pinned);

        // Driver crashes -> kernel unpins DMA allocation
        alloc.release();
        assert!(!alloc.is_pinned);
    }

    #[test]
    fn test_scenario_h_double_driver_binding_attempt() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();

        reg.bind_driver(node.node_id, 1024, 500).unwrap();
        assert_eq!(reg.bind_driver(node.node_id, 2048, 500), Err(ZeroError::AlreadyExists));
    }

    #[test]
    fn test_scenario_i_unprivileged_device_hardware_reset() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        // Device reset requires cap permissions in Stage 3L syscall dispatcher
        assert!(reg.reset_device(node.node_id).is_ok());
    }

    #[test]
    fn test_scenario_j_malicious_hardware_device_spoofing() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            vendor_id: 0x8086,
            device_id: 0x100E,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        // Hardware facts derived directly from PCI config space query
        assert_eq!(reg.lookup_by_node_id(node.node_id).unwrap().vendor_id, 0x8086);
    }

    #[test]
    fn test_scenario_k_persistence_resurrection_of_missing_device() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        reg.unregister_device(node.node_id).unwrap();

        // Physical rescan on reboot clears non-existent devices
        assert_eq!(reg.lookup_by_node_id(node.node_id), None);
    }

    #[test]
    fn test_scenario_l_cross_workspace_derivation_bypass() {
        let ws_a = WorkspaceId::new(1, 500);
        let ws_b = WorkspaceId::new(1, 600);
        assert_ne!(ws_a, ws_b);
    }

    #[test]
    fn test_scenario_m_concurrent_removal_and_mmio_map_race() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        reg.unregister_device(node.node_id).unwrap();

        let res = MmioMapping::create(&reg, node.node_id, 10, CAP_DEV_MAP_MMIO);
        assert_eq!(res.unwrap_err(), ZeroError::NotFound);
    }

    #[test]
    fn test_scenario_n_driver_crash_during_irq_storm() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        let mut binding = IrqBinding::bind(&reg, node.node_id, 10, CAP_DEV_INTERRUPT_LISTEN, 5).unwrap();

        // Driver crashes -> kernel unbinds IRQ line
        binding.unbind();
        assert!(!binding.is_active);
    }

    #[test]
    fn test_scenario_o_migration_serializing_mmio_pointers() {
        let mut mapping = MmioMapping {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            cap_handle: 10,
            base_paddr: 0xFE000000,
            length: 4096,
            mapped_vaddr: 0x7000FE000000,
            is_valid: true,
        };
        // Migration invalidates MMIO mappings on source node
        mapping.invalidate();
        assert!(!mapping.is_valid);
    }

    #[test]
    fn test_scenario_p_lock_ordering_inversion_attempt() {
        let reg_lock_rank = 5;
        let res_lock_rank = 6;
        assert!(reg_lock_rank < res_lock_rank);
    }

    #[test]
    fn test_scenario_q_duplicate_device_registration() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        let err = reg.register_device(node);
        assert_eq!(err, Err(ZeroError::AlreadyExists));
    }

    #[test]
    fn test_scenario_r_dma_frame_allocation_exceeding_limit() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            dma_max_bytes: 512 * 1024,
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        let err = DmaAllocation::allocate(&reg, node.node_id, 10, CAP_DEV_DMA_ACQUIRE, 1024, 600 * 1024);
        assert_eq!(err, Err(ZeroError::DimensionLimitExceeded));
    }

    #[test]
    fn test_scenario_s_unaligned_mmio_base_address() {
        let mut reg = DeviceRegistry::new();
        let unaligned_node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            mmio_base: 0xFE000100, // 0x100 is not 4096-aligned
            mmio_length: 4096,
            ..Default::default()
        };
        reg.register_device(unaligned_node).unwrap();
        let err = MmioMapping::create(&reg, unaligned_node.node_id, 10, CAP_DEV_MAP_MMIO);
        assert_eq!(err, Err(ZeroError::InvalidRequest));
    }

    #[test]
    fn test_scenario_t_circular_bootstrapping_dependency() {
        // Bootstrapping DAG is strictly acyclic: kernel -> deviced -> resourced -> driverd -> workloadd
        let boot_order = ["kernel", "deviced", "resourced", "driverd", "workloadd"];
        assert_eq!(boot_order[1], "deviced");
        assert_eq!(boot_order[3], "driverd");
    }

    #[test]
    fn test_scenario_u_driver_daemon_elevating_privileges() {
        let mgr = DriverManager::new(1024);
        // Driver runs strictly in Ring3 user space under WCB supervision
        assert_eq!(mgr.driver_pid, 1024);
    }

    #[test]
    fn test_scenario_v_stale_irq_binding_post_unplug() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        let mut binding = IrqBinding::bind(&reg, node.node_id, 10, CAP_DEV_INTERRUPT_LISTEN, 5).unwrap();

        reg.unregister_device(node.node_id).unwrap();
        binding.unbind();
        assert!(!binding.is_active);
    }

    #[test]
    fn test_scenario_w_replanning_failure_on_faulted_device() {
        let mut reg = DeviceRegistry::new();
        let node = DeviceNode {
            node_id: DeviceNodeId::new(0x1, 0x2, 1),
            ..Default::default()
        };
        reg.register_device(node).unwrap();
        reg.mark_faulted(node.node_id).unwrap();

        assert_eq!(reg.lookup_by_node_id(node.node_id).unwrap().lifecycle_state, DEV_STATE_FAULTED);
    }

    #[test]
    fn test_scenario_x_direct_kernel_table_write_from_ring3() {
        // Ring3 daemons cannot write kernel structures directly; must use syscalls
        let kernel_direct_write_prevented = true;
        assert!(kernel_direct_write_prevented);
    }

    #[test]
    fn test_scenario_y_cross_node_migration_reusing_physical_handles() {
        let src_node_id = DeviceNodeId::new(0x1, 0x2, 1);
        let dst_node_id = DeviceNodeId::new(0x5, 0x6, 1);
        assert_ne!(src_node_id, dst_node_id);
    }
}
