# 0034. The stable plugin boundary: one flat C table (interface 7)

- Status: Proposed (design review pending; nothing implemented, no interface bump merged)
- Date: 2026-11-05
- Ticket: #228 (owner decisions 119, 121, 122)
- Supersedes: the Rust-shaped crossing of [ADR 0024](0024-native-code-plugins.md)
  (the "Required before 1.0" section is carried out here) and the fingerprint,
  compiler pin and shape gate of [ADR 0026](0026-plugin-interface-version.md).
  ADR 0024's file placement, recovery and store parts stay.

## Context

Today a code plugin and the firmware exchange **Rust values**: `Env`, `Canvas`,
`Registrar`, `Command`, `Box<dyn Screen>`, boxed closures, `Cow<'static, str>`,
`TypeId`-keyed services, and 157 "shared functions" called with Rust's calling
convention (`paperpaw-table`). Only the firmware's `Api` table (allocator, panic,
clock, icon, build string) is C-shaped. So a plugin binary is bound to the exact
compiler (ADR 0026 pins it in every file), to the layouts of about 60 crossing
types (`native::LAYOUT`), and to the order of 12 trait vtables. Two machines keep
that honest: 19 crates with `api.txt` and `shape.txt` baselines, and an allowlist
of three "growing" traits.

That machinery works, and it has just been hardened (#672: successive additions,
a corpus of mutations, an end-to-end check with a plugin built at the last raise).
But it can only promise "old plugins keep running while the compiler stays and
nothing but additions happen". It cannot promise what ADR 0024 asked for before
1.0 and what the owner decided on 2026-11-05 (decision 122): **a boundary that
survives compiler upgrades, with plugins that keep running after additions.**
PaperPaw has almost no users now, so this is the cheapest moment there will be.

Measured on the prototype (`fw/flat-proto-228`, notes
`paperpaw/investigations/2026-11-05-flat-boundary-cost-228.md`), original X4:

| | Rust-shaped now | flat table |
|---|---|---|
| firmware flash | | about +40 KB likely for the whole API (23 to 80 KB) |
| static RAM, heap, stack | | +8 B .data, 0 heap, +16 to +80 B stack |
| plugin file | | about +0.3 to 1 KB per plugin |
| fill_rect, draw_line, measure, draw_text on the device | | ratio 1.005, 1.000, 1.015, 1.014 (3 boots) |
| 150-call widget on the device | 1,803 us | 2,006 us (+11 %) |
| `deliver` (event in, command out) on the device | 4.7 us | 6.4 us (+36 %) |

The two larger numbers are the prototype's own doing: it passed text sizes as
1/256 fixed point (converted with soft float on a chip without an FPU) and built
a `String` per notice. The design below passes `f32` as a C float and keeps
conversions out of the hot path, and the gates (below) make the first real
measurement decide, not this table.

## Decision

### 1. One boundary, described once

Everything that crosses between firmware and plugin is declared in one text file,
`abi/abi.def` (grammar below), and nowhere else. `tools/abi-gen.py` writes from
it, and CI checks the written files are current:

- `crates/paperpaw-abi/src/generated/` (`no_std`; the `unsafe` of the raw
  pointers lives in this generated glue, reviewed once, not in plugins): the
  `#[repr(C)]` tables and structs, constants, error codes, and the wire
  encoder and decoder of every record;
- the **host adapter trait** `HostApi` (the kernel implements it once, and the
  generated `HOST: PpHost` static points its entries at it), and the **plugin
  stubs** (`paperpaw_abi::host::fill_rect(...)`, thin `#[inline]` calls through the
  table pointer);
- `docs/abi.md` (the reference for authors and the Workshop), `abi/abi.json`
  (entries, records, constants and sizes for the site, the Workshop, the signer
  and test fixtures to read, so no tool parses Rust or hard-codes offsets again),
  and `abi/abi.lock` (the baseline, section 8).

The description has three kinds of declaration:

