# Contributing to Project Zero

Thank you for your interest in Project Zero.

Project Zero is an experimental operating system engineered from first principles. Because operating systems development requires rigorous precision and correctness, all contributions must adhere to our engineering disciplines and architectural standards.

---

## Core Engineering Principles

1. **Architecture Before Implementation**: Do not write code without an established, approved architectural design. Every major subsystem begins with an Architecture Decision Record (ADR) in `docs/decisions/`.
2. **Read the ADRs**: Before modifying any subsystem, thoroughly read and understand the relevant ADRs and existing documentation.
3. **Make Focused, Atomic Changes**: Avoid sweeping, unrelated refactorings. Keep pull requests focused on a single well-defined problem or feature.
4. **No Fake or Stub Implementations**: Never introduce stub or mock functionality while presenting it as complete or working. Every claim must be backed by concrete implementation.
5. **Keep Claims Proportional to Evidence**: If a feature is experimental or verified only under specific constraints (e.g. QEMU x86-64 single-core), state that explicitly. Do not claim production readiness.
6. **Deterministic Verification**: Every behavioral change must be accompanied by automated unit or integration tests that verify both correctness and invariant maintenance.

---

## Kernel Engineering Workflow

All changes touching Ring 0 or low-level bootstrap code must follow this sequential lifecycle:

```text
Inspect → Design → ADR → Implement → Compile → Test → Runtime Verification → Review → Merge
```

1. **Inspect**: Thoroughly inspect the existing implementation, linker layout, memory map, and CPU register state before making changes.
2. **Design**: Establish the invariants and memory models required.
3. **ADR**: For architectural decisions, draft an ADR in `docs/decisions/`.
4. **Implement**: Write minimal, robust, `no_std` Rust or assembly code adhering strictly to the design.
5. **Compile**: Verify compilation with strict warnings and zero errors (`cargo check`, `cargo build`).
6. **Test**: Run the existing test suite (`python -m unittest discover -s tests`) to ensure zero regressions.
7. **Runtime Verification**: Run QEMU headless verification (`python tools/run_qemu.py`) to confirm hardware-level execution and telemetry.
8. **Review**: Submit a pull request with full telemetry logs, disassembly where appropriate, and clear rationale.

---

## Testing & Verification

Before submitting any code, verify that the complete test suite passes locally:

```powershell
# Run the automated build and QEMU hardware verification harness
python tools/run_qemu.py

# Run the complete test suite across all subsystems
python -m unittest discover -s tests -v
```

All tests must pass cleanly, and the QEMU kernel must shut down deterministically with exit code 33 (via `isa-debug-exit`).

---

## License & Attribution

Project Zero is dual-licensed under the [MIT License](LICENSE-MIT) and [Apache 2.0 License](LICENSE-APACHE). By contributing to Project Zero, you agree that your contributions will be licensed under these dual terms.
