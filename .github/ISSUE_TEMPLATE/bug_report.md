---
name: Bug Report
about: Create a report to help us improve Project Zero
title: '[BUG] '
labels: bug
assignees: ''

---

**Describe the Bug**
A clear and concise description of what the bug is.

**Subsystem Affected**
- [ ] Boot / Multiboot
- [ ] HAL / GDT / IDT / Exceptions
- [ ] Physical Memory Manager (PMM)
- [ ] Virtual Memory Manager (VMM) / Paging
- [ ] Higher-Half Transition / HHDM
- [ ] Threading / Scheduler
- [ ] IPC
- [ ] Other

**To Reproduce**
Steps to reproduce the behavior:
1. Command run (e.g. `python tools/run_qemu.py`)
2. QEMU flags or configuration
3. See error output

**Expected Behavior**
A clear and concise description of what you expected to happen.

**Telemetry & Logs**
If applicable, paste serial output, register dumps, or `#PF` error codes:
```text
(paste logs here)
```

**Environment**
 - OS: [e.g. Windows, Linux]
 - Rust Toolchain version: [e.g. nightly-2024-...]
 - QEMU version: [e.g. 9.x]
 - NASM version:
