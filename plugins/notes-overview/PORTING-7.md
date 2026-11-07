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

## Not ported yet (each is an ignored test in `tests/acceptance_pending.rs`)

- Annotations from `core.library` op 15 (Services todo 93): counts by first-page
  `end.total`, rows with 512 B previews, cut values by op 4 in 1,024 B parts.
- Files (PR #339): `Exports/` folder, `.part` file, append and rename.
- Native screens (core.ui compose, Input H3) and the draft-22 `doc_open` to open
  a book at a note.
- Registration (action `notes-overview.open`, Tools menu entry order 37,
  icon `bookmarks`): the host does not admit menu entries and icons from native
  plugins yet (as in `plugins/power/PORTING-7.md`).
