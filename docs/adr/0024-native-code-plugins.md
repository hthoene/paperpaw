# 0024. Native code plugins as files

- Status: Accepted, partly implemented; supersedes the WebAssembly
  modules of [ADR 0004](0004-everything-is-a-plugin.md); its fingerprint
  refined by [ADR 0026](0026-plugin-interface-version.md), a plugin
  interface version in place of the SDK's version. The X4s and the
  desktop emulator load code plugins; Android loads none and will not
  (see the amendment of 2026-11 at the end).
- Date: 2026-10-19

## Context

Code plugins are Rust crates built into the firmware; data plugins are
folders on the card (ADR 0004). The plugin store (ADR 0023) brings data
plugins to the X4 and the X4 Pro. A community store that can grow
without limit needs code plugins too, without a new firmware: a firmware
built for each reader's choice (a build service) was ruled out (waiting
time, GitHub's terms, a signing key on a server), and building every
plugin into the firmware does not scale.

So a code plugin must be a file the device loads. ADR 0004
planned WebAssembly for that. A spike measured both ways
(its report and prototype are kept on the branch `spike/native-plugins`,
not merged: the prototype is reference, not the implementation):

- **WebAssembly** (wasmi 2.0): one small plugin took 73 to 121 KB of
  heap, more than the X4 has free after opening a book; plugin code ran
  about four times slower than native code on a computer; wasmi does not
  build for the X4's chip without patches; and the SDK would have to be
  redone as an interface of numbers and bytes.
- **Native code**: the SDK's sample plugin, unchanged, loaded from the
  card on both devices and registered like a plugin built in. On the X4
  Pro it ran from PSRAM (2.2 times faster than the firmware's own code
  from flash when the cache misses); on the X4 from flash mapped through
  the MMU, at 380 bytes of heap. Calls between firmware and plugin cost
  0.07 us, as calls within the firmware do.

PaperPaw is a free system: plugins should have as much room as
possible, also past the SDK, and speed and battery matter more than
isolation. The risks of plugins without a sandbox are accepted. Before
PaperPaw 1.0 a release may break code plugins; after 1.0 plugins must
survive minor releases.

## Decision

### Plugins are native code, one file per platform

- **No sandbox.** A code plugin runs as part of PaperPaw, with
  everything the SDK offers and everything past it.
- **One source, one file per platform.** A plugin's crate builds, from
  the same source against the same SDK, into:

  | Platform | Target | File | Loaded |
  |---|---|---|---|
  | X4 | `riscv32imc-unknown-none-elf` | `.ppn` | written once into flash, mapped through the MMU |
  | X4 Pro | `xtensa-esp32s3-none-elf` | `.ppn` | read into PSRAM, run there |
  | Android (Boox and others) | `aarch64-linux-android` | `.so` | ~~`dlopen` from the app's own storage~~ not loaded (amendment 2026-11) |

  The author's template repository and its CI job build all three; the
  catalogue lists one file per platform, and each device's store takes
  its own. The X4 and the X4 Pro come first; Android is in the format,
  the template and the catalogue from the start.
- **The `.ppn` file** holds the plugin's code and read-only data as one
  block, the offsets of the 32-bit words that hold an address, and a
  header: chip, plugin API version, sizes, its entry. Plugins there have
  no writable data of their own; their state lives on the kernel's heap.
  On Android the `.so` is an ordinary shared library: the system's
  dynamic linker relocates it, and it exports the same entry
  (`PAPERPAW_ENTRY`). The plugin's code does not change: one
  macro (`plugin!`) makes the entry, the heap and the panic handler on
  every platform.
- **Google Play forbids** apps from loading executable code that does
  not come from Play. A Play build of PaperPaw loads data plugins only;
  builds from the APK itself, F-Droid and Boox's store load code
  plugins too.

### Where it runs on the X4 and the X4 Pro

- The **X4 Pro** reads the block into PSRAM and runs it there (the MMU
  serves PSRAM to the instruction bus): no flash written, as many
  plugins as PSRAM holds.
