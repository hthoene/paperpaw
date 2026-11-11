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

- *Events 75 and 76:* the host admits `event.registry_changed` and `event.preview_changed` since #916
  (coalesced hints), so the plugin's four event subscriptions are admitted as a whole and
  `tests/hosted_construction.rs` constructs it in the real registered scope (no longer ignored).
  A registry change (and a plugins change) rebuilds what shows on top and notes the covered pages
  as stale; a preview change redraws. The widget preview above a group's settings stays blocked on
  the registry's `previews` read (SDK accessor) and the host's slot 35 (`settings-group-preview`).

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
  (`keep`, `remove`) are run over the scripted host (`tests/scripted_secrets.rs`): the question for
  a removal and its yes/no, a value typed anew (kept byte for byte as typed, white space trimmed; nothing typed leaves the secret), and
  a secret no longer kept removed without asking. The list runs page after page and every refusal of
  the host is swept; the rows' allocations are swept. That the typed and read buffers are wiped is not asserted by a test: they are freed straight after (`wipe_typed`, `mem::wipe`), so no memory can be read back without undefined behaviour; the wiping calls are on every path (`read_typed`, `change`, `wipe_typed`) and the kit's own `mem::wipe` is unit-tested. Not run: drawing, moving the focus and going
  back, which are screen commands the scripted host refuses (BLOCKED H1).
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

- *Placement pages:* the screen `settings.group` with `id` = `widgets.clock@2` shows the settings of
  its widget's group as that placement has them: values read with the placement's key (slot 5
  falls back to the widget's value, `registry.md` 10.1), a value the placement does not set itself
  counts as default (op 18 with the placement), changes are set under the placement's key, and
  *Reset to defaults* resets that placement only (`reset_setting` bit 3). No parts, actions, theme or
  preview on such a page. The chooser for a placement's choice reads the options of the setting it
  stands for. The id rules (`unplaced`, `unplaced_key`) are unit-tested and swept; the page itself
  needs the host (ignored case `settings-placement-page`).

- *Widget look:* on the appearance page a second row, *Widget look*, opens the chooser of the
  presets of kind `core.widget-look` (`settings.themes` with `mode` = `widgets`), as the themes
  row opens the themes': *None* first (every covered setting back to its default), the look in
  use marked, choosing one applies it (`apply_preset`). What a look covers is each widget's
  frame, padding, size, alignment, paper, ink, corners, frame line and shadow, and the settings of
  all widgets and the gap (`native_model::widget_look_covers`, from the registry's widgets;
  tested). A theme no longer resets the frame of all widgets (it never did in interface 6). The
  row shows only where a look exists. The chooser's drawing needs the host (`settings-widget-look`).
  **Differences and host duties:** (1) `apply_preset` takes the covered set per kind from the host
  (`registry.md` 4.3): the host's theme set must leave out `core.widgets.frame` and its look set must
  equal `widget_look_covers`, else the mark here and the result of choosing differ. (2) Beta 3
  counted a widget's `.frame` as in use only where it was written down, even at its default; here
  "in use" compares with `is_default`, so a look that names a frame at its default may be marked as
  *None*. Not asserted by a test (the host decides what `is_default` means for a named default).

## Tests

`native_model`: group membership, offered/visible levels, main-page order, sections, stepping and
ends, units, placements, search folding, theme in use. Declaration parity with the interface-6
registration, linked entry, construction in the kernel's registered scope (refused as `Unsupported`
until the host admits every declaration family, #944). No desktop golden: pages need host input
(H1/H4). `tests/acceptance_pending.rs`: refusals that never pass (pages, number preview, reset,
search, secrets, restart).

## Not ported yet (explicit)

- The preview of a group's settings (a widget composed above them, interface 6
  `env.widgets().preview`): the facade (`ui.compose`, `widget_make`) exists, but the registry does
  not say which widget previews a group (`widget.spec.previews` is not in `reg_widget`; asked of
  Kernel). Ignored case `settings-group-preview`, blocked by `reg_widget.previews` and the host's
  slot 35 (H3).
- No icons in rows, switches are shown as *On*/*Off* text, no sliders (numbers use − and +); the kit
  draws none yet. Reading-font sizes that look alike are not merged when stepping.
- A unit is the host's translated text: `{count}` is replaced, else the number comes first. Whether
  the registry's `unit` is the plural template or a finished word is for Kernel to say.
- Per-setting `core.registry` op 2 for page rows (range, unit, section, description): a page of 40
  settings makes 40 calls when it is built, search up to one per listed setting. Peak memory on the
  X4 is not measured: device gate open.
- Settings is built into the firmware (no `.ppn`), so there is no store-file size to record; the
  native packing waits for the shared 6→7 linker migration.

## Right-to-left (`ar`: a regression against Beta 3 until this is done)

Beta 3 mirrored the interface where its language is written right to left (Arabic). The UI kit now
does so for what it draws (`Style::rtl`, read from `core.language`: header, list rows, items, dialog,
chips, the sheet's pager; `Style::flip`, `flip_x` and `text::label_in` for the rest). What this plugin
draws itself is not mirrored yet, so its screens are left to right in those parts for `ar` until it
takes the kit's helpers (and, where it keeps its own keys, swaps left and right as `Chips::step` does).

## Heap of the open pages (#248, X4)

- The registry is read **a page at a time** (`Reader::each_setting`): a page of 250 settings is never held
  whole. The main page, the group page, the chooser, the themes, search and the reset plan each copy only
  what they need: the heading groups need no titles or values (`Fields::Light`); a group's page copies only
  its own group's settings; the chooser one; the themes the covered ones; search copies a setting and drops
  it unless it matches. Behaviour is as before (search finds the same settings).
- `ui_kit::mem::push` grows a full vector by its length up to 16 elements, not by doubling, so a list of a
  few hundred rows has at most 15 spare slots instead of up to half of it.
- Measured on the native-default branch (`fw/native-default-228`, emulator `--heap-report`, X4 profile): the
  peak of opening Settings from a reading screen fell from 248.5 to 154.2 KiB (live heap 136.5 KiB at the
  end); the TXT reading flow's peak (`memory.rs`) from 234.1 to 168.0 KiB, which the start and the book now
  set (168.0 KiB at the first page, 136.2 KiB live), not Settings. The gate of #248 (108.5 KiB) is not met
  by the Settings change alone: what native plugins keep resident at start is Core's.
