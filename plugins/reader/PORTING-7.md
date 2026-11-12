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
- **No native declaration exists yet for** the screen hints *on press* and the first-run help's wait
  for the book (both SDK/host gaps; the help hints, the setup step *text size* and the sample widget
  are declared since #257, see *The first run* at the end). Each is a gap in the declaration set;
  `tests/abi7_declarations.rs` fails when the host admits the wait, so it cannot be forgotten.
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
- Wired in R4: the native selection flow (`native_marking/act.rs`) runs the stage when words are looked
  up. The interface-6 reader screen (`screen/selecting.rs`) cannot run a stage, so it still emits
  `core.word-looked-up`; that emitter stays until vocabulary (Port D) and readwise/joplin (Port E)
  subscribe to the stage and the native reader is default-on.

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


## The stage `reader.decorations@1` (schema approved by Kernel; todos 338, 448)

Replaces the interface-6 decorators (`add_decorator`, `PageText`, `Decorated`) and the event
`core.decoration-tapped` (Kernel ruling 334: a reader-defined stage, `document.md` 9). Schema
`stages/reader.decorations@1.def`, codec `src/stages.rs`, look codes `src/native_marks_codes.rs`.
Declared (`declarations::stages`) and fired by the reading screen: `src/native_decorations.rs`
(runs, fires, checks, the marks kept; `draw.rs` draws them through `native_marks`, the
reader's own highlights' drawing, so they look as Beta 3's decorations did) and `src/native_view/decorations.rs`
(the stage run at a page's first draw, the tap). Open, against Beta 3:

- **Not on the X4 yet:** the host refuses `stage.run` there until the stack gate of nested
  callbacks passes (`native_host/stage_run.rs`), so no marks show. The reader asks with empty
  buffers first (refused there before any buffer is looked at) and takes that as "no
  subscriber" for the book: nothing is encoded or allocated at a page turn on the X4.
- **Asking whether anyone subscribed:** no call tells it, so the first fire of a book finds out;
  with none the book is not asked again until the screen is back on top (a plugin may have been
  switched on). That is one encoded fire and one reply buffer a book, never one a page. A device
  without stages is remembered for the session (Kernel: only a firmware update changes it).
- **When (#248):** never in the open. A book's first page is asked in a step of the background
  work after it is shown (the step that counts the pages; `View::marks_step`) and drawn again
  only where it got marks; later pages at their first draw.
- **Not yet:** the ink of a look (text in another colour; backgrounds, lines, struck lines and
  frames are drawn), choosing a mark that takes taps with the buttons (as links are), and a log
  line for marks dropped (counted in `Decorations::dropped`; the native SDK has no log).

- **Asking a page.** Only where a plugin subscribed (none: nothing is encoded, no buffer is
  taken). Once a page is laid out, and again when the reader screen comes back on top (a word
  added in a lookup shows), the reader fires one head record `decorate.page` (`path` str 512,
  the library's path limit; `language` str 16; `runs`: how many run rows follow, which must equal
  the rows sent, else the fire is invalid) and `runs` rows `decorate.run`. A run is `section`
  and `offset`, the place of its first character in the document's anchor units (the same
  section and offset as annotations and saved places), and `text` (str 3,200): the page's text
  of that section from there, words apart by one space, a word broken over two lines once, whole.
  `text` is normalized and is no anchor: a mark's place is its run's place plus its byte range.
  A section's text over 3,200 bytes is split at a word boundary into several runs of the same
  section, each with its own `offset`; a mark never crosses runs. A page over the stage's 3,840
  bytes is asked in several fires of whole runs, at most 32 runs a fire. The answer is kept with
  the page; turning back to a page asks again (nothing kept between pages, #248).
- **Looks and marks.** A subscriber replies at most 64 rows: `decorate.look` rows, each a `look`
  number of its own (0 to 63), its `role` (str 64) and the look it resolved from its own role
  settings, then `decorate.marks` rows, each a `look` and `ranges` (bytes, at most 3,600): packed
  marks of 6 bytes, little-endian `run` u16, `from` u16, `to` u16 (byte offsets in that run's
  `text`), about 590 marks a fire. Checks: a `ranges` whose length is not a multiple of 6, or
  whose `look` the same reply did not define, drops its row; a triple with `run` at or past the
  runs sent, `from` >= `to`, `to` past its run's text or a bound not on a character boundary is
  dropped alone, its neighbours stay. Where marks meet, the subscriber answered first wins an
  aspect both set (Beta 3's order).
- **Memory (#248).** The reader keeps at most 256 marks a page over all subscribers, packed as
  they came (about 2 KiB at most); past them the rest is dropped in the order the subscribers
  answered, and counted. A look is kept only while a mark uses it. Marks are decoded
  straight from the reply buffer, never into a list of rows. The X4 page-turn peaks (TXT, EPUB,
  one subscriber, `--heap-report`) are measured before and after the reader side: input and reply
  are about 7.5 KB at a turn.
- **Roles are the subscriber's own.** A `role` must be `<subscriber id>.<name>`; a look with any
  other role is dropped with its marks, so no plugin styles another's marks or takes its taps.
- **Look codes** (fixed, never the order of `Tone::ALL` or `LineKind::ALL`): `background`,
  `ink` and `line_tone`: 0 none (the ink: the text as it is), 1 light gray, 2 gray, 3 dark gray,
  4 black, 5 white, 6 yellow, 7 green, 8 blue, 9 pink, 10 orange, 11 red; `line`: 0 none,
  1 solid, 2 dotted, 3 dashed, 4 wavy, 5 double; `thickness` in tenths of a millimetre, 0 the
  default, at most 20; `flags` bit 0 frame, bit 1 struck, bit 2 the subscriber takes taps on its
  marks, other bits 0. A tone code a reader does not know draws gray, a line code solid.
- **Taps.** A tap on a mark whose look has bit 2 (or choosing it with the buttons) fires the
  stage again with one record `decorate.tapped` (`role`, the mark's `text` str 256, cut at a
  character boundary, `path` str 512, start and end place in anchor units). Only the
  subscriber whose id is the role's plugin acts; it replies one `decorate.taken` (`taken` 1)
  and opens what it shows itself. The reader reads `decorate.taken` only from that
  subscriber; no answer or `taken` 0 and the tap goes on as a tap on the page.
- **Bounds.** At most 4 subscribers; input and reply at most 3,840 bytes each; the reader holds
  one reply buffer of that size while it fires and nothing between fires (#248). A subscriber's
  bad reply (framing, a tag it should not send) drops that subscriber's marks for the page only.

## The native entry (slice 2)

- `NATIVE_ENTRY` (feature `linked`; `native-entry` for the packed form), `src/native_plugin.rs`,
  `build.rs` (the batches of `declarations.rs` built into the image: settings, screens, navigation
  with the six `slot.offer`s, keys, hold keys, events, stages) and `native/metadata*` (regenerate with
  `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
  native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).
- `make` submits seven groups, the events too (the start and a moved book have no handler yet; the host admits `event.book_moved` now). At this slice an opened screen draws nothing
  (`Error::State`). **The plugin is not in `paperpaw_app::NATIVE_PLUGINS`**: with no view the
  screens it offers would be empty. The views come next: the annotations screen (model in
  `native_annotations.rs`), then TOC, skim, search, footnote and the reading screen with the
  document host.
- Still undeclared: the screen hints *on press* and the help's wait (see *The first run*); the status
  places, the help hints, the setup step and the sample widget are declared since.
  `tests/hosted_construction.rs` runs `make` in the registered scope of the kernel's native host.
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
- `draw_run` is the one place that decides how a run reaches the host: `Font::draw_inked` (slot 41),
  with the ink of `native_look::Look` read from the settings once per draw: `core.font-weight` 0-4 →
  extra pixels (`weight × size / 48`, rounded, at least 1), `core.text-contrast` and
  `core.text-sharpness` → the tone curve (soft a step lighter, sharp a step darker and without grays),
  `core.text-curve` (the entry without a script) replacing both, `core.smooth-text` off → sharp except on a
  two-gray panel (Beta 3's `ink`). Gaps: a `core.text-curve` longer than 256 bytes is read as no curve of its own (the contrast is used); the per-script entry of `core.text-curve` is not used (the view
  tells no script yet); the per-character host call is uncached; X4 flash cost is not measured.
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

## The reading screen (view slice R1e, `native_view.rs`, `native_pages.rs`)

- `core.reader`'s screen (token `SCREEN`): on open it reads the hand-over (`path`, `root`, `section`,
  `offset`, `page`), opens the book through `core.document` (publisher styles on; root 1, 0 or the handed
  file 5), reads the section count, and starts at the place the hand-over names if the book has it
  (`native_open::opening`). Preparing, failed, not ready and "no book" are screens that say so (the
  Beta 3 texts `reader.preparing`, `reader.error`, `reader.not-ready`, `reader.no-book`; out of memory
  and no reading font too). The document is closed with the screen.
- Drawing: the reading settings (`core.font-size`, line and word spacing, margins at the sides and at
  the ends, justify) and the display give the content box (`native_pages::content_box`; a margin that
  leaves less than 16 px is a screen that says "no memory"-class error, not a page), the page is laid
  out once (`layout_page` over `HostDocument` and `Faces`) and drawn on white with `draw_lines`; four of
  the plugin's eight font handles are held for the length of a draw or a back-layout.
- Turning: the actions `core.next-page`/`core.prev-page` and the arrows (the keys, edge swipes and
  tap zones are the host's input mapping). On: the start of the page is kept in a trail of 64; at the
  end of the book nothing happens (the end-of-book hand-over is the progress slice). Back: the trail,
  else the page before is found by laying pages out from the section's start (at most 4,096), as the
  trail is empty after opening in the middle.
- Metadata now asks `core.document` and `core.ui` and the feature `uses.storage` (root 1); the host
  calls `services.query` added.
- **Not yet (named gaps):** the place saved last time and `page` numbers (`LIB-position`, R2); progress,
  the page index and status lines (R2, op 5 sections in #973); the menus and the font/layout panel (R3);
  annotations (R5); TOC and skim (R6); search (R7); reflow on
  a turned screen and on a changed setting (the page is laid out again only after a turn); a document
  that is still preparing is not read again when the host says it is ready (`document_changed`); the
  paragraph indent and spacing, widows and orphans, hyphenation and the book-style switches of the
  layout `Style` (defaults only); pictures and tables (see the assembler); a hosted end-to-end test
  (open a book of a real format plugin, draw, turn) waits for DocHost's root 5 and a format plugin in
  the hosted harness; first-page timing on the X4 is not measured.

### R1e review notes (#1000)

- The handover read turns an out-of-memory error into "no book" (a named gap).
- One input callback can lay out up to 4,096 pages going back: an X4 timing gate, not measured.
- Settings and fonts are read on every draw band (cost not measured on X4).
- `uses.storage` is declared but no call uses it in this slice (R2 saves the place).
- Whole-page books (xtc, images) use margins of 0, as Beta 3; a margin setting of 0 is 0 mm.

## The saved place and progress (view slice R2a, `native_place.rs`)

- The sections' lengths are read at open through `core.document` op 5 (`Progress::read`, pages of the
  facade; at most 4,096 sections, 16 KiB; more, or a refusal, is no progress); progress before a
  page's start is the source text before it over all of it (N5 rule 1, as Beta 3's `Document::progress`),
  in millionths, integer arithmetic (no float on the X4).
- The place saved last time is read through `core.library` (`position`, op 13) when the hand-over names
  no place; it is used if its section exists and its conversion is the open document's (or none was
  stored). Else the start of the book.
- It is written (`set_position`, op 14, with progress and the conversion) when the screen closes and,
  while pages are turned, at most once in 5 seconds (Beta 3's `SAVE_EVERY_MS`), by the host's monotonic
  clock; a refused write is tried again at the next turn or the close.
- Metadata asks `core.library` now (both `.bin` pairs regenerated).
- A refused write says `reader.not-saved` once for a book (`command.notify`, `announce` when it was the
  close that failed); `updated` is the snapshot's wall time, 0 only where the device has none.
- The quiet-time write (Beta 3's 2 s after the last turn): `core.work` `declare` with the key `place`
  and `after_ms(2000)` at every turn (a key declared again replaces the work and restarts the time);
  when it is due, and at `event.suspend` and `event.stopping`, the place of every reading screen not
  saved is written. The host answering `Unsupported` for work (not wired) leaves the write to sleep,
  stop and close. The events image is submitted now (the start and a moved book have no handler yet).
  Metadata asks `core.work`. Tests: the declarations (five events), the hosted construction, the
  tokens; the writing itself needs a library host in the harness (named above).
- **Not yet:** the KOReader
  pointer written with the place; a saved place of another conversion is not mapped (the start is
  used); a `page` number of the hand-over (needs the page index); a hand-over `root` other than the
  library's for the saved place's path; the status lines and the page index; the hosted end-to-end test
  of save and restore (needs a library host in the harness). Progress is not shown yet.
- Review remarks of #1019, named: a close without a turn does not write the place (Beta 3 forced it;
  here it would overwrite a place that could not be read); a stored place that cannot be read means
  the start, and the first turn overwrites it (Beta 3 retried the read and set the old file aside); a
  book of more than 4,096 sections writes progress 0; a book outside the catalog gets NOT_FOUND on
  read and write, so it is "not saved" (said once).
- **The gates before the reader goes into `NATIVE_PLUGINS`** (Port Reviewer 2) are done: a read of the
  stored place that fails other than as "no place" is tried once more; a place is written only after a
  turn (so an unreadable one is not overwritten before the reader reads on; Beta 3's setting aside of
  the old file is not ported); the progress stored is written again where it is unknown. **X4 timing list:** the extra
  `work.declare` at every page turn. The quiet time is started again on a turn only (as Beta 3), and a
  host without work is asked once.

## The reader's report (`core.document` op 22, todo 455)

- On open and after every turn the screen lays out the page on show (if it is not yet: the draw then
  reuses it) and calls `Documents::report` with the place, the progress (the stored one where the
  book's sections are too many), the title (the book's, cut at 256 bytes on a character boundary) and
  `finished` once, when the page ends the book (progress at its end 0.999 or more, as Beta 3's
  `at_end`). The host raises `book_opened`, `page_turned`, `book_finished` and `book_closed` from it; the
  reader raises none itself. A refusal or a page that cannot be laid out is no report.
- **Not told yet:** the chapter and `chapter_left` (the table of contents: R6), `page`/`pages` (the page
  index), a report for a page seen again after a setting changed (no reflow yet), and the end of a
  one-page book is told with its open. The page is laid out in the input callback now (it was at the
  draw): on the X4 timing list with the back-layout.
- A report carries the path where the format gives no title (as Beta 3), and 1,000,000 once the book was
  read to the end, also when the reader pages back. **Named:** a book of more than 4,096 sections has no
  progress, so it never raises `book_finished`; a report of a screen that is not topmost is dropped by
  the host, so a `finished` set locally may never be raised (a one-page book too).

## Selection, words, links and footnotes (R4, #243)

- `native_wordbox.rs` measures the words of a laid-out page the way `draw_lines` sets them (same spaces,
  same runs); `native_selection/` is the host-free movement of the chosen words; `native_marking.rs` (with
  `bar.rs`, `act.rs`, `linking.rs`) is the controller: touch (long press chooses, tap extends or
  presses a bar button) and keys (`reader.select-text`, `reader.links`), the bar (look up, copy,
  *Extend* on buttons, *Cancel* on touch), the frames, the clipboard and the lookup (slot `core.lookup`,
  then the stage). `native_links.rs` reads the links of a page, finds a target, keeps the trail for *Back*
  and the text of a note; `native_footnote.rs` is the note over the page (an overlay screen, `core.footnote`);
  *Go to the note* is carried by the plugin, which follows it in the reading screen when that is shown again.
  `native_view.rs` asks the marking first about every input and draws its overlay last.
- **Differences from Beta 3, named:** the bar has fixed items (look up, copy, extend/cancel): *Highlight*,
  *Note*, *Read aloud* and *Translate* and the plugins' items need the annotations (R5), the aloud and the
  selection-bar contract, which the stage does not replace. The book's language is not read (native metadata
  has none), so the lookup is asked with an empty language. Words on a line set for mixed directions
  (`LineBidi::REORDER`) are read in logical order, not where drawn. At most 2,048 words of a page are
  chosen or hit. Taking the lookup of a word to a plugin other than `core.lookup` is not changed.
- Footnote popup: share of the display and buttons as Beta 3; *Close* by tap anywhere outside the buttons.
  The setting `core.footnotes` decides popup or jump as before. Width, text size and button rows follow the
  ui-kit `Style`, so pixels differ from Beta 3 (measure on the X4 at activation).
- Tests: movement and bar (`native_selection/moves_tests.rs`), word boxes with the allocation sweep,
  links (`native_links/tests.rs`), the marking (`native_marking/tests.rs`). Not tested without the host: a
  hosted touch flow over a real book (waits for the hosted harness, as the reading screen).
- The marking never opens a `core.ui` handle: the plugin's held one is in `Ctx` (copy, lookup, links and
  the note use it). Out-of-memory in the frames of a selection leaves the rest of the frames out. **X4 list:**
  up to 2,048 words per page (~100 KB worst case), 16 KiB of copied text, two service buffers per open, and
  `line_boxes` is quadratic in the words (measure at activation).

## Table of contents (native model, `native_toc.rs`)

- Host-free: headings as entries (title cleaned and cut at 120 characters, levels from the highest,
  at most 500), the kept copy `headings.txt` (form of interface 6, valid while the book file keeps its
  size), preferring the headings when the book's own table has at most one entry, the focus on the
  chapter of a place, the scan's step past a block and the `section`/`offset` parameters. Allocation
  is fallible and tested refused in turn.
- **Not yet: the screen.** It needs the block and table-of-contents ops of `core.document` in the SDK
  (Services, #2223), the storage for the kept copy, and the native reader entry (#954). Choosing an
  entry goes back and goes to the place through `core.goto`'s successor, which is not declared yet.
- Review remarks (PR #958), for the screen: the kept copy is read up to `MAX_KEPT` (80 KiB, as
  500 headings of 120 characters need) and written only after a complete scan; a scan that ran out of
  memory writes no mark; a refused write of the copy stays best effort. `kept_text` builds the whole
  text in one `String` (about 70 KB at the limit): its size on the X4 is on the measurement list
  (pieces through a sink if it does not fit).

## The table-of-contents screen (view slice R6a, `native_toc_view.rs`)

- The action `reader.show-toc` (menu, key) opens the screen `reader.toc` with a hand-over of the book
  on show (`path`, `root`, `section`, `offset`). The screen opens the book itself (its own document, closed
  with the screen), lists the book's own table (`core.document` op 7, at most 500 entries, titles cleaned
  and cut as `native_toc::title_of`, indented by level down to the 4th), puts the focus on the chapter the
  reader is in (`current_of_table`, exact targets by op 8 where the host's place is approximate) and says
  `reader.no-toc` / `reader.preparing` / `reader.error` where there is nothing to list.
- A choice (tap, press) goes to the entry's exact place in the reading screen below (`View::go_to`: the
  page left is kept to turn back to; the place is written as a turn's and reported to the host when the
  reading screen is shown again, as the host drops the report of a screen below the top) and closes the
  screen. Both screens are this plugin's, so no hand-over carries the place back.
- **Not yet:** the headings found by scanning blocks where the book's table has at most one entry
  (`native_toc`'s model: the scan, the kept copy `headings.txt`, the "Finding the chapters" screen), the
  chapter of the page for the status line and the report, next/previous chapter actions, and the
  skim screen (R6b). The hosted end-to-end test waits for a document host in the harness.
- The screen reads the reading screen's document (`View::doc_id`), never opening a second one: the X4 holds
  one open document per plugin. **X4 gates (Port Reviewer 2):** a list of 500 entries holds its titles twice
  (the entries and the items), about 100-190 KB of the 256 KiB heap (pieces or a paged read if it does not
  fit); finding the focus asks `toc_target` once per approximate entry of the current section; a
  `read_handover` error is "no book".

## The font, justify and chapter actions (view slice R3a, `native_actions.rs`)

- `reader.font-larger` / `reader.font-smaller`: the reading size a point more or less within 6-40 (the
  setting's range; a font pack may allow less: the host fits it back; Beta 3 stepped to the next size that "looks different" by the setting's own spec, which
  a native plugin cannot read: named). `reader.toggle-justify`. Both change the setting by command; the
  page is laid out again by the setting-changed reflow (Port E, #1064), until it lands the change shows at
  the next layout.
- `reader.next-chapter` / `reader.prev-chapter`: the first chapter start at or after the page's end / the
  last before its start, from the book's table (`core.document` op 7; starts in the page's section made
  exact by op 8, as Beta 3). Goes as a turn (trail, place saved, reported); nothing happens where there is
  no chapter. A book with no table has none (the headings scan is not ported: R6).
- **Not yet:** go to page / percent (need a number dialog and the page index), the quick-settings panel
  screen, book info, the status-line actions, the bookmark (annotations screen, R5).

## Reflow (view slice R8a, Port E)

- The page on show is laid out with a key (the reading settings and the display's width, height and density,
  `LayoutKey`); each draw compares it with the current one and, when they differ, lays the page out again from
  the place it starts at and forgets the trail of earlier pages (their starts belong to the old layout). A
  turned screen is covered by this: it is another size.
- `event.setting_changed` (token 8 in `declarations::events`; a host that does not admit it answers
  `Unsupported` and the next draw's key check shows the change): a layout setting calls `View::reflow` (page
  laid out again, the place reported to the host anew), an ink setting only `View::repaint`.
- The layout settings are listed by id (`native_pages::affects_layout`: font size, line and word spacing,
  margins, justify, `core.reading-font`, `core.hyphenate`, `core.hyphen-min-left/right`; an empty key, "read all
  settings again", too); the ink settings are `core.font-weight`, `core.smooth-text` and the `core.text-*` ones.
  `core.publisher-styles` is applied when the book opens (`open_book`), so a change of it takes effect when the
  book is opened again, not by a reflow. X4 list: every setting change wakes the plugin, and the page is laid
  out in the event callback.
- **Not yet:** hands-free and auto-turn, read-aloud (pictures and table rows: see the section after the decorations). Unmeasured: the time of a reflow on the X4 and the heap of laying out twice.

## Lines under, over and through text, and table rules (view slice R8b, Port E)

- `native_decor.rs`: after each run `draw_lines` draws what the run's `TextStyle` asks for, over the run's
  whole width (its spaces too; Beta 3 decorated each word and the space between two words, so a run that
  ends in a space has a line one space longer here), with the geometry of Beta 3's `ReadingFaces::decorate`: underline at `size / 10` (at least 1)
  below the baseline, overline at 0.78 of the size above it, strike-through at 0.28; the thickness is
  `size / 16` (at least 1); the line style is one line, two (the second as far below as it is thick), dotted
  (dots as long as thick with gaps as wide) or wavy (every second piece one thickness higher); a dotted or wavy
  line has at most 4,096 pieces. `Ink::fill` (a filled rectangle in the pen's colour, `canvas.fill_rect`) is the
  one new call. The page's `rules` (a table's borders) are filled the same way, at least a pixel thick.
- Differences: a run's colour is the page's (no per-run ink); lines of mixed direction are set in logical order
  (the existing gap), so a line follows the run's place in that order.
- X4 gate: Beta 3 skipped lines outside the clip; every draw band here sends all fills (the host clips).
  Beta 3 drew no `Page::rules`: they occur only once table rows are carried over.
- Pictures and table rows: see the next section.

## Pictures and table rows (view slice R8e, Port E)

- `native_block.rs`: a block whose head says picture and that brings its `doc_image` record is kept as a picture
  block: `Block::image` is the picture's host handle (the decimal text of `ImageRef::path`) with its size; the
  layout scales it into the content box as before. A table row (`doc_columns` weights, up to 16, and its
  `doc_cell` records, up to 16, with the head's header/first/last flags) is kept as `Block::table`; the cells'
  ends must not go back and must lie in the text on character boundaries, else the row reads as a paragraph of
  its text (as does a picture or row block that brings none of its records). More than 16 cells is `NoSpace`.
- `native_draw.rs`: `Ink::image` draws each `Page::images` entry with `canvas.draw_image` (scaled into the box the
  layout gave it, centred, default style). A handle the host no longer knows (`NotFound`) or a host that draws
  no pictures (`Unsupported`) leaves the place empty; any other refusal ends the drawing. With rows carried over,
  the layout's table rules (`Page::rules`) are now filled by the decorations' rule drawing.
- **Not yet:** a picture the format has not decoded (`pending`) is not asked for again (op 16 `image`), and a handle
  that ended is not renewed by its `index`: both leave the place empty until the next layout; the view does not
  release pictures (they end with the document). Unmeasured on the X4: the drawing time of a full-page picture
  and the heap of a 16-cell row (a `Vec` of cells and one of weights per row being assembled).
- The metadata asks for the host call `canvas.draw_image` (slot 37) for the pictures.
- A picture with host handle 0 (the host had no id left: eight are out and none drawn yet) is left empty, the page
  still draws. Differences: pictures are drawn with the default style, so Beta 3's `core.pictures-in-grays` setting
  is not applied yet.

- **Not yet:** pictures (`Page::images` through `canvas.draw_image`) and table rows: the block assembler turns
  both into paragraphs (`native_block::start`), so a table's rules never occur yet; carrying them over needs the
  `doc_image` and `doc_columns` pieces of the document service (docs/abi/document.md section 5).

## Read aloud (view slice R8d, Port E)

- Action `reader.read-aloud` (menu, short menu; shown only where the device needs `speech`) on the reading
  screen. The model is `native_speech.rs` (host-free, tested with a recording fake of the screen), the screen's
  side is `native_view/aloud.rs`, the sentence rules are `native_aloud.rs` (`sentence_end_by` and `spoken_into`
  added; the interface-6 comparisons in `native_tests.rs` still hold). The texts are the interface-6 keys
  (`reader.aloud.started`, `.paused`, `.failed`, `.unavailable`, `.page-done`, `.timer-done`; en and de are
  already in the plugin's i18n).
- Words: those of the page on show, in reading order, borrowed from the laid-out lines (20 bytes each, one page
  at a time; Beta 3 kept a `WordBox` with a `String` for each word and a second list of `&str`). Sentences end as
  in Beta 3 (at most 80 words, closing quotes and brackets belong to the sentence, hyphenated words are joined
  again, a sentence that goes on on the next page is spoken whole when pages turn). `command.speak` takes at most
  2,048 bytes: a longer text is cut after a word.
- Flow as Beta 3's: the sentence is spoken and the next is handed over at once with `queue`; `event.spoken` for the
  sentence now speaking hands over the one after it and turns the page where the next sentence is on the next one.
  Each speech has a cookie (the screen's instance in the upper 16 bits, a counter in the lower) so that an answer
  of an older speech (`status` 1, replaced or stopped) is never taken for the current one. `rate` is
  `reader.aloud-speed` (50-300), `language` the book's (empty: the system's; a tag the SDK refuses is empty too),
  the timer's minutes `reader.aloud-timer` (checked when a sentence ends, as Beta 3 did).
- Pause and stop: any action or gesture while it reads pauses it and does nothing else (also "Read aloud"); it
  says `reader.aloud.paused`, stops the speech and remembers the place of the sentence (not an index: after a
  turn or a reflow "Read aloud" goes on at the word at or after that place, else the page's start). The book
  closing, the device sleeping or stopping (`event.suspend`, `event.stopping`) and the reading screen no longer
  being on top (`event.stack_changed`, new token 9 in `declarations::events`; the stack is read only while it
  reads aloud) end it. A layout change (turned screen, reading setting) pauses it. `status` 2 (no voice) says
  `reader.aloud.failed`, 3 (no speech) `reader.aloud.unavailable`, 1 for the speech now speaking (another voice
  replaced it) is a pause.
- Underline: `native_draw::draw_lines_marked` draws one line (the geometry of the book's own underline) under the
  marked words of a line, over the spaces between them, one line for each line of the sentence; `Open::mark` is the
  range of words of the page on show, set when a sentence starts and cleared when it ends. It follows the setting
  `reader.aloud-highlight`.
- Differences from interface 6: the page is redrawn whole for each sentence (Beta 3 redrew the lines of the
  sentence; `Fast` refresh); words of a line that needs reordering are in logical (spoken) order, not in the order they are
  drawn; the speech and the notices are commands (a notice is the device's copy of the text, 256 bytes at most);
  the page-turn and the place write of a turn by reading aloud are those of a turn by hand (report, save after
  5 s or 2 s quiet). A device with no speech is told by the host (`status` 3) after the first request, not
  beforehand (the entries are hidden there by `menu.need`).
- **Needs the host (not in this tree):** `command.speak`, `command.stop_speaking` and `event.spoken` (89) are in
  the contract (`docs/abi/ui.md` section 7) and the SDK (`CommandSender::speak`, `SpeechRequest`,
  `Event::Spoken`), but the native host here does not apply either command (`commands_ui.rs` `owned_ui` answers
  `Unsupported`) nor raise the event. Until it does, "Read aloud" says the device cannot read aloud. `command.notify`
  is in the same state (the other notices of the screen, not saved and now these, stay unseen). Not declared: `event.spoken` needs
  no subscription (like the answers of questions); if the host asks for one, it is one line in `declarations::events`.
- **Not yet:** reading aloud from the selected text (`reader.selection.read-aloud`, with the selection flow), the
  status-line icon *volume* (the status lines), the hands-free auto-turn, a timer that ends a sentence in the middle
  (the minutes are checked at a sentence's end), a hosted test of the whole (a speech host and a document host in
  the harness: `acceptance_pending.rs` *reader-aloud-and-auto-turn* stays blocked).
- **Unmeasured X4 gates:** the whole-page redraw for each sentence (a sentence is a few seconds of speech, a
  `Fast` refresh each); the layout of the page after the next one while the sentence goes over a page end
  (laid out twice: for the text and for the turn; transient, as Beta 3); `Setup::read` (about eight host calls)
  and the page fonts for each page asked for; the stack read (two 4 KiB buffers) at each stack change while it
  reads; the `command.speak` scratch (the text and 128 bytes) and the words of a page (about 6 KB for 300 words).
  Notices use 256 + 320 bytes on the stack and no heap.
- Review remark (#1103): the notice *started* is said after the first sentence was handed to the host, so a device
  without speech says *cannot read aloud* only. `event.spoken` (89) goes only to plugins that subscribed (Services, `ui.md` 7), so the reader subscribes
  (`SPOKEN`, token 16 in `declarations::events`; the declaration test now counts 8 events). Device gate: a notice must not count as a stack row (op 1), or it ends the reading at once.

## Hands-free and auto-turn (view slice R8c, Port E)

- **Ported:** the action `reader.auto-turn` (menu *More*, the key; Beta 3's name and texts
  `reader.auto-turn.started` with `{seconds}` and `.stopped`, en and de) starts and stops pages that turn by themselves,
  every `reader.auto-turn-seconds` seconds (5 to 600, default 30, whole seconds, no float). `src/native_handsfree.rs`:
  the model `AutoTurn` (running for one screen instance; toggle, an input's `Reaction`, close), `classify` (what an input is
  to the timer), the interval, and the glue `Hands` (`core.work`, the setting, the messages). The timer is one work with the
  key `auto-turn`, declared with `after_ms` and declared again after each turn (a key declared again replaces the work, so
  at most one waits and nothing is left behind; `every_ms` cannot be used, its floor is a minute). The work is the host's
  (`event.work_due`, the subscription of the quiet-time save); the tick asks the reading screen for the next page by the
  action `core.next-page` through `View::input`, so a tick is a turn by hand: the place is written (5-second rule), reported
  and redrawn, and the quiet-time save starts again. A page turned by hand sets the count anew (Beta 3); the menu and the
  confirm button (a tap in the middle) stop it and are taken (Beta 3); the end of the book stops it.
- **Memory:** Beta 3 had no per-tick buffers beyond the page's layout, and neither has this: the work call, the setting read
  and the messages use stack arrays (256 + 256 B at most; messages 128 + 256 + 512 B, only on start and stop, never in a tick),
  no `String` or `Vec` is made, no float is touched (`interval_ms` is integer arithmetic on the clamped value). A refused
  or missing work service is "cannot turn pages by itself" (`reader.auto-turn.unavailable`, new, en and de) and starts nothing.
- **Differences from interface 6:** (1) *any* other action, gesture or widget input also stops it (Beta 3 let them run on;
  the plugin cannot see an overlay such as the menu or the font panel open over it, so every input that could is a stop);
  raw key and contact records do not (the host resolves them to actions and gestures). (2) It also stops when another screen
  opens over the reading screen, when the reading screen closes, at `event.suspend`/`event.stopping` and when another of the
  reader's actions runs (`event.action_run`); Beta 3's screen simply was not ticked then. (3) The tick is a quiet run
  (`work.md` 2.3): no message is possible in it, so a stop at the end of the book says nothing (Beta 3 did not either). (4)
  The status line's clock icon is not shown (status lines are not declared yet); Beta 3's refusal to let go of the book for a
  screen above while it ran is not ported (no squeeze event). (5) The timer is counted by the host's work clock, which runs
  only while the device is awake (`work.md` 0).
- **Not yet:** read-aloud (another slice; Beta 3's hands-free icon for it too); the overlays that the host opens without a
  word to the plugin (the quick-settings tap) are seen only through the input or the action that opened them, so a due call
  can turn a page under an overlay the plugin was not told of (named gap, see the gates); status icon.
- **Unmeasured X4 gates:** the extra `work.declare` (one host call, stack buffers) at each tick and at each page turn by hand
  while it runs, on top of the quiet-time declare of every turn; the time of a tick (a page's layout in the event callback:
  as the back-layout, with the page turn); heap during a tick (as a turn by hand); the accuracy of the host's work timer
  against Beta 3's `wake_at` (the host's naps wake for it, `work.md` 2.1, to the millisecond on the X4 only for exact
  timers: this one is `after_ms`, so it may be late by a main-loop nap); at most 8 works per plugin on the X4 (this is the
  second, the quiet-time save the first).

## The page count (view slice R2b-1, `native_index.rs`, `native_view/count.rs`)

- The pages are laid out in steps by `core.work` (key `count`, the reader's `WorkDue` token): 8 pages a
  step and 2 ms apart; on a small device (memory class Small, or a class the host does not tell:
  `core.device` op 1, asked once per book) 1 page and 500 ms (Beta 3's values). The count starts at open
  and again after a layout change, and runs while the reading screen is on top.
- The file is `pages-<hash>.bin` in the data folder, named by the book's path, the edition (the open
  conversion) and the layout (size, spacing, margins, justify, display), 8 bytes a page and a closing
  mark, as `PageIndex` keeps it. The files of the last 4 layouts are kept (Beta 3's `INDEXES_KEPT`; the
  data file `pages.list` names them, oldest first), the oldest is removed: switching back finds its count.
  A font the host could not read mid-step (a card pulled) makes the step's pages untrusted: not kept,
  laid out again at the next step, 3 times in a row at most, then the count stops (Beta 3's retry).
  **Gap:** a file of a book closed long ago stays until its place in the list is needed.
- Once the count is complete the host's report carries `page` and `pages` where the page starts at one
  of the book's pages (a page started elsewhere, by a jump, reports none until a turn). Not yet:
  `chapter` and `chapter_left` (R6 chapter of the page), the status lines (R2b-2).
- `PageIndex::append` reserves its small batch (at most 64 bytes) with `Vec::with_capacity`: SDK code.

## Book info, the font panel and the bookmark (view slice R3b, Port D, `native_bookmark.rs`, `native_view/r3.rs`)

- `reader.book-info` opens the slot `core.book-info` with the book's path as a hand-over parameter (the files
  plugin's information screen fills it); `reader.quick-settings` opens `core.panel` with the menu
  `core.reader-panel` (the font and layout panel: the entries are the host's). Both as Beta 3's actions did.
- `reader.toggle-bookmark` (`native_bookmark::toggle`): a page that has a bookmark loses all of them, any other gets
  one, kept as an annotation of the book (`core.library` 2.2: the book's annotations are read page by page, up to 8,192
  rows, then one staged transaction at the generation read: remove the ids / add one, commit; a `State` answer, another
  change in between, looks again once; a refused edit aborts the transaction and says "the bookmark could not be kept"
  once, `reader.marks-not-saved`). A bookmark is on the page when it starts between the page's start and the next
  page's; a row in another conversion is not compared. A new bookmark keeps the page's text from its lines as the
  book has it (a word broken by the layout is whole again, the layout's hyphen left out, a filter's lines left out;
  `native_bookmark::text_of`), on one line and cut at 512 bytes at a character (Beta 3 ended a cut text in "…": the
  library cuts at its own limit), the progress in whole percent (rounded, as Beta 3), the book's clock and
  the conversion. At most 24 bookmarks of one page are removed in one go.
- The ribbon on a bookmarked page and the tap on the top right corner (`reader.corner-bookmark`) are
  `native_view/ribbon.rs` (#257): Beta 3's ribbon (3 mm wide, a notch, four fifths of the room above the text)
  and corner (12 mm, at most a quarter of the width). The book's bookmark starts are read once at the opening
  (at most 256, 8 bytes each) and follow the toggles made here; a toggle redraws the page.
  Go to a page / percentage is `native_goto.rs` and `native_view/goto.rs` (#257): the number is asked on the keyboard
  (`core.ui` ask, number mode), a page goes to its start in the count (clamped to the pages counted so far), a
  percentage to the start of its page where the count is complete, else to the place that far into the text, as Beta 3
  did. The status-line actions (`reader.next-status-line`, R2b) are not here. The annotations screen itself is R5.
- An empty page (nothing laid out) is a silent no-op: no bookmark, no notice. Beta 3 sent `annotations::changed`
  itself after an edit; here the plugin does not: `docs/abi/library.md` lists event 91 (`annotations-changed`)
  as the host's after a commit, but `paperpaw-core` does not raise it yet. **Gap:** the library annotation
  transactions (ops 15-21) are not wired on devices (`ERR_UNSUPPORTED`) and event 91 is not raised: Services'
  todo 307/476 (with a host test for the edit transaction). Until it lands the toggle does nothing on a device;
  it gates the reader entering `NATIVE_PLUGINS`.
- Not tested with a host (no harness for `core.library` edits here): the pure parts are (the page's range, which
  rows are the page's, the percent, the page text and the snippet, with refused allocations); host acceptance of the
  edit transaction (H-library) is pending.


## Annotations (native model, `native_annotations.rs`)

- Host-free: the Markdown export (same form as interface 6, plugins read the file), the notes folder
  beside the library folder, the file name (author and title, fit for a card), the list rows and the
  export order (the `reader.notes-exported@1` stage runs only after a write that worked). Allocation
  is fallible; every allocation refused in turn is tested.
- **Not yet: the screen.** It needs a native `core.reader` entry, the book data of a book
  (`add_book_data` / `LIB-position` have no native declaration) and the files service for the write.
  When those exist the screen calls `native_notes::run` after `export` succeeds.
- Review remarks (PR #950), for the screen: it creates the `Notes` folder first, and writes the file
  staged and then moved where the files service allows (Beta 3 wrote straight onto an existing file).
  The whole Markdown is one `String`: its size on the X4 is on the measurement list (pieces through a
  sink if it does not fit). The screen, the book data and the files write stay on the activation list.

## The annotations screen (`native_annotations_view.rs`, R5)

- `AnnotationsView` (token `ANNOTATIONS`, id `reader.annotations`): opened by the reading screen's action
  `reader.show-annotations` with the same hand-over as the contents (`View::toc_params`: path, root,
  section, offset; the hook is `NativeReader::show_notes`, no change to `native_view.rs`). It reads the notes with `core.library` op 15
  (`Library::annotations`, `BookRef::path`, restart after a stale generation, at most three times), shows each as one
  `ItemList` row (`native_annotations::row`), and a choice goes to the reading screen below
  (`View::go_to`) and closes, as the contents screen does. The last row, *Export as Markdown*, writes the file.
- **Rows.** At most 100 notes are held, each as the row the list draws (the text cut at 160 bytes) plus eight bytes of
  place; a book with more shows the first 100 and a note says so (`reader.annotations.more`, en/de). Beta 3 listed all.
  X4 measurement: 100 rows of 160 + 160 bytes are about 35 KiB at worst (a bookmark is far less); the cap is the number to
  change if the measurement says so.
- **Export.** The Markdown is written in pieces of at most 4 KiB into `<file>.new` (highlights from one pass over
  the notes, bookmarks from a second, so only a page of notes is in memory: this answers the review remark that the whole
  text is one string), moved over the old file at the end, the `Notes` folder made first (beside the library folder,
  `core-library-folder`), the stage `reader.notes-exported` run after a write that worked
  only. The title and author come from the library's book row. The streamed text is tested to be the model's
  `markdown` (same form, readwise and joplin read it).
- **Missing / for activation.** Metadata must ask `core.library`, `core.files` and `uses.storage` (root 1 for the
  export); with no library or no `uses.storage` the list shows the notes it can and the export says it failed. A note's text is
  the library's preview (512 bytes), not whole (`annotation_text` would read it whole: the export uses the preview, as
  Beta 3's snippet was short too). Editing a note, deleting one and the bookmark toggle are not in this screen (Beta 3 had none
  here); `reader.toggle-bookmark` belongs to the reading screen.

## Chapter in the report (R2b-3)

- `native_chapter.rs`: `Chapter` (start, next start, title) is the chapter of the page on show. `Chapter::offer` takes one
  table entry (exact start, title) for a place: the last start at or before it is the chapter (the later of equal starts,
  as Beta 3's `update_chapter`), the first start after it ends the chapter; the table need not be in order. A place before
  the first entry gives the empty chapter. The title is trimmed and cut at 256 bytes on a character boundary.
  `pages_left(page, pages, next)` is Beta 3's `chapter_left`: the page the next start is on (from the count's
  `locate`; the page before it when the start begins that page, the page itself when it begins inside it) minus the current
  page, the last chapter ending with the book, never more than `pages - page`.
- **Report.** `Open::announce` sets `.chapter(title)` always (empty where the book has no entry at or before the page, or
  the table cannot be had) and `.chapter_left(n)` only with `.pages(..)` (the count complete, the page starting a counted
  page) and a non-empty title, so the host's `ERR_INVALID` cases cannot arise. The report is sent on each page turn, jump,
  count completion and close, as before; only the topmost screen's counts (host rule).
- **X4 notes.** Only the current chapter is held (`Open::chapter`: two places and one title of at most 256 bytes). Nothing
  is asked of the table while pages turn inside that chapter (`covers`). On leaving it `find` reads the table through
  `core.document` toc pages (at most `MAX_ENTRIES` = 500 entries, one 256 B + one `SERVICE_BUFFER` pair of buffers for the
  rows and a second pair for `toc_target`) and one `toc_target` per approximate entry in the page's section; no title is kept
  but the chapter's. `chapter_left` costs one `PageIndex::locate` (a read of the index file) per announce, none for the
  last chapter. A table the host cannot give leaves no chapter (no `chapter_left`) rather than a wrong one.
- **Missing.** No scripted-host test of the report call itself (the pure model, the allocation sweep and the page-turn
  and chapter-change sequence are tested; the announce wiring is checked on a device/simulator). The next-start of a
  table that is out of order costs a full scan (up to 500 entries) per chapter change.

## The status lines (R2b-2)

- **Declared** (`declarations::status`, batch `STATUS`; `status_settings` inside `settings`): the places
  `status.top` and `status.bottom` with their three part settings, the items `status.title`,
  `status.chapter`, `status.page` and `status.progress` (token `STATUS_ITEM`, priorities as Beta 3: 64, 64, 224,
  128), the lines `status-line.progress` and `status-line.empty` (token `STATUS_LINE`, height 2100 thousandths of the
  text size), and the eleven settings of Beta 3's `status.rs` with its ids, defaults, bounds, units and sections
  (`status.size`, `status.edge`, the two lines lists, the six parts, `status.battery-percent`). Texts are Beta 3's
  keys (en and de already there; no new string). The declaration test compares them with the interface-6
  registration, the editors of the lists apart.
- **Own items are filled by the reader** (the host never calls it back, `ui.md` 5.3). `native_status::Texts` makes
  the four texts: title; chapter (the chapter's title once the contents slice gives it, else the title, as Beta 3);
  page (`12 / 340` from the counter, `native_view/count.rs` `page_numbers`, else the percentage; `reader.page`,
  `reader.percent`); progress (`Progress::ppm`, whole percent cut). `paperpaw-ui-kit::status_line` has
  `draw_own`, `targets_own` and `action_own` taking those values (`draw`, `targets`, `action` pass none).
- **Lines.** A place shows one of the lines of its setting; a tap shows the next (`native_status::choice`,
  `State::advance`), the choice kept in `status-lines.txt` as interface 6 kept it (same file and form). The items
  line (`core.status-line.items`, also `reader.status-line.items`) is the kit's `status_line` over the place's three
  parts (other plugins' items composed by the host, `ui.compose`; a tap on an item with an action of its owner runs
  it, elsewhere shows the next line); `reader.status-line.progress` is the bar (`native_status::bar`, whole numbers,
  marks at chapter starts, the page's label on the right); `reader.status-line.empty` nothing; any other id is a
  line of another plugin, composed whole. The actions `reader.next-status-line` and `reader.next-top-status-line`
  (the bottom line, the top where there is none below) show the next line.
- **Room** (`native_status::room::Room`, whole numbers): size from the setting (5 to 16 pt) in pixels, a line 2.1
  times as high, its distance from the edge, and 2 mm the text keeps: `Reading::status` carries the two reserved
  heights into `content_box` and into the layout key (`native_index::layout_text` gains `/<top>-<bottom>`, so the
  page count of another room is another file). A place with no line in its setting keeps nothing; the height
  depends on the status text size, not on the line showing, so a tap never changes the pages. Books of whole pages
  have no status lines.
- **Hooks:** `native_view.rs` (`Setup::room`, `area()`, one call in `draw_with`, one in `input`, the field
  `Open::status`; the glue is `native_view/status.rs`) and `native_plugin.rs` (the `STATUS` batch; `draw` hands the
  view the `Ui`). Metadata asks `core.registry` and `ui.compose` (all four files regenerated).
- **Not yet (named):**
  - The editors of the lines and the parts: interface 6 used the kernel's `core.status-lines` and
    `core.status-items` screens, which are not slots of the native host; a plugin names its own screen built on the
    kit's `status_items` (as picture-home does, Kernel's ruling todo 121). The settings are declared without an
    editor (plain lists) until the reader has those two screens.
  - `status.battery-percent` is declared and kept, but the kit's items line has no way to hand it to the kernel's
    battery item.
  - The items `status.aloud`, `status.printed-page`, `status.chapter-page`, `status.chapter-left` and
    `status.chapter-bar` of Beta 3 (the default of the bottom left part still names the first, which the host
    leaves out where no plugin declares it). Context values (`reader.chapter-page` and the rest) are not passed to
    other plugins' items (`status_values` is asked with no context).
  - The chapter's title and the chapter marks of the progress bar wait for the chapter slice (`Facts::chapter`,
    `Facts::marks`).
  - The reader's items and lines when another plugin shows them (`draw` purpose 3 and 5, `core.status` stage):
    not answered, so a line of another plugin that lists the reader's items shows nothing for them.
  - The lines are not redrawn on the minute tick (the clock changes at the next turn or tap); the pause while pages
    are turned quickly (Beta 3's `signature`) is not ported.
  - Not tested with a host: painting and the tap (the harness has no status service); the pure parts are tested
    (`room`, `bar`, `choice`, the state, `content_box` with room, the declarations).

## Skim and headings (R6b)

- `native_headings.rs`: the headings scan as a background step model like `native_index::Counter`.
  `Scan::step(storage, read)` reads at most `STEP_BLOCKS` (60) blocks through `read(place)` and returns
  `Step::More`, `Done` or `Stopped`; `read_seen` is the ready `read` over `core.document` op 6
  (`native_block` joins the fragments; only the heading's text is kept). A section that does not read
  is passed over; `Busy` or a refused allocation stops the scan and keeps nothing. At the end the
  headings are normalized (`native_toc::normalize`) and kept, then `load` gives them back for a book of
  the same size. **The caller wires `core.work`** (declare with a key, `native_index::pace` for the
  wait, `Scan::percent` for the screen) and starts the scan where `native_toc::wants_scan(len)` and
  `load` finds nothing; `TocView` is not changed yet (its `message` is where `native_finding` shows).
- The kept copy is `headings-<hash of the path>.txt` in the data folder, not `headings.txt`: the data
  storage is flat, so one name per book (`headings.list` keeps the newest 16, older files are removed).
  Its form is the interface-6 one (`size N`, then tab-separated section, offset, level, title). It is
  written in pieces of about 1 KiB to a `.new` file and renamed, so a cut leaves the old copy and the
  whole text (up to 80 KB) is never one `String`; reading it back is one 80 KB read at most
  (`read_file`). The size the caller passes is the book file's (`native::storage` for the book root).
- `native_finding.rs`: the "Finding the chapters" model, `Finding::update(percent)` (steps of 5 so the
  e-paper is not drawn for every block) and `draw` (the title and the text where the list is);
  `reader.toc.finding` (existing) before a number is known, `reader.toc.finding-percent` (new, en and de)
  after.
- `native_skim.rs` is `skim.rs` without a host: the focus (bar or four buttons), what keys and taps ask
  (`Move::Pages(+-10)`, `Chapters(+-1)`, `Back`, `Permille(n)`, `Number`) and when the screen closes.
  `native_skim_view.rs` is the screen (`reader.skim-screen`, slot `core.skim`, token `SKIM`): an
  overlay (now declared as one, like the note; the ABI-7 declaration test expects flag 4 for it)
  with the page label and chapter, the bar (track, read part, chapter ticks, knob, focus frame) and the
  buttons as `Chips` (they wrap to two rows or one column where the words do not fit, as Beta 3's
  4/2/1). The action `reader.skim-book` (not `reader.skim`: the declared name) opens it from the reading
  screen (`native_plugin`, as `reader.show-annotations`), with a hand-over of `page`, `chapter`,
  `permille` and `marks`.
- **Not yet:** the plugin gets the `Move` from `SkimView::input` and drops it: `View` has no page index
  moves, no chapter marks, no percent jump and no `permille` / label / chapter to put in the hand-over
  (it opens with an empty bar). To do: `View` methods for `Pages(n)` (needs the page index, R2b),
  `Chapters(n)` (exists as `step_chapter`, private), `Permille(n)` (needs the sections' lengths, R2a's
  `Progress`), `Back` (the place where skimming began, to keep in the screen) and `Number` (the
  go-to-page dialog, not ported), `SkimView::update` after each move, and the marks from the table
  of contents. `TocView` does not start the scan or show `Finding` yet. The slider's accessibility
  description (`Role::Slider`) has no native counterpart in `Paint`. The skim bar's colour for a dark
  focus (Beta 3's thin line) is not ported.
- X4: the screen is one draw of up to ~25 paint operations and four chips (fonts acquired once per draw);
  no float formatting; a frame is far below 2,048 bytes. The scan holds at most 500 headings (about
  70 KB, shared with `TocView`'s list when both exist: on the measurement list) and one block's text
  (up to 64 KiB, as the layout's) per step.

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
- **The screen is `native_search_view.rs` (R7, below).** (It needed the document's block, progress and section ops (`core.document`, #2223),
  the font measures (`Measure`), the timer (`core.work`) that drives `step` (interface 6 woke itself every
  2 ms), the list, and, to choose a hit, going back and to the place with the match marked (the successor of
  `core.goto`, not declared yet). Choosing a hit is `Search::target`: where the match begins and ends.)

## The search screen (`native_search_view.rs`, R7)

- Flow: the action `reader.search` on the reading screen asks a text question (`core.ui` ask, `reader.search.prompt`;
  the keyboard plugin answers it), the answer (cut to `MAX_NEEDLE` = 32 characters, so the screen searches what it
  says) opens `reader.search-results` with the contents screen's hand-over plus a `query` param (one instance per
  text), and `SearchView::open` takes the text the plugin kept. Choosing a hit goes to where the match begins
  (`View::go_to`) and closes the screen, as the contents screen does.
- The scan: `core.work` key `search`, token `SEARCH_WORK` (9, a token of the work itself: the reader hears
  `event.work_due` with its single `WORK_DUE` subscription and tells the two works apart by the event's token, so there is
  no new subscription and the declaration test is unchanged). A step reads 24 blocks of the **reading screen's**
  document (`HostDocument::read_at`, new: the block with the host's refusal as it is), redraws when the header's
  progress (tens of a percent) or the hits changed, and is declared again after 50 ms while the book is read. A failure
  that repeats twice in a row ends the search with `reader.search.failed`; the hits so far stay.
- Snippets are cut to the row with the UI font at the UI size (`Style`), the display's width less the margins and the
  room for "100 %"; where the font cannot be asked, half the size is a letter's width and 300 px a row's room.
- How far in a hit is: by the sections' lengths (`Progress`) where the book has few enough sections, else by the
  section.
- **Differences / gaps (named, not hidden):**
  - *No match mark.* Choosing a hit goes to the place only: there is no successor of `core.goto` to mark the match; a
    `go_to_match(place, end)` with a highlight is a follow-up in the page drawing (Port E).
  - *Text filters are not applied* to the blocks the scan reads (interface 6 searched the text as the page shows it,
    after the filters); `core.input-filter` is Port C's.
  - A search needs the reading screen's document: from no reading screen it does nothing (interface 6 opened the book
    itself); the X4 holds one open document per plugin.
  - **Memory (Port Reviewer's must-fix, Beta 3 held only the hits):** a hit's snippet lives in its list row only: when a
    step finds hits, `extend_list` takes each snippet out of the model (`Search::take_snippet`, the hit keeps its place
    and progress), makes its row (`ItemList::push`, the list grows in place, never rebuilt) and keeps the hit's start in
    `places` (8 bytes a row) for the choice. A refused allocation in that ends the search with `reader.search.failed`
    (the hits so far stay). Worst case 100 rows of at most 24 + 32 + 48 characters: about 20-40 KB. **X4 gate:**
    measure it with `--heap-report`; `ItemList` still lays out every row (a row source would build per page).
  - Each step builds a `HostDocument` (a 4 KiB reply buffer and a 256 B one) and acquires the UI font; the text size and
    the row room are read from the display once per screen.

## The start of the device (`native_start.rs`)

- `Event::Started` (token `STARTED`, already declared): where `reader.open-last` is on and `state.ini` names a
  book that `core.files` `stat` finds on the card (root 1), the book opens through a hand-over with its
  path in the reader bound to `core.reader` (as the home screen opens a book). A missing file, an
  unreadable `state.ini` or a refused stat leaves the start screen. Beta 3's `reopen_last_book`.
- The last book is written (`state.ini`, Beta 3's form) when a library book opens; best effort.
- `event.book_moved` (token `BOOK_MOVED`): the last book read follows a moved book (`native_start::follow`; Beta 3's
  `book_moved`; the book's data folder is moved by the host).
- The host asks for a reply buffer of the operation's `max_reply` (4,096 bytes for `core.files` stat and
  `ui.handover`) and answers `NoSpace` below it: both buffers are 4,096 (transient, freed at once).
- **Blocked on Core (todo 474):** the host's command stage has no hand-over store yet (`commands.rs`
  `handovers: None`; `with_handovers` is only used by tests), so the open with the book's hand-over is
  refused `ERR_INVALID` until it is wired, in event callbacks as in input callbacks.
- Not covered by a test: the host path (no scripted host); `state.ini`'s reading is the tested `native_state`.

## Marks on the page: highlights and the search hit (`native_marks.rs`, #257)

- The page draws the book's **highlights** (library annotations of kind highlight, read with `core.library` op 15 when the book
  opens: rows of another conversion are left out, one with none is taken, a highlight of no length marks nothing) and the
  **match of a search hit** gone to, in the looks of the roles `core.highlight` and `core.search-hit` (settings
  `<role>.background|ink|line|line-tone|thickness|frame|struck`, read on the pages that have a mark, as Beta 3's
  `Decoration::of_role`). Backgrounds go behind the words (`Painted::behind`, before `draw_lines_marked`), lines under, frames
  and strikes over them (`Painted::over`); a box is one for each line of covered words (`native_selection::line_boxes`).
- **Memory.** Only the highlights are held: 16 bytes each, at most 512 (`MAX_HIGHLIGHTS`; Beta 3 held every note with its
  text). The cap is 8 KiB at most, kept for the whole reading, and a page rarely has more than a few; a book with more draws
  the first ones the library gives. The list grows by exactly one entry (`try_reserve_exact`) and is shrunk to fit after the
  read. No room for it (or a refusal of the library) at the open: the book opens without highlights.
- **Marks never fail a page.** `Painted::new`, `behind` and `over` run on the page's draw: any error (no memory, a host
  refusal) leaves the page drawn without the marks (or with those drawn so far), as the text is drawn after.
- **Few grays.** On a panel of two or four grays (not colour) a background is not filled with the tone, which would turn
  white (light gray 210) or black behind black text: it is rows of black by the tone's darkness (one in four for light gray,
  three in four for dark gray), counted from the top of the screen so boxes of two lines meet without a seam; Beta 3's
  `fill_shaded`. The looks are read on each draw that has a mark on the band (a handful of small host reads), not kept, so a
  look changed in the settings shows at once. The read is one pass of at most 8,192 rows through a 256 B
  and a 4 KiB buffer at the open. The search results keep 4 bytes more a row (where the match ends, 100 rows at most).
- **The hit.** Choosing a search hit goes to the page that starts at the match (`View::go_to_match`) and marks it there; the next
  page drops the mark (it is kept with the page it was set for).
- **Not here (named):** the highlights are read at the open and after the reader's own edits (the creation, next slice); the
  event 91 (`annotations-changed`) has no SDK decoder yet, so an edit by another plugin shows at the next open. The `ink` aspect
  (a tone for the words' letters) is not drawn: the text is one colour. Words a text filter inserted have no places and are not
  marked.

## The first run (#257 item 16, todo 529)

- **Help hints** (`declarations::first_run`, `HINTS`): Beta 3's nine for `reader.screen`, in its order: touch
  *previous*, *menu* and *next* with their zones (thousandths: 0/333/667 across, 120 to 880 down), touch *quick*
  and *night*, buttons *turn*, *select*, *back* and *panel*. The keys `reader.help.*` are in `en` and `de`.
  `tests/abi7_declarations.rs` decodes them and compares them with the interface-6 `help.hints` lines.
- **The wait for the book (`help::wait`): not declared.** Its native counterpart is `ScreenDecl::waits_ready`
  (`screen.spec` bit 3), but the host's admission refuses the bit (`ERR_UNSUPPORTED`, `declarations.rs`
  `screen_check`: "not connected to the host help policy yet"), and a refused screen would fail `make`. Until it
  is admitted the host raises `event.screen_ready` after the reading screen's first draw, so the hints can come
  over *Preparing* or an error message. The reader's half is done (`native_ready.rs`): it sends
  `command.screen_ready` as Beta 3's `on_show` sent `help::ready` (the screen on top with its book open, and a
  prepared book that opened; hooks in `native_plugin.rs` `reading_shown` and `document_changed`); the host takes
  it as nothing for a screen that does not wait. A test fails once the host admits bit 3: then add
  `.waits_ready()` to the reading screen in `declarations::screens` (one line).
- **Screen hints *on press* (`HintVisibility::OnPress`): not declarable.** `ScreenDecl` has `hints_hidden`
  (bit 8, Beta 3's `Hidden`) and nothing for *on press*; the reader declares neither, so the row of button
  hints shows as on any screen (and takes its room). SDK/ABI/host gap: a `screen.spec` bit (9) `hints_on_press`
  with `ScreenDecl::hints_on_press`, the host showing the row for a few seconds after a press (as
  `command.show_hints`). A test records the gap.
- **Setup step *text size*** (`setup.font-size`, order 30, the page `Setting("core.font-size")`): admitted
  (`setup.step`, `registry.md` 6) and listed by `core.registry` op 15. Its title is the setting's own
  (`core.setting.font-size`): Beta 3's empty title meant that, and a native step needs a title; no new text.
  No help (as Beta 3); op 22 (`setup_step`, the help) is not needed for it.
- **Sample widget** `reader.sample` (token `SAMPLE` 40, category `core.widgets.other`, previews
  `core.group.reading`): `native_sample.rs`. Measured a fifth of the display high (`core.widget` stage call,
  least height the same), placed, no targets (a tap does nothing). Drawn from scratch at each draw: the
  reading settings read then (`core.font-size`, line and word spacing, side margins, justify; the values on
  show while one is chosen), the two texts `reader.sample.1` and `.2` (two 1 KiB transient buffers), the four
  reading fonts, the layout (`layout_page` over a two-paragraph `Document`) and the page's ink (`Look`,
  `draw_lines`); nothing is kept across frames. Text box: a page's side margins and width (no wider than the
  frame), 2 mm above and below.
  - *Differences from Beta 3:* no hyphenation, no text filters and no page frames (columns), as the native
    reading page has none yet; Beta 3 kept the laid-out lines between draws (the native one lays out at
    each draw). The language of the layout is not set (Beta 3 passed the interface language for the script).
  - *Not shown yet:* the settings plugin does not compose a group's preview widget above its settings
    (`plugins/settings/PORTING-7.md`, ignored case `settings-group-preview`); the host lists it
    (`reg_widget.previews`, op 19 flag). Untested on a host (no scripted widget call); the layout and the
    draw are tested over a fixed-width measure.
