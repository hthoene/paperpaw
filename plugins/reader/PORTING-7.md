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

## The block assembler (view slice R1a, `native_block.rs`)

- Joins the fragments of `core.document` op 6 into the `Block` the layout takes: head fields as data
  (`Head`), text parts, runs, checkpoints, inline languages, annotations. Checked: the text is as long
  as the head said (at most 64 KiB), runs sorted, inside the text and on character boundaries, checkpoints
  in order, at most 2,048 parts and 15 languages. A refusal forgets the block; allocation is fallible
  and tested refused in turn. `pieces(&BlockFragment)` turns a fragment into the data the assembler
  takes (the fragment type has no public constructor, so the join is tested on pieces).
- Gaps: a picture block and a table row read as a paragraph of their text (images wait for `core.image`
  and the table records); the fragment-to-pieces step is covered by the hosted test of the view slice.
- Builds on the SDK Documents facade (PR #959, merged into this branch locally: the stack is #954,
  #959, this).
- Review remarks (PR #969), for the view: a text part that would take the text past the head's length
  is refused at once. A block is up to 64 KiB of text and 2,048 runs, checkpoints and annotations each
  (over 100 KB in all): too much for the X4's heap, so the view must read with lower caps by device class
  (measurement list), and a block refused as `NoSpace` is shown as a visible notice in the page, never
  cut silently. The view always advances past a block, also when its end is not after its start or when
  reading it failed (the next place is the block's end, else one further), or a page could repeat for
  ever. A checkpoint's text index is not checked against the text length (the layout clamps it).

## The document adapter (view slice R1b, `native_document.rs`)

- `HostDocument` implements the layout's `Document` over `Documents::block_at` for the length of one
  callback (it holds the callback's host); its read buffers (256 B ask, 4,096 B reply) are on the heap,
  made once. `read_block` joins the fragments, reads a block that changed again from its start (three
  times at most) and refuses one of more than 64 fragments; a section's end (`NotFound`) is `None`.
- Gaps: `section_len` is 0 (progress needs op 5 of the SDK facade); no table of contents or links
  through it yet (the facade has them: the TOC and links slices); the hosted end-to-end test of
  `block_at` (the host's real `core.document` with a format) comes with the view.

## The typesetter (view slice R1c, `native_typeset.rs`)

- `Faces<M>` is the layout's `Typesetter` over the host's four reading fonts (`role:reading`,
  `-bold`, `-italic`, `-bold-italic`; the host resolves and falls back at each call; four of the
  plugin's eight font handles). The size follows Beta 3's `ReadingFaces`: the book's percent first, else a heading's level (1.6,
  1.4, 1.2, 1.1, 1.0, 1.0 of the body size), else the reader's. The font comes from the style bits
  only. Body lines keep the reader's ascent and line height from the line spacing (`Faces::new`,
  scaled by the book's percent); a heading is at least 1.2 of its size high with the extra shared
  above and below. Widths of single characters are kept in a 256-entry table
  (direct-mapped, 4 KiB on the heap; one host call for each character not in it, and four of the
  plugin's eight font handles are held); a host that refuses a measure gives width 0 and sets `failed`, which
  the view turns into a notice. `Measure` is the seam: tests use a fake that counts the asks.
- Gaps: no kerning, small capitals, superscripts, letter spacing, the book's own fonts or shaped (Arabic)
  text widths yet; the base size comes from the reader's settings in the view slice.
- Review remarks (PR #972): `NotFound` ends a section only as the answer to the first ask of a block;
  after a fragment or a restart it is `Invalid` (a malformed block), so a section is never cut short in
  silence. The document's metadata (title, authors, language, direction) is not read into the adapter
  yet (the facade has it): the layout does not need it. X4 measurement list: the two read buffers (4.3 KiB),
  a block of over 100 KB (see the assembler's caps) and the frame of `read_block`.

## The page drawn (view slice R1d, `native_draw.rs`)

- `draw_lines` sets the words of each laid-out line one after the other (the layout's gaps and the
  line's word spacing), in the role of each run; `Ink` is the pen (`HostInk` draws with the host's
  reading fonts on the frame's canvas in one colour). The tests lay real pages out with a fake document
  and measure, and record what is drawn.
- `draw_run` is the one place that decides how a run reaches the host: `Font::render_text` today; the
  font weight, smooth text and text contrast (Beta 3's extra/tone) come in with Kernel's new `text.draw`
  slot (todo 430), a one-line change there.
- Gaps: lines of mixed direction are drawn in logical order; underline, overline, strike-through,
  super/subscript shifts, small capitals, pictures, rules (tables), annotations and the lines between
  inserted blocks are not drawn; no hanging punctuation.
- Review remarks (PR #986): characters that are not drawn (zero-width, direction marks, byte order mark) take
  no room and are left out of a word (copied only where one is present); a space is the font's own
  times the reader's word spacing (`Faces::word_spacing`, 0.5 to 2.0). Named, not changed: the pen
  moves by the advance the host drew while the layout summed the widths it measured (the same font,
  so they agree); all lines of a page are sent for each drawn band (Beta 3 skipped lines outside the
  clip: the host clips, the cost is on the first-page timing list for the X4); a page is drawn in one
  colour and each word is one host call, no per-word ink.
