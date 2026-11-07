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
  appended, as for a read error (the old entry then stays).
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

## Not ported yet (follow-ups; each is an ignored case in `tests/acceptance_pending.rs`)

- `export`, `table`, `sync`, `sdcard`, `apkg`, `phone` (they still use `format!`, `collect`,
  `to_lowercase`; `fsrs.rs` is pure and needs nothing). Next slices.
- The native shell: `Plugin` impl, declarations, `build.rs`, metadata, linked entry.
- Screens (word list, decks, card files, review, sleep card) on `core.ui`.
- The widgets on the widget stage (host call, Core todo 76) with Port A's toolkit (#366):
  `widget::{cx,names,options}`; no drawing of its own.
- The dictionary hand-off (tap a word) on `core.lookup` (#352, draft 23).
- Marks in books (decorations).
- Phone page (`phone.rs`): `core.net`/sockets, secrets (Services todo 79).
- Card-folder files: `core.files` (#339).
