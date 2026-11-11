# Interface-7 port (slices 1 to 3: the menu list, the quick panel, the reader's bar)

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

## Slice 3: the reader's bar (`src/native_bar.rs`, `src/native_bar_screen.rs`)

Ported. `menu.list` opened with `mode=bar` (the reader opens `core.menu` so, with `menu`
`core.reader-menu`, `hide`, `title`, `text`, `page` and `action`; `menu_bar.rs` in the reader) is
Beta 3's bar (`bar.rs`) at the top of the screen. `native_bar` is the bar without the host: the
hand-over read (`Handed`, every text copied to its size), the entries (no headings, none named by
`hide`, no setting shown only while another has a value), the layout (the title on the first line,
the chapter at the start of the second and the page whole at its end, `fit_pair` as Beta 3; then the
entries as columns as wide as their icon or label, as many to a row as fit with two gaps between
them, rows filled evenly, justified between the margins, taps counting halfway to the next entry,
mirrored right to left; a line under the bar), what a tap reaches (entries, then the two lines, then
nothing; outside closes), the buttons (up/previous/page back, down/next/page on, left and right
swapped right to left; the first press on a touch screen only shows the focus, pages turned there
are taken and do nothing; *Select* chooses; the menu button and *Back* close; others are the
host's), the focus round the ends past faint entries (it stays where all are), and what choosing
does (an action closes the bar, then runs; a submenu opens as a list over the bar, depth 1, so an
action in it closes both; a switch and a choice of two change in place; longer choices open
`core.choose-value`, a setting's own editor opens, numbers, text and lists open their settings page;
an entry the registry knows no setting for runs; a faint entry does nothing). A tap on the two
lines closes the bar and runs `action` (only an id; none without). `native_bar_screen` reads the
rows and settings from `core.registry`, draws with `paperpaw-ui-kit` in the status font (as Beta 3,
over the status lines) with the icons of the icon font (`icons::glyph`, 1.6 × the text size): an
entry that is on filled dark, the focus a frame round it, faint entries in the note ink, each told as
a button (focused, on, disabled). It follows `SettingChanged`, `PluginsChanged` and
`RegistryChanged` like the list (read anew, the focus kept on its entry); the focus shows where
there is no touch screen or buttons were used last (`ScreenShown.last_input`). Memory as Beta 3:
the texts, the entries (descriptions let go) with their settings, and the cells of the last frame;
the paint list lives for one draw. Tests: `tests/bar_mode.rs` (hand-over, `hide`, the title area's
action, keys and focus, what entries do, layout, `fit_pair`; over the scripted host the bar reads
the menu it was handed, passes hidden entries, goes round with the buttons and opens a submenu
through a hand-over).

Differences from Beta 3, named:
- Short labels: Beta 3 translated the entry's *title key* followed by `.short`; a registry row has
  only the translated title, so the bar asks for the *entry id* followed by `.short`
  (`reader.read-aloud.short` = "Listen") and, for a setting, its title key's
  (`core.night-mode` → `core.setting.night-mode.short` = "Night"). The reader names the short labels
  of the entries whose ids differ from their title keys (`reader.show-toc.short`,
  `reader.quick-settings.short`, `reader.show-annotations.short`, `reader.skim-book.short`, en/de),
  so the bar matches Beta 3 (emulator, X4, en/de). A plugin whose entry ids differ from its title
  keys needs such `<entry id>.short` keys; else its full title shows. `shorten()` has no scripted-host
  test (the scripted host has no message catalogs).
- The page does not follow the reader while the bar is open (Beta 3's `MENU_PAGE` event, #117:
  pages counted after the bar opened); interface 7 has no such event (SDK/host).
- Where no status font is bound the bar uses the interface font (Beta 3 drew only the empty bar).
- A text setting opens its settings page, not the keyboard, and a number opens the page focused on
  it rather than its group's page, as the interface-7 list does.
- The focus's redraw is the whole screen (partial), not the bar's area; a tap before the first
  frame is not taken.
- The scripted host has no fonts: drawing and the taps on a drawn bar are tested through the model,
  not over a host; no allocation-refusal sweep of the host paths (H1/H4, as the list).

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

- Tiles (style *Tiles*): open as the list meanwhile. (The reader's bar is ported: slice 3.)
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