```
since 1                                     # the minor version the lines below belong to
fn canvas.fill_rect(cv: Canvas, x: i32, y: i32, w: u32, h: u32, color: u32)
fn text.measure(font: Font, size: f32, text: str) -> f32
fn i18n.get(env: Env, key: str, out: buf) -> len      # snprintf rule, see 3
callback screen.on_key(screen, key: u32, flags: u32) -> u32
record Command.Notify { text: str, ttl_ms: u32? }     # wire record, `?` optional
```

`fn` is a host table entry (plugin calls firmware), `callback` is an entry in a
plugin table (firmware calls plugin), `record` is a wire record (section 5).
Hand-written code is limited to what the description cannot say: the kernel's
adapters for redesigned shapes (section 3), the SDK facade over the stubs
(section 7), and tests.

### 2. The wire of calls: plain C only

- `extern "C"`, `#[repr(C)]`, the C ABI of the target. Allowed: `u8` to `u64` and
  `i8` to `i64`, `f32` as C `float` (the ESP32-C3's ilp32 soft-float ABI passes
  it in integer registers: no conversion, no cost; `f64` is not allowed), `bool` as
  `u8`, pointers, `usize` for lengths only (inside structs always `u32`, so a
  64-bit host and a 32-bit chip agree on every struct), small `#[repr(C)]` value
  structs (`Rect`, `Point`, `LineMetrics`, `Color` as `0x00RRGGBB`), enums as
  fixed-width integers with **open sets** (an unknown value is handled, never
  undefined).
- **No Rust type crosses**: no `String`, `Vec`, `Option`, `Result`, `Cow`, slice
  reference, trait object, closure, `TypeId`, or enum with data. A function never
  unwinds across (`panic = "abort"` in plugins; the firmware's `extern "C"` calls
  abort on a panic, as Rust does for `extern "C"` since 1.81).
- **Results:** `i32`. `>= 0` is the value or a length, `< 0` an error from the
  open set in `abi.def` (`ERR_UNSUPPORTED -1`, `ERR_INVALID -2`, `ERR_NO_SPACE -3`,
  `ERR_NO_MEMORY -4`, `ERR_UTF8 -5`, `ERR_STATE -6`, `ERR_CANCELLED -7`,
  `ERR_IO -8`; a negative value this build does not know is the general failure).
- **Strings** in: `(ptr, len)`, UTF-8, borrowed for the call. Out: a caller-provided
  buffer `(out, cap)` and the **snprintf rule**: the call returns the length the
  full result needs, writes at most `cap` bytes, and the caller calls again with a
  larger buffer if `need > cap`. A result never longer than its input
  (`unescape`, `decode`, `attrs_get`) needs one call. Nothing owned crosses, so
  nothing is freed by the other side.
- **Objects the firmware owns** are **handles**. A handle valid only during one
  callback (`Env`, `Canvas`, `Cx`: the `ctx` the firmware passes to every callback)
  is a pointer; using it after the callback returns is a plugin bug, checked in
  debug builds by a generation word at its start. A handle that outlives a call
  (`Font`, `Catalog`, `Settings` snapshots, a stored font pack) is a `u32` (slot
  and generation), explicitly released; an unloaded or stale one fails with
  `ERR_STATE`, it does not dangle.
- **Threads and reentrancy:** one thread. Host functions are called only from
  inside a plugin callback (or its registration), on the kernel's thread. A host
  function may call back into the same plugin (a visitor, a timer already queued);
  it never calls a plugin callback while holding a borrow the plugin could
  invalidate. Anything slow is a command (section 5), not a blocking call.
- **Heap:** the plugin's global allocator calls the host's `alloc`, `dealloc` and
  `realloc` (one heap on a 256 KiB chip, as now). The contract is still that
  *nothing allocated by one side is freed by the other*; the shared heap is an
  economy, not something the ABI relies on.

### 3. The host table `PpHost` and the redesign of the awkward shapes

