# 0026. Code plugins bound to a plugin interface version

- Status: Accepted; refines the fingerprint of
  [ADR 0024](0024-native-code-plugins.md) until the stable plugin API it
  plans for 1.0
- Date: 2026-10-23

## Context

ADR 0024 binds a code plugin file to the PaperPaw it was built for: a
fingerprint of compiler, target, SDK version and features, made by build
scripts of `paperpaw-sdk` and `paperpaw-native`. Two things go wrong
with that before the betas of 0.5:

- **Every release breaks every code plugin**, also a beta that changes
  nothing a plugin sees: the SDK's version is in the fingerprint. Betas
  come one after another, and authors would rebuild for each.
- **A build on Windows differs from one on Linux.** A build script in a
  plugin's dependencies is built for the computer, and Cargo hashes the
  computer's target into every crate's `-C metadata`, which flows into
  the SDK and the plugin's own crate (their `TypeId`s, their code). The
  plugin library rebuilds each file on Linux and compares, so an author
  on Windows never matched, however the paths were written.

## Decision

- **A plugin interface version, raised by hand.** `native::INTERFACE`
  (a `u16`) names what code plugins are built against: the SDK's items
  they may use, the layouts of the types that cross between plugin and
  PaperPaw, the firmware's table (`Api`), how a plugin is linked
  (`paperpaw-plugin.x`) and packed, and the release's compilers. It is
  raised when one of them changes, and only then. A plugin file runs on
  every PaperPaw of its interface, whichever release made it.
- **The fingerprint from constants alone.** `native::FINGERPRINT` is the
  interface in its top 16 bits, and below them a hash of the chip
  (`cfg!(target_arch)`), the pointer width, the features that change what
  a plugin's own copy of the code does with what crosses
  (`paperpaw_text::OUTLINES`; colour does not: Android and the emulator
  draw in colour, and plugins built without it run there), and
  `native::LAYOUT`: a digest, made at compile time, of the sizes and
  alignments of the types that cross (`Env`, `Registrar`, `Command`,
  `Canvas`, `Settings`, `Box<dyn Plugin>` and the others) and of where
  the firmware's table keeps each entry. No build script: the SDK's and
  `paperpaw-native`'s are gone. A file of another interface says which
  (`Fingerprint::misfit`): the X4's log and the plugin manager say so.
- **The linker script lies beside each plugin project**, as the sample
  (`samples/hello-native/paperpaw-plugin.x`) and `tools/plugin-files`
  have it; `.cargo/config.toml` links with it by its rustflags, which are
  not in `-C metadata`. The copy is part of the interface: a test keeps
  the repository's copies equal to `paperpaw-native`'s.
- **The compiler is pinned by the release, and each file names its
  own.** Until 1.0 the SDK's Rust types cross as they are, and a new
  compiler may lay them out or call them otherwise in ways no size or
  alignment shows (the order in a vtable, how a `repr(Rust)` value is
  passed). So the compiler is part of the interface: the X4's from
  `shells/xteink/rust-toolchain.toml`, the X4 Pro's from `versions.env`,
  Android's from its `versions.env`, named by `native::TOOLCHAIN` (a test
  keeps them equal, and the repository's plugin projects pinned to the
  X4's), and a new one raises the interface. Without a build script no
  constant of the SDK knows the compiler, but every object Rust makes
  names it (`rustc version 1.99.0 (b940084d7 2026-09-28)`, in `.comment`).
  `paperpaw-sign pack` takes it from there, refuses a plugin built by
  none or by two, and writes a hash of it into the file's header (format
  2). The whole release counts, espup's own version too, which tells two
  of its releases on one commit of Rust apart. The firmware knows its own
  from its build script, which a shell may have, and loads only files of
  it (`Native::compiler`). The store installs, and an update fetches,
  only files that fit that way (`Native::fits`); an update answer names
  the fingerprint and compiler of its firmware (`abi`, `compiler`), and
  where it does not, a fetched file must have the running firmware's.
  The emulator checks a library built for the computer the same way, by
  the release an ELF library names or by the commit in the paths of its
  `std`; the repository's own `rust-toolchain.toml` pins the release's
  compiler too, so that the emulator and the plugin projects agree.
