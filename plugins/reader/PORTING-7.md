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
- `src/native_open.rs`: what a `core.reader` hand-over asks (`document.md` section 1, *Opening at
  a place*): `section` + `offset` (decimal `u32`, both or neither) win over `page`; a missing,
  single or non-decimal value is ignored, and a place the document lacks (`opening`'s check) falls
  back to `page`, else the saved place. Tested for both, neither, one and bad values. The
  optional `conversion` of the second paragraph of that section is not read yet.
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
  hints (*on press*) and the first-run help. The setup step *text size* is pending the host (op 22,
  Host todo 99); at P6 time its title reuses an existing reader title string, with no new text. Each
  is a gap in the declaration set; the declaration test counts the status settings so they cannot be
  forgotten.
- **Owner ask #241 (the reading plugin extensible through plugins).** The reader's stages
  (`stage.spec`: inserted blocks between lines, text style, an input filter, with the layout key
  and page numbers from the source text) wait for #224 (`paperpaw-reader-api`) and the stage host
  (`stage.run`). The key bindings above are the reader's layer as records; a plugin adds its own to
  `core.reader` with `key_binding`. No other contract is declared.
- `font_step` (larger/smaller text) rounds to the next size that looks different on this device
  (`ui::shown_step`, setting specs): it needs the settings host and comes with the actions.

## Differences from interface 6

None in behaviour. `state.ini` and its form are unchanged (version 1 files are read; the path is
relative to the data folder).

## The notes stage (todo 1937)

- `reader.notes-exported@1` replaces the event `core.notes-exported` (`core-events.md` 5): schema
  `stages/reader.notes-exported@1.def` (input `notes.exported`: `root` u32, `path` str 512, `title`
  str 256; reply one `notes.done` with an optional u32 `status`, 0 taken, which the reader ignores;
  at most 8 subscribers; `root` uses `core.files` numbering: 0 the reader's own data folder, 1 the user's files with
  the library and `Notes`, so a subscriber knows where `path` lives; the reader sends 1), codec `src/stages.rs` by `tools/stage-codec.py` (CI checks it is fresh),
  declaration `declarations::stages` (host admission and bounds tested in `tests/abi7_declarations.rs`).
- `src/native_notes.rs`: `input` (a title over 256 bytes is cut at a character, a path over 512 is
  refused, never cut), `run` (every subscriber once, answers and refusals not read) and `after_write`
  (the stage runs after a successful write only; a failed run changes nothing), tested host-free.
- The interface-6 `annotations.rs` still emits `core.notes-exported` (it has no stage host): the
  native annotations screen calls `native_notes::run(host, ROOT_FILES, path, title)` after its
  write, and the event goes with the interface-6 code at activation. readwise and joplin (Port E)
  subscribe to the stage. The metadata of the reader is unchanged.

## The stage `reader.word-looked-up@1`

- Replaces the event `core.word-looked-up` (`core-events.md` 5; asked for by the vocabulary port):
  schema `stages/reader.word-looked-up@1.def`, input `word.looked`: `word` str 1,024, `sentence` str 512,
  `path` str 512 (the book's file; never cut, a longer path is refused with `NoSpace`, the stage is not
  run), `section` u32, `offset` u32, then optional `language` str 35 (dropped when longer) and `title`
  str 256; reply `word.done` with an optional `status` the reader ignores; at most 8 subscribers.
- `native_words::run` asks every subscriber once and reads nothing of what they answer or fail with;
  the lookup is the user's. Declared in `declarations::stages` (host admission and the codec's bounds
  are tested).
- `native_words::announce` is the one place a lookup becomes a stage run (a test records one run with the
  right payload: word, sentence, path, section, offset, language, title; a path over 512 bytes is refused and
  nothing runs; `tests/native_words_alloc.rs` refuses every allocation of the input); `run` does it with the
  host's stage and gives every refusal back for the caller to ignore.
