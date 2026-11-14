# Vocabulary on interface 7

Status: **complete.** The interface-6 plugin (registration, screens, widget, marks, flash cards,
phone page, sleep card, its tests) is removed (#244); what is below is the native port, with
what was done on purpose differently. Interface-6 tests that were deleted with it are mapped
to native ones in the PR text of #1452 (coverage table, with the gaps named).

## Ported: the words' data layer (`src/store.rs`, `src/lines.rs`, `src/files.rs`)

- Everything allocates fallibly: a refused allocation ends the call with
  `StorageError::TooLarge`, files untouched, never an abort. `src/alloc_refusal_tests.rs`
  refuses each allocation of count, range, summary, add, remove and export in turn (the test
  allocator of `sync_tests` can now refuse the n-th allocation; the storage it runs on is not
  counted).
- File names are constants (`words.txt`, `words.tmp`): no formatting to name a file.
- Reading is `lines::each_line`: a fallible line buffer, lines cut at 4,096 B, invalid UTF-8
  replaced (fallibly), a last line without a line break is a line.
- `Word::line` / `parse` are `push_line` / `Result<Option<Word>>`: fields are pushed with
  reservations, never `format!`.
- Matching words (`add`, `remove`) no longer makes a `Word` per line, nor lower-cases whole
  strings: it compares the fields of the line, folded char by char (final sigma `ς` = `σ`).
- `rewrite` (remove, add of a known word, the 2,000-word limit) writes the lines it keeps as
  they are, not read and written again as words, so it cannot change a line. It stages in
  `words.tmp` and renames, as before (`recover` at start is unchanged).
- `add` builds its line before reading the list: where the heap refuses, nothing is touched.
  Where the list cannot be read (also when the heap refuses a line buffer), the word is only
  appended, as for a read error (the old entry then stays). A later successful `add` of
  that word heals the duplicates (it removes every older entry); a test covers it.
- `DataFolder` (`files.rs`) takes the plugin folder off the paths, so the layer runs on the
  host's root 0 (`native::storage::DataStorage`, PR #358) unchanged. It goes when the layer's
  paths are the short ones.

## Ported: decks and cards (`src/cards.rs`)

- Same treatment as the words: fixed names are constants (`decks.txt`, `due.txt`, `answers.txt`, ...),
  deck files are named with `push_number` (no `format!`), every `String`/`Vec` grows after a
  `try_reserve`, lines are written with `push_*` (`Card::push_line`, `State::push_fields`,
  `Index::push_lines`), texts are escaped/unescaped into reserved strings, `clean` is fallible.
- `BTreeMap` (no fallible insert) is gone: answers and deck sizes are sorted `Vec`s (`Answers`,
  `Sizes`) with a reserved insert. The stable `sort_by_key` (allocates) is
  `sort_unstable_by_key` on `(due, deck, offset)`, so ties come in the same order as before.
- `due`, `due_count` and `sizes` return `Result`; a refused allocation is `TooLarge`, never half a
  queue. The screens' callers use `unwrap_or_default` (display only).
- Lines are read by `lines::each_line_limited` (the 8 KiB limit of the cards is a parameter);
  `put_in_place`/`part_of` in `files.rs` replace the SDK's interface-6 `files` helpers (rename
  only: no copy-and-delete fallback; root 0 and the card both rename).
- `clean_or_empty` and `cards::path` are for the tests (the phone page is native now).
- `alloc_refusal_tests.rs` sweeps decks, sizes, due, due_count, card_at, visit_page, next_due,
  add_deck, rename/delete deck, add, edit, delete, record, fold, queue_lookup, take_lookups and
  recover: after any refusal the decks read, card ids are unique and no `.tmp`/`.part` is left.

