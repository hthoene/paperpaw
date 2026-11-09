# Interface-7 port (slices 1 to 3b: the pages' model, the declarations, the native entry, the dashboard)

Version 1.0.6. Partial: the host-free model of the home screen's pages, its declarations, and (slice
3a) the native entry with the footer items' editor. The dashboard, *Add* and the drawer are still
to come; the interface-6 `Home` and everything of it stays until the host activates native plugins
(then it goes together, no shim).

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

## Slice 3a: the native entry and the footer items' editor

`src/native_plugin.rs` (`NativeHome`), `src/native_entry.rs` (`NATIVE_ENTRY`, features `linked` and
`native-entry`), `build.rs` (the declarations of `declarations.rs` built into the image: settings,
screens, navigation, events), `native/metadata*.json|bin` (services `core.registry` and `core.ui`;
regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
--bytes native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).

- The screen `status-items` (the editor of `home.footer.items`, opened by the settings editor with
  `id`): the kit's `StatusItemsView` over the items `core.registry` lists, the setting's text read
  with `settings.read`, each change `set_setting`. Any other `id` is refused (`Invalid`); the
  setting is the only one the screen is declared as the editor of.
- *All tools* opens the screen `home.drawer`; *Edit home screen* opens the slot `core.home` with the
  parameter `mode` = `edit` (a handover). As on interface 6, the dashboard turns to editing when it
  is shown; that is the dashboard's slice.
- `add-widget` and `drawer` answer `Unsupported` when opened: they are declared (the declaration
  parity test pins that) and nothing opens them while the app has the interface-6 home screen.

### Gaps (each an ignored acceptance case in `tests/acceptance_pending.rs`)

- *The grid's columns or rows changed* (the subscription to setting changes is declared and ignored):
  the pages are fitted to the new grid by the dashboard's slice. A widget's size in cells comes from
  `widget_measure` of an instance, which only a screen showing it makes (docs/abi/ui.md 5.2); the
  registry's widget rows carry no size. `native_pages::reflow` is ready for it.
- The drawer waits for the dashboard slice (it shares the tool cell) and for icons (#517).
- The editor draws and takes input through the host (H1/H4): `home-footer-items-editor`. The drag of
  a grip is a hold and a tap in the kit (`home-footer-items-editor-grip-drag`).
- `make` is refused by the host today as `Unsupported` (#944, `tests/hosted_construction.rs`).

### What legacy removal deletes

Nothing new yet: the slice adds no interface-6 code and removes none. The activation items above
still apply (the removal of `Home`, `blocks`, `pages`, `add`, `drawer`, `covers`, `recent`, their
suites and the app wiring) in the last slice.

### Worst-case memory

The editor holds at most 128 entries (the kit's `MAX_ENTRIES`, about 100 bytes each plus the rows,
under 40 KiB with the 4 KiB read buffers released before drawing); one editor is open at a time. Not
measured on the device.

## Slice 3b: the dashboard, viewing

`src/native_dashboard.rs` (the screen: reads, draws, hands inputs on through the host), over four
parts without the host:

- `native_source.rs` (`HostSource`): what the layout reads, once per layout and with fallible
  allocation: the sixteen page settings, the grid's columns and rows, the status bar, the pencil,
  dots, tool labels and *focus at once* switches, the widgets and tools (the registry's menus
  `home.actions`, `core.tools`, `core.apps` as interface 6 listed them, the settings last), the
  footer's actions. It is the `Source` and `Sizes` of `native_pages`; a refusal ends the whole read
  (nothing made up), as the notes above ask.
- `native_dash.rs`: the places of the status line, grid and footer (the same arithmetic as the
  interface-6 `frame`, with the widget gap from `core.widgets.gap`), the footer's buttons (actions
  from the right, as many as leave the dots their middle), the dots, the page that shows and how
  pages turn.
- `native_focus.rs`: the buttons' focus over the page's targets (tools, what each widget answers with
  `widget_targets`, the footer's actions): the first press only shows the focus, left/right and
  next/previous go round, up/down go to the nearest target (round where the arrows stay on the page:
  `core.arrows-stay-on-page`, read at every layout like `ui::list::stays_on_page`), confirm runs the
  target.

What it does: the page's items are fitted to the grid (`native_grid::fit`); each tool shows its name
in its cell (`home.tool-labels`), each widget is made (`widget_make` at the place `home.pages`),
placed and composed into its cell (`ui.compose`, after the tools and the footer, the focus ring
over it). *Next page* / *Previous page* turn the page (a swipe is the host's key map turning it to
those actions; at the first or last page the press is taken, nothing turns), a tap runs the tool,
the footer's action, or is forwarded to the widget (`widget_input`; a press of *Confirm* on a widget's
target is a tap in its middle). A long press starts editing (see below). A widget the host cannot compose is left out of the draw, the rest
of the page shows; widgets made when a layout fails are let go. At every show the focus is drawn
only if `home.focus-at-once` is on. A widget that is gone or not for this screen leaves no cell; an own widget (home's own,
not ported) is not drawn.

### Editing and *Add* (slice 3c)

The pencil (a long press anywhere too) starts editing: the pencil becomes *Done* (*Back* ends it
too), *Add* shows at the left and the footer's actions go. The page opened to edit (`mode` = `edit`,
from *Edit home screen*) starts in it. While editing what is tapped runs nothing. *Add* opens the
screen `home.add-widget` for the page that shows (`native_add_screen.rs`, the rules in `native_add.rs`
without the host): the kinds (the widgets' registry kind, the tools last) with how many there are,
*Add a page* (under 8 pages), *Remove this page* (more than one), *Reset landscape layout* (where
one is kept); a kind lists its things (the widgets, the tools not on the page). A widget goes into
the first free cells as a placement of its own (`id@n`: no page of either layout holds it and the
registry has no values for it), as wide as the page and as high as it measures at the grid's width
(`ui.widget_measure` on a widget made for it, `native_measure.rs`, the interface-6 arithmetic; one
row where the host cannot measure it); a tool one cell; it is added to the other layout too. Only the pages whose text changes are written. All of a choice's writes (the
settings, the placements a removed page's widgets set reset with `reset_setting`, the message) are
staged in one callback, and an error return drops them all (`screens-input.md` 2.1), so a choice is
all or nothing. No room, a landscape layout too full, and a choice whose keys and texts exceed the host's 4,096 B budget for one callback (a full home of 8 pages that shifts every page) say so ("This change is too big to save at once."), stage nothing and keep the screen. The kinds are in the kernel's order, then plugins' own, the tools last. A dashboard opened again keeps its view.

### Arranging (slice 3d)

While editing each item is one target and a tap (or *Confirm*) on it opens its options, a dialog
over the page (the kit's dialog: *Move*, *Change the size*, *Remove*, *Cancel*, the core's words).
*Move* and *Change the size* pick the item up: left/right and up/down (the page buttons too, as on
interface 6) move it a cell or make it a cell wider/narrower or taller/lower; where another item is in
the way the core's message says so; *Select* or *Back* sets it down and writes the page
(`native_pages::store`, for a screen on its side the whole derived layout, as *Add* does).
*Remove* takes it off the page and resets what its placement (`id@n`) set. A widget is kept at
least as high as `widget_measure` gives as its least height at the grid's width (measured when the
page is laid out), and one cell wide. Where the host refuses to place a moved widget again, the page
is laid out anew from the settings. A change too big for one callback says so
and leaves the old layout (see *Add*). The page is laid out anew from the settings after a write.

