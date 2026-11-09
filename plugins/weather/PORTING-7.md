# Weather on interface 7

Status: **complete.** The plugin is native only; no interface-6 code is left. Version 1.0.7 (above
its Beta 3 version 1.0.5: no further bump).

## Where things are

- `declarations.rs` (built into the image by `build.rs`): the settings group (order 88), the place
  (edited on its own screen), the units and how often as choices, the coordinates (advanced), the
  place's screen, the actions `choose-place` and `refresh`, the icons, the widgets *Weather now*
  and *Forecast* with their settings (the widget kit's options), and the events subscribed.
- `src/native_weather.rs`: what the plugin decides without a host: numbers and dates of
  Open-Meteo's answers, the places' coordinates, the addresses asked, the weather kept in its small
  file (`KEPT_CAP` 4 KiB, a larger or bad file counts as none), the conditions of the WMO codes,
  the units, when to fetch. Allocation is fallible.
- `src/native_answers.rs`: the forecast and the places a search found, read in pieces with the
  SDK's fallible JSON parser; a place's name is cut at 120 bytes, its region and country at 80.
- `src/native_fetch.rs`: the fetch over `core.net` (GET, answer read as it comes), the timer over
  `core.work` (`weather.refresh`, only while the radio is on anyway), the rules of when and whether
  to fetch (a failed fetch is not tried again for a quarter hour, a new place replaces the one under
  way, no fetch without a network where PaperPaw manages Wi-Fi).
- `src/native_place.rs`: the place screen (rows, search by name over `core.net`, the places found,
  *No place*, the offer to go online) with the keyboard's answer.
- `src/native_now.rs`, `src/native_forecast.rs`, `src/native_draw.rs`, `src/native_symbol.rs`: the
  two widgets laid out as drawing operations, the symbols and digits drawn as paint operations,
  the waiting messages.
- `src/native_plugin.rs` (`NativeWeather`), `src/native_entry.rs` (`NATIVE_ENTRY`): the glue.

## Tests

- `tests/native_fetch_flow.rs`: scripted host: a placed widget fetches and shows the answer; a
  failed fetch is not retried for a quarter hour; a new place replaces the fetch; a second ask
  waits; a unit change shows the widgets without a fetch; no answer is a failure.
- `tests/native_declarations_golden.rs` (`testdata/declarations.bin.v1`),
  `tests/native_translations.rs`, `tests/native_linked_entry.rs`.
- `src/native_golden_tests.rs` (`testdata/model.v1`): the reviewed transcript of the interface-6
  model's decisions (numbers, URLs, the kept file, every WMO code), which replaces the parity
  tests against the interface-6 code that was removed.
- Unit tests beside the widgets, the symbols and the place screen.

## Differences from interface 6, on purpose

- A refused allocation is `NoMemory`, never an abort; an answer is read in pieces and never held
  whole.
- A place's name, region and country are cut at 120 and 80 bytes.
- The weekday of a date near the ends of `i32` no longer overflows.
- The line in *About* naming Open-Meteo comes through `about.lines`, a contribution the host takes
  later.
