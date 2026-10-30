# Contributing to PaperPaw

Thank you for helping! This guide explains how to get started and what we
expect from contributions.

PaperPaw is in beta (0.5.0-beta.12). The plugin SDK may still change
before 1.0.

## Ground rules

- Be kind and constructive; see the [Code of Conduct](CODE_OF_CONDUCT.md).
- Everything in the repository is written in **English**: code, comments,
  documentation, commit messages, issues and pull requests.
- Discuss larger changes in an issue first, especially changes to the SDK
  (`crates/paperpaw-sdk`), which is a public API.

## Development setup

1. Install Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pulls
   in `rustfmt`, `clippy` and the embedded/WASM targets automatically.
2. Run the checks CI runs:

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
   rustfmt --edition 2024 --check shells/xteink/src/main.rs
   cargo build --release --target riscv32imc-unknown-none-elf -p paperpaw-app --no-default-features --features packs,store-plugins,diagnostics
   ```

   The last builds the kernel and every first-party plugin for the X4's
   chip (`no_std`). The firmware itself (`shells/xteink`) is a workspace
   of its own; its README says how to build it. CI also builds with the
   oldest supported Rust (1.88), compares each API file, builds the
   firmware and the Android app, fuzzes the parsers and checks licences.

   `tools/ci-local.sh` runs these and the other host-side checks of
   `.github/workflows/ci.yml` in one command, with `RUSTFLAGS=-D warnings`
   and 4 cargo jobs (`-j N` changes that): `tools/ci-local.sh` (fmt, lint,
   tests), `tools/ci-local.sh full` (everything below that can run on a
   Linux computer), `--only <group>` for one group, `--list` for the
   names. Each step's exit code is its own: the summary lists the failed
   steps and the skipped ones (a missing tool is a skip, not a pass).
   Not run there: the Xtensa firmware (X4 Pro, X4 Classic), the Windows and
   macOS emulators, the update path, the Android APK, the reproducible
   build, CodeQL. Run it before pushing a pull request: CI minutes are
   limited, and a draft pull request starts no CI jobs.

3. Try your change in the emulator:
   `cargo run -p paperpaw-emulator -- --profile xteink-x4`.
   A change to what a screen shows is checked by the snapshot tests of
   `shells/desktop/tests/snapshots.rs`, which say how to write the
   pictures anew when the change is meant.

## Tools

- **Emulators.** `shells/desktop` is the package `paperpaw-emulator`
  with the binary `paperpaw-emu`. It runs the kernel and plugins on an
  emulated device, in a window or headless. `--help` lists the options,
  and `--list-profiles` lists the devices. `shells/web` builds the same
  device model for a browser; see
  [docs/web-emulator.md](docs/web-emulator.md).
- **`paperpaw-dev`** (`tools/dev`) checks plugins on emulated devices:
  texts, layout, reach with buttons alone, memory and frame time, flows
  ([docs/plugins.md](docs/plugins.md#checking-your-plugin)):
  `cargo run -p paperpaw-dev -- check --plugin <id> --quick`.
- **`paperpaw-sign`** (`tools/sign`) signs firmware updates, makes
  signing keys, and packs and signs plugin files (`--help`).
- **`tools/plugin-files`** builds PaperPaw's own plugins that the X4s
  install from the store as files, for every release (its README). Each
  plugin has a version of its own: one that changed gets a newer one
  (CI checks it), or its commits say `Plugin-unchanged: <id>` where no
  reader sees the change.
- **`tools/xtensa-std`** builds and installs the X4 Pro's core libraries
  for code plugins (its README).
- **`tools/fontgen`** converts fonts into bitmap font packs (`.ppfp`) for
  small devices.
- **`tools/x4-remote.py`** drives an X4 over USB with firmware built
  with the `remote` feature: buttons, screenshots, files, the log, the
  heap (`--help`).

## Architecture rules

These keep PaperPaw small, portable and extensible. Reviews check them.

1. **The kernel contains no product features.** Screens, formats, themes and
   the like are plugins.
2. **Plugins depend on `paperpaw-sdk`**, never on `paperpaw-core` (a CI
   job checks it). They may use PaperPaw's library crates too
   (`paperpaw-sqlite`, `paperpaw-xml` and the like), as any community
   plugin can. First-party plugins have no special access.
3. **If a community plugin could not build a feature, extend the SDK first.**
4. **Core crates are `no_std`.** They must build for
   `riscv32imc-unknown-none-elf` (the X4's ESP32-C3, with a heap of
   256 KiB). Avoid hidden allocations in hot paths and never load whole
   files into memory.
5. **Namespaced ids** (`plugin-id.name`) for every contribution; `core.*` is
   reserved for the kernel.
6. **No panics on bad input.** Configuration, books and plugins can be
   broken; report a warning and carry on with defaults.
7. **The SDK is shared as native code** ([ADR 0024](docs/adr/0024-native-code-plugins.md)):
   until 1.0 a plugin file is built for a plugin interface and by the
   compiler that built the firmware ([ADR 0026](docs/adr/0026-plugin-interface-version.md)),
   its fingerprint says so, and Rust types cross unchanged. From 1.0
   only plain values are to cross (numbers, text, slices, handles), so
   new APIs take such arguments and keep no borrowed host state across
   calls.
8. **The SDK changes visibly and compatibly** ([ADR 0020](docs/adr/0020-sdk-compatibility.md)):
   a change to its public API, or to a crate whose types cross with it
   (`crates/paperpaw-sdk/interface/crates.txt`), comes with that crate's
   `api.txt` and `shape.txt` brought up to date
   (`python3 tools/interface.py`, with the nightly it names and
   cargo-public-api). What goes is `#[deprecated]` first and gone before
   the next release. What code plugins see is the plugin interface
   ([ADR 0026](docs/adr/0026-plugin-interface-version.md)): any change
   to a type that crosses raises `native::INTERFACE`, a new item alone
   does not, and `cargo test -p paperpaw-sdk --test interface` says which
   (`PAPERPAW_BLESS=1` writes its files anew). The SDK's features
   `kernel` and `unstable` add only functions and modules, never a
   field, variant or trait item: a plugin built without them runs with
   firmware built with them.
