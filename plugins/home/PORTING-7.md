# Interface-7 port (slices 1 to 3f: the pages' model, the declarations, the native entry, the dashboard, *Add*, arranging, icons, the tools drawer)

Version 1.0.6. Partial: the host-free model of the pages, the declarations, the native entry, the
dashboard (viewing, editing, *Add*, arranging), the icons and the tools drawer are native; the
built-in widgets, the status line, the fitting to a new grid and what needs the host are not (see
*Not ported yet* and *Blockers*). The interface-6 `Home` and everything of it stays until the host
activates native plugins (then it goes together, no shim).

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

Each is an ignored case in `tests/acceptance_pending.rs`; the section *Blockers* below says for
each what it waits for.

- The built-in widgets still in `blocks.rs`: the status line, continue reading and recently read,
  with their settings and groups, and the covers and the recent books they show (the date and the
  clock are native: slice 4a).
- The status line above the grid (composed through slot 35).
- *Add*'s gallery, the ready-made layouts and the rows' icons; the options' *Settings* entry;
  the finger's drag of an item and its corner (the footer items' grips are native: slice 4f).
- The fitting of the pages to a new grid (`native_pages::reflow` is ready).
- The ready-made layouts and the rows' icons of *Add*.
- The help hints (`help.hint` declarations).

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
native in the kit since slice 4f (below).
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
- The drawer is native since slice 3f (below); nothing opens the screens while the app has the
  interface-6 home screen.

### Gaps (each an ignored acceptance case in `tests/acceptance_pending.rs`)

- *The grid's columns or rows changed*: done in slice 4d (below).
- The editor draws and takes input through the host (H1/H4): `home-footer-items-editor`. The drag of
  a grip is native in the kit (slice 4f).
- The host admits every declaration home makes and `make` runs to the end in the real registered
  scope (`tests/hosted_construction.rs`; it was refused with `DeclarationViewsUnavailable` before
  the registry views were wired).

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
only if `home.focus-at-once` is on. A widget that is gone or not for this screen leaves no cell; a widget of home's own
(date, clock, card, shelf, status line) is drawn by home itself (see *Own widgets* below).

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

- **Cell hints (draft 29), done:** a widget's declared cells (`reg_widget.cells`: size, least size,
  `square` flag) give the size it gets when added or placed (`native_source::declared`: within the
  grid, no less than its least, square where it is, its least rows from `min_h`), without being made
  and measured, as interface 6 `Geometry::size_of`. A widget that declares none (0 by 0) is measured
  as before. Resizing with the buttons keeps a widget at least as high as it can do with and one
  cell wide, as interface 6 did (it did not read the least width or the square hint there either).

- Arranging while editing with a finger (slice 4i, todo 417): a finger that comes down on an item
  takes it (the screen answers *handled*, so the host makes no gesture of that finger and sends its
  moves and its lift); one that comes down on the corner square (half a row, at least 16 px) sizes
  it. The item stays where it is and a frame shows where it would land (thick where the cells are
  free, thin where not); the frame follows the finger and the screen is drawn again only where the
  landing cells change. Lifted where it came down it is a tap: the item has the focus and its options
  open. Lifted elsewhere the item goes there if the cells are free (`native_arrange::dragged` is the
  grid's `dragged`, then a widget's declared least size and its least rows, as the buttons' sizing
  keeps them), is kept like a button step, and the page is laid out anew; where another item is in
  the way it says so and the item stays. One finger only (`contact`); a cancelled finger puts
  nothing down; a finger down again, or the screen shown again, starts afresh (no end is waited for).
  The drop is put on a copy of the items and written, so a refused write leaves what shows as it was.
  Two small strokes in the corner of each item while editing show where to size it.
  Not covered without a real host: the finger itself and the refresh cost of a redraw per landing
  change on the X4 (a partial redraw of the whole page; Beta 3 redrew the cells' area only, there is
  no area command), and the corner marks drawn on each item while editing (X4 per-sample cost list): `home-edit-arrange` (H1). The options' *Settings* entry is native (slice 4e,
  below).
