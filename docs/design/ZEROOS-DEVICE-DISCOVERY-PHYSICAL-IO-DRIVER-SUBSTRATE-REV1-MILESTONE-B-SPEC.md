# ZEROOS DEV-MODEL-REV1 MILESTONE B SPECIFICATION
## Ring3 Driver Substrate & Hardware I/O Execution

### Executive Summary & Decision Verdict

| Metric / Item | Status / Finding |
|---|---|
| **Specification Target** | ZeroOS DEV-MODEL-REV1 Milestone B (Ring3 Driver Substrate & Hardware I/O Execution) |
| **Mandatory Feasibility Audit** | **HARDWARE I/O AUTHORITY BLOCKER IDENTIFIED** |
| **Ring3 Direct Port I/O** | `UNAUTHORIZED` (`RFLAGS.IOPL` = 0, `TSS.iomap_base` = 0x68 [No IOPB], `#GP(0)` on `inb`/`outb`) |
| **Port I/O Syscalls / Capabilities** | `NON-EXISTENT` (`0` port I/O syscalls, `0` port I/O capability bits in frozen kernel) |
| **QEMU PIIX3 ATA Hardware** | Requires legacy port I/O (`0x1F0-0x1F7`, `0x3F6`) or Bus Master IDE I/O ports |
| **Zero Kernel Modification Constraint** | `git diff -- kernel/` = 0 required by frozen baseline |
| **Final Decision Status** | **`🔴 ARCHITECTURE AMENDMENT REQUIRED`** |

---

## 1. Mandatory Prerequisite — Hardware I/O Authority Feasibility Findings

Before finalizing this specification, an exhaustive forensic inspection of the frozen Stage 3L kernel implementation was performed. The results across all five mandatory prerequisite questions are documented below with authoritative source references.

### 1.1 Ring3 Legacy x86 Port I/O Privilege Configuration
- **Finding**: **UNAUTHORIZED / FEASIBILITY FAILED**.
- **Source Reference**: `kernel/src/hal/arch/x86_64/gdt.rs` (Lines 44-79, 360-370).
- **Technical Analysis**:
  - In x86_64 Long Mode, Ring3 user processes execute at Current Privilege Level 3 (CPL = 3).
  - Executing port I/O instructions (`inb`, `outb`, `inw`, `outw`, `inl`, `outl`) at CPL = 3 requires either `CPL <= RFLAGS.IOPL` (where default `IOPL = 0`) or an explicit bit entry in the Task State Segment (TSS) I/O Permission Bit Map (IOPB).
  - `kernel/src/hal/arch/x86_64/gdt.rs` initializes the TSS with:
    ```rust
    TSS.iomap_base = size_of::<TaskStateSegment>() as u16; // 0x68 (104 bytes)
    ```
  - Because `iomap_base` points beyond the TSS segment limit, **no I/O permission bitmap exists**.
  - Any attempt by a Ring3 process to execute `inb` or `outb` triggers an immediate General Protection Fault (`#GP(0)`).

### 1.2 Existing Syscall or Capability Authorization for ATA Ports
- **Finding**: **NON-EXISTENT / FEASIBILITY FAILED**.
- **Source Reference**: `kernel/src/syscall/dispatch.rs` (Lines 628-986), `kernel/src/syscall/numbers.rs`.
- **Technical Analysis**:
  - The frozen kernel defines five Stage 3L device syscalls:
    1. `SYS_DEV_QUERY` (17): Queries device metadata.
    2. `SYS_DEV_MAP_MMIO` (18): Maps MMIO physical memory pages to user virtual addresses.
    3. `SYS_DEV_DMA_ALLOC` (19): Allocates physical DMA frames and maps to HHDM / user addresses.
    4. `SYS_DEV_RESET` (20): Resets device state machine in the device registry.
    5. `SYS_DEV_BIND_IRQ` (21): Binds hardware IRQ vectors to IPC Event handles.
  - There is **no `SYS_DEV_PORT_IO` syscall** and **no capability right** in `kernel/src/cap/types.rs` for port I/O permission.
  - The kernel ABI provides zero mechanisms to delegate, proxy, or authorize port I/O operations for Ring3 processes.

