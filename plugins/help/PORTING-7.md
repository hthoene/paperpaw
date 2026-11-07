# Interface-7 port

Version 1.0.4. Native plugin: `src/native_plugin.rs` (`NativeHelp`), the hints'
geometry without the host in `src/layout.rs`, the `seen.txt` store in `src/seen.rs`.
`NATIVE_ENTRY` (feature `linked`), declarations in `declarations.rs` built into the
image by `build.rs` (settings, screens, events), metadata in `native/` (regenerate
with `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
--bytes native/metadata.bin --json native/metadata.inspected.json`, likewise
`metadata.host.*`). Drawing uses `paperpaw-ui-kit` (`Dialog`, `text::message`).

## Ported

- The hints over a screen the first time it opens: zones framed with their text in a
  box, the other hints and the two buttons in a dialog placed where it hides least; a
  tap or button closes, *No more hints* switches `help.show` off, a card that cannot keep
  what was seen switches them off too.
- Hints and the stack come from `core.ui` (`hints`, `stack`); a screen that waits for
  `ScreenReady` shows its hints then; no hints while the first-run setup is on the stack;
  another screen opened over the hints closes them and they show the next time.
- *Show hints again* (the `help.again` editor screen) clears `seen.txt` as it opens and says so.
- `seen.txt` unchanged (a screen id a line, version 1); settings `help.show` and `help.again`
  and their section unchanged; en/de texts unchanged.

## Tests

Unit: layout of zones and the box (`layout`), `seen.txt` lines, allocator-failure sweeps for
both and for the kit's dialog. Declaration parity with the interface-6 registration
(`tests/abi7_declarations.rs`: settings, extras, screens, the overlay flag), linked entry,
construction in the kernel's real registered scope (`tests/hosted_construction.rs`: refused
by the host today, see below). `tests/help.rs` still pins the interface-6 behaviour.

## Acceptance pending on the host

No desktop golden exists. The native cases in `tests/acceptance_pending.rs` are refusals
(never pass): first open (H1, H3, H4), switching off and again (H1, H4), covered by a
question (H1, H2, H3).

## Not ported yet (explicit)

- The button-hint row's labels while the hints are open ("Got it" / "No more hints" on
  Confirm): there is no native hint-row API (H3).
- The readable column on wide screens, right-to-left mirroring and the room of the hint
  row (dialog differences listed in `paperpaw_ui_kit::dialog`).
- Legacy `Plugin` registration (`Help`, `overlay.rs`, `seen`/`mark_seen` over `Storage`) and
  the app's `offered(.., Help)` stay until the host activates native plugins; the removal step
  deletes `src/overlay.rs`, the interface-6 half of `src/lib.rs` (`Help`, `SCREENS`, `explain`,
  `covered`, `seen`, `mark_seen`, `forget_seen`, `again`), `tests/help.rs` and the app wiring
  together (no shim).
- `core.ui` opens in the first callback; make has no frame. Peak memory on the X4 is not measured.
