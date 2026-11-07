# Interface 7 family DEV: device, power, radios, installing, firmware (draft)

Contract draft for #228 (assignment #1025). Items marked [D] are proposed decisions, [Q] open
questions for Plugin, Firmware and Senior. Inputs: Firmware's DEV inventory and draft (#931,
`/home/agent/shared/dev-family-228.md`, whose sections 1, 2 and 8 are taken as the inventory and
whose open questions section 9 settles), the plugin needs inventory (#984, DEV section), the
C-family proposals (#890 2.12), `registry.md` (plugin rows, `set_plugin_enabled`, presets,
`request_plugins` / `answer_plugin`), `core-events.md` (events 70-72), `ui.md` (host-drawn
confirmations, the guard) and owner decisions 124, 132 and 134. Round 2 folds in Firmware's check
of the OTA path (`crates/paperpaw-x4/src/updates.rs`, `ota.rs`, `shells/xteink/src/`) and Plugin's
gap review (`/home/agent/shared/plugin-dev-gaps-1025.md`). Doc-only; records follow once approved.
Tags are proposed and numbered without gaps at the records step.

## 0. Scope and principles

- [D] **Reads are a service, changes are commands, outcomes are events.** The host service
  `core.device` (section 2) answers what the device is and how it stands; commands in the slot-15
  growth block (section 3) ask for changes, applied after the callback; events (section 4) say what
  came of them, after which a plugin reads again. Interface 6 worked this way already
  (`Command::Wifi` → `WIFI_CHANGED` → `env.wifi()`); interface 6's `Env` accessors have no copies.
- [D] **Consent is a declared feature per area** (signed metadata, as `uses.storage`): `uses.wifi`
  (the radio and network policy), `uses.power` (sleep, off, restart, power modes), `uses.install`
  (installing and removing plugins, trust evidence), `uses.firmware` (installing firmware),
  `uses.bluetooth`. Reads need none. A command whose feature is not declared refuses the whole batch
  with `ERR_DENIED` at submit. Separate keys tell the user more on the store page than one
  `uses.device` (Firmware's Q6). First-party plugins that sent these commands under `Uses::NOTHING`
  (diagnostics, power, settings, plugin-manager, reading-goal) declare them in their ports.
- [D] **The host decides and enforces trust.** Whether a plugin or a firmware image fits, is signed,
  is revoked and has room is decided by the host's own checks (the loader's), never by the asking
  plugin. The store's trust rules move into the host unchanged (5.2): an author key pinned at the
  first install, a key change only along rotations signed by the pinned recovery key, revoked keys
  never loaded. The store carries the catalog's evidence in typed records and parses or decides
  nothing. **Every plugin-triggered install asks the user** (decision 132) and **every firmware
  change asks the user** (decision 134), in a host-drawn confirmation no plugin draws, covers or
  answers, and what is installed is exactly what the user was shown (5.2, 5.3).
- [D] **Interrupted changes never activate half a state** (inventory rule 2): plugin installs are
  staged and swapped by rename with a journal, firmware goes to the other slot and becomes bootable
  only once wholly written and verified, and a start after a cut finishes or rolls back (section
  5.4).
- [D] **Enabled, available and running are different states** (rule 3): REG op 6 rows
  (`reg_plugin.state`, `problem`) keep them apart; DEV adds no second inventory, only the plugins
  the start could not install (op 14), which REG never lists.
- [D] **Portable batches:** a well-formed command without a request for a capability the device
  lacks (Wi-Fi on Android, Bluetooth on the X4) is accepted and ignored with a log line (interface 6
  "ignored elsewhere"), so one missing capability never drops a batch's other records; plugins gate
  on `dev_device.caps`. A command **with a request** (install, remove, firmware, Wi-Fi join and
  host) always ends: where the capability is missing its outcome is delivered at once with status
  `ERR_UNSUPPORTED` (reason 19), so no caller waits for an outcome that never comes. Feature checks
  are not capability checks: they refuse.
- Out of DEV: the plugin inventory and enabling (REG), stored secrets and randomness (SEC), HTTP and
  sockets (NET, SOCK), scheduled work (ASY), the battery and display snapshot (slot 4, done). Not
  carried for 1.0 (no first-party user; additive later): Bluetooth GATT and advertising, the
  developer link, wake holds, USB drive mode (#32), screenshots.

## 1. Features

| Key | Allows | Plugins (first-party) |
| --- | --- | --- |
| `uses.wifi` | commands 1561-1568 | wifi, transfer, diagnostics |
| `uses.power` | commands 1569-1572 | power, diagnostics, settings, plugin-manager, reading-goal, store, update |
| `uses.install` | commands 1573, 1574, 1577-1579; REG `answer_plugin` | store, fonts-browser, plugin-manager, update |
| `uses.firmware` | commands 1575-1576 | update |
| `uses.bluetooth` | commands 1580-1588 | bluetooth |

- [D] With `uses.install` defined, REG's `answer_plugin` refusal (`registry.md` 4.2: `ERR_DENIED`
  for every plugin until DEV defines it) becomes: allowed for a plugin that declares `uses.install`,
  `ERR_DENIED` for others; the test that fails once `uses.install` exists is replaced by tests of
  both.

## 2. The service `core.device` (major 1)

No feature. Bare replies and paging as `core.registry` (`page_rows` 32). A part the device has
nothing for answers `ERR_NOT_FOUND` (Bluetooth on the X4, Wi-Fi on Android, firmware on the desktop
and Android), never a record of zeros, so "not here" differs from "off"; `caps` and these answers
agree (a host test).

| Op | Name | Input | Reply | Notes |
| --- | --- | --- | --- | --- |
| 1 | `device` | (none) | `dev_device` | identity, class, capabilities, start kind |
| 2 | `values` | (none) | `dev_value`*, `end` | named hardware values, at most 8 |
| 3 | `firmware` | (none) | `dev_firmware` | version, install state and progress |
| 4 | `builtin` | `reg_query` | `dev_builtin`*, `end` | plugins built into the image, paged |
| 5 | `native` | (none) | `dev_native` | what a code plugin must be built for, safe mode, code room |
| 6 | `check` | `dev_check_ask` | `dev_check` | will this plugin or firmware file load here (section 5.1) |
| 7 | `memory` | (none) | `dev_memory` | heap and stack, at most one real probe per 250 ms |
| 8 | `wifi` | (none) | `dev_wifi` | radio state |
| 9 | `networks` | `reg_query` | `dev_network`*, `end` | found networks, strongest first, at most 32 |
| 10 | `bluetooth` | (none) | `dev_bluetooth` | radio state |
| 11 | `bonded` | `reg_query` | `dev_bonded`*, `end` | bonded devices, at most 16 |
| 12 | `power` | `dev_interval` | `dev_power` | power level and mode; `every(base)` |
| 13 | `install` | `dev_request` | `dev_install` | state of an install, removal or firmware request |
| 14 | `not_installed` | `reg_query` | `dev_not_installed`*, `end` | plugins this start could not install, at most 32 |
| 15 | `keys` | (none) | `dev_key`*, `end` | the device's physical keys, at most 32 |

| Tag | Record | Fields |
| --- | --- | --- |
| 1094 | `service.dev_device` | `id: str 64`, `model: str 64`, `catalog: str 32`, `class: u32`, `caps: u64`, `controls: u32`, `tls_sessions: u32`, `start: u32`, `slept_ms: u64`, `flags: u32` |
| 1095 | `service.dev_value` | `name: str 32`, `value: i64` |
| 1096 | `service.dev_firmware` | `version: str 64`, `project: str 32`, `chip: u32`, `room: u64`, `undone: u32`, `state: u32`, `done: u64`, `total: u64`, `reason: u32`, `request: u32` |
| 1097 | `service.dev_builtin` | `id: str 64`, `version: str 32` |
| 1098 | `service.dev_native` | `abi_major: u32`, `abi_draft: u32`, `target: u32`, `code_file: str 32`, `safe_mode: u32`, `crashed: str 64`, `last_loaded: str 64`, `pending: u32`, `not_installed: u32`, `room_free: u64`, `room_largest: u64` |
| 1099 | `service.dev_check_ask` | `root: u32`, `path: str 512`, `kind: u32` |
| 1100 | `service.dev_check` | `reason: u32`, `needs_draft: u32`, `bytes: u64`, `signer: str 64`, `key: bytes 32`, `sha256: bytes 32`, `id: str 64`, `version: str 64`, `flags: u32` |
| 1101 | `service.dev_memory` | `heap_free: u32`, `heap_size: u32`, `largest: u32`, `stack_free: u32`, `stack_size: u32`, `external_free: u32`, `external_size: u32`, `flags: u32` |
| 1102 | `service.dev_wifi` | `state: u32`, `ssid: str 32`, `address: str 45`, `host_name: str 63`, `guests: u32`, `flags: u32` |
| 1103 | `service.dev_network` | `ssid: str 32`, `signal: i32`, `flags: u32` |
| 1104 | `service.dev_bluetooth` | `radio: u32`, `max_links: u32`, `flags: u32` |
| 1105 | `service.dev_bonded` | `device: u32`, `name: str 64`, `kind: u32`, `flags: u32`, `given: str 64` |
| 1106 | `service.dev_interval` | `base_ms: u32` |
| 1107 | `service.dev_power` | `level: u32`, `mode: str 128`, `flags: u32`, `stretch: u32`, `every_ms: u32` |
| 1108 | `service.dev_request` | `request: u32` |
| 1109 | `service.dev_install` | `request: u32`, `kind: u32`, `state: u32`, `status: i32`, `reason: u32`, `flags: u32`, `done: u64`, `total: u64`, `id: str 64` |
| 1110 | `service.dev_not_installed` | `plugin: str 64`, `reason: u32` |
| 1111 | `service.dev_key` | `name: str 32` |

- [D] `dev_device` (interface 6 `env.device()`, `caps()`, `catalog_device()`): `id` the stable
  device id (`xteink-x4`), `model` its name, `catalog` the store's device name (`x4`, empty for
  none), `class` the memory class (0 small, 1 medium, 2 large), `caps` bit 0 touch, 1 network, 2
  Wi-Fi managed by PaperPaw, 3 Bluetooth, 4 firmware updates, 5 speech, 6 sleep, 7 rotation, 8
  colours, 9 button row, 10 system clipboard, 11 frontlight, 12 black and white, 13 power off, 14
  pen (unknown bits ignored, new ones appended); `controls` the power controls (bit 0 radio, 1 CPU
  clock, 2 buttons, 3 frontlight, 4 system saver); `start` 0 unknown, 1 power on, 2 woke, 3 restart,
  4 after a fault; `flags` bit 0 `slept_ms` known. The user's device name is the setting
  `core.device-name` (slot 5).
