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
`paperpaw-ui-kit`. The interface-6 plugin is removed (see below); no shim stays.

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
- The interface-6 plugin is removed: `Keyboard`, `KeyboardScreen`, `rows_for` (the interface-6 form;
  `try_rows_for` stays), `draw.rs`, `choose.rs`, the interface-6 point reading of added layouts
  (`parse`, `added`) and the suites `typing.rs`, `presses.rs`, `multiline.rs`, `long_text.rs` and
  `size.rs`, with the app's `.plugin(Keyboard)` wiring (the keyboard does not show in the app until
  the host activates native plugins) and the desktop golden `boox-keyboard`. The wifi and store tests
  open a test-only stand-in for the dialog (`plugins/test-support/keyboard_dialog.rs`). Parity tests
  against the interface-6 registration became image-versus-builder and host-admission tests
  (`tests/abi7_declarations.rs`); the reading of added layouts is tested against stated results, not
  against the old `parse`. The test that held the script packs' `keyboard.layouts` lines
  (`tools/plugin-files/keyboards`) to the old reading went with it: those packs still bring
  interface-6 lines, and a pack that adds layouts through the stage is a later step.

## Tests

`native_model`: layout of the keyboard inside the screen (touch and buttons), shift and its lock,
automatic capitals, cut text, paste filters, layers and layouts, focus moves, variants, German
holds, and allocation-refusal sweeps. The image is what the declaration builders make, the host admits
the screens, linked entry, construction in the kernel's registered scope. `tests/acceptance_pending.rs`: ignored cases that
name what they wait for. No desktop golden: the dialog needs host input (H1/H2/H4).

- Layouts other plugins add (interface 6 point `keyboard.layouts`, now the stage
  `keyboard.layouts@1`): the schema is `stages/keyboard.layouts@1.def` (input `layouts.ask`: the UI
  language; reply: up to 3 `layouts.layout` records of `id`, `code`, `letters`, `variants`, at most
  3,200 bytes), the codec `src/stages.rs` (generated: `python3 tools/stage-codec.py stages
  src/stages.rs --plugin keyboard`), the declaration `declarations::stages`, and the reading
  `src/native_layouts.rs`. The dialog and the chooser run the stage (`stage.run`, metadata gains it)
  and take the rules of the old point: the id is also the translation key of the name (a layout
  without one shows as its id), at most 4 visible characters of code, 6 rows of 16 letters, 16
  variants a letter; an id that names a built-in layout, *Automatic* or one added before is left
  out. Tests compare the reading with the old `parse` on samples and edge cases, sweep every
  allocation, round-trip the largest answer through the codec, and pin the stage's declaration and
  bounds to the schema. **Differences:** a subscriber answers records, not lines (the variants are
  one field of `e=éèë` pairs, at most 512 bytes; a plugin gives up to 3 layouts a run and 16 are
  taken in all); where the host does not run stages (nothing subscribed, or not wired) there are no
  added layouts and the built-in ones work as before.

## Not ported yet (explicit; each is an ignored acceptance case)

- Text from a hardware keyboard or a remote control (interface 6 `type-text`) is taken as
  `event.text_typed` (109, `Event::TextTyped`): added to the field as if typed on the keys
  (characters that are no text left out, digits only for numbers), `done` confirms. It reaches the
  plugin only while a text question is on top (the host drops it otherwise, and the plugin ignores
  it when no dialog is on top). What needs the host is the dialog and the dispatch (H1/H4): the
  ignored case `keyboard-type-text`.
- Layouts and variants the fonts cannot draw (interface 6 `has_char`) are left out through
  `Font::covers` (`text.covers`, slot 39): built-in and added layouts whose letters (with their
  capitals) the fonts lack are not on the keyboard or in the chooser, and variants of added
  layouts they lack are dropped. Letters are asked in pieces of at most 256 bytes. A host that
  cannot answer (no slot, no font) leaves every layout offered, as before. Tested on what the
  query answers (`layouts.rs` tests); the host's own answer needs a font to ask.
- Right-to-left mirroring (title, hint, buttons, cursor; `ar` is a regression against Beta 3 until it is
  done): the kit's style has a direction now (`Style::rtl`, `flip`, `flip_x`, `text::label_in`); the keyboard
  does not use it yet.
- Wrapped lines of several lines are broken at spaces and a run of spaces is shown as one.
- Peak memory on the X4 is not measured (device gate open). The keyboard is built into the
  firmware, so there is no `.ppn` size to record.
- Repaint memory: `native_paint::lines` wraps the whole text on every repaint (the wrapped lines
  and the joined candidates copy it; up to about 4 bytes per text byte, so up to about 8 KiB for
  2,000 characters and 24 KiB for 6,000 characters of several lines) and the password dots take 3
  bytes per character. Bounded by `MAX_CHARS` / `MAX_CHARS_MULTILINE`, fallible, freed after each
  repaint; a reused, line-by-line wrap buffer needs a streaming wrap in the kit and is left for
  the X4 memory measurement.