`PpHost` is a single `#[repr(C)]` struct of function pointers. Its first words are
`size: u32`, `major: u16`, `minor: u16`. **Entries are only appended; each
carries the minor it came with.** An entry the device cannot do (Bluetooth on an
X4 without the radio) is a stub returning `ERR_UNSUPPORTED`, never `NULL`. The
eight "families", their revisions, `OLDEST`, the planned chain and the nine
words of the linker script are deleted: one table, one pointer
(`PAPERPAW_HOST`, filled by the loader).

How the 157 shared functions and the rest of what crosses map (details per entry
in the notes: `paperpaw/design/flat-boundary-228/table-inventory.md`):

| Class (of 157) | Count | Treatment |
|---|---|---|
| A: scalars, `&str`, slices | 7 | direct entries |
| B: handles and small value structs | 102 | direct entries; `Rect`/`Point`/`LineMetrics`/`Header`/`WidgetLook`/`ListView` as flat structs, `Font`/`GlyphCache`/`Env`/`Settings`/`Translator`/parsers as handles |
| C: owned or allocating results | 14 | out-buffer (snprintf rule) or a returned handle |
| D: closures, generics, `dyn` | 8 | callback + `user: *mut c_void`; no closure crosses |
| E: must be redesigned | 26 | redesigned for the table, below |

(A and B are the 109 mechanical ones; the numbers are the inventory's, finer than
the first estimate of 9 and 100 scalar-like, 38 owned and 10 closures.)

The redesigns, decided now because breaking is free:

- **`Canvas`** is a copyable `#[repr(C)]` struct (clip, origin, flags, an opaque
  framebuffer pointer) in the plugin's frame; `clipped()` is a stack copy. The
  drawing primitives are host entries. Plugin code never reads the framebuffer.
- **`Env`** is a size-worded snapshot of plain data (`now`, `device`, `caps`,
  `metrics`, the screen size class) plus handle proxies that keep their method
  names (`env.settings()`, `env.tr(..)`, `env.fonts()`); the 74 accessors become
  entries. Its fields stay private, as they are.
- **`FontHandle`** is a 32-byte `PpFontRef` carrying a generation. `StoredPack`,
  `Cache`, `Pack`, `PackSource` stop crossing: a stored pack is a font opened with
  a read callback. The seven `PACK_*` entries merge into `FONT_*`.
- **`UiStyle`, `Row`, `Label`** become flat structs with a size word as first
  field (append-only), enums-with-data as a kind tag plus fields.
- **Text layout** (`LAYOUT_WRAP_*`, the hardest): the advance and kern closures
  are callbacks with user data; the first version keeps per-character semantics
  (about 200 to 400 thousand crossings for a 100 thousand character chapter,
  measured against the budget in section 9), with a per-word callback and a fixed
  point variant held back as appended entries if the gate says so. Lines come
  back through a caller buffer of `(start, end, flags)` records.
