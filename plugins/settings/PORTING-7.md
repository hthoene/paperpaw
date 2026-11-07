# Interface-7 port

Version 1.0.6. Native plugin: `src/native_plugin.rs` (`NativeSettings`: five screens, questions,
number editing), registry reads in `src/native_data.rs`, what the pages are made of without the host
in `src/native_model.rs` (visibility, groups, sections, stepping, units, search words, theme in use).
`NATIVE_ENTRY` (feature `linked`), declarations in `declarations.rs` built into the image by
`build.rs`, metadata in `native/` (regenerate with `python3 tools/abi-metadata.py --schema
abi/abi.def --input native/metadata.json --bytes native/metadata.bin --json
native/metadata.inspected.json`, likewise `metadata.host.*`). Drawing uses `paperpaw-ui-kit`.
The interface-6 `SettingsPlugin` and its suites stay: `paperpaw-app` and other plugins' tests still
link them until the host activates native plugins; then they go together (no shim).

## Ported

- Main page: groups with visible settings (by order, then name; widgets' own groups left out), the
  pages other plugins add to `core.settings-pages` under their section headings, *Search*, the
  advanced switch, *Reset all*. Group page: parts, actions plugins add to the group, the theme row in
  the appearance group, settings under their sections with value, unit and description, *More*
  (advanced settings one level down), *Reset to defaults* (asks first, leaves language, kept-on-reset
  settings and secrets, "only this book" last). Rows follow the registry's `shown`, `advanced`,
  `guarded` and `secret` flags and the developer switch.
- Numbers: confirm starts changing, left/right (or up/down, page buttons) preview the value
  (`preview_setting`), confirm keeps it, back drops it; touch screens get − and + on the row.
  Switches and choices of two change at once; longer choices open `core.choose-value`
  (`settings.choose`); text asks with the host dialog; a setting's own editor opens as declared.
- Themes: *None* (every appearance setting but size, contrast and logo back to default) and the
  `core.theme` presets through `apply_preset`; the one in use is marked.
- Search over names and descriptions (accents folded) of the listed settings and the pages.
- Slots `core.settings` and `core.choose-value`; parameters (hand-over) `id`, `more`, `focus`,
  `text`, and `scope` (a root-1 book path: the screen sets the book scope while it is open).

## Tests

`native_model`: group membership, offered/visible levels, main-page order, sections, stepping and
ends, units, placements, search folding, theme in use. Declaration parity with the interface-6
registration, linked entry, construction in the kernel's registered scope (refused as `Unsupported`
until the host admits every declaration family, #944). No desktop golden: pages need host input
(H1/H4). `tests/acceptance_pending.rs`: refusals that never pass (pages, number preview, reset,
search, secrets, restart).

## Not ported yet (explicit)

- *Passwords and keys* (list, type anew, remove) and secret text settings: need the `core.secrets`
  SDK facade and host service (SEC; Services todo 79). Secret settings are not listed meanwhile.
- *Restart PaperPaw*: no DEV restart command facade yet (command 1574, H10).
- Placements (`widgets.clock@2`) pages and a widget look row: need the placement/widget host pieces
  (H3); the preview of a group's settings (a widget composed above them) needs slot 35.
- No icons in rows, switches are shown as *On*/*Off* text, no sliders (numbers use − and +); the kit
  draws none yet. Reading-font sizes that look alike are not merged when stepping.
- A unit is the host's translated text: `{count}` is replaced, else the number comes first. Whether
  the registry's `unit` is the plural template or a finished word is for Kernel to say.
- Per-setting `core.registry` op 2 for page rows (range, unit, section, description): a page of 40
  settings makes 40 calls when it is built, search up to one per listed setting. Peak memory on the
  X4 is not measured: device gate open.
- Settings is built into the firmware (no `.ppn`), so there is no store-file size to record; the
  native packing waits for the shared 6→7 linker migration.
