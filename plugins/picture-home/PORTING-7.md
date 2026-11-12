# Interface-7 port (slices 1 to 6: the declarations, the decisions, the native entry, the dashboard, Add, the picture)

Version 1.0.2. Partial. The plugin has a native entry (`NATIVE_ENTRY`, features `linked` and
`native-entry`) with the picture chooser, the bar items' editor, the dashboard (on a plain
background until *Slice 6* gives it the picture) and *Add* (*Slice 5*). What is still missing for the plugin to be
native-only is listed under *Still missing for native-only*. The interface-6
`PictureHome` and everything of it stays until the host activates native plugins (then it goes
together, no shim): see *Removal step*.

## Slice 3: the native entry

`build.rs` (the declarations of `declarations.rs` built into the image: settings, screens,
navigation, events), `native/metadata*.json|bin` (services `core.files`, `core.registry`, `core.ui`, the
consent `uses.storage`; the host calls used include `icon.glyph`, `canvas.draw_image` and
`ui.compose` since #257, which leave the bytes and the needed minor as they were; regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
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
- `add` was declared and answered `Unsupported` here; it is open since slice 5. The dashboard is
  slice 4. Nothing opens the screens while the app has the interface-6 screens.
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

- `native_grid` (in `paperpaw_ui_kit::dashboard::grid`, shared with home; re-exported under this name): items (a widget or a tool) with their cells, the text they are kept as
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
- `native_focus` and `native_arrange` (in `paperpaw_ui_kit::dashboard::{focus, arrange}`, shared with home; re-exported under these names) and `native_measure.rs`: the buttons' focus over the page's
  targets, the steps of moving and sizing an item and what removing leaves, the rows of a widget.
- `native_source.rs` (`HostSource`: the pages, the grid's columns and rows, the switches, the plate,
  the bar's visibility and place, the widgets and the tools from the registry's menus
  `home.actions`, `core.tools`, `core.apps`, read once per layout with fallible allocation) and
  `native_shown.rs` (the style's metrics and the frame).

The screen, `native_dashboard.rs` (and `native_dashboard/arrange.rs`, `native_stage.rs`), opened
through `native_plugin.rs` (the token `DASHBOARD`): the page's items fitted to the grid, each tool
with its icon above its name in its cell (*Icons* below), each widget made (`widget_make` at the place `picture-home.pages`), placed
and composed into its cell (`ui.compose`) on a plate of the colour `picture-home.plate` says (white,
a gray, a colour, or none), the dots (`picture-home.dots`) and the pencil (`picture-home.pencil`;
the tick of *Done* while editing) below, the page buttons turning the page, the buttons' focus (drawn at a show
only with `picture-home.focus-at-once`), a tap running a tool or reaching a widget. A long press or
the pencil starts editing and *Back* or *Done* ends it; the screen opened with `mode` = `edit`
(*Edit home screen*) or with `id` naming a page's setting (the settings editor) starts in it, on that
page. While editing a tap on an item opens its options (*Move*, *Change the size*, *Remove*,
*Cancel*, a dialog of the kit); the buttons then move or size it, *Select* or *Back* sets it down
and writes the page (`native_pages::store`), *Remove* also resets what its placement set. A change
over the host's budget says so ("This change is too big to save at once.") and keeps the old layout.
On its side the pages are shown (placed anew) but not arranged: the pencil says to turn the screen
upright, as interface 6 did. A dashboard opened again keeps its page and its widgets.

Differences from interface 6, each a named step below: a bar that goes within a minute, a
picture tiled more than 256 times (or 32 in a row) not drawn (*Slice 6*), and the rows of *Add*
without icons.

### Icons (#257)

`native_icons.rs` (copied from `plugins/home`'s and adapted) and `native_dashboard/page.rs`: as
interface 6 drew them. A tool shows its icon above its name (the icon its menu row names, in the frame
of the circle's ink, so that icons side by side are alike; interface 6's grid of this plugin did not keep
names in a row to one height, and neither does this), light on a dark ground where its plugin shows it on
and faint where not enabled (the registry's menu flags, read with the tools: `ToolLook` in
`native_source.rs`). The footer: the pencil faint (`style.note`) on a disc of paper over the picture, the
tick of *Done* and the plus of *Add* in black while editing, and the dots on a pill of paper over the
picture (`native_dash::pill`, interface 6's sizes). The characters come from the host (`icon.glyph`) when
the page is laid out and are drawn with the font bound to `role:icons` (`Paint::Icon`,
`paint::replay_icons`); where the host has none (no icon font bound, a name its set lacks) a tool shows
its name alone and the buttons their words (*Edit*, *Done*, `+`), as before. The pill needs no icon.
Memory: a face (one character and two flags) for each tool cell and three characters for the footer,
held with the page as its names are; the tools' icon names (a few dozen bytes each) only while the page is
laid out. Nothing is drawn through a host yet (`picture-home-dashboard`, H1).
Not here: the icons of *Add*'s rows (the kinds' icons, the tools' own, *Add a page*'s plus,
*Remove this page*'s bin): `paperpaw_ui_kit::ListRenderer`, which the screen draws its rows with, has no
icon; a row renderer of the plugin's own or an icon in the kit's (a kit change) would bring them
(`picture-home-add`, `KIT-list-icons`).

### The widgets' updates (#257)

`event.widget_changed` (86) is subscribed to (`declarations::WIDGET_CHANGED`, token 15; the host sends it
only to the maker that subscribes, once per instance between callbacks). The dashboard whose page shows
that instance (`Dashboard::widget_changed`) reads what its cell answers to the buttons anew (not while
editing, where each item is one target; a refusal keeps the old targets) and, where it is on top of the
screen stack (`ui.stack`, as home asks: a redraw of a screen under another is the host's `State`), draws
the cell anew (`queue_redraw_area`, a partial refresh; the whole screen where the focus shows and the
targets moved), as interface 6 redrew only that widget's area and read its targets again. The host
composes the new content at the draw. Nothing is laid out anew (home lays its page out anew instead): an
item or an arrangement in hand while editing stays. A widget whose rows changed keeps its cell until the
page is laid out (shown again, turned), as on interface 6. An instance of a page under another screen, or
while the picture is made or the page is to be laid out anew anyway, draws nothing (it is laid out when
it shows). Tested without the host: the area drawn (`Redraw::after`) and the pages that look for no cell;
the subscription in `tests/abi7_declarations.rs`.

## Slice 5: Add

`native_add.rs` (without the host) and `native_add_screen.rs` (the screen), copied from
`plugins/home`'s `native_add*.rs` and adapted to this plugin's settings (`picture-home.page-1` to
`-4`, `picture-home.pages`): there is no layout for a screen on its side, no status line among the
widgets and no *Reset landscape layout*. The tests are `native_add/tests.rs` and
`native_add_screen/tests.rs` (sweeps that refuse each allocation included).

- The screen `add` (`picture-home.add`, token `ADD`), opened by the dashboard's *Add* button with
  `page` (the page that shows) and, for the second step, `kind`; a page that is not a number is the
  first, a key given twice differently is `Invalid`. Step one: the kinds (the registry's widgets
  by kind in the order of `categories::ALL`, plugins' own kinds after them, the tools last under
  `picture-home.add.tools`) with their counts, *Add a page* (under four pages), *Remove this page*
  (over one). Step two: the things of one kind, each with `+`. The tools are those the dashboard
  runs (`home.actions`, `core.tools`, `core.apps`) that are not on the page; a widget may be added
  again (each placement `id@n` has settings of its own: the first `n` that no page holds and that
  has no values set, asked of the registry with `ValuesQuery::Placement`). `core.space`,
  `core.fill` and `home.tools` are the grid's own and not offered.
- A choice is staged in one callback (`native_stage::apply`): the page settings that changed
  (`native_pages::store` for an item added, which also keeps what the page hides, and
  `store_all` for a page added or removed: pages move up, only changed settings are written), the
  placements a removed page's widgets set that no page left shows (`ResetTarget::Placement`), and
  the way back to the page (two `back`s from step two). Over the 4,096 B budget it says "This
  change is too big to save at once." (`picture-home.too-big`) and keeps the old layout; no room
  says `picture-home.no-room` and keeps the screen; any error drops all that was staged. A widget
  to add is measured at the grid's width (made, asked, let go) so that it gets the rows it needs.
- On a screen on its side nothing is added (`Plan::Upright`: it says to turn the screen upright,
  `picture-home.edit-upright`): the cells of the grid there are derived and are not what a page
  keeps. The dashboard does not edit on its side either, so only a direct open reaches it.
  *Add a page* and *Remove this page* work in either position.
- The dashboard's editing footer has its *Add* button again (the plus icon, the word `+` where the
  host has no icon, at the left, reached first by the buttons, before the pencil); it opens the
  screen for the page that shows, and the dashboard is laid out anew when it shows again.
- No new texts: the keys are interface 6's (`picture-home.add`, `.add.tools`, `.add-page`,
  `.remove-page`, `.no-room`, `.too-big`), present in `en.ini` and `de.ini` (a test checks).
- Memory: the entries (at most the 256 widgets and tools the snapshot holds) and the row labels;
  one screen open at a time.
- todo 323: the widget picker queries registry op 19 without bit 0 (`Registry::widgets` sends
  flags 0), so the host leaves preview widgets out; pinned by the SDK test
  `the_widget_list_never_asks_for_preview_widgets` (`crates/paperpaw-sdk/src/native/registry/tests.rs`:
  the flags sent are 0) and the host test
  `op_19_with_flags_bit_0_lists_the_widgets_that_preview_a_group`
  (`crates/paperpaw-core/src/native_host/registry_service/menus_tests.rs`: bit 0 is what lists them;
  `registry_service/widgets.rs` takes a kind's widgets only with empty `previews`). The SDK's
  `RegistryWidget` does not expose `previews`, so the plugin has no client-side filter to test;
  `native_add::tests::only_the_widgets_the_registry_lists_are_offered` pins that what the registry
  lists is what is offered, and the screen itself needs a host to run (`picture-home-add`).

## Slice 6: the picture behind the dashboard

The image facade (`CommandSender::image_prepare`, `Event::ImageReady`, `image.info`, `Canvas::draw_image`,
`docs/abi/document.md` section 5) is on integration, so the dashboard draws the picture natively.
`native_backdrop.rs` (without the host; `native_backdrop/tests.rs`, sweeps that refuse each allocation
included) decides, `native_backdrop_host.rs` reads, asks, draws and says, `native_dashboard.rs` and
`native_plugin.rs` wire it.

- Which picture: `picture-home.picture` (the name) in `picture-home.folder`, as interface 6 (the
  chosen file only: interface 6 never took "the next picture of the folder"). Read with the file's
  size and the time of its last change (`core.files` `stat` in root 1, the request's root; 0 where the
  card does not tell, as interface 6), the fit, the place, the zoom and the paleness into the recipe (`Recipe::new`, whose `key` says whether a request is
  the same as the one before). Brightness, contrast and the render are read too (the request and the
  draw carry them), so a change of them asks again. The path goes to root 1 (the library,
  `uses.storage`); one that is over 512 bytes is no picture.
- When: at every `ScreenShown` of the dashboard (the first show, and after a settings screen or *Add*
  closes over it), so a changed setting is met there. A recipe that is the same as the held, asked or
  given-up one asks nothing; another gives the held picture back (`image_release`) and asks anew.
  `image_prepare` is sent with the side the fit needs (`native_backdrop::side`: the screen's long side
  times the zoom, a half more for a picture cut to fill it, at most 1,600, half the screen for a tile,
  1,600 for *as large as it is*), one request at a time (the cookie of each request is the plugin's, so
  the answer of an old one is given back). The request is sent from an event, never from a draw.
- Drawn: first of all, before the plates and the page, and not while editing (`draw_image`, one call
  for each place). The place is `native_picture::plan` of the size the host reports in `ImageReady`:
  *Fill* and *Fit* (the picture whole with paper round it), *As large as it is* and the zoom and the
  place of the anchor are the rectangle the plan gives (the host scales the picture into it, keeping its
  shape, clipped by the screen). *Paler by* is the host's transparency (255 times the percent) over the
  paper that is painted first (`Style::paper`; also under a picture that does not cover the screen).
  *Tile* is the one prepared picture drawn again for each tile, from the tile before the first edge as
  interface 6 started it, at most 256 tiles and 32 in a row (`TILES_MAX`, `TILES_ACROSS_MAX`); nothing is
  allocated for them (`native_backdrop::Tiles`), and a draw asks only for the tiles that meet the rows it
  may change (`DrawFrame::clip_top`/`clip_bottom`: the X4's band, elsewhere the whole screen).
- The notice while it is made: the screen says *Preparing the picture* (`picture-home.preparing` and
  `.text`, centred) instead of the page and takes no input (the host's own keys apply, so *Back*
  leaves), as on interface 6; the redraw that shows it is queued with the request. The page is not laid
  out (no widget is made) until the picture lands.
- The answer (`ImageReady`, routed to the dashboard whose cookie it has): ready, the page is drawn
  anew with a full refresh (the picture's grays, as interface 6 refreshed once the picture was made); any
  other status leaves the page plain and says why with a message (`picture-home.unreadable` for an
  unsupported, damaged, too large or missing file, `picture-home.short` for a card failure or no
  memory), as before the picture was ported. A request the host refuses (too many waiting, no consent)
  leaves the page plain without a message. A request is not made again for the same recipe whatever
  became of it, until the picture or a setting changes. An answer nobody waits for gives its picture back.
- Released: when the screen closes (`ScreenClosed`), and when another picture replaces it; the host ends
  the rest with the plugin.
- The refresh after a small change: the native dashboard only asks for partial redraws, never the fast
  (black and white) one that breaks the picture's grays into dots, so interface 6's rule (fast unless
  *Every gray*) is met by never asking for fast.
- Memory: the picture is the host's (a file of the card, drawn in bands); the plugin holds the id, the
  size and the recipe (a path). A draw allocates nothing for the places (the list of at most 64 places
  before #257 is gone); the recipe's read of the file's size and time takes the service's two buffers
  (1,536 and 4,096 bytes) for that callback only, after the settings' scratch is given back.

Blocked, each named (nothing invented):

- Done on #1076's fields (the recipe of interface 6): `IMG-turn` (every request carries `turn()`: a
  phone's JPEG comes upright and the size in `ImageReady` is the size after the turn; the crop is worked
  out on that), `IMG-tone` (*Brightness* and *Contrast* as the 256-byte curve `native_picture::tone`, sent
  with the request only when either differs from the middle), `IMG-dots` (*Render* as dots: `dots()`; as
  every gray: `grays()`; automatic: the panel's own rule, which is what interface 6's shell flag decided),
  `IMG-stretch` (*Stretch to the screen* is `stretch()` into the whole screen, no longer cropped). A change
  of brightness, contrast or render asks for the picture again (they are in the recipe's key).
- `IMG-tile-many` (partial, #257): the cause of the cap is the number of `canvas.draw_image` calls, not the
  plugin's memory (the places are no list now) nor the host's 8 prepared images (one picture is drawn
  again for every tile): the host opens and checks the picture's file on the card at every call, also
  for a tile it then clips away (`native_host/image_service.rs`: `probe_picture` before the clip). So a
  draw now asks only for the tiles in its rows, which on the X4 (bands of 192 rows) is about a band's
  share of them, and the cap is 256 tiles (32 in a row: what a band meets once for each row of tiles in
  it), up from 64 asked in every band. A 64-pixel pattern (9 by 14 tiles) now tiles; a smaller one, or
  one narrower than the screen's 32nd, is not drawn. Interface 6 had no cap: it made one screen-sized
  file of the tiles once. Lifting the rest needs the host: a tiled draw (`canvas.draw_image` repeating
  the picture over the rectangle from one read of the file a band) or `image_prepare` making the
  screen-sized tiled picture (Kernel, about 6 hours with tests either way). The time of 256 tiles on the
  X4 is unmeasured (X4 measurement list).
- `IMG-replaced` (done, #257): the recipe's key carries the file's size and the time of its last change
  from `core.files` `stat` (`native_backdrop_host::file_facts`), as interface 6 read them, so a picture
  replaced by another file of the same name is asked for anew at the next show of the dashboard (where it
  is met, as a changed setting is). A file the card cannot describe keeps 0 for both, as interface 6; a
  file replaced by one of the same size within the card's step of time (two seconds on FAT) is not
  noticed, as on interface 6.
- `IMG-ink` (done): the ink of a bar without a plate (`Look::Clear`) is read from the 8 by 8 grid of
  the prepared picture (`ImageReady::luma()`): 48 points across the plate are each read from the grid cell
  they fall on (the paper, white, where the picture does not reach; tiles repeat; the lightening is
  applied) and `native_picture::ink_over` gives white ink below 128, black above. No grid (a host that
  made none, or no picture) leaves the ink black. The paper is taken as white (the plugin does not read
  the style's paper there).
- A picture over 1,600 pixels on its long side is made smaller than interface 6's 2,400 (the host's limit).
- A change of the screen's turn while the dashboard is open is met at the next show, as the page itself.

### Named steps still BLOCKED or not yet ported (ignored cases in `tests/acceptance_pending.rs`)

*Add* is no longer one of them (slice 5); its rows' icons wait for a row renderer with icons
(`picture-home-add`, `KIT-list-icons`; see *Icons*).


- Moving and sizing with a finger: ported (the drag of an item or its corner, as home's; the cells
  it makes are unit-tested in `paperpaw_ui_kit::dashboard::arrange`). Seen only on a host with a
  screen stack that delivers pointers: `picture-home-edit` (H1).
- The picture's part the host cannot do is named under *Slice 6* (`IMG-tile-many` past 256 tiles);
  the rest is ported and waits for the production loader (`picture-home-picture`). The recipe's
  turn, tone, render, stretch and the bar's ink are written against the host's fields (#1076) and
  unseen on a device (`picture-home-picture`, H1/H7).
- The bar: ported (`native_bar.rs`: the five looks, full or compact, where its plate sits;
  `paperpaw_ui_kit::status_line`: the items of the three lists with their values, laid out as interface 6,
  drawn here and by their owners; `native_dashboard/bar.rs`: drawing, a tap on an item that does
  something, and the bar that shows after a press: the first key press (not the power key) only shows
  it, and the row of hints with it through `command.show_hints`). Differences: a bar shown after a
  press goes at the first minute tick after its four seconds (`event.tick` is the only clock a screen
  has, so up to a minute, not four seconds); the row of hints shows after a press or by the kernel's
  own setting, no contract lets a screen ask for it always or never; the bar is on the dashboard only (not while the picture is made, as the page).
  The bar's drawing and taps need a host with a screen stack (`picture-home-bars`, H1).
  X4 measurement list: a shown bar holds two `SERVICE_BUFFER` buffers while the dashboard draws, and
  reads the registry's status items and their values on each draw (heap peak and time unmeasured).
- Icons: ported (#257, *Icons* above), but for *Add*'s rows.
- The widgets' updates: the minute tick (the bar, a cover being made) and `event.widget_changed` (#257,
  *The widgets' updates* above) are subscribed to. A change of settings while the dashboard is open is met
  when it shows again (after the settings screen closes) and when the page turns
  (`picture-home-dashboard`). The grid's columns or rows changed while open: the same.
- A widget's least width and square hint (`KERNEL-cell-hints`, draft 29): as home's, a widget may be
  made narrower than it wants (`native_measure.rs`).
- `H1/H4`: the dashboard, the chooser and the editor through the production loader
  (`picture-home-dashboard`, `picture-home-pick`, `picture-home-bar-items-editor`), and
  the grip's finger drag.

Worst case memory: the snapshot (4 page texts of at most 4 KiB, 256 widgets and tools of a few dozen
bytes) and one page of cells with their names; one dashboard open at a time. Not measured on the
device.

### Still missing for native-only

With *Add*, the picture, the bar, the finger's drag, the icons and the widgets' updates open, one
part of the picture stands between this plugin and the interface-6 one:

1. A picture tiled more than 256 times or 32 in a row (*Slice 6*, `IMG-tile-many`): accepted as
   named, or asked of Kernel as a tiled draw.

The icons of *Add*'s rows are a smaller step named above.

### Gate: not in `NATIVE_PLUGINS` or `complete.txt` until the activation (with the removal step)

`make` submits the `core.home` offer at the priority of a replacing plugin. The dashboard, *Add* and
the picture open (slices 4 to 6), the bar and the finger's drag too (slice 7); the picture lacks only
the part named under *Slice 6* (`IMG-tile-many` past 256 tiles). `NativePictureHome` is listed in `NATIVE_PLUGINS` and in
`tools/plugin-files7/complete.txt` with the activation (together with the removal step below), once
the picture's last part is in or accepted as named.

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

- Through the production loader (H1, H3 compose, H2/H4 input) every screen, *Add* included
  (`picture-home-add`) and the picture (`picture-home-picture`); the bars drawn through the host's
  status frame (see *Slice 4*).
- The bar's three item settings are edited on the screen `status-items` of this plugin
  (`EditorDecl::Screen`, one screen for the three, opened with the setting's id), built on
  `paperpaw_ui_kit::status_items` as home's footer items are (Kernel's ruling, todo 121). The
  declaration is compared with the interface-6 registration (only the editor's name differs); its
  callbacks wait for the native entry (H1/H4); the finger's drag of a grip is native in the kit
  (as home's, slice 4f).
  The native entry will need `uses core.registry` in its metadata: the view reads the registered
  items with the kit's `status_items::view::read_entries`.
