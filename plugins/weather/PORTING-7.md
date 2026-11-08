# Interface-7 port (slices 1 and 2: the decisions, the declarations and the readers)

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
- `src/native_answers.rs` (slice 2): the forecast (`Forecast`: the conditions now and up to 8 days)
  and the places a search found (`Places`: at most 10, with a name and coordinates in range), read
  with `paperpaw_sdk::json::stream::Parser`'s `try_feed`, so a refused allocation of the parser's path or
  text ends in `NoMemory` and nothing is returned in part; what the plugin keeps is allocated with
  `try_reserve`. The parser is never finished, as in interface 6: a body cut inside a number
  (`12` of `12.5`) gives no value (tested at every cut of the recorded answers). A place's name is
  cut at 120 bytes, its region and country at 80 (`PLACE_NAME_CAP`, `PLACE_PART_CAP`; interface 6
  kept them whole up to the parser's 4 KiB; a new cap, at a character's end, the place stays). The caller feeds the reply's body in pieces (`feed`, then
  `finish`); after an error the reading is over and every later `feed` says the same.
  `src/native_answers_tests.rs` compares each with `parse_forecast`/`parse_places` at piece sizes
  from 1 byte to 2 KiB, on the recorded answers and on odd ones, and sweeps every allocation.
  The weekday of a date no longer overflows near the ends of `i32` (`native::weekday` computes in 64
  bits; the interface-6 one is left as it is, it goes with the legacy code).
- `declarations.rs`: the settings group (order 88), the place (edited on its own screen), the units
  and how often as choices, the coordinates (advanced), the place's screen, the actions
  `choose-place` and `refresh`, and the icons. `tests/abi7_declarations.rs` decodes the batches
  by the host's rules and compares each with the interface-6 registration, including the screen's
  instance policy from the decoded flags.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The fetch itself (the timer, the network, the kept file, the place's change and the checks of
  when and whether to fetch: `lib.rs` `check`, `online`, `placed`).
- The place's screen (H1, H2: the search by name, the places found, the offer to go online) and
  the keyboard slot.
- The two widgets *Weather now* and *Forecast* with their settings (the widget pack's kit; H1, H3,
  H4) and the symbols' drawing (`symbol.rs`).
- The line in *About* naming Open-Meteo (`about.lines`; a contribution the host takes later).
