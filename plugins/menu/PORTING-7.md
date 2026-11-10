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

## Slice 2a: the quick panel's model (`src/native_panel.rs`)

The panel without the host (slice 2b below is the screen).
`lines` turns a menu's rows (with the registry's setting facts and a choice's translated options)
into rows of controls as interface 6 did: switches and buttons several to a row (at most 3, as many
as stay readable, shared out evenly, never switches with buttons), a number as `−` / `+`, a choice
of 2 to 4 options that fit as segments, everything else (a setting with its own editor, more
options, text, lists) opens. A setting the registry does not know is a button, as in interface 6.
`layout` places them at the bottom (80 % of the screen at most) or hanging from the top (95 %, from
the very edge), in pages with a pager bar when they need more. Controls name their menu row by
index. Everything allocates fallibly; text widths come from a `Measure` the screen will give.
Tests: grouping, per-row shrink, segments and their fit, label width, bottom and top layout, pages,
right-to-left columns, allocation-refusal sweeps.

Differences from interface 6, named: no slider rows (the kit has none; a marked number is a
`−` / `+` stepper on touch screens too, until the kit has a slider: SLIDER); the pager height and
button size come from the screen's style when the screen (2b) is made.

## Slice 2b: the quick panel's screen (`src/native_panel_screen.rs`)

`menu.panel`, declared `overlay`, fills the slot `core.panel` (parameters `menu`, and `mode=top` for
the quick panel hanging from the top edge). The rows come from `core.registry` (op 12 and the
settings' facts; a choice of two to four options also reads its values and their translated labels).
Drawing is done in the draw callback with the layout of `native_panel` (the layout is kept for the
taps and the buttons of the last completed frame): switches and buttons as outlined buttons (filled
while on), numbers as `−` value `+`, choices as segments (the chosen one filled), anything else a
row with its value and `›`, faint rows drawn in the disabled ink and not chosen, the pager of the
sheet (`page of pages`, arrows on touch screens), the focus a ring round the control. Input: the
buttons step the focus over the enabled controls round the ends and across pages, *Select* uses it,
*Back* and the menu button close, a tap on a control uses it (the focus shown goes), a tap on the
pager's arrows turns the page, a tap outside closes. Using a control stages `set_setting` (switch,
`−` / `+` stepped to the number's ends, the chosen option), or closes the panel and runs the action,
or closes it and opens a submenu as the main menu's slot, or opens the setting's list, editor or page
(as the menu list does). The panel follows `SettingChanged`, `PluginsChanged` and `RegistryChanged`:
the top one is read anew and redrawn, the others when they show. The four help hints are declared
(`menu.help.panel*`, en and de).

Differences from interface 6, named: no sliders (the kit has none: SLIDER; a number is `−` / `+` on
touch screens too); no icons (ICON: the SDK wrapper for the icon slot and its host wiring); a switch
has no bullet for "this setting is scoped to the book" (the registry gives none); the focus's
redraw is the whole sheet, not the two rows (partial redraws of an area wait for the area command);
a number's `−` and `+` stand at the ends of the row, not by the value; the held tap and the touch
pointer (slider drags) are not handled (H4); a font that cannot measure a text counts it as no
width. The panel reads at most 64 rows of a menu (`MAX_ROWS`; interface 6 had no cap). The help hint says only "tap to switch" until the held tap works (H4). Scripted-host cases for what a control stages wait for H1. Memory: one `Part` per row (row, setting facts, up to 4 values and labels), the layout of
its controls, the paint list of one page, a registry reader (512 + 4,096 B) while reading; the
X4 peak is not measured.

## Moved to the host

- The notice (`menu.notice`, slot `core.notice`): notices are `command.notify` drawn by the host
  (`docs/abi/ui.md`, *Messages*); the plugin has nothing to show and the screen goes with the
  interface-6 code at activation. This is an accepted change from interface 6 (Kernel): a plugin can
  no longer replace how notices look; the host shows each for `core.notice-seconds`.

## Tests

`native_model`: hidden items, depth, headings, tiles rule, the focus passing faint rows, what
choosing does, stepping numbers and their ends, units, listing rules, and allocation-refusal sweeps.
`paperpaw-ui-kit`: sheet geometry, faint rows. Declaration parity with the interface-6
registration, linked entry, construction in the kernel's registered scope (a real pass: the host admits every
declaration, the events 64, 70 and 75 among them).
`tests/acceptance_pending.rs`: ignored cases that name what they wait for.

## Not ported yet (explicit; each is an ignored acceptance case)

- Tiles and the reader's bar (`mode=bar`, style *Tiles*): icons and the status font role; the
  canvas has no icon operation. Both open as the list meanwhile.
- The quick panel: sliders (the kit has none), icons and the touch drag (slice 2b lists the rest).
- A switch that is off: `reg_menu_row.flags` carry "checked" only, so a switch off looks like a
  plain action (shown with no value). Needs a flag for "is a switch" (REG).
- Icons in rows, row sliders, switches as On/Off text: the kit draws none yet.
- New limit: submenus stop at a depth of 8 menus (`MAX_DEPTH`); interface 6 had no cap, so a
  loop of submenus ended only at the stack. Choosing a submenu at the cap does nothing.
- `draw_sheet` (kit) and `send_open` / `Reader::open` (plugin) have no allocation-refusal sweep:
  they call the host (fonts, snapshot, `core.ui`, registry), which the unit tests do not have. The
  model, the sheet geometry and the copies are swept; the host paths wait for the production
  loader (H1/H4).
- Open gate: peak heap on the X4 is not measured. Per open the menu holds two `SERVICE_BUFFER`s
  for the hand-over, a registry reader (512 + 4,096 bytes) and the rows of the menu; to be
  measured at activation.
- Peak memory on the X4 is not measured: device gate open. The menu is built into the firmware,
  so there is no store-file size to record.

## Activation items (todo 90)

- Remove the interface-6 `Menu`, `bar.rs`, `panel.rs`, `notice.rs`, `track.rs` and the suites in
  `tests/` with the app wiring (`.plugin(paperpaw_plugin_menu::Menu)` in `crates/paperpaw-app`
  and the harnesses that use it); then no shim stays.
