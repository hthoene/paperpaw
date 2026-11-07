# Interface-7 port (slice 1: the pages' model)

Version 1.0.5. Partial: the host-free model of the home screen's pages. No native screen, no
declarations, no `NATIVE_ENTRY` yet; the interface-6 `Home` and everything of it stays until the
host activates native plugins (then it goes together, no shim).

## Ported

- `src/native_grid.rs`: the grid of cells the pages are laid out in. Items (a widget or a tool)
  with the cells they take, the text they are kept as (`parse` / `write`, the SDK's format; a test
  pins that the SDK writes the same text), and putting them in a grid: `fit`, `refit` (a
  landscape home from the portrait one), `add`. The same rules as `paperpaw_sdk::widgets::grid`,
  with fallible allocation; the size of a widget comes from the caller (`Sizes`, which the native
  screen will answer with `widget_measure`) instead of an environment. `Cells`, `Geometry` and
  `free_place` are the SDK's (they allocate nothing).
- `src/native_pages.rs`: the pages (`home.page-1` … `-8` and `.wide`): reading them, the layout a
  screen on its side derives from the upright one while it has none of its own (`pages_wide`),
  what a change writes (`store`, `store_all`, `add_to_other`, `reset_wide`) as `Write`s the
  screen turns into `set_setting` commands, keeping what a page hides (a widget not loaded, the
  status line while it shows above the grid), a space for an emptied page, and `reflow` when the
  grid's columns or rows change. What the pages read comes from a `Source` (settings, tools,
  widgets there, the status bar), which the native screen implements over `core.registry`,
  `settings.read` and the widget service.

## Tests

`native_grid` and `native_pages`: layout text, placing, fitting and refitting, adding, page
storage and the layout on its side, reflow, with allocation-refusal sweeps over every function. The
interface-6 suites in `tests/` still pin the behaviour end to end.

## Not ported yet (explicit)

Everything that draws or takes input, in later slices: the dashboard (`Dashboard`, targets, the
status line, page dots, the pencil), the built-in widgets (`blocks`: status, date, continue,
recent), *Add* (`add`), the tools drawer (`drawer`), editing by dragging (needs the H4 pointer
forwarding and H3 composition of widgets), covers and the recent books (LIB service), the
settings and the menu entries. Each will be an ignored acceptance case naming what it waits for.

## Activation items (todo 90)

- Remove the interface-6 `Home`, `blocks`, `pages`, `add`, `drawer`, `covers`, `recent` and the
  suites in `tests/` with the app wiring (`.plugin(paperpaw_plugin_home::Home)` in
  `crates/paperpaw-app` and the harnesses that use it); then no shim stays.
- Peak memory on the X4 is not measured: device gate open.
