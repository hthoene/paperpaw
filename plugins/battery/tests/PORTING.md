# Battery behavior baseline for interface 7

These tests exercise the current plugin through the public host Kernel. They
are acceptance fixtures for comparing a future port, not proof that a flat
callback dispatcher or a separately loaded Battery plugin works. Keep the
observable assertions when the shared SDK test host moves to the generated
table. Do not keep an alternative direct production backend for the tests.

## Existing coverage retained

- `view_with_kernel.rs`: start/sample/suspend log records, initially disabled
  logging, normal log rotation, unknown battery, charge/voltage, rounding,
  historical pace, button reach of all rows, hostile storage and network usage.
- `view.rs`: forward/back paging with long history and bounded log replacement
  when renaming fails.
- Local log tests and `log/hostile_tests.rs`: CSV/model parsing, bounded history,
  partial or invalid input and property-based round trips.

## Additional port contracts

- `port_lifecycle_contract.rs`: start-after-sleep and repeated-resume
  deduplication; sleep/resume/stop ordering and timestamps; runtime enable
  changes preserving the current awake state; typed page-event counting across
  sleep; periodic plugin-usage writes and the unconditional session-end write.
  CSV fields are found by public header names. Variable metering/timing fields
  are not golden data.
- `port_view_contract.rs`: localized screen/charge/unknown/optional-voltage
  observations and rendering checks. The plugin's OPEN action, screen
  construction and semantic output stay on the public host boundary.

The test helpers must not depend on private Battery state or invented wire
records. Lifecycle tests need no fonts; view tests use the existing font-pack
provider. Setup, input sequence and assertions should remain reusable when the
port's canonical host fixture is available. Tests deliberately preserve current
behavior, including logging being skipped before its awake state changes when
battery.enabled is false.

## Services still required by a complete port

The minimal callback proof (setting -> render -> event -> command) precedes
Battery. A complete Battery port must also retain all of the following:

- Typed settings with defaults, absence and wrong-type distinctions: battery
  enabled/by-plugin, core developer, dynamic network and saving settings.
- Refreshable battery/optional voltage, monotonic and wall clocks, global usage
  and per-plugin usage, metadata/name lookup and work-note queries.
- Scoped storage size/read_at/append/rename/remove/sync and plugin-root paths.
  Keep bounded log reading and rotation/error behavior; do not remove persistence
  to fit the initial host table.
- Versioned lifecycle/page/battery events, captured registration tokens, screen
  factory/instance lifetime and exactly-once teardown.
- Translation copy plus format/plural/number/digit and locale behavior, theme/UI
  inputs, font handles, clipped rendering and semantic descriptions.
- OPEN/Run commands, button/touch input, redraw and partial refresh, with explicit
  refusal and host-stamped command authority.

These contracts require published reviewed schema, callback dispatch and shared
facades. The tests do not authorize speculative codecs or raw ABI declarations.
The existing maximum-input graphics evidence and open device heap/stack gates
remain separate. No device, publication or production migration occurs here.