### Gaps (each an ignored case in `tests/acceptance_pending.rs`)

- **Named blocker `KERNEL-cell-hints` (draft 29):** a widget's least *width* and its square hint
  (`CellSpan::min_w`, `square`; interface 6 `CellSpan::keep`) have no field in `widget_measure`'s
  reply, so a widget may be made narrower than it wants or any shape when it is sized (or, when
  added, gets the grid's width): `home-edit-arrange`, `home-add`. The TODO is in `native_measure.rs`.

- Arranging while editing with a finger (drag an item, drag its corner) waits for the pointer input (H4); the options' *Settings* entry (the widget's settings, with a placement of its own) is not ported: `home-edit-arrange`.
- *Add*'s gallery (widgets drawn as they would show, with a page of them at a time), the ready-made
  layouts and the rows' icons: `home-add` (#517). The things are listed by name.
- The kind rows are named by the catalog key the registry gives; a kind without a translation shows
  the key.
- At a show the focus is drawn only if `home.focus-at-once` is on; interface 6 also asks whether the
  last input was by keys (`ui::focus_shown`), which waits for the input mode (H4): `home-dashboard`.
- The status line above the grid is made room for but not drawn (`home-status-line`).
- Icons (slice 3e): a tool shows its icon above its name, the icon in the same frame for every tool (the
  ink of a circle) so that names side by side line up, names of tools in a row keeping to the most lines
  any of them takes; a tool its plugin shows on is drawn light on a dark ground and one it shows not
  enabled in the faint ink. The footer's actions show their icons (the puzzle piece where the menu row
  names none) and the pencil its icon (*Done* stays a word). The characters come from the host
  (`icon.glyph`, slot 40, `paperpaw_sdk::native::fonts::IconGlyph`) when the page is laid out; the icon
  font is the one bound to `role:icons`. Where the host has none (no icon font bound, a name its set lacks)
  a tool shows its name alone and the pencil its word, a footer action nothing. Nothing is drawn through a
  host yet: `home-tool-icons` (H1). Differences from interface 6: the focus ring goes round the whole cell,
  not only round the icon and name's ink; a tool's label or badge as its plugin shows it now is not shown
  (only its name). Drawing uses `paperpaw_ui_kit::icons` and `Paint::Icon` (new, with `replay_icons`).
  The icons of *Add*'s rows and the drawer's tools follow with their screens.
- The widgets' updates (the minute tick, `widget_changed`) and a screen that turned are not
  subscribed to: the page is laid out when it shows and when the page turns (`home-widget-updates`).
  A widget that pages itself (a list, a calendar) is not told the page buttons.
- The built-in widgets of the home screen (status, date, clock, continue, recent) are not declared:
  `home-builtin-widgets`.
- The drawer: `home-drawer`.
- Nothing here is drawn or taken through a host yet (H1/H4): `home-dashboard`.

Worst case memory: the snapshot (16 page texts of at most 4 KiB, 256 widgets and tools of a few
dozen bytes) and one page of cells with their names; one dashboard open at a time. Not measured on
the device.
