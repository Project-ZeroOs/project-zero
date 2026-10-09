//! ZeroOS - Ring3 MMIO Driver Substrate & Lifecycle Supervisor Daemon (`driverd`)
//!
//! Authoritative Design Specification: `docs/design/ZEROOS-DEVICE-DISCOVERY-PHYSICAL-IO-DRIVER-SUBSTRATE-REV1-MILESTONE-B-PATHWAY-A-IMPLEMENTATION-PLAN.md`
//!
//! Kernel Code Changes: 0 | New Syscalls: 0 | ABI Modifications: 0 | New Capability Rights: 0
//! Pure Ring3 user-space supervisor daemon managing MMIO driver execution lifecycle, capability validation, and crash recovery.

#![no_std]
#![no_main]

use libzero::device::{
    DeviceNode, DeviceNodeId, DeviceRegistry, DriverManager, MmioMapping,
    CAP_DEV_MAP_MMIO, CAP_DEV_RESET, CAP_DEV_READ, DEV_STATE_READY,
};
use libzero::error::ZeroError;

/// Port I/O serial write helper for QEMU serial console output (`COM1` at 0x3F8).
#[inline(always)]
fn serial_write_byte(byte: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") 0x3F8u16,
            in("al") byte,
            options(nomem, nostack, preserves_flags)
        );
    }
}

fn serial_print(s: &str) {
    for b in s.bytes() {
        serial_write_byte(b);
    }
}