- *Add*'s ready-made layouts and the rows' icons: `home-add` (#517). The things of a tool kind are
  listed by name.
- *Add*'s gallery (slice 4g, `native_gallery`, `native_add_screen/view.rs`): the widgets of a kind
  as they would show, a page of them at a time, as interface 6. Kernel's ruling (#228): live
  instances. At the first draw or input the widgets of the kind are measured (made, measured,
  dropped; `native_measure`) and laid out as large as each would be on the page (up to three rows,
  as many to a page as fit). The page with the focus is made and placed (`widget_make` +
  `widget_place`) and composed (`ui.compose`) into its cell, under its name, with a frame (strong
  where it has the focus); leaving the page or the screen lets the instances go. The keys move among
  the widgets (the arrow keys stay on the page where `core.arrows-stay-on-page` is on), the page
  keys go to the first widget of the page before or after, *Confirm* or a tap on a name or widget
  adds it, a tap in the header goes back or turns the page. Fallbacks, never a panic: a widget the
  host cannot make or compose (full, no memory, a trap, gone) (home's own widgets are drawn by
  home itself, see *Own widgets*) is a text row in its cell
  (icon of its kind, name, cells as `4×2`); where the host composes or places nothing
  (`ERR_UNSUPPORTED`: the X4 until its stack gate, ui.md 5.1/8.2) the whole gallery is the list of
  names, as before. The tests drive the page logic with a scripted stage (compose, refusals, and
  the all-`Unsupported` host). **Not covered without a real host:** the drawing and composition
  themselves (the scripted host has no screen stack or slot 35), and on the X4 the instances of
  one page (at most the cells that fit) and the measuring at open: every widget of the kind is
  made, measured and dropped in one callback, with two 4 KiB buffers for each `make`, which goes on
  the X4 measurement list (heap peak and time). The gallery's draw (header, names, frames,
  composition, the text rows, a page turn) is a case of the activation list (`home-add`, ignored
  until H1 gives the screen a stack and slot 35 is wired). One cell is silently left without its
  text row where the bookkeeping of the cells that failed cannot grow (`Gallery::compose`): the
  rest of the page still shows, nothing panics. The 2,048 B frame limit of the draw path
  (`paint_with`, `paint_rows`, `gallery_input` hold a style, a header page and a few small
  arrays) is **not measured**: home is not in `tools/plugin-files/plugins.txt` yet (no X4 file until
  the store build moves to interface 7), so no CI step scans its frames and there is no local ELF.
  It is a gate of the activation list; the stage's `Made`, `Gallery` and `Scene` are small, the
  buffers are heap (`buffer`), and the largest local arrays are 20 to 128 bytes.
- The kind rows are named by the catalog key the registry gives; a kind without a translation shows
  the key.
- At a show the focus is drawn only if `home.focus-at-once` is on **and** the last input was by buttons
  (`event.screen_shown.last_input`, draft 30: buttons 1; touch, pen and "not known" get none, as
  Kernel ruled): `focus_at_show` in `native_dashboard.rs`. Drawing it waits for H1/H4: `home-dashboard`.
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
- The drawer (slice 3f, `native_drawer.rs`): *All tools* opens `home.drawer`, a header (title, back arrow
  and page arrows on touch screens) over the tools as icons in a grid (4 columns upright, more on a
  screen on its side up to 6, cells about as high as wide, a line higher where a name takes two), each
  drawn as on the dashboard (`native_icons::draw_tool`). Buttons move the focus by one tool or a row and
  stop at the ends; the page buttons move it a screenful; the first press only shows it (the focus is
  drawn at once only where the last input was buttons, `event.screen_shown.last_input`); *Confirm* or a
  tap goes back and runs the tool (both staged together, so a refusal stages neither); a swipe down goes
  back. `Grid` (arithmetic, hits, moves) is tested without the host. Gaps: the grid is not mirrored where
  the interface runs right to left (no native home screen is yet), and the interface-6 once-only hint on
  the gestures is not carried; nothing is drawn or taken through a host yet (H1/H4): `home-drawer`.
  Worst case memory: the tools' ids (at most 256) and their wrapped names for one draw.