### 1.3 QEMU PIIX3 ATA MMIO Support
- **Finding**: **UNSUPPORTED BY HARDWARE / FEASIBILITY FAILED**.
- **Source Reference**: `deviced/src/main.rs` (Lines 50-65), QEMU PIIX3 IDE emulation spec.
- **Technical Analysis**:
  - The QEMU PIIX3 IDE controller (`0x8086:0x7010`) is a legacy PCI IDE device.
  - While `deviced` in Milestone A published a synthetic 4 KiB MMIO BAR for structure validation, physical PIIX3 ATA sector reads and writes in QEMU require commands issued to primary command/control I/O ports (`0x1F0-0x1F7`, `0x3F6`) or Bus Master IDE I/O ports.
  - The PIIX3 controller does not support MMIO-based ATA sector transfers on standard QEMU target architectures (`qemu-system-x86_64`).

### 1.4 Device-Query Result & Authority Mechanism
- **Finding**: **VERIFIED OPERATIONAL**.
- **Source Reference**: `kernel/src/syscall/dispatch.rs` (Lines 628-693).
- **Technical Analysis**:
  - `SYS_DEV_QUERY` correctly validates `KernelObjectType::Device` handles, checks `INSPECT` rights, and populates `DeviceInfo` structs.
  - Device identification, handle derivation, and capability object table lookups are fully functional and secure.

### 1.5 Sector Read Execution Under Zero-Kernel-Modification Constraint
- **Finding**: **IMPOSSIBLE WITHOUT KERNEL AMENDMENT**.
- **Source Reference**: `git diff -- kernel/` = 0 constraint.
- **Technical Analysis**:
  - A real physical ATA sector read requires sending `0x20` (READ SECTORS) to command port `0x1F7` and reading words from data port `0x1F0`.
  - Because Ring3 direct port I/O triggers `#GP(0)` and no port I/O syscall exists, a real ATA sector read **cannot be executed** under the zero-kernel-modification constraint.

---

## 2. Architectural Decision & Proposed Pathways

Because the hardware authority does not exist in the frozen kernel baseline, this specification formally issues a status of:

**`🔴 ARCHITECTURE AMENDMENT REQUIRED`**

To resolve this blocker without violating ZeroOS design discipline, two mutually exclusive architectural pathways are defined:

```text
                                              ┌──────────────────────────────────────────────────────────┐
                                              │           HARDWARE I/O AUTHORITY BLOCKER                 │
                                              │ Ring3 cannot perform port I/O; 0 port I/O syscalls exist │
                                              └────────────────────────────┬─────────────────────────────┘
                                                                           │
                                  ┌────────────────────────────────────────┴────────────────────────────────────────┐
                                  ▼                                                                                 ▼
             ┌───────────────────────────────────────────┐                                     ┌───────────────────────────────────────────┐
             │       PATHWAY A (RECOMMENDED)             │                                     │                PATHWAY B                  │
             │   MMIO Framebuffer / RAM-Block Driver     │                                     │     Kernel Architecture Amendment Rev1    │
             │   Zero kernel modifications required      │                                     │     Add SYS_DEV_PORT_IO (Syscall 22)      │
             │   Uses existing SYS_DEV_MAP_MMIO          │                                     │     Controlled port permission checks     │
             └───────────────────────────────────────────┘                                     └───────────────────────────────────────────┘
```

### Pathway A (Recommended — Zero Kernel Modification)
- **Scope**: Re-target Milestone B to drive a **Ring3 MMIO Framebuffer / RAM-Block Device Driver (`ramdisk_driverd`)**.
- **Mechanism**: Uses existing Stage 3L `SYS_DEV_MAP_MMIO` (Syscall 18) and `SYS_DEV_DMA_ALLOC` (Syscall 19).
- **Demonstration**: Ring3 `driverd` maps MMIO display memory or RAM-backed sector blocks, validates `DevCap` handles, executes memory-mapped sector read/write operations, and recovers from driver process crashes.
- **Kernel Changes**: `0 lines` (`git diff -- kernel/` = 0). Preserves 100% of frozen Ring0 binaries.

### Pathway B (Kernel Architecture Amendment Required)
- **Scope**: Formally amend Stage 3L ABI to introduce **Syscall 22: `SYS_DEV_PORT_IO`** or **Ring3 IOPB Delegation**.
- **Mechanism**: `SYS_DEV_PORT_IO(handle, port, size, dir, val_ptr)` checks `DEV_PORT_IO` capability rights and performs sanitized port reads/writes on behalf of authorized Ring3 drivers.
- **Kernel Changes**: Requires explicit approval of Stage 3L Architecture Amendment Rev1.

---

## 3. Architecture & Authority Boundaries

Milestone B maintains strict separation across all five ZeroOS core layers:

