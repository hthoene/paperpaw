# Diagnostics on interface 7

Status: **complete.** Native only, no interface-6 code left; the X4 file (`diagnostics`) packs at 142,240 B (format 5, interface 7.1, no frame refused).

Version 1.0.9. Native plugin `src/native_plugin.rs` (`NativeDiagnostics`); the test
screen in `src/test_screen.rs` (layout and shapes without a host), the device check in
`src/check/` (`mod.rs` the steps and their decisions, `view.rs` the page as ops,
`reads.rs` what it reads from the host), the report and verdicts in `src/steps.rs`
(host-free). Drawing uses `paperpaw-ui-kit` (Style, Paint, header, text fitting).
Built-in descriptor `NATIVE_ENTRY` (feature `linked`), declarations in `declarations.rs`
built into the image by `build.rs`, metadata in `native/` (regenerate with
`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
--bytes native/metadata.bin --json native/metadata.inspected.json`, likewise
`metadata.host.*`). The interface-6 plugin and its Kernel tests are gone.

## Ported

- Test screen: gray ramp (one stripe per gray level, colours under it on a colour panel),
  four boxes (focus by left/right/up/down, check by Select or a tap), page buttons and the
  setting `diagnostics.pages`, progress bar, a held Select (`diagnostics.reset` bound on the
  screen) or a held finger starts over.
- Device check: ten steps and the end page, answers Fail/Skip/Pass with the one the
  measurement suggests, tools (flip, full refresh, start buttons, light again, Wi-Fi page,
  sleep now), the touch targets, the buttons asked in turn (a held wrong key gives the asked
  one up), light levels with the reader's own light kept in a file and set again, a 128 KiB
  card test with speeds, battery with charging seen both ways and the USB values, Wi-Fi
  radio on/scan/off again, sleep and the verdict after the wake, memory with the stack.
  Steps a device lacks are skipped with the same notes. en/de texts unchanged.
- Declares `uses.power` (sleep) and `uses.wifi` (radio); services `core.device`, `core.ui`.

## Differences from interface 6 (on purpose or forced)

- The report, the light file and the card test file live in the plugin's data folder
  (`.paperpaw/plugins/diagnostics/`), not in `.paperpaw/`. The report is a plugin's own file
  now; the texts that name it (`diagnostics.check.end.help`, How it works) say so.
- The clock page shows UTC date and time (the snapshot carries Unix seconds; the time zone
  is a setting of the host, not read here yet).
- The Wi-Fi tool is always offered; without the Wi-Fi plugin it does nothing.
- Time goes by `Tick` events (deadlines): the light levels, the 20 s search and the
  10 s sleep wait are as fast as ticks come.
  **TODO before activation: Kernel's answer on the tick rate a plugin with a screen gets
  (how often `Tick` arrives, and whether it stops while the screen is off). Fill it in
  here and check the 20 s search, the 10 s sleep wait and the light levels against it.**
- No gray view hint: the interface-6 `Screen::grays` (the gray page shown in the panel's
  four grays) has no native counterpart; the ramp is drawn in grays as before.
- Right-to-left mirroring of the answers row is not in the UI kit; the unit test of it
  went with the old code.
- Redraws are of the whole page (`queue_redraw` has no area).

## Not ported yet (explicit)

- Everything that needs a host running native plugins end to end: see
  `tests/acceptance_pending.rs` (ignored; each names what is missing). The host does not
  admit menu entries, needs, icons and key bindings yet (`tests/declarations.rs` pins
  that these are the only declarations that wait).
- The app (`paperpaw-app`, feature `diagnostics`) no longer registers the plugin, and the
  central wiring that loads linked native plugins does not list it yet. Until it does, the
  desktop emulator's Developer settings page has no *Test pages* row (the
  `boox-developer-settings` snapshot was blessed without it). The row, the screens and the
  report return with the wiring; bless the snapshot again then.
- Peak memory on the X4 and the packed `.ppn` size are not measured.

## Tests

`src/test_screen/tests.rs` (layout fits, stripes, colour hues, checked box, focus border,
progress), `src/steps.rs` (report round trip, verdicts, buttons), `tests/declarations.rs`,
`tests/linked_entry.rs`.
