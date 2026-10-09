# ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 (DEV-MODEL-REV1) OPERATIONAL PLAN & RUNTIME MILESTONE DECISION

```text
ZEROOS DEVICE DISCOVERY, PHYSICAL I/O & DRIVER SUBSTRATE MODEL REV1 (DEV-MODEL-REV1)
POST-FORENSIC GAP CLOSURE & RUNTIME DEMONSTRATION PLAN

DECISION VERDICT: 🟢 READY FOR RUNTIME DEMONSTRATION
TARGET MILESTONE: Milestone A — QEMU Boot & Physical Device Discovery Demonstration
FORENSIC BASELINE: 🟡 FORENSIC VERIFICATION PASSED WITH LIMITATIONS (122/122 Host Tests Passed)

FROZEN-LAYER CHANGES: 0
KERNEL CHANGES: 0
NEW SYSCALLS: 0
NEW ABI: 0
NEW CAPABILITY RIGHTS: 0
ARCHITECTURAL DRIFT: NONE
```

---

## 1. Current Verified Baseline

The independent forensic verification (`docs/design/ZEROOS-DEVICE-DISCOVERY-PHYSICAL-IO-DRIVER-SUBSTRATE-REV1-FORENSIC-VERIFICATION.md`) established:
- **Kernel Integrity**: 0 lines changed in `kernel/src/dev/`. 0 new syscalls, 0 ABI modifications, 0 new capability rights.
- **Ring3 Composition Layer**: `libzero/src/device.rs` fully implements `DeviceNodeId`, `DeviceNode`, `DeviceRegistry`, `MmioMapping`, `IrqBinding`, `DmaAllocation`, and `DriverManager`.
- **Host Test Execution**: 122 out of 122 tests passed under `x86_64-pc-windows-gnu`.
- **Operational Limitation**: All Ring3 capability handle checks, identity separation rules, and state machine transitions have been proven on host unit tests, but real PCI hardware bus enumeration under QEMU runtime execution remains unexecuted.

---

## 2. Remaining Gap Matrix

| Property / Feature | Classification | Gap Closure Target | Required for Milestone A |
|---|---|---|---|
| **1. PCI/PCIe Bus Topology Discovery** | `UNPROVEN` (QEMU Runtime) | QEMU PCI bus scan via `SYS_DEV_QUERY` | Yes (Primary Target) |
| **2. Device Identity Separation & Incarnation** | `HOST-BEHAVIORALLY-TESTED` | `DeviceNodeId` vs `ResourceId` trace in QEMU | Yes |
| **3. MMIO VMM Page Table Mapping** | `HOST-BEHAVIORALLY-TESTED` | `SYS_DEV_MAP_MMIO` syscall in QEMU | No (Milestone B) |
| **4. DMA Frame Pinning & Buffer Pool** | `HOST-BEHAVIORALLY-TESTED` | `SYS_DEV_DMA_ALLOC` syscall in QEMU | No (Milestone B) |
| **5. IRQ Vector Delivery & Channel Routing** | `HOST-BEHAVIORALLY-TESTED` | LAPIC IRQ IPC notification in QEMU | No (Milestone C) |
| **6. Device Hardware Reset & State Quiescence** | `HOST-BEHAVIORALLY-TESTED` | `SYS_DEV_RESET` execution in QEMU | No (Milestone C) |
| **7. Driver Lifecycle & Supervisor Restart** | `HOST-BEHAVIORALLY-TESTED` | `driverd` process restart in QEMU | No (Milestone C) |
| **8. Hotplug Unplug & Dynamic Disappearance** | `HOST-BEHAVIORALLY-TESTED` | QEMU device detach telemetry | No |
| **9. Ring3-to-Kernel Capability Enforcement** | `SOURCE-PROVEN` | `cap_rights::DEV_*` validation in QEMU | Yes |
| **10. Resource Graph Capacity Publishing** | `SOURCE-PROVEN` | `publish_device_to_resource_graph` | Yes |
| **11. Workspace Capability Isolation** | `SOURCE-PROVEN` & `HOST-TESTED` | Workspace envelope check | Yes |
| **12. Migration Handle Invalidation** | `SOURCE-PROVEN` & `HOST-TESTED` | Non-transferable handle rules | No |
| **13. Reboot Rescan & Persistence Recovery** | `HOST-BEHAVIORALLY-TESTED` | Metadata rescan post-reboot | No |
| **14. Daemon Bootstrapping DAG** | `SOURCE-PROVEN` | `deviced` init launch in QEMU | Yes |