- [D] `dev_value` (interface 6 `env.status().value(name)`, diagnostics only; Firmware's Q3): named
  values (`usb-pin-high`, `usb-frames`, `frontlight-percent`, `temperature-c`), at most 8, names
  `[a-z0-9-]{1,32}`; a build may add names without a new interface. A value the device cannot read
  now is left out, never sent as 0.
- [D] `dev_firmware` (interface 6 `env.firmware()`): `state` 0 idle, 1 waiting for the user, 2
  installing, 3 installed (restart to use), 4 failed; `reason` the outcome reason (5.5) of the last
  request, 0 while none ended; `request` the request it belongs to; `undone` 1 when the last start
  undid a new image (5.4). No public keys are listed: verifying is the host's.
- [D] `dev_native` (interface 6 `env.native()`): `abi_major` 7, `abi_draft` the interface draft,
  `target` the chip id (5 ESP32-C3), `code_file` (`code.ppn`), `safe_mode` 0 none, 1 the last starts
  with code plugins failed, 2 the user held the key at power-on (unknown values as 1);
  `crashed` the plugin that was running when a start failed and `last_loaded` the plugin loaded last
  (each empty when not known, interface 6 `SafeMode`); `pending` how many installed or removed code
  plugins take effect at the next start; `not_installed` how many rows op 14 has;
  `room_free`/`room_largest` the code flash. Interface 6's `fingerprint` and `toolchain` go (the C
  boundary does not depend on the compiler); see section 7 for their replacements in about and the
  store.
