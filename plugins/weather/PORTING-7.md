# Interface-7 port (slice 1: the decisions and the declarations)

Version 1.0.7. Partial: what the plugin decides and what it declares, without the host. No native
place screen, no widgets and no `NATIVE_ENTRY` yet; the interface-6 `Weather` and everything of it
stays until the host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_weather.rs`: the numbers and dates of Open-Meteo's answers (`tenths`, `parse_date`,
  `parse_minutes`, `days_from_civil`, `weekday`), the places' coordinates (`split_coordinates`,
  `short_coordinate`, `Place::coordinates`, `Place::whereabouts`), the addresses asked
  (`forecast_url`, `search_url`, with its own percent-encoder), the weather kept in its small
  file (`Weather::to_text`, `from_text`, `from_file`, `days_from`), the conditions and symbols of
  the WMO codes (`condition`), the units (`Temperature`, `Wind`, `degrees_text`), and when to
  fetch (`every`, `due`, `until_due`, `is_old`). Allocation is fallible and swept (text is written
  through a `fmt::Write` that reserves with `try_reserve`). `src/native_parity_tests.rs` compares
  each with the interface-6 code, including odd coordinates, kept files and every code from 0 to
  255.
- Read cap of interface 6, enforced in code: `Weather::from_file` takes the kept file up to
  `KEPT_CAP` (4 KiB); a larger or bad file counts as none and is written over, as in interface 6.
  The forecast's and the search's answers come as the HTTP reply's body (the host bounds it); they
  are read in the next slice.
- `declarations.rs`: the settings group (order 88), the place (edited on its own screen), the units
  and how often as choices, the coordinates (advanced), the place's screen, the actions
  `choose-place` and `refresh`, and the icons. `tests/abi7_declarations.rs` decodes the batches
  by the host's rules and compares each with the interface-6 registration, including the screen's
  instance policy from the decoded flags.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The answers' JSON (`parse_forecast`, `parse_places`: `paperpaw_sdk::json::stream`, to be read
  with `try_feed`/`try_finish` and the caps above in slice 2).
- The fetch itself (the timer, the network, the kept file, the place's change and the checks of
  when and whether to fetch: `lib.rs` `check`, `online`, `placed`).
- The place's screen (H1, H2: the search by name, the places found, the offer to go online) and
  the keyboard slot.
- The two widgets *Weather now* and *Forecast* with their settings (the widget pack's kit; H1, H3,
  H4) and the symbols' drawing (`symbol.rs`).
- The line in *About* naming Open-Meteo (`about.lines`; a contribution the host takes later).