---

## 3. Discovered Build & Emulator Facilities

Inspection of `tools/run_qemu.py` and `boot/` reveals existing QEMU infrastructure:
- **Build Toolchain**: NASM assembler for boot code (`boot/boot.asm`, `boot/isr.asm`), Cargo for kernel staticlib, `rust-lld` for 64-bit ELF linking, `objcopy` for Multiboot container creation.
- **QEMU Launcher**: `tools/run_qemu.py` launches QEMU headlessly (`qemu-system-x86_64`) with serial console redirection to stdout and ISA debug exit port (`0xf4`).
- **Emulated Hardware Available**: PCI Root Port, IDE/ATA Controller, LAPIC Timer/PIC, PS/2 Keyboard/Mouse, VGA Framebuffer.

---

## 4. Recommended Runtime Milestone

**RECOMMENDED MILESTONE: Milestone A — QEMU Boot & Physical Device Discovery Demonstration**

### Objective:
Demonstrate end-to-end physical device discovery in QEMU without modifying frozen kernel layers or inventing new syscalls.

### Execution Flow:
1. Boot freestanding ZeroOS image in QEMU via `tools/run_qemu.py`.
2. Initializing `deviced` process in user space.
3. `deviced` invokes Stage 3L `SYS_DEV_QUERY` (Syscall 17) to scan QEMU PCI bus slots.
4. Discover real QEMU hardware device (PCI Vendor ID `0x8086`, Device ID `0x7010` - PIIX3 IDE Controller / BOOTSTRAP_ATA).
5. Construct authoritative Ring3 `DeviceNode` record with `DeviceNodeId` (bus address + hardware UUID + incarnation 1).
6. Publish resource capacity descriptor (`ResourceDescriptor`) to `resourced`.
7. Demonstrate safe handling when querying an unassigned/non-existent PCI slot (returns `Err(NotFound)` / `DeviceNotFound`).
8. Print deterministic serial telemetry output confirming physical discovery success.

---

## 5. Architectural Integrity Statements

- **Kernel Changes**: **0 lines** (`git diff -- kernel/` = 0).
- **Syscall ABI**: No new syscalls added; uses existing Stage 3L `SYS_DEV_QUERY` (17).
- **Capability Rights**: Uses existing `cap_rights::DEV_READ` / `DEV_CONTROL`.
- **Identity Separation**: Preserves $\text{DeviceNodeId} \neq \text{DeviceId} \neq \text{ResourceId} \neq \text{NodeId}$.
- **Authority**: `deviced` retains single-writer authority over `DeviceRegistry`.

---

## 6. Execution & Validation Commands

1. **Freestanding Target Check**:
   ```bash
   cargo check --target x86_64-unknown-none (in libzero)
   ```
2. **QEMU Demonstration Run**:
   ```bash
   python tools/run_qemu.py
   ```

---

## 7. Success & Failure Criteria

- **Success**: QEMU boots, `deviced` discovers PIIX3 PCI device via `SYS_DEV_QUERY`, registers `DeviceNode`, publishes `ResourceDescriptor`, handles non-existent device lookup safely, and exits cleanly via `isa-debug-exit`.
- **Failure**: Kernel crash, invalid handle permissions, duplicate registration error, or panic during device query.

---

## 8. Final Decision

```text
FINAL DECISION VERDICT: 🟢 READY FOR RUNTIME DEMONSTRATION

THE OPERATIONAL PLAN IS APPROVED.
NEXT STAGE: IMPLEMENT AND EXECUTE MILESTONE A (QEMU RUNTIME DEMONSTRATION).
```