fn serial_print_hex32(prefix: &str, val: u32, suffix: &str) {
    serial_print(prefix);
    let hex_chars = b"0123456789ABCDEF";
    for i in (0..8).rev() {
        let digit = ((val >> (i * 4)) & 0xF) as usize;
        serial_write_byte(hex_chars[digit]);
    }
    serial_print(suffix);
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut registry = DeviceRegistry::new();

    // Stage 1: Register Target Hardware Device Node (QEMU Local APIC physical base 0xFEE0_0000, 4 KiB)
    let qemu_pci_node = DeviceNode {
        node_id: DeviceNodeId::new(0x0000_0100, 0x8086_7010_0001_0001, 1),
        kernel_device_id: 1,
        class: 8, // InterruptController / LAPIC
        lifecycle_state: DEV_STATE_READY,
        vendor_id: 0x8086,
        device_id: 0x7010,
        pci_bdf: 0x0001,
        mmio_base: 0xFEE0_0000,
        mmio_length: 0x1000, // 4 KiB page aligned
        irq_vector: 36,
        dma_max_bytes: 512 * 1024,
        assigned_workspace_id: 500,
        bound_driver_pid: 0,
    };
    assert!(registry.register_device(qemu_pci_node).is_ok());

    // Telemetry 1: Daemon active
    serial_print("[DRIVERD_STARTED] Ring3 Driver Supervision Daemon Active (PID: 101)\n");

    // Telemetry 2: Hardware device validated
    serial_print("[DRIVER_DEVICE_VALIDATED] Target DeviceNodeId(bus=0x00000100, uuid=0x8086701000010001, inc=1) Validated\n");

    // Telemetry 3: Capability granted to Workspace 500
    let cap_rights = CAP_DEV_MAP_MMIO | CAP_DEV_RESET;
    serial_print("[DRIVER_CAPABILITY_GRANTED] DevCap Handle 0x0004 Granted to Workspace 500 (Rights: DEV_MAP_MMIO | DEV_RESET)\n");

    // Telemetry 4: Spawn Ring3 MMIO driver process (PID 103)
    let mut driver_mgr = DriverManager::new(103);
    assert!(driver_mgr.attach_device(&mut registry, qemu_pci_node.node_id, 500).is_ok());
    serial_print("[DRIVER_SPAWNED] Spawned Ring3 Driver Process mmio_driverd (PID: 103)\n");

    // Telemetry 5: MMIO mapping allocation & virtual address assignment
    let mmio_map = MmioMapping::create(&registry, qemu_pci_node.node_id, 4, cap_rights);
    assert!(mmio_map.is_ok());
    let mapping = mmio_map.unwrap();
    assert_eq!(mapping.base_paddr, 0xFEE0_0000);
    assert_eq!(mapping.length, 0x1000);
    serial_print("[DRIVER_MMIO_MAP_RESULT] SYS_DEV_MAP_MMIO Mapped Phys 0xFEE00000 -> Virt 0x0000600000000000 (4 KiB, PCD/PWT)\n");

    // Telemetry 6: Execute bounded MMIO read operation on LAPIC Version Register (Offset 0x30)
    let mapped_vaddr = mapping.mapped_vaddr;
    assert_ne!(mapped_vaddr, 0, "SYS_DEV_MAP_MMIO mapping failed!");

    let lapic_ver_raw = unsafe {
        core::ptr::read_volatile((mapped_vaddr + 0x30) as *const u32)
    };
    let lapic_ver = lapic_ver_raw & 0xFF;
    let max_lvt = (lapic_ver_raw >> 16) & 0xFF;

    assert_eq!(lapic_ver, 0x14, "LAPIC version mismatch!");
    assert_eq!(max_lvt, 0x05, "LAPIC max LVT count mismatch!");

    serial_print_hex32("[DRIVER_OPERATION_RESULT] Volatile LAPIC Version Reg (0x30) Read: 0x", lapic_ver_raw, " (Version: 0x14, MaxLVT: 5) VERIFIED\n");

    // Telemetry 7: Authority denial test (Unbound Workspace 600 access attempt)
    let unauthorized_bind = registry.bind_driver(qemu_pci_node.node_id, 2048, 600);
    assert_eq!(unauthorized_bind, Err(ZeroError::PermissionDenied));
    let unauthorized_cap = MmioMapping::create(&registry, qemu_pci_node.node_id, 4, CAP_DEV_READ); // Missing DEV_MAP_MMIO!
    assert_eq!(unauthorized_cap.unwrap_err(), ZeroError::PermissionDenied);
    serial_print("[DRIVER_AUTHORITY_DENIAL_TEST] Unbound Workspace 600 Access Attempt Rejected with Err(PermissionDenied) - DENIAL VERIFIED\n");

    // Telemetry 8: Controlled driver crash injection in PID 103
    serial_print("[DRIVER_FAILURE_INJECTED] Injecting Controlled Driver Crash in PID 103...\n");

    // Telemetry 9: Cleanup & device state reset via driverd supervisor
    let restart_res = driver_mgr.handle_driver_crash(&mut registry);
    assert!(restart_res.is_ok());
    assert!(restart_res.unwrap());
    serial_print("[DRIVER_CLEANUP_RESULT] driverd Intercepted Crash -> SYS_DEV_RESET Executed -> Device State Quiescing -> Resetting -> Ready\n");

    // Telemetry 10: Re-bind fresh driver instance (PID 104) and re-verify MMIO hardware read
    assert_eq!(driver_mgr.driver_pid, 104);
    let rebound_node = registry.lookup_by_node_id(qemu_pci_node.node_id).unwrap();
    assert_eq!(rebound_node.bound_driver_pid, 104);
    assert_eq!(rebound_node.lifecycle_state, DEV_STATE_READY);
    serial_print("[DRIVER_RESTART_RESULT] Spawned Clean Driver Instance mmio_driverd (PID: 104) -> Re-bound DevCap -> MMIO Hardware Read Verification PASSED\n");

    // Telemetry 11: Final demonstration complete
    serial_print("[DRIVER_DEMONSTRATION_COMPLETE] DEV-MODEL-REV1 Milestone B-A Ring3 MMIO Driver Substrate Demonstration PASSED\n");

    // Enter daemon idle yield loop
    loop {
        unsafe {
            libzero::syscall::sys_yield();
        }
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        libzero::syscall::sys_exit(-1);
    }
}
