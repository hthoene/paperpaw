# M1 entry and metadata locator contract

This draft supplies raw generated declarations and compile/link probes. It does
not implement a loader, a safe short-table reader, plugin admission, callback
exclusion, output leases or the runtime adapter registry. Those remain gates.

`abi.def.entry_contract` owns the export names. `PAPERPAW_METADATA` is a global,
default-visible read-only byte object. Its symbol size is exactly the metadata
prefix's `total_bytes`, without a suffix or internal padding. Resolve it through
the container's authoritative symbol/mapping structures, never by scanning for
magic or trusting a section name. Reject duplicates, ambiguity, truncated or
unmapped ranges. `generated/metadata-layout.json` carries the locator descriptor
and framing offsets. The container identifiers remain placement hints.

The generated `PpEntry` is a fixed major-7 entry record: `size`, `abi_major`,
`needs_minor`, a static `PpBytes` metadata view, and `make`. Its exact layout is
part of each target profile. The metadata length and contents must equal the
preflight record byte for byte, and the entry major/minor must agree with that
record, before `make` runs. The metadata already contains target identity,
services, features and entry/export requirements. The ADR's earlier illustrative
compressed identity word is not used. PpEntry itself does not grow within this
contract; the metadata and sized plugin callback table supply extensibility.

The factory signature is:

```
status make(const PpHost *host, void *host_ctx, const PpPlugin **out);
```

The loader supplies the host table and context only as arguments to `make`, on
every platform. There is no global host-pointer slot or writable plugin static,
with one named exception: the optional read-only heap word `PAPERPAW_HEAP`, which
the loader fills before `make` with the address of the context-free heap table
`PpHeap` and nothing else (`abi/heap-word.md`).
The plugin retains these arguments in its owned instance state. The host table
and opaque owner context remain valid through instance destruction and
all associated leases. Only host code casts `host_ctx`; only plugin code casts
plugin state. The plugin never copies a Rust host object through this boundary.
The core's host context must integrate shell and core services explicitly; casting
an arbitrary core context to the present `ShellCtx` is not authorized by this
record definition.

The caller initializes `out` to null. `make` must return a negative status and
leave `out` null on failure, or return `OK` and a non-null plugin-owned sized table
on success. The initial `PpPlugin` prefix is `size`, opaque `state`, `drop(state)`.
Its table storage and state remain valid until the single `drop` call completes;
the host must not read the table after drop, which may free it. Drop is required,
non-reentrant and non-failing, even for a static/stateless instance (use a no-op
callback). Nullable raw function-pointer representations permit validation:
null does not mean that mandatory make/drop may be omitted.

Future dispatch callbacks append to PpPlugin with minor/size checks. No reference
to a larger Rust table may be constructed over shorter storage. Validate readable
prefix bounds and mandatory callbacks before calling. Busy exclusion is per
mutable plugin instance; all callbacks and output leases finish before drop or
module unload. The entry types alone do not enforce these runtime obligations.

The first declared profiles are `linux-x86_64-sysv64-le` and
`esp32c3-riscv32imc-ilp32-soft-le`. Independent C/Rust compile assertions verify
pointer/function-pointer and entry/plugin sizes, alignment and field offsets.
A native linked C/Rust call probe verifies metadata/context and make/drop exchange.
The X4 C probe uses Clang's freestanding rv32imc/ilp32 target; Rust uses the installed
riscv32imc target. This is layout evidence, not device execution or complete
calling-convention qualification. Xtensa, AArch64 and other profiles remain absent
until their assertions and platform contracts are available.

The native fixture host table has unused null slots and is never exposed as a
Rust reference or used to call those slots. It is deliberately a test fixture,
not evidence that such a table is valid for production. Production host tables
must implement unsupported slots as returning ERR_UNSUPPORTED, not null.

## Text drawing effects (slot 14)

The agreed M1 backend uses one existing `FontHandle.draw` call, with a 4,096-byte
input cap. Input validation precedes drawing effects. The actual returned advance
must be finite and nonnegative before success is returned, preserving the existing
mode-dependent result in raster and describing modes. There is no additional
`x + advance` rejection. Output validation can fail after drawing effects; this
operation promises neither atomic rendering nor rollback. It does not add a
prepared shaping stage or promise recoverable allocator failures. Maximum-input
shaping memory remains a qualification gate.

## Linked built-ins

A plugin can also be linked into the program that hosts it (the firmware's
built-ins, an app's) instead of loaded from a file. It is the same plugin: the
same `PpEntry`, `make` and metadata bytes, made by the same SDK macro
(`native_plugin!(P, metadata = M, linked = NAME)`). Only how the host finds it
differs.

- **Form.** The macro defines `pub static NAME: native::Linked` and no
  exported symbol: no `PAPERPAW_ENTRY`, `PAPERPAW_METADATA` or heap word, and
  no global allocator or panic handler (the program's own serve the plugin).
  Several plugins therefore link into one program. The program lists its
  built-ins as Rust statics; the host reads each entry through
  `Linked::entry` (`kernel` feature), never through a cast.
- **Admission is a loaded plugin's.** The host checks the `PpEntry` size, major
  and minor, reads the metadata through the same bounded D1 metadata reader
  (within its maximum), requires it to agree with the entry byte for byte and
  in major/minor, and requires its target identity to equal the running
  program's. Format 2 metadata and `PpEntry` carry no layout fingerprint, and a
  linked plugin needs none: it is compiled in the program's own build. Nothing in the bytes is trusted that would not be for a
  file. Skipped, because they do not apply: the signature, flash placement, the
  symbol lookup above and the image mapping.
- **No image range.** A linked plugin has no single loaded image, so the host
  lends it none (`(0, 0)`): every declaration it keeps is copied, fallibly
  (`ERR_NO_MEMORY`). A shell-supplied list of the program's read-only ranges,
  to keep declarations by reference, would be an additive later step, taken
  only if the retained bytes measured on the X4 ask for it.
- **Commit.** Declarations map into the host's registry all or nothing, as for a
  loaded plugin; a record the host cannot take yet refuses the batch with
  `ERR_UNSUPPORTED`.
- **X4.** Linked built-ins are interface-7 plugins for the X4 activation gate
  (`heap-word.md` section 6): their callbacks run on the same main stack.
