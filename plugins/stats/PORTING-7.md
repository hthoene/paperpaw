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
  `stage_books`) and then puts both in place with `commit_pair`: a marker file
  (`merge.commit`) says that both are whole, then the two renames follow, the books first. A cut
  between them leaves the marker and the log beside its place, and the next start (`recover`, and
  `update_books`) finishes the rename; a cut before the marker leaves the old pair and the
  staged files go as unfinished. A refusal or a full card while writing leaves the pair as it
  was (the sweep found a refusal in `save_books` after the log was replaced; the cut test
  `a_cut_between_the_renames_...` found that recovery took the staged books for unfinished).
- A rename that *fails* (FAT removes the old file first, then fails) is treated like a cut: where
  the old file is gone, or the log fails after the books, nothing staged is deleted and the
  marker stays; the next start finishes both (`a_failed_rename_that_took_the_old_file_...`).
  Where the first rename fails with the old books still there, the old pair stays and the staged
  files go. While a pair cannot be finished (`finish_commit` fails again) `recover` leaves all
  files alone and `update_books` fails (`Io`) rather than change the books over it; stays
  recorded meanwhile go to the old log, which the roll-forward replaces (a narrow window that
  only a failing card opens). Where the card cannot rename at all the books are written in place
  as before.

## Ported: the rows of the books and book screens (`texts.rs`, `book_list.rs`, `book_rows.rs`)

- Host-free: rows are made over `Words` (catalog texts, fallible; `HostWords` is the host's
  `Messages`) and shown by the shell in an `ItemList` (kit). Tests in `screens_tests.rs`
  cover both languages, local days, ordering and refusals of the heap.
- Local days come from a `day_of: impl Fn(u32) -> i32` the caller gives (as `store::days`
  takes it), so summer time and the zone plug in. Reading the zone the user chose
  (`core.time-zone`, `core.time-zone-rule`) is the shell's and not built yet. The system
  zone is not in the snapshot (draft 25 adds `utc_offset_minutes`, per Port A).
- A book's last open (Unix seconds, `i64`) is cut to a `u32` for its day, as in interface 6;
  a time past 2106 wraps.
- **For the shell:** the host's `Messages::format` with `Arg::Count` must choose the same
  plural form as the catalog's `plural()` (the tests use `plural()`): check it with the host
  catalog in both languages before activation.
- Later, with the shell: an `inert` flag on `RowItem` (not selectable, normal ink) for the
  book screen's rows, with Port C; not `disabled`.
- Differences from interface 6: the rows of the book screen can take focus (an `ItemList`
  has no unselectable value rows); the books are ordered by last open, then id (the
  interface-6 sort was stable over the same order).
- Not built here: the shell (`Plugin` impl, declarations, screens opened with the book id as
  a hand-over param, `Screens`), the chart of a book's last days, the overview and the
  calendar. `totals::seconds_per_book` and `totals::of_book` are still infallible and
  interface-6 shaped (`Env`); the shell needs fallible versions.

## Ported: the native shell and the recording (slice 1)

- `src/native_plugin.rs` (`NativeStats`), built-in descriptor `NATIVE_ENTRY` (feature `linked`),
  declarations in `declarations.rs` built into the image by `build.rs`, metadata in `native/`
  (regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
  native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`,
  likewise `metadata.host.*`). Declares `uses.storage` (the books are root 1, read only, for
  KOReader's fingerprint).
- Settings: group `stats.group` (order 25), `enabled`, `min-seconds` (1-60, default 5) and
  `max-seconds` (30-600, step 10, default 120), the last two advanced, unit `stats.unit.seconds`,
  keys, titles and help as in interface 6 (`testdata/declarations.bin.v1`); chart icon on the group.
- `src/recorder.rs` is the recording of interface 6 over the events (book opened/closed/
  finished/moved, page turned, suspend/stopping/resume/started, status changed for the clock):
  host-free, over `Surroundings` (data folder, user files, monotonic clock, wall clock, day,
  limits). Stays are timed on the event time and dated from a snapshot (`unix_at`); read before the
  device knows the time they wait (128 at most) and are recorded when `StatusChanged` says it does.
  A write that fails is told once (`stats.not-saved` as an announced notice).
- The stage `stats.read-today` (schema 1, reply: day `i32` and seconds `u32`, little-endian, none
  where the date is unknown) is declared for reading-goal; it replaces the event
  `core.read-today` that interface 6 sent when the figure changed (the asker pulls).
- Everything the recorder takes is fallible: names of the books are noted once in a vector
  (`Rc<str>` cannot be made fallibly; the sweep found it), the book added to `books.ini` is made
  with `room::string`, a refusal leaves the book out (no half book, no stay twice).
  `recorder/alloc_tests.rs` refuses each allocation of a session of two books with the clock known
  from the start and learned in the middle.

### Differences from interface 6 (on purpose)

- A book is added with the title the reader shows (the event has no authors and language, and
  the document service is not in the SDK yet): authors and language are *N/A* as for a book KOReader
  knows no file of; a book without a title is named by its file. Books KOReader already knows
  keep their names (`merge` takes names from the other side).
- Days are UTC days until the system time zone comes with the snapshot (draft 25); interface 6 used
  the device's zone. The seconds read are unaffected; a stay near local midnight may fall on the
  neighbouring day until then.
- `core.read-today` is a stage the asker pulls (above), not an event. Its day is the **UTC day**
  until draft 25 brings the system time zone, so near local midnight it can differ from the
  user's calendar day by up to the zone's offset: the stage may answer yesterday's (or
  tomorrow's) day for a few hours. **reading-goal must tolerate that mismatch**: it compares the
  day in the reply with its own day and shows no figure (or zero) for a day that is not its today,
  never a neighbouring day's seconds as today's.
- `make` submits the icon and the stage; the host refuses them (`ERR_UNSUPPORTED`) until it admits
  them (`tests/declarations.rs` pins that these are the only declarations that wait). The stage is
  the only one the plugin tolerates being refused; the rest of `make` fails as for every plugin
  of this port until the host admits icons.

## Not ported yet (follow-ups; each is an ignored case in `tests/acceptance_pending.rs`)

- Screens (overview, calendar, books, book) on `core.ui`.
- Widgets (today, week, ring, streak, calendar, year) and the status item (`left.rs`) on the
  widget and status stages (host call, Core todo 76).
- Sync (WebDAV conditional upload with ETag, credentials): `core.net` (#351) and secrets
  (Services todo 79).
- Folder listing (`sync` working-file sweep, `merge` tests): `core.files` (#339).
- The fixture `tools/dev/tests/fixtures/flows/stats.flow` (#321) points at the new screens.
