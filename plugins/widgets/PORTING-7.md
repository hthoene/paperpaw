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

## Not ported yet (explicit)

The words family's picture and logo widgets, and the books family except the battery (see below):
Ported from the words family so far: greeting (by time of day) and
note (tap asks for a text with `core.ui`; host acceptance pending H1/H2/H4). The interface-6 pack stays in `src/` until
every widget is ported; then the last commit deletes it, its app wiring and
its tests together (no shim).

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
- **Not ported yet:** the list's own screen (add, edit, move, remove). The heading,
  "more" and the empty list are the targets that open it; until it exists a tap on
  them does nothing (`Handled::No`). The widget screen of interface 6 stays until
  removal.
- Pure model (`native_todo_list`) is tested with the interface-6 properties (round
  trip, any bytes, huge years, version 1 file); drawing and file calls need a host
  and have no refusal sweep until a mock host exists. Ignored case
  `widgets-todo-ticks-on-tap` (H1/H3/H4).

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
