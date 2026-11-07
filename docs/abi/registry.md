# Interface 7: registry (REG)

Status: **draft** of family 2 (#228), for review before records and schema land. Items marked [D]
are proposed decisions, [Q] open questions. Nothing here is generated yet.

REG lets plugins see and change what the host has registered: settings and their groups, plugins,
actions, screens, slots, menus, key bindings and setup steps. The settings editor, menu editor,
keys, profiles, setup, plugin manager, store, about, home and help are plugins in interface 7; this
family is what they need from the host, and nothing more.

It builds on: the service rules (`stages-services.md` section 2, D4), the command rules
(`commands.md`, slot 15), the event rules (`core-events.md`), the D1 features
(`screens-input.md` section 7) and the declaration records already in place (`setting.*`,
`screen.spec`, `action.spec`, `menu.*`, `stage.spec`).

## 1. Shape

- [D] **Reads are one call service, `core.registry` major 1** (slots 33 and 34): synchronous,
  bounded, paged. No new host slot.
- [D] **Writes are commands** (slot 15), staged in the callback and applied after it, in batch
  order, as `command.set_setting` already is. One rule for every write: whole-batch validation
  before anything is staged, the host-known submitter, authority checked at submit and again when
  applied. No REG write is a service op, so a write never runs inside another plugin's callback
  and never has to undo half a change.
- [D] **Changes are announced as events**: `event.setting_changed` (64) and
  `event.plugins_changed` (70) exist; REG adds the menu, binding and preview hints and the plugin
  request pair (section 5).
- [D] **No callbacks into plugins while answering.** Everything a reply holds is host state:
  declarations, stored values, item state pushed with `command.set_item_state` (60). An interface-6
  `ItemStateFn` or `AppliesFn` callback has no interface-7 form; section 6 replaces the one
  that is still needed.

## 2. The service `core.registry` (major 1)

### 2.1 Paging

- [D] Every list op takes one `service.reg_query` record (`scope`, `cursor`, `generation`,
  `flags`) and answers with up to 32 rows followed by exactly one `service.end` (52:
  `flags`, `total`, `generation`, `next`), the paging record of `core.usage`.
- [D] A page holds as many **whole** rows as fit in the op's `max_reply` (4,096 bytes), at most 32.
  A row is never cut; text fields inside a row are cut at a UTF-8 boundary where section 3 says so.
- [D] `generation` is the host's **registry generation**: it changes when the set or order of a
  list changes (a plugin loaded, stopped, enabled, removed; a declaration admitted; a menu, binding
  or slot changed). The first call passes `cursor` 0 and `generation` 0; later calls pass `next`
  and the `generation` of the last `service.end`. A call whose non-zero generation is not the
  current one returns `ERR_STATE` with `out` untouched: start again at 0. `service.end.flags`
  bit 0 means "last page".
- [D] **Values are read live**, not under the generation: a setting value may change between two
  pages without a new generation. `event.setting_changed` announces it.

### 2.2 Operations

All query ops: `effect` `query`, `feature` null, `bound` `none`, `max_input` 256, `max_reply`
4,096 unless stated. Op ids are fixed once published.

| Id | Op | Input | Reply | Notes |
| --- | --- | --- | --- | --- |
| 1 | `settings` | `reg_query` (`scope`: group id, plugin id or empty for all) | `reg_setting`*, `end` | registry order |
| 2 | `setting` | `reg_key` | `reg_setting`, `reg_setting_extra`, `reg_text`?, `reg_choice`* | one setting in full; `max_reply` 4,096; more than fits of the choices: section 3 |
| 3 | `groups` | `reg_query` (`scope`: parent group or empty) | `reg_group`*, `end` | |
| 4 | `plugins` | `reg_query` (`scope` empty) | `reg_plugin`*, `end` | every plugin the host knows, loaded or not |
| 5 | `plugin` | `reg_key` | `reg_plugin`, `reg_text`?, `reg_need`* | description, features, requirements |
| 6 | `actions` | `reg_query` (`scope`: plugin id or empty) | `reg_item`*, `end` | |
| 7 | `screens` | `reg_query` (`scope`: plugin id or empty) | `reg_item`*, `end` | |
| 8 | `slots` | `reg_query` (`scope` empty) | `reg_slot`*, `end` | with the bound and the default screen |
| 9 | `menu` | `reg_query` (`scope`: menu id) | `reg_menu_row`*, `end` | evaluated rows, section 2.4 |
| 10 | `bindings` | `reg_query` (`scope`: layer id, `global` or empty for all layers) | `reg_binding`*, `end` | |
| 11 | `inputs` | `reg_query` (`scope` empty) | `reg_item`*, `end` | the inputs this device has (`button.menu`, `tap.center`, ...) |
| 12 | `setup` | `reg_query` (`scope` empty) | `reg_setup_step`*, `end` | evaluated applicability, section 6 |

- [D] `scope` that names nothing known gives an empty page (`total` 0), not an error: a plugin may
  ask for a group or menu of a plugin that is not installed.
- [D] `setting` and `plugin` with an unknown key: `ERR_NOT_FOUND`.
- [Q] Op 13 `stack` (screen ids on the stack, owner, guarded) belongs to UIX, not here.

### 2.3 Authority (what a reply shows)

- [D] **Values:** `reg_setting.value` carries the value exactly when slot 16 would give it to the
  caller. Otherwise it is empty and flag `hidden` is set. A **secret** setting is never shown,
  only whether it has a value (`has_value`). This is the rule of `event.setting_changed.old`.
- [D] **Guarded** settings are listed with flag `guarded`; writing them follows `commands.md`
  (own plugin, the host-resolved settings editor and `core` only).
- [D] **Everything else is public**: titles, groups, plugins, actions, screens, slots, menus and
  bindings are what the user sees on the device anyway. Plugin freedom (decision 44): no feature
  gates a read. A plugin declares `core.registry` in its metadata (D1 service catalog) and that
  declaration is what the store shows.

### 2.4 Menus

- [D] `menu` answers the rows the host would show for that menu now: the user's own definition
  (`command.set_menu`) if there is one, otherwise the declared `menu.entry`/`menu.section` rows in
  order, without `menu.hide` rows and rows whose `menu.need` the device does not meet. Each row
  carries the item's last pushed state (`command.set_item_state`: checked, enabled, shown, label,
  badge), resolved to text in the user's language.
