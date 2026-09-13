# ADR-0007: GDT, TSS, and Privilege Boundary Architecture

## Status
**Accepted**

## Date
2026-09-13

## Context & Architectural Problem

Project Zero is designed as a capability-based microkernel where untrusted drivers, user applications, and distributed runtime services execute in Ring 3, separated from the Ring 0 kernel nucleus. 

In x86-64 Long Mode, segmentation is largely vestigial for address translation (base is forced to 0 for CS, DS, ES, SS), but it remains **mandatory for privilege separation (CPL/DPL), 64-bit submode activation, interrupt stack switching (IST), and hardware privilege transitions**.

We must establish a formally defined Global Descriptor Table (GDT) and Task State Segment (TSS) that:
1. Provide valid 64-bit code and data segments for Ring 0 (Supervisor) and Ring 3 (User).
2. Configure a 64-bit Task State Segment (TSS) defining `RSP0` for Ring 3 $\to$ Ring 0 transitions.
3. Configure an independent Interrupt Stack Table (`IST1`) for Double Fault (#DF) handling.
4. Establish named selector constants to avoid magic numbers throughout the kernel.
5. Provide an architectural path toward future Ring 3 user mode entry via `sysretq` / `iretq`.

---

## 1. GDT Descriptor Layout & Bit Invariants

The GDT table is 8-byte aligned and contains 7 slots (56 bytes total), representing 5 architectural descriptors (the 64-bit TSS descriptor spans two contiguous 8-byte slots):

| Index | Selector | Description | Type | DPL | P | L | D/B | G | Hex Value |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **0** | `0x00` | **Null Descriptor** | — | — | 0 | 0 | 0 | 0 | `0x0000000000000000` |
| **1** | `0x08` | **Kernel Code 64-bit** | `0xA` (Exec/Read) | 0 | 1 | 1 | 0 | 1 | `0x00AF9A000000FFFF` |
| **2** | `0x10` | **Kernel Data 64-bit** | `0x2` (Read/Write) | 0 | 1 | 0 | 1 | 1 | `0x00CF92000000FFFF` |
| **3** | `0x1B` | **User Data 64-bit** | `0x2` (Read/Write) | 3 | 1 | 0 | 1 | 1 | `0x00CFF2000000FFFF` |
| **4** | `0x23` | **User Code 64-bit** | `0xA` (Exec/Read) | 3 | 1 | 1 | 0 | 1 | `0x00AFFA000000FFFF` |
| **5** | `0x28` | **TSS Descriptor (Low)** | `0x9` (64-bit TSS) | 0 | 1 | 0 | 0 | 0 | `tss_low` |
| **6** | `0x30` | **TSS Descriptor (High)** | Upper 32-bit Base | — | — | — | — | — | `tss_high` |

### Architectural Invariants:
* **Named Constants**: All selectors are referenced via `KERNEL_CODE_SELECTOR`, `KERNEL_DATA_SELECTOR`, `USER_DATA_SELECTOR`, `USER_CODE_SELECTOR`, and `TSS_SELECTOR`.
* **RPL Encoding**: User selectors encode Requestor Privilege Level 3 (`0x18 | 3 = 0x1B`, `0x20 | 3 = 0x23`).
* **SYSRET Alignment**: Descriptors 2, 3, and 4 are ordered such that User Data (`0x18`) and User Code (`0x20`) match the standard offset arithmetic assumed by the x86-64 `sysret` instruction (`STAR[63:48] + 8` for SS, `STAR[63:48] + 16` for CS).

---

## 2. 64-Bit Task State Segment (TSS) & Stack Architecture

In Long Mode, hardware task switching via TSS is deprecated and unsupported. The TSS serves two critical architectural purposes:
1. **Privilege Transition Kernel Stack (`RSP0`)**: When a hardware interrupt, trap, or CPU exception occurs while the CPU is executing in Ring 3, the CPU hardware automatically loads the stack pointer from `TSS.rsp0`. 
   * **Scope Clarification**: `TSS.rsp0` is *specifically and solely* the stack pointer loaded by x86-64 hardware during interrupt/exception vectoring from Ring 3 to Ring 0. It is **not** the universal mechanism for all future Ring-3 $\to$ Ring-0 transitions.
   * **Syscall Path Distinction**: The future `syscall`/`sysretq` fast system call path bypasses IDT vectoring and does not load `TSS.rsp0`. Instead, `syscall` preserves user `RIP` in `RCX` and `RFLAGS` in `R11`, leaving `RSP` untouched. In Stage 3, the system call entry will use its own architecture (e.g. `swapgs` to obtain a per-CPU scratch pointer and switch to the thread's dedicated kernel syscall stack).
2. **Interrupt Stack Table (`IST1..7`)**: Allows specified IDT gates to switch unconditionally to an independent, dedicated stack regardless of the current stack pointer state.

### Critical Stack Layout & Isolation:
```
  Physical Memory / Identity Map
  ┌────────────────────────────────────────────────────────┐
  │ Normal Kernel Stack (64 KiB)                           │
  │ Bottom: 0x00136000  ───►  Top: 0x00146000 (TSS.RSP0)   │
  ├────────────────────────────────────────────────────────┤
  │ [Safety Gap: 16 KiB Unallocated Memory]                │
  ├────────────────────────────────────────────────────────┤
  │ Double-Fault IST1 Stack (16 KiB)                       │
  │ Bottom: 0x0014A070  ───►  Top: 0x0014E070 (TSS.IST1)   │
  └────────────────────────────────────────────────────────┘
```

* **Independent IST1 Allocation**: Statically allocated 16 KiB array aligned to 16 bytes.
* **Explicit Disjointness Invariant**: Verified that all critical memory regions (Kernel Image, Normal Stack, IST1 Stack, GDT, TSS, IDT, and Page Tables) are strictly disjoint half-open intervals `[start, end)`.
* **Protection Scope**: Provides an independent stack for double-fault handling and reduces the probability that stack corruption prevents the #DF handler from executing.

---

## 3. IDT Integration & Double Fault Structural Verification

* Vector 8 (#DF, Double Fault) in the Interrupt Descriptor Table is explicitly configured with `ist = 1`.
* All other exception handlers use `ist = 0` (retaining the active stack).
* **Structural Verification (No Induced Abort)**: Rather than inducing an unrecoverable double fault, Stage 2B performs structural verification:
  - Asserts `IDT[8].ist == 1`
  - Asserts `TSS.ist1 != 0`
  - Asserts `TSS.ist1 % 16 == 0` (16-byte alignment)
  - Asserts `IST1` range does not overlap the normal kernel stack, GDT, TSS, IDT, page tables, or kernel image.

---

## 4. Privilege Boundary & Ring 3 Strategy

Stage 2B establishes the descriptor tables and Task Register configuration required for privilege separation. However, **executing arbitrary Ring 3 user code is intentionally deferred**.

### Dependencies for Safe Ring 3 Execution:
1. **Isolated Address Spaces**: A user process requires dedicated page tables where kernel pages are flagged with Supervisor (`U/S = 0`) to prevent Ring 3 code from accessing kernel state.
2. **Dedicated User Stack**: User mode execution requires an allocated Ring 3 stack pointer (`RSP3`).
3. **Controlled Entry/Exit Mechanism**:
   * Initial entry: `iretq` stack frame containing `SS (0x1B)`, `RSP (user_stack)`, `RFLAGS (IF=1)`, `CS (0x23)`, `RIP (user_entry)`.
   * Fast kernel entry: `syscall` / `sysretq` with MSRs `IA32_STAR`, `IA32_LSTAR`, and `IA32_FMASK` properly configured, using a per-CPU stack switch mechanism.
4. **Capability Tokens**: Microkernel system call dispatching must validate caller capabilities.

Attempting to jump to Ring 3 in Stage 2B before isolated address spaces and capability tables exist would violate security boundaries and create fragile kernel state.


---

## 5. Verification & Diagnostics

Stage 2B provides runtime telemetry validating:
1. Active selector in Task Register (`TR = 0x0028`).
2. Reloaded segment registers (`CS = 0x0008`, `DS = SS = ES = FS = GS = 0x0010`).
3. Active CPU privilege level (`CPL = 0`).
4. Exact bounds and 16-byte alignment of normal kernel stack and IST1 stack.
5. Regression test suite confirming all Stage 2A CPU exceptions (#BP, #UD, #DE, #PF) continue to function deterministically.
