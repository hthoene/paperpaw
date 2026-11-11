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
- **Not yet:** pictures and table rows (see below), hands-free and auto-turn, read-aloud. Unmeasured: the time of a reflow on the X4 and the heap of laying out twice.

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
- **Not yet (named):** the ribbon on a bookmarked page and the tap on the top right corner
  (`reader.corner-bookmark`): both belong to the draw and the gesture code (`native_view.rs`, R2b's files) and
  follow when that lands; until then a toggle shows no mark on the page (the annotations screen, R5, lists it).
  Go to a page / percentage (a number dialog; its answer is an event of the reader's own) and the status-line actions
  (`reader.next-status-line`, R2b) are not here. The annotations screen itself is R5.
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
