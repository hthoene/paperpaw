# File manager on interface 7 (partly ported)

Status: **not complete.** The native shell is in (below); the interface-6 plugin (`lib.rs`:
`Plugin`, `Browser` screen, `folder.rs`) is still in place, because the library bookkeeping
(books opened, books told of a move) and the host admission of menu entries and icons are not
there. It is removed in the PR that finishes the port; `folder.rs` and the SDK `files`
helpers it uses are then dead.

## Ported: the native shell (browse, rename, move, copy, delete, new folder)

- `src/native_plugin.rs` (`NativeFileManager`), built-in descriptor `NATIVE_ENTRY` (feature
  `linked`), declarations in `declarations.rs` built into the image by `build.rs`, metadata in
  `native/` (regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
  native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`,
  likewise `metadata.host.*`). Declares `uses.storage` and the services `core.files`, `core.ui`.
  Version 1.0.4.
- Declared as interface 6 registered it (`tests/parity.rs`, field by field): the setting
  `file-manager.hidden` (advanced, bool, off, in `core.library`), the screen `browser`, the
  action `open`, the Tools menu entry (order 45) and the `folder` icon. The menu entry and the
  icon wait for the host's admission like every plugin of this port (`tests/declarations.rs`).
- `src/card.rs`: the card as `Storage` over `core.files` (folders a page at a time, information,
  `make_dir`, `remove_tree`, `move_entry` never with replace: a taken name, file or folder, is refused
  and both stay; `exists` is false only on "not found", a read error counts as taken) and the card root of the storage calls
  (reads and appends in pieces of 4 KiB). The host calls are behind a trait so that the
  translation (pages put together, errors told as storage errors, a move that makes the folder it
  goes into, a folder that has no size) is tested without a host. A whole write of more than one
  piece is refused (`TooLarge`): the model never makes one. The service buffers (1.5 and 4 KiB)
  are taken once per callback on the heap, fallibly.
- `src/browser.rs`: the folder on show (a page held, the focus on the list or the bar, the
  picked entry, the line that says what happened) and the changes, over `Storage`, tested with
  the memory storage and the fake card. `src/native_draw.rs`: layout and drawing with the kit
  (header with the back arrow and pages, rows of name and size, the line, the bar).
- Behaviour kept: folders first then names, hidden files only with the setting, tap or confirm
  opens a folder or offers *Rename, Move, Copy, Delete* for a file (hold or the menu button for
  any entry), *Paste here* puts the picked entry under a free name (`name (2)`), moving into
  itself is refused, deleting asks first and starts on the safe answer, PaperPaw's own folder is
  never moved, renamed or deleted, names that FAT or Windows would not take are refused, Back goes
  up a folder before it leaves.

### Gap: the library is not told (on purpose, and nothing is lost)

Interface 6 told the library of every book moved, renamed or deleted (`library::tell_moved`,
`books_in`, `BOOK_REMOVED`), so that the reading place, bookmarks, notes and statistics kept by a
book's path went along. The services for that (`core.library` op 15 and ops 19 to 21, Services/
Host work) are not there. Meanwhile:

- A move, a rename and a copy only touch the files of the user: a copy is whole or not there
  (`.part` beside it, put in place whole), a move is a rename or a copy then a delete, never a
  delete first; a folder is never moved into itself; nothing is replaced (a taken name gets a
  free one). What the plugins keep by a book's path is **not** deleted or changed: it stays where
  it is, under the old name, and is found again when the book is moved back.
- After a move or a rename the line above the bar says so (`file-manager.not-told`: the book's
  place and notes stay under its old name for now). Deleting does not remove the data kept for a
  book either (it stays, unused, until the storage is cleaned up), and the question says only
  what is deleted.
- Not in this slice: *Open*, *Open with*, *Reading app* in the options (the readers service and
  `core.format`, `tests/acceptance_pending.rs`), the refusal of a folder with more than 2,000 books
  (it was a limit of telling) and `moved-many`/`too-many-books` texts (kept in the catalogue for
  the interface-6 plugin, which stays).

### Other differences and open points

- Deleting a folder removes it with `core.files` `remove_tree` in a loop, not cancellable and
  without progress (Kernel's ruling for `remove_tree`); a very large tree blocks the callback.
  `remove_step` across ticks would give progress and cancel: the work service is not used yet.
- The rows have no folder or file icon (the kit draws none); folders end in `›`.
- `Back` first goes up; at the top it closes the screen. The screen cannot be opened with a
  folder as hand-over yet (interface 6 took `params::PATH`).
- Peak memory and the time of a page of a 1,000-file folder on the X4 are not measured: device
  gate open. The `.ppn` size is not measured either (no X4 build in this slice).

## Ported (host-free, over `Storage`): `src/model/`

The native shell gives the model the platform `Storage` (the `core.files` adapter over
`native::files`, whose folder reads are page by page); nothing here holds more than a page
of entries, two paths, a page per folder level or one 4 KiB copy buffer.

- `path`: names, parents, `same_path`, `is_within` (no allocation), fallible `join`/`copy`,
  `free_path` (`name (2).ext`, one reused buffer).
- `folder`: `Folder::{count, rank, after, before, page, page_of}`, the keyset pages in the
  order folders first, then names regardless of case, then exact names (moved from `folder.rs`,
  fallible: each page reserves `n + 1` once, each name is copied fallibly).
- `ops`: `move_to` (rename, else copy and delete; never a folder into itself), `copy` (file
  to a `.part` beside it through a 4 KiB buffer (interface 6: 16 KiB; one fallible allocation per file), put in place whole; a folder with everything in it, a page of 8
  entries at a time and at most 16 levels deep, `NoSpace` beyond), `delete`, `rename` (to a
  free name where the name is taken; through a name between where the storage ignores case).
- Errors are the host's (`paperpaw_sdk::native::Error`): a refused allocation is `NoMemory`;
  a storage failure that has no variant of its own (card gone, any other I/O failure) is `Io`
  (interface 6 knew `CARD_GONE`: the shell tells it from `Io` by the storage's state).
- A file is never copied (or moved on a storage without rename) onto itself, whatever the
  case on a storage that ignores it: that would empty it first (`Invalid`).
- Tests: the pages and ranks of the interface-6 `folder.rs` tests on explicit expectations;
  every allocation refused in turn for the pages, a tree copy, a file copied over another, a
  move without rename and the three renames (source kept, a copy whole or not there, no
  `.part` left, no `.renaming` name left).

Behaviour changes, on purpose: a folder is copied a page at a time (interface 6 held the
whole listing, and a storage may not take calls from inside its listing); folders nested more
than 16 deep are refused (`NoSpace`) instead of recursing; `delete` of a missing path is
still not an error (the storage's rule).

## For the shell and for activation

- The shell needs its own texts for `NoSpace` (a folder nested deeper than 16, a path over
  the cap) and `NoMemory` (a refused allocation); interface 6 had no such messages.
- Measure a folder of 1,000 files on the X4 when this is activated: a page of 8 entries
  is one directory pass (`Folder::after`/`before`), so a page costs a pass over the folder and
  a copy of a big folder costs one pass per 8 entries. Raise the page (a few more names on the
  heap) if the measurement asks for it.

## Not ported yet

- The `Plugin`/`Browser` screens on `core.ui` compose (Input H3), the choice overlay, the
  keyboard and confirmation replies (`Question::choice`/`Ui::ask`), the registration (action,
  Tools menu entry order 45, icon `folder`, the `file-manager.hidden` setting) as in
  `plugins/power/PORTING-7.md`.
- Out of memory: a host out-of-memory in a card call (`card::Watch` notes it) reaches the
  reader as "not enough memory" (`Browser::lacked_memory`), though `StorageError` has no word
  for it. Sweeps (every allocation refused in turn, files intact) run at model, card and
  browser level over the memory storage and the fake card. Not covered: the allocations of
  `HostCard::open` and the `core.ui` glue (they need a host); `take_buffers` (a nested call is
  `Busy`) is tested on its own.
- The reading-app bookkeeping (`library::books_in`, `formats().supports_file`, own data
  folders): `core.library` (op 15 / Services) and `core.format`.
- Reading `size` for the row text (`ops::shown_size`, from the SDK) and the page-turn
  state of the screen.

## What blocks removing interface 6 (checked 2026-11-09, base `abi7/integration`)

The interface-6 `impl Plugin` stays until these exist; each is asked of Services (the manager
queued them):

- **`Documents::readers`** (DOC op 21, `document.md`) in `native::document`: *Open with* and
  *Reading app* list the plugins that can read a file; the native plugin has no way to ask.
- **`Library::delete`** (LIB op 10, #668, only on the draft-30 base): deleting a book also removes
  the data kept for it, as Beta 3 does (today the native plugin leaves it, see the gap above).
  `Library::can_delete` (op 9) is the preflight.
- **Not blocking:** moves. The host follows `core.files` moves itself (journal and job in
  `files_service` and `lib_service/moves.rs`, `files.md` 2.6), so the library is told by the host
  and the `not-told` line can go once host acceptance is possible. `Library::move` and the
  2,000-book limit are not needed.

Version: 1.0.4 is above Beta 3 (1.0.3): no bump when the port finishes.