- [D] The editor's "all entries" view is `menu` with `flags` bit 0 (`all`): every row that could be
  in the menu, hidden ones flagged `hidden`, and rows the user removed flagged `removed`.

## 3. Records (service block, 1025-1047)

All are `service.*` records in the growth block 1024-1535 (`stages-services.md` section 2.2).
Text the user reads (`title`, `help`, `label`, `badge`, choice titles) is **translated by the host**
in the user's language, from the owning plugin's catalog, and cut at a UTF-8 boundary to the cap.
Ids (`key`, `plugin`, `item`, ...) are never cut: a declaration that admitted them fits the caps.

| Tag | Record | Fields |
| --- | --- | --- |
| 1025 | `service.reg_query` | `scope: str 128`, `cursor: u32`, `generation: u32`, `flags: u32` |
| 1026 | `service.reg_key` | `key: str 128` |
| 1027 | `service.reg_setting` | `key: str 128`, `group: str 128`, `kind: u32`, `flags: u32`, `source: u32`, `title: str 128`, `value: bytes 64` |
| 1028 | `service.reg_setting_extra` | `min: i64`, `max: i64`, `step: i64`, `unit: str 32`, `section: str 128`, `follows: str 128`, `shown_key: str 128`, `shown_value: bytes 64`, `editor_kind: u32`, `editor: str 128`, `choices: u32` |
| 1029 | `service.reg_choice` | `value: str 64`, `title: str 128` |
| 1030 | `service.reg_text` | `text: str 1024` |
| 1031 | `service.reg_group` | `id: str 128`, `parent: str 128`, `page: str 128`, `order: i32`, `title: str 128` |
| 1032 | `service.reg_plugin` | `id: str 64`, `name: str 128`, `version: str 32`, `kind: u32`, `state: u32`, `flags: u32`, `problem: u32`, `problem_arg: u32`, `update: str 32` |
| 1033 | `service.reg_need` | `kind: u32`, `key: str 128`, `version: str 32`, `flags: u32` |
| 1034 | `service.reg_item` | `id: str 128`, `plugin: str 64`, `flags: u32`, `title: str 128` |
| 1035 | `service.reg_slot` | `id: str 128`, `screen: str 128`, `default: str 128`, `title: str 128` |
| 1036 | `service.reg_menu_row` | `item: str 128`, `kind: u32`, `flags: u32`, `order: i32`, `title: str 128`, `icon: str 64`, `badge: str 64` |
| 1037 | `service.reg_binding` | `layer: str 128`, `input: str 64`, `action: str 128`, `origin: u32` |
| 1038 | `service.reg_setup_step` | `id: str 128`, `page_kind: u32`, `page: str 128`, `order: i32`, `flags: u32`, `title: str 128` |

