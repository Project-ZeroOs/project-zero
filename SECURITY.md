# Security Policy

## Experimental Software Notice

Project Zero is an experimental, research operating system currently under active foundational development. 

> [!IMPORTANT]
> **Project Zero is not production-ready software.** It is intended for systems research and architecture evaluation. Do not run Project Zero on production hardware or rely on its security boundaries for untrusted multi-tenant workloads at this stage.

---

## Supported Versions

Only the current `main` branch of Project Zero receives active updates and fixes.

| Branch | Supported |
|---|---|
| `main` | :white_check_mark: |
| Older commits | :x: |

---

## Reporting a Security Vulnerability

If you discover a security vulnerability or critical architectural flaw in Project Zero, please report it responsibly:

1. **GitHub Security Advisory**: Use GitHub's [Private Vulnerability Reporting](https://github.com/Project-ZeroOs/project-zero/security/advisories/new) on the repository.
2. If private advisories are not yet enabled, please open an issue describing the technical mechanism without disclosing exploitable payloads, or contact the project maintainers directly through GitHub.

Please include:
* A detailed description of the vulnerability or flaw.
* Steps to reproduce the issue under QEMU or on bare metal (including configuration and flags).
* Any relevant register dumps, serial logs, or crash telemetry.
* Your assessment of the impact.

We will review the issue promptly and work on an architectural fix or patch.