- **CI fails where what plugins see changes without a raise, and any
  change raises it.** The crates whose types cross are the SDK and every
  crate of PaperPaw it depends on (`crates/paperpaw-sdk/interface/
  crates.txt`; `native::CROSSING_CRATES` names the same). Each keeps
  `api.txt`, its public API, and `shape.txt`: the fields, variants and
  trait items of its public types and of every private type their fields
  reach, in the source's order, private fields too, each with its type,
  functions' pointers with their ABI; and the values of its public and
  associated constants (`tools/interface.py`, from rustdoc's JSON with
  every feature a plugin may turn on, on the nightly and the
  cargo-public-api of CI). Not the SDK's `kernel` feature: what only the
  kernel and the shells build (ADR 0020) is in no plugin, so its types
  change with the firmware alone, like any type of the firmware's own.
  `api.txt` alone does not do: it is sorted, so an inserted variant or
  two fields of one type swapped do not show, and it shows no private
  field and no value. `interface/` keeps both as they were when the
  interface was last raised, and `interface.txt` the linker script's
  hash, the compilers and the crates of others the crossing crates take.
  Their types may lie inside those that cross (skrifa's in an outline
  font, ed25519-compact's in a signature's check, miniz_oxide's in a
  ZIP's reader), so each is pinned to one version (`=x.y.z`, the test
  sees to it), and a new version raises the interface. A test
  (`cargo test -p paperpaw-sdk --test interface`) compares them. By
  default a change raises the interface: a field, a variant or an item
  of a trait that was there, a change of order, of a private field or of
  a signature, anything gone. Free is only what no plugin built before
  can rely on: a new type, module or re-export, a new function or
  constant outside a trait, a new `impl`, the members of a type that
  is itself new, and the type behind a trait object: state that only
  the firmware's code reads sits behind one (`StoredPack`, `GlyphCache`,
  made by the shared table's `STORED_OPEN` and `CACHE_NEW`), so how it is
  kept can change in any firmware. That holds where
  1. the trait's items are all the other side calls, and none exposes
     the type behind it (no `Any`, no downcast, no pointer to its
     fields);
  2. an object runs the code of the side that made it, and does not
     outlive that code (plugins are unloaded only by a restart);
  3. its table of functions is the pinned compiler's;
  4. the trait has no supertraits, items generic over types or
     constants (lifetimes are fine) or items returning `Self`, so its
     table's order is its items' order;
  5. items are only appended, each with a higher `revision()`, the
     trait's first item, which code checks before it calls a newer one.
     The gate lets such a trait (its `GROWING`) gain items at its end and
     nothing else, and pins every trait's items with their signatures
     and the layouts of the types those reach;
  6. both sides allocate from the one global allocator: a plugin that
     drops the last handle frees what the firmware allocated, with the
     size the table gives.

  `Screen` (interface 6) is the first such trait plugins implement
  rather than the firmware: its `revision()` is a default method, so a
  plugin's screens say the revision of the SDK it was built with, and
  plugins never implement it. The kernel calls every item newer than
  revision 1 through one helper, `paperpaw_sdk::call_since`. A screen
  that passes items on to another it holds (a host, the sleep screen
  showing another plugin's) says no higher a revision than that one
  (`revision_holding`), so it is never asked what it could not pass on.

  Any other new item of a trait is never free: the firmware would call
  it through the table of a plugin built before, which does not have it,
  and the same holds for the firmware's own trait objects a plugin is
  handed. Either way the files are written anew (`PAPERPAW_BLESS=1`).
- **Shared functions run the firmware's body, so a body change is
  judged like an interface change** (#44). On a RISC-V chip a plugin
  calls the functions of `crates/paperpaw-table/shared.list` (families
  text, ui, graphics, data) in the firmware, by index, whichever release
  built it: their signatures are pinned (`table.sig`), their bodies are
  not, and no check sees what a body does. A change to the body of an
  entry is one of three:
  1. **Free**: for the same input it returns, writes into its arguments
     and leaves in what a plugin keeps the same as before; only how it
     looks inside the area it was given changes (a new look, a fix of a
     drawing), or its speed or memory, or what it does with input no
     plugin can give it (a panic that never happened). Plugins built
     before take it up without a rebuild, which is the point of sharing.
  2. **Free as well**, where a result does change (a height, a position,
     a hit, a line break, a focus or page, a parsed value), when every
     function that must agree with it is an entry too, of any family (all
     of them run the one firmware's bodies, whichever family a plugin's
     calls go through), changed in the same commit, and no value a plugin
     keeps (a cache, a file, a page index, a revision such as
     `paperpaw_layout::VERSION`) and no promise of the documentation rests
     on the old result. A plugin built before then calls all of them
     anew, and they agree. Examples: `Row::height`, `Row::draw` and
     `Row::hit` are all ui entries, so a row 4 pixels taller is free; the
     ui entry `fit` shortening Arabic as the text entry `measure` sets it
     is free too.
  3. **A bump** otherwise: a line `bump <family> <revision>` appended to
     `shared.list` in the same commit, at the family's next revision.
     The firmware then refuses the plugins of that family built before
     it ("rebuild the plugin", `Misfit::Older`) and runs the others.
     Example: a ui height that changes while the function that must
     agree with it (`switch_rect`, a constant, a helper that is no
     entry) stays a copy in the plugin. A bump costs every user of those
     plugins a rebuild, so it is reviewed as an interface change is and
     named in the release notes; often making the partner an entry too
     (appended at the next revision) turns it into case 2.

  A changed signature, an entry removed or moved, or a type's layout
  is never a bump: those raise the interface. `tools/shared-table.py`
  keeps a digest of each entry's body and of the functions of its crate
  that it calls (`crates/paperpaw-table/table.body`), and CI fails where
  one changed without the file written anew: so every such change shows
  in its commit, which says which of the three it is, and reviewers
  check that.
- **A crash in shared code is the calling plugin's.** A panic in a
  shared function, or in any other code of the firmware a plugin's code
  called, that has the plugin's code below it on the stack is blamed on
  the innermost such plugin, as its own panic is: safe mode switches that
  plugin off, not all of them. The firmware's parsers (JSON, INI, XML)
  are shared and read what plugins hand them, and an allocation that
  fails in a shared body is the plugin's request: before the families,
  both happened in the plugin's own copy, and so they still count. A bug
  of the firmware that only one plugin's calls reach is then also that
  plugin's to the reader, which is the lesser harm. This covers more
  than the `shared!` entries: the firmware's objects a plugin calls
  through its table, such as its `dyn Storage` (a fault of the FAT code
  reached through one plugin's reads switches off that plugin, not all of
  them). The plugin is looked for in the 64 innermost calls.
- **Cache revisions can increase without changing the interface.** The
  exact `paperpaw_layout::VERSION: u32` constant identifies the layout
  algorithm in page-index keys. Raising it keeps indexes made by the old
  algorithm apart from those made by the new one. Each plugin's copy of
  the layout code uses its own revision; a newer revision does not change
  the shape or interpretation of any value passed between it and the
  firmware. The gate permits only an increase of this exact constant in
  `paperpaw-layout`, with both values valid `u32` integers. A decrease,
  removal, type change or change to another constant remains guarded.
  The current shape file is updated while the interface baseline stays
  as it was. This is separate from the existing exception for the SDK
  release number in its HTTP User-Agent string.
- **The layouts are checked on each chip by the fingerprint.**
  `native::LAYOUT` is made by the compiler for the chip it builds for,
  so a plugin for the X4 has the X4's layouts in its fingerprint, and the
  X4 loads only files with its own. The interface's files do not keep a
  digest of the computer's layouts: those differ from the chips'
  (`usize` is 32 bits there) and change with the computer's compiler,
  while what changes a type's layout on a chip is either in its source,
  which the gate sees, or the compiler, which the file names.
- **Services are found by a key.** A service type of a plugin's own
  crate may be another type of the same name in another build, so a
  service has a contract (`ServiceType`, written with `service_type!`):
  a key that its author raises when the type changes. A service is found
  across builds by that key (`Services::get`), with its size and
  alignment checked.
- **Where a plugin's code lands binds it to nothing.** A `.ppn` file is
  linked at a fixed base and relocated where it is loaded; it reaches the
  firmware only through the table whose address the loader writes into
  it, and through what the SDK hands it. The X4's index of the plugins in
  flash names the firmware that placed them; behind another firmware the
  plugins are placed anew from the same files, at the next start. The X4
  Pro relocates into PSRAM at every start.

## Consequences

- Authors rebuild when the interface is raised, not with every release
  or beta. The release notes and the plugin library say which interface
  a release has; the store lists a build for each fingerprint, so a
  reader not updated still finds the one for it.
- A plugin built on Windows is the same as one built on Linux, given the
  paths are written as the same (`build.sh`, the rebuild, and
  `tools/reproducible-plugin.py`, which CI runs on both).
- Raising the interface stays a decision of its own: a pull request that
  changes a type that crosses fails until it raises it. Raising is the
  default, so the gate errs on the side of a new interface: a private
  field renamed raises it too.
- The gate sees types and signatures, not behaviour. A plugin carries
  its own copy of the SDK's code (except the shared functions, see
  above), so a function whose body changes what
  it writes into a value both sides read (a format, an invariant) is the
  same item to the gate, and such a change raises the interface by hand.
- Interface 1 was never released: 2 is the first, with the files of
  format 2 that name their compiler.
- The stable, C-shaped plugin API of ADR 0024 for 1.0 is still to come;
  then its major version becomes the interface version.
- Interface 5 was never released either: 0.6.0-beta.1 took the
  interface-6 round too, as one raise from 4 to 6 (#363).

## Amendment: the fingerprint kept (2026-11, #94)

A plugin is loaded where its fingerprint is the firmware's, and the
fingerprint holds the interface and the sizes and alignments of what
crosses, not the order of a trait's items or an enum's variants. The
first bless of the interface-6 round stayed at interface 5 with the
number in `interface.txt` lowered by hand: the `Screen` trait's items
moved, the fingerprint stayed, and plugins built before would have been
loaded and called the wrong functions. So:

- `interface.txt` keeps the fingerprint the bless was made with, and
  what made it: the computer's architecture, with `+outline` where the
  `outline` feature added its layouts (`fingerprint = x86_64 …`; a
  workspace build turns the feature on, `-p paperpaw-sdk` does not).
- A bless of a change plugins rely on is refused unless the interface
  is above both the number `interface.txt` says and the interface of the
  fingerprint it keeps.
- Layouts that changed while the shapes did not (the fingerprint of the
  same architecture and features is another at the same interface) are
  such a change.
- Outside a bless the fingerprint kept must be the build's: its
  interface on any computer, its value on one of the same architecture
  and features.
- Items appended to a growing trait and anything new stay free: they
  change neither the shapes kept nor the fingerprint.

## Amendment: old plugins keep running after additions, enforced (2026-11, #228)

The decision above has one consequence worth saying on its own, and now
tests: **a plugin built for interface N runs on every PaperPaw of interface
N.** What may change inside an interface is what a plugin built before
cannot rely on (the list under "CI fails where what plugins see changes
without a raise"; `docs/plugins.md`, "What a release may add without a
rebuild", has it as a table for authors). Everything else raises the
interface; the compiler stays part of it, pinned by the release.

- **Additions are checked one after another.** Writing the baselines anew
  after an addition makes the next addition be compared with a baseline that
  already has the first. The firmware's table (`Api`) therefore may have
  fields after `build_len` in the baseline too, and a second appended field
  is as free as the first. (Before this amendment the gate compared with a
  table that had to end at `build_len`, so the second field was seen as a
  change.)
- **Lines may stand twice.** `api.txt` is compared as a multiset: one of two
  identical lines going (a method listed under two impls) is a change.
- **The gate has a corpus.** `crates/paperpaw-sdk/tests/interface/corpus.rs`
  changes every type and constant of every baseline in the ways above (a
  line gone, a member added first or last, dropped, swapped or retyped, a
  value changed) and fails if anything passes that is not one of the three
  named exceptions (the table's end, a growing trait's end, the constants
  `tools/shared-table.py` derives); it adds to every crate in sequence and
  fails if an addition is refused.
- **An old plugin is run, not only compared.** `tools/compat-e2e.sh`
  builds the test plugin at the last raise
  (`crates/paperpaw-table/table-baseline.txt`) and loads it, with the
  emulator's own loader and in a kernel, into copies of the tree with two
  fields appended to the table and two SDK items added (it runs) and with
  a field inserted into the middle of the table, or the interface number
  raised (it is refused as another build before any of its code runs).
  The copies are `tools/compat/*.patch`.
