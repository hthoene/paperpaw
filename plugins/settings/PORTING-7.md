# Interface-7 port

Version 1.0.7. Native plugin: `src/native_plugin.rs` (`NativeSettings`: seven screens, questions,
number editing), passwords and keys in `src/native_secrets.rs`, registry reads in `src/native_data.rs`, what the pages are made of without the host
in `src/native_model.rs` (visibility, groups, sections, stepping, units, search words, theme in use).
`NATIVE_ENTRY` (feature `linked`), declarations in `declarations.rs` built into the image by
`build.rs`, metadata in `native/` (regenerate with `python3 tools/abi-metadata.py --schema
abi/abi.def --input native/metadata.json --bytes native/metadata.bin --json
native/metadata.inspected.json`, likewise `metadata.host.*`). Drawing uses `paperpaw-ui-kit`.
The interface-6 `SettingsPlugin` and its suites stay: `paperpaw-app` and other plugins' tests still
link them until the host activates native plugins; then they go together (no shim).

## Ported

- *Passwords and keys* (`src/native_secrets.rs`, screens `settings.secrets` and `settings.secret`, the
  action `settings.secrets.open`, its entry in `core.settings-pages` at order 95 and the icon `key`,
  as in interface 6; parity in `tests/abi7_declarations.rs`). On `core.secrets` with the feature
  `uses.secrets` (not required: the device may not let the settings list; the list is then empty and
  the main page's row stays hidden): the list of every plugin's secrets by label (the owner's text for
  a label that is a catalogue key, else the label as it is, else the name), the value never shown, in
  the host's order (at most 128); choosing one offers *Type anew* (only for a changeable one; a
  masked question, the answer trimmed, an empty one ignored, over 1,024 bytes told with
  `settings.secret.too-long`) and *Remove* (asked first, *Remove*/*Keep*), after which the screen
  goes back and the pages that show secrets are made anew. A secret that is not kept (any more)
  offers only *Remove*, which is nothing, and goes back. The main page's row shows only while a secret
  is kept (the service is asked when the page is built). **Differences from interface 6:** a refused
  save (card away, secret gone, not changeable) is told with `settings.secret.failed`, it was silent;
  an owner of a secret is named by its plugin id and the secret's screen is opened with the hand-over
  `id` = `plugin:name` (a name may hold `:`; an owner's id never does); the host, not the plugin,
  checks that a replaced secret exists and is changeable. What is typed is read through buffers that
  are wiped, and the text copy is wiped after the host has it. The calls that use the service
  (`keep`, `remove`) are not run in a test (the scripted host does not answer `i18n` and the
  questions' answers need dialogs); the list is, over a scripted service, page after page, and
  every refusal of the host is swept (`tests/scripted_secrets.rs`); the rows' allocations are swept.
  Secret *text settings* stay out of the pages as in interface 6 (they are secrets, listed here).

- Main page: groups with visible settings (by order, then name; widgets' own groups left out), the
  pages other plugins add to `core.settings-pages` under their section headings, *Search*, the
  advanced switch, *Reset all*, *Restart* (asks first; `CommandSender::restart`, so the
  metadata declares `uses.power` as not required: a shell without it refuses the command, and a notice says so). Group page: parts, actions plugins add to the group, the theme row in
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
