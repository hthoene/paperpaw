# Interface-7 port (in progress)

Native plugin: `src/native_plugin.rs` (`NativeWidgets`): the host makes an
instance when a screen shows a widget (`event.widget_made`), asks for its
height and where it sits through the `core.widget` stage (`WidgetCall`:
measure, place, targets), and calls the plugin to draw it (`DrawPurpose::Widget`,
`frame.id` the instance). Each widget is declared in `declarations.rs` (name,
token, category, `live`, its settings group and every setting; none at run
time) and built into the image by `build.rs`. Metadata in `native/`
(`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
--bytes native/metadata.bin --json native/metadata.inspected.json`, likewise
`metadata.host.*`). `src/native_defs.rs` maps a token to the widget's
`height`, `draw` and `shows`; `paperpaw_ui_kit::widget::Cx` is what a widget draws with in
one callback (settings of its placement, the interface font, the shared look,
the clock in the chosen zone, the catalog). Nothing of a callback is retained.

The shared widget toolkit is `paperpaw_ui_kit::widget` (settings read at draw
time with the placement id, the frame/paper/ink/corners/line/shadow/padding
look, fitted and wrapped text, stacks, packs): every widget plugin of the
interface-7 port uses it, none builds its own.

## Ported

- Day and date (`widgets.day`): the three styles (weekday large, the day's
  number large, a calendar sheet), the shared look settings, size, date format,
  year and week number; redrawn when the day changes (`live`, the minute tick
  sends `command.widget_changed` only when what it shows changed).

- The time family: analogue clock (hands as capsules, dial ticks), word clock
  (words and dot grid), tiles (digits as tiles), world clock (places, days
  ahead), calendar (month grid, week start, sheet), year progress (bar, day,
  week, days left), countdown (title and date; settable) and moon phase. A
  widget that shows what the reader chose (countdown, world) opens its settings
  on a tap (`Behaviour::settable`). `shows` of every widget takes the host and
  the widget id and returns a number that changes with what is shown (the
  day, the minute, a hash of a text), so the minute tick opens no drawing
  context and keeps no text.
- The settings parity test (`tests/settings_parity.rs`) compares every native
  widget's settings (ids, kinds, defaults, choices, titles, help keys) with the
  interface-6 registration.

- Favourites and Series (`native_favourites`, `native_series`, the shared shelf in `native_shelf`):
  the books as covers in a row or in rows (favourites), or in the series' order with their numbers
  (series), a caption just above them, those read marked, the one being read underlined or its label
  light on dark, a tap on a cover or its label opens the book (one target for each, at most 24). The
  books are the library's (`Show::Favourites`, or one query of the series ordered by the catalog's
  numbers), kept for the catalog's generation; a book whose file is gone from the card is left out.
  The host draws the covers (`core.library` op `cover`, `draw_image` after the paint is replayed:
  `paperpaw_ui_kit::covers`); a book without one, or whose cover is refused, is a frame with its
  title, and one still being made is drawn again at the next minute tick (the item keeps a `retry` flag that the
  tick turns into `widget_changed`, cleared once queued).
  Differences from interface 6, none with a visible effect on a card of up to 8 books:
  - The host keeps 8 picture ids for a plugin, so a shelf asks for the covers of its first 8 books
    only (`MOST_COVERS`); the books after them show as frames with their titles.
  - Favourites come newest *added* first (the catalog does not keep when a book was made a
    favourite); at most one library page (up to 32 books) is read, so a *count* above what a page
    holds shows what it holds.
  - Series: a book's name is matched ignoring ASCII case; the members are the library's own order.
  - The targets are the places as laid out, not the packed ones (a cover of another shape than 2:3
    is narrower than its place); a tap beside it, within the place, still opens the book.
  Host acceptance (H1/H3: widgets composed, covers drawn) is pending; no pixels were checked.
  X4 list (unmeasured): the favourites read each of up to 24 books with `library.book` when the
  catalog's generation changes (once per generation, not per draw); two Series widgets with
  different names share one cache, so each draw of one re-reads the library for its own series.
