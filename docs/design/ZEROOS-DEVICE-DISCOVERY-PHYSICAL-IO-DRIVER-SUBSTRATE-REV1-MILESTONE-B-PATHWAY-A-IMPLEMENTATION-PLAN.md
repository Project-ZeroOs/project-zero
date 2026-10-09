# ZEROOS DEV-MODEL-REV1 MILESTONE B — PATHWAY A IMPLEMENTATION PLAN
## Ring3 MMIO Driver Substrate & Supervised Hardware I/O Execution

### Executive Summary & Decision Verdict

| Metric / Item | Status / Verdict |
|---|---|
| **Target Stage** | ZeroOS DEV-MODEL-REV1 Milestone B-A (Pathway A: Ring3 MMIO Driver Substrate) |
| **Kernel Code Modifications** | `0 lines` (`git diff -- kernel/` = 0 required) |
| **New Syscalls / ABI Modifications** | `0` new syscalls, `0` ABI changes |
| **New Capability Rights** | `0` new capability bits |
| **Target Device Hardware** | QEMU PCI Physical Memory-Mapped BAR Window (`0xFE000000`, 4 KiB) / Linear Framebuffer MMIO |
| **Supported Syscall Mechanisms** | Stage 3L `SYS_DEV_QUERY` (17) & `SYS_DEV_MAP_MMIO` (18) |
| **Final Decision Status** | **`🟢 READY FOR IMPLEMENTATION`** |

---

## 1. Verified Device & MMIO Feasibility Analysis

Before establishing this implementation plan, an empirical audit of the Stage 3L VMM MMIO subsystem and QEMU hardware configuration was conducted.

```text
                  RING 3 USER APERTURE (PML4 ENTRY 192)
     0x0000_6000_0000_0000 ──► [ 4 KiB MMIO Page Window ]
                                        │
                                        │ Page Table Mapping (PTE: User | Present | CacheDisable | WriteThrough)
                                        ▼
                  PHYSICAL MMIO ADDRESS SPACE
     0x0000_0000_FE00_0000 ──► [ QEMU PCI MMIO BAR Window (4 KiB) ]
```

### 1.1 Kernel MMIO Aperture & VMM Integration
- **Source Reference**: `kernel/src/dev/mmio.rs` (Lines 16-94).
- **Aperture Range**: Dedicated 1 TiB user MMIO aperture `[0x0000_6000_0000_0000, 0x0000_7000_0000_0000)`.
- **Page Table Attributes**: `PRESENT | USER_ACCESSIBLE | NO_EXECUTE | CACHE_DISABLE | WRITE_THROUGH` (+ `WRITABLE`).
- **Syscall Routing**: `SYS_DEV_MAP_MMIO` (Syscall 18 in `kernel/src/syscall/dispatch.rs`) validates handle, checks `DEV_MAP_MMIO` right, allocates virtual MMIO window, and maps physical pages into the active page table.

### 1.2 Target Hardware & Observable Outcome
- **Device Node**: QEMU PIIX3 PCI Controller / MMIO BAR Window (`DeviceId(1)`, Class `Storage`/`BusController`).
- **MMIO Physical Window**: Base `0xFE00_0000`, Size `0x1000` (4 KiB page aligned).
- **Observable Outcome**:
  1. `mmio_driverd` (Ring3 driver process) maps physical `0xFE00_0000` to user virtual `0x0000_6000_0000_0000`.
  2. Driver reads initial hardware signature / registers from virtual MMIO address.
  3. Driver writes structured control block to MMIO window and reads back verification pattern.
  4. Failure injection: `driverd` sends SIGKILL to `mmio_driverd`. Kernel tears down page tables. `driverd` resets device and restarts clean driver process instance (`PID 104`).

---

## 2. Exact Syscall & Capability Flow