- Not wired yet: the native reader has no selection flow, and the interface-6 reader screen
  (`screen/selecting.rs`) cannot run a stage, so it still emits `core.word-looked-up`. That emitter stays
  until vocabulary (Port D) and readwise/joplin (Port E) subscribe to the stage; the native selection flow
  calls `native_words::run` with the same seven values (`reader-word-looked-up-run` in
  `tests/acceptance_pending.rs`, blocked on the stage host #828 and the flow).

## The stage `reader.filter@1`

- Replaces the interface-6 text filters (`add_text_filter`): schema `stages/reader.filter@1.def` (Kernel
  approved; input `filter.ask`: language str 16, format str 32, `image_only` 0/1; reply `filter.rules`
  (signature u64, bold 0/1, pictures 0..2, caption word str 96) then at most 32 `filter.pair`; at most
  4 subscribers, 3,840 bytes).
- Kernel's design: no host hook. The reader asks each subscriber once per book open, setting change or
  subscriber change; a reply that is invalid is dropped whole; the rules are applied to a whole block with
  `Block::try_transform` after the `core.document` block and before layout, subscribers in order (a dropped
  picture is not seen by later ones); the layout cache key folds in each subscriber's signature (in order),
  plugin id and version.
- Done here: the schema, the generated codec, the declaration and the test that holds the declared bounds to
  the codec. **Not done:** asking the subscribers and applying the rules (the native reader screens).
- Limits that meet: 32 pairs of 96 + 96 bytes (and the flag) are about 6.4 KiB, more than the 3,840-byte
  reply, so a setting at its maximum cannot be answered whole. The reply is refused by the encoder, and
  the subscriber (aids) must validate its setting against the reply size (named there as a settings
  validation); the reader drops a reply that is invalid whole.


## The native entry (slice 2)

- `NATIVE_ENTRY` (feature `linked`; `native-entry` for the packed form), `src/native_plugin.rs`,
  `build.rs` (the batches of `declarations.rs` built into the image: settings, screens, navigation
  with the six `slot.offer`s, keys, hold keys, events, stages) and `native/metadata*` (regenerate with
  `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
  native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).
- `make` submits six groups. **Not the events** (start, a book moved): the host's event admission
  (`native_host/events.rs` `is_event_tag`) lacks `event.book_moved`, and the registration is refused
  as a whole (`Factory(-1)`, found by `tests/hosted_construction.rs`); they are in the image and go
  in once the host admits it; an opened screen draws nothing
  (`Error::State`). **The plugin is not in `paperpaw_app::NATIVE_PLUGINS`**: with no view the
  screens it offers would be empty. The views come next: the annotations screen (model in
  `native_annotations.rs`), then TOC, skim, search, footnote and the reading screen with the
  document host.
- Still undeclared: the slice-1 list (status places, the sample widget, the reading app, screen hints,
  first-run help, the setup step). `tests/hosted_construction.rs` runs `make` in the registered scope
  of the kernel's native host.
- Metadata asks `core.ui` and no features yet; it grows with the views (storage for the last book,
  `core.library` for annotations, `core.files` for the export).

## Search (native model, `native_search.rs`)

- Host-free: the text looked for (trimmed, compared by lower case, whole characters, hits do not overlap,
  walked in place without collecting a block's characters), the snippet round a hit (24 characters before,
  48 after, marks `…`, white space as spaces) and its cut to the row (`with_hit`, over a `Measure` the screen
  backs with the font), the hits kept (100 at most, in the order of the book; more are counted as "100+"),
  and the background scan, `Search::step`, that reads 24 blocks of a book (`Blocks`, backed by
  `core.document` with the filters of the page applied) and keeps their hits. Allocation is fallible and
  tested refused in turn; a block whose hits could not all be kept is read again and none is kept twice.
- The text looked for counts its first 32 characters (`MAX_NEEDLE`), so a snippet is at most 24 + 32 + 48
  characters and the 100 hits stay under about 41 KB even in four-byte characters (typically 10–20 KB); this
  worst case belongs on the X4 list when the screen lands. A step that fails with an error other than the
  three that end a section (e.g. `NoSpace` on a block over the caps) leaves the scan where it was, and a
  screen that gets the same error again must end the search with a notice, not step for ever; as `step`
  does not return `first_new` after an error, the screen compares `hits().len()` before and after.
- Differences from interface 6, on purpose: a query longer than 32 characters is cut to them (so the text looked for
  can match more than was typed): the screen limits what can be typed to 32 characters, or shows the cut, so that
  the header never counts hits for text the user did not type; a block that cannot be read is skipped as an end of the section
  only for `NotFound`, `Invalid` and `Denied` (a refused allocation stops the step and is not taken for the
  end of the book); the matches are an iterator (interface 6 collected them); the hit's place is the
  `native_open::Place` the document host counts with.
- **Not yet: the screen.** It needs the document's block, progress and section ops (`core.document`, #2223),
  the font measures (`Measure`), the timer (`core.work`) that drives `step` (interface 6 woke itself every
  2 ms), the list, and, to choose a hit, going back and to the place with the match marked (the successor of
  `core.goto`, not declared yet). Choosing a hit is `Search::target`: where the match begins and ends.