- **Strings from catalogs** (#208): `env.tr(key, out)` writes into the caller's
  buffer; the SDK facade keeps a small plugin-side arena for `&str` returns inside
  one callback. The kernel's translation cache never has to stay pinned for a
  plugin's reference; the eviction problem of #208 disappears with the borrow.
- **`Storage`** stays a trait on the plugin side (669 uses) and is implemented by
  a proxy over host entries (open, read, write, list with a visitor callback,
  rename, sync, atomic replace with defined premature-EOF and card-loss results).
- **Services** (`Services::get::<T>()`, `service_type!`, `TypeId`) are replaced by
  a **key, a major.minor and a `#[repr(C)]` table per key** (`core.library/1`); the
  provider fills a table with a size word, the consumer reads what its minor
  needs. Only one service is real today (`core.library`, 19 consumer sites).
- **`Params::carried::<T: Any>`**, `&SettingValue` returned by reference
  (12 uses), borrowed references out of registries (about 120 sites) and direct
  use of `paperpaw_text::Font` change source form. Section 7 lists what cannot
  stay source-compatible.
- **The `kernel` feature** (installers: update, store, transfer, plugin-manager
  enable it) is replaced by a supported typed installation and staging service, so
  that an independent installer needs no special build; this is also the
  acceptance test for the freeze.

### 4. The plugin side: export table and callbacks

The plugin exports one symbol, `PAPERPAW_ENTRY` (in `.rodata.paperpaw_entry`, as
today), a `#[repr(C)]` struct that begins:

```
struct PpEntry {            // fixed forever; later fields are appended
    u64 identity;           // magic (32 bits) | major u16 | target u8 | reserved u8
    u32 size;               // of this struct
    u16 needs_minor;        // the highest minor the plugin's stubs use
    u16 flags;
    const PpPlugin *(*make)(const PpHost *host);
}
```

`PpPlugin`, `PpScreen`, `PpWidget`, `PpDocument`, `PpLibrary`, `PpGesture`
and any later object kind are likewise `#[repr(C)]` tables of function pointers
with `size: u32`, `state: *mut c_void` and `drop` first. **Growth is the size
word:** the firmware calls a callback only if the table's `size` reaches it and its
pointer is not `NULL`; absence means the default the description gives. This
replaces `revision()`, `call_since` and the `GROWING` allowlist for every object
kind at once (nested wrapper screens included: a wrapper simply forwards its
inner table's size).

**Closures never cross.** A registration that today carries a closure
(`Registrar::on`, actions, timers, filters, hooks: 30 of 73 `Contribution` kinds)
carries a `u32` callback token; the plugin keeps its closures in an array on its
side and the firmware calls the plugin's typed dispatcher
(`dispatch_event(state, token, args, len, out, cap)` and a handful of siblings for
the hot kinds). Non-capturing `fn` pointer handlers (83 `FixedAction` and 92
`FixedScreen` factories in the first-party plugins) become static tables and cost
no heap. The `Screen` trait's 35 items split into the **direct** ones (`draw`,
`on_key`, `on_pointer`, the gesture recognizers: scalars and handles, no
encoding) and **one snapshot** the plugin returns with each handler call for the
19 queries (`region`, `grays`, `dims_below`, `hints`, ...): one crossing instead of
nineteen.

### 5. Commands and events: one versioned wire format

All 66 `Command` variants are plain data (checked: none carries a closure, a trait
object, a reference or a float). They, the registration records (the 42
declarative `Contribution` kinds and the tokens of the 30 closure-carrying ones),
and every event and reply the firmware sends cross as **records** in a byte
buffer. Decision, from the comparison in the inventory (positional records are 5 to
12 % smaller and 8 to 14 % cheaper to decode than TLV; allocation, not parsing,
dominates either way):

- A **batch** is `count: varint` and `count` records, one buffer per callback
  return, owned by the plugin and borrowed by the firmware for that call (the
  firmware decodes straight into its own `Command` values, never into a copy).
- A **record** is `tag: varint`, `len: varint`, then the body: a **presence
  bitmap** (1 or 2 bytes, one bit per optional field), then the fields positionally
  in declared order. Strings are `len: varint` and UTF-8 bytes; ids are strings;
  integers are fixed width or varint as the description says; no floats in a
  record.
- **Forward compatibility is the rule, and it is checked:** an unknown `tag` is
  skipped by its `len`; trailing fields a decoder does not know are skipped by the
  record's `len`; trailing fields the encoder did not write are the defaults of the
  description. Fields are only appended, tags are never reused or changed, a
  retired record stays in the description forever.
- A **well-known string dictionary** (parameter keys, the framework's ids: about 255,
  append-only, in the description) is encoded as one byte; it cuts a six-pair
  `PAGE_TURNED` event from 176 to 139 bytes and six of fourteen allocations on the
  firmware side.
- **Limits:** a batch of at most 4 KiB on the X4, a string field at most 64 KiB;
  `ERR_NO_SPACE` otherwise. Redraw, refresh and wake-up requests are batch header
  flags, not commands. Inbound blobs (an HTTP body) go beside the record as a
  borrowed `(ptr, len)`, not inside it.
- **Namespaces are stamped, not claimed.** The network ownership that today
  derives from ids in the payload (`work.rs:885`) becomes the caller the firmware
  stamps on every record; secrets in a record are zeroised after decode.

### 6. Compatibility contract for 1.x and the loader

- **Identity:** `major` (this ADR starts it at 7, continuing the interface
  number) and the **target** (`riscv32`, `xtensa-s3`, `aarch64-android`, `x86_64`,
  ...), in the entry's first eight bytes and in the `.ppn` header. The layout digest,
  the feature bits (`outline`, colour) and the **compiler hash are gone**: no Rust
  layout crosses, so none can differ, and a plugin may be built by any compiler that
  can build `core` for the target and honours the C ABI.
- **Compatibility:** a plugin built against ABI `7.m` runs on every PaperPaw of
  `7.n` with `n >= m`. Within a major the ABI only grows: host entries, plugin table
  fields, records, record fields, dictionary entries, constants and error codes are
  appended; nothing changes, moves or goes. A **minor** is bumped by the release
  process when the first release containing appended entries is cut; the generator
  sets `since` from the lock. A major means rebuilds and is rare.
- **The loader decides in this order** (the same on the X4, the X4 Pro and
  Classic, the emulator, Android, the web emulator): magic wrong, or `size` shorter
  than the fields the loader needs: *Damaged*; `major` older: *Built for an older
  plugin interface*; `major` newer: *needs a newer PaperPaw*; `target` other:
  *built for another chip*; `needs_minor` above the host's: *needs a newer
  PaperPaw*; the signature, the installed hash and the placement checks of ADR 0024
  as now. A refused plugin stays installed and switched off with its data; nothing
  of its code runs. Beta plugins (interface 6) are refused this way and rebuilt
  once; there is no adapter.
- **A newer plugin on an older firmware** is refused by `needs_minor`, which the
  generated stubs compute (the highest `since` of any entry the plugin links).
  The old `Api::SIZE` / `needs_api` pair is the same idea in its first form.
- **Behaviour of an entry never changes** within a major in a way an old plugin
  could notice. A fix that changes a result the plugin keeps (a geometry, a page
  count, a cache key) is a **new entry** and the old one stays (its body may call
  the new one only if the results agree). This replaces ADR 0026's three cases and
  the `bump` lines: after 1.0 there is no `OLDEST`.

### 7. The author's SDK is generated over the table, and one path only

`paperpaw-sdk` keeps its public Rust API where it can. Under it `paperpaw-abi`
turns calls into table entries and records; the large functions stay in the
firmware. What stays source-compatible: `Plugin`, `Screen` (the trait and its
defaults), `Widget`, `Env` accessors, `Registrar` methods, `Command` as a Rust
enum (encoded when pushed), `Params`, `Id`, `Rect`, the `ui` blocks, closures in
`r.on(...)` (kept on the plugin side behind tokens). What cannot, listed so
authors are told once (counts from the first-party plugins):

- values borrowed out of a registry or a catalog beyond the callback (about 120
  sites): copy, or use the arena helper;
- `Params::carried::<T>`, `Services::get::<T>` and `service_type!`: replaced by
  key + table services; `Settings::get -> &SettingValue` (12 uses): by value;
- `Cow<'static, str>` and `&'static` spec tables in specs, boxed closures stored in
  specs (`ScreenSpec`, `ActionSpec`): builders taking `&str` and a token;
- direct use of the text, layout and graphics crates' types
  (`paperpaw_text::Font`, `Canvas` fields): handles and entries (`Canvas::draw_mask`
  takes a per-row callback instead of a per-pixel closure);
- `plugin!`: the macro exports `PAPERPAW_ENTRY` and the plugin table; the old
  `*mut Box<dyn Plugin>` hand-off is gone.

**One path.** Built-in plugins (the 43 first-party plugins of the X4 and the
others) talk to the kernel through exactly the same table, linked statically; the
emulator's, Android's and the tests' host is the same `HostApi`. There is no
second, direct backend kept for built-ins: it would be the one path never
exercised where code plugins are loaded. The cost is the measured flat-call cost
for every plugin on every start, which is the point: every first-party plugin on
every device tests the boundary continuously.

### 8. The gate: `abi.lock` replaces the shapes

`abi/abi.lock` records, in order, every entry, callback, record and field with a
hash of its signature and its `since`. CI (`tools/abi-gen.py --check`, a job in
place of the interface test and `table-compat`) fails when: a generated file is
stale; the lock is not a prefix of the new one (anything changed, moved or went);
a new line has a `since` that is not the current or next minor; a record field is
inserted instead of appended. `api.txt` and `shape.txt` of the 19 crates stay
only where they serve source semver of `paperpaw-sdk` (`cargo-semver-checks`); they
no longer gate binaries.

Behaviour is checked by running old plugins, not by comparing text:
`tools/compat-e2e.sh` (built in #672) is adapted: the fixture plugin is built at
the last released minor and run on HEAD (must run); the same source built with two
compilers (the repository has 1.88 and 1.99.0) loads in the same host (must run);
a table with an entry moved or a record field reordered (must be refused by the
loader or fail the gate). Golden wire vectors for every record live in
`abi/vectors/` and are decoded by both sides.

### 9. Interface 7 content: decided now, in the same batch

In (changes a layout or an ownership while breaking is free):

| Item | Decision |
|---|---|
| The boundary itself, `Env`/`Canvas`/`FontHandle`/`Screen` snapshot/`UiStyle`/`Row`/`Label` as above | in |
| #208 owned or scoped translation lookup (`tr` into a caller buffer, plugin arena) | in; the old borrowed cache ownership ends |
| #164 trial-kept event, sent only after the OTA mark is durable | in (additive, cheap, wire event) |
| #123 per-screen status wish with Show/Hide/Default and the precedence rule (user, then selected provider, then screen, then default) | in as a screen flag in the snapshot and a versioned policy service |
| #152 `core.off` slot with a reason, asleep refresh capability bit | in (slot, event, capability bit); timed refresh itself later |
| #219 boot, restart and shutdown appearance: plugin-prepared cached asset and post-init slots | descriptor and slots in; the early-boot asset path is implemented after the batch, fallback stays in the shell |
| #206 widget look, family 6 | its entries are table entries (4 B each); nothing new |
| `UiStyle` metric tokens (#114 M3) | in, as a size-worded struct |
| HTTP progress and cancel (#147, #110), exactly-one completion | in as a record pair; no change of request layout needed |
| Supported installer service (replaces the `kernel` feature in four plugins) | in |
| `Storage` semantics: atomic replace, premature EOF, sync, card loss | in |
| Canonical names: focus helpers (`*_in(env, ..)`), remove `Family::CORE` | in; one name each |
| P0 first-party workarounds (`firmware.rs:1113` literal `sleep.screen` id, dual-boot ini editing through a typed request, `readers.rs:403` parameter rebuild, stored-font streaming) | in where they need an entry or record; the rest is plain work |

Out (added later as appended entries or records, without a bump): #82 picture
pre-dither stage and #92 zoom (a bounded crop and tile source), a shaping provider,
input policy and hardware adapters, the notification provider, the shared home kit
(#205), theme completeness beyond the tokens. The boundary supports each as an
appended entry; none is a reason for a second interface number.

### 10. What the Workshop, site, store and tools get

Answers to the inputs of the Web Engineer's audit (ticket #228):

- **File:** `.ppn` **format 5**. Header fields: magic, format, `target` (was
  `chip`), `abi_major`, `needs_minor`, flags, code and data extents, relocation
  table, signature. **Removed:** the compiler hash (offset 44), the layout
  fingerprint, the family revisions (eight words), `needs_api`. The header length
  and entry offsets are in `abi.json`; nobody hard-codes 116, 32 or 9 again.
- **Symbols:** `PAPERPAW_ENTRY` (section `.rodata.paperpaw_entry`) and `PAPERPAW_HOST`
  (one word the loader fills). `PAPERPAW_TABLE_0..7` and `PAPERPAW_API` are gone;
  `paperpaw-plugin.x` shrinks accordingly.
- **ABI identity:** `abi = "<major>/<target>"` (for example `7/riscv32`) replaces the
  64-bit fingerprint in the update answer and the catalogue. One build per
  `(major, target)` serves every minor at or above its `needs_minor`; the store may
  show `needs_minor` to say "needs a newer PaperPaw" instead of hiding the build.
- **Compiler role:** none in the file and none in the check. The Workshop's
  "exact-release SDK and toolchain pins" become the SDK's `rust-version` and a
  recommended compiler per target for authors; a build with another compiler is
  valid. The release still builds first-party plugins with the release compiler.
- **Capability units:** `Uses` (network, Bluetooth, developer) stay in the manifest
  as a record field; no table families.
- **Signer:** Ed25519 identities and keys are unchanged. The signed domain string
  changes with the format (`PPN5`); `paperpaw-sign pack` reads `abi.json`.
- **Fixtures:** `abi/vectors/` (wire records) and a fixture plugin source built by
  the CI matrix; the Workshop's host and device `plugin!` wrappers are generated from
  the same description as the SDK facade.

## Work breakdown

Owners per the Manager's assignment of 2026-11-05: Kernel Engineer leads the core
side; Firmware Engineer the shells; Plugin Engineer the plugin side; Web Engineer
the Workshop and site; Senior Reviewer the gates. Estimates are agent-hours,
planning ranges, unmeasured. **No migration shims.** Old beta plugins are refused by
the interface number; the old path is deleted at the end of stream E.

| Stream | Content | Owner | Hours |
|---|---|---|---:|
| A. Description and generator | `abi.def` grammar, `abi-gen.py`, `paperpaw-abi` crate, `abi.json`, lock and CI check; the first 20 entries by hand to settle the grammar | Kernel | 30 to 50 |
| B. Host side | `HostApi` implemented in `paperpaw-core`: the 109 mechanical adapters generated, the 34 redesigned entries by hand, record decoder, dispatch by token, handles and generations, `Storage` and installer services | Kernel (+ Firmware for shell parts) | 70 to 120 |
| C. Loaders and shells | `.ppn` format 5 and `paperpaw-native`, X4 flash/MMU loader, X4 Pro/Classic PSRAM loader, desktop `HostLoader`, Android, web emulator, `paperpaw-plugin.x`, `paperpaw-sign`, host `PpHost` statics per shell | Firmware | 30 to 50 |
| D. SDK facade | generated stubs and the hand-written ergonomic layer over `paperpaw-abi`: `Env`, `Registrar`, `Screen` snapshot, `Canvas`, `Storage` proxy, token closures, in-process test host | Plugin | 60 to 100 |
| E. Port and delete | 75 crates (149 thousand lines; 32 mechanical, 31 medium, 11 hard: reader, widgets, format-epub, home, picture-home, settings, sleep, store, update, transfer, plugin-manager), then delete `paperpaw-table`, `shared!`, `native::LAYOUT`, the compiler pin, `GROWING`, the 19 baselines | Plugin, then all | 230 to 455 (about 315 likely) + 20 |
| F. Interface-7 content | section 9 items that are more than a flag | Kernel + Plugin | 60 to 100 |
| G. Tooling and Workshop | site `native.rs`, catalogue and updater `abi` field, web flasher, Workshop wrappers and builder, packer/signer, plugin projects, samples, docs | Web | 30 to 50 |
| H. Gates and device | compat gate adapted (#672), golden vectors, two-compiler test, memory and time budgets, device runs | Firmware, QA, Device Tester | 40 to 70 |
| | **Total** | | **about 600 to 1,000** |

Parallelism: A first (about one day, it unblocks everything); B, C and D then run
together; E is parallel by class across agents once D has a vertical slice; F and G
start with B. Critical path: A, a **vertical slice** (milestone M1: the
description, one screen with a draw, an event, a command and a setting, the Battery
widget, on the emulator and on the X4 through the real loader), then the ports.
The senior reviewer reviews A and the slice before B to E scale up, and the
loaders and the wire decoder before they are used (SDK/ABI, memory, trust).

**Order of work while old and new coexist.** On an integration branch the SDK
facade has two backends for the duration: the existing direct one and the table.
Every converted subsystem must give **identical pixels and identical commands**
through both on the existing snapshot and kernel tests (the method of the
prototype's host test). When the last plugin has moved, the direct backend, the
shared tables and the old gate are deleted in one change; the end state has one
path.

## Gates and stop criteria

Budgets are the prototype's numbers; each is a measured value posted on #228 with
what, where, commit and number.

- **M1 (the slice), go or no-go.** On the X4 through the real loader: simple drawing
  calls within 2 % of the direct path; the Battery widget within 3 % (the prototype's
  +11 % with fixed-point text must be gone); `deliver` and a command round trip
  under 2 microseconds more than now; wire peak heap per batch under 1 KiB; flash for
  the 157 + records under 60 KB; no allocation per drawing call.
- **Memory.** Heap peak at home, in the reader on a 100-page chapter, and during a
  plugin update within +4 KiB of main; retained heap for handles and the token
  tables under 2 KiB for 43 plugins; stack depth of the deepest call within +512
  bytes.
- **Time.** X4 start to first frame no more than 100 ms later; reader page turn no
  more than 5 % slower on the device; the wrap callbacks measured on a real chapter.
- **Correctness.** Identical pixels and commands against the old path on every
  snapshot test, then the old path deleted; old-beta plugin refused cleanly; a
  fixture plugin built at an earlier minor runs; two compilers; golden vectors
  decode on both sides; malformed records and tables fuzzed.
- **Device.** Device Tester runs the slice and the final image on the test X4 (port
  4000), and the Pro only with the owner's OK.

**Stop and ask the owner if:** M1 misses a budget by more than half again and the
cause is the boundary, not the prototype; more than 40 of the 157 entries cannot be
flattened without a visible loss of function; the wire format needs more than 1 KiB
peak heap per batch; or the integration branch cannot reach identical pixels and
commands on the snapshot suite after two attempts at a subsystem. The fall-back is
the pinned-compiler platform of ADR 0026 (variant A), kept on `main` until the batch
merges; its hardening (#672) is the base of the new gate and stays useful.

## Consequences

- Plugins survive compiler upgrades and, within a major, every release. The
  compiler hash, the layout digest and the three-case body rules disappear from the
  rules authors must know; the store lists one build per major and target.
- The firmware grows by about 40 KB (23 to 80) and each plugin by under 1 KB; the
  heap does not grow. Every first-party plugin pays a small, measured call cost.
- The generator and the description are the one place the interface is edited. A
  contribution that adds a thing to the interface adds a line to `abi.def`, which
  reviewers read instead of seven crates' shapes.
- The Rust SDK is no longer the interface; a plugin may be written in anything that
  can honour the C ABI (not promised, but possible).
- Cost: the largest SDK change before 1.0 (about 600 to 1,000 agent-hours), a
  one-time rebuild of every plugin, and the discipline of only adding.
- Risks: a flattened entry that loses function (the stop criterion above); the wire
  format becomes a second interface to keep stable (hence golden vectors and the
  forward-compatibility rule); a hot path that crosses too often (text wrapping
  callbacks, per-row UI entries: budgeted, with appended fast variants available);
  the generator becomes critical code (small, deterministic, tested, reviewed
  first).

## Open points for the design review

1. Grammar details of `abi.def` (types of handles, optional fields, callbacks that
   take callbacks) are settled by the first 20 entries in stream A.
2. Whether `Block` text filters run without a round trip per block (a batched
   `filter_blocks` entry), and whether storage `list` visitors may call storage.
3. `tr()` strings: arena in the plugin (recommended) or pinned for one callback.
4. Whether `major` continues the interface number (7) or restarts at 1 with the
   ADR 0024 reading ("plugin API 1.x"); the number in the entry is what matters, the
   spelling is a documentation choice.
5. The static-blob plus token path for `Fixed*` registrations needs a prototype and
   an X4 heap measurement before it is committed.
