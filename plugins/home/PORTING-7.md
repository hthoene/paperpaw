# Interface-7 port (slices 1 and 2: the pages' model and the declarations)

Version 1.0.6. Partial: the host-free model of the home screen's pages and its declarations. No
native screen and no `NATIVE_ENTRY` yet; the interface-6 `Home` and everything of it stays until the
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

## Notes for the native screen

`Source::columns`, `has_widget` and `status_bar` (and `Sizes::widget`, `widest_px`) are infallible
in the trait: they have no way to report a host refusal. The shell must not hide one there by
answering a default (a made-up column count, "no such widget"): that would reflow or drop a page's
widgets on a mere refusal and write that back. A failed read must end the whole operation (read the
values fallibly first, then call the model), so that nothing is stored.

## Slice 2: declarations

`declarations.rs` (not yet built into an image: no `NATIVE_ENTRY`, no `build.rs`): the group
(*Home screen*, order 220), the 16 page settings (advanced, edited on the dashboard), the grid
(`columns` 2-6, `rows` 0-12), the switches (tool labels, focus at once, status bar, footer pencil
and dots), the footer items, the four screens (the fourth is the footer items' editor), the offer of `core.home`, the actions and their
icons and menu places, the swipe up that shows every tool, the pages as a place of widgets, the
`NoTouch` need of *focus at once* and the subscription to setting changes (grid reflow).
`tests/abi7_declarations.rs` compares each with the interface-6 registration.

Not yet (ignored cases in `tests/acceptance_pending.rs`): the dashboard and editing (H1, H2, H4),
the five built-in widgets with their settings and groups (`blocks.rs`) and the help hints.

The footer items' editor is the screen `status-items` of home's own (Kernel's ruling, todo 121):
`footer.items` names it with `EditorDecl::Screen("status-items")` and the settings editor opens it
with the setting's id. It is built on `paperpaw_ui_kit::status_items` (the model, and the view
with its texts), so the same screen serves picture-home and the reader. What is declared is
compared with the interface-6 registration (only the editor's name differs). Its callbacks wait for
the native entry (ignored case `home-footer-items-editor`, H1/H4); the finger's drag of a grip is
a hold and a tap in the kit (ignored case `home-footer-items-editor-grip-drag`, `KIT-grip-drag`).
The native entry will need `uses core.registry` in its metadata: the view reads the registered
items with the kit's `status_items::view::read_entries`.
