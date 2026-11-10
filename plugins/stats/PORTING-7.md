# Stats on interface 7

Status: **complete.** The plugin is the native plugin only: the interface-6 plugin (the `Stats` plugin in
`lib.rs` and its registration, screens, widgets, sync, `book.rs`, `books.rs`, `calendar.rs`, `chart.rs`,
`left.rs`, `overview.rs`, `sync.rs`, `widget*`, `fmt.rs`), its integration tests (`tests/recording.rs`,
`recording_kernel.rs`, `screens.rs`, `widgets.rs`, `widgets_parity.rs`, `sync.rs`, `webdav_sync.rs`, `books.rs`,
`common/`), its registration in `paperpaw-app` and its store-file builder (`tools/plugin-files`; `plugins.txt`
keeps a comment line) are gone. What waits for hosts is listed under *Not tested here* at the end; the code
that sends and reads it is in place.
The X4 file (`stats`) packs at 486,328 B (format 5, interface 7.1, version 1.0.8; no frame over 2,048 B).

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

- **Memory (review of #470).** Metadata names the service the screens use (`core.ui`) and the host
  calls behind it (`services.query`, `blob.read`, `blob.release`). The books screen keeps at most
  the 200 books read last (`book_list::MAX_ROWS`, most recent first; interface 6 listed every
  book), each title and author line cut to 80 bytes with `…` (`book_list::cut`, at a character).
  The calendar holds no list of books at all.
  A book older than the 200th is not listed (a difference from interface 6, on purpose: the list
  lives as long as the screen is open on a 256 KiB heap). `book_list.rs` tests the cut, the cap and
  the order.
- **`books.ini` is streamed (follow-up of #470).** `store::each_book` makes one `Book` at a time
  (with an 8 KiB `IdSet` of the ids made, for the "first section of an id wins" rule), by the rules
  `load_books` has, which now collects from it. The books screen reads the log once for the ids with
  reading time, then the file once, holding only the 200 most recently opened of them cut to a row's
  texts (`store::most_recent`, `book_list::recent`); a book's screen keeps the one book
  (`find_book`), the overview only counts (`count_books`), and the calendar looks up the titles of the
  at most 3 books of the chosen day as the file streams (`store::titles`). The peak no longer grows
  with the library: `book_list.rs` measures it for 300 and 3,000 books (`testing::heap::peak`).
  The merge and the recorder still load the whole list (they change it and write it back).
  Follow-up: the books screen keeps a slim `book_list::Row` (id, last open, the cut title and authors)
  instead of a `Book` (peak of reading the list 80 KB → 58 KB; `BooksView::build` 91 KB, the same for
  300 and 3,000 books, tested). The calendar's `titles` stops reading `books.ini` once the books of
  the day are found, so **a damaged line after them is not noticed there** (the books and book
  screens read it all and show the same error as before); a title not in the file is read to the end.

## Ported: the overview and the action that opens it (slice 3)

- `src/native_overview.rs`: `stats.overview` (token 20), the action `stats.open` (token 30) with its
  menu entries (tools 35, reader-more 30) and chart icon. `stats.open` records the page being read so
  far (`Recorder::settle`) and opens the overview with the book being read as the hand-over `id`
  (interface 6: a screen parameter); that book becomes the first way on.
- Tiles side by side (today, week, streak: label, value at the largest size that fits, note), the last
  14 days as bars with today framed, the totals line, then the ways on as rows; the geometry is
  interface 6's (`Layout`). Where a tile's words do not fit whole at 0.85 of the small size, or the
  chart has less than six small lines, the screen is one column: the tiles as rows of the kit's
  `ItemList` above the ways, the totals as its footer note.
- **Differences from interface 6** (on purpose): in one column the chart is left out (the kit's list
  has no custom rows; interface 6 kept the chart as a row there, with hardly any room at that size);
  the sync row is not shown until its screen is on interface 7; no pixel goldens yet (a kernel-backed harness is needed,
  see slice 2); the focus is always drawn (interface 6 drew it on button devices only).
- The menu entries and the icon of the action wait for the host (`tests/declarations.rs` pins the list
  of declarations that are refused).
- Not yet: sync.

## Ported: the calendar (slice 4)

- `src/native_calendar.rs`: `stats.calendar` (token 23), reached from the overview's calendar row. The
  contents are `calendar_model.rs`'s (month, shading, chosen day, books); this lays out and draws:
  the month with arrows, weekday names, the days shaded by minutes (and a bar, an hour at most),
  today framed, the chosen day boxed, and below it its date, time, pages and up to three books.
  Taps choose a day or turn the month; left/right/previous/next move a day, up/down a week, the page
  keys and swipes a month, as in interface 6. Choosing reads the day's pages from the log again
  (fallible, the chosen day is kept where that fails).
- The first day of the week is `core.week-start` (`core.week.sunday`/`monday`), else the language's
  `core.week-start-day`.
- **Differences from interface 6** (on purpose): no "close" layout for the largest text on a small
  screen (the days are as large as the room holds, the books of the chosen day fewer, down to its
  date and time); the month's name is one line (smaller, then shortened), never two; the streak note
  is drawn at the right end of the header where there is room (the kit's header has no note);
  the focus ring of the chosen day is a drawn frame, not a semantic focus; no pixel goldens (see
  slice 2); days are UTC days until draft 25.

## Ported: the widgets, first part (*Read today*, *Reading per day*, the daily goal) and the ring

- `src/native_widgets.rs`: the widgets on the `core.widget` stage and the draw callback, as the widget
  pack does it (`plugins/widgets`): the host makes an instance (`event.widget_made`) for a screen,
  asks for its height (`WidgetCall::Measure`), keeps its place and asks for its targets, and calls
  the plugin to draw it (`DrawPurpose::Widget`, `frame.id` the instance). An instance is only the widget's id
  with its placement; what it shows is read from the statistics' files when it is measured or drawn, and
  nothing of a callback is kept. A tap anywhere on a widget opens the overview (as the action
  `stats.open` does, on the book being read). The widgets are `native_widget_today.rs` and
  `native_widget_week.rs`, laid out into the kit's `Paint` operations (fallible, no `Canvas`).
- The minute tick (`TICK_TOKEN`, subscribed in `events`) compares a stamp of the day and the sizes of the
  log and the books for every instance and tells the maker with `command.widget_changed` only when it
  changed (the same rule interface 6 had with its `Stamp`); one that fails is left for the next tick.
- `declarations::widgets`: the setting `stats.goal` (the 12 choices, in the group `stats.group`) and per
  widget a group, the widget (category `core.widgets.reading`, live, its group as the preview) and its
  settings from the kit's `Opt` rows. `tests/widgets_parity.rs` compares every setting of both
  widgets (id, kind, default, choices, title, help, order) and the goal with what the interface-6
  plugin registered; the host admits every record (`tests/declarations.rs`).
- When the goal is set for the first time, *Read today* is put on the first home page after the book being
  read (`offer_today`, once, a file `goal-widget-offered` in the data folder, as in interface 6); the
  event `setting_changed` is subscribed for it.
- `native/metadata*.json` name the service `core.registry` (the kit's widget settings ask the registry
  whether the reader chose a value for the widget).
- **Differences from interface 6, on purpose:** the days are UTC days until the system time zone comes with
  the snapshot (draft 25), as in the rest of the plugin; the shades are solid grays (the canvas has no
  dithered fill: the snapshot does not say how many grays the panel has, so a panel with few grays shows
  a gray where interface 6 dithered it); rounded ends are the kit's `Paint::Focus` rounded rectangles;
  the wrapped notes and captions are the kit's (`Wrapped`, `layout::caption`). The widgets read all the
  kept days (at most 4,096, 48 KiB) when they are measured and when they are drawn.
- **Not tested end to end:** the drawing and the height (`Cx` needs the host's fonts and snapshot; the
  scripted host has neither): the layout code is the interface-6 one over the kit and is covered by
  the pixel acceptance case below, and by the settings and the declarations. Tested here: the
  declarations and settings parity, the host's admission, that instances come and go and a widget that
  is not the plugin's is left alone, that a goal with no home screen to offer on does nothing, the
  shades and the dashed line, the goal's minutes.

- **The minute tick is allowed (Kernel's ruling, todo 255).** The plugin subscribes to `Tick` and `SettingChanged` for
  good (interface 7 has no unsubscribe). The host does not filter ticks by live widgets: at most one a minute, only
  while awake, riding the wake the shell already makes, never an extra wake. The idle tick must stay cheap: it reads
  the stamp (the day and the sizes of two files), writes nothing to storage and sends `command.widget_changed` only
  for an instance whose stamp changed.
- **`offer_today`** keeps `goal-widget-offered` only after the change of the page is staged, so a refused change is
  offered again at the next change of the goal; a marker that cannot be written costs at most a second offer, and none
  while the widget is on the page.

## Ported: *Daily goal* (the ring)

- `src/native_widget_ring.rs`, token 42 (`stats.widget.goal`), declared with `Opt::PartHelp` (new in the kit: a
  switch with a help text of its own, for "streak or time left"). Same layout rules as interface 6: the ring beside
  its time and note where the place is much wider than high, else the ring above the note, else the time as text; the
  minutes inside, the unit below them, round ends on the share, a gauge (270 degrees open at the bottom) or a whole
  ring, three thicknesses. Without a goal the ring is empty, the note asks for one and a tap opens
  `core.settings` on `stats.group` (a hand-over `id`), not the statistics.
- **Difference from interface 6, on purpose:** the canvas has no arc. A ring is made of short thick segments along the
  circle (at most 120 a arc, each about half a line's thickness long) with a disc at each end of the share; the sine
  is an integer approximation (within 0.003). The track is a solid gray (`shade`), as for the other widgets.
- Tested: settings parity with the interface-6 registration (`widgets_parity.rs`, now three widgets), host admission,
  the sine, the clockwise degrees, and the arc's segment count. Not pixel-tested (see above).

## Ported: *Reading streak*

- `src/native_widget_streak.rs`, token 43 (`stats.widget.streak`): the days in a row (any reading, or the daily
  goal reached by the choice `counts`), the longest streak or how to begin one, and the last 7 or 14 days as
  marks (filled, with a check if chosen; an outline for the others, today's darker; the weekdays below where their
  letters fit). Same layout rules as interface 6: marks beside the text where the place is wide and low, else
  below it, the weekdays and then the caption left out before the marks. Settings in declaration order with
  `Opt::PartHelp` for the marks' help; `widgets_parity.rs` compares all four widgets with interface 6.
- The streak and the marks are cached per widget instance (`Slot.streak`, as interface 6 kept its figures per
  widget): the day log (up to 4,096 days, 48 KiB) is read again only when the stamp (the day and the sizes of the
  log and the books), the least seconds that count or the number of days shown changed, not at each draw. The cache
  itself is a streak and at most 14 marks. The cache is not unit-tested (it needs the host's `Cx`).
- Difference on purpose: the marks are the kit's rounded rectangles (a disc is a rectangle with half its side
  as radius; an outline is a stroked one), the check two segments in the paper's colour. Not pixel-tested.
- `paperpaw_ui_kit::widget::Opt` is `#[non_exhaustive]` now (a new kind of setting is no break for outside
  matchers).

## Ported: *Reading calendar* (the widget)

- `src/native_widget_calendar.rs`, token 44 (`stats.widget.calendar`; the screen `stats.calendar` keeps its token 23):
  this month as a calendar (weekday names, the days shaded by `level`, today ringed, the numbers where they fit,
  "October 2026" or the month alone as caption) or the last 8 to 26 weeks as a heat map (a column a week, a row a
  weekday), and below a footer with the streak and the time read in the days shown ("5 days in a row · 6 h 20 min in
  October"), the whole footer, else the total, else the streak. Same layout rules as interface 6 (footer goes first,
  then the weekdays, where the days would be small). Settings parity with interface 6 in `widgets_parity.rs`.
- The figures (the shades, the total, the streak) are cached per widget instance like the streak's (`Slot.calendar`;
  key: the stamp, the goal, the style, the weeks and the first day of the week), the day log is not read at each draw.
- `draw_day`, `draw_month` and `draw_weeks` are `#[inline(never)]` so that no frame holds a whole view. Stack
  frames on the X4 are not measured (no build of the plugin files here).
- Difference on purpose: days are rounded rectangles (the kit's `Paint::Focus`; the "round" cell is a rectangle with
  half its side as radius), shades are solid grays (as in the other widgets; the "bar instead of a dark shade"
  rule for panels with up to 4 grays is kept), the dots of empty days are small discs the same way.
- No float `clamp` in the new code (`.min().max()`), as Firmware asked (it drags in panic formatting on the X4).

## Ported: the sync with KOReader's WebDAV folder, the screen and the import

- `src/native_webdav.rs` is the interface-6 `Sync` as a host-free state machine over a `Storage` (the data folder
  as the data layer names it): `start`, `downloaded`, `uploaded`, `decide` each say the next `Step` (download, upload
  on a condition, a message, ask the user), every allocation fallible. Same rules as interface 6: a download into
  `incoming.sqlite3`, the merge (`merge::merge_as`), the upload from `statistics.sqlite3` only if nobody changed the
  file (`If-Match` with the ETag, weak mark left off, or `If-None-Match: *`; again from the download on a 412, at most
  three times), what was uploaded kept per folder and user (`statistics.sqlite3.<hash>.sync`, the others forgotten),
  a file gone from a folder uploaded to is not replaced at once, a file lacking much asks (*Remove* / *Keep*) and the
  sync starts again with the answer. The messages are `Say`, made into the catalog's words in both languages
  (`tests/` in `native_webdav_tests.rs` check each key and fill-in). `Both` routes the data folder's paths and the
  card's (root 1, read only) for the import of KOReader's statistics.
- `src/native_webdav_net.rs` makes the calls: the requests of `core.net` (a download saved to a file and an upload sent
  from one, so the statistics' file is never in memory; headers `Authorization`, `User-Agent`, the conditions,
  `Content-Type`) and the password as the plugin's own secret `webdav` (labelled `stats.secret`; the secret store
  moves interface 6's `stats.webdav` to it). The password, its buffers and the `Authorization` value are wiped. The
  request ends with one `http_done` (token 3); the ETag is read with `answer` while it is delivered. A request that
  cannot be made ends the sync as an unreachable server does (status 0). A password in `webdav.ini` of earlier
  versions moves into the secrets at the start (best effort).
- `src/native_sync_screen.rs` and `src/native_sync_shell.rs`: the screen `stats.sync` (token 24) with the rows *Folder*,
  *User name*, *Password* (dots where one is kept), *Sync now* and *Take over from KOReader* (where its file is on the
  card), the message below as the list's note; the host's keyboard for the three values (the folder in URL mode, the
  password masked), the questions *Remove*/*Keep* and, in offline mode, *Go online*/*Not now* once per sync. The
  overview shows its sync row again. The settings `stats.webdav-url` and `stats.webdav-user` (user advanced, both kept
  on reset, edited on the sync page) are declared and compared with interface 6 (`tests/widgets_parity.rs`).
  Metadata names `core.net`, `core.secrets` and `uses.network`.
- Differences from interface 6, on purpose: the message below the rows is the list's note (as high as its words need),
  so the row *More about this* (a dialog for a message too long for the room) is gone; offline mode is found by the
  request ending with reason `offline`, not before it (the offer comes then, once per sync); a request that fails
  because the heap refuses ends the sync with the event's error and leaves the files as they were.
- **Fixed on the way:** `NativeStats::event` did not decode the events of screens, actions, requests and answers
  (`ScreenOpened`, `ScreenShown`, `ScreenClosed`, `ActionRun`, `HttpDone`, `Answered` were not in its `read_event!`
  list), so a real host's screen events would have been ignored; they are decoded now.
- Not tested end to end: a sync against a server (the host runs no native plugin with `core.net` here); the engine and
  the shell's pieces are tested without one, with the refusal of every allocation of the start and the authorization.

## Ported: *Books this year* (the widget)

- `src/native_widget_year.rs`, token 45 (`stats.widget.year`): the books read to the end this year towards a yearly goal
  (off, 6, 12, 24, 36 or 52) as a bar, a ring (the count inside, "of 12" below it) or a dot a book (60 at most, in as few
  rows as let them be large), the headline ("3 of 12 books", "3 / 12" or the number alone where it is too wide), how the
  year goes ("2 ahead of plan", behind, on plan, reached) and the title of the book finished last. Same layout rules as
  interface 6 (the caption goes before a note, the notes that fit as many as there is room for, the plan first, the
  ring beside its notes where the place is much wider than high). Settings in declaration order (`Opt::PartHelp` for the
  plan's help); `widgets_parity.rs` compares all five widgets with interface 6. A tap opens the books (`stats.books`;
  `Tap::Books`), not the overview.
- **`books.ini` is streamed** (`store::each_book`): `Tally` holds the count, the title of the book finished last (cut to
  80 bytes with `…`, `book_list::cut`) and, of the books without a finishing date that were opened this year, the
  `LIBRARY_LOOKUPS` (24) opened last (id, last open, path, title cut): the memory does not grow with the library (tested
  for 300 and 3,000 books). A file that cannot be read whole counts as no books, as in interface 6 (a refused allocation
  is an error).
- **The library lookup** (books without a date count in the year they were last opened if the library has them read to the end):
  through the SDK's `native::library::Library::book` (`core.library` op 3; progress 100), opened once for the
  at most 24 lookups of a refresh and released in the same callback. The metadata of both targets name the service
  `core.library` and the feature `uses.library` as *optional* (`"required": false`, so that a host without the library
  still loads the plugin): where it cannot be opened, or a book is unknown to it, that book is not counted (only the dated
  ones are). A refused allocation is the callback's error. Two 4 KiB buffers live while the lookups run.
- The figures (the year, the count, the last title, the day of the year) are cached per widget instance (`Slot.year`; key:
  the stamp of the day and the sizes of the log and of `books.ini`): `books.ini` and the library are read again only when
  that changed, not at each draw. The cache is not unit-tested (it needs the host's `Cx`). A book marked read in the
  library, with no reading recorded, changes neither file: it counts when the day or a file changes (interface 6 had the
  same stamp).
- **Differences from interface 6, on purpose:** the year and the day of the year are UTC (as everywhere in this port until
  draft 25); the height reserves the line of the last book where its part is shown, even in a year with no book finished
  (the height cannot wait for the books: it is asked without the cache; interface 6 had no such line then); ring and bar are
  the kit's rounded rectangles and the ring a run of short segments (as for *Daily goal*, which supplies `arc`, `disc` and
  `point_at`); two books finished at the same moment: the smaller id is the last (interface 6: the first of the id-sorted
  list, the same), an undated one never replaces a dated one at a tie; the list is in file order, not id order, but nothing
  depends on it.
- Tested here: the day of the year (leap years), the plan, the year of a moment (clamped), counting by year with and without
  the library's answer, the tie rules, the 24 most recently opened being asked (newest first), the title's cut, a refused
  allocation, the dots' rows, the streamed file (peak memory for 300 and 3,000 books), a file that cannot be read, that
  instances of the widget come and go with the scripted host. **Not tested:** the drawing and the height (`Cx` needs the
  host's fonts; the scripted host has none) and the lookup against a real `core.library` (no scripted library service
  answers `book` here).

## Ported: the status items "Time left in chapter" and "Time left in book"

- `src/native_left.rs`: the items `stats.status.left-chapter` and `stats.status.left-book` (declared in `ui()` as
  `StatusItemDecl`s, priority 128 as interface 6's default, token 3) and the plugin's subscription to the stage `core.status`
  (`events()`, same token). The stage answers one text per item asked for ("Chapter: 12 min", "Book: 3 h 05 min", same
  catalog keys, `stats.left.chapter`/`stats.left.book`, and the same duration text); an item that has nothing to say
  (too little read for a speed, the reader has not counted the pages) is answered with an empty text, which the reader
  hides (interface 6 left the item without a value: the same to the reader), and is then cached by the reader.
- The speed (seconds a page over the last 30 days, 10 pages at least) is worked out where interface 6 did: when a book
  is opened (not again for the same book while there is a speed) and then every 50 pages turned, every 10 while there is
  none, ahead of the recording of the same event, and kept in `NativeStats.left`; the stage only does the sum
  from the context (`pages`, `progress`, `chapters`), never reads the log.
- Differences: a context parameter that cannot be read is skipped (interface 6 had no such case); the first of a key that
  comes twice is read. `store::days` (up to 48 KiB, as the widgets read it) is read at each recount.
- Tested here: the sum of pages left to the next chapter or the end (all shapes of the context), the recount rules and the
  wrap of the page counter. **Not tested end to end:** the stage's reply (the scripted host lends an event no reply buffer,
  so a status call cannot be answered there: the same is open for Power and Bluetooth), and the host calling the stage for
  plugin items (Core todo 76).

## Not tested here (each is an ignored case in `tests/acceptance_pending.rs`)

- The pixels of every widget (a kernel-backed harness), and the end-to-end answer of the status stage.
- The fixture `stats.flow` of the interface-6 tests is gone (the app no longer hosts the plugin; `settings.flow` took its place in the flows test).

## The stage of Reading goal

- Reading goal owns the stage `reading-goal.read-today` (Kernel's ruling) and the statistics subscribe to it
  (`declarations::stages`, token 5; `native_plugin.rs` `tell_today`). Asked with nothing or today's day they
  answer the day and the seconds read on it (8 bytes, little-endian); another day is `NotFound`, a payload of
  another length `Invalid`; nothing where the date is not known. The stage `stats.read-today` of the earlier
  slices is gone (nothing asked it).

## Widgets and the settings pages

The widgets are declared without `previews` (Kernel ruling, #228, as in Beta 3): they are offered in the Add picker, and a settings page does not show a sample of the widget. A preview-only widget would add one later.