- The **X4** writes it once into flash, into the free end of the app
  partition it runs from (behind the firmware's image: 2,273,280 bytes
  with PaperPaw's layout, a full flash by USB or the web flasher, about
  640 KB after an install by CrossPoint's SD installer), and maps it through free MMU entries, so its code runs from
  flash and only its data takes RAM. Updates are written into the other
  app partition, so plugins in use are never erased, and going back to
  the old firmware finds the old plugins behind it.
- Wherever it lands, the loader adds the same delta to the listed words;
  code and data keep their distance (memory mapped at data address A
  runs at A + `0x06000000` on both chips).
- **Unloading** is a restart: plugins are loaded at start only, and
  switching one takes `Command::Restart` (ADR 0004), a reset on the X4
  and a new process on Android.

### The interface, and how plugins survive updates

**Until PaperPaw 1.0: the SDK itself, tied to one release.** A plugin is
built by the same rustc, for the same target, from the same SDK (version
and features) as the firmware or app, and carries its own copy of the
SDK code it uses. Types, vtables and closures cross unchanged: the
plugin's entry returns a `Box<dyn Plugin>` that the firmware gives to
the kernel like any other. A small C table from the firmware gives the
plugin the kernel's heap, panics and the clock (`Api`, which grows only
at its end and says its size). A **fingerprint** of compiler, target and
SDK (version and features: Android builds the SDK with `outline` and
`color`, the X4 without; `kernel` and `unstable`, which only add
functions and modules, are left out) is in the firmware and in every plugin file;
only files with the firmware's own are loaded. Every release breaks
every code plugin until it is rebuilt, which is accepted before 1.0.

**Required before 1.0: a plugin API with its own version.** As the
Flipper Zero's firmware exports its API (`api_symbols.csv`, with a
major.minor version):

- **The firmware exports the SDK's functions** through a versioned
  table: plugin API `major.minor`. A minor version only adds to the end
  of it; plugins built for 1.x run on every 1.y with y >= x, without a
  rebuild. A major version means rebuilds and stays rare; ADR 0020's
  checks guard it (`cargo-semver-checks`, `api.txt`), and a CI job
  compares the table with the last release's and fails when an entry
  changed or went.
