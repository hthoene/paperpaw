# Interface-7 port

Version 1.1.2. Native plugin: `src/native_plugin.rs` (`NativeKeyboard`: the dialog of text
questions on the slot `core.keyboard`, and the screen that chooses the layouts), what typing is
without the host in `src/native_model.rs` (text, shift and its lock, automatic capitals, rows of
keys and where they lie, focus moves, variants), drawing as owned `Paint` operations in
`src/native_paint.rs`, layouts for the native plugin in `layouts::chosen_for`. `NATIVE_ENTRY`
(feature `linked`), declarations in `declarations.rs` built into the image by `build.rs`,
metadata in `native/` (regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def
--input native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`,
likewise `metadata.host.*`; services `core.device`, `core.registry`, `core.ui`). Drawing uses
`paperpaw-ui-kit`. The interface-6 `Keyboard` plugin, `draw.rs`, `choose.rs` and the suites in
`tests/` stay: `paperpaw-app` and other plugins' tests still link them until the host activates
native plugins; then they go together (no shim).

## Ported

- The dialog: the host opens the screen for a text question (hand-over `core.request`); it reads
  the question (`ui_question`: title, multiline, secret, mode, live) and the starting text (op 10),
  types on the keys, and answers (op 9 in parts of at most 3,072 bytes, op 11): *OK* or held
  Back = answered, Back = cancelled, live typing as outcome 2 (a refusal of a live update
  stops the live text, the final answer is whole).
- Keys as before: letters, symbols, digits (number mode), shift (twice in a row locks), delete
  (long press clears; Korean jamo are taken back one at a time), space, line break (several
  lines), layout switch, automatic capitals (plain text only), variants on a long press (with
  buttons: long confirm), German ä ö ü ß on their letters, the umlaut keys setting, key shape and
  size, the button focus and its column memory, the text field (end of the text with a cursor, the
  last lines of several lines), the hint of letters held or of how typing ends.
- *Paste* (clipboard, op 19) and *Phone* (a nested text question on `core.remote-keyboard`, op 8,
  with the text so far; never for a password; shown where the device has a network and the slot is
  filled). A password shows dots, makes the instance secret and holds the edges
  (`command.screen_policy`), keeps only Sleep, refresh and night mode, and is never put on the
  clipboard or sent to the phone.
- The layouts chooser (*On (2)* shows the place in switching; *Automatic* stands for itself) over
  the setting `keyboard.layouts`; the five settings, the group, the icon, the first-run step, the
  held-Back action `keyboard.done` bound to `long.back` on `keyboard.screen`, and the need of
  *Key shape* for a touch screen.
- Text, titles and en/de catalogs are unchanged.

## Tests

`native_model`: layout of the keyboard inside the screen (touch and buttons), shift and its lock,
automatic capitals, cut text, paste filters, layers and layouts, focus moves, variants, German
holds, and allocation-refusal sweeps. Declaration parity with the interface-6 registration,
linked entry, construction in the kernel's registered scope (refused as `Unsupported` until the
host admits every declaration family, #944). `tests/acceptance_pending.rs`: ignored cases that
name what they wait for. No desktop golden: the dialog needs host input (H1/H2/H4).

## Not ported yet (explicit; each is an ignored acceptance case)

- Text from a hardware keyboard or a remote control (interface 6 `type-text`): needs the host event
  `event.text_typed` (Kernel draft 22, event 109). `Board::typed` is ready for it.
- Hiding layouts the fonts cannot draw (interface 6 `has_char`): needs the host call
  `text.covers` (draft 22, slot 39). Until then every built-in layout is offered; on the X4 packs
  a layout in a script they lack would show question marks.
- Layouts other plugins add (interface 6 point `keyboard.layouts`; only tests use it): Kernel's
  answer is a plugin-defined stage `keyboard.layouts@1` (schema plus `tools/stage-codec.py
  --plugin keyboard`) that adding plugins subscribe to and the keyboard collects with `stage.run`.
  Not written yet; the built-in layouts work without it.
- Right-to-left mirroring (title, hint, buttons, cursor): the kit's style has no direction yet.
- Wrapped lines of several lines are broken at spaces and a run of spaces is shown as one.
- Peak memory on the X4 is not measured (device gate open). The keyboard is built into the
  firmware, so there is no `.ppn` size to record.
