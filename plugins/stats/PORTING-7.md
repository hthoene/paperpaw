# Stats on interface 7 (partly ported)

Status: **not complete.** The interface-6 plugin (registration, screens, widgets, sync,
events) is still in place. It is removed in the PR that finishes the port.

## Ported: the data layer's files (`src/store.rs`, `src/files.rs`)

- File names are constant strings: logging a page allocates nothing.
- `days()` streams `days.bin` in 768 B chunks (no 1 MiB read), allocates fallibly, reads at
  most 4,096 kept days (48 KiB; a longer file is summed anew from the log, not refused),
  returns `Result` (`TooLarge` where the days do not fit) and writes the shortcut through
  `days.new` and a rename, so a cut leaves the old file, never a header covering days that
  are not there.
- `DataFolder` (`files.rs`) takes the plugin folder off the paths, so the layer runs on the
  host's root 0 (`native::storage::DataStorage`, PR #358) unchanged. `put_in_place` needs
  `rename`, which root 0 has. It goes when the layer's paths are the short ones.
- `books.ini` is loaded in id order with fallible allocation (no stable sort, which
  allocates) and written in pieces of at most 3.5 KiB, never held whole. Sections in a
  row of one id (also with empty sections between) still merge as in interface 6; a
  section of an id read before that is ignored, so the first book of an id is the book
  (interface 6 kept it too, by sorting stably and dropping later copies).
- `Value::try_as_str()` (`paperpaw-config`) reads the values of `books.ini`: a quoted value with
  escapes that cannot be allocated is `TooLarge`, not an abort.
- Callers that read today's seconds (`read_today`) do not cache a failed read as 0.

## Ported: `extsort`, `merge`, `koreader` (fallible allocation)

- Every allocation of `extsort` (runs, readers, the `Records` iterator), the working-file
  names, the books of both sides, the pages read per book and the books' names is
  fallible: a refused one is `StorageError::TooLarge` (`Failure::Here`), the working files
  are removed as after any error, and the statistics here are as they were.
- `Records` yields a fixed `[u8; 20]` (no `Vec` per record); `extsort::each` can fail from
  its visitor; `Appender`, `Reader` and `Records` take all their memory when made.
- `Key::from_hex` decodes the MD5 without allocation. `totals::mark_read` is fallible; where
  `totals::of_book` is refused it leaves that stay uncounted (its own pass is still to do).
- `store::write_log` (the `replace_log` path) reserved its buffer infallibly; fixed.
- Allocation-refusal sweeps (`merge/alloc_tests.rs`, a refusing global allocator in
  `testing.rs`): `extsort::sort` (two merge passes), `each`, `Records`, a merge of this
  device alone, `merge_books` and `take_names`.
- `paperpaw-sqlite` and `Md5` are fallible now (todo 104, #383): `koreader::write` takes every
  text, record and schema entry with `room::string` / `try_encode_record` (a refusal is
  noted and the call that took the rows says `Error::OutOfMemory`), and `Key::of` and
  `register_book` use `hex_into` / `partial_into` on the stack. Sweeps cover writing the other
  app's database and a merge with its file (`merge/alloc_tests.rs`).
- A merge writes the new log and the new `books.ini` beside the old ones first (`stage_log`,
  `stage_books`) and renames both after, so a refusal or a full card while writing leaves
  the pair as it was (the sweep found a refusal in `save_books` after the log was replaced).
  Only the rename itself, or the upload's own file after both, can fail past that point.

## Not ported yet (follow-ups; each is an ignored case in `tests/acceptance_pending.rs`)

- The native shell: `Plugin` impl, declarations, `build.rs`, metadata, linked entry.
- Screens (overview, calendar, books, book) on `core.ui`.
- Widgets (today, week, ring, streak, calendar, year) and the status item (`left.rs`) on the
  widget and status stages (host call, Core todo 76).
- Events: page turned, book opened/closed/finished/moved, started/resume/suspend/stopping,
  and emitting `stats.read-today` as a stage for reading-goal.
- Sync (WebDAV conditional upload with ETag, credentials): `core.net` (#351) and secrets
  (Services todo 79).
- Folder listing (`sync` working-file sweep, `merge` tests): `core.files` (#339).
- The fixture `tools/dev/tests/fixtures/flows/stats.flow` (#321) points at the new screens.