- [D] Op 14 `dev_not_installed` (interface 6 `NotInstalled`): the plugins the start had to install
  (written by the web flasher behind the firmware) and did not, with `reason` 1 other author (code
  signed by another key than the one pinned), 2 damaged or not a plugin file this firmware reads, 3
  revoked (unknown values as 2). Plugin manager names each with its reason, as today. Plugins
  switched off by safe mode stay REG rows (`reg_plugin.state` 4, `problem`).
- [D] `dev_memory` (interface 6 `env.memory()`): a probe walks the stack on the X4, so the host does
  one real probe per 250 ms and answers the cached one inside that window. `flags` bit 0 cached, bit
  1 `largest` known, bit 2 the stack values known, bit 3 external memory present (its values known);
  a field whose bit is clear is 0 and means "not known", never "none free", as diagnostics'
  `memory_lines` shows today.
- [D] Op 15 `dev_key` (interface 6 `InputCaps.keys`, `Buttons::new`): the names of the physical keys
  (`back`, `confirm`, `left`, `right`, `up`, `down`, `power`, the interface 6 `buttons` names), in
  the device's order; an empty list when not known. `caps` bit 9 says only whether a button row
  exists.
- [D] `dev_wifi` (`state` 0 off, 1 on, 2 joining, 3 online, 4 hosting; `flags` bit 0 scanning, 1
  wanted, 2 busy), `dev_network` (`flags` bit 0 secured, 1 known: a password is stored, SEC),
  `dev_bluetooth`, `dev_bonded` (`flags` bit 0 connected, 1 remote keys on; `given` the plugin a
  sensor is handed to, shown only to the plugin bound to the host slot `core.bluetooth`, else
  empty).
- [D] `dev_power` (interface 6 `env.power()`): `level` 0 normal, 1 saving, 2 offline; `mode` the
  worn power mode (a REG preset id of kind `core.power-mode`, `registry.md` op 16); `flags` bit 0
  automatic, bit 1 online: plugins may use the network for their own work now (interface 6
  `online()`); `every_ms` the interval for `dev_interval.base_ms` stretched by the level (interface
  6 `every(base)`; 0 for no base).

## 3. Commands (slot 15, growth block)