- Nothing here is drawn or taken through a host yet (H1/H4): `home-dashboard`.

Worst case memory: the snapshot (16 page texts of at most 4 KiB, 256 widgets and tools of a few
dozen bytes) and one page of cells with their names; one dashboard open at a time. Not measured on
the device.

## Slice 4a: the built-in widgets, the date and the clock

`declarations.rs` (`WIDGETS`, `widgets`, the batch `WIDGETS` of `build.rs`): the widgets `home.date`
and `home.clock` (category *Time*, read each minute), each with a group of settings named as it and
titled `home.widget.<name>`, and the settings of interface 6 in its order: the shared look (alignment
with its default, frame, padding, paper, ink, corners, frame line, shadow, from the kit's `Opt`),
the size (the clock's offers *as large as fits*, the date's does not) and the clock's *Date* switch.
`tests/widgets_declarations.rs` compares every setting (id, kind, default, choices, title, help)
and each widget's title, category and group with the interface-6 registration.

`src/native_widgets.rs` (`Widgets`, in `NativeHome`): the instances a screen made
(`event.widget_made` / `widget_dropped`; a widget that is none of ours is none of our business), the
`core.widget` stage (measure: the height; place; targets: none, a tap on these does nothing as on
interface 6), the draw of an instance (`DrawPurpose::Widget`, the kit's `Cx` and `paint::replay_with`)
and the minute tick that tells the maker (`command.widget_changed`) what shows something new.
`src/native_date.rs` and `src/native_clock.rs` are the two widgets, over the kit's look, text and
layout (the same arithmetic as interface 6 `blocks.rs`: the date as large as set, smaller down to the
body's size, then in short words, then in two lines; the clock's time as large as set or fits, what
follows the digits ("PM") beside them, the date below it in proportion).

Differences from interface 6 (each deliberate):

- The digits are measured and drawn at any size by the host's font; interface 6 asked a font pack
  for its large sizes of the digits (`FontHandle::display`). Where the host has no such size it
  chooses; nothing in the plugin depends on it.
- A clock shown in a language with other digits splits the time from "PM" by any numeric character,
  not only ASCII digits (interface 6 drew the whole time small there).
- The clock's height counts the frame's padding once (`Shape::frame_height` includes it); interface
  6 added it twice. Only where a padding is set.
- The accessibility description of the time and the date (`Canvas::describe`) has no native call
  yet.
- What the clock shows is tracked by the minute, not by its text: a change of the 12/24-hour setting
  is seen by the screen when it shows again.

### Tick limits

- One `widget_changed` is staged per changed instance, and a callback holds at most 8
  commands. With more than 8 date or clock instances the rest are left for the next
  minute (a failed command is retried then).
- The stack and heap use of measure and draw on X4 is unmeasured: it stays on the
  activation gate list. The accessibility `describe` call has no SDK wrapper yet
  (Kernel SDK list).

### Gaps

- The status line is still an interface-6 widget (`blocks.rs`); the next slice.
- Drawing and sizing go through the host (H1/H3 composition, #827): the cases `home-builtin-widgets`
  and `home-dashboard` stay ignored. The scripted host has no fonts, so the drawing is covered by the
  pure parts (`split_time`, the shares) and the declaration tests; the widgets' draw is not run
  without a host.
- Worst-case memory: the widgets keep one small record per instance (id, token, last shown); the
  text of a draw is a few short strings and at most a dozen paint operations; frames stay far under
  2,048 B. Not measured on the device.

## Slice 4b: the continue card and the recent shelf

`declarations.rs`: the widgets `home.continue` and `home.recent` (category *Books*, not read each
minute), each with a group of settings named as it, and the settings of interface 6 in its order
(the shared look with the card's frame *line* and left alignment, its *Cover* and *Progress*
switches; the shelf's *Heading* and *Titles* switches and the number of books, 1 to 8, 6). `Def`
gained the frame a widget has at first and whether it is live. `tests/widgets_declarations.rs`
compares all four widgets with the interface-6 registration.

`src/native_recent.rs`: the books opened last, one `core.library` query sorted *recently opened*
(one page, at most 10 rows, the list ends at the first book never opened), a book's cover as an
image id (`library.cover`, side 0: the list's size, kept in the host's cover cache), the fitting of
a picture in its place and the signature of what the books show. `src/native_card.rs` and
`src/native_shelf.rs` are the two widgets, with the arithmetic of `blocks.rs`: the card's cover on
the left where the text keeps room, the title as large as fits in two lines, then the progress with
its bar and the caption where there is room; the shelf's covers 3:4 across the screen (at most one
per 18 mm), the heading above and the titles below, a frame with the title for a book without a
cover. A tap on the card or on a book of the shelf opens it in the reader bound to `core.reader`
(a hand-over with the book's path); the card with no book opens the library (`core.library`).
Targets are owner-defined ids: the card's is 1, a shelf book's is its place from 1.

Differences from interface 6 (each deliberate):

- The cover is the host's (`library.cover`, drawn by `canvas.draw_image`), not a thumbnail kept
  beside the book (`home-card`/`home-shelf` variants): the host's cover cache replaces it. A cover
  the host is still making (`Busy`) is drawn as a title only and asked for again at the next minute
  (the widget tells its maker it changed); an image id the host ended is asked for again on the next
  draw. The plugin may hold 8 image ids: a card and a shelf of six use seven, so a second shelf
  on the screen may show titles alone.
- A book whose file is gone is left out, as in interface 6 (which filtered once when it built the
  shelf): the instance asks the library about each book it shows (`library.book`, op 3,
  `Book::missing()`, draft 34; the host looks at the card for each) once per catalog generation
  and keeps the answer (key and gone), so a redraw or a tap does not ask again. It asks anew when
  the generation changes or when the tick finds that what the widget shows changed. A file that
  goes with the list unchanged is noticed then, not at the next draw. The tick itself asks only the
  catalog's list (one query per instance). At most ten rows are read, and only until the books
  shown are found. The buffers (1 KiB and 4 KiB more per draw read; the tick's read has none, so a
  read peaks at about 13 KiB) are on the device measurement list below.
- Each tick asks the library once per card or shelf instance, and each draw takes two 4 KiB buffers
  for the query, two more for the book calls, and one 1 KiB and one 4 KiB for each cover: both are measured on the device before
  activation (they are on the activation gate list).
- The titles are the library's row titles (cut by the host at 128 bytes), not the file's.
- The book count of the shelf is limited by one library page (at most 9 books after the current
  one; the setting's largest is 8).
- The accessibility description of the card's and the shelf's books (`Canvas::describe`) has no
  native call yet.
- The order of the host's *recently opened* sort (the one opened last first) is the library's
  contract; it is not exercised here without a host that has a catalog (the scripted host has none).

### Gaps

- The status line is slice 4c (below).

## Slice 4c: the status line

Base: this slice merges #827 (`ui.compose`, `b5f82e6e7`) and #860 (`core.widget` ops,
`478a782bc`), both approved and in train 7, onto #864. It cannot merge before them.

`declarations.rs`: the widget `home.status` (category *Device*, live: redrawn each minute) with
its group of settings as interface 6 had them (the three lists *left*, *centre* and *right*,
each edited on home's items screen, the chosen whole *line*, and the frame's look), and the status
place `status-line` that names the three lists. `tests/widgets_declarations.rs` compares the settings,
the id, the title and the category with the interface-6 registration; `tests/scripted_widgets.rs`
has a status instance.

`src/native_status_layout.rs` is the pure arithmetic of `blocks.rs`: the three parts (left, centre
and right) and the dropping of the items of the lowest priority (of equal ones the centre's, then the left's,
then the right's) until the line fits, with 0.4 of a text size between items; a text that is
still too wide is shortened to its room.
It has unit tests of its own (no host).

`src/native_status.rs` is the widget. It reads the registry's status declarations once
(`registry.status`, scope *items*: the item's owner, priority, least width, tap action) for the
items the lists name, asks the host for their values (`ui.status_values`, eight items per
call, two buffers), draws the texts itself and composes the values the owners draw
(`ui.compose` with the item's handle, inside the same draw callback; a refusal leaves the place
empty). A whole line chosen by the user (`ui.status_line`, op 15) is composed over the full
area; one that is gone, or is the caller's own, draws nothing. A tap on an item with an action of
its owner's runs that action (the target is the item's place from 1 in reading order); the tick
reads the values and tells its maker when the line changed.

Differences from interface 6 (each deliberate):

- The step "drop the battery percentage first" is gone: an owner draws its own value (a symbol
  and its text) and says its least width; the layout drops by priority and width only.
- The picker screen of the setting `home.status.line` is not ported: the setting is a text that
  holds an id; it is not chosen from a list. (The three lists use home's items screen.)
- Items another plugin owns and says it draws itself (`is_own`) are left to it and not shown;
  hidden private values (`is_hidden`) are left out.
- The status bar above the grid (a line the dashboard shows by itself) waits until the dashboard
  composes widgets; this slice is the widget.

Allocation refusals: where a list cannot grow (the placed items, the tappable ones) the item
is left out silently, as a line too narrow would leave it out; the draw does not fail for it.

The compose family (`ui.status_values`, `ui.status_line`, `ui.compose`) answers `Unsupported` on
the X4 until its stack gate passes: the line is then drawn empty (frame only), not as a failed
draw. **Activation gate:** the stack gate for those calls on the X4.

Touch only (Kernel ruling): interface 6 made the line's targets touch-only. A `ui_target` has no
focus meaning (ui.md §5.2): moving between targets with the buttons is the showing screen's
business, and home is that screen. The status line's targets therefore stay out of home's button
focus order and only taps (`widget_input` kinds 0 and 1) are forwarded to them; no flag is needed.
Home's dashboard does not compose widgets yet, so this binds the slice that does.

On the device (gates, not measured): a draw peaks at about 17 KiB (the two 4 KiB value buffers,
the 4 KiB input, the registry read and the paint), and the tick repeats the read (registry and
values) per status instance each minute; both are on the X4 measurement list. The line reads at
most 64 registry rows and asks for at most 16 items; a tap and the target list lay the line out
again (two 4 KiB buffers more). A refused allocation elsewhere fails the draw, not the screen.

## Slice 4d: the pages fitted to a new grid (reflow)

`src/native_reflow.rs` handles `event.setting_changed` (64, subscribed since slice 3) for the
grid's two settings (`home.columns`, `home.rows`; `native_source::is_grid_setting`): it reads the
device (`Shown::current`), fits each layout to its own grid with `native_pages::refit` (the upright
pages to the upright grid, the pages of a screen on its side to the wide grid, whichever way the
screen is now, as interface 6 did: what has no room goes to the next page, a page is added where
there is none, the tools and spaces stay, nothing is lost) and stages the writes with the Add
screen's `apply` (only the settings whose text changes; over the budget of a callback's commands
nothing is staged, the old layout stays and the reader is told `home.grid.too-big`). Then a
dashboard that is on top is drawn anew. A refusal while staging (allocation or the host's) fails the
callback on purpose, which drops everything staged: no page is written half (an item twice or
none), and the dashboard shows the old layout until the next minute's tick draws it anew.

Tests: `native_pages` (the writes per layout, none where it fits, refusal sweep over `refit`).
There is no scripted-host test: the scripted host has no settings store to change and read back
(the same limit as the other settings-driven cases). **On the device (gate, not measured):** the
read of the device (`Shown::current`) plus two layouts' pages in one callback.
## Slice 4e: the options' *Settings* entry

A widget that has settings of its own (a settings group with its id that holds settings:
`registry.settings` with the widget's id as the scope) offers *Settings* between *Change the size*
and *Remove* (`native_arrange::choices(true)`, the core's `core.grid.settings`); tools and widgets
without settings keep the four options. *Settings* gives the widget a placement of its own where it
has none (`id@n`: the first number no page holds and no values are kept for, `add::placement`, also
used by *Add*), keeps it on the page (the page is written, the old layout stays if the change is over
a callback's budget) and opens the settings slot (`core.settings`) with the hand-over parameter
`id` = the placement, as interface 6 did (`slots::SETTINGS` with `params::ID`). Whether a plugin's
settings plugin shows a placement's page is the settings plugin's (`model::unplaced`).

Tests: the options' words with and without *Settings* (`native_arrange`). The registry reads and the
opening are host calls: the scripted host has no screen stack to open a slot on and no settings
rows in a dashboard, so the whole path is an acceptance case on a real host (`home-edit-arrange`,
H1/H4). **On the device (gate):** one registry read (4 KiB) when the options open, and the same
plus a values read and the handover (3 x 4 KiB) when *Settings* is chosen.

## Slice 4f: the grip drag of the footer items' editor

`paperpaw_ui_kit` (shared by home, picture-home and the reader's status editors):
`ItemList::set_grips` gives the first rows a grip (three bars at the row's end, the start in a
right-to-left language; the focus fill runs under it) and `set_drag` frames the row taken along and
draws a bar where it would land; `grip_at` and `drop_row` read the last frame. `StatusItemsView`
takes `Input::Pointer`: a contact that comes **down on the grip of a shown item** (another is shown to
move it past) is taken (`Handled::Yes`, so the host sends it its moves and its end and makes no
gesture of it); a move to another row redraws the bar, a move within the row draws nothing
(`Step::Taken`); lifting puts the item at the row under the finger (above the shown ones the first,
below them the last) and the setting is written; a cancel puts nothing. The same finger down again over
a lost end starts afresh; another finger meanwhile is not taken; a finger takes over from the buttons.
The hold-and-tap and the buttons work as before. Tests: `status_items/view/tests.rs` (take, move,
drop in either direction and past the ends, one shown item has no grip, other contacts, cancel, buttons
and finger, refusal sweep).

Not in this slice: the **dashboard's** finger drag of an item and of its corner (the cells' drag with
a shadow and the resize grip): it takes its own slice (cell hit testing, a drop check). Buttons and
the options' dialog move and resize items as before. **On the device (gate):** the redraw per move
sample is a partial redraw of the list; the cost per sample is measured with the e-ink driver.

## Blockers (checked 2026-11-09 against integration `2990429e3`, train 6 and the approved PRs of train 7)

Legend: (a) solved here, (b) solved by an approved or open PR, (c) open: the missing piece.

| Case | Blocker | State |
| --- | --- | --- |
| all cases | H1 native screens and commands | (b) navigation commands on the real stack #820 and `screen_policy` #829 (approved, train 7). (c) code activation (todo 90, `shell_commands` wiring) has no PR |
| dashboard, drawer, editors | H4 input | (b) key layer #805, escape #815, `help.hint`/`input.spec` admission #818. (c) gestures and pointer input to native screens (H4a) has no PR |
| edit | H2 dialogs, hand-over store | (c) H2a (the stack opens the request's dialog instance) has no PR |
| widgets in the screen, status line, reflow, gallery | H3 composition (slot 35) | (b) #827 (draft, needs Senior; the X4 refuses until its stack gate, ui.md 8.2) |
| dashboard slice (`HOME-dashboard-slice`) | the dashboard measures widgets | (a) done in slices 3b–3d; the stale blocker is gone |
| cell hints (`KERNEL-cell-hints`) | a widget's declared cells | (a) done (#824) |
| subscriptions (`HOME-subscriptions-slice`) | tick and `widget_changed` | (a) the minute tick is subscribed to (`TICK`) and redraws a dashboard that is on top. (c) the host does not deliver `event.widget_changed` (86) yet (core has only its codec; `command.widget_changed` is unsupported too) |
| built-in widgets (`WIDGETS-declarations`) | status line, continue, recent are still `blocks.rs` | (a) date and clock native (slice 4a); the continue card and the recent shelf native (slice 4b: `core.library`, `library.cover`, `canvas.draw_image`). (c) the status line (composition #827 is wired by H3a) |
| status line (`HOME-status-line-slice`) | the line above the grid | (c) plugin work on top of #827 (the line is composed like a status place) |
| reflow (`HOME-reflow-slice`) | the setting-change handler | (a) done (slice 4d: `native_reflow`; event 64 admitted by #763) |
| help hints | `help.hint` declarations (`home.tips.*`) | (b) admitted by #818 (draft): home declares them after it lands |
| gallery (`HOME-add-gallery`) | H3 composition | (a) done (slice 4g); drawn on a real host only |
| ready-made layouts, rows' icons (`HOME-add-layouts`) | | (c) plugin work; #517 (icon admission) and #624 (glyph call) are merged, so the `517` blocker is gone |
| options' *Settings* (`HOME-item-settings`) | | (a) done (slice 4e: `native_item_settings`) |
| grip drag (`KIT-grip-drag`) | the kit's view takes a hold and a tap | (a) done (slice 4f: pointer samples in the kit's items view); the host already forwards raw pointers to the screen on top (Input Engineer, todo 379), a status 1 on the down captures the contact |
| hosted construction | #944 | (a) the host admits all declarations and `make` constructs in the real registered scope (`tests/hosted_construction.rs`, hosted harness #788) |

## Own widgets (slice 4h, todo 362)

The host never calls a plugin back from that plugin's own draw (`ui.compose` of a widget the caller
owns is `ERR_BUSY`, `docs/abi/ui.md` 5.1), and answers op 4 for a caller-owned widget with
`ui_instance{instance: 0, flags: own}` and makes no instance. Home therefore treats its own widgets
as `native_made::Made::Mine`: for an id of its own (`home.date`, `home.clock`, `home.continue`,
`home.recent`, `home.status`, placed or not) it makes **no** host call; the screen measures it
(`Mine::measure`), tells it where it is (`Mine::place`), reads its targets, draws it into its own frame
(`Mine::draw`: the same drawing as a widget draw, for the screen's frame) and forwards taps to it
(`Mine::tap`: the card and the shelf open the book, the status line runs an item's action). Another
plugin's widget is `Made::Host`, an instance the host measures, places, composes and forwards to. The
dashboard and the *Add* gallery use the same `Made`; the interface-6 behaviour (the date, clock,
card, shelf and status line visible on the home screen and in the gallery) is back.

- **Contract mismatch found (Kernel told):** Core answers an own widget with instance 0; the SDK's
  `Ui::widget_make` rejects instance 0 (`Instance` is non-zero) as `Invalid`. Home does not meet it
  (it never calls op 4 for its own ids) but another plugin that owns widgets would.
- **Behaviour (differs from a widget made for another plugin's screen):** a cover still being made
  is drawn at the **next minute tick** (the dashboard redraws on it); an own widget has no instance
  to say `command.widget_changed` for, so the "waiting" flag of such a cover is not read.
- A widget that fails to draw is left out and the rest of the page shows, except for `NoMemory`,
  which fails the screen's draw (`compose_each`), so a cell is never left blank for want of memory.
- **X4 list:** the heavy draws now run inside the dashboard's own draw callback: the status line
  (about 17 KiB) and the recent shelf (about 13 KiB), plus the dashboard's cells and paint lists, are
  one combined peak that is to be measured on the device, not the sum of two callbacks.
- Not covered without a real host: the drawing of own widgets on the dashboard and in the gallery
  (the scripted host has no screen stack); the status line widget composes its values inside the
  screen's draw as it does inside a widget's draw.
