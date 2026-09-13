## Description

Please summarize the architectural change, the motivation behind it, and any affected subsystems.

## Subsystems Impacted
- [ ] Low-Level Boot (`boot/`)
- [ ] Hardware Abstraction Layer (`kernel/src/hal/`)
- [ ] Memory Management (`kernel/src/mm/`)
- [ ] Threading / Concurrency (`kernel/src/task/`)
- [ ] Inter-Process Communication (`kernel/src/ipc/`)
- [ ] Documentation / ADRs (`docs/`)
- [ ] Test Harness & Tooling (`tests/`, `tools/`)

## Invariants Maintained
- [ ] No regression in PML4 mapping layout (`PML4[0]` absent, `PML4[256]` HHDM, `PML4[511]` Kernel VMA).
- [ ] Kernel VMA $W \oplus X$ permissions intact.
- [ ] Stack guard unmapped.
- [ ] Hardware protections (`CR0.WP`, `EFER.NXE`) preserved.
- [ ] Deterministic QEMU shutdown exit code 33 maintained.

## Verification & Telemetry
Provide evidence of automated verification:
```powershell
python tools/run_qemu.py
python -m unittest discover -s tests -v
```
*(Paste test summary or telemetry log here)*

## Checklist
- [ ] Code compiles cleanly with zero warnings or errors.
- [ ] Relevant ADRs updated or authored in `docs/decisions/`.
- [ ] Automated tests added or updated.
- [ ] No fake or stub implementations introduced.
