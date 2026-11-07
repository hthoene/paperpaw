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
`height`, `draw` and `shows`; `src/native_cx.rs` is what a widget draws with in
one callback (settings of its placement, the interface font, the shared look,
the clock in the chosen zone, the catalog). Nothing of a callback is retained.

The shared widget toolkit is `paperpaw_ui_kit::widget` (settings read at draw
time with the placement id, the frame/paper/ink/corners/line/shadow/padding
look, fitted and wrapped text, stacks, packs): every widget plugin of the
interface-7 port uses it, none builds its own.

## Ported (this slice)

- Day and date (`widgets.day`): the three styles (weekday large, the day's
  number large, a calendar sheet), the shared look settings, size, date format,
  year and week number; redrawn when the day changes (`live`, the minute tick
  sends `command.widget_changed` only when what it shows changed).

## Not ported yet (explicit)

The rest of the time family (analogue, word clock, tiles, world, calendar,
year, countdown, moon), the words family (quote, note, to-do, picture,
greeting) and the books family. The interface-6 pack stays in `src/` until
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
(H1, H3) and the minute update (H1, H3). `tests/hosted_construction.rs`
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
