# File manager on interface 7

Status: **native only.** The interface-6 plugin (`Plugin`, `Browser` screen, `folder.rs`) and its
kernel tests are removed; the native plugin is linked (`NATIVE_PLUGINS`) and listed in
`tools/plugin-files7/complete.txt`. Host acceptance waits for the activation of native plugins
(`tests/acceptance_pending.rs`).

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

### Books: open, open with, delete (through the library)

- A file the library takes for a book (the delete preflight `Library::can_delete` answers `OK`; any
  other answer, or no library service, makes it a plain file) offers **Open**: `command.open` into the
  slot `core.reader` with the hand-over `path` (the reading app the user chose), and, where more than
  one reading app reads its format (`Documents::readers`, scope the book's `Library::book` format, all
  readers where the library does not know the book yet; at most 12), **Open with**: a choice of the
  apps by their titles, opened with `Open::in_slot(screen, "core.reader")` (this time only).
- **Deleting a book** asks (`file-manager.delete-book-question`) and then `Library::delete` with notes:
  the book and everything kept for it (position, annotations, fields, plugins' book data, caches,
  cover). A refusal (`Busy`, `State`, card) is the line above the bar; `Invalid` (not a book after all)
  goes to the card as a file. A folder is deleted with `core.files` `remove_tree`; the books in it are
  forgotten by the host's next walk, with what is kept for them.
- **Moves and renames:** the host follows `core.files` moves itself (`files.md` 2.6), so the library is
  not told from here and the line "the library was not told" is gone (with its texts
  `file-manager.not-told`, `moved-many`, `too-many-books`). The 2,000-book limit of a folder is gone.
- **Dropped on purpose: "Reading app" for a folder** (the per-folder choice of interface 6, kept in
  the files plugin's data): `document.md` section 1 does not carry the interface-6 resolver (a reader
  per book, folder or format); the user's reader is the one bound to `core.reader`.
- A file the library cannot judge (no `core.library`) is offered no *Open* here; the open screens of
  the reading apps are reached from the library and the home screen.

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

## Open points

- Out of memory: a host out-of-memory in a card call (`card::Watch` notes it) reaches the reader as
  "not enough memory" (`Browser::lacked_memory`). Sweeps (every allocation refused in turn, files
  intact) run at model, card and browser level over the memory storage and the fake card. Not covered:
  the allocations of `HostCard::open` and the `core.ui`/`core.library`/`core.document` glue
  (`native_plugin.rs`: offer, open, delete, readers), which need a host: no scripted-host test drives
  them yet.
- Host acceptance (the screen on the real kernel, opening a book, deleting one with what is kept for
  it) is pending activation (Core #944, Host #417) and the host's library and document services; the
  cases are ignored in `tests/acceptance_pending.rs`.
- The interface-6 kernel tests (`tests/manage.rs`, `manager.rs`, `names.rs`, `parity.rs`, the lib
  tests) are gone with the code; the model, card and browser tests carry the behaviour, and
  `tests/declarations.rs` pins what the plugin declares.
- X4 `.ppn` size and peak memory are not measured.

Version: 1.0.4 is above Beta 3 (1.0.3): no bump.