| Tag | Record | Fields | Feature |
| --- | --- | --- | --- |
| 1561 | `command.wifi_on` | (none) | `uses.wifi` |
| 1562 | `command.wifi_off` | (none) | `uses.wifi` |
| 1563 | `command.wifi_scan` | (none) | `uses.wifi` |
| 1564 | `command.wifi_join` | `request: u32`, `ssid: str 32`, `password: str 63`, `flags: u32` | `uses.wifi` |
| 1565 | `command.wifi_leave` | (none) | `uses.wifi` |
| 1566 | `command.wifi_host` | `request: u32`, `ssid: str 32`, `password: str 63` | `uses.wifi` |
| 1567 | `command.network_none` | (none) | `uses.wifi` |
| 1568 | `command.go_online` | `for_ms: u32` | `uses.wifi` |
| 1569 | `command.sleep` | (none) | `uses.power` |
| 1570 | `command.power_off` | (none) | `uses.power` |
| 1571 | `command.restart` | (none) | `uses.power` |
| 1572 | `command.wear` | `mode: str 128`, `flags: u32` | `uses.power` |
| 1573 | `command.install_plugin` | `request: u32`, `root: u32`, `path: str 512`, `flags: u32` | `uses.install` |
| 1574 | `command.remove_plugin` | `request: u32`, `plugin: str 64`, `flags: u32` | `uses.install` |
| 1575 | `command.install_firmware` | `request: u32`, `root: u32`, `path: str 512`, `key: bytes 32`, `flags: u32` | `uses.firmware` |
| 1576 | `command.firmware_restart` | (none) | `uses.firmware` |
| 1577 | `command.install_author` | `request: u32`, `author: bytes 32`, `recovery: bytes 32`, `sha256: bytes 32` | `uses.install` |
| 1578 | `command.install_rotation` | `request: u32`, `to: bytes 32`, `signature: bytes 64` | `uses.install` |
| 1579 | `command.trust_revoked` | `plugin: str 64`, `key: bytes 32`, `signature: bytes 64` | `uses.install` |
| 1580-1588 | `command.bt_scan` (`lasting_ms`), `bt_stop_scan`, `bt_pair`, `bt_connect`, `bt_disconnect`, `bt_forget` (`device`), `bt_give` (`device`, `plugin: str 64`), `bt_take_back` (`device`), `bt_keys` (`device`, `on`) | as named, `device: u32` | `uses.bluetooth` |