Field meanings:

- `reg_setting.kind`: the `setting.spec` kind numbers. `flags`: bit 0 guarded, 1 secret, 2 scoped,
  3 kept on reset, 4 advanced, 5 hidden (value withheld), 6 has value, 7 long (value over 64
  bytes: read it with slot 16), 8 preview (the value shown is a preview, section 4.3). `source`:
  0 default, 1 preset, 2 profile, 3 user, 4 book scope.
- `reg_setting.value` is the `settings_value` encoding, empty when hidden or long.
- `reg_setting_extra` repeats `setting.spec`/`setting.extra` fields the editor needs;
  `choices` is the number of choices. `setting` answers as many `reg_choice` rows as fit after the
  other records, in order; [Q] whether a choice list longer than a page needs a paged `choices` op
  (Plugin's inventory: the longest list today).
- `reg_text` in `setting` is the help text; in `plugin` the description.
- `reg_plugin.kind`: 0 built in, 1 data, 2 code; `state`: 0 running, 1 off, 2 failed; `flags`:
  bit 0 enabled, 1 restart needed, 2 required by another enabled plugin, 3 linked into the
  firmware, 4 safe mode; `problem`: interface-6 `PluginProblem` order (0 none), `problem_arg`
  its number (`built_for`); `update`: the version waiting, empty if none.
- `reg_need.kind`: 0 feature (`key` = `uses.*`, `flags` bit 0 required), 1 service (`key`,
  `version` = major.revision), 2 required plugin (`key` = id, `version` = at least).
- `reg_item.flags` (actions, screens, inputs): bit 0 hidden from lists, 1 sealed (not rebindable,
  not removable from menus), 2 gesture (inputs).
- `reg_menu_row.kind`: 0 action, 1 setting, 2 submenu, 3 section; `flags`: bit 0 checked, 1
  enabled, 2 hidden, 3 removed, 4 user-defined menu.
- `reg_binding.origin`: interface-6 `BindingOrigin` (0 plugin, 1 device, 2 preset, 3 user); an
  empty `action` is "bound to nothing".
- `reg_setup_step.page_kind`: 0 setting, 1 screen; `flags` bit 0 applies now, bit 1 done.

## 4. Commands (slot 15)

Command records in the growth block 1536-2047 (`screens-input.md` section 8). The submit rules of
`commands.md` apply to each: whole batch validated before staging, nothing staged on any failure,
key and text bytes within the stage budget.

| Tag | Record | Fields |
| --- | --- | --- |
| 1536 | `command.reset_setting` | `key: str 128`, `flags: u32` |
| 1537 | `command.preview_setting` | `key: str 128`, `value: bytes 512`, `flags: u32` |
| 1538 | `command.preview_end` | (none) |
| 1539 | `command.set_scope` | `path: str 512` |
| 1540 | `command.set_menu` | `menu: str 128`, `items: str 2048`, `flags: u32` |
| 1541 | `command.bind_key` | `layer: str 128`, `input: str 64`, `action: str 128`, `flags: u32` |
| 1542 | `command.bind_slot` | `slot: str 128`, `screen: str 128` |
| 1543 | `command.set_plugin_enabled` | `plugin: str 64`, `enabled: u32` |
| 1544 | `command.request_plugins` | `plugins: str 1024` |
| 1545 | `command.answer_plugin` | `request: u32`, `plugin: str 64`, `outcome: u32` |

### 4.1 Settings

- [D] `reset_setting`: `flags` 0 resets `key`; bit 0 resets the group `key`; bit 1 every setting
  of plugin `key`; bit 2 every setting (`key` empty). Settings kept on reset and secrets are left
  as they are, as in interface 6. Authority per setting as `command.set_setting`: a group or plugin
  reset skips guarded settings the submitter may not write, and says so in the log; a single
  guarded key it may not write is `ERR_INVALID`. In a book scope it forgets the scope's value.
- [D] `set_scope`: the book scope for scoped settings, by a path on root 1, while the submitting
  screen stays open; empty ends it. The screen identity is stamped at submit (as `command.redraw`):
  not the submitter's own top screen gives `ERR_STATE`.

### 4.2 Menus, bindings, slots, plugins

These change the user's configuration of the whole device, so they need the D1 feature
**`uses.registry`** (the store shows it); without it, `ERR_DENIED` for the batch.

- [D] `set_menu`: `items` is the menu's new rows as item ids, one per line (`\n`), at most 64;
  `flags` bit 0 (`reset`, `items` empty) returns to the declared rows. The host refuses the batch
  with `ERR_INVALID` for an unknown id, a submenu cycle, or a menu that would lose a **sealed**
  item. Saved in `[menus]` of `paperpaw.ini`, as in interface 6.
- [D] `bind_key`: binds `input` in `layer` (a screen or slot id, or `global`) to `action`; empty
  `action` binds nothing (interface 6 `NOTHING`); `flags` bit 0 (`reset`) returns the input to the
  declared and preset bindings. The host keeps the **escape**: a batch that would leave no input
  that opens the main menu, or that rebinds the host's reserved chord (hold Power), is refused with
  `ERR_DENIED`. Saved in the user's `paperpaw.ini`, in effect after the callback.
- [D] `bind_slot`: binds `slot` to `screen`; empty `screen` returns to the default. The screen must
  be declared and offered for that slot, else `ERR_INVALID`.
- [D] `set_plugin_enabled`: switches a plugin on or off; it takes effect at the next start
  (`reg_plugin` flag `restart needed`). Switching off a plugin another enabled plugin requires,
  or the submitter itself, is `ERR_DENIED`. A built-in the host needs (the settings editor while
  it is the only one) is `ERR_DENIED`.

### 4.3 Preview

- [D] `preview_setting` rows of one batch are **one preview set**: it replaces the previous set
  wholly. `flags` bit 0 previews "as if not set" (`value` empty). Values are checked as
  `command.set_setting` checks them. `preview_end` ends the preview. A preview also ends when the
  submitting screen closes (identity stamped at submit, as `set_scope`).
- [D] While a preview is active, slot 16 and `reg_setting` give the previewed value (flag
  `preview`). Only one preview exists device-wide; a new set from another plugin replaces it.
  Kernel side effects of a setting (language, rotation) wait for the kept value, as in
  interface 6.

## 5. Events

| Tag | Record | Fields | To |
| --- | --- | --- | --- |
| 75 | `event.registry_changed` | `now_ms: u64`, `what: u32` | subscribers (hint) |
| 76 | `event.preview_changed` | `now_ms: u64` | subscribers (hint) |
| 77 | `event.plugins_requested` | `now_ms: u64`, `request: u32`, `by: str 64`, `plugins: str 1024` | the installer only |
| 78 | `event.plugin_answered` | `now_ms: u64`, `request: u32`, `plugin: str 64`, `outcome: u32` | the requesting plugin only |

- [D] `registry_changed.what` bits: 0 menus, 1 bindings, 2 slots, 3 actions/screens, 4 setup, 5
  groups. A hint (`core-events.md` section 2): re-read what you show. The registry generation
  (section 2.1) changed with it. Plugin set changes stay `event.plugins_changed` (70); setting
  values stay `event.setting_changed` (64).
- [D] **Plugin requests.** `command.request_plugins` (ids, one per line, at most 16, the interface-6
  `MAX_REQUESTED_PLUGINS`) is refused with `ERR_INVALID` for a bad id. The host drops ids already on
  the device (any version, on or off); if none is left, nothing is sent. Otherwise it numbers the
  request and raises `plugins_requested` with the host-known submitter as `by` to **the installer**:
  [Q] the plugin bound to slot `core.installer` (the store), as interface 6 routes it today.
  The installer asks the user and answers each id with `command.answer_plugin` (`outcome` 0
  installed, 1 declined, 2 failed, 3 not found); the host checks that the submitter is the installer
  and the request is open, and raises `plugin_answered` to the requester only. A request stays open
  until every id is answered, the installer stops, or the device restarts; then the open ids are
  answered `failed` by the host.
- [D] All four follow the EVT pending limits and drop rules (`core-events.md` section 3.3).
  `plugins_requested` and `plugin_answered` are edges and never coalesced; the two hints are.

## 6. Setup steps

- [D] Setup steps become a declaration, `setup.step` (tag 21, declaration block):
  `name: str 128`, `title: str 128`, `help: str 128`, `order: i32`, `page_kind: u32`,
  `page: str 128`, `applies_key: str 128`, `applies_value: bytes 64`, `flags: u32`.
  `title` and `help` are message keys of the declaring plugin, as in `setting.spec`.
- [D] It replaces the interface-6 `AppliesFn` callback with the `shown_if` rule of
  `setting.extra`: the step applies while setting `applies_key` has the value `applies_value`
  (empty key: always). The host evaluates it for op `setup`; no plugin code runs.
- [D] Done is the host's record of a step finished (`flags` bit 1 in `reg_setup_step`), set with
  [Q] a `command.setup_done` record or by the setup plugin's own setting; Plugin's inventory decides.

## 7. Memory (X4)

- Replies are written into the caller's buffer (at most 4,096 bytes); the host keeps no per-call
  state between pages. A cursor is an index into the host's own ordered tables.
- Translation for a reply resolves one message at a time into the reply buffer; no catalog stays
  loaded for it beyond the host's on-demand catalog rule.
- Commands keep `commands.md`'s stage byte budget. `set_menu` (2 KiB) and `request_plugins`
  (1 KiB) are the largest; a profile may set a lower budget, which then refuses them with
  `ERR_NO_SPACE`.
- The preview set is host heap: at most 16 rows, each within the setting caps, allocated
  fallibly (`ERR_NO_MEMORY` at submit, nothing staged).

## 8. Interface-6 calls and their interface-7 form

| Interface 6 | Interface 7 |
| --- | --- |
| `env.settings()` specs, groups, values | ops 1-3; values also slot 16 |
| `env.plugins()` | ops 4, 5; `event.plugins_changed` |
| `env.actions()`, `env.screens()`, `env.screen_title()` | ops 6, 7 |
| `env.slots()`, `slot_screen`, `slot_screens`, `slot_title`, `slot_choice`, `has_slot` | op 8 |
| `env.menus()` (`MenuView`) | op 9 |
| `env.keys()` (`KeyBindings`), `env.inputs()` | ops 10, 11 |
| `env.setup()` | op 12, declaration `setup.step` |
| `Command::SetSetting` | `command.set_setting` (4), unchanged |
| `Command::ResetSetting` | `command.reset_setting` |
| `Command::PreviewSettings`, `PREVIEW_CHANGED` | `command.preview_setting`/`preview_end`, event 76 |
| `Command::SetScope` | `command.set_scope` |
| `Command::SetMenu`, `MENU_CHANGED` | `command.set_menu`, event 75 bit 0 |
| `Command::Bind` | `command.bind_key` |
| `Command::BindSlot` | `command.bind_slot` |
| `Command::SetPluginEnabled` | `command.set_plugin_enabled` |
| `Command::RequestPlugins`, `PLUGINS_REQUESTED`, `PLUGINS_ANSWERED` | `command.request_plugins`, `command.answer_plugin`, events 77, 78 |
| `Command::SetKeyLayer` | UIX (a screen's own key layer) |
| `Command::LoadPlugin`, `InstallPlugin`, `RemovePlugin`, `Restart` | DEV (`core.install`, power commands) |
| `env.stack()`, `env.top()` | UIX |

## 9. Hand-offs

- **Kernel:** records and schema in `abi.def` and `abi/services/core.registry@1.def`, generator,
  lock, tests of the pinned lists; this document final.
- **Core:** the `core.registry` service over the host tables (paging, generation, translation,
  authority), the command arms, the four events, `setup.step` admission.
- **Services:** the SDK facade (typed rows, paging iterator, command builders).
- **Plugin:** the ports; confirm the [Q] items from the needs inventory.

## 10. Open questions

1. [Q] The installer's identity (section 5): a slot, a declared role, or a stage.
2. [Q] Choice lists longer than one `setting` page (section 3).
3. [Q] Setup "done" (section 6).
4. [Q] Presets: interface 6 applies them through settings files; is a `preset.apply` command
   needed, or do profiles keep writing settings with `command.set_setting`?
5. [Q] Plugin-defined inputs and gestures (`InputSpec`, `GestureSpec`): UIX or later.
