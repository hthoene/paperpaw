# Vocabulary on interface 7 (partly ported)

Status: **not complete.** The interface-6 plugin (registration, screens, widget, marks,
flash cards, phone page, sync, apkg) is still in place. It is removed in the PR that finishes
the port.

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
- `clean_or_empty` and `path` stay only for `sync`, `export`, `phone`, `apkg` until they are ported.
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
  stays for `apkg` (interface 6) until it is ported: where the heap refuses, the text of that
  Anki note field becomes empty (a note whose front is then empty is skipped as a card without
  a front), not an abort and not half a text.
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
- `apkg::tidy` names its two files by constants and allocates nothing (the rest of `apkg` is
  next).
- Sweeps (`alloc_refusal_tests::folder`): listing, pending, folder, names, `sources`,
  `write_sources` and a whole job (a changed file and a new one) with each allocation refused.
- The card folder is still the platform `Storage`'s (`list`, `exists`, `modified`,
  `create_dir`): the native adapter over `core.files` (SDK `native::files`, PR #384) is the
  shell's, and an ignored case names it.

## Not ported yet (follow-ups; each is an ignored case in `tests/acceptance_pending.rs`)

- `export`, `sdcard`, `apkg`, `phone` (they still use `format!`, `collect`,
  `to_lowercase`; `fsrs.rs` is pure and needs nothing). Next slices.
- The native shell: `Plugin` impl, declarations, `build.rs`, metadata, linked entry.
- Screens (word list, decks, card files, review, sleep card) on `core.ui`.
- The widgets on the widget stage (host call, Core todo 76) with Port A's toolkit (#366):
  `widget::{cx,names,options}`; no drawing of its own.
- The dictionary hand-off (tap a word) on `core.lookup` (#352, draft 23).
- Marks in books (decorations).
- Phone page (`phone.rs`): `core.net`/sockets, secrets (Services todo 79).
- Card-folder files: `core.files` (#339).

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