- [D] One record per action (Firmware's Q2): tags are plentiful in the growth block, each record is
  checked by the generated codec without a per-action table, and a later action is a new record.
- [D] **Requests:** `request` is the sender's own non-zero number; a request number the plugin
  already has waiting is `ERR_INVALID` at submit. The host reserves the request's table entry and
  its kept outcome **at submit** (fallibly; device-wide at most 4 install, removal or firmware
  requests and 4 radio requests waiting, else `ERR_FULL` for the batch), so an accepted request
  always has room for its outcome and is never dropped later. Outcomes are kept until delivered,
  then readable with op 13 until the plugin's next request of the same kind or its stop.
- [D] **Wi-Fi** (interface 6 `WifiCommand`, `NoNetwork`): `join` and `host` need a non-empty `ssid`;
  `host`'s password is 8-63 bytes, `join`'s 0-63; `join.flags` bit 0 remember the password (SEC's
  store, owned by the Wi-Fi plugin). A password is never logged nor shown in a record any other
  plugin reads. A newer `join` or `host` from any plugin supersedes a waiting one, whose outcome is
  `wifi_failed` reason 4. `go_online` (`for_ms` 1-600,000) is honoured only within 2,000 ms after a
  user input (else `OK` and ignored, logged): no plugin turns the radio on in the background by it.
- [D] **Power** (`Sleep`, `PowerOff`, `Restart`, `Wear`): commands of a batch apply in order. A
  power command waits until every screen command before it in the same batch (`command.open`,
  `ui.md`, e.g. the sleep slot with `off=1`, as power does today) has drawn its first frame and that
  frame's refresh has ended (the panel idle), then acts; so the e-ink keeps the screen that shows
  the device's real state. If that screen fails to open, the power command still acts, on the frame
  shown. `restart` saves what plugins keep first; `power_off` and `sleep` deliver `suspend` first
  (after the frame). `wear.mode` is a REG preset id of kind `core.power-mode` (`registry.md` ops
  16-18; an unknown id is `ERR_NOT_FOUND` at submit), `flags` bit 0 automatic. While an instance
  guards (`ui.md` 4.3) only the guard's plugin and the host may restart or switch off
  (`ERR_DENIED`); sleep stays open to all.
- [D] Batch limits: at most one power command (1569-1572) and one firmware command per batch, at
  most two Wi-Fi commands, at most one `install_author` and 8 `install_rotation` per request (the
  store's `MAX_STEPS`), both after their `install_plugin` in the same batch, at most 16
  `trust_revoked` per batch (`ERR_INVALID` beyond or out of place).
- [D] **Network manager** (Firmware's Q7): `event.network_wanted`/`idle` go only to the plugin the
  user chose as network manager, the host setting `core.network-manager` (a plugin id; the Wi-Fi
  plugin by default; REG-visible and resettable). Without one they go nowhere and network work waits
  for the user.

## 4. Events

| Tag | Record | Fields |
| --- | --- | --- |
| 71 | `event.wifi_changed` (EVT) | as in `core-events.md` |
| 72 | `event.power_changed` (EVT) | as in `core-events.md` |
| 70 | `event.plugins_changed` (EVT) | as in `core-events.md` |
| 92 | `event.wifi_scanned` | `now_ms: u64`, `networks: u32` |
| 93 | `event.wifi_failed` | `now_ms: u64`, `request: u32`, `ssid: str 32`, `reason: u32` |
| 94 | `event.network_wanted` | `now_ms: u64` |
| 95 | `event.network_idle` | `now_ms: u64` |
| 96 | `event.firmware_changed` | `now_ms: u64`, `request: u32`, `state: u32` |
| 97 | `event.install_done` | `now_ms: u64`, `request: u32`, `kind: u32`, `status: i32`, `reason: u32`, `flags: u32`, `id: str 64` |
| 98 | `event.bluetooth` | `now_ms: u64`, `kind: u32`, `device: u32`, `name: str 64`, `device_kind: u32`, `signal: i32`, `reason: u32` |

- [D] 92, 94-96 are hints (coalesced per kind; read again with section 2). 93 and 97 are **edges
  kept like answers** (`ui.md` 3.2) in the room reserved at submit (section 3).
- [D] 93 (`reason` 0 other, 1 not found, 2 password, 3 timeout, 4 superseded, 5 unsupported here;
  unknown as 0) goes to the plugin whose `join` or `host` it answers (`request`); a failure of a
  join the host started itself (a remembered network at wake) has `request` 0 and goes to the
  network manager. A join that succeeds raises 71 to all; its sender reads op 8.
- [D] 97 ends every install, removal and firmware request exactly once and goes to its sender only:
  `kind` 0 plugin install, 1 firmware, 2 removal; `status` OK or the error of 5.5 with its `reason`;
  `flags` bit 0 the plugin's data was kept (removal), bit 1 takes effect at the next start; `id` the
  plugin. A removal or install is reported **only once done** (5.2), never when queued. 98 goes to
  the plugin bound to `core.bluetooth`. `firmware_changed` comes at most every 250 ms while
  installing (at most once per written block, 5.3).

## 5. Installing plugins and firmware

### 5.1 Checking before asking

- [D] Op 6 `check` (`dev_check_ask`: `root` 0 the caller's Data or 1 with `uses.storage`, `path` a
  `.ppn` file, a plugin folder or a firmware image, `kind` 0 plugin, 1 firmware; Firmware's Q4) runs
  the loader's own checks without changing anything: `reason` 0 fits, else the reason of 5.5 that
  an install would end with (`needs_draft` for reason 10); `bytes`; `signer` the key's name (empty
  for a key the device does not know); `key` the signing key and `sha256` the code's (plugin) or the
  image's (firmware) digest, both empty when unsigned; `id`, `version`; `flags` bit 0 data plugin
  (no code), bit 1 this id's author key is pinned here and is `key`, bit 2 this id is pinned to
  another key (an install needs a rotation, 5.2), bit 3 the image is signed by PaperPaw, bit 4 a
  foreign image (another firmware than PaperPaw). The store and update show it before they ask; they
  never parse `.ppn` headers or update trailers themselves. A check reads the whole file to hash it
  (bounded steps, about the time of a copy).

### 5.2 Plugins

- [D] `install_plugin` (interface 6 `InstallPlugin`, `LoadPlugin`, `install::adopt`): the plugin
  downloaded a `.ppn` or a folder into `path` (its root 0, or root 1 with `uses.storage`); `flags`
  bit 0 replace an installed plugin of the same id (an update), bit 1 allow an older version.
- [D] **Trust evidence** (store `trust.rs` `Named`, `expected`, `follow`, `revoked.rs`): the
  catalog's word for a code plugin travels in the same batch as `install_author` (the author key the
  catalog names, the recovery key it names for a first install, empty for none, and the code's
  expected `sha256`, empty for none) and up to 8 `install_rotation` (the catalog's rotation
  statements, in the author's order). The host applies the store's rules itself: an id pinned to the
  signing key installs; an id pinned to another key installs only if the rotations lead from the
  pinned key to the signing key, each checked against the pinned recovery key (steps before the
  pinned key are skipped, as `follow`), else reason 6 key changed; a new id installs only if
  `install_author.author` is the signing key (reason 5 not the author), pinning it and `recovery`; a
  signing key revoked for that id never installs (reason 7). A code digest other than `sha256` is
  reason 18. Without evidence (a plugin the user sideloads through plugin manager) a new id is
  pinned to its signing key once the user confirms the key's fingerprint, shown by the host as "not
  from a catalog"; a pinned id still needs its rotations. Pins and rotations change **only when the
  install commits** (the journal of 5.4), never at a check, a declined prompt or a failure, so a
  declined update leaves the installed plugin loading as before.
- [D] `trust_revoked` (store `revoked.rs`, `keys::keep_revoked`): a revocation the store's library
  lists, checked by the host against the revoked key itself or the plugin's pinned recovery key, and
  kept only if it holds (never taken off); one that does not hold is dropped with a log line, the
  batch's others stay. It answers nothing: a plugin whose key it revokes shows REG `problem`
  revoked.
- [D] **What the user sees is what installs.** The host copies the source into its own stage
  `plugins/.<id>.part` (outside every root a plugin reaches) while hashing it, runs all checks of
  5.1 and the trust rules on the stage, and only then **asks the user** (decision 132) in a
  host-drawn confirmation naming the asking plugin, the plugin's id, name, version, signer (or the
  key's fingerprint) and what it declares (`uses.*`, as the store page). On yes it swaps the stage
  it checked (journal, 5.4); a change to the source after the copy has no effect. Declined is reason
  2 (`ERR_DENIED`), dismissed reason 3 (`ERR_CANCELLED`); the stage is removed. Code plugins load at
  the next start (`install_done.flags` bit 1, `dev_native.pending`). While a guard is up the
  confirmation waits (`ui.md` 4.3).
- [D] **Grants for data plugins**: the user may allow one plugin to install data plugins (fonts,
  hyphenation, translations) without asking each time, the host grant `core.grant.install.<asking
  plugin>` (a host setting, off by default, REG-visible); code plugins always ask.
- [D] `remove_plugin` (`RemovePlugin`): removes an installed plugin (not one built into the image:
  reason 15); the host asks the user whether its Data folder goes too, default keep (decision 124,
  its safety rules: removal only of `.paperpaw/plugins/<id>/` resolved from a validated id, by a
  journalled rename into `.removing`). The plugin's instances are closed first; a loaded code
  plugin's flash room returns at the next start (`flags` bit 1). Its `install_done` (kind 2) comes
  only once the folder is renamed away and the plugin is gone from REG op 6 (event 70 raised), with
  `flags` bit 0 when the data was kept; declined or dismissed as for installs, and nothing is
  removed. `flags` reserved.
- [D] **One job at a time:** the host runs install, removal and firmware jobs one after another in
  submit order (one I/O buffer, 6.2). Firmware installs and code-plugin installs both write the code
  flash, so neither starts while the other runs; a request waits its turn (`dev_install.state` 0
  waiting), it is not refused.

### 5.3 Firmware

- [D] `install_firmware` (`FirmwareCommand::Install`, `InstallForeign`): an image on the card
  (`root` 0 or 1 with `uses.storage`); `flags` bit 0 allow an older or the same version, bit 1
  foreign (another firmware than PaperPaw, unsigned, checked as the boot loader would start it; the
  confirmation says so in its own words and that the way back is the web flasher).
- [D] **Keys** (update `page.rs` `Signer::Unknown`, `firmware.rs`): an image signed by a PaperPaw
  key installs with the normal confirmation. An image signed by another key (e.g. the user's own
  builds) installs only when `key` names exactly that key **and** the user accepts it in the host's
  confirmation, which shows the key's fingerprint and says it is not PaperPaw's; acceptance holds
  for this install only, as in interface 6; else reason 9 unknown key. Unsigned PaperPaw images are
  reason 8. A revoked key (reason 7) is a separate case: it never installs, acceptance or not;
  firmware has no revocation list today, so this reason is reserved for one.
- [D] **Bound to what the user saw:** the host checks the image (chip, size, version, signer) and
  hashes it, then **always asks the user** (decision 134, whatever grant there is). On yes it writes
  the other slot from its main loop in bounded steps of one 64 KiB erase block each (erase, then
  write; the card layer feeds the watchdog on each block it reads), hashing and checking the
  signature over the bytes as they are written, without reading the flash back. An image whose
  digest differs from the one asked about ends with reason 18 and the slot stays unbootable. The
  image's first word is written last, after the check passed, so a half-written slot never looks
  bootable; then the slot to start is chosen in one entry. `firmware_changed` reports progress,
  `install_done` the end. `firmware_restart` starts the installed image (asks nothing more: the user
  said yes to the install).
- [D] **Busy and pending:** a second firmware request while one runs waits its turn (5.2); one sent
  while an installed image waits for its restart (`dev_firmware.state` 3) ends with reason 21
  restart pending (`ERR_STATE`), a status of its own and not `ERR_BUSY`.
- [Q] (Firmware, owner device) the real time of one 64 KiB step and the card's read speed decide
  whether a progress redraw every 5 % stays smooth; a device-checklist item.

### 5.4 Power loss and card failure

- [D] **Plugins:** the swap writes a journal entry (`plugins/.journal`: the id, the stage path, the
  old folder's new name, the pins and rotations to take), synced, then renames old → `.old-<id>`,
  stage → `<id>`, takes the pins, removes `.old`; at start the host finishes or rolls back each
  journalled step, so the plugin is the old one or the new one with its matching pins, never a mix.
  Fault-injection tests cut after every write.
- [D] **Firmware:** the running slot is never written; a cut while writing leaves the old slot
  starting, since the new slot's first word is still missing. The new image runs as a trial: it is
  marked trial at its first start and becomes valid once it has run 30 s in total across starts; a
  start that ends uncleanly during the trial is undone at the next start, which boots the old image
  again (the firmware's own trial, not the boot loader's; `dev_firmware.undone`). A foreign image
  has no PaperPaw trial.
- [Q] (Firmware) confirm that a foreign image cannot be undone from PaperPaw's side and that the
  confirmation's wording covers it.
- [D] **Card:** a missing or failing card during a check or a copy ends with reason 16
  (`ERR_IO`) and nothing changes; a full card reason 17 (`ERR_NO_SPACE`).

### 5.5 Outcome reasons

One list for `dev_check.reason`, `dev_install.reason`, `dev_firmware.reason` and
`install_done.reason` (store `Refusal` and interface 6 `InstallFailure` map onto it one to one);
unknown values read as 1.

| Reason | Meaning | Status |
| --- | --- | --- |
| 0 | none (fits, or done) | `OK` |
| 1 | other | `ERR_IO` |
| 2, 3 | declined, dismissed | `ERR_DENIED`, `ERR_CANCELLED` |
| 4 | damaged, or not a file this firmware reads | `ERR_INVALID` |
| 5, 6, 7, 8, 9 | not the author, key changed, revoked, unsigned, unknown key not accepted | `ERR_DENIED` |
| 10, 11, 12 | needs a newer interface (`needs_draft`), built for an older one, another chip | `ERR_UNSUPPORTED` |
| 13 | no room in the code flash or the slot | `ERR_NO_SPACE` |
| 14 | older or the same version, not allowed | `ERR_UNSUPPORTED` |
| 15 | id taken (built in, or another source's) | `ERR_DENIED` |
| 16, 17 | card error, card full | `ERR_IO`, `ERR_NO_SPACE` |
| 18 | content changed since checked or asked | `ERR_STATE` |
| 19 | not on this device | `ERR_UNSUPPORTED` |
| 20 | the host stopped it (shutdown, card swap) | `ERR_CANCELLED` |
| 21 | an installed firmware waits for its restart | `ERR_STATE` |

## 6. Sizes and memory

### 6.1 Every record fits its limit

| Record or reply | At its caps | Limit |
| --- | --- | --- |
| `dev_device` | 1 + 66 + 66 + 34 + 4 + 8 + 4×4 + 8 + 4 + 4 = 211 | 4,096 |
| `dev_value` × 8 + `end` | 8 × (1 + 34 + 8 + 5) + 30 = 414 | 4,096 |
| `dev_firmware` | 1 + 66 + 34 + 4×4 + 8×3 + 4×2 + 4 = 153 | 4,096 |
| `dev_builtin` page | 1 + 66 + 34 + 5 = 106; 32 rows + `end` = 3,422 (96 built-ins: 3 pages) | 4,096 |
| `dev_native` | 1 + 4×3 + 34 + 4 + 66×2 + 4×2 + 8×2 = 207 | 4,096 |
| `dev_check` | 1 + 4 + 4 + 8 + 66 + 34 + 34 + 66 + 66 + 4 = 287 | 4,096 |
| `dev_install` | 1 + 4×6 + 8×2 + 66 = 107 | 4,096 |
| `dev_not_installed` × 32 + `end` | 32 × (1 + 66 + 4 + 5) + 30 = 2,462 | 4,096 |
| `dev_key` × 32 + `end` | 32 × (1 + 34 + 5) + 30 = 1,310 | 4,096 |
| `dev_network` × 32 + `end` | 32 × (1 + 34 + 8 + 5) + 30 = 1,566 | 4,096 |
| `dev_bonded` × 16 + `end` | 16 × (1 + 4 + 66 + 8 + 66 + 5) + 30 = 2,430 | 4,096 |
| `command.install_plugin` | 1 + 4 + 4 + 514 + 4 + 5 = 532 | 4,096 |
| install with full evidence | 532 + `install_author` (1 + 4 + 34×3 + 5 = 112) + 8 × `install_rotation` (1 + 4 + 34 + 66 + 5 = 110) = 1,524 | the 4,096-byte batch |
| `command.install_firmware` | 1 + 4 + 4 + 514 + 34 + 4 + 5 = 566 | 4,096 |
| `trust_revoked` × 16 | 16 × (1 + 66 + 34 + 66 + 5) = 2,752 | the 4,096-byte batch |
| `event.install_done` | 1 + 8 + 4×5 + 66 + 5 = 100 | the 1.1 KB event buffer |
| `event.bluetooth` | 1 + 8 + 4×2 + 66 + 4×3 + 4 = 99 | the 1.1 KB event buffer |

### 6.2 Memory (X4)

- Replies: on the host scratch, freed after the call; nothing kept per plugin.
- The memory probe's cached answer: 32 bytes.
- Requests: at most 4 install, removal or firmware requests (about 48 bytes each plus the kept
  `install_done`, 104 bytes) and 4 radio requests (about 64 bytes with the kept `wifi_failed`)
  device-wide, reserved at submit: about 0.9 KiB at most.
- Trust evidence of waiting requests: one fixed pool of 2 KiB device-wide (a full chain is 1,000
  bytes), taken at submit (`ERR_FULL` beyond); usually 0 or 100 bytes.
- One 4 KiB I/O buffer, shared by the copy, the hashing and signature check, and the firmware
  writer (today's OTA path uses one buffer for its verify and page work), allocated only while a
  job runs; jobs run one at a time (5.2).
- The confirmation: host-drawn over the screen with the host's own text, about 1 KiB while shown.
- **X4 worst case for DEV's host side: about 8 KiB** (I/O 4 + evidence pool 2 + confirmation 1 +
  requests 0.9) while an install runs with full evidence waiting; about 5 KiB for a firmware
  install; nothing when idle. All fallible: today's OTA buffer is an infallible `Box::new`
  (`updates.rs:532`), which Firmware makes fallible with DEV (section 9). An activation-gate item
  beside UIX, DOC and LIB.

## 7. Interface-6 calls and their interface-7 form

| Interface 6 | Interface 7 |
| --- | --- |
| `env.device()` (id, name, memory class), `env.caps()`, `env.catalog_device()`, `env.started()` | op 1 |
| `env.device().input` (`InputCaps.keys`, `Buttons::new`) | op 15; op 1 `caps` bits 0, 9, 14 |
| `env.status().value(name)` | op 2 |
| `env.firmware()` (`version`, `project`, `chip`, `room`, `undone`, `state`, `plugins`) | ops 3, 4; event 96 |
| `env.native()` (`safe_mode`, `not_installed`, code room, `file`), `fingerprint`, `toolchain` | ops 5, 14 (fingerprint and toolchain go, see below) |
| `store/trust.rs` `check_file`, `native::read_header`, `room.rs`, `verdict` | op 6; commands 1577-1578 |
| `keys::accept_rotation`, `keys::keep_revoked`, `revoked.rs` | commands 1578, 1579 (host-checked, taken at commit) |
| `env.memory().read()` | op 7 |
| `env.wifi()`, `Command::Wifi(..)`, `NoNetwork`, `WIFI_*`, `NETWORK_WANTED`/`IDLE` | ops 8, 9; commands 1561-1567; events 71, 92-95 |
| `Command::GoOnlineFor` | command 1568 (`GoOnline` not carried) |
| `env.power()` (`level`, `mode`, `automatic`, `stretch`, `online`, `every`), `Command::Wear`, `POWER_CHANGED` | op 12; command 1572; event 72 |
| `Command::Sleep`, `PowerOff`, `Restart` | commands 1569-1571 |
| `Command::InstallPlugin`, `LoadPlugin`, `install::adopt`, `Installer` | command 1573, event 97 |
| `Command::RemovePlugin` | command 1574, event 97 |
| `Command::SetPluginEnabled`, `env.plugins()`, `PLUGINS_CHANGED` / `REQUESTED` / `ANSWERED` | REG (commands 1543-1545, op 6, events 70, 77, 78) |
| `Command::Firmware(Install, InstallForeign, Restart)`, `FIRMWARE_CHANGED` | commands 1575, 1576; events 96, 97 |
| `env.bluetooth()`, `Command::Bluetooth(..)`, `BtEvent`, `BLUETOOTH_CHANGED` | ops 10, 11; commands 1580-1588; event 98 |
| `env.random()`, `env.secrets()`, `env.work()` | SEC, ASY |

- [D] **Fingerprint and toolchain are a deliberate change, not parity** (about `lib.rs:222-229`, the
  store's catalog and queue): about shows "plugin interface 7, draft N, <target>" from op 5 in their
  place, and the store selects catalog entries by `abi_major`, `abi_draft`, `target` and
  `code_file`. The store site's interface-7 catalog ships with the interface-7 firmware (a release
  gate with Web, in the release notes), so no device sees a catalog it cannot filter; Plugin changes
  the about labels and their en/de texts in the port.

## 8. What each device delivers

Firmware's table (`dev-family-228.md` section 8) holds; in short: the X4 has Wi-Fi (station and
access point, one TLS session), sleep, power off, firmware updates (two slots), the memory probe and
no Bluetooth (`ERR_NOT_FOUND` / ignored); the X4 Pro adds Bluetooth and touch but runs no native
plugins today; the desktop emulates; Android has no Wi-Fi, Bluetooth or firmware of PaperPaw's own
and restarts only. Plugins read `caps` and degrade, as in interface 6; requests end with reason 19
where a capability is missing.

## 9. Hand-offs (after approval)

- **Kernel:** records, `core.device@1` schema, the five features, REG's `answer_plugin` change, this
  document final.
- **Core:** the service over the kernel's device state, the command arms and feature checks, the
  request table with reservations and kept edges, the job queue, the stage-then-ask install with
  the trust rules moved from the store (pins, rotations, revocations taken at commit), the plugin
  install journal with fault-injection tests (with the uninstall data removal of decision 124), the
  host confirmation (with Plugin for the words), the grant setting, the power ordering after screen
  commands.
- **Firmware:** the X4 adapters (Wi-Fi, power, memory probe, values, keys), the firmware writer in
  64 KiB steps with digest binding and its trial, the fallible OTA buffer (`updates.rs:532`, offered
  by Firmware), the device checks with the owner (install confirmation, Wi-Fi join/host, sleep,
  power off, restart, step time and card read speed).
- **Services:** the SDK facade (typed reads, command builders, install request helpers with
  evidence).
- **Web:** the store catalog keyed by `abi_major`, `abi_draft`, `target`, shipped with the firmware
  (section 7).
- **Plugin:** the ports: power, wifi, bluetooth, diagnostics, plugin-manager, store, update,
  transfer, fonts-browser, setup, about, kosync, weather, settings, reading-goal. The store keeps
  fetching, its size bounds (catalog, pages, screenshots) and its queue; it hands evidence and
  revocations to the host instead of deciding.
