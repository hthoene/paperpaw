# Interface-7 port

Version 1.0.11. Native plugin: `src/native_plugin.rs` (`NativeProfiles`), settings
reads and changes in `src/native_data.rs`, the file of profiles and the values
without the host in `src/store.rs` (version 2 of `profiles.ini`, version 1 read,
names, tidying, value parsing). `NATIVE_ENTRY` (feature `linked`), declarations in
`declarations.rs` built into the image by `build.rs`, metadata in `native/`
(regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`,
likewise `metadata.host.*`). Drawing uses `paperpaw-ui-kit` (`ItemList` with a note
footer or the empty message).

## Ported

- The list (profile with *In use* and its number of settings, *Save as new profile*,
  the hint, the empty message), tap to apply, hold or the menu button to update,
  rename or remove (host questions with the original words and labels), *Next profile*.
- `profiles.ini` and `last.txt` in the plugin's data folder, byte-compatible with
  versions 1 and 2 (the version 2 test file is written back byte for byte). Files over
  one storage call (4 KiB) are written under another name and renamed over the old.
  Where the file cannot be read nothing is saved over it, as before.
- The settings are those of the reading, display and status line groups (and their
  subgroups) except folders, languages, *scope only* and *notice seconds*; applying
  changes only what differs and only values the setting accepts.

## Tests

`store`: round trip with odd names, version 2 byte for byte, version 1 upgrade, files
of another version, names, values by kind and bounds. Kit: item lists (held rows,
focus, pages), word wrapping. Declaration parity with the interface-6 registration
and linked entry.

## Acceptance pending on the host

No desktop golden. Native cases in `tests/acceptance_pending.rs` are refusals (never pass):
save new (H1, H2, H4), apply (H1, H4), hold options (H1, H2, H4), next (H1).

## Applying a profile

- A value a setting no longer accepts is skipped, as in interface 6, and the rest of
  the profile still applies: an integer out of its bounds, a word that is no boolean,
  a choice that is no longer one of the setting's choices (a removed font). Choices
  are read from the registry (`choices`, op 3) only for a value that differs from the
  current one. Tests: `native_data` (fake limits).
- Changes are staged one command each. A refusal (`NoSpace` over the stage's byte
  budget, or its command count: 8 per callback today, 64 with draft 27 / Core #484)
  stops the apply and the callback returns the error, so the host drops everything
  staged in it (`commands.md`: submit 1): nothing is applied, `last.txt` is not written
  and no "applied" note is shown. Test: `native_data::commit` returns `NoSpace`
  and runs no note (stage with a byte budget); the
  host side is part of the pending acceptance (H1).

## Not ported yet (explicit)

- The empty message has no icon (the stack icon): the kit draws no icons yet.
- Integer bounds come from one registry call per differing number when applying or
  checking *In use*; a profile with many differing numbers makes that many calls.
- Legacy `Plugin` registration and the app's `offered(.., Profiles)` stay until the host
  activates native plugins; then they go together (no shim).

## Robustness (review of PR #317)

- `core.ui` opens in the first live callback, not in `make` (construction has no
  frame); `tests/hosted_construction.rs` constructs in the kernel's real registered
  scope. Today the host refuses that construction with `Unsupported` because it
  admits screen declarations only (#310); the positive case is an ignored refusal.
- Every allocation on the callback paths is reserved first (`paperpaw_ui_kit::mem`,
  `text::concat`); allocator-failure sweeps (`plugins/test-support/fail_alloc.rs`)
  refuse each allocation in turn for the model/store code and the kit and require
  `NoMemory` and recovery. Views and dialog state that call the host are not covered
  by a sweep: that needs the host's UI service (H1-H4).
- `core.ui` calls use 4096-byte buffers (the schema's reply capacity). Registry
  readers allocate their buffers before opening the handle.
- Peak memory on the X4 (buffers, paint operations, rows) is not measured: device gate open.
- The encoded file is measured against the 64 KiB read limit before it replaces the card file;
  a refused save restores the list (`Undo`) and shows what the card holds. Names are read whole
  (at most 256 bytes). `profiles.ini` quoting and reading are local, fallible and tested against
  the `config` crate on arbitrary values. A name of invalid UTF-8 in `last.txt` is treated as no
  profile applied (it used to be read lossily).
- Rename (second review round): the card is read and both name copies are made before
  the profile is touched (`rename_in`), so a refused allocation leaves the list as it
  was; `last.txt` is compared as bytes (`is_last`): bytes that are not UTF-8 name no profile.
  Tests: sweep at every allocation of `rename_in`, a retry, malformed bytes.
- `profiles.ini` (Senior review): a leading UTF-8 byte order mark is skipped as interface 6
  did; setting ids are checked with the SDK's `Id::try_parse` (so `widgets.clock@2.face`
  is kept); a save first reads the start of the file and leaves a newer version alone
  (`may_save_over`). Tests: BOM, id and damaged-file round trips (`store.rs`).
- `files::replace` (ui-kit): a small write also removes a `name.new` an earlier cut left.
  The sequencing is tested against an in-memory volume with a refusal and a power cut at
  every step, at 4096 and 4097 bytes (`crates/paperpaw-ui-kit/src/files.rs`). The host's
  own power-cut behaviour of rename over an appended file (FAT) is the host's test, not
  the kit's: the kit assumes the host's atomic rename-over and states it.
- The interface-6 plugin is removed: `Profiles`, `profile.rs` (the legacy file format) and its tests, the kernel tests that pinned them (`apply.rs`, `names.rs`), the app's `offered(.., Profiles)` wiring and
  `STORE_PLUGINS` entry, and the plugin-file build (`tools/plugin-files` bin, `plugins.txt`).
  It does not run in the app until the host activates native plugins. The behaviour it pinned
  is covered by the model/store unit tests above; the golden stays for the native acceptance.
