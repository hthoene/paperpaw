# Interface-7 port

Version 1.0.3. Native plugin: `src/native_plugin.rs` (`NativeBluetooth`), pure
model in `src/native_model.rs`, built-in descriptor `NATIVE_ENTRY` (feature
`linked`), declarations in `declarations.rs` (built into the image by
`build.rs`), metadata in `native/` (`python3 tools/abi-metadata.py --schema
abi/abi.def --input native/metadata.json --bytes native/metadata.bin --json
native/metadata.inspected.json`, and the same for `metadata.host.*`; services
`core.device`, `core.registry`; feature `uses.bluetooth`, not required: the
host ignores Bluetooth commands where there is no radio). The screens use
`paperpaw-ui-kit`. The interface-6 plugin, its page, its search and its Kernel
tests are gone; the app does not register it. It builds on the typed Bluetooth
senders and event 98 of the SDK (PR #363).

## Ported

- **Page** (`bluetooth.page`, also the settings group's page): the bonded devices
  (device op 11) with kind and *connected*; a keyboard or remote shows *On*/*Off*
  and choosing it switches its keys (`bt_keys`); a sensor has one row per plugin
  that uses Bluetooth (read from the registry's needs, only while a sensor is
  bonded) and choosing it gives the sensor to the plugin or takes it back; a
  long press or the menu button forgets a device (`bt_forget`, with a notice);
  *Add a device* opens the search; *Remotes* steps through by hand, while
  reading, always (setting `bluetooth.remotes`). The rows are read again on
  every Bluetooth event and when a setting changes, the focus kept.
- **Adding** (`bluetooth.search`): opening starts a scan (`bt_scan`, 30 s); found
  devices strongest first, each once; choosing one stops the scan, connects and
  then pairs (the kernel asks the reader to confirm); on *paired* a remote's or
  keyboard's keys go to the key maps, the device is disconnected, a notice says
  so and the screen goes back; a declined pairing, a failed link and a failed
  scan say why (*Not paired*, *It did not answer*, *Bluetooth is off*). Event 98 has no
  bonded flag, so *paired* beside a found device comes from comparing its id with the
  bonded list (read when the search opens and on every *changed* or *paired*). The
  state of a search (*connecting*, *searching*) follows the command that was queued: a
  refused command shows nothing new.
- **Remotes** (events `Started`, `Resume`, book opened/closed): by hand nothing
  connects by itself and the remotes switched on are wanted again; while
  reading the remotes connect while a book is open; always at start and on
  waking. Nothing is read or drawn on an event while neither screen is open.
- **Slot** `core.bluetooth` is offered for `bluetooth.page`: the plugin bound to
  it sees `given` in device op 11 and gets event 98 (no subscription; Kernel
  ruled the same for `event.answered`).
- Fallible allocation throughout (`mem`, `text`, `Messages`); no panics; no
  `&s[a..b]`.

## Not ported yet (named, tests ignored)

- **Status item** (the Bluetooth sign while a device is connected, tapping it
  opens the page): the host asks the owner through the stage `core.status` and
  the plugin trait has no stage callback yet; drawing the sign needs the native
  widget toolkit (`status-item-stage-callback`,
  `status-item-drawn-icon-widget-toolkit`). The item is not declared until it
  can answer.
- **How it works** text (`how/*.txt`) stays in the package; the registry cannot
  read it (`plugin-how-text-read`).
- Menu place, icons, menu needs and the slot offer are refused by the host until
  #944 (`tests/abi7_declarations.rs` says which); `make` is refused as
  unsupported until then, the same state as the other native plugins.

## Tests

- Unit (`src/native_model.rs`): rows, remotes policy and plan, search order,
  adding (connect, pair, report), declined/lost/failed, kinds.
- `tests/native_alloc_refusal.rs`: every allocation of the rows and the search
  refused in turn (no abort).
- `tests/abi7_declarations.rs`: the image equals the builders; the host admits
  the settings, the screens, the action and the events.
- `tests/linked_entry.rs`, `tests/translations.rs` (en/de agree; every key asked
  for exists).
- `tests/acceptance_pending.rs`: the interface-6 behaviours as ignored,
  explicit refusals naming what is missing (radio through the loader, the host
  binding the slot, H1/H3/H4, #944, status stage, widget toolkit).

## Elsewhere

- `shells/desktop/tests/memory.rs` `bluetooth_stays_within_the_x4_pro_budget` drives
  the interface-6 plugin (the page, `bluetooth.remotes`, a remote's keys); it is
  `#[ignore]`d with "re-enable at Bluetooth activation".
- The headless step `a_plugin_switched_on_applies_after_the_restart`
  (`shells/desktop/tests`) is ignored: the interface-7 plugin manager has no
  restart row until the DEV command senders (#342). Re-enable it with them.
