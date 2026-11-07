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
  a hand-over param, `Screens`), the chart of a book's last days. `totals::seconds_per_book`
  is still infallible (`vec!`) and `of_book` takes a closure for local days; the shell needs
  `seconds_per_book` fallible.

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
