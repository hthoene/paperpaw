# Interface 7 family DEV: device, power, radios, installing, firmware (draft)

Contract draft for #228 (assignment #1025). Items marked [D] are proposed decisions, [Q] open
questions for Plugin, Firmware and Senior. Inputs: Firmware's DEV inventory and draft (#931,
`/home/agent/shared/dev-family-228.md`, whose sections 1, 2 and 8 are taken as the inventory and
whose open questions section 9 settles), the plugin needs inventory (#984, DEV section), the
C-family proposals (#890 2.12), `registry.md` (plugin rows, `set_plugin_enabled`, `request_plugins`
/
`answer_plugin`), `core-events.md` (events 70-72), `ui.md` (host-drawn confirmations, the guard) and
owner decisions 124, 132 and 134. Doc-only; records follow once approved. Tags are proposed and
numbered without gaps at the records step.

## 0. Scope and principles

- [D] **Reads are a service, changes are commands, outcomes are events.** The host service
  `core.device` (section 2) answers what the device is and how it stands; commands in the slot-15
  growth block (section 3) ask for changes, applied after the callback; events (section 4) say what
  came of them, after which a plugin reads again. Interface 6 worked this way already
  (`Command::Wifi` → `WIFI_CHANGED` → `env.wifi()`); interface 6's `Env` accessors have no copies.
- [D] **Consent is a declared feature per area** (signed metadata, as `uses.storage`): `uses.wifi`
  (the radio and network policy), `uses.power` (sleep, off, restart, power modes), `uses.install`
  (installing and removing plugins), `uses.firmware` (installing firmware), `uses.bluetooth`. Reads
  need none. A command whose feature is not declared refuses the whole batch with `ERR_DENIED` at
  submit. Separate keys tell the user more on the store page than one `uses.device` (Firmware's Q6).
  First-party plugins that sent these commands under `Uses::NOTHING` (diagnostics, power, settings,
  plugin-manager, reading-goal) declare them in their ports.
- [D] **The host decides trust.** Whether a plugin or a firmware image fits, is signed, is revoked
  and has room is decided by the host's own checks (the loader's), never by the asking plugin; the
  store's `trust.rs` verdict becomes advice shown to the user. **Every plugin-triggered install asks
  the user** (decision 132) and **every firmware change asks the user** (decision 134), in a
  host-drawn confirmation no plugin draws, covers or answers.
- [D] **Interrupted changes never activate half a state** (inventory rule 2): plugin installs are
  staged and swapped by rename with a journal, firmware goes to the other slot and is verified
  before the switch, and a start after a cut finishes or rolls back (section 5).
- [D] **Enabled, available and running are different states** (rule 3): REG op 6 rows
  (`reg_plugin.state`, `problem`) keep them apart; DEV adds no second inventory.
- [D] **Portable batches:** a well-formed command for a capability the device lacks (Wi-Fi on
  Android, firmware on the desktop) is accepted and ignored with a log line (interface 6 "ignored
  elsewhere"), so one missing capability never drops a batch's other records; plugins gate on
  `dev_device.caps`. Feature checks are not capability checks: they refuse.
- Out of DEV: the plugin inventory and enabling (REG), stored secrets and randomness (SEC), HTTP and
  sockets (NET, SOCK), scheduled work (ASY), the battery and display snapshot (slot 4, done). Not
  carried for 1.0 (no first-party user; additive later): Bluetooth GATT and advertising, the
  developer link, wake holds, USB drive mode (#32), screenshots.

## 1. Features

| Key | Allows | Plugins (first-party) |
| --- | --- | --- |
| `uses.wifi` | commands 1561-1568 | wifi, transfer, diagnostics |
| `uses.power` | commands 1569-1572 | power, diagnostics, settings, plugin-manager, reading-goal, store, update |
| `uses.install` | commands 1573-1574; REG `answer_plugin` | store, fonts-browser, plugin-manager, update |
| `uses.firmware` | commands 1575-1576 | update |
| `uses.bluetooth` | commands 1577-1585 | bluetooth |

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
| 13 | `install` | `dev_request` | `dev_install` | state of an install or firmware request |

| Tag | Record | Fields |
| --- | --- | --- |
| 1092 | `service.dev_device` | `id: str 64`, `model: str 64`, `catalog: str 32`, `class: u32`, `caps: u64`, `controls: u32`, `tls_sessions: u32`, `start: u32`, `slept_ms: u64`, `flags: u32` |
| 1093 | `service.dev_value` | `name: str 32`, `value: i64` |
| 1094 | `service.dev_firmware` | `version: str 64`, `project: str 32`, `chip: u32`, `room: u64`, `undone: u32`, `state: u32`, `done: u64`, `total: u64`, `failure: u32`, `request: u32` |
| 1095 | `service.dev_builtin` | `id: str 64`, `version: str 32` |
| 1096 | `service.dev_native` | `abi_major: u32`, `abi_draft: u32`, `target: u32`, `code_file: str 32`, `safe_mode: u32`, `safe_mode_plugin: str 64`, `not_installed: u32`, `room_free: u64`, `room_largest: u64` |
| 1097 | `service.dev_check_ask` | `root: u32`, `path: str 512`, `kind: u32` |
| 1098 | `service.dev_check` | `verdict: u32`, `needs_draft: u32`, `bytes: u64`, `signer: str 64`, `id: str 64`, `version: str 64`, `flags: u32` |
| 1099 | `service.dev_memory` | `heap_free: u32`, `heap_size: u32`, `largest: u32`, `stack_free: u32`, `stack_size: u32`, `external_free: u32`, `external_size: u32`, `flags: u32` |
| 1100 | `service.dev_wifi` | `state: u32`, `ssid: str 32`, `address: str 45`, `host_name: str 63`, `guests: u32`, `flags: u32` |
| 1101 | `service.dev_network` | `ssid: str 32`, `signal: i32`, `flags: u32` |
| 1102 | `service.dev_bluetooth` | `radio: u32`, `max_links: u32`, `flags: u32` |
| 1103 | `service.dev_bonded` | `device: u32`, `name: str 64`, `kind: u32`, `flags: u32`, `given: str 64` |
| 1104 | `service.dev_interval` | `base_ms: u32` |
| 1105 | `service.dev_power` | `level: u32`, `mode: str 128`, `flags: u32`, `stretch: u32`, `every_ms: u32` |
| 1106 | `service.dev_request` | `request: u32` |
| 1107 | `service.dev_install` | `request: u32`, `kind: u32`, `state: u32`, `status: i32`, `done: u64`, `total: u64`, `id: str 64` |

- [D] `dev_device` (interface 6 `env.device()`, `caps()`, `catalog_device()`): `id` the stable
  device id (`xteink-x4`), `model` its name, `catalog` the store's device name (`x4`, empty for
  none), `class` the memory class (0 small, 1 medium, 2 large), `caps` bit 0 touch, 1 network, 2
  Wi-Fi managed by PaperPaw, 3 Bluetooth, 4 firmware updates, 5 speech, 6 sleep, 7 rotation, 8
  colours, 9 button row, 10 system clipboard, 11 frontlight, 12 black and white, 13 power off
  (unknown bits ignored, new ones appended); `controls` the power controls (bit 0 radio, 1 CPU
  clock, 2 buttons, 3 frontlight, 4 system saver); `start` 0 unknown, 1 power on, 2 woke, 3 restart,
  4 after a fault; `flags` bit 0 `slept_ms` known. The user's device name is the setting
  `core.device-name` (slot 5).
- [D] `dev_value` (interface 6 `env.status().value(name)`, diagnostics only; Firmware's Q3): named
  values (`usb-pin-high`, `usb-frames`, `frontlight-percent`, `temperature-c`), at most 8, names
  `[a-z0-9-]{1,32}`; a build may add names without a new interface.
- [D] `dev_firmware` (interface 6 `env.firmware()`): `state` 0 idle, 1 waiting for the user, 2
  installing, 3 installed (restart to use), 4 failed; `failure` 0 none, 1 declined, 2 cancelled, 3
  damaged, 4 unsigned or revoked, 5 wrong chip, 6 older (not allowed), 7 no room, 8 card error;
  `request` the request it belongs to. No public keys are shown: verifying is the host's.
- [D] `dev_native` (interface 6 `env.native()`): `abi_major` 7, `abi_draft` the interface draft,
  `target` the chip id (5 ESP32-C3), `code_file` (`code.ppn`), `safe_mode` 0 or a reason,
  `safe_mode_plugin` the plugin that caused it, `not_installed` how many plugins wait for a restart,
  `room_free`/`room_largest` the code flash. Interface 6's `fingerprint` and `toolchain` go: the
  catalog filter uses `abi_major`, `abi_draft`, `target` and `code_file` (a release dependency of
  the store site, Firmware's Q9).
- [D] `dev_memory` (interface 6 `env.memory()`): a probe walks the stack on the X4, so the host does
  one real probe per 250 ms and answers the cached one inside that window (`flags` bit 0 cached).
- [D] `dev_wifi` (`state` 0 off, 1 on, 2 joining, 3 online, 4 hosting; `flags` bit 0 scanning, 1
  wanted, 2 busy), `dev_network` (`flags` bit 0 secured, 1 known: a password is stored, SEC),
  `dev_bluetooth`, `dev_bonded` (`flags` bit 0 connected, 1 remote keys on; `given` the plugin a
  sensor is handed to, shown only to the plugin bound to the host slot `core.bluetooth`, else
  empty).
- [D] `dev_power` (interface 6 `env.power()`): `level` 0 normal, 1 saving, 2 offline; `mode` the
  worn power mode; `flags` bit 0 automatic, bit 1 online: plugins may use the network for their own
  work now (interface 6 `online()`); `every_ms` the interval for `dev_interval.base_ms` stretched by
  the level (interface 6 `every(base)`; 0 for no base).

## 3. Commands (slot 15, growth block)

| Tag | Record | Fields | Feature |
| --- | --- | --- | --- |
| 1561 | `command.wifi_on` | (none) | `uses.wifi` |
| 1562 | `command.wifi_off` | (none) | `uses.wifi` |
| 1563 | `command.wifi_scan` | (none) | `uses.wifi` |
| 1564 | `command.wifi_join` | `ssid: str 32`, `password: str 63`, `flags: u32` | `uses.wifi` |
| 1565 | `command.wifi_leave` | (none) | `uses.wifi` |
| 1566 | `command.wifi_host` | `ssid: str 32`, `password: str 63` | `uses.wifi` |
| 1567 | `command.network_none` | (none) | `uses.wifi` |
| 1568 | `command.go_online` | `for_ms: u32` | `uses.wifi` |
| 1569 | `command.sleep` | (none) | `uses.power` |
| 1570 | `command.power_off` | (none) | `uses.power` |
| 1571 | `command.restart` | (none) | `uses.power` |
| 1572 | `command.wear` | `mode: str 128`, `flags: u32` | `uses.power` |
| 1573 | `command.install_plugin` | `request: u32`, `root: u32`, `path: str 512`, `flags: u32` | `uses.install` |
| 1574 | `command.remove_plugin` | `plugin: str 64`, `flags: u32` | `uses.install` |
| 1575 | `command.install_firmware` | `request: u32`, `root: u32`, `path: str 512`, `flags: u32` | `uses.firmware` |
| 1576 | `command.firmware_restart` | (none) | `uses.firmware` |
| 1577-1585 | `command.bt_scan` (`lasting_ms`), `bt_stop_scan`, `bt_pair`, `bt_connect`, `bt_disconnect`, `bt_forget` (`device`), `bt_give` (`device`, `plugin: str 64`), `bt_take_back` (`device`), `bt_keys` (`device`, `on`) | as named, `device: u32` | `uses.bluetooth` |

- [D] One record per action (Firmware's Q2): tags are plentiful in the growth block, each record is
  checked by the generated codec without a per-action table, and a later action is a new record.
- [D] **Wi-Fi** (interface 6 `WifiCommand`, `NoNetwork`): `join` and `host` need a non-empty `ssid`;
  `host`'s password is 8-63 bytes, `join`'s 0-63; `join.flags` bit 0 remember the password (SEC's
  store, owned by the Wi-Fi plugin). A password is never logged nor shown in a record any other
  plugin reads. `go_online` (`for_ms` 1-600,000) is honoured only within 2,000 ms after a user input
  (else `OK` and ignored, logged): no plugin turns the radio on in the background by it.
- [D] **Power** (`Sleep`, `PowerOff`, `Restart`, `Wear`): act after the frame is shown; `restart`
  saves what plugins keep first; `power_off` delivers `suspend` first; `wear.mode` a declared power
  mode (`ERR_NOT_FOUND`), `flags` bit 0 automatic. While an instance guards (`ui.md` 4.3) only the
  guard's plugin and the host may restart or switch off (`ERR_DENIED`); sleep stays open to all.
- [D] Batch limits: at most one power command (1569-1572) and one firmware command per batch, at
  most two Wi-Fi commands (`ERR_INVALID` beyond).
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
| 93 | `event.wifi_failed` | `now_ms: u64`, `ssid: str 32`, `reason: u32` |
| 94 | `event.network_wanted` | `now_ms: u64` |
| 95 | `event.network_idle` | `now_ms: u64` |
| 96 | `event.firmware_changed` | `now_ms: u64`, `request: u32`, `state: u32` |
| 97 | `event.install_done` | `now_ms: u64`, `request: u32`, `kind: u32`, `status: i32`, `id: str 64` |
| 98 | `event.bluetooth` | `now_ms: u64`, `kind: u32`, `device: u32`, `name: str 64`, `device_kind: u32`, `signal: i32`, `reason: u32` |

- [D] 92, 94-96 are hints (coalesced per kind; read again with section 2). 93 (`reason` 0 other, 1
  not found, 2 password, 3 timeout; unknown as 0) and 97 are **edges kept like answers** (`ui.md`
  3.2): each failed network and each install outcome reaches its plugin, the store being full never
  drops one. 97 and 96 go to the requesting plugin only; 98 to the plugin bound to `core.bluetooth`.
  `firmware_changed` comes at most every 250 ms while installing.

## 5. Installing plugins and firmware

### 5.1 Checking before asking

- [D] Op 6 `check` (`dev_check_ask`: `root` 0 the caller's Data or 1 with `uses.storage`, `path` a
  `.ppn` file, a plugin folder or a firmware image, `kind` 0 plugin, 1 firmware; Firmware's Q4) runs
  the loader's own checks without changing anything: `verdict` 0 fits, 1 needs a newer interface
  (`needs_draft`), 2 built for an older interface, 3 another chip, 4 no room, 5 damaged, 6 signed by
  a revoked or unknown key, 7 unsigned where the user requires signatures, 8 older firmware than
  installed; `bytes`, `signer` (the key's name), `id`, `version`, `flags` bit 0 data plugin (no
  code). The store and update show it before they ask; they never parse `.ppn` headers themselves.

### 5.2 Plugins

- [D] `install_plugin` (interface 6 `InstallPlugin`, `LoadPlugin`, `install::adopt`): the plugin
  downloaded a `.ppn` or a folder into `path` (its root 0, or root 1 with `uses.storage`); `request`
  its own number for the outcome; `flags` bit 0 replace an installed plugin of the same id (an
  update), bit 1 allow an older version. The host checks it (5.1), then **asks the user** (decision
  132) in a host-drawn confirmation naming the asking plugin, the plugin's id, name, version, signer
  and what it declares (`uses.*`, as the store page); declined or dismissed is `install_done` status
  `ERR_DENIED` or `ERR_CANCELLED`. On yes it copies into `plugins/.<id>.part`, syncs, verifies
  again, then swaps (journal, 5.4); the outcome is `install_done`; code plugins load at the next
  start (`dev_native.not_installed`). While a guard is up the confirmation waits (`ui.md` 4.3).
- [D] **Grants for data plugins**: the user may allow one plugin to install data plugins (fonts,
  hyphenation, translations) without asking each time, the host grant `core.grant.install.<asking
  plugin>` (a host setting, off by default, REG-visible); code plugins always ask.
- [D] `remove_plugin` (`RemovePlugin`): removes an installed plugin (not one built into the image:
  `ERR_DENIED`); the host asks the user whether its Data folder goes too, default keep (decision
  124, its safety rules: removal only of `.paperpaw/plugins/<id>/` resolved from a validated id, by
  a journalled rename into `.removing`). `flags` reserved.

### 5.3 Firmware

- [D] `install_firmware` (`FirmwareCommand::Install`, `InstallForeign`): an image on the card
  (`root` 0 or 1 with `uses.storage`); `flags` bit 0 allow an older version, bit 1 foreign (another
  firmware than PaperPaw: the confirmation says so in its own words). The host checks it (signature
  for PaperPaw images, chip, size, version), then **always asks the user** (decision 134, even with
  any grant), then writes the other slot in bounded steps from its main loop (one flash page per
  step, the watchdog fed), verifies the written slot, and only then marks it to boot;
  `firmware_changed` reports progress, `install_done` the end. `firmware_restart` starts the
  installed image (asks nothing more: the user said yes to the install). At most one firmware
  request at a time (`ERR_BUSY`).

### 5.4 Power loss and card failure

- [D] **Plugins:** the swap writes a journal entry (`plugins/.journal`: the id, the stage path, the
  old folder's new name), synced, then renames old → `.old-<id>`, stage → `<id>`, removes `.old`;
  at start the host finishes or rolls back each journalled step, so the plugin is the old one or the
  new one, never a mix. Fault-injection tests cut after every write.
- [D] **Firmware:** the running slot is never written; a cut while writing leaves the old slot
  booting; the new slot boots once and confirms itself after the first good start (the boot loader's
  rollback, `dev_firmware.undone`), else the old one comes back.
- [D] **Card:** a missing or failing card during a check or a copy is `install_done` `ERR_IO` and
  nothing changes; a full card `ERR_NO_SPACE`.

## 6. Sizes and memory

### 6.1 Every record fits its limit

| Record or reply | At its caps | Limit |
| --- | --- | --- |
| `dev_device` | 1 + 66 + 66 + 34 + 4 + 8 + 4×4 + 8 + 4 + 4 = 211 | 4,096 |
| `dev_value` × 8 + `end` | 8 × (1 + 34 + 8 + 5) + 30 = 414 | 4,096 |
| `dev_firmware` | 1 + 66 + 34 + 4×4 + 8×3 + 4×2 + 4 = 153 | 4,096 |
| `dev_builtin` page | 1 + 66 + 34 + 5 = 106; 32 rows + `end` = 3,422 (96 built-ins: 3 pages) | 4,096 |
| `dev_network` × 32 + `end` | 32 × (1 + 34 + 8 + 5) + 30 = 1,566 | 4,096 |
| `dev_check` | 1 + 4 + 8 + 66 + 66 + 66 + 4 + 4 = 219 | 4,096 |
| `dev_bonded` × 16 + `end` | 16 × (1 + 4 + 66 + 8 + 66 + 5) + 30 = 2,430 | 4,096 |
| `command.install_plugin` | 1 + 4 + 4 + 514 + 4 + 5 = 532 | 4,096 |
| `event.bluetooth` | 1 + 8 + 4×2 + 66 + 4×3 + 4 = 99 | the 1.1 KB event buffer |

### 6.2 Memory (X4)

- Replies: on the host scratch, freed after the call; nothing kept per plugin.
- The memory probe's cached answer: 32 bytes.
- Requests: at most 4 install or firmware requests waiting device-wide (`ERR_FULL`), about 48 bytes
  each plus the kept `install_done` edge (64 bytes).
- Verifying: one 4 KiB buffer for hashing and the signature check, during a check or a copy only.
- The confirmation: host-drawn over the screen with the host's own text, about 1 KiB while shown.
- Firmware writes: one flash page buffer (4 KiB) while installing (the existing OTA path).
- **X4 worst case for DEV's host side: about 9.5 KiB while a firmware install runs** (verify 4 +
  page 4 + confirmation 1 + requests 0.5), all fallible, nothing when idle; an activation-gate item
  beside UIX, DOC and LIB.

## 7. Interface-6 calls and their interface-7 form

| Interface 6 | Interface 7 |
| --- | --- |
| `env.device()` (id, name, memory class), `env.caps()`, `env.catalog_device()`, `env.started()` | op 1 |
| `env.status().value(name)` | op 2 |
| `env.firmware()` (`version`, `project`, `chip`, `room`, `undone`, `state`, `plugins`) | ops 3, 4; event 96 |
| `env.native()` (`safe_mode`, `not_installed`, code room, `file`), `fingerprint`, `toolchain` | op 5 (fingerprint and toolchain go) |
| `store/trust.rs` `check_file`, `native::read_header`, `room.rs` | op 6 |
| `env.memory().read()` | op 7 |
| `env.wifi()`, `Command::Wifi(..)`, `NoNetwork`, `WIFI_*`, `NETWORK_WANTED`/`IDLE` | ops 8, 9; commands 1561-1567; events 71, 92-95 |
| `Command::GoOnlineFor` | command 1568 (`GoOnline` not carried) |
| `env.power()` (`level`, `mode`, `automatic`, `stretch`, `online`, `every`), `Command::Wear`, `POWER_CHANGED` | op 12; command 1572; event 72 |
| `Command::Sleep`, `PowerOff`, `Restart` | commands 1569-1571 |
| `Command::InstallPlugin`, `LoadPlugin`, `install::adopt`, `Installer` | command 1573, event 97 |
| `Command::RemovePlugin` | command 1574 |
| `Command::SetPluginEnabled`, `env.plugins()`, `PLUGINS_CHANGED` / `REQUESTED` / `ANSWERED` | REG (commands 1543-1545, op 6, events 70, 77, 78) |
| `Command::Firmware(Install, InstallForeign, Restart)`, `FIRMWARE_CHANGED` | commands 1575, 1576; events 96, 97 |
| `env.bluetooth()`, `Command::Bluetooth(..)`, `BtEvent`, `BLUETOOTH_CHANGED` | ops 10, 11; commands 1577-1585; event 98 |
| `env.random()`, `env.secrets()`, `env.work()` | SEC, ASY |

## 8. What each device delivers

Firmware's table (`dev-family-228.md` section 8) holds; in short: the X4 has Wi-Fi (station and
access point, one TLS session), sleep, power off, firmware updates (two slots), the memory probe and
no Bluetooth (`ERR_NOT_FOUND` / ignored); the X4 Pro adds Bluetooth and touch but runs no native
plugins today; the desktop emulates; Android has no Wi-Fi, Bluetooth or firmware of PaperPaw's own
and restarts only. Plugins read `caps` and degrade, as in interface 6.

## 9. Hand-offs (after approval)

- **Kernel:** records, `core.device@1` schema, the five features, REG's `answer_plugin` change, this
  document final.
- **Core:** the service over the kernel's device state, the command arms and feature checks, the
  request table and kept edges, the plugin install journal with fault-injection tests (with the
  uninstall data removal of decision 124), the host confirmation (with Plugin for the words), the
  grant setting.
- **Firmware:** the X4 adapters (Wi-Fi, power, memory probe, values), the firmware writer in bounded
  steps and its rollback, the device checks with the owner (install confirmation, Wi-Fi join/host,
  sleep, power off, restart).
- **Services:** the SDK facade (typed reads, command builders, install request helpers).
- **Web:** the store catalog keyed by `abi_major`, `abi_draft`, `target` instead of the interface-6
  fingerprint.
- **Plugin:** the ports: power, wifi, bluetooth, diagnostics, plugin-manager, store, update,
  transfer, fonts-browser, setup, about, kosync, weather, settings, reading-goal.
