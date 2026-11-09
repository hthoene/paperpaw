# Interface-7 port (slices 1 to 3: the declarations, the decisions, the native entry)

Version 1.0.2. Partial. The plugin has a native entry (`NATIVE_ENTRY`, features `linked` and
`native-entry`) with the picture chooser and the bar items' editor; the dashboard and *Add* are
declared and answer `Unsupported` when opened. The interface-6 `PictureHome` and everything of it
stays until the host activates native plugins (then it goes together, no shim): see *Removal step*.

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
- `dashboard` and `add` answer `Unsupported` when opened: they are declared (the declaration
  parity test pins that) and nothing opens them while the app has the interface-6 screens.
- `core.ui` opens in the first callback that needs it, not in `make` (construction has no frame).
  `tests/hosted_construction.rs`: `make` runs to its end in the kernel's registered scope; the host
  then refuses the commit (`DeclarationViewsUnavailable`, #944), pinned by the second test; the
  positive case is ignored.
- Memory: the chooser holds at most 300 names (24 KiB) and the rows made of them; the listing's two
  4 KiB buffers are released with the files handle before drawing. Not measured on the device.

### Named steps still BLOCKED or not yet ported (ignored cases in `tests/acceptance_pending.rs`)

- `dashboard` (view, H1/H3/H4): the page grid of widgets and tools on the picture, the status bar,
  dots and pencil. The host contracts it needs exist (`ui.widget_make`/`widget_place`/`compose`,
  `core.registry`); the model (`native_grid`, `native_pages`, `native_dash`) is to be ported as
  `plugins/home` did in its slices 3b to 3d, over `picture-home.page-1` to `-4` and
  `picture-home.pages`. Next slice. Case `picture-home-dashboard`.
- *Add*, moving and sizing (H2/H4; the drag of an item or its corner waits for the pointer input):
  `picture-home-edit`.
- `IMG-slots-36-37`: the picture's decode and scale (image slots 36 and 37, `core.document` ops
  15 and 16) are not on integration, so the gray file `picture` cannot be made natively; what the
  screen decides from the picture is ported (`native_picture.rs`). `picture-home-picture`.
- `H1`: the bars drawn through the host's status frame and the hints row (`picture-home-bars`).
  The hints row is the kernel's: no contract lets a native screen ask for it to show (always,
  never, after a press) yet.
- `H1/H4`: the chooser and the editor through the production loader (`picture-home-pick`,
  `picture-home-bar-items-editor`), and `KIT-grip-drag` for the grip's finger drag.

### Gate: not in `NATIVE_PLUGINS` or `complete.txt` before the dashboard

`make` submits the `core.home` offer at the priority of a replacing plugin, but `dashboard` and `add` answer
`Unsupported` until the dashboard slice. Listing the plugin before then would replace the home screen with one that
cannot open. So `NativePictureHome` is listed in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt` only with
the dashboard slice (together with the removal step below).

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

- The screens (H1, H3 compose, H2/H4 input), the picture's decode and scale (image slots 36/37,
  `core.files`), and the bars drawn through the host's status frame.
- The bar's three item settings are edited on the screen `status-items` of this plugin
  (`EditorDecl::Screen`, one screen for the three, opened with the setting's id), built on
  `paperpaw_ui_kit::status_items` as home's footer items are (Kernel's ruling, todo 121). The
  declaration is compared with the interface-6 registration (only the editor's name differs); its
  callbacks wait for the native entry (H1/H4), and the finger's drag of a grip is a hold and a tap
  in the kit (`KIT-grip-drag`).
  The native entry will need `uses core.registry` in its metadata: the view reads the registered
  items with the kit's `status_items::view::read_entries`.