- **What crosses is C-shaped and stable within a major version.**
  Borrowed text as pointer and length; owned strings and vectors as
  pointer, length and capacity, rebuilt on the other side with
  `String::from_raw_parts` / `Vec::from_raw_parts` (one heap, the
  kernel's, so ownership moves without copying); the kernel's objects
  (`Env`, `Canvas`, `Registrar`, `Settings`, fonts) as opaque handles
  with functions in the table; plugin objects (`Plugin`, `Screen`,
  handlers) as `#[repr(C)]` tables of functions the SDK builds from the
  traits on the plugin's side. Plain types that cross by value
  (`Rect`, `Color`, `Version`, ids as text) are `#[repr(C)]`.
- **The SDK splits in two.** The part authors use stays Rust as it is
  (traits, generics, closures, `ui` helpers, compiled into the plugin);
  under it, a thin layer turns calls into table entries on the plugin's
  side and back into the kernel's types on the firmware's. What changes
  in today's SDK: `Env`'s public fields (about 25 references to kernel
  types such as `Settings`, `Translator`, `Fonts`) become methods;
  public structs that carry Rust types (`Cow<'static, str>` in
  `Manifest`, `ScreenSpec` and `RegisterError`, boxed closures in
  `ScreenSpec`, `ActionSpec` and `Registrar::on`) are converted at the
  boundary; `Canvas` crosses as a handle with its drawing primitives,
  and `ui`'s blocks draw on top of them inside the plugin; services
  found by `TypeId` need ids with versions instead.
- **Then the fingerprint is the plugin API's major version and the
  chip**, no longer the compiler: only C-shaped values cross, so a
  plugin may be built by any rustc. Should some Rust types still cross,
  the rustc is pinned for each major version.
- **What it costs**: the larger SDK work before 1.0, a few hundred table
  entries made by a macro from the SDK's own functions
  (`#[plugin_api(since = "1.2")]`) rather than by hand, conversions at
  the boundary (text and vectors move without copying; commands and
  specifications are converted once, at registration or per event:
  small beside drawing), and the discipline of only adding within a
  major version. Plugins get smaller in return: the SDK's large
  functions stay in the firmware.
- **Plugins that reach past the SDK** (the firmware's own functions,
  peripherals) are tied to one firmware build: their file says so, the
  store marks them, and they are rebuilt for every release.

**Every plugin file carries the version it was built for.** The loader
refuses one that does not fit (another fingerprint now, another plugin
API major or a newer minor later): the plugin stays installed and
switched off, its settings and data stay on the card, nothing crashes,
and the plugin manager says why.

### The store, updates and the web flasher

- **A code plugin is a data plugin folder** (ADR 0023: `plugin.ini`,
  translations, settings) plus its code for this device as
  `plugins/<id>/code.ppn` (on Android `code.so`, in the app's own
  storage). The list gives code plugins `"kind": "code"` and
  `latest.builds`: per platform (`x4`, `x4-pro`, `android`) the build's
  plugin API or fingerprint (`abi`), address, size, SHA-256 and
  the author's signature. A device takes the build that fits it.
- **Updates extend ADR 0022.** The update check knows the code plugins
  installed and the version each was built for; the server says which
  have builds for the new firmware; before anything is installed the
  device names those that would stop working (*Update anyway*, *Not
  now*). The server's answer lists, with each update, the builds for it
  of the code plugins the device has and of plugins built in now that the
  update has as files instead (`code`: id, version, author's key, files);
  once the update is downloaded, the device fetches them, checks each
  author, and puts them into `plugins/.<id>.next`, their code named as
  fetched (`native::remember_fetched`); the updated firmware installs
  those built for it before plugins are found, from the first start it
  loads code plugins at: an update's first start, on trial, loads none,
  so once it keeps the update (half a minute) it starts once more, and
  they run from then on, also offline. A fetch cut off by a restart goes
  on when the update page shows, as far as the room for code holds the
  plugins. Their settings and data on the card were
  never touched. A plugin that could not be fetched comes from the store
  after the update. The Android app does the same when it is updated.
- **The web flasher** writes the firmware and, behind its image in the
  same app partition, a bundle of the chosen builds with their folders'
  files, which the firmware installs onto the card at its first start.
  The same file serves the store and the flasher; nothing is built on a
  server.

### Recovery instead of confinement

- **Safe mode.** The firmware counts, in RTC memory (where the crash log
  already lives), starts that load code plugins and never reach their
  first frame. After two such starts it starts without code plugins. A
  key held at power-on starts without them too: *Back* on the X4,
  *Home* on the X4 Pro. Afterwards a notice names the plugin loaded
  last; the plugins are switched on again one by one, and one that
  crashes again stays off until it is switched on on purpose. On
  Android the count lives in the app's own files, and the app's
  launcher offers to start without code plugins.
- A panic's calls inside a plugin are written with the plugin's name
  and the offset into its file.
- A/B updates (ADR 0019) and, as the last resort, the ROM's download
  mode with the web flasher.
- **Signatures and permissions say, they do not confine.** No key
  lives on a server: the plugin library's list (over HTTPS) gives every
  file's hash, and an author signs their own files with their own key
  (Ed25519, in their own CI). A device remembers the author's key at the
  first install and refuses, in plain words, an update signed
  by another, so a taken-over server cannot swap a known plugin quietly.
  It also names the file installed by its digest (in `.paperpaw`, out of
  the Wi-Fi transfer's reach) and loads only that file, so one put into
  a plugin's folder otherwise (over the transfer, on the card) does not
  run in its place: the plugin manager says it was not installed, or was
  changed, and to install it again.
  A plugin's manifest declares what it uses (storage, network,
  secrets, other plugins' data), shown before installing; natively that
  is a promise checked by review, not a fence. A sandbox would not
  change that: a plugin allowed to read books and use the network can
  send them away either way.

## Consequences

- Community code plugins reach the X4, the X4 Pro and Android from the
  store and the web flasher without a firmware build; the X4 Pro and
  Android have room for very many, the plain X4 for a few (2,273,280 bytes
  in all with PaperPaw's layout, about 640 KB after a CrossPoint install,
  as measured on a device; the sample plugin alone is 70 KB today,
  less once the SDK's functions stay in the firmware).
- Plugins run at full speed and can do anything the firmware can,
  past the SDK too: their own drivers, the radio, the flash.
- A faulty plugin can crash the device or damage its data; a hostile one
  can read every secret and could even burn eFuses and ruin the device
  for good. Users install code plugins as they would install programs on
  a computer; the store says so, and lists who made each and its source.
- Until 1.0 every release breaks code plugins until they are rebuilt;
  authors rebuild with each release (their CI can do it on the SDK's
  tag), and the store and the update check make the gap visible. From
  1.0 on, plugins survive minor releases; the versioned plugin API is a
  blocker for 1.0.
- A Play build of the Android app has no code plugins.
- ADR 0004's "sandboxed WebAssembly modules (larger devices)"
  changes to native plugin files.

## Open points

What the spike left open, as the implementation found it (2026-10-20):

- **Services by type: did not hold.** The `TypeId` of an SDK type is the
  same in two workspaces built alike, but any difference in cargo's
  profile changes it (`debug = 2` against `"line-tables-only"`,
  `panic = "abort"`), so a plugin built apart did not find a service by
  it. Services were then published with their type's name
  (`Registrar::publish`) and found by it where the `TypeId` differs. A
  service now has a contract instead: its type names a key
  (`service_type!`), the plugin provides it with `Registrar::provide`,
  and others find it with `Services::get` by id, type and version; a
  service whose type changes takes a new key. A test loads a plugin built
  in a workspace of its own into a kernel and finds its service so
  (`crates/paperpaw-core/tests/native_plugin.rs`).
- **On the devices**: several plugins at once (packed one after the
  other, a sector each, on the X4), a plugin that panics at every start
  (two starts, then one without code plugins, the panic's calls named by
  plugin and offset, that plugin switched off), and plugins from the web
  flasher's bundle (installed at the first start, loaded at that start)
  run on the X4 and the X4 Pro. A key held at power-on is not tried on a
  device yet.
- **No mutable statics**: `paperpaw-sign statics` lists the writable
  symbols of the SDK's crates (built without LTO, whose archives hold
  objects); none today. It and the test above wait for a CI job.
- **Toolchains**: the sample pins the X4's compiler
  (`samples/hello-native/rust-toolchain.toml`); the firmware's are to be
  pinned by the release, which names them; the device shows them under
  *About*.
- **Android's `.so`.** Untried: `dlopen` of a library the app wrote
  into its own storage (apps for API level 29 and later may not run
  such files as programs, and loading them as libraries has to be
  checked on the Boox devices' Android versions); the library must be
  built against the same SDK features and NDK target as the app. `plugin!`
  builds a library the system loads already (the test above loads one),
  its table set by the shell.
- **Only the file installed runs.** The store and the firmware
  installing the bundle name a code plugin's file by its header's
  SHA-256 (`native::remember_code`, `.paperpaw/plugin-code.txt`); the
  kernel hands it to the shell's loader (`CodeFile`), which loads only a
  file with that header, its content checked against it where it is
  placed. A file never installed is refused (`PluginProblem::NotInstalled`)
  rather than confirmed on the device: copying a file by hand is an
  author's way, who names it over USB (`x4-remote.py --trust-code`).
  The loader is told the plugin's id from its `plugin.ini`, so safe mode
  and a crash name it by that, in whatever folder it is.
- **Author signatures** are a trailer in the `.ppn` itself
  (`paperpaw_native::signature`), so a file carries its signature
  wherever it goes; the store and the firmware installing the web
  flasher's bundle remember the author's key (one file for both,
  `native::remember_author`). From the bundle, code signed by another
  author than the version installed is not installed and the plugin
  manager says so (`Native::not_installed`). The store refuses it too
  (`Refusal::KeyChanged`): nowhere can the reader accept a new key.

## Amendment: Android loads no code plugins (2026-11, #228)

Android loads no code plugins: no `code.so`, of any plugin interface, is ever
opened, mapped or run there. This settles the text above, which spoke of an
Android `.so` as planned and "untried" and elsewhere said Android "does not load
`code.so` yet". The decision, taken with the interface-7 raise:

- **Superseded** are the Android row of the platform table, the passages on
  Android `.so` files, `dlopen`, and the Android store builds, the Android
  launcher's start without code plugins, the Android share of the room
  argument in the consequences, and the open point on Android's `.so`. Where
  they stand, they are history, not plan. A Play build had no code plugins
  already; now no Android build has.
- **Why.** The project decided it with the interface-7 raise; the open point
  below already noted that apps for API level 29 and later may not run files
  they wrote as programs and that loading them as libraries was untried. The
  interface-7 boundary (ADR 0034) is checked with the loaders that exist: the
  desktop's and the X4's.
- **What a user sees.** A plugin folder with a code file (`code.so`,
  `code.ppn`, `code-host.so`, ...) is listed in the plugin manager as not
  running (`PluginProblem::Unsupported`: this PaperPaw loads no code plugins);
  its data (translations, settings) is listed like a switched-off plugin's.
  Data plugins load as on every device. An `android` code build in the store
  tools or the catalogue has nothing to load it; removing that platform there
  is follow-up work of the store's owners, not part of this amendment.
- **How it is kept.** The shell installs no code loader
  (`shells/android/src/code.rs`); tests there fail if it does, and show a
  code plugin of each file name refused cleanly with a data plugin loading
  beside it.