- When the heap refuses something after a deck was put in place, the index (`due.txt`) no
  longer says what the decks hold: it is taken away (`keep_index`, a removal needs no heap) and
  made anew from the decks (`rebuild_index`) at start, when the cards are chosen (`due`), and by
  the next `set_index`/`add_to_index` (an index is never started from one deck). The sweeps
  assert that after the next start the widget's `due_count` equals what the decks hold.
  `delete_deck` makes every allocation before it changes anything.
- `Answers::note` inserts into a sorted `Vec`: folding `n` answers of different cards is
  quadratic in `n`. Fine for the few hundred answers of a review or a day (`answers.txt` is
  folded when a review ends and before any change of a deck); not a design for tens of thousands.
- Ties between cards due at the same time are broken by the deck's place in `decks.txt`, then the
  card's offset: as the stable sort did (a test lists the decks with the higher id first).

## Ported: reading cards from files (`src/table.rs`)

- The CSV/TSV/TXT reader allocates fallibly: the parser's field and column buffers grow after
  `try_reserve` (`Parser::feed` returns `Result`), the header's roles are an array, not a `Vec`
  of `String`s, column names are compared case-folded without a lower-case copy, Windows-1252
  and UTF-8 fields are decoded into reserved strings, and slicing of read chunks is `get(..n)`.
- `plain` (HTML to text) is one pass with one reservation (the text is never longer than the
  HTML): tags out, the six character references turned, white space collapsed. `plain_or_empty`
  is gone: `apkg` uses `plain`.
- `alloc_refusal_tests.rs` sweeps `detect`, `each`, `scan` in steps, a Windows-1252 file and
  `plain`.

## Interim: interface-6 callers

`lib.rs` and `widget.rs` (still interface 6) call the Result API with `unwrap_or(0)` /
`unwrap_or_default()`: on a read error (or a refused allocation) they show an empty list or
a count of 0, where they used to show what was read so far. Nothing is written from that view
(`add`/`remove` read the list themselves and never write anew from a partial read). The
native screens replace these callers.

## Ported: the sync of card files (`sync.rs`, host-free over `Storage`)

- The folder listing, the ledger (`sources.txt`, read line by line, written in pieces through
  a staged part and a rename), the pending count, the folder and its note, deck names (no
  `to_lowercase`: compared folded, extensions cut ASCII-insensitively), the job (files, work,
  reports, questions) and the merge of a file into a deck allocate fallibly. A refused
  allocation in a step makes the file `Unreadable` (never marked failed, so it is tried
  again); one that cannot even begin a file ends the job and sets `Job::refused()`; a
  report that cannot be kept sets it too.
- `Step::Ask` carries nothing now: the question is `Job::question()` (no clone per step).
  `table::record_at` returns `Result<Option<_>>` (a refusal or read error was a missing line:
  a change then went unmade and the content hash said "same" next time).
- A card changed from its line that cannot be copied is left as it is and the update fails
  (the ledger is not written), so the next update tries again.
- The ledger larger than 128 KiB or of another version is `Io` (unreadable, every file fails
  and none is written over it); a refused allocation is `TooLarge` (the job is not made).
  The shell tells the user that a manual sync tries the files that failed again (Auto does not
  retry a file that is itself to blame: too large, no cards, `Crowded`).
- Changes of behaviour: a ledger file that begins with a blank line is unreadable as before; a
  byte order mark before the header is now accepted; the sort of the files is by folded name
  then name (the old stable sort over `to_lowercase` had no tie-break).