9. **The SDK is designed to grow without breaking**: a change to it
   follows [docs/sdk-design.md](docs/sdk-design.md), whose checks run
   with the workspace's tests.

Significant decisions are recorded as [Architecture Decision Records](docs/adr/).

## Code guidelines

How PaperPaw's code is written, beside the architecture rules. Where a
check enforces a rule, it says which; clippy and the tests run them with
everything else (`cargo clippy --workspace --all-targets`, `cargo test
--workspace`). The length limits are a ratchet: what was longer before
stays listed until it is changed, the lists only shrink, and nothing new
may exceed them. Existing code is not reworked for them in bulk.

1. **Functions of at most 100 lines.** Check: clippy's
   `too_many_lines`, the limit in `clippy.toml`. A longer one from
   before carries `#[expect(clippy::too_many_lines, reason = "...")]`;
   split it when you change it, and the `expect` goes.
2. **Files of at most 1500 lines.** Check: `tools/checks`. The longer
   ones from before are in `tools/checks/big-files.txt` with their
   length; none may grow, and one split below the limit leaves the list.
3. **No control characters in source files** but tabs and line ends:
   write `\0` and the like as escapes. Check: `tools/checks`.
4. **Errors on the user's data are never discarded.** Handle them or
   tell the user; where one truly does not matter, say why in
   `#[expect(clippy::let_underscore_must_use, reason = "...")]`. Check:
   the code that keeps user data denies `let_underscore_must_use`:
   storage (`paperpaw-fs`, `paperpaw-fat`, `paperpaw-sqlite`), the SDK's
   library, the reader, the files, sync (`kosync`, `koreader`), the
   statistics and settings. The check sees only `let _ = ...;`: an
   error dropped with `_ = ...;` or `.ok();` passes it unseen, so do not
   write those for user data either.
5. **Every `allow` and `expect` says why** (`reason = "..."`). Check:
   clippy's `allow_attributes_without_reason`.
6. **Names and paths are constants**, not the same literal written in
   two places: a file a plugin keeps, a setting's id, an action's.
7. **Memory is the limit on the X4** (a heap of 256 KiB): no hidden
   allocations in paths that run for every page or frame, nothing kept
   in memory while it is not used, large buffers allocated fallibly.
   Measure new features on the `xteink-x4` profile of the emulator. The
   memory tests (`shells/desktop/tests/memory.rs`) fail when opening and
   reading a book takes more than 85 % of the heap. The stack (about
   41 KiB) keeps 10 KiB or more unused: no large values built on it.
   A stable sort (`sort`, `sort_by`, `sort_by_key`) takes 4 KiB of it
   for any slice: use `sort_unstable*` where equal items cannot be told
   apart, else `sort_by_cached_key`. `tools/stack-frames.py` shows the
   biggest frames and the deepest chains of a firmware build.
8. **No panics on bad input**: see architecture rule 6.
9. **Comments in plain English**, written for readers whose first
   language is not English. English comes first everywhere, other
   languages in their translation files.
   - Short sentences: subject, verb, object.
   - Say the why directly. The code shows what it does; a comment says
     what it cannot.
   - At most one parenthesis in a sentence, and no aside inside another.
   - No em or en dashes: a colon, a semicolon, a comma or a new sentence
     does it.
   - Old comments are not rewritten in bulk; mend those you touch.
     Reviews check new comments against these rules.

## Adding a translation

The kernel, each plugin and the shells keep their catalogs in
`i18n/<language>.ini` (the kernel's in `crates/paperpaw-core/i18n`, the
Pebble theme's in `crates/paperpaw-app/i18n/pebble`, the shells' in
`shells/xteink/i18n` and `shells/android/i18n`). To add a language:

1. Copy each `i18n/en.ini` to `i18n/<language>.ini`.
2. Set `language`, `name` and `plural` in `[meta]` (see `PluralRule` in
   `crates/paperpaw-i18n/src/plural.rs` for the available rules).
3. Translate the messages; keep `{placeholders}` unchanged.
4. Register the catalog where the others are registered (the plugin's
   `register`, `r.add_translations(...)`) and run `cargo test`: the
   tests report missing, unknown and mismatched messages.

A language can also come as a data plugin, without changing PaperPaw
([docs/plugins.md](docs/plugins.md#plugins-without-code)).

## Adding a device

[docs/porting.md](docs/porting.md) says what a port to another device
touches and what it leaves alone, how the X4 Pro was added step by step,
the pitfalls met on the way, how to check a port on the device (the
diagnostics plugin's device check) and what a pull request for a new
device brings.

Emulator profiles live in `shells/desktop/src/profiles.rs`. Please state
where the specifications come from and mark values you could not verify.

## Commits and pull requests

- Use [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:` …), with an
  optional scope such as `feat(sdk): …`.
- Keep pull requests focused; include tests for new behaviour.
- Sign off your commits (`git commit -s`) to certify the
  [Developer Certificate of Origin](https://developercertificate.org/).
  By contributing, you agree that your contribution is licensed under the
  [Apache License 2.0](LICENSE).