```text
  deviced (PID 100)          resourced (PID 102)        driverd (PID 101)          mmio_driverd (PID 103)
         │                          │                          │                             │
         │ 1. Discovers PCI BAR     │                          │                             │
         │    0xFE000000 (4KB)      │                          │                             │
         │                          │                          │                             │
         │ 2. Publishes Node        │                          │                             │
         └─────────────────────────►│                          │                             │
                                    │ 3. Admits & Issues       │                             │
                                    │    DevCap Handle         │                             │
                                    └─────────────────────────►│                             │
                                                               │ 4. Spawns Driver Process    │
                                                               ├────────────────────────────►│
                                                               │                             │ 5. SYS_DEV_MAP_MMIO(DevCap)
                                                               │                             ├──────────────────────────┐
                                                               │                             │                          │ Kernel maps MMIO
                                                               │                             │◄─────────────────────────┘
                                                               │                             │ 6. Executes MMIO Reads/Writes
                                                               │                             │
                                                               │ 7. Controlled Crash Injected│
                                                               ├────────────────────────────X (Process Terminated)
                                                               │
                                                               │ 8. SYS_DEV_RESET & Respawn
                                                               └────────────────────────────► mmio_driverd (PID 104)
```

### Step-by-Step Authority Handshake
1. **Discovery (`deviced`)**: Scans hardware PCI topology and constructs `DeviceNode` (ID `1`, BAR Base `0xFE00_0000`, Size `0x1000`, Class `Storage`).
2. **Resource Admission (`resourced`)**: Registers `ResourceDescriptor` in `ResourceGraph` and derives `DeviceCapabilityHandle` (`DevCap`) with `DEV_MAP_MMIO | DEV_RESET` rights for `WorkspaceID(500)`.
3. **Driver Lifecycle Supervision (`driverd`)**: Spawns Ring3 driver process (`mmio_driverd`, PID 103) and transfers `DevCap` handle via IPC channel.
4. **Hardware MMIO Mapping (`mmio_driverd`)**: Invokes `SYS_DEV_MAP_MMIO(handle, res_idx=0, out_vaddr)`. Kernel verifies `DEV_MAP_MMIO` right, maps physical `0xFE00_0000` to user virtual `0x0000_6000_0000_0000`, and returns success.
5. **Physical Hardware I/O**: `mmio_driverd` performs atomic 32-bit MMIO reads/writes over mapped virtual address.
6. **Fault Recovery**: Simulated crash is triggered in PID 103. `driverd` intercepts process exit, invokes `SYS_DEV_RESET` to transition device state (`Faulted` $\to$ `Resetting` $\to$ `Ready`), and spawns PID 104 to resume operation cleanly.

---

## 3. Subsystem Authority Boundaries & Isolation Invariants

- **`deviced` Authority**: Sole owner of `DeviceRegistry` & `DeviceNodeId` allocations.
- **`resourced` Authority**: Sole owner of `ResourceGraph` & `DevCap` handle issuing.
- **`driverd` Authority**: Sole supervisor of Ring3 driver process lifecycle, hardware handle binding, and crash recovery.
- **`mmio_driverd` Authority**: Executes MMIO reads/writes strictly within mapped virtual MMIO pages authorized by assigned `DevCap` handle.
- **Kernel Authority**: Authoritative gatekeeper of page table isolation, capability rights checks, and process memory reclamation.

---

## 4. Driver Crash Supervision & Cleanup Protocol

When a Ring3 driver process crashes or deadlocks:

```rust
// 1. Process termination intercepted by driverd
let exit_code = driverd.wait_process_exit(driver_pid);

// 2. Step device through lifecycle recovery: Faulted -> Resetting -> Ready
sys_dev_reset(dev_handle, RESET_FLAG_FORCE);

// 3. Kernel automatic cleanup:
//    - User PML4 mappings in 0x0000_6000_0000_0000 unmapped & TLB flushed.
//    - Handle table entries closed.
//    - Driver process slots reclaimed.

// 4. Respawn clean driver instance
let new_pid = driverd.spawn_driver("mmio_driverd");
driverd.transfer_handle(new_pid, dev_handle);
```

---

## 5. Adversarial Failure Scenarios (20 Failure Scenarios)

