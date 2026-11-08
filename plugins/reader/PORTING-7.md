# Interface-7 port (slice 1: declarations, last book, reading aloud rules)

Version 1.0.10. Partial: what the reader declares and the host-free rules it has. No native screen
and no `NATIVE_ENTRY` yet; the interface-6 `Reader` and everything of it stays until the host
activates native plugins (then it goes together, no shim). Owner decision 131 stands: the user's
reading settings (font, size, spacing, margins, justification, hyphenation, publisher styles) are
the kernel's settings (`core.*`); the reader declares none of them and adds no default over them.

## Ported

- `declarations.rs`: the eight settings (kinds, defaults, bounds, groups, help, the units, steps and
  the slider), the six screens and the slots they fill (`core.reader`, `.toc`, `.annotations`,
  `.footnote`, `.search`, `.skim`), the 29 actions, the 34 menu entries (selection bar, reader menu,
  font and layout panel, the *More* menu), the 13 icons, the device needs (speech, no touch,
  touch), the key bindings of the reader's layer (six always, two while *hold-chapters* is on, as
  records with a condition), and the events heard (start, a book moved).
  `tests/abi7_declarations.rs` compares every field with the interface-6 registration.
- `src/native_state.rs`: the book read last (`state.ini`, `[reader] last = …`), read and written as
  interface 6 does (quoting and escapes as `config::quote`), allocation fallible.
- `src/native_aloud.rs`: the timer's minutes, where a sentence ends and the text spoken from words
  (hyphenated words joined again), allocation fallible.
- `src/native_tests.rs`: both compared with the interface-6 code on fixed inputs and at random, and
  every allocation refused in turn.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- Everything that needs the host: opening a book, laying out pages and the page index (document
  host and formats, `DOC-open-layout`), positions and book data (`add_book_data` has no native
  declaration; `LIB-position`), the screens, input and dialogs (H1–H4), reading aloud (a speech
  facade) and the timers.
- **No native declaration exists yet for**: the status lines (the two places, the lines
  *progress* and *empty*, the items title, chapter, page and progress, and their settings:
  `status.size`, `status.edge`, the lines and parts, `battery-percent`), the sample widget above the
  reading settings (`previews(groups::READING)`), the reading app (`readers::ReaderSpec`), the screen
  hints (*on press*) and the first-run help, and the setup step *text size*. The setup step's title
  is empty in interface 6; the native record needs a title, so a text is to be decided (not invented
  here). Each is a gap in the declaration set for Kernel; the declaration test counts the status
  settings so they cannot be forgotten.
- **Owner ask #241 (the reading plugin extensible through plugins).** The native declarations must
  offer: the reader's stages (`stage.spec`: inserted blocks between lines with the insert ordinal in
  the locator, text style with the grant of the user's settings, an input filter), declared by the
  reader with the schemas of `paperpaw-reader-api` (PR #224, not on integration); the layout key
  with the subscribers' signatures; page numbers from the source text. Those wait for #224 and the
  stage host (`stage.run`). The key bindings above are the reader's layer as records; a plugin adds
  its own to `core.reader` with `key_binding`. I found no "logical-key table" in the docs, the
  attachments of #241 or the abi files; **to settle** with Manager/Kernel what it should contain.
- `font_step` (larger/smaller text) rounds to the next size that looks different on this device
  (`ui::shown_step`, setting specs): it needs the settings host and comes with the actions.

## Differences from interface 6

None in behaviour. `state.ini` and its form are unchanged (version 1 files are read; the path is
relative to the data folder).
