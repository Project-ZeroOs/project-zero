# ADR-0001: Selection of Operating System Kernel Architecture

## Status
**Accepted**

## Date
2026-09-13

## Context & Evaluation Criteria

Project Zero requires an operating system architecture capable of supporting distributed computing, capability-based security, extreme responsiveness, and multi-device portability. 

Historically, three primary kernel archetypes have dominated OS design:
1. **Monolithic Kernel** (e.g., Linux, BSD)
2. **Microkernel** (e.g., seL4, Minix 3, QNX, Zircon)
3. **Hybrid Kernel** (e.g., Windows NT, macOS XNU)

To make a principled decision grounded in systems engineering rather than dogma, we evaluate these architectures against our eight core criteria:

| Criterion | Weight | Monolithic | Microkernel | Hybrid Kernel |
| :--- | :--- | :--- | :--- | :--- |
| **1. Security & Attack Surface** | Critical | **Poor** (Millions of LoC in Ring 0; driver vulnerability = full exploit) | **Exceptional** (Minimal LoC in Ring 0; capability-enforced boundary) | **Moderate** (Core drivers remain in Ring 0; broad attack surface) |
| **2. Fault Isolation & Reliability** | Critical | **Poor** (Driver crash = kernel panic; no restartability) | **Exceptional** (Crashed driver restarts as unprivileged process) | **Moderate** (Certain subsystem failures crash kernel) |
| **3. Raw IPC & Context-Switch Perf** | High | **Exceptional** (Direct function calls within shared memory space) | **Challenging** (Requires direct register handoff / shared memory) | **High** (Trades isolation for performance) |
| **4. Driver Isolation** | Critical | **None** (Drivers run with full hardware and memory privileges) | **Complete** (Drivers run in Ring 3 with explicit MMIO caps) | **Partial** (User-mode driver frameworks exist, but high-perf drivers stay in Ring 0) |
| **5. Distributed Services Support** | High | **Poor** (Kernel components assume uniform physical memory) | **Native** (Services already communicate via typed IPC messages) | **Moderate** (Requires RPC bridging layers) |
| **6. Maintainability & Modularity** | High | **Moderate** (Subsystems develop complex internal coupling) | **Exceptional** (Strict interfaces; components can be upgraded independently) | **Moderate** (Large, interdependent kernel codebase) |
| **7. AI & Sandbox Integration** | High | **Poor** (Difficult to sandbox complex AI agents inside kernel space) | **Exceptional** (AI engines run as isolated userspace actors with attenuated caps) | **Moderate** (Requires complex eBPF / hypervisor filters) |
| **8. Portability (x86_64, ARM64)** | High | **Moderate** (Extensive arch-specific code across hundreds of drivers) | **Exceptional** (HAL localized to tiny nucleus; drivers mostly portable) | **Moderate** (Large HAL surface across kernel) |

---

## Analysis of Alternatives

### 1. The Monolithic Kernel
* **Pros**: In-kernel function calls eliminate context-switch overhead; rich legacy hardware driver implementations available in the industry.
* **Fatal Flaw for Project Zero**: Monolithic architectures are incompatible with capability-based security and the Personal Compute Fabric. In a monolithic system, device drivers and networking stacks execute with Ring 0 privileges. A single flaw in a Wi-Fi driver exposes the entire user environment. Furthermore, because monolithic components assume shared physical memory, distributing a subsystem across physical machines requires invasive retrofitting.

### 2. The Hybrid Kernel
* **Pros**: Offers a pragmatic balance between driver performance (graphics/storage in Ring 0) and service abstraction.
* **Fatal Flaw for Project Zero**: Hybrid kernels inherit the security fragility of monolithic designs while retaining much of the architectural complexity of microkernels. They do not provide true driver fault isolation: if a GPU or network driver panics in Ring 0, the system crashes.

### 3. The Capability Microkernel
* **Pros**: 
  * The kernel nucleus contains $< 20,000$ lines of code, enabling rigorous verification and minimal vulnerability surface.
  * Fault isolation is absolute: if a GPU driver or network stack crashes, the supervisor restarts it without dropping the active user session.
  * Network transparency: because services interact via typed IPC channels and capability handles, dispatching a call to a local userspace service vs. a remote laptop over the Compute Fabric uses identical abstractions.
* **Mitigating the Performance Challenge**: Modern microkernel engineering has solved the historic 1980s Mach performance deficit through:
  * **Direct Thread Switch (Hand-off scheduling)**: Bypassing scheduler queues during synchronous RPC.
  * **Zero-copy shared memory channels** for large data streams (framebuffers, audio, network buffers).
  * **Register-passed IPC payloads** for small control messages.

---

## Decision

**We select a Microkernel-Inspired Modular Capability Architecture for Project Zero.**

The kernel nucleus will execute in Ring 0 (EL1 on ARM64) and provide only:
1. Low-level virtual memory mapping primitives.
2. Capability table verification and rights attenuation.
3. Thread context switching and preemptive scheduling.
4. Hardware interrupt dispatch to userspace event ports.
5. High-performance synchronous and shared-memory IPC channels.

All filesystems, network protocol stacks, device drivers, compositors, AI engines, and user workspaces will execute in isolated userspace domains (Ring 3 / EL0).

---

## Consequences

### Positive
* Incomparable stability and security: zero kernel panics caused by third-party drivers.
* Natural alignment with the Personal Compute Fabric: services communicate over IPC, making local vs. remote distribution a transparent transport routing decision.
* Clean separation of concerns: kernel engineers can focus purely on primitives, while driver and UI engineers work in standard unprivileged environments.

### Negative & Mitigations
* Requires meticulous optimization of IPC fast-paths: we must benchmark every clock cycle spent in context switches and register preservation.
* Driver development requires custom messaging protocols rather than direct hardware access: mitigated by writing driver frameworks with typed Rust abstractions.