| # | Adversarial Failure Scenario | Expected Architectural Invariant | Point of Enforcement | Required Empirical Evidence |
|---|---|---|---|---|
| **1** | Unauthorized MMIO map attempt without `DEV_MAP_MMIO` right | Kernel rejects request with `SyscallError::PermissionDenied`. | `dispatch_dev_map_mmio` | Syscall return == `-13` (`PermissionDenied`) |
| **2** | Cross-workspace device handle usage (Workspace 600 accessing Workspace 500 device) | Handle validation rejects request with `IpcError::PermissionDenied`. | `resourced` capability check | Return code == `PermissionDenied` |
| **3** | Driver process attempts to map MMIO base into kernel virtual space (`0xFFFF...`) | `validate_user_range` rejects vaddr outside user aperture. | `kernel/src/dev/mmio.rs` | Return code == `InvalidParameter` |
| **4** | Driver process segfaults during MMIO read/write operation | Kernel traps `#PF`, terminates process, `driverd` cleans up and restarts driver. | IDT Vector 14 (`#PF`) / `driverd` | Serial output: `#PF trapped; driver restarted` |
| **5** | Stale `DevCap` handle used after device node removal | Kernel object table checks generation counter and returns `Err(BadHandle)`. | `KERNEL_OBJECT_TABLE` validation | Return code == `-9` (`BadHandle`) |
| **6** | Duplicate device registration attempt for same physical PCI slot | `deviced` rejects insertion with `ZeroError::AlreadyExists`. | `DeviceRegistry::register_device` | Return code == `AlreadyExists` |
| **7** | Driver process attempts port I/O instruction (`out dx, al`) | CPU generates `#GP(0)` fault; kernel terminates process without system crash. | CPU Hardware / IDT Vector 13 | Serial output: `#GP(0) trapped; driver terminated` |
| **8** | MMIO mapping request with non-page-aligned size (`size` = 1000) | Kernel rejects mapping with `SyscallError::InvalidArgument`. | `map_device_mmio` | Return code == `-22` (`InvalidArgument`) |
| **9** | Concurrent MMIO map requests from multiple driver worker threads | Kernel spinlock (`DEVICE_REGISTRY_LOCK`) prevents race conditions. | Kernel synchronization | 100% thread safety in multithreaded test |
| **10** | Driver process attempts rights amplification via `SYS_CAP_DERIVE` | Monotonic capability derivation rejects child rights exceeding parent rights. | `capability_derive` | Return code == `RightsAmplificationRejected` |
| **11** | Driver process hangs indefinitely during hardware access | `driverd` watchdog timer expires (2000 ms), sends abort, and resets driver. | `driverd` watchdog loop | Serial output: `[DRIVER_IO_TIMEOUT_ABORT]` |
| **12** | Partial page table mapping failure during 16 MiB MMIO window allocation | Kernel rolls back mapped pages, unmapping partial entries cleanly. | `map_device_mmio` rollback | Active page table count before == count after |
| **13** | Non-driver process attempts `SYS_DEV_RESET` on active device | Kernel checks `driver_pid` ownership and returns `PermissionDenied`. | `dispatch_dev_reset` | Return code == `-13` (`PermissionDenied`) |
| **14** | Daemon startup out-of-order (`driverd` starts before `deviced`) | `driverd` retries IPC handshake with exponential backoff until `deviced` is ready. | Ring3 IPC client loop | Serial output: `[DRIVERD_WAITING_DEVICED]` |
| **15** | Driver process crashes while holding active MMIO virtual window | Kernel process teardown automatically unmaps MMIO pages from active page table. | `process_exit` page table reclaim | Memory leak count == 0 |
| **16** | Driver process attempts to map MMIO size exceeding 16 MiB limit | Kernel checks `MAX_MMIO_MAP_SIZE` and returns `SyscallError::InvalidArgument`. | `allocate_mmio_virtual_window` | Return code == `InvalidParameter` |
| **17** | Invalid resource index passed to `SYS_DEV_MAP_MMIO` (`res_idx` = 99) | Kernel checks `resource_mask` bounds and returns `InvalidArgument`. | `dispatch_dev_map_mmio` | Return code == `-22` (`InvalidArgument`) |
| **18** | Driver process attempts to execute code from MMIO mapped virtual page | CPU generates `#PF` with NX fault flag set; process terminated immediately. | Page Table `NO_EXECUTE` bit | Serial output: `#PF NX fault trapped` |
| **19** | Device state machine transition out-of-order (`Discovered` $\to$ `Active`) | `transition_state` rejects invalid state transition with `DeviceError::InvalidParameter`. | `transition_state` validation | Return code == `InvalidParameter` |
| **20** | Accidental kernel file edit attempted during build audit | Git diff check fails build immediately (`git diff -- kernel/` != 0). | Build verification script | Exit code != 0 with audit failure |