- Next up (`native_next`, token 118): the book to read next by the reader's pick (the next book of the
  series of the book being read that was not begun, the newest new book, or an unread one by chance,
  another each day), its cover (left where the place is wide, else above the text, left out where
  too small), the caption, title (as large as the size setting allows, wrapped), author and series
  where they fit (the series, then the author, then the caption go first). A tap opens the book, or the
  library where there is none. Differences: which books are new is the catalog's (`Show::New`: added
  lately and never opened), so the interface-6 files `seen.bits` and `newest.txt` are gone and the
  first run after porting finds no "newest" backlog; the chance picks among the first page of the
  unread books (at most 32) the day's lowest scores, the first whose file is on the card. The series pick
  skips a book once it is opened (the catalog's `opened()`), where Beta 3 counted a book as begun only with
  a progress above 0.
- Book progress (`native_progress`, token 119; shares `native_block` with Next up and `native_current` with
  Reading): the book being read is the one the library lists as opened last whose file is on the card
  (read once per catalog generation, up to 8 candidates). Styles: cover beside a ring, ring round a small
  cover, ring alone, bar below the title; where the place is too small for a ring the percentage stands
  alone or beside the title. A tap opens the book, or the library where none was read.
- Now reading (`native_reading` and `native_recorder`, token 120): cover, title, author, a bar and the page
  and percentage ("Page 34 of 210 · 16 %", else "34/210 · 16 %", else "34/210"), the time left at one's
  speed and the chapter, giving way in the order time left, heading, author, bar, chapter, page. The page,
  the pages, the chapter and the speed are recorded from the reading events (book opened, page turned,
  book closed, suspend, stopping; one subscription token, 2) in `reading.ini` of the plugin's data
  folder (the file of interface 6 is read as it is). Differences: the pages left in the chapter (and
  the time they take) are not shown yet, because the reading position of interface 7 carries no chapter
  end: pending on Kernel's `chapter_left` field of `ReadingView`. The setting `chapter-left` stays declared
  (it does nothing for now) and its texts stay in the catalogs, so the line comes back without a settings change; the chapter's
  title stays on one line, shortened, where interface 6 broke it in several; the title is set in the
  space the other parts leave. The plugin is woken on every page turn (it subscribes to the reading events to record
  them): put it on the X4 page-turn timing list (unmeasured). If Kernel's `chapter_left` (todo 446) misses
  1.0, drop the inert `chapter-left` setting. Host acceptance (the reading events delivered to this plugin, H1/H3) is pending.

## Not ported yet (explicit)

Nothing: with the picture widget (this slice) every widget of the families has a native port. The interface-6 pack stays in `src/`
until activation; then the last
commit deletes it, its app wiring and its tests together (no shim).
Ported from the words family so far: greeting (by time of day) and
note (tap asks for a text with `core.ui`; host acceptance pending H1/H2/H4).

## Gaps and differences

- The snapshot has no pixel format (gray levels): a card is drawn as on a
  panel with grays (the same gap as the soft focus). The legacy sparse pattern
  on black-and-white panels is not drawn.
- The system's time zone (the shell's UTC offset) is not in the snapshot yet (draft 25 adds `utc_offset_minutes`; ignored case `widgets-system-utc-offset`): only
  a chosen zone (`core.time-zone`, `core.time-zone-rule`) gives an offset; with
  none, times read as UTC. Reported to Kernel.
- Circles and masks use `fill_round`/`stroke_round` and span fills (the native
  canvas has no circle or mask primitive).
