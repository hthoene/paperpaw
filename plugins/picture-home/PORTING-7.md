# Interface-7 port (slices 1 to 4: the declarations, the decisions, the native entry, the dashboard)

Version 1.0.2. Partial. The plugin has a native entry (`NATIVE_ENTRY`, features `linked` and
`native-entry`) with the picture chooser, the bar items' editor and the dashboard (on a plain
background: see *Slice 4*); *Add* is declared and answers `Unsupported` when opened. The interface-6
`PictureHome` and everything of it stays until the host activates native plugins (then it goes
together, no shim): see *Removal step*.

## Slice 3: the native entry

`build.rs` (the declarations of `declarations.rs` built into the image: settings, screens,
navigation), `native/metadata*.json|bin` (services `core.files`, `core.registry`, `core.ui`, the
consent `uses.storage`; regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`, likewise
`metadata.host.*`), `src/native_entry.rs`, `src/native_plugin.rs` (`NativePictureHome`),
`tools/plugin-files7` (the bin `picture-home`).

- The screen `pick` (the picture chooser, `native_pick.rs` for the rows and what a choice sets,
  the same rules as the interface-6 `pick.rs`): the folder of `picture-home.folder` listed through
  `core.files` (`Root::Files`, page by page into `native_picture::Pictures`, which keeps the
  alphabetically first 300 names that fit 24 KiB whatever the card's order), *No picture* at the
  head, the chosen row marked, the focus starting on it, a choice sets `picture-home.picture` and
  closes the screen in one callback (an error drops both), an empty folder says so with its words
  (`picture-home.no-pictures`, `{folder}`). A folder that is missing, a file, or not readable by
  the plugin has no pictures, as on interface 6. The listing stops after 1,024 pages (32,768
  entries at most): interface 6 read all, which would be a long callback; a folder that big keeps
  what the first pages hold.
- The screen `status-items` (the editor of `bar.left-items`, `bar.center-items` and
  `bar.right-items`, opened by the settings editor with `id`): the kit's `StatusItemsView` over the
  items `core.registry` lists, the setting read with `settings.read`, each change `set_setting`.
  Any other `id` is refused (`Invalid`).
- `add` answers `Unsupported` when opened: it is declared (the declaration parity test pins that)
  and nothing opens it while the app has the interface-6 screens. The dashboard is slice 4.
- `core.ui` opens in the first callback that needs it, not in `make` (construction has no frame).
  `tests/hosted_construction.rs`: `make` runs to its end in the kernel's registered scope; the host
  then refuses the commit (`DeclarationViewsUnavailable`, #944), pinned by the second test; the
  positive case is ignored.
- Memory: the chooser holds at most 300 names (24 KiB) and the rows made of them; the listing's two
  4 KiB buffers are released with the files handle before drawing. Not measured on the device.

## Slice 4: the dashboard

The model, without the host (the same rules as `plugins/home`'s slices 3b to 3d, copied and adapted:
the two plugins are separate crates), with the unit tests that pin it (sweeps that refuse each
allocation, the SDK's grid text, the interface-6 settings and constants):

- `native_grid.rs`: items (a widget or a tool) with their cells, the text they are kept as
  (`parse`/`write`, the SDK's format), `fit` and `add`. Home's `refit` (a landscape home derived from
  the portrait one) is not here: this plugin has no layout of its own for a screen on its side.
- `native_pages.rs`: the four pages (`picture-home.page-1` to `-4`), `expand` (the tools for
  `home.tools`; spaces, fills and widgets not loaded left out), `placed_anew` (on its side the items
  of a page are placed anew in the order they have and nothing is kept, as interface 6 did), what a
  change writes (`store`: what a page hides is kept, an emptied page after the first keeps a space;
  `store_all`: pages move up, only changed settings are written), `holds`, and the 4,096 B budget of
  a callback's staged writes (`fits`).
- `native_dash.rs`: the places of the bar's strip (kept free, shown or not, at the top or bottom, as
  interface 6), the grid, the footer (the pencil, *Add*), the dots and page turning.
- `native_focus.rs`, `native_arrange.rs`, `native_measure.rs`: the buttons' focus over the page's
  targets, the steps of moving and sizing an item and what removing leaves, the rows of a widget.
- `native_source.rs` (`HostSource`: the pages, the grid's columns and rows, the switches, the plate,
  the bar's visibility and place, the widgets and the tools from the registry's menus
  `home.actions`, `core.tools`, `core.apps`, read once per layout with fallible allocation) and
  `native_shown.rs` (the style's metrics and the frame).

The screen, `native_dashboard.rs` (and `native_dashboard/arrange.rs`, `native_stage.rs`), opened
through `native_plugin.rs` (the token `DASHBOARD`): the page's items fitted to the grid, each tool
with its name in its cell, each widget made (`widget_make` at the place `picture-home.pages`), placed
and composed into its cell (`ui.compose`) on a plate of the colour `picture-home.plate` says (white,
a gray, a colour, or none), the dots (`picture-home.dots`) and the pencil (`picture-home.pencil`;
*Edit*, then *Done*) below, the page buttons turning the page, the buttons' focus (drawn at a show
only with `picture-home.focus-at-once`), a tap running a tool or reaching a widget. A long press or
the pencil starts editing and *Back* or *Done* ends it; the screen opened with `mode` = `edit`
(*Edit home screen*) or with `id` naming a page's setting (the settings editor) starts in it, on that
page. While editing a tap on an item opens its options (*Move*, *Change the size*, *Remove*,
*Cancel*, a dialog of the kit); the buttons then move or size it, *Select* or *Back* sets it down
and writes the page (`native_pages::store`), *Remove* also resets what its placement set. A change
over the host's budget says so ("This change is too big to save at once.") and keeps the old layout.
On its side the pages are shown (placed anew) but not arranged: the pencil says to turn the screen
upright, as interface 6 did. A dashboard opened again keeps its page and its widgets.

Differences from interface 6, each a named step below: no picture behind the page, no bar in its
strip, no hints row, *Add* left out of the editing footer, words instead of the pencil, plus and
tick icons, no drag with the finger.

### Named steps still BLOCKED or not yet ported (ignored cases in `tests/acceptance_pending.rs`)

- *Add* (the screen `add`: the tools and widgets to put on a page, *Add a page*, *Remove this page*;
  its button in the editing footer): the rules and the screen of `plugins/home`'s `native_add*.rs`
  are to be copied as this slice did for the dashboard; `native_stage.rs` (staging the writes,
  `fits`, `say`) and `native_pages::store_all` are ready for it. Next slice (`picture-home-edit`
  with the drag below). The footer has no *Add* button until then.
- Moving and sizing with a finger (H2/H4; the drag of an item or its corner waits for the pointer
  input): `picture-home-edit`.
- `IMG-slots-36-37`: the picture's decode and scale (image slots 36 and 37, `core.document` ops
  15 and 16) are not on integration, so the gray file `picture` cannot be made natively and the
  dashboard has a plain background (the plates are painted as on interface 6; the picture's
  settings are not read by the screen); what the screen decides from the picture is ported
  (`native_picture.rs`). `picture-home-picture`. When it lands: the picture drawn first (not while
  editing), the notice while it is made, the bar's ink over it, the grays/dots refresh choice.
- `H1`: the bar drawn through the host's status frame and the hints row (`picture-home-bars`). The
  bar's strip is kept free when it is not off (the page does not move when it comes) and the
  settings that place it are read (`native_dash::frame`); nothing is drawn in it, and the press that
  shows a bar that shows after a press (`takes_buttons`, `HintVisibility`) is not there. The hints
  row is the kernel's: no contract lets a native screen ask for it to show (always, never, after a
  press) yet.
- Icons: tools show their name only, the pencil, *Done* and the dots' pill no icon until icons are
  admitted (#517).
- The widgets' updates (the minute tick, `widget_changed`) and a change of settings while the
  dashboard is open are not subscribed to: the page is laid out when it shows again (after the
  settings screen closes) and when the page turns (`picture-home-dashboard`). The grid's columns or
  rows changed while open: the same.
- A widget's least width and square hint (`KERNEL-cell-hints`, draft 29): as home's, a widget may be
  made narrower than it wants (`native_measure.rs`).
- `H1/H4`: the dashboard, the chooser and the editor through the production loader
  (`picture-home-dashboard`, `picture-home-pick`, `picture-home-bar-items-editor`), and
  `KIT-grip-drag` for the grip's finger drag.

Worst case memory: the snapshot (4 page texts of at most 4 KiB, 256 widgets and tools of a few dozen
bytes) and one page of cells with their names; one dashboard open at a time. Not measured on the
device.

### Gate: not in `NATIVE_PLUGINS` or `complete.txt` before the dashboard and *Add* are both open

`make` submits the `core.home` offer at the priority of a replacing plugin. The dashboard opens (slice
4), but `add` answers `Unsupported` until its slice: a home screen whose editing cannot add anything
is not the one the reader has now. So `NativePictureHome` is listed in `NATIVE_PLUGINS` and in
`tools/plugin-files7/complete.txt` only when the dashboard and *Add* are both open (together with the
removal step below). The gate stays.

### Removal step (one later sweep)

When the host activates native plugins, delete together: `PictureHome` (`lib.rs`), `add.rs`,
`bar.rs`, `dashboard.rs`, `orient.rs`, `pages.rs`, `pick.rs`, `picture.rs`, `settings.rs` and the
interface-6 suites (`tests/home.rs`, `tests/common`), the app's wiring of `PictureHome`
(`crates/paperpaw-app`, `shells/*`), and the `paperpaw-app` dev-dependency; the plugin's
`tools/plugin-files` entry, if any. `native_picture.rs` and its tests stay.

## Ported

- `declarations.rs`: the group (order 221, `layout-grid` icon), the 28 settings (the folder by the
  folder chooser slot, the picture by its own picker, the 4 pages by the dashboard, the bar's
  alignment shown only for the compact bar, hints as an advanced setting), the three screens, the
  `core.home` offer at the priority of a replacing plugin, the pages as a place of widgets in a
  grid, the bar's place of status items, and the no-touch need of the focus setting.
  `tests/abi7_declarations.rs` compares each with the interface-6 registration.

- `src/native_picture.rs` (slice 2): what the screen decides, without the host, with fallible
  allocation and the facts given by the caller. The picture's recipe from the settings (`Asked`,
  `Recipe::new`: fit, place, zoom 100-300, paler 0-60, brightness and contrast steps, dots) and its
  note (`key`), the plan (size and offset on the screen), the crop of the decoded picture that the
  screen shows (`visible`, with `Turn` and `Crop`), the tone curve, ordered dither and paleness, a
  JPEG's size and EXIF turn read through `ReadAt` (offsets, no file name), which files of the
  folder are pictures and the alphabetically first 300 of them, as far as their names fit in 24 KiB
  (the heap is small; interface 6 kept the first 300 the card listed, of any length, then sorted
  them). The set is kept sorted while the folder is read, so what is kept does not depend on the
  order of the card (`pictures`; names compare without regard to case, a Greek capital sigma
  ending a name is lowercased `σ`, not `ς` as `str::to_lowercase` does, so only names that differ
  by that sigma can sort differently), and the
  status bar: when it shows, its look and plate, its place (`Placing`, `plate_area`; 55 percent
  wide as a pill, in `u32` so that a wide screen cannot wrap) and the ink over a picture. Every
  function is compared with the interface-6 one on the same inputs (`src/native_picture/tests.rs`),
  and sweeps refuse each allocation.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- Through the production loader (H1, H3 compose, H2/H4 input) every screen; *Add*, the picture's
  decode and scale (image slots 36/37) and the bars drawn through the host's status frame (see
  *Slice 4*).
- The bar's three item settings are edited on the screen `status-items` of this plugin
  (`EditorDecl::Screen`, one screen for the three, opened with the setting's id), built on
  `paperpaw_ui_kit::status_items` as home's footer items are (Kernel's ruling, todo 121). The
  declaration is compared with the interface-6 registration (only the editor's name differs); its
  callbacks wait for the native entry (H1/H4), and the finger's drag of a grip is a hold and a tap
  in the kit (`KIT-grip-drag`).
  The native entry will need `uses core.registry` in its metadata: the view reads the registered
  items with the kit's `status_items::view::read_entries`.
