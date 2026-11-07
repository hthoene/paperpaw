# Interface-7 port (slice 1: the menu list)

Version 1.0.6. Native plugin: `src/native_plugin.rs` (`NativeMenu`: the menu as a sheet at the
bottom of the screen), registry reads in `src/native_data.rs`, what a menu is made of without the
host in `src/native_model.rs` (rows, shown rows, focus, what choosing does, a setting's row).
`NATIVE_ENTRY` (feature `linked`), declarations in `declarations.rs` built into the image by
`build.rs`, metadata in `native/` (regenerate with `python3 tools/abi-metadata.py --schema
abi/abi.def --input native/metadata.json --bytes native/metadata.bin --json
native/metadata.inspected.json`, likewise `metadata.host.*`; services `core.registry`, `core.ui`).
Drawing uses `paperpaw-ui-kit` (new: `ItemList::draw_sheet`, `sheet::geometry`, faint rows).
The interface-6 `Menu`, `bar.rs`, `panel.rs`, `notice.rs` and the suites in `tests/` stay: the app
and other plugins' tests still link them until the host activates native plugins; then they go
together (no shim).

## Ported

- The list (`menu.list`, slot `core.menu`, declared `overlay`): the rows of `core.registry` op 12
  (the user's own definition, else the declared rows; hidden rows and unmet needs already left
  out), headings with nothing below them dropped, items named by the `hide` parameter dropped,
  descriptions under rows, a row's badge at the right, faint rows (not enabled) passed by the
  focus and not chosen by a tap, `menu.empty` for a menu with nothing in it.
- Choosing: an action closes this menu and those below it (`depth` + 1 backs), then runs; a
  submenu opens over this one (`depth` + 1, at most 8 deep); the menu button and a tap outside the
  sheet close it; the first press of a button on a touch screen only shows the focus.
- Settings in place: a switch toggles, a number changes by − and + (touch) or, with buttons, by
  confirm then left/right at once (confirm or back ends), a choice of two takes the other; longer
  choices open `core.choose-value`, a setting's own editor opens as declared, text and lists open
  their settings page (`core.settings` with `focus`). A setting shown only while another has a
  value (`SHOWN`) and a choice with fewer than two options are not listed.
- Paging: a pager bar (page of pages, arrows on touch screens) when the sheet needs more than the
  screen below its header.
- The setting `menu.style` (List or Tiles).
- The menu follows `SettingChanged`, `PluginsChanged` and `RegistryChanged`: the top one is made
  anew (aside; a refusal leaves the old rows), the covered ones when they show.

## Moved to the host

- The notice (`menu.notice`, slot `core.notice`): notices are `command.notify` drawn by the host
  (`docs/abi/ui.md`, *Messages*); the plugin has nothing to show and the screen goes with the
  interface-6 code at activation. This is an accepted change from interface 6 (Kernel): a plugin can
  no longer replace how notices look; the host shows each for `core.notice-seconds`.

## Tests

`native_model`: hidden items, depth, headings, tiles rule, the focus passing faint rows, what
choosing does, stepping numbers and their ends, units, listing rules, and allocation-refusal sweeps.
`paperpaw-ui-kit`: sheet geometry, faint rows. Declaration parity with the interface-6
registration, linked entry, construction in the kernel's registered scope (refused as
`Unsupported` until the host admits every declaration family, #944).
`tests/acceptance_pending.rs`: ignored cases that name what they wait for.

## Not ported yet (explicit; each is an ignored acceptance case)

- Tiles and the reader's bar (`mode=bar`, style *Tiles*): icons and the status font role; the
  canvas has no icon operation. Both open as the list meanwhile.
- The quick panel (`core.panel`, `menu.panel`): icons, sliders (the kit has none) and its help
  hints. Declared when it is ported; the slot is unbound in the native plugin meanwhile.
- A switch that is off: `reg_menu_row.flags` carry "checked" only, so a switch off looks like a
  plain action (shown with no value). Needs a flag for "is a switch" (REG).
- Icons in rows, row sliders, switches as On/Off text: the kit draws none yet.
- Peak memory on the X4 is not measured: device gate open. The menu is built into the firmware,
  so there is no store-file size to record.

## Activation items (todo 90)

- Remove the interface-6 `Menu`, `bar.rs`, `panel.rs`, `notice.rs`, `track.rs` and the suites in
  `tests/` with the app wiring (`.plugin(paperpaw_plugin_menu::Menu)` in `crates/paperpaw-app`
  and the harnesses that use it); then no shim stays.