- "Chosen for this widget" (the reader's value against the one for all widgets)
  is the registry's `source` of the setting not being the default, or the
  placement setting it itself; the legacy `is_chosen` said the same.

- The widgets are declared without `previews` (Kernel ruling, #228): a widget
  that previews a settings group is never offered in the Add picker, and Beta 3
  declared none. The widget pack's settings groups are named like their widget,
  but the registry links a group to a widget only through `previews`, so a
  settings page does not list a sample of the widget. Beta 3 had no such
  sample either; a separate preview-only widget would add one later.

## Acceptance pending on the host

`tests/acceptance_pending.rs` holds refusals (never pass): day in each style
(H1, H3) and the minute update (H1, H3). The time family is covered by the
same two cases for its widgets once the host composes widgets (H1, H3);
what the toolkit lays out is unit-tested without the host. `tests/hosted_construction.rs`
constructs in the kernel's real registered scope; the host refuses that with
`Unsupported` until it admits every family (#310), so the positive case is an
ignored refusal.

## Native image and frame scan (widgets.day slice)

Built as a riscv32imc plugin (`opt-level = "s"`, fat LTO, `-C jump-tables=no`,
`default-features = false`, `native-entry`) and packed with `paperpaw-sign pack`:
format 5, interface 7.1, ESP32-C3, file 281,196 bytes (25,280 data, 253,172
code, 662 relocated words). The pack-time frame scan passes; the largest frames
are 448 bytes (`Settings::align`, `Settings::shows`, in the kit), then 352
(`local_time_in`) and 336 (SDK record decode). By code size the image is about
176 KB SDK (record encode/decode of the declarations), 42 KB this port (kit
22 KB, plugin 20 KB), 25 KB core float formatting. The SDK share is not this
port's; it is the same for every plugin that submits declarations.

With the time family (all nine time widgets, `native-entry`, no default
features): file 421,236 bytes (67,472 data, 349,212 code, 1,114 relocated
words); the pack-time frame scan passes, largest frames 560 bytes
(`paperpaw_ui_kit::style::setting_text`), then 528 (`native_tiles::draw`,
`chosen_name`), 512 (SDK `draw` entry). The code includes about 25 KB of
core float formatting (where it comes from is not traced yet).

After the review of the time family (fallible allocation everywhere, the float
formatting removed, `shows` as numbers): file 396,840 bytes (60,896 data,
331,612 code, 1,059 relocated words); no float formatting is left (12
`flt2dec` symbols before, none now). The float formatting came from
`f32::clamp` in `native_day::sheet`: its panic message formats the two bounds
with `Debug`, which links the float formatter; use `.max(min).min(max)` on
floats in plugin code (integer `clamp` has a fixed message and is fine).
With `opt-level = "z"` for `paperpaw-plugin-widgets` and `paperpaw-ui-kit`
only (not adopted): 373,116 bytes (57,584 data, 311,372 code), 23,724 bytes
smaller. Refusal sweeps (`src/refusals_tests.rs`) cover the pure widget
calls (tiles, word clock text and grid, moon rows, the kit's joins); the
calls that need a host (drawing, measuring, settings reads) have no refusal
sweep until the SDK has a mock host; their allocations are fallible by
reading and by the kit's own sweeps.

## Words family: greeting, note, quote

- Greeting draws its sun and moon as shapes (discs, rays and a bite), not with
  the icon font. Pure calls (`Part`, `short_date`, `draw_icon`, the quote's
  `parse`, `marks`, `of_the_day`) are tested directly; calls that need the host
  (settings, fonts, storage) have no refusal sweep until the SDK has a mock host.
- Text settings are read up to 4096 bytes (the editor holds 4084): a long note,
  heading, name or title is read whole.
- Note: a tap asks for the text on the keyboard (`core.ui`), the old text first
  (written in parts after the request is made); a second tap while a request is
  open does nothing, the first answer counts.
- Quote: PaperPaw's quotes come from `data/quotes.<language>.txt` in the install
  folder (else those the app carries), the reader's own from `quotes.txt` in the
  data folder; both are streamed a line at a time (a line over 4096 bytes is cut, a file of any size is read to its end). The turns are kept in
  `quote.txt` of the data folder. The UI language is the setting `core.language`
  (read when drawing, nothing cached): `de` picks the German quotes, any other
  language the English, and the quotation marks follow the tag. "Another each time it shows" counts showings in
  memory, once for all made within a minute.

## To-do list (widget)

- The widget shows the first items of `todo.txt` (data folder) with boxes; a tap
  on an item ticks it off or on (a target for each, named by what the item says
  and whether it is done, so a tap meets the item that was shown even if the list
  changed since). The file is read up to 32 KiB, written back whole (replaced
  atomically); a file that is no text or too large is left as it is and the widget
  says so. The settings (`count` is the first number setting of the pack) are
  checked by `settings_parity`.
- The list's own screen (`native_todo_screen`): *Add* first, then the items with
  *Done* in the value column of those ticked (the box and the strike-through of
  interface 6 are not drawn in the kit's list); a tap ticks an item off or on, a
  hold or the menu button offers tick, edit, move up/down and remove (host
  questions with the original words); the heading, "more" and an empty list of
  the widget open it (action `widgets.todo.open`, icon `list-check`). One
  question is open at a time. The widgets showing the list are told when it
  changes. Ignored case `widgets-todo-screen-adds-ticks-and-moves` (H1/H2/H4).
  Two differences from interface 6, both without a visible effect: the screen is
  declared `Single` (interface 6: `PER_PARAMS`; the action opens it without
  parameters, so one list is open either way), and the options of an item at the
  top or the bottom of the list leave out *up* or *down* (interface 6 offered them
  as no-ops). `tests/todo_declarations.rs` compares the screen, the action and the
  icon with interface 6's.
  If the rows cannot be rebuilt after a change (memory refused), they are dropped
  and the screen says *The list could not be shown. It is saved; open it again.*
  (`widgets.todo.not-shown`, also as the notice of a tap or the menu button) until
  it is opened again, which reads the card (where the change was saved): a tap can
  then never act on another item than the one shown.
- Metadata now lists the `storage.*` host calls the quote and the to-do list use.
- Pure model (`native_todo_list`) is tested with the interface-6 properties (round
  trip, any bytes, huge years, version 1 file); drawing and file calls need a host
  and have no refusal sweep until a mock host exists. Ignored case
  `widgets-todo-ticks-on-tap` (H1/H3/H4).


## No writable statics

A native plugin file may have no writable `.data` or `.bss` (the interface-7 file pipeline,
`tools/plugin-files7`, refuses to pack it). The quote's count of showings (`SHOWINGS` and
`COUNTED_AT`, two atomics) was the only such state; it is now `native_quote::Showings` held by
`NativeWidgets` beside the to-do `Cache`, passed to the quote's drawing and tick code
(`draw_with`, `shows_with`) as the to-do list's cache is. The rule is unchanged (a showing counts once a
minute at most, in memory only); the count now starts at zero with each plugin instance. Checked with
`tools/plugin-files7/build.sh x4 <out> --only widgets`: packs, no writable data symbols left.

## Battery (widget)

- The battery (`widgets.battery`, category device): the charge large beside a
  battery drawn to scale (lying or standing) or inside a ring, a bolt while it
  charges, shaded when it is at or below `core.battery-low`, an optional caption;
  read from the device snapshot (`battery_percent`, `is_charging`), redrawn when
  the charge or the charging changes (`live`, the minute tick). Where the device
  reports no battery the widget says so. No time left is shown (no service offers it),
  as before. The settings are the interface-6 ones (`settings_parity`).
- Shapes (`native_battery_shapes`): the battery, the ring and the bolt are unit-tested
  and have an allocation sweep (`refusals_tests`); the layout needs fonts and the host,
  so it has no sweep until a mock host exists. Ignored case
  `widgets-battery-draws-each-style` (H1/H3).
- Differences: the bolt and the rounded battery are made of rounded rectangles and span
  fills (no smooth edges); the low charge is a plain gray (110), not a dither.

## Library in numbers (widget)

- `widgets.library` (token 114, category books): the books, those finished, being read and not read
  yet as numbers in a row (as many columns as make them largest, the bar kept where it costs little,
  else each number beside its name on a line), a bar of the share read, a caption. The settings are
  the interface-6 ones (`settings_parity`). A tap runs `core.library`.
- Where the numbers come from (`native_numbers::read`): `core.library` op 1 (`books` of the catalog)
  and two queries (`Show::Finished`, `Show::Reading`) of which only `end.total` is used, three calls
  with two 4,096-byte buffers, the service released in every case (`native_books::with_library`).
  The counts are kept in the plugin (`native_numbers::Cache`) for the catalog's generation, so a redraw
  asks the library for its state only (one call). Where a query is refused the widget shows the books
  of the catalog alone (tried again at the next draw), and where the library is not there at all it
  shows nothing counted; only a refused allocation fails the draw.
  `shows` (the minute tick) reads the catalog's generation and book count only: the generation moves
  when books come or go and when one is first opened or finished (`library.md` 2), so the widget
  follows within a minute.
- **Differences from interface 6** (all without a loss for the reader): interface 6 counted the
  finished and the reading books among the hundred opened last, from the progress each held; the
  library's totals cover every book, so a library of more than a hundred opened books is counted
  whole. "Reading" is the library's (`Show::Reading`), and a book opened outside the library folder
  counts as a book only if the catalog lists it (interface 6 added it). The books being read shaded
  in the bar are a plain gray (150, or 140 on dark paper), not a dither (the canvas has none). The
  accessibility text of the numbers (`canvas.describe`) has no native operation.
- Memory: two buffers of 4,096 bytes while the three calls run and the parts of the widget (four
  short texts and the paint list), about 10 KiB; nothing is kept between callbacks. Not measured on a
  device.
- `native/metadata*.json|bin` list `core.library` (and the optional feature `uses.library`).
- The pure part (`Numbers::count`, `of`) is unit-tested; the layout and the calls need a host (no
  refusal sweep until a mock host exists; every allocation is fallible by reading). Ignored case
  `widgets-library-draws-its-numbers` (H1/H3/H4).

## Highlight (widget)

- `widgets.highlight` (token 115, category words): one of the reader's highlights, from the books read
  lately or from all, another each day or each time it shows; the text in quotes (or under a large
  opening mark), the note, the book and its author below; a tap opens the book at the highlight. The
  settings are the interface-6 ones (`settings_parity`). Where the text does not fit whole the mark goes
  first, then the note, then the book, then the text is shortened (`lay_out`, as interface 6).
- Where it comes from (`native_highlight::choose`): `core.library` only. The books: the first 12 opened
  last (`Sort::RecentlyOpened`, descending, only those opened) or the first 200 by title. The seed is the
  day (or, each time it shows, the day and the plugin's count of showings, in the plugin struct like the
  quote's), `mix` as in interface 6; it picks the book to start at, books without a highlight with a
  text are passed over, and the highlight is picked by its place among the book's highlights (counted
  first, then read by index), so one highlight is held and nothing of the others. A cut text or note is
  read whole up to 2,048 bytes through `annotation_text`.
- The choice is kept in the plugin (`Cache`) under the source, the seed and the catalog's generation, and
  read anew when the chosen book's annotation generation moved (checked with one call at each tick and
  draw). `shows` (the minute tick) hashes the chosen highlight and the parts shown.
- **Differences from interface 6**: it chooses among the first 200 books by title (interface 6 took
  every book with an annotation file, up to 200, whichever they were), and which highlight shows on a
  given day is another one than in interface 6 (the day picks among every book the scan lists, not only
  among those with notes); the tap opens the book at the highlight's start and does not mark its end
  (the reader's hand-over carries `section`, `offset` and `conversion` only); a book without a title
  shows its file name. The widget does not subscribe to the library's change event (no first-party plugin does
  yet, and a refused subscription would fail the whole pack's construction); it follows within a minute by
  the generation checks above.
- Memory: two buffers of 4,096 bytes while a scan runs, a list of up to 200 book handles (1.6 KiB), one
  highlight (text and note up to 2 KiB each) in the plugin; not measured on a device.
- The pure part (`mix`, `picks`, `cut`, `is_highlight`) is unit-tested; the scan and the layout need a
  host (no refusal sweep until a mock host exists). Ignored case
  `widgets-highlight-draws-and-opens-the-book` (H1/H3/H4).
- Where the library refuses a call, the highlight says there is none (`widgets.highlight.none`); only a
  refused allocation fails the draw. The widget's layout arithmetic saturates.
- A cached "no highlight" is looked for again a minute after the last scan (`RECHECK`, the host's
  monotonic clock), so a first highlight appears within a minute without a day or catalog change, and a
  library without any highlight costs one scan a minute (up to about 200 host calls with the source
  *all*, 12 books with *recent*), not one per draw. That worst case is an X4 timing gate (not measured).
  `shows` (the minute tick) treats a refused library call as the draw does.

## Logo (widget, token 121)

- `native_logo.rs`. The logo is a picture the host decodes and draws (`image_prepare`, `canvas.draw_image`,
  `docs/abi/document.md` 5): the one the setting `core.logo` names (JPEG, PNG or GIF on the card, library root,
  `uses.storage` optional), else PaperPaw's own (`pictures/logo.png`, 192 × 192 like the sleep screen's and square like the drawn logo was, in the plugin's install folder, root 2;
  `card-files` ships it). One request is open at a time and one picture (decoded to 320 px at most) is held,
  shared by every logo widget; the last widget going gives it back. `Logo` decides what to ask and what an
  answer means (tested without a host); `native_plugin.rs` sends the commands, reacts to `event.image_ready` and
  to `core.logo` changing (held picture released, new one asked for), asks again at the next minute after a
  refusal for memory or the card, and draws the picture over the replayed paint.
- Differences from interface 6, on purpose: PaperPaw's own logo is no longer drawn as shapes (the native canvas has
  no mask primitive) but is the installed picture (black sheet, white paw, so the widget's ink setting no longer
  colours it; a test checks that the file is square); until it is ready, or where the host cannot read it (a plugin
  built into the image has no install folder), only the name shows, if wanted. A picture set is no longer
  re-decoded into a file of the plugin's own: the host's picture cache does that. Settings, sizes, name and
  layout are the same (parity test).
- A request's cookie carries a generation, counted up when `core.logo` changes, so the answer to a request made
  before the change is not taken for the new one's. A request the host refuses outright (the card not granted
  to the plugin, `uses.storage` being optional) moves on to the next file, so PaperPaw's own logo shows then.
  The host's transient heap for drawing the picture (up to about 30 KiB) belongs on the X4 measurement list
  with home's peaks.
