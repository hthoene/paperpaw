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
- Still infallible: `Value::as_str()` of `paperpaw-config` allocates for a quoted value with
  escapes. Its `quoted` flag is private, so it cannot be redone here cheaply; it wants a
  fallible variant in `paperpaw-config` (Kernel).
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
- **Still infallible** (a refusal aborts; swept around, not through): what `paperpaw-sqlite`
  allocates while it reads or writes a database (`Value::Text`, `encode_record`,
  `SchemaEntry`, `Database::table` column names; the rows are small and short-lived), and
  `Md5::hex` (32 bytes) for a book without a usable MD5. Both need fallible variants in
  their crates (Kernel).

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
- The overview (`overview_rows.rs`: tiles, the 14-day chart's values and names, the totals line,
  the ways on, `distinct_pages`) and the calendar (`calendar_model.rs`: month math, the first
  day shown for a week start, shades, the chosen day's date, time, pages and books, how keys
  move it, the title and streak note) are host-free too, with the same tests and sweeps. The
  shell passes `week_start` (a setting of the host) and `day_of`. Kept from interface 6:
  `store::scan_recent`/`scan_range` still swallow a read error (what could be read counts).
  The calendar keeps at most 3 books per day (what the screen shows).
- **For the shell:** where `overview_rows::tiles` and `chart` return `None` (no clock) it shows
  `stats.no-clock`, and keeps the old chart title (`stats.last-days`) for the chart it does draw.
  The key, swipe and tap mapping of the calendar is `calendar_model::{key_step, swipe_step,
  tap_day}` (tested against interface 6).
- Not built here: the layout and drawing (tiles side by side or stacked, the bars, the grid),
  the shell (`Plugin` impl, declarations, screens opened with the book id as
  a hand-over param, `Screens`), the chart of a book's last days. `of_book` takes a closure for local
  days. `totals::seconds_per_book` and `of_book` are fallible (`TooLarge`; their sweeps are in
  `merge/alloc_tests.rs`); the interface-6 views show nothing on an error.

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
- `core.read-today` is a stage the asker pulls (above), not an event.
- `make` submits the icon and the stage; the host refuses them (`ERR_UNSUPPORTED`) until it admits
  them (`tests/declarations.rs` pins that these are the only declarations that wait). The stage is
  the only one the plugin tolerates being refused; the rest of `make` fails as for every plugin
  of this port until the host admits icons.

## Ported: the books and book screens (slice 2)

- `src/native_screens.rs`: `stats.books` (token 21) and `stats.book` (token 22), declared in
  `declarations.rs`, kept per instance in the kit's `Screens`. Views are built when the screen
  opens (`ScreenOpened`) from owned data over the data folder, with the host-free rows of
  `book_list`/`book_rows`; the draw callback only replays prepared drawing. A failed read (bad
  UTF-8 in `books.ini`, a refused allocation) is the event's error and the screen stays without
  a view (`draw` then fails with `State`), as interface 6 showed nothing; the list is never
  built from a partial read.
- The books list is the kit's `ItemList` (paging, focus, header with back/pager on touch); an
  empty list says `stats.no-books` (the title of the kit's empty footer, no second text).
  Choosing a book makes a hand-over with `id=<book id>` (`core.ui` op 16) and opens
  `stats.book` with it; the book screen reads the id from its hand-over (op 17).
- The book screen draws its rows at four fifths of a row (no focus, only back does anything) and
  below them the last 10 days read as bars, when there is room for 2 rows (`native_chart.rs`:
  the layout is plain numbers and tested, the painter measures with the font). The bar chart is
  shared with the overview. A book that is not in `books.ini` shows `stats.no-books`.
- Days are UTC days until draft 25 (as in the recorder). Dates and plural forms come from the
  catalog through `Messages` (the host's `Arg::Count` must pick the catalog's plural form; checked
  for the rows in `screens_tests.rs` with the catalog's own `plural()`).
- Sweeps refuse each allocation of both builders (`native_screens/tests.rs`). Pixel goldens of
  these screens need a kernel-backed harness like Battery's (`native_battery_pixel_tests.rs`)
  and are not made here.
- Not yet: the overview, the calendar, the action `stats.open` and its menu entries (the books
  are reachable from the overview), sync.

## Not ported yet (follow-ups; each is an ignored case in `tests/acceptance_pending.rs`)

- Screens (overview, calendar, books, book) on `core.ui`.
- Widgets (today, week, ring, streak, calendar, year) and the status item (`left.rs`) on the
  widget and status stages (host call, Core todo 76).
- Sync (WebDAV conditional upload with ETag, credentials): `core.net` (#351) and secrets
  (Services todo 79).
- Folder listing (`sync` working-file sweep, `merge` tests): `core.files` (#339).
- The fixture `tools/dev/tests/fixtures/flows/stats.flow` (#321) points at the new screens.
