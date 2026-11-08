# Notes overview on interface 7 (partly ported)

Status: **not complete.** The interface-6 plugin (`clippings.rs`, `scan.rs`,
`screen.rs`, the `Plugin` in `lib.rs`) is still in place, because the host
services it needs are not there. It is removed in the PR that finishes the port.

## Ported (no host needed): `src/model.rs`

- The list: books with notes, at most 200, by title without regard to case
  (ties by path), the *Export* row, the "more" flag, the count pieces
  (`3 highlights · 1 bookmark`).
- The `My Clippings.txt` text: BOM, CR LF, Kindle weekday/month names in
  English, `Highlight at location p-p`, own note as `Note`, bookmark without
  text, one-line texts, local time passed in. Written into a fallible
  `Sink`; every allocation can be refused (`Error::NoMemory`), tested at
  every byte of an export (`model/tests.rs`).
- Input is plain `Note` values: the caller copies the host's rows out, so
  the model does not depend on the library facade.
- Tests of the interface-6 `clippings.rs` moved over unchanged (date, entries,
  book line); texts (en/de) are unchanged.

## Ported (over `Storage`, no host service needed): `src/export_file.rs`

- `PartFile` is a `Sink` over the platform `Storage`: `Exports/My Clippings.txt.part`
  made anew, appended to in 6 KiB pieces from one reserved buffer, renamed into place
  when whole (`finish`). A refusal (memory: `NoMemory`, full card: `Full`, other: `Io`)
  or a drop before the end removes the part and leaves the old file; where a failed
  rename took the old file (FAT), the part is kept (it is all there is).
- The rename must replace an existing file: the `core.files` adapter the shell builds
  must offer it (`move_entry` with replace). Tested on a storage that allocates nothing,
  every allocation refused in turn (old file kept, no part left), and a full card at
  four places.
- `model::put_detail`: the second line of a note's row (`what · 42 %`).

## Ported (over the library service, slice 2): `src/native_scan.rs`, `src/native_library.rs`

- Which books have notes: `native_scan::scan` passes the catalog (by title) a page at a time and
  counts each book's highlights and bookmarks by paging its annotations (`core.library` op 15, one
  4,096-byte reply at a time; kinds only, the 512 B previews are not kept). Only the books of the
  list are kept (200, `MOST_BOOKS`); once it is full, a further book with notes is looked at until
  its first highlight or bookmark and only sets `more`, so a library of thousands costs host calls,
  not memory. The interface-6 scan listed the folder of notes files first; there is no such folder
  on interface 7.
- Memory of the list, worst case: a listed title is cut to `TITLE_MAX` = 128 bytes (at a
  character's end), and a book with a path over `PATH_MAX` = 512 bytes (the most the library names
  a book by; `BookRef::path` refuses longer ones) is left out, since it could not be opened from
  the list. So 200 books hold at most 200 x (128 + 512) = 128,000 bytes of text, plus 200 x 56
  bytes of headers (about 139 KiB; a library of ordinary paths and titles needs about 30 KiB).
  Interface 6 cut neither.
- The walk ends at the first book past the 200 (it only sets `more`), without walking the rest of
  the catalog. So the books listed are the first 200 with notes **in the catalog's title order**;
  interface 6 took the first 200 in the order of its walk of the library and then sorted them
  (deliberate; accepted in review). The list is sorted again by title without regard to case, ties
  by path, as before.
- The scan is written over a `Source` trait; `native_library::HostSource` fills it in from the SDK's
  `Library` (`query`, `annotations`, `book`; key and catalog generation make a `BookRef`). Tested
  over a scripted library (`src/native_scan_tests.rs`): order and counts, any page size, a full list,
  stale pages (a book's notes are read again, the whole scan after that, `RESTARTS` = 3 each, then
  `State`), replies that do not add up (rows over the total, a next page without rows: `Invalid`, so
  a host that never ends cannot hold the scan), a book gone meanwhile (left out), and every
  allocation refused in turn. `HostSource` itself is thin and not covered without a host (no SDK
  mock host): it compiles against the SDK and is exercised by the acceptance cases below.
- Not read here: the cut texts (op 4) and the notes' own texts; the export slice does that.

## Not ported yet (each is an ignored test in `tests/acceptance_pending.rs`)

- The notes' rows with 512 B previews and the cut values by op 4 in 1,024 B parts (the export
  and the book's screen).
- Files (PR #339): `Exports/` folder, `.part` file, append and rename.
- Native screens (core.ui compose, Input H3) and the draft-22 `doc_open` to open
  a book at a note.
- Registration (action `notes-overview.open`, Tools menu entry order 37,
  icon `bookmarks`): the host does not admit menu entries and icons from native
  plugins yet (as in `plugins/power/PORTING-7.md`).
