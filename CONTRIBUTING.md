# Contributing to PaperPaw

Thank you for helping! PaperPaw is in beta (0.6.0-beta.4); the plugin SDK
may still change before 1.0. Be kind and constructive: see the
[Code of Conduct](CODE_OF_CONDUCT.md). Everything in the repository is in
**English**: code, comments, documentation, commits. The UI is English and
German. Discuss larger changes, above all to the public SDK
(`crates/paperpaw-sdk`), in an issue first.

## How a change lands

- **Team and agents:** branch from `origin/abi7/integration` (or from the
  unlanded branch the work builds on), check with the fastest check that
  catches your own mistake, push once and hand over with `merge_when_green`.
  The repository merges branches at once and runs CI afterwards on the
  base, once for all that landed; a red base is repaired by one assignment,
  and only conflicts come back to the author. No pull requests, and nobody
  starts or watches CI. Details: [AGENTS.md](AGENTS.md).
- **Outside contributors:** open a pull request against `abi7/integration`,
  keep it focused, include tests for new behaviour, and sign off your
  commits (`git commit -s`) to certify the
  [Developer Certificate of Origin](https://developercertificate.org/).
  You license your contribution under the [Apache License 2.0](LICENSE).

Use [Conventional Commits](https://www.conventionalcommits.org/)
(`feat(sdk): …`, `fix:`, `docs:` …).

## Setup and checks

Install Rust via [rustup](https://rustup.rs); `rust-toolchain.toml` pulls in
`rustfmt`, `clippy` and the embedded and WASM targets.

Build little. Check only what your change can break:

- `tools/check-local.sh [crate ...]` runs fmt, clippy `-D warnings`, the
  crates' tests, rustdoc, changelog, lockfile and licence checks and
  `paperpaw-checks` for the crates you name. Read its summary or exit code,
  not a marker line. `tools/ci-local.sh --list` names the larger CI groups
  (`--only <group>`); use them only when your change reaches that far.
  The `abi` group needs `ABI_BASE_REV` (the reviewed base commit) and
  `ABI_BASELINE` (its `abi/abi.lock`, from `git show COMMIT:abi/abi.lock`);
  the candidate's own lock is no baseline.
- A new plugin version or dependency changes every `Cargo.lock` that lists
  the crate: run `python3 tools/check-lockfiles.py`.
- A change to what a screen shows: the snapshot tests of
  `shells/desktop/tests/snapshots.rs` say how to write the pictures anew.
- Try a change in the emulator:
  `cargo run -p paperpaw-emulator -- --profile xteink-x4`.
- The firmware (`shells/xteink`) is a workspace of its own; its README says
  how to build it. Xtensa builds are slow: build once, at the end.

Limits CI enforces: `x4-classic` stays at or under 6,392,000 B (hard limit
6,400,000 B); the X4 stack keeps 2 KB or more unused (`tools/stack-frames.py`
shows the frames); a frame is at most 2,047 B.

## Tools

- `shells/desktop` (`paperpaw-emu`, `--help`, `--list-profiles`) and
  `shells/web` ([docs/web-emulator.md](docs/web-emulator.md)) are the
  emulators.
- `paperpaw-dev` (`tools/dev`) checks plugins on emulated devices:
  `cargo run -p paperpaw-dev -- check --plugin <id> --quick`
  ([docs/plugins.md](docs/plugins.md#checking-your-plugin)).
- `paperpaw-sign` (`tools/sign`) signs firmware updates and plugin files.
- `tools/plugin-files` (store release scripts) and `tools/plugin-files7`
  (PaperPaw's own plugins as store files): see their READMEs. Each plugin has
  a version of its own. Until 1.0 only published versions count: a plugin at
  its Beta 3 version gets exactly one patch bump, one already above it gets
  none; where no reader sees a change, a commit says
  `Plugin-unchanged: <id>`. Reviews check against the `v0.6.0-beta.4` tag.
- `tools/xtensa-std`, `tools/fontgen`, `tools/hyphenation-packs`,
  `tools/x4-remote.py` (an X4 over USB): see their READMEs or `--help`.

## Architecture rules

1. **The kernel contains no product features.** Screens, formats and themes
   are plugins.
2. **Plugins depend on `paperpaw-sdk`**, never on `paperpaw-core`. Library
   crates (`paperpaw-sqlite`, `paperpaw-xml` …) are allowed. First-party
   plugins have no special access.
3. **If a community plugin could not build a feature, extend the SDK first.**
4. **Core crates are `no_std`** and build for `riscv32imc-unknown-none-elf`
   (the X4, heap 256 KiB). No hidden allocations in hot paths, never load
   whole files into memory.
5. **Namespaced ids** (`plugin-id.name`); `core.*` belongs to the kernel.
6. **No panics on bad input.** Configuration, books and plugins can be
   broken: warn and carry on with defaults.
7. **The SDK is shared as native code**
   ([ADR 0024](docs/adr/0024-native-code-plugins.md),
   [ADR 0026](docs/adr/0026-plugin-interface-version.md)): a plugin file is
   built for one plugin interface and compiler. New APIs take plain values
   (numbers, text, slices, handles) and keep no borrowed host state across
   calls.
8. **The SDK changes visibly and compatibly**
   ([ADR 0020](docs/adr/0020-sdk-compatibility.md),
   [docs/sdk-design.md](docs/sdk-design.md)): a change to its public API, or to
   a crate listed in `crates/paperpaw-sdk/interface/crates.txt`, updates that
   crate's `api.txt` and `shape.txt` (`python3 tools/interface.py`). What goes
   is `#[deprecated]` first. A change to a type that crosses raises
   `native::INTERFACE`; `cargo test -p paperpaw-sdk --test interface` says
   which (`PAPERPAW_BLESS=1` writes its files anew). The features `kernel`
   and `unstable` add only functions and modules, never a field, variant or
   trait item.

Decisions are recorded as [ADRs](docs/adr/).

## Code guidelines

Where a check enforces a rule, it is named. The limits are a ratchet:
older code that exceeds them stays listed, the lists only shrink, nothing
new may exceed them.

1. **Functions of at most 100 lines** (clippy `too_many_lines`). A longer
   old one carries `#[expect(clippy::too_many_lines, reason = "...")]`.
2. **Files of at most 1500 lines** (`tools/checks`; old exceptions in
   `tools/checks/big-files.txt`, none may grow). Generated files are exempt.
3. **No control characters in sources** but tabs and line ends (`tools/checks`).
4. **Errors on the user's data are never discarded**: handle or report them.
   The data-keeping crates deny `let_underscore_must_use`; do not hide an
   error with `_ = ...;` or `.ok();` either.
5. **Every `allow` and `expect` says why** (`reason = "..."`).
6. **Names and paths are constants**, not one literal in two places.
7. **Memory is the limit on the X4.** No allocations per page or frame,
   nothing kept while unused, large buffers allocated fallibly. The memory
   tests (`shells/desktop/tests/memory.rs`) fail above 85 % of the heap.
   A stable sort takes 4 KiB of stack: use `sort_unstable*` or
   `sort_by_cached_key`.
8. **Tests:** a new test must fail without the fix. Never weaken, ignore or
   delete a failing test; the only known flaky one is `shared_table`.
9. **Comments in plain English** for readers whose first language is not
   English: short sentences, say the why, at most one parenthesis, no em or
   en dashes. Mend old comments only where you touch them.

## Changelog

Add `changelog.d/<issue>-<slug>.md` ([README](changelog.d/README.md)) for
each change readers or plugin authors can see; never edit `CHANGELOG.md`
(release preparation collects the fragments, see
[RELEASING.md](RELEASING.md)). Check with `python3 tools/changelog.py check`.
Without a visible effect: a `No-changelog: <reason>` trailer on the head
commit. Documentation-only changes are exempt.

GitHub actions in `.github/` are pinned by full commit SHA with the version
as a comment (`uses: owner/action@<40 hex> # v1.2.3`); a test fails on an
unpinned one.

## Adding a translation

Catalogs live in `i18n/<language>.ini` (kernel: `crates/paperpaw-core/i18n`,
theme: `crates/paperpaw-app/i18n/pebble`, shells: `shells/xteink/i18n`,
`shells/android/i18n`, plugins: their own `i18n`).

1. Copy each `en.ini` to `<language>.ini`.
2. Set `language`, `name` and `plural` in `[meta]` (rules: `PluralRule` in
   `crates/paperpaw-i18n/src/plural.rs`), and where they differ `direction`,
   `script`, `digits`, `calendar`.
3. Translate; keep `{placeholders}` unchanged.
4. Register the catalog (`r.add_translations(...)`) and run `cargo test`: it
   reports missing, unknown and mismatched messages.

A language can also be a data plugin
([docs/plugins.md](docs/plugins.md#plugins-without-code)): add
`tools/plugin-files/languages/<language>.ini` and a `[language-<language>]`
entry in `tools/plugin-files/data.toml` (copy an existing language's, with
`build = "tools/language-pack.py"`). `python3 tools/language-pack.py split`,
`merge`, `check <language>` and `build <language> <dir>` help; see its
`--help`. Adding a language needs no version bump of other packs.

## Adding a device

[docs/porting.md](docs/porting.md) says what a port touches, the pitfalls,
how to check it on the device, and what a pull request for a new device
brings. Emulator profiles live in `shells/desktop/src/profiles.rs`: state
where the specifications come from and mark what you could not verify.