```text
HUMAN INTENT DAG
    ↓
PLAN DAG
    ↓
WORKLOAD EXECUTION DAG (driverd supervisor PID 101)
    ↓
CAPABILITY SYSTEM (DevCap handle validation)
    ↓
FABRIC SCHEDULER & RESOURCE GRAPH (resourced PID 102)
    ↓
DEVICE REGISTRY (deviced PID 100)
    ↓
RING3 HARDWARE DRIVER PROCESS (ata_driverd / mmio_driverd PID 103)
```

### 3.1 Subsystem Authority Ownership

| Subsystem | Authority Owned | Single-Writer Invariant |
|---|---|---|
| **`deviced`** | `DeviceRegistry` & `DeviceNodeId` allocation | Only `deviced` can insert/mutate physical device nodes in `DeviceRegistry`. |
| **`resourced`** | `ResourceGraph` & `DevCap` capacity leasing | Only `resourced` can grant or revoke resource leases and issue `DevCap` handles. |
| **`driverd`** | Driver process lifecycle & hardware binding | Only `driverd` can spawn, monitor, bind, and restart Ring3 driver processes. |
| **Driver Process** | Physical MMIO/Port I/O execution | Only the bound driver process holding a valid `DevCap` handle can execute I/O on the device. |
| **Kernel** | Memory page tables & syscall capability gatekeeper | Kernel holds 0 driver code; validates handles and page mapping limits in Ring0. |

---

## 4. Milestone B Detailed Architectural Specification

### 4.1 Driver & Device Lifecycle State Machines

```text
      ┌────────────┐
      │ Discovered │ (deviced discovers hardware)
      └─────┬──────┘
            │
            ▼
      ┌────────────┐
      │  Attached  │ (resourced leases DevCap handle)
      └─────┬──────┘
            │
            ▼
      ┌────────────┐      driverd spawns driver process
      │  Binding   │──────────────────────────────────────┐
      └─────┬──────┘                                      │
            │ Driver process validates DevCap             │
            ▼                                             ▼
      ┌────────────┐                              ┌──────────────┐
      │   Ready    │                              │   Faulted    │ (Driver process crashes)
      └─────┬──────┘                              └──────┬───────┘
            │ I/O request active                         │
            ▼                                            │ driverd cleans handles
      ┌────────────┐                              ┌──────▼───────┐
      │   Active   │                              │  Resetting   │ (driverd restarts process)
      └────────────┘                              └──────┬───────┘
                                                         │
                                                         └────────► Ready
```

### 4.2 Capability Rights & Validation Protocol
1. **Handle Allocation**: `resourced` issues a `KernelObjectType::Device` capability handle to `driverd`.
2. **Right Requirements**:
   - `DEV_MAP_MMIO` (0x0100): Authorizes MMIO window mapping via `SYS_DEV_MAP_MMIO`.
   - `DEV_DMA_ACQUIRE` (0x0200): Authorizes physical DMA frame allocation via `SYS_DEV_DMA_ALLOC`.
   - `DEV_RESET` (0x0400): Authorizes device state reset via `SYS_DEV_RESET`.
   - `DEV_INTERRUPT_LISTEN` (0x0800): Authorizes hardware IRQ vector binding via `SYS_DEV_BIND_IRQ`.
3. **Monotonic Derivation**: Drivers cannot amplify rights. Sub-capabilities derived for worker threads must be equal to or weaker than parent handles (`SYS_CAP_DERIVE`).

### 4.3 Bounded I/O Requests & Timeout Enforcement
- **I/O Request Structure**:
  ```rust
  #[repr(C)]
  pub struct DriverIoRequest {
      pub request_id: u64,
      pub op_type: u8,        // 1 = ReadSector, 2 = WriteSector, 3 = Flush
      pub sector_lba: u64,
      pub sector_count: u32,
      pub buffer_vaddr: u64,
      pub timeout_ms: u32,
  }
  ```
- **Timeout Rule**: Every I/O request issued to a Ring3 driver must specify a maximum execution deadline (default: 2000 ms).
- **Timeout Action**: If a driver fails to complete an I/O request within `timeout_ms`, `driverd` sends an abort signal, terminates the driver process, transitions device state to `Faulted`, and triggers driver restart recovery.

### 4.4 Driver Crash Supervision & Clean Restart
- **Monitoring**: `driverd` holds the process completion handle of the spawned Ring3 driver process.
- **Crash Detection**: When a driver process panics, segfaults, or exits unexpectedly, `driverd` receives a process termination notification.
- **Cleanup Protocol**:
  1. `driverd` invokes `SYS_DEV_RESET` to transition device state to `Quiescing` $\to$ `Resetting`.
  2. Kernel automatically reclaims user page tables, closing stale MMIO mappings and releasing DMA buffer pins owned by the dead PID.
  3. `driverd` spawns a clean driver process instance (e.g., PID 104).
  4. `driverd` re-binds the `DevCap` handle to the new PID and transitions state back to `Ready`.

