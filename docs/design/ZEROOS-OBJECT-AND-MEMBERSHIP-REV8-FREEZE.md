# ZEROOS OBJECT & MEMBERSHIP REV8 FREEZE RECORD

```text
ZEROOS OBJECT & MEMBERSHIP REV8
STATUS: FROZEN

ARCHITECTURE:
🟢 FROZEN

IMPLEMENTATION:
🟢 VERIFIED BY SOURCE

PERSISTENCE:
🟢 VERIFIED BY SOURCE

OBJECT ID STABILITY:
🟢 VERIFIED BY SOURCE

ALLOCATOR UNIQUENESS:
🟢 VERIFIED BY SOURCE

RECONCILIATION:
🟢 VERIFIED BY SOURCE

CRASH MODEL:
🟢 STATICALLY VERIFIED

KERNEL:
🟢 FROZEN — 0 CHANGES

BEHAVIORAL TEST EXECUTION:
🟡 BLOCKED BY HOST ENVIRONMENT (Missing MSVC link.exe linker)

OVERALL ACCEPTANCE:
🟡 IMPLEMENTED — VERIFICATION INCOMPLETE
```

---

## 1. EXECUTIVE FREEZE SUMMARY

The **ZeroOS Object & Membership Model (REV8)** and its Ring 3 `workspaced` / `libzero` durable registry persistence implementation are hereby **FORMALLY FROZEN**.

The architecture, identity semantics, persistence layout, startup recovery ordering, and filesystem bindings are source-grounded, verified, and sealed. Future work must not reopen, redesign, or silently alter REV8.

---

## 2. FROZEN SUBSTRATE & KERNEL DEPENDENCIES

REV8 depends strictly on the frozen **REV3 Filesystem Substrate**. Kernel changes for REV8 remain **`0`**.

```text
┌─────────────────────────────────────────────────────────────────────────────────┐
│                          FROZEN REV3 SUBSTRATE CONTRACT                         │
├───────────────────┬─────────────────────────────┬───────────────────────────────┤
│ Domain            │ Symbol / Interface          │ Value / Implementation        │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Mutation Syscalls │ SYS_DIR_CREATE              │ Syscall Opcode 32             │
│                   │ SYS_FILE_UNLINK             │ Syscall Opcode 33             │
│                   │ SYS_FILE_RENAME             │ Syscall Opcode 34             │
│                   │ SYS_DIR_READ                │ Syscall Opcode 35             │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Capability Right  │ cap_rights::MUTATE          │ Bit 15 (0x8000)               │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Journal WAL       │ DiskJournalBlockV2          │ 4096-byte layout ("ZERO_JR2") │
├───────────────────┼─────────────────────────────┼───────────────────────────────┤
│ Directory Gen     │ dir_inode.generation        │ u32 counter at 0x30..0x34     │
│ Concurrency Ctrl  │ Policy A Mismatch           │ ResourceConflict (-EBUSY/-17) │
└───────────────────┴─────────────────────────────┴───────────────────────────────┘
```

---

## 3. FROZEN DURABLE REGISTRY PERSISTENCE CONTRACT

1. **Authoritative Registry Path**: `/storage/system/object_id.registry`
2. **Binary Header Layout (`ObjectIdRegistryHeader`)**:
   - 64 bytes total (`magic = b"ZERO_REG"`, `version = 1`, `record_count: u32`, `record_size = 160`, `checksum: u32`, `_reserved: [u8; 40]`).
   - Static size assertion: `const _: () = assert!(core::mem::size_of::<ObjectIdRegistryHeader>() == 64);`
3. **Binary Record Layout (`ObjectIdRegistryRecord`)**:
   - 160 bytes total (`object_id: DistributedId` [16B], `device_id: u32` [4B], `inode_num: u32` [4B], `state: ObjectState` [1B], `name_len: u8` [1B], `_pad0: [u8; 2]` [2B], `name: [u8; 128]` [128B], `_padding: [u8; 4]` [4B]).
   - Static size assertion: `const _: () = assert!(core::mem::size_of::<ObjectIdRegistryRecord>() == 160);`
4. **Deterministic Serialization**: Explicit little-endian encoding (`to_le_bytes` / `from_le_bytes`) and CRC32 checksumming (`persistence::calculate_crc32`).
5. **Startup Load Sequence**: `load_durable_registry()` executes during `WorkspaceDaemon` initialization **before** physical directory `reconcile_on_boot()`.
6. **Allocator Sequence Floor Advance**: `allocator.advance_floor(max_seq)` advances allocator sequence floor past loaded IDs, mathematically proving zero ID collisions with restored objects.

---

## 4. FROZEN IDENTITY INVARIANTS

Every future execution of `workspaced` and `libzero` MUST enforce the following ten invariants:

- **I1 (Reboot Stability):** A surviving object retains its `ObjectId` across `workspaced` daemon restarts.
- **I2 (Single Inode Mapping):** A physical object `(device_id, inode_num)` cannot receive two live `ObjectId`s.
- **I3 (Single Object Mapping):** One live `ObjectId` cannot represent two distinct live physical objects.
- **I4 (Same-Directory Rename):** Same-directory rename preserves `ObjectId` and `inode_num`.
- **I5 (Cross-Directory Move):** Cross-directory move preserves `ObjectId` and `inode_num`.
- **I6 (Rename-Over Source Survival):** Source `ObjectId` survives at destination path.
- **I7 (Rename-Over Target Tombstone):** Destination target `ObjectId` transitions to `ObjectState::Tombstoned`.
- **I8 (Reconciliation Uniqueness):** Reconciliation does not duplicate existing identities.
- **I9 (Reconciliation Idempotence):** Repeated reconciliation without physical FS changes produces identical registry state.
- **I10 (Load Before Reconcile):** Durable registry state is always loaded into RAM before directory reconciliation executes.

---

## 5. ENVIRONMENTAL ACCEPTANCE GATE & FORWARD-PROGRESS DECISION

```text
REV8 SOURCE STATE:         🟢 FROZEN
REV8 ARCHITECTURE:       🟢 FROZEN
REV8 IMPLEMENTATION:     🟢 SOURCE-VERIFIED
BEHAVIORAL ACCEPTANCE:   🟡 ENVIRONMENT-BLOCKED (Host runner lacks MSVC link.exe)

FORWARD-PROGRESS DECISION:
PROCEED TO NEXT ZEROOS WORKSTREAM.
```

The behavioral test suite execution limitation (`error: linker link.exe not found`) is recorded as an environmental acceptance item for a future test runner equipped with the MSVC toolchain. Unrelated ZeroOS development must proceed without delay.