- Acceptance on the host (`widgets-logo-asks-for-and-draws-its-picture`) waits for H1/H3.

## Picture (widget, token 122)

- `native_picture.rs` (what to choose, what an answer means: host-free, tested) and `native_picture_host.rs` (the
  host calls). The folder (`widgets.picture.folder`, chosen on the folder chooser, `core.choose-folder`; default
  `Pictures`) is listed with `core.files` (root 1, `uses.storage` optional) a page at a time: once to count the
  pictures (JPEG, PNG or GIF, not hidden), once to read the name of the one chosen; names are never kept. Which one:
  the day's (`of_the_day`, as the quote) or the next each time it shows (`Showings`, counted in memory), moved on
  by the turns a tap makes (`picture.txt`, the file of interface 6, same format). The chosen file is asked for with
  `image_prepare` (library root, the screen's size, colour where the screen has it); the host decodes it and
  draws it in bands (`canvas.draw_image`, filled with `ImageStyle::fill` or whole), the name below as a caption
  (without ending, `_` as spaces). One picture is held for each widget placed; the one shown stays until the new is
  ready, then it is given back; the widget going gives it back too.
- Chosen again at the widget being made, at a tap, and at the minute tick when what it depends on (day, turns,
  showing, folder) changed, or after a refusal for want of memory. A folder that cannot be read or holds no picture,
  and a file the host cannot show, give the interface-6 words (`widgets.picture.none`); not asked again for the same
  day or turn.
- Differences from interface 6, on purpose: the picture is no longer decoded into a file of the plugin's own
  (`picture.pixels`, `picture.stamp`) and drawn row by row by the plugin: the host's picture cache decodes and draws
  it, so those files are not written; the folder is read through `core.files` (a page at a time) and counted, as
  before; a tap chooses the next at once. While the picture is asked for, the widget shows its frame only.
- X4: the host draws the picture in bands (transient host heap, to measure next to home's peaks); the plugin holds
  one `ImageId` and a caption per widget, and listing buffers (1.5 KiB + 4 KiB, heap) while it lists.
- A refusal means "no picture" only for `NotFound`, `Denied`, `Unsupported` and `Invalid` (a missing or forbidden folder, a
  request that cannot be made); `Full`, `Busy`, `Io`, `State`, `NoSpace` and the like (the host prepares 8 images and queues 4
  for a plugin, and a widget holds two while the next loads) are chosen again at the next minute. A tap turns the shared
  `picture.txt` and every picture widget is chosen again, as in interface 6. Every request has a cookie of its own (one
  counter), so answers of two widgets cannot be mixed. A widget whose slot cannot be made is not kept.
- With more than 8 picture widgets, one tap or tick stages more than 8 `image_prepare` commands in one callback; the host caps
  a callback at 8 (open ruling #422), so the rest are refused and chosen again at the next minute.
- X4 timing list: a folder of about 1,000 pictures is walked twice for each choice (count, then name).
- Acceptance on the host (`widgets-picture-shows-and-turns-on-tap`) waits for H1/H3/H4.