---

## 6. Required Build, QEMU & Runtime Evidence

The end-to-end operational verification of Milestone B-A must be demonstrated by building the system binaries and running QEMU to capture the following serial output stream:

```text
[BOOT_COMPLETE] ZeroOS QEMU Multiboot Kernel Initialization Complete
[DEVICED_STARTED] Ring3 Physical Device Discovery Daemon Active (PID: 100)
[RESOURCED_STARTED] Ring3 Resource Graph Daemon Active (PID: 102)
[DRIVERD_STARTED] Ring3 Driver Supervision Daemon Active (PID: 101)
[DEVICE_DISCOVERED] DeviceId(1) PCI MMIO BAR 0xFE000000 (4 KiB) Registered in DeviceRegistry
[RESOURCE_LEASED] DevCap Handle 0x0004 Leased to Workspace 500
[DRIVER_SPAWNED] Spawned Ring3 Driver Process mmio_driverd (PID: 103)
[MMIO_MAPPED_SUCCESS] SYS_DEV_MAP_MMIO Mapped Phys 0xFE000000 -> Virt 0x0000600000000000 (4 KiB, PCD/PWT)
[MMIO_READ_VERIFIED] MMIO Read Header: 0x80867010 (Vendor: 0x8086, Device: 0x7010) PASSED
[MMIO_WRITE_VERIFIED] MMIO Write Pattern 0xDEADBEEF -> Readback PASSED
[UNAUTHORIZED_ACCESS_DENIED] Unbound Process MAP_MMIO Attempt Rejected with Err(PermissionDenied)
[DRIVER_FAULT_INJECTED] Injecting Simulated Driver Crash in PID 103...
[KERNEL_TRAP_HANDLED] Process PID 103 Terminated Cleanly (Exit Code: -1)
[DRIVERD_RECOVERY] driverd Intercepted Crash -> SYS_DEV_RESET Executed -> Spawning Clean Driver...
[DRIVER_RESPAWNED] Spawned Clean Driver Instance mmio_driverd (PID: 104)
[RECOVERY_VERIFIED] Re-bound DevCap -> MMIO Hardware Read Verification PASSED (PID: 104)
[MILESTONE_B_PASSED] ZEROOS DEV-MODEL-REV1 MILESTONE B-A DEMONSTRATION PASSED
```

---

## 7. Frozen-Layer Impact Analysis

- **Kernel Source Code (`kernel/`)**: `0 lines modified` (`git diff -- kernel/` = 0).
- **Syscall Numbers (`numbers.rs`)**: `0 new syscalls` added.
- **Syscall ABI (`abi.rs`)**: `0 ABI changes`.
- **Capability Rights (`cap/types.rs`)**: `0 new capability bits` added.
- **Bootloader (`boot/`)**: `0 lines modified`.

---

## 8. Summary & Actionable Decision Status

This implementation plan establishes that **DEV-MODEL-REV1 Milestone B-A (Ring3 MMIO Driver Substrate & Supervised I/O Execution)** is fully feasible, operationally verifiable in QEMU, and strictly compliant with the zero-kernel-modification constraint.

Final Decision Status:
**`🟢 READY FOR IMPLEMENTATION`**

Awaiting approval to proceed to code implementation of Milestone B-A.