### 4.5 Workspace Isolation & Device Removal
- **Workspace Boundary**: A device assigned to `WorkspaceID(500)` cannot be accessed by processes executing inside `WorkspaceID(600)`. `resourced` rejects cross-workspace capability handle derivation.
- **Device Removal**: When a device is hot-unplugged or removed, `deviced` marks the `DeviceNode` as `Released`. Any subsequent syscalls on existing `Device` handles return `Err(BadHandle)`.

---

## 5. Adversarial Failure Scenarios (20 Scenarios)

To guarantee architectural resilience, Milestone B specifies exact invariant assertions across 20 concrete failure scenarios:

| # | Adversarial Failure Scenario | Expected Architectural Invariant | Point of Enforcement | Required Empirical Evidence |
|---|---|---|---|---|
| **1** | Unauthorized Ring3 port I/O execution (`out dx, al`) | Process fault (`#GP(0)`) intercepted by kernel IDT; kernel remains stable. | CPU hardware / IDT Vector 13 | Serial log contains `#GP(0) trapped; process terminated` |
| **2** | Workspace boundary violation (PID in Workspace 600 accessing Workspace 500 device) | Handle validation rejects request with `Err(PermissionDenied)`. | `resourced` capability check | `resourced` returns `Err(PermissionDenied)` |
| **3** | Unbound process invoking `SYS_DEV_MAP_MMIO` without `DEV_MAP_MMIO` right | Kernel syscall router rejects request with `SyscallError::PermissionDenied`. | `dispatch_dev_map_mmio` | Return code == `-13` (`PermissionDenied`) |
| **4** | Driver process crash during active sector I/O transfer | `driverd` detects crash, kernel reclaims page tables, device transitions to `Faulted` $\to$ `Resetting` $\to$ `Ready`. | `driverd` process monitor | Serial log contains `[DRIVER_CRASH_RECOVERED]` |
| **5** | Stale `Device` capability handle used after device removal | Syscall router checks generation counter and returns `Err(BadHandle)`. | `KERNEL_OBJECT_TABLE` validation | Return code == `-9` (`BadHandle`) |
| **6** | Double driver registration attempt for same physical PCI BDF | `deviced` rejects registration with `Err(AlreadyExists)`. | `DeviceRegistry::register_device` | Return code == `ZeroError::AlreadyExists` |
| **7** | Driver process requests DMA allocation exceeding physical pool limit (512 KiB) | Kernel PMM / DMA manager rejects request with `SyscallError::OutOfMemory`. | `dispatch_dev_dma_alloc` | Return code == `-12` (`OutOfMemory`) |
| **8** | Malicious driver attempts MMIO map to kernel address space (`0xFFFF_FFFF_8000_0000`) | VMM user aperture check rejects mapping with `SyscallError::InvalidArgument`. | `validate_user_range` / VMM | Return code == `-22` (`InvalidArgument`) |
| **9** | Concurrent MMIO map requests from multiple driver threads | `DEVICE_REGISTRY_LOCK` serializes access; mapping succeeds without data races. | Kernel spinlock synchronization | 100% thread safety in multithreaded test |
| **10** | Driver process attempts rights amplification via `SYS_CAP_DERIVE` | Capability system rejects requested child rights exceeding parent rights. | `capability_derive` | Return code == `IpcError::RightsAmplificationRejected` |
| **11** | Hardware I/O request hangs indefinitely (driver deadlock) | `driverd` watchdog timer expires at 2000 ms, sends abort, and resets driver. | `driverd` watchdog loop | Serial log contains `[DRIVER_IO_TIMEOUT_ABORT]` |
| **12** | Partial DMA frame allocation failure midway through buffer construction | Kernel rolls back already-allocated frames, restoring PMM bitmap state cleanly. | `alloc_dma_buffer` unwind | Free frame count before == free frame count after |
| **13** | Ring3 driver accesses unmapped MMIO virtual memory page | Kernel traps Page Fault (`#PF`), terminates driver process, `driverd` restarts driver. | IDT Vector 14 (`#PF` handler) | Serial log contains `#PF trapped; driver restarted` |
| **14** | Daemon startup out-of-order (`driverd` starts before `deviced`) | `driverd` retries IPC handshake with exponential backoff until `deviced` is active. | Ring3 IPC client logic | Serial log contains `[DRIVERD_WAITING_DEVICED]` |
| **15** | Device reset requested by non-owning process (PID != driver_pid) | Kernel rejects reset with `SyscallError::PermissionDenied`. | `dispatch_dev_reset` | Return code == `-13` (`PermissionDenied`) |
| **16** | Driver process crashes while holding active DMA buffer pins | Kernel process teardown unpins physical frames, returning them to free PMM pool. | `process_exit` cleanup | PMM frame leak count == 0 |
| **17** | Invalid resource index passed to `SYS_DEV_MAP_MMIO` (`res_idx` = 99) | Kernel checks `resource_mask` bounds and returns `SyscallError::InvalidArgument`. | `dispatch_dev_map_mmio` | Return code == `-22` (`InvalidArgument`) |
| **18** | High-frequency IRQ storm generated by faulty peripheral | Kernel IRQ storm detector masks vector, notifies `driverd`, avoiding CPU lockup. | `IrqLineState` storm monitor | Serial log contains `[IRQ_STORM_MASKED]` |
| **19** | QEMU execution with missing backing block disk image | `driverd` detects device query size 0, marks device `Quiescing`, logs diagnostic. | `driverd` initialization | Serial log contains `[DRIVER_NO_MEDIA_PRESENT]` |
| **20** | Accidental kernel source modification attempted during build | Git diff audit fails build immediately (`git diff -- kernel/` != 0). | Build verification script | Exit code != 0 with audit failure |