- `apkg`: the plugin's own part allocates fallibly: both files are constants, a note's
  fields are one reservation each (`note_text`: sounds and `data:` addresses cut out, then
  `plain`), the collected text is reserved once (3 KiB) and grows fallibly, `quote` reserves
  its worst case. A refused allocation is `Fault::Memory` (the notes written so far stay
  until the next job tidies them), never a note with a missing field. The zip and sqlite
  readers are fallible now (`paperpaw-zip` #394, `paperpaw-sqlite` #383) and the whole-read
  sweep (`reading_an_anki_deck_refuses_cleanly`) runs with every allocation refused. It found
  that a refusal while reading the table's columns was told as `NoNotes` ("not an Anki deck,
  not tried again"); it is `Fault::Memory` now. A `Fault::Memory` is a storage failure of the
  step (the deck is tried again), never `DeckTooLarge` (the deck's own fault, not tried again);
  `Crowded` is set only by the limits on cards, not by a refusal.
- Sweeps (`alloc_refusal_tests::folder`): listing, pending, folder, names, `sources`,
  `write_sources` and a whole job (a changed file and a new one) with each allocation refused.
- The card folder is still the platform `Storage`'s (`list`, `exists`, `modified`,
  `create_dir`): the native adapter over `core.files` (SDK `native::files`, PR #384) is the
  shell's, and an ignored case names it.

## Ported (the native shell, first part): `src/native_plugin.rs`, `declarations.rs`

- `NativeVocabulary` (`native_plugin!`, features `linked` and `native-entry`, metadata in `native/`
  — regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
  --bytes native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`;
  `tools/tests/test_abi_plugin_metadata.py` fails on a stale one). Declares `uses.storage`.
- The eight settings (`declarations::settings`, pinned in `testdata/declarations.bin.v1`, compared
  field by field with what interface 6 registered in `tests/settings_parity.rs`): `collect`, `mark`,
  `new-per-day`, `retention`, `cards-from-lookups`, `review-on-wake`, `sd-auto`, `sd-folder`, all in
  `core.group.reading`. The seven settings of the role of the marked words (`vocabulary.learning.*`)
  come with the marks.
- The start event puts right what a power cut left (`store::recover`, `cards::recover`, over
  `DataFolder` on the host's data folder); nothing else runs yet. Best effort as in interface 6.
- Not declared yet: the events for the words looked up and the wake (the
  native word-looked-up path is not defined; the dictionary port, #425, is where it will come from).
  The flash cards' action, entry and screens are declared (next section).

## Not ported (follow-ups; each is an ignored case in `tests/acceptance_pending.rs`)

- `sdcard`, `sdmenu` (done: see *Ported: the card files*), `phone` (done: see *Ported: the phone page*);
  `fsrs.rs` is pure and needs nothing.
- The rest of the native shell: the scan action, events and help.
- The sleep card: done, see *Ported: the sleep card* below.
- The widgets: done, see *Ported: the home widgets* below.
- The dictionary hand-off (tap a word) on `core.lookup` (#352, draft 23).
- Marks in books: done on the stage, see *Ported: the marks in books* below (the reader's side asks the stage per page: Plugin Engineer, after #1275).
- Phone page (`phone.rs`): done, see *Ported: the phone page* below.
- Card-folder files: done, see *Ported: the card files* below.

## Ported: the marks in books (`native_marks.rs`, `native_marks_stage.rs`, `stages/decorations/`; #257)

- Vocabulary subscribes to the reader's stage `reader.decorations@1` (`reader.decorations@1.def` is a copy of the reader's,
  held to it by `tests/stages.rs`; the codec is `src/stages_decorations.rs`, `tools/stage-codec.py stages/decorations
  src/stages_decorations.rs`) with the token 8. The reader sends a page as `decorate.page` (path, language, number of runs)
  and `decorate.run` rows (the page's text of a section, words apart by one space); the plugin answers one `decorate.look` (number 0,
  role `vocabulary.learning`) and one `decorate.marks` (`run`, `from`, `to` as little-endian u16 each, at most 3,600 bytes: 600 marks
  a fire).
- **What is marked** is what Beta 3 marked (`marks.rs`, the SDK's `WordSet`): the words and phrases of the list that belong to the
  book's language (primary subtag, any language where either side says none), found as whole words without regard to case
  (`ß` as `ss`, apostrophes inside words kept, accents counted), the longest entry winning, a mark covering the words as the page
  shows them (punctuation around them too), no entry across a token with no word. The search is `native_marks::Entries`
  (fallible allocation, sorted 32-bit hashes of the folded entries, binary search; no `WordSet`, which allocates infallibly).
- **Memory (#248):** nothing is kept between two calls of the stage (Beta 3 kept the set by the list's change count). The set
  is 4 bytes an entry (and 4 for each start of a phrase) in a side file of the language (`marks[-<language>].idx` in the data
  folder: magic, the list's size, a hash of the list's last 512 bytes, the number of hashes, the hashes; a file shorter than its count
  says is not read), read in 512-byte pieces; it is made again from the list (one pass, by the same code) when the list's size or
  its end is not what it was made from (a word replaced by one as long at the 2,000 cap changes the end). Measured on the host
  (in-memory storage, a list of the most, 2,000 words of 250-byte sentences = 562,890 bytes): a page turn reads 8,016 bytes of the side file and the list's last 512 and
  takes 4 µs, where the previous version read the whole list each page (562,890 bytes, 785 µs, a set of about 32 KiB on the
  heap); the heap held for the call is 8,000 bytes (4 bytes a hash), released at its end. Not measured on an X4. **A hash
  shared** by an entry and a word of the page (about one in 2^32 for each pair) marks a word that is not in the list: a tap
  looks it up, nothing else.
- **The look** is read from the role's seven settings each call (`vocabulary.learning.background`, `.ink`, `.line`, `.line-tone`,
  `.thickness`, `.frame`, `.struck`: group `core.group.marks`, section `vocabulary.role.learning`, Beta 3's defaults: a dotted line,
  four tenths of a millimetre; declared in `declarations.rs`, held to the interface-6 registration by `tests/settings_parity.rs`,
  the declarations golden blessed for them) and sent in the stage's fixed codes; taps are always taken. The switch is
  `vocabulary.mark` (off: no marks).
- **A tap** on a mark of the role (`decorate.tapped`) opens the lookup slot `core.lookup` with the word (the punctuation around
  it cut) and the language it was kept in, and answers `decorate.taken` 1; a mark of another role is not ours.
- Remarks: the reply's marks are up to 600 a fire; the reader keeps at most 256 a page. **Not run on a host:** the reader does not
  ask the stage yet (`tests/acceptance_pending.rs`); the scripted host lends events no reply buffer, so the reply is tested as
  records (`native_marks_stage`) and the search on a list (`native_marks`). The interface-6 `marks.rs` and its decorator go with
  the interface-6 removal.

## Ported: the flash cards' first screen as a view (`native_decks.rs`, `native_words.rs`)

- The rows of the screen (*Review* with the cards due, *Edit with your phone*, *Cards on the SD card*
  with what its folder holds, then the decks with their sizes) are made from owned `Numbers` over a
  `Words` trait (the host's catalog behind `HostWords`), so they are tested in both languages without a
  host; the texts and the five states of the files row are those of interface 6's `decks.rs`. The decks
  and sizes are read by `Numbers::read` over `cards::decks` and `cards::sizes` (a refused allocation is
  `TooLarge`, as there); the cards due, the folder and what it holds are the shell's to give (they depend
  on the clock, the settings and `sdcard`, which are still interface 6).
- `DecksView` draws the list (`ItemList`, title `vocabulary.cards.title`) and, on a chosen row, opens the
  screen of its `Target` with a hand-over (a deck is named under `deck`); `tests` pin that the screen ids
  and the key are those of the interface-6 screens. `native_decks_tests.rs` refuses each allocation of the
  rows and of the numbers in turn.
- Registered with the flash cards' shell (next section). The phone row is always offered (its screen says
  why it cannot serve), the SD-card row where the card can be read. The automatic update of the decks at opening (`SD_AUTO`) waits with the card adapter.
  `paperpaw-ui-kit` joins the dependencies (the lock files of the plugin's three workspaces).

## Ported: save to card (`export.rs`, host-free over `Storage`)

- The save (`Saver`) allocates fallibly: names (`file_name_of`, `backup_name`, a free name
  tried without an allocation per candidate), the part and file paths (made once), the head,
  each piece of rows (one reservation of 8 KiB, quoting reserves its worst case first), the
  backup copy (`files::copy_file`: a part beside the copy, one 4 KiB buffer, put in place when
  whole) and the ledger link. A refusal ends the save as `Failed` ("could not be written",
  never `TooLarge`, which is the 4 MiB rule): the part is removed, what was in place stays.
  A refusal while linking after the file is in place is `NotLinked`.
- `export::update_first` and `Saver::new` return `Result`: a refusal means the save must not
  go on (it would not have read what changed on a computer). Interface 6 turned it into "no
  job" and wrote over the file. The screens (`sdcard.rs`, still interface 6) tell it as
  "save failed".
- `sync::tidy` takes a predicate (no list of names is built). `report()` moves the strings.
- `files::put_in_place` (and so `copy_file`, the ledger and the save) moves a part into place with
  a plain `rename`, no copy-and-delete fallback: the `core.files` adapter the shell builds over the
  card folder must offer rename (`move_entry`, with replace), or the plugin must switch to the
  SDK's `move_to` fallback.
- Not done: `sdcard.rs` and `sdmenu.rs` (screens, with the native shell); the card folder is
  still the platform `Storage` (the adapter over `core.files` belongs to the shell).

## Ported: the flash cards (`native_plugin.rs`, `native_review.rs`, `native_review_screen.rs`)

- Declared (`declarations::ui`): the action `vocabulary.flash-cards` (the id interface 6 used), its place in
  the Tools menu (order 36), its icon `stack-2`, the screens `vocabulary.cards` (the decks) and
  `vocabulary.review` (one instance for each deck or card named in the hand-over: the default
  per-parameters reuse of interface 6), and the minute tick. `core.ui` joins the services in the metadata.
- The decks' screen reads the numbers when it opens and when it is shown again (a review changed what is
  due), the focus kept; the words looked up that wait become cards first (`take_lookups`, as interface 6's
  `load`). `Numbers` is built over `DataFolder`; the due count is the widget's (`due_now`).
- The review: `native_review.rs` holds the state and the decisions over a `Storage`, a `Time` and `Words`
  (tested without a host: `native_review_tests.rs`), `native_review_screen.rs` draws it (card, *Show
  answer* or the four answers with when each would bring the card back, the cards left in the header)
  and takes the input (*Select* and a tap show the back and answer; left and right move the focus, which
  starts on *Good*; *Back* is the host's). Each answer is kept at once (`cards::record`, with
  "could not be kept" told if it fails), the answers go into the decks when the screen closes
  (`ScreenClosed`), and a card answered *Again* comes back in this review after its 10 minutes.
- Differences from interface 6, on purpose: the minute tick (`event.tick`) wakes the review to show such a
  card (interface 6 woke exactly then with `wake_at`; there is no wall-clock alarm for a screen yet); the
  card text is wrapped to 12 lines at most; a deck screen shown below another reads its numbers again
  only when it is shown.
- Time: `HostTime` reads the snapshot's wall clock and zone offset for each input (the device may have
  none: the last time known stands, as in `cards::now_of`).
- Not offered until their screens exist: the scan action (`vocabulary.scan-card`), the words' list
  (done: next section), marks in books. Each is an ignored case in `tests/acceptance_pending.rs`.
- Not proved here: drawing and input of the two screens on a scripted host (H1: the scripted host
  refuses screen commands), a snapshot of the review.

## Ported: the list of words (`native_list.rs`)

- Declared: the action `vocabulary.open` (the id interface 6 used), Tools menu order 36, icon `vocabulary`,
  the screen `vocabulary.list` (the interface-6 id).
- Newest first, each row the word with the sentence it was found in as its note (else the book's title).
  Up to 2,000 words, so only the rows of the page shown are made: `WordRows` is the kit's `RowSource` over
  `store::range` (`WindowList`, #663), the page count follows from the word count and the fixed row height.
  Counted when the screen opens and when it is shown again (words looked up meanwhile; a dialog closed).
  No word: the empty state (*No words yet*) as interface 6.
- Holding a word (or *Menu*) asks what to do with it where its book is on the card (`core.files` `stat`): a
  choice *Go to the book* / *Delete* (the texts of interface 6; *Look up* waits for the dictionary's panel on
  `core.lookup`, #352/dictionary port, and is not offered); *Delete* asks the yes-or-no with *Delete* / *Keep*.
  Without the book (no path, or taken off) the yes-or-no is asked directly, as before. One question at a time. Yes
  removes the word (`store::remove`, "could not be deleted" told on a failure), then the list is counted again.
- *Go to the book* (`native_goto.rs`): `command.open` into `core.reader` (the reader the user bound there) with
  the book's path and the word's section and offset (`docs/abi/document.md`, *Opening at a place*), as Notes
  overview opens a note.
- *Export* (`native_export.rs`) is the row after the last word, as in interface 6 (title and help
  `vocabulary.export`, `.export.help`). Choosing it writes all words to `Notes/Vocabulary.tsv` beside the
  library folder (the folder of `core.library-folder`, default `books`): staged in the data folder
  (`export.part`, `store::export`), then moved onto the card with `core.files` `move` (replacing the old file)
  once whole, so a refusal (memory, a full card) leaves the old file and removes the staged one. The reader is
  told "Saved as {path}." or "The words could not be saved." (en and de, interface 6's texts). The notice is best
  effort: a refusal of it is not an error of the export. The whole export runs in one callback (as in interface 6);
  it moves to the work service (`core.work`) when long exports matter.
- *Go to the book* is offered when the book's file is on the card; every refusal of the check other than memory
  counts as "no book" and the hold asks about deleting directly.
- A tap or *Select* on a word says "Hold a word to delete it." (`vocabulary.hold-to-delete`, en and de) until looking
  it up works.
- Tested: the rows (pages, order, notes), the word a row names, the delete and the count after it
  (`native_list::delete`, also over a storage that fails), both languages of the question, and every
  allocation of an *Again* answer refused in turn (`native_review_tests`: the card stays shown). Not tested
  (they need the host): that the `Answered` event is matched to the question and a *yes* (choice 1) deletes,
  the stack query that keeps a tick from redrawing under another screen, and what is drawn.
- Differences: the words looked up while the list is open show when it is shown again, not at once (no
  word-looked-up event yet). Rows are numbered from the newest, so a hold first counts the words again: a list
  that changed since it was counted is counted again and drawn anew, never deleting another word.

## Right-to-left (`ar`: a regression against Beta 3 until this is done)

Beta 3 mirrored the interface where its language is written right to left (Arabic). The UI kit now
does so for what it draws (`Style::rtl`, read from `core.language`: header, list rows, items, dialog,
chips, the sheet's pager; `Style::flip`, `flip_x` and `text::label_in` for the rest). What this plugin
draws itself is not mirrored yet, so its screens are left to right in those parts for `ar` until it
takes the kit's helpers (and, where it keeps its own keys, swaps left and right as `Chips::step` does).
## Ported: the card files (`native_card.rs`, `native_sd.rs`, `native_sd_shell.rs`)

- **The card as a `Storage`** (`native_card.rs`): `core.files` (folders, information, moves) and the card root of the
  storage calls, behind a `Card` trait like the file manager's adapter (the same code; a shared SDK helper would
  take both). Differences: `rename` **replaces** (the save and the ledger put a part in place with it, as
  `files::put_in_place` expects). `Both` puts the plugin's data folder (`.paperpaw/plugins/vocabulary/…`: the
  decks, the ledger) and the card in one `Storage`, as the data layer and `sync` name them, so the sync and the
  save run unchanged. A move across the two is `Unsupported` (nothing is ever moved between them).
  `with_both` opens the service for a callback and gives it back.
- **The menu** `vocabulary.sd-menu` (`MenuView`): *Update the decks*, then for each deck a row that saves it and,
  where it is linked to a file, a switch for removing the cards the file lacks. Texts, notes and the Anki rule are those
  of interface 6; the rows are made over `Words`, tested in both languages.
- **The update/save screen** `vocabulary.sd-card` (`Run`): the sync `Job` or the export `Saver`, one bounded step at a
  time. Each step is a `core.work` callback (`WorkDue`, key `vocabulary.sd`) that asks for the next while there is
  one; the screen draws the rows as the host's list, so the progress is **a percentage in the first row** where
  interface 6 drew a bar (no `Canvas`). The question about a deck of the file's name is the two rows *Add them to my
  deck* / *Make a new deck* under the question. A refused allocation ends the run with *Not enough memory…*, never a
  panic; closing the screen cancels a save (no part of a file stays). The flash cards' opening updates from the
  new and changed files (`vocabulary.sd-auto`, once as the decks open).
- The files row of the decks' screen is offered where the card can be read (the service is there).
- Tests: `native_sd_tests.rs` (rows, an update, the ask, a save, the progress, cancelling a save, and every
  allocation of a whole update refused in turn, with the decks still whole). Not tested (needs a host with `core.ui`,
  `core.files` and `core.work`): what the shell draws and the work callbacks.
- Metadata: `core.files` and `core.work` join the services.

## Ported: the words looked up (`native_collect.rs`, `stages_subscribed.rs`)

- Vocabulary subscribes to the stage `reader.word-looked-up@1` (Plugin Engineer's, PR #791; `stages/subscribed/` holds
  a copy of the schema, `tests/stages.rs` pins it equal to the reader's and the generated codec to it:
  `python3 tools/stage-codec.py stages/subscribed src/stages_subscribed.rs`). The call carries the word, the sentence, the book's path,
  the place (section, offset) and, where known, the language and the title; the reply is empty.
- `native_collect::keep` (host-free) keeps the word as interface 6's `record` did: newest first, once, blanks are no word,
  nothing where `vocabulary.collect` is off, and with `vocabulary.cards-from-lookups` a card is queued for the deck
  *Looked up*. A word that could not be kept tells the reader (*The word could not be kept…*), as before. Tested in
  `native_collect_tests.rs`, also with every allocation refused.
- **The interface-6 handler of `core.word-looked-up` stays** until the interface-6 plugin goes (the interface-6 reader
  still fires that event, and the native reader does not run the stage yet, so removing it now would collect nothing);
  the marks, the interface-6 list and `tests/collecting.rs` use it. It goes with the interface-6 removal.
- Not run against a host (hosted construction): that the reader's call reaches `word_looked_up`.

## Ported: the home widgets (`native_widgets.rs`, `native_widget_word.rs`, `native_widget_saved.rs`, `native_widget_due.rs`)

- *Word of the day*, *Words saved* and *Cards due* are declared with the same ids, groups and settings as
  interface 6 (look, size, alignment, `pick`, the parts), in `declarations.rs` (`widgets`), category
  `core.widgets.words`; the word of the day is live. The host makes an instance (`event.widget_made`), measures
  it through the `core.widget` stage and calls the plugin to draw it, as for Stats' widgets. Nothing of a
  callback is kept: what a widget shows is read from the files when it is measured or drawn (one pass over the
  words). A tap opens the list of words (the first two) or the flash cards (the third).
- The minute tick asks the host to draw a widget again when what it shows changed (the day, the size of the
  words file, the number of cards due); a word looked up does the same at once.
- One difference from interface 6: where the device does not know the day, the word of the day is picked by a
  seed made when the widget is made (the plugin has no random source), so it changes each time the screen
  makes the widget, not at each reading of the words.
- Not proved here: measuring and drawing a widget on a scripted host (`tests/acceptance_pending.rs`, hosted
  construction #944). The declarations and the made/dropped events are tested (`tests/declarations.rs`,
  `tests/scripted_widgets.rs`). The interface-6 widgets (`widget.rs`) go with the interface-6 removal.

## Ported: the sleep card (`native_sleep_card.rs`, `declarations.rs` `navigation`)

- *A flash card due* is offered as a sleep screen as in Beta 3: the screen `vocabulary.sleep-card` (one at a
  time) is offered to the slot `core.sleep` at priority -1 (the sleep plugin's own is 0, so it stays the
  default) and is a choice of `sleep.show` (declaration agreed with Port F, as Reading goal's). The host shows
  the screen as the sleep screen and replaces it on waking; the plugin has no stage or wake callback for it.
- The screen shows the front of the card due next, large in the middle (six lines at most), and below it how
  many cards are due, or that none is. Opening it keeps the card shown in `cards/asleep.txt` (`<deck> <id>`),
  forgetting the last one.
- On waking (`event.resume`) the review opens with that card first where the sleep card was the sleep screen,
  or, with `vocabulary.review-on-wake`, where cards are due. The decisions (`next`, `shown`) are host-free and
  tested, with every allocation refused. Not proved here: the slot shown and the wake on a scripted host
  (hosted construction, #944; the ignored case says so).
- The colours are the theme's (`style.paper`, `style.ink`, `style.note`), not Beta 3's fixed white and black: on a
  dark theme the card is light on dark, like the other screens.
- A refused read of the card leaves the sleep screen saying that none is due; a refused read of `sleep.show` or
  of `vocabulary.review-on-wake` counts as "not chosen"/"off", never ends the wake.

## Ported: the help hints (`declarations.rs` `help`)

- The review's four hints of interface 6 (`vocabulary.help.answers`, `.tap`, `.select`, `.back`; any input,
  touch, buttons, any input) are declared as `help.hint` for the screen `vocabulary.review`; the host answers
  them from `core.ui` op 3 `hints` (#892). Tested as declarations (`tests/declarations.rs`).


## Ported: the phone page (`native_phone.rs`, `native_phone_screen.rs`, `native_phone_draw.rs`, `native_key.rs`; #2861, #257)

- *Edit with your phone* on the flash cards' screen opens the declared screen `vocabulary.phone` (single instance).
  Opening it starts a `core.net` server (`SERVER`); its state events and the requests it gets (`event.server`,
  `event.served_request`, declared for this plugin) are answered from `native_phone::answer`, which is host-free
  and tested: good, wrong key (403; too many wrong ones lock the guard, 429, as in Beta 3), malformed (400/404/422),
  too long (413, refused before a byte is read), and every allocation refused in turn (200 or 503, the decks
  whole). The same page (`data/phone.txt`, with the texts of the language filled in) is written to `phone.html` in the data folder and sent from there; the screen shows the
  address and a QR code (copied from the transfer plugin's drawing, a plain canvas) and the key.
- The access key comes from `core.secrets` randomness as in transfer (`native_key.rs`: the same bytes the SDK
  turns into letters; a source that gives no or short bytes gives no key and the screen says so). Nothing is
  served without a key; the key is not logged or stored.
- Declared: `uses.network` optional, `core.net`, `core.secrets`, the screen and the two events. The row is always
  offered; a device without a network says "Not connected to a network. Switch on Wi-Fi, then open this again."
- No interface-6 code is in this path (`phone.rs` stays only in the interface-6 plugin, removed with #1026).
  The cases that need the real server on a host are ignored until native activation (headless test in
  `shells/desktop/tests/headless.rs`).
