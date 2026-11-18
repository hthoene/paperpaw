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

The declared profiles are `linux-x86_64-sysv64-le`,
`esp32c3-riscv32imc-ilp32-soft-le` and, since draft 29 (owner decision 142),
`esp32s3-xtensa-lx7-ilp32-windowed-le`. Independent C/Rust compile assertions verify
pointer/function-pointer and entry/plugin sizes, alignment and field offsets.
A native linked C/Rust call probe verifies metadata/context and make/drop exchange.
The X4 C probe uses Clang's freestanding rv32imc/ilp32 target; Rust uses the installed
riscv32imc target, and the esp toolchain's `xtensa-esp32s3-none-elf` for the S3 profile
(where installed). This is layout evidence, not device execution or complete
calling-convention qualification.

### Linked-only profiles (owner decision 172)

Every shell hosts the linked built-ins, so five more profiles are declared for
them: `windows-x86_64-win64-le`, `macos-x86_64-sysv64-le`,
`macos-aarch64-darwinpcs-le`, `android-aarch64-aapcs64-le` (64-bit, entry 32 bytes,
8-aligned) and `web-wasm32-c-le` (32-bit, entry 20 bytes, 4-aligned). No card or store
file loads on these targets: no container, packer or loader is defined for them, and
a shell there admits linked plugins only. Their evidence is Rust compile assertions
of the entry and plugin layouts for each target (`x86_64-pc-windows-msvc`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, `aarch64-linux-android`,
`wasm32-unknown-unknown`).

A linked plugin's metadata names the profile of the target its program is built for,
never one a shell chooses: `paperpaw_native::profile::BUILD_PROFILE` is picked by the
compiler's `cfg` (`None` for a target with no declared profile), the SDK sets the
plugin's generated metadata to it when it links the plugin (`native_plugin!(…, linked
= NAME)`, refused at compile time where there is none), and the SDK asserts that the
profile's entry size is the build's own `PpEntry`. The shell hosts with the same
`BUILD_PROFILE`; admission still compares the two byte for byte. Other targets (AArch64
Linux, 32-bit Arm …) have no profile until a shell needs one.

## The Xtensa profile (X4 Pro, X4 Classic; draft 29)

`esp32s3-xtensa-lx7-ilp32-windowed-le` is the ESP32-S3's (Xtensa LX7), target 9
in a format-5 file (`abi/ppn5.md`). Owner decision 142: the X4 Pro and X4 Classic
keep installing card and store plugins in 1.0.

- **Layout:** 32-bit pointers and `usize`, little-endian; `PpEntry` 20 bytes,
  4-aligned (offsets 0, 4, 6, 8, 16), `PpPlugin` 24 bytes, `u64` 8-aligned:
  the same as the ESP32-C3's, asserted against the generated Rust module
  (`tools/tests/test_abi_entry_layout.py`).
- **Calls:** every function pointer across the boundary (`make`, `drop`, the
  plugin's callbacks, every `PpHost` slot, `PpHeap`) is `extern "C"` of the
  Xtensa **windowed** ABI (`call8`/`callx8`, `entry`, `retw`), which is what
  `xtensa-esp32s3-none-elf` and the firmware use. Arguments go in `a2`-`a7`
  of the callee's window, 64-bit values in an even-odd pair, the rest on the
  stack; results in `a2`/`a3`. No boundary type is a float, so the FPU's
  registers never carry an argument. A plugin built with the call0 ABI is not
  a plugin of this profile: the packer refuses an object whose `.xtensa.info`
  section, where present, says `ABI=1` (call0); without the section the
  target triple decides.
- **Stack:** plugin frames run on the shell's main stack, as on the X4. The
  windowed ABI spills register windows into the callers' frames (the 16-byte
  base save area below each stack pointer), so the frame gate counts them.
  The packer keeps the 2,048-byte frame gate (`FRAME_LIMIT`) for Xtensa too,
  reading each function's frame from its `entry` operand and counting the
  16-byte base save area. The band at the stack's end is the debug
  assistant's region monitor, with a reserve for six windowed frames below
  it, and the packer refuses `call0`/`call4`, writes to `a1` but `entry`'s and
  the instructions that change the interrupt level or window state
  (`abi/heap-word.md` §6, "The ESP32-S3's band").
- **Build:** the S3 plugin build sets `-Z crate-attr=feature(asm_experimental_arch)`
  (the esp compiler is a nightly), so the SDK's heap word stays `global_asm!`
  (a plain static could be folded to its zero or land in writable data), and the
  SDK's heap `trap()` is the Xtensa `ill` instruction, which always raises an
  exception (a `break` needs a debugger).
- **Placement:** the loader reads the image into PSRAM and runs it there
  through the instruction-bus mapping (`CODE_OFFSET`), as interface 6 did on
  the X4 Pro. Code reaches its data, its literals and itself by PC-relative
  instructions (`l32r`, `call8`, branches), which stay right wherever the
  block lands.
- **Relocations:** only 32-bit words that hold an address in the plugin
  (`R_XTENSA_32`: literals, vtables, function pointers, the entry's
  `metadata.ptr` and `make`) are relocated; the packer lists them as format
  5's `relocs`. An instruction that holds an absolute address
  (`R_XTENSA_SLOTn_ALT`), a difference (`R_XTENSA_DIFF*`, `PDIFF*`, `NDIFF*`)
  or an unknown type is refused, as format 4 refused them
  (`tools/sign/src/relocation.rs`).
- **No hot code, no patches:** Xtensa code stays in place, so a target-9 file
  has `hot_at`, `hot_len` and `patches` 0; anything else is refused.
- **Heap word and entry:** `PAPERPAW_HEAP` and `PAPERPAW_ENTRY` as on the X4
  (`abi/heap-word.md`; 8-aligned entry, `entry_bytes` 20).

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