---

## 6. Runtime Verification & Test Demonstration Plan

To achieve formal verification under **Pathway A (MMIO Framebuffer / RAM-Block Driver)**, the execution harness must capture deterministic serial output in QEMU:

```text
[BOOT_COMPLETE] ZeroOS QEMU Multiboot Kernel Initialization Complete
[DEVICED_STARTED] Ring3 Physical Device Discovery & Registry Daemon Active (PID: 100)
[RESOURCED_STARTED] Ring3 Resource Graph Daemon Active (PID: 102)
[DRIVERD_STARTED] Ring3 Driver Supervision Daemon Active (PID: 101)
[DRIVER_SPAWNED] Spawned Ring3 MMIO Driver Process (PID: 103, DeviceId: 1)
[CAP_VALIDATED] DevCap Handle 0x0004 Validated (DEV_MAP_MMIO | DEV_DMA_ACQUIRE)
[MMIO_MAPPED] SYS_DEV_MAP_MMIO Mapped Phys 0xFE000000 -> Virt 0x00007FFF00000000 (4 KiB)
[SECTOR_READ_SUCCESS] Driver Executed Sector Read (LBA: 0, Bytes: 512, Magic: 0x55AA)
[SECTOR_WRITE_SUCCESS] Driver Executed Sector Write (LBA: 1, Pattern: 0xDEADBEEF)
[SECTOR_VERIFIED] Sector Readback Verification PASSED (LBA: 1 Matches Pattern)
[DRIVER_FAULT_INJECTED] Injecting Simulated Driver Crash in PID 103...
[KERNEL_TRAP] Process PID 103 Terminated (Exit Code: -1)
[DRIVERD_RECOVERY] driverd Detected Crash -> Resetting Device -> Restarting Driver...
[DRIVER_RESPAWNED] Spawned Clean Driver Instance (PID: 104)
[RECOVERY_VERIFIED] Re-bound DevCap -> Sector Read Verification PASSED (PID: 104)
[MILESTONE_B_PASSED] DEV-MODEL-REV1 Milestone B Demonstration PASSED
```

---

## 7. Frozen-Layer Impact Analysis

- **Kernel Source (`kernel/`)**: `0 lines modified` (`git diff -- kernel/` = 0).
- **Syscall Numbers (`numbers.rs`)**: `0 new syscalls` added.
- **Syscall ABI (`abi.rs`)**: `0 ABI changes`.
- **Capability Rights (`cap/types.rs`)**: `0 new capability bits` added.
- **Bootloader & Multiboot (`boot/`)**: `0 lines modified`.

---

## 8. Summary & Next Steps

This specification establishes that while direct Ring3 port I/O against legacy PIIX3 ATA hardware is blocked by frozen kernel security boundaries (`🔴 ARCHITECTURE AMENDMENT REQUIRED`), a complete, highly valuable Ring3 Driver Substrate demonstration can be executed cleanly under **Pathway A (MMIO Framebuffer / RAM-Block Driver Substrate)** with **zero kernel changes**.

Proceed to the independent adversarial specification review in `docs/design/ZEROOS-DEVICE-DISCOVERY-PHYSICAL-IO-DRIVER-SUBSTRATE-REV1-MILESTONE-B-SPEC-REVIEW.md`.
