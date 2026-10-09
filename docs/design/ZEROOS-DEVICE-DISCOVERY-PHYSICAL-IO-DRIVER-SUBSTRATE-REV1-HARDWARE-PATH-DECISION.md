# ZEROOS DEV-MODEL-REV1 — HARDWARE PATH DECISION
## Hardware MMIO Access & Architecture Feasibility Study

### Executive Summary & Decision Verdict

| Metric / Item | Status / Decision |
|---|---|
| **Target Milestone** | ZeroOS DEV-MODEL-REV1 Milestone B-A Hardware Path Selection |
| **Kernel Code Modifications** | `0 lines` (`git diff -- kernel/` = 0) |
| **New Syscalls / ABI Changes** | `0` new syscalls, `0` ABI modifications |
| **New Capability Rights** | `0` new capability bits |
| **Selected Hardware Target** | Local APIC Physical MMIO Window (`0xFEE0_0000`, 4 KiB, Offset `0x30` Version Register) |
| **Verifiable Hardware Read** | `core::ptr::read_volatile(0x0000600000000030 as *const u32)` $\to$ `0x00050014` (LAPIC Version 0x14) |
| **Final Decision Status** | **`🟢 EXISTING INTERFACES SUFFICIENT`** |

---

## 1. Verified Platform Inspection

An audit of the QEMU machine configuration (`tools/run_qemu.py`), Stage 3L VMM MMIO mapping router (`kernel/src/dev/mmio.rs`), and capability dispatcher (`kernel/src/syscall/dispatch.rs`) was performed to evaluate physical MMIO hardware targets.

### 1.1 QEMU Hardware Target Analysis

| Candidate Hardware Device | Physical BAR Base | BAR Length | BAR Type | Documented Register & Offset | Expected Hardware Return | Zero Kernel Change Feasibility |
|---|---|---|---|---|---|---|
| **Candidate 1: Local APIC (LAPIC)** | `0xFEE0_0000` | `0x1000` (4 KiB) | Memory-Mapped (MMIO) | Offset `0x30` (LAPIC Version Register) | `0x00050014` (Version `0x14`, Max LVT `0x05`) | **100% FEASIBLE** |
| **Candidate 2: Linear Display Framebuffer** | `0xFD00_0000` | `0x100000` (1 MiB) | Memory-Mapped (MMIO) | Offset `0x00` (Pixel 0 dword) | Framebuffer pixel data | **FEASIBLE** (Requires VGA mode) |
| **Candidate 3: PIIX3 IDE Controller** | `0x1F0-0x1F7` / `0xC000` | `8 bytes` | I/O-Port Space | Offset `0x1F7` (Command Register) | Status byte `0x50` / `0x00` | **INCOMPATIBLE** (Port I/O only) |

---

## 2. Zero-Kernel-Change Feasibility (Candidate 1 — LAPIC MMIO)

### 2.1 Hardware Identity & Register Specification
- **Physical MMIO Base**: `0xFEE0_0000` (4 KiB page aligned).
- **Target Register**: LAPIC Version Register at Offset `0x30` (`0xFEE0_0030`).
- **Hardware Behavior**: LAPIC is a standard CPU MMIO peripheral in x86_64 QEMU architecture (`qemu-system-x86_64`). Reading physical offset `0x30` returns a 32-bit dword containing LAPIC version (`0x14`) in bits 0..7 and Max LVT entries (`0x05`) in bits 16..23, yielding exact hardware value `0x00050014`.

### 2.2 Capability & Mapping Execution Flow
1. **Device Registration**: `kernel/src/dev/registry.rs` registers `LAPIC` device node (`DeviceId(0)` or `BOOTSTRAP_MEM_DEVICE_ID`) with MMIO resource `base: 0xFEE0_0000`, `size: 0x1000`.
2. **Resource Graph Admission**: `resourced` admits the node and issues a `DevCap` handle with `DEV_MAP_MMIO | DEV_RESET` rights to `WorkspaceID(500)`.
3. **Supervisor Spawn**: `driverd` (PID 101) spawns Ring3 driver process (`mmio_driverd`, PID 103) and transfers `DevCap` handle.
4. **Syscall 18 Mapping**: `mmio_driverd` executes `sys_dev_map_mmio(handle, res_idx=0, &mut out_vaddr)`.
5. **Kernel Authorization**: `dispatch_dev_map_mmio` verifies `DEV_MAP_MMIO` capability right, maps physical `0xFEE0_0000` into calling process's page table at virtual address `USER_DEV_MMIO_BASE` (`0x0000_6000_0000_0000`), setting uncacheable PTE flags (`USER | PRESENT | NO_EXECUTE | CACHE_DISABLE | WRITE_THROUGH`).
6. **Volatile Load Execution**: Driver executes:
   ```rust
   let lapic_version_reg = unsafe {
       core::ptr::read_volatile((out_vaddr + 0x30) as *const u32)
   };
   assert_eq!(lapic_version_reg & 0xFF, 0x14);
   ```
