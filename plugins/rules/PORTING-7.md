# Interface-7 port

Version 1.0.8. Native plugin: `src/native_plugin.rs` (`NativeRules`: events, list, dialogs,
choice of an action or setting), the file and the engine without the host in
`src/native_rules/` (`rule.rs`: triggers, effects, conditions, `rules.txt`, battery levels;
`engine.rs`: which rules an event sets off, the chain, the battery memory, the clock), the
registry reads in `src/native_data.rs`. `NATIVE_ENTRY` (feature `linked`), declarations in
`declarations.rs` built into the image by `build.rs` (screens, navigation, events), metadata in
`native/` (regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input
native/metadata.json --bytes native/metadata.bin --json native/metadata.inspected.json`,
likewise `metadata.host.*`).

## Ported

- Every trigger, effect and condition of interface 6, with the same words in `rules.txt`
  (`time 22:00 62`, `battery-at 80`, `switch-on <id>`, ...): version 1 files read byte for byte
  (`testdata/rules.txt.v1`), lines this version cannot read are written back as they were, a
  file that cannot be read is never saved over. The file is read once and kept; saving
  measures the file against the 64 KiB read limit first.
- The rules on the host's events: start, wake, sleep, book opened/closed/finished, battery
  and charging (`event.status_changed`: a level reached once until it fell clearly below), a
  switch turned on or off; rules set off by a setting another rule changed follow each other at
  most four deep. A switch or number that has the value is not set again.
- **Time of day and "every N minutes" follow the host's minute tick (`event.tick`)**, not
  scheduled work (interface 6's `Work::at`, which has no interface-7 form). A rule applies at
  the first tick of its minute (once per minute, whatever the tick count); "every N" counts ticks
  from the start. The local time is the snapshot's Unix time in the zone of `core.time-zone-rule`
  or `core.time-zone`; with neither, UTC (interface 6 also had the device's clock offset, which the
  snapshot does not carry). A tick missed while asleep is not made up.
- The list: a row a rule (when, then what, on or off), tap pauses, hold offers a condition or
  removal, *Add rule* asks when / what with the same dialogs and texts; the choice of an action,
  a switch or a number lists them by title.
- Texts, ids, menu entry (`core.settings-pages`, order 230), icon and en/de catalogs unchanged.

## Tests

Unit (`src/native_rules/tests.rs`): file round trips of every kind, version 1, unread and
damaged lines, the limit, allocator-failure sweeps of read, write and copy, the engine
(conditions, chain, battery levels, time and days, local time). The image holds what the builders make, and the host admits the screens
(`tests/abi7_declarations.rs`),
linked entry, construction in the kernel's real registered scope (`tests/hosted_construction.rs`:
a real pass; rules registers 9 events and the host's per-plugin cap is 32 since #871). The kernel tests that pinned the interface-6 plugin (`battery.rs`, `timing.rs`, `picking.rs`)
went with it; the engine's logic is pinned without the host above.

## Acceptance pending on the host

The native cases in `tests/acceptance_pending.rs` are refusals (never pass): battery level and
time of day (H1), pause (H1, H4), add and hold (H1, H2, H4).

## Not ported yet (explicit)

- The choice of an action or setting is a flat list by title; interface 6 grouped long lists
  by owner.
- The paused state is a word (on/off) in the row, not a switch row (the kit has none).
- Views and dialog state that call the host have no allocator-failure sweep (no mock host).
- The interface-6 plugin is removed: `Rules`, `rule.rs`, `screens.rs`, the kernel tests, the app's
  `offered(.., Rules)` wiring and `STORE_PLUGINS` entry, and the plugin-file build (`tools/plugin-files`
  bin, `plugins.txt`). Rules do not run in the app until the host activates native plugins.
- `core.ui` opens in the first callback; make has no frame. Peak memory on the X4 is not measured.
