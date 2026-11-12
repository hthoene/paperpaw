# Interface-7 port

Version 1.0.6. Native plugin: `src/native_plugin.rs` (`NativePluginManager`),
pure model in `src/native_model.rs`, built-in descriptor `NATIVE_ENTRY` (feature
`linked`), declarations in `declarations.rs` (built into the image by
`build.rs`), metadata in `native/` (`python3 tools/abi-metadata.py --schema
abi/abi.def --input native/metadata.json --bytes native/metadata.bin --json
native/metadata.inspected.json`, and the same for `metadata.host.*`; services
`core.registry`, `core.device`, `core.ui`; no feature: the registry and device
reads need none, the own data folder is not root 1). The screens use
`paperpaw-ui-kit`. The interface-6 plugin, its screens and Kernel tests are
gone; the app does not register it, and no store-file row existed.

## Ported

- **List** (`plugin-manager.list`): every plugin the host knows (registry op 5),
  the host first, then by name; state (*On*, *Off*, *Always on*, *Required*,
  *Error*), note (what happens to it, its description, version, *With code*,
  why code does not run, the problem numbers in the interface-6 order); choosing a
  row switches the plugin (`set_plugin_enabled`, applies after a restart; a plugin
  that must stay on is not switched); the footer says *Applies after a restart*
  while a change waits. The places to get more (the menu `plugin-manager.more`,
  e.g. the store) are rows above the plugins that run their action.
- **Screens** (`plugin-manager.screens`): the places that several screens are
  offered for, the user's choice on the right (or *Automatic*); choosing a place
  opens its candidates (a hand-over with `slot`), choosing a candidate or
  *Automatic* binds the place (`bind_slot`) and goes back. The kept list of
  places already seen (`screens.txt` in the data folder) and the question *Use
  {name} for {slot} now?* (confirm, answered through `event.answered`) are as
  before: asked once, not where the user chose, nor where the screen is used
  already, nor for a screen bound lower than the one used; the first start only
  notes what there is.
- **Notices at a start** (`Started`): the plugins the shell did not install (device
  op 14) and safe mode (device op 5), announced; safe mode's last.
- **Plugins changed**: the open screens read their rows again, the focus kept.
- Fallible allocation throughout (`mem`, `text`, `Messages`); no panics; no
  `&s[a..b]`; descriptions are cut to 160 bytes at a letter's edge (a note
  shows two lines).

## Tests

- Unit (`src/native_model.rs`): problem order, state, sort, notes, notices, offers.
- `tests/native_alloc_refusal.rs`: every allocation of the model's text and
  notice building refused in turn (no abort).
- `tests/abi7_declarations.rs`: the image equals the builders; the host admits
  the screens, the actions and the events and refuses the menu places and icons
  (#944).
- `tests/linked_entry.rs`, `tests/translations.rs` (en/de agree; every key asked
  for exists).
- The plugin-manager list and the card-language tests of interface 6 read the screen
  through a kernel; they are the acceptance cases below. Elsewhere: the store's
  test registers a stub carrying the manager's English texts (the store still
  says why a plugin did not start with them); the settings and desktop tests that
  open the manager are `#[ignore]`d with the reason.

## Acceptance pending (explicit, never faked)

All cases are in `tests/acceptance_pending.rs` (`#[ignore]`d refusals; the
interface-6 plugin had no pictures of its own).

| Need | Affects |
| --- | --- |
| H1 screen commands, H3 composition, H4 input; H2 dialogs; registry and device reads through the loader | the screens, the question, the notices |
| `plugin-how-text-read`: the registry gives no "how it works" text | holding a row says so (notice) |
| `reg-plugin-required-for`: only flag bit 2 says "needed", not for which places | *Needed by other plugins.* instead of *Needed for: home screen, ...* |
| `plugin-origin-read`: the interface-6 plugin read other plugins' install marks | the *From the store / Local* words are not shown |
| `code-room-larger-layout`: the device reads give the free room, not the larger layout | the web-flasher wording of *No room* |
| `restart-now-sender-342`: `restart` is in the typed DEV commands (#342) | the *Restart now* row (the footer still says it applies after a restart) |
| `setting-editor-opener`: no SDK way to open the setting editor of `core.reading-app` | the reading-app row of *Screens* |
| #944: the host refuses the menu places and icons | the entries in the settings and in `plugin-manager.more` |

## Not carried

- `install::adopt`: the host takes plugins installed by hand (docs/abi/device.md,
  command 1576 and event 97).
- The *Restart now* button of the bar (row and footer, see above).
- The plugin manager's texts are still used by the interface-6 store
  (`plugin-manager.code.*`); the store carries its own when it is ported.

## Format and reader caps (todo 377, #867 R2)

The host refuses a plugin's whole declaration batch with `ERR_FULL` when it would pass the caps of
`document.md` section 1 (8 formats a plugin, 32 on the device; 4 readers a plugin, 16 on the device). The
registry says so as `reg_plugin.problem` 14 (too many formats) or 15 (too many readers), state failed, with
`problem_arg` the cap the batch ran into (`registry.md`, Kernel's `kernel/docs-problem-caps-228`; Host fills
the values). The list's note says it, en/de, with the number: *plugin-manager.formats.too-many*,
*plugin-manager.readers.too-many*; a value without a limit (0) says "Could not be loaded" (the text would have a hole),
as does a value this plugin does not know. The limit is whichever the batch ran into (8 formats or 4
readers a plugin, 32 or 16 on the device), so the words say "limit", not "the device". **Host side pending:** until Host fills 14 and 15 a refused
batch shows as before (`plugin-manager.failed`). Version unchanged (1.0.6, above Beta 3's 1.0.5).

## Field, badge and lookup-provider caps (draft 37, problems 16 to 18)

The same refusal for the other declaration caps (`registry.md`, Kernel PR #1039): 16 too many book fields
(`problem_arg` 8 a plugin or 64 on the device, `library.md` 1), 17 too many book badges (4 or 16), 18 too many
lookup providers (8 or 32, `document.md` 7). The note says it with the number (*plugin-manager.fields.too-many*,
*plugin-manager.badges.too-many*, *plugin-manager.providers.too-many*, en/de, as 14/15); a value without a limit
(0) or one this plugin does not know says "Could not be loaded". **Host side pending:** until Host fills 16 to 18 a
refused batch shows as before. Version unchanged (1.0.6, above Beta 3's 1.0.5).
