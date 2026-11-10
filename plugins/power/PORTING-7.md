# Power on interface 7

Status: **complete.** Native only, no interface-6 code left; the X4 file (`power`) packs at 49,068 B (format 5, interface 7.1, no frame refused).

Version 1.1.2. Native plugin `src/native_plugin.rs` (`NativePower`), decisions
without a host in `src/model.rs` (which mode the battery calls for, what the
power menu offers and does). Built-in descriptor `NATIVE_ENTRY` (feature
`linked`), declarations in `declarations.rs` built into the image by
`build.rs`, metadata in `native/` (regenerate with `python3 tools/abi-metadata.py
--schema abi/abi.def --input native/metadata.json --bytes native/metadata.bin
--json native/metadata.inspected.json`, likewise `metadata.host.*`).
The interface-6 plugin and its Kernel tests are gone.

## Ported

- The four settings (`power.mode`, `saving-below`, `offline-below`,
  `normal-while-charging`), group `power.group` (order 86), keys, titles, help,
  ranges and defaults unchanged (`testdata/declarations.bin.v1`).
- The mode in the quick panel (order 30), the leaf icons, the held Power key
  (`long.power` → `power.menu`), the action `power.open` that opens the power
  settings (handover `id = power.group` to `core.settings`).
- Automatic: start, resume, battery measured, charge changed and a change of a
  `power.*` setting read the settings, the battery snapshot and `core.device`
  op 12 and send `command.wear` only when the mode or its automatic flag
  differs. The 5-point margin, Normal while charging and the system saver are
  as before (`model` tests).
- The power menu is a host question (`core.ui` choice); its answer is delivered
  as `event.answered`. Sleep, switch off, restart, lock/unlock touch (the lock
  plugin's setting `lock.touch` and action `lock.touch-toggle`, only on a touch
  device when they exist). Sleeping and switching off put the sleep screen
  (slot `core.sleep`, parameter `off=1` for switching off) and the power command
  in one batch (`CommandSender::show_then`), so the host draws the frame before
  power is cut. Without a sleep screen the command acts alone.
- Declares `uses.power`; services `core.device`, `core.ui`.
- en/de texts unchanged.

## Differences from interface 6 (on purpose)

- The status item is answered by the stage `core.status` as text or a drawn
  value; interface 6 gave it an icon name (leaf, Wi-Fi crossed out). The SDK has
  no icon API for plugins, so the value is the mode's name (the existing keys
  `power.mode.saving`, `power.mode.offline`) — see "Not ported yet".
- The two-step mode lookup (`env.power()`, `env.status().battery`) is a read of
  `core.device` op 12 and of the snapshot; the "charging" test uses the snapshot
  (`ChargeChanged` wakes the plugin).

## Not ported yet (explicit)

- The status stage is answered (`StatusCall`/`StatusAnswer` from PR #335, stacked here
  until it merges), but the host does not call it yet (Core todo 76): the answer is only
  covered by the ignored acceptance test.
- The host does not admit menu entries, icons, key bindings and stage
  subscriptions yet (`native_host/declarations.rs`: `ERR_UNSUPPORTED`).
  `tests/declarations.rs` pins that these, and only these, are the records that wait.

## Tests

`src/model/tests.rs`: the decisions of the interface-6 `modes.rs`, `menu.rs` and
`off_screen.rs` that need no host. `tests/declarations.rs`: golden settings, UI and
event declarations, host validation. `tests/linked_entry.rs`. `tests/acceptance_pending.rs`:
ignored cases (end-to-end wear, menu, off screen, status, quick panel), each naming
what is missing. Peak memory on the X4 is not measured (device gate).
