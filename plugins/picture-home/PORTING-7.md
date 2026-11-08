# Interface-7 port (slices 1 and 2: the declarations and the decisions)

Version 1.0.2. Partial: what the plugin declares, without the host. No native screen and no
`NATIVE_ENTRY` yet; the interface-6 `PictureHome` and everything of it stays until the host
activates native plugins (then it goes together, no shim).

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
  folder are pictures and the first 300 of them, as far as their names fit in 24 KiB (the heap
  is small; interface 6 kept 300 names of any length), sorted without regard to case
  (`pictures`; a Greek capital sigma ending a name is lowercased `σ`, not `ς` as `str::to_lowercase`
  does, so only names that differ by that sigma can sort differently), and the
  status bar: when it shows, its look and plate, its place (`Placing`, `plate_area`; 55 percent
  wide as a pill, in `u32` so that a wide screen cannot wrap) and the ink over a picture. Every
  function is compared with the interface-6 one on the same inputs (`src/native_picture/tests.rs`),
  and sweeps refuse each allocation.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- The screens (H1, H3 compose, H2/H4 input), the picture's decode and scale (image slots 36/37,
  `core.files`), and the bars drawn through the host's status frame.
- The bar's three item settings have no editor: the picker is the host's `core.status-items`
  screen, which `ExtraDecl` cannot name (the same gap as the home screen's footer items).