7. **Independent Validation**: `lapic_version_reg & 0xFF == 0x14` verifies that the load accessed the actual physical LAPIC MMIO hardware register.

---

## 3. Architecture Amendment Evaluation (Syscall 22 — Optional Future Path)

If dynamic PCI bus scanning over ports `0xCF8`/`0xCFC` from user space is desired in future stages, an architectural amendment is evaluated below for completeness.

### 3.1 Syscall Number & ABI Design
- **Syscall Number**: **22** (`SYS_DEV_PCI_READ`). Syscall number 22 is completely unallocated in `kernel/src/syscall/numbers.rs`.
- **Signature & Pointer-Free Return**:
  ```rust
  // SYS_DEV_PCI_READ (Syscall 22)
  // Parameters: rdi = handle (u32), rsi = pci_bdf (u16), rdx = offset (u8)
  // Returns: positive i64 (32-bit dword value) on success, or negative SyscallError code on failure.
  ```

### 3.2 Capability Authority & Bounds Validation
- **Capability Check**: Checks `INSPECT` (0x0001) or `DEV_MAP_MMIO` (0x0100) right on `KernelObjectType::Device` handle.
- **Bounds Checks**: `pci_bdf` (bus $\le 255$, dev $\le 31$, func $\le 7$), `offset % 4 == 0` and `offset \le 252`.
- **Kernel Port I/O Execution**: Kernel executes `outl(0xCF8, 0x8000_0000 | (bdf << 8) | offset)` followed by `inl(0xCFC)` in Ring 0.

### 3.3 Requirement Assessment for Milestone B-A
- **Verdict**: **NOT REQUIRED FOR MILESTONE B-A**. Candidate 1 (LAPIC MMIO Register `0xFEE0_0000` Offset `0x30`) enables 100% genuine, verifiable hardware MMIO reads with **0 kernel changes**.

---

## 4. Security & Isolation Tradeoffs

- **Workspace Isolation**: `resourced` enforces workspace containment. A driver in `WorkspaceID(600)` attempting to access a device handle owned by `WorkspaceID(500)` is rejected with `Err(PermissionDenied)`.
- **Memory Isolation**: `SYS_DEV_MAP_MMIO` maps physical MMIO pages strictly into `USER_DEV_MMIO_BASE` (`0x0000_6000_0000_0000` PML4 entry 192). `NO_EXECUTE` prevents code execution from MMIO pages.
- **Supervisor Teardown**: When a driver process crashes, `driverd` detects termination, executes `SYS_DEV_RESET` to transition state (`Faulted` $\to$ `Resetting` $\to$ `Ready`), and kernel process teardown automatically unmaps user page table entries.

---

## 5. Recommended Path & Final Decision Status

### Recommendation
Adopt **Candidate 1 (LAPIC Physical MMIO Window `0xFEE0_0000` Offset `0x30`)** for DEV-MODEL-REV1 Milestone B-A:
1. Re-target `deviced` resource registration to physical LAPIC MMIO base `0xFEE0_0000` (4 KiB window).
2. Execute `sys_dev_map_mmio` (Syscall 18) to map physical `0xFEE0_0000` to user virtual `0x0000_6000_0000_0000`.
3. Perform a real volatile load `core::ptr::read_volatile(0x0000_6000_0000_0030 as *const u32)` and assert `(lapic_ver & 0xFF) == 0x14`.
4. Retain 0 kernel changes (`git diff -- kernel/` = 0).

### Final Decision Status

**`🟢 EXISTING INTERFACES SUFFICIENT`**

Stop condition preserved. No code modifications or stage freezes performed. Awaiting review.
