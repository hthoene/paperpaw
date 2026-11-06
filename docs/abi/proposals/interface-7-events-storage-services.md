# Interface 7: events, storage, services and text contracts for the Battery port

Status: DRAFT proposal for the Kernel Engineer to review and own (#228, #745).
Nothing here is decided or implemented. Base: `origin/abi7/integration` 23086bb9.

Conventions. **[V]** is a verified fact with a `path:line` citation (lines on the
base above unless a ref is named). **[P]** is a proposal. **[Q]** is an open
question. Plugin-side facts come from `origin/samples/abi7-snapshot-settings-734`
(`plugins/battery`, `plugins/battery/tests/PORTING.md`). ADR 0034 is read from
`origin/fw/adr-flat-boundary-228:docs/adr/0034-stable-plugin-boundary.md` (cited
as ADR34:line).

Out of scope: declarative registration (manifest, translations, setting specs,
resources, actions, menus, icons, screens) is in
`docs/abi/proposals/declarative-registration.md` (another document, referenced by
name only). The slot 15 `command.set_setting` record (tag 4) is in progress on
Kernel's commands branch; its `docs/abi/commands.md` was not reachable from this
worktree (`origin/kernel/abi7-scoped-commands-228` does not exist on origin when
checked), so its content is taken from the coordinator's message only.

## 0. Shared rules used by all four sections

Verified starting point:

- [V] 20 host slots exist, 0 to 19 (`abi/abi.def`): heap 0-2, clock 3, env.snapshot 4,
  settings.read 5, i18n.copy 6, font.acquire 7, handle.release 8, canvas 9-12,
  text.measure 13, text.draw 14, commands.submit 15, registrations.submit 16,
  services.query 17, blob.read 18, blob.release 19.
- [V] Argument types are closed: `context pointer bytes buffer handle usize u16 u32
  u64 i32 f32 f64 out_ptr out_u64 out_handle out_f32` (`tools/abi-gen.py:21`). There is
  no `i64`, `u8` or `bool` argument. Wire field types are `u32 u64 i32 str bytes`
  (`tools/abi-gen.py:27`): no `i64` in records either. Each entry has one
  `max_bytes` (`abi.def`, `abi-gen.py:123`).
- [V] The record codec generator accepts only the three initial shapes
  (`registration.event`, `registration.draw`, `event.tick`):
  `tools/abi_records.py:10-14,50` ("unsupported initial record fields"). Every record
  proposed below needs generator work (str fields, more shapes, more tags).
- [V] Batch grammar: LEB128 count, then records `tag, len, body`; body = bitmap length,
  bitmap, positional fixed-width little-endian scalars; cap 4096 B and 8 records
  (`docs/abi/records.md`, `REGISTRATION_MAX_RECORDS`
  `crates/paperpaw-core/src/native_host/registrations.rs:17`).
- [V] Statuses: OK 0 and `ERR_*` -1..-9 (`abi/abi.def` `statuses`). The set is open and
  appended (`abi/README.md`).
- [V] The host table is append-only; a new slot is added at the end with `since` set
  to the unreleased minor 1 (`abi/README.md`, "Multiple additions may use the same
  unreleased minor"). ADR34:~"Entries are only appended".

Proposed shared rules **[P]**:

1. **Record tags.** Tags 1-3 exist, 4 is `command.set_setting`, 5-31 belong to the
   declarative-registration document. This document uses **32-63** only:
   events 32-43, i18n argument records 44-47, service response records 48-55,
   command records 56-62, 63 reserved. Tags are never reused.
2. **Event header.** Every event record's first required field is `now_ms: u64`
   (monotonic milliseconds at emission, same clock as `env.snapshot.monotonic_ms`),
   matching `event.tick {now_ms}` (`abi.def` record 3).
3. **New statuses** (appended): `ERR_NOT_FOUND -10` (no such file or key) and
   `ERR_FULL -11` (medium full or free-space reserve reached). Reason: `ERR_NO_SPACE`
   already means "output too small / batch too large" (`docs/abi/records.md`), so reusing
   it for a full card would be ambiguous. Mapping from the SDK's `StorageError`
   (`crates/paperpaw-platform/src/storage.rs:17-38`): NotFound to NOT_FOUND,
   InvalidPath/InvalidData to INVALID, Unsupported to UNSUPPORTED, Full to FULL,
   Io/TooLarge/Other(_) to IO (`CARD_GONE` is `Other(1)`, storage.rs:45; **[Q]** whether
   it deserves its own status).
4. **Validation before effects.** Every slot validates all pointers, lengths, UTF-8,
   path grammar, caps, owner and root before the first side effect; a refused call
   leaves output buffers untouched (as `docs/abi/snapshot.md` and `abi/settings.md` already
   require).
5. **No panics, no unbounded allocation.** Every variable-length input has a cap below;
   host scratch is stack or fixed.
6. **Closed schemas.** Unknown record tags in a position that requires support return
   `ERR_UNSUPPORTED`; only bounded appended optional fields extend a record
   (`docs/abi/records.md`).

## 1. Event payload grammar

### 1.1 Facts

- [V] Interface 6 Battery subscribes to six events: `core.started`, `core.resume`,
  `core.suspend`, `core.stopping` (each ends or starts an "awake" session),
  `core.battery-measured` (the sample) and `core.page-turned` (a counter)
  (`plugins/battery/src/lib.rs:184-211` on the 734 ref). Only page-turned has a payload
  and Battery ignores it.
- [V] The SDK has 60 built-in events (`grep -c "^pub const .*: Event<"
  crates/paperpaw-sdk/src/builtin/events.rs`). `Reading` (path, title, `progress: f32`,
  section, offset, chapter, optional page/pages) is the payload of page-turned and
  three more (`events.rs:7-27`, `:574`). A float cannot be a record field (ADR34 records
  section "no floats in a record").
- [V] `BATTERY_MEASURED` is documented as "about once a minute where the device
  measures it" (`events.rs:742-745`) and is emitted by `Kernel::set_status` whenever the
  shell passes a status that has a battery (`crates/paperpaw-core/src/lib.rs:785-787`).
  `Kernel::tick(now)` is a different thing: it advances time when a requested wake is due
  (`lib.rs:808`) and has no fixed period.
- [V] `STARTED` then `RESUME` are both emitted at a start that ends a sleep
  (`events.rs:726-730`); Battery relies on getting both and deduplicates with its own
  `awake` flag (`lib.rs:103-107`; test `port_lifecycle_contract.rs:94-98`).
  `SUSPEND` is "a moment to save"; `STOPPING` may be followed by power loss
  (`events.rs:722-739`).
- [V] The only event record is `event.tick {now_ms: u64}` (tag 3); the host admits only
  the tick tag in registrations (`native_host/registrations.rs:55-64`); the plugin table has
  `dispatch_event(state, token, input: bytes) -> i32`, where positive results are
  invalid (`abi/callbacks.md`); no event is delivered yet ("defines no event payload
  grammar", `abi/callbacks.md`).
- [V] Registration is `registration.event {event_tag: u32, token: u32}`; for tick,
  `event_tag` is the record tag (3) (`docs/abi/records.md`).

### 1.2 Decisions (proposed)

**D1.1 Battery's sample is NOT driven by tick; a separate event is kept.** [P]
`event.battery_measured` stays. Rationale: (a) the event means "a new measurement
exists", and the host already gates it on a battery being present; a tick-driven sample
would re-read an unchanged snapshot (the snapshot only changes when the shell calls
`set_status`) and would write rows for devices without a battery unless the plugin
re-checks the presence bit; (b) the plugin cannot ask for a 60 s tick (no timer
registration exists in slots 15/16) and ticks have no fixed rate; making ticks periodic
would be a new kernel guarantee that costs wake-ups on a device where each wake costs
battery; (c) behaviour parity with `view_with_kernel.rs` tests that drive the log through
`BATTERY_MEASURED`. `event.tick` stays for plugins that need time (the pilot sample).
**[Q]** Cadence on the X4 and desktop shells was not verified (the shell sources are
not in this worktree).

**D1.2 Lifecycle and page-turn are event kinds, not a services query.** [P] A query
returns state; Battery needs the edges (`suspend`/`stopping` must write the "end" line
and sync before power is cut). They are delivered synchronously inside the shell's
suspend/stop call, so a handler's storage calls complete before power-off (**[Q]** the
shells' guarantee that nothing cuts power until the dispatch returns was not verified;
Battery's existing data-loss window depends on it).

**D1.3 Input is NOT an event.** [P] Key, pointer, gesture and action input goes to the
top screen and the screen answers `Handled` (`crates/paperpaw-sdk/src/screen.rs:568-572,
670,712`). `dispatch_event` can only return zero or a negative status
(`abi/callbacks.md`), so it cannot say consumed/not consumed. Input therefore belongs in
the screen callback table (ADR34 section 4: `on_key`, `on_pointer`, gesture recognisers
as direct callbacks), which is not declared yet (only `dispatch_event` and `draw` exist in
`PpPlugin`). Battery's view needs `on_action(action id string)` and `on_gesture(kind,
start point)` (`view.rs:430-466` on the 734 ref). **[Q1]** how an action id (a string) is
delivered; sketch: `on_action(state, token, id: bytes) -> i32` with a handled flag in an
out value. No key/touch records are proposed here.

### 1.3 Records

All fields required, positional, little-endian, `now_ms` first (rule 2). Tags per rule 1.

| Tag | Record | Fields in order | max_bytes (str) |
| --- | --- | --- | --- |
| 32 | `event.started` | `now_ms: u64` | - |
| 33 | `event.resume` | `now_ms: u64` | - |
| 34 | `event.suspend` | `now_ms: u64` | - |
| 35 | `event.stopping` | `now_ms: u64` | - |
| 36 | `event.battery_measured` | `now_ms: u64` | - |
| 37 | `event.page_turned` | `now_ms: u64`, `section: u32`, `offset: u32`, `page: u32`, `pages: u32`, `progress_ppm: u32`, `path: str`, `title: str`, `chapter: str` | path 512, title 256, chapter 256 |

Notes on tag 37: `page`/`pages` are optional in `Reading` (`events.rs:24-27`); proposed
encoding is the presence bitmap already in the wire grammar (absent = counter not
available), not a sentinel zero. `progress_ppm` is millionths of the book (0..1,000,000);
`Reading.progress` is an `f32` percent. **[Q]** precision (ppm vs permille) and whether
the three strings belong in M1 at all: Battery needs none of them; readers, statistics
and sync plugins do. Alternative: ship only tags 32-36 now and add tag 37 when the first
consumer is ported (it is an additive record).
Strings over their cap are cut at a UTF-8 boundary by the host before encoding
(cap violations are the producer's, so never a plugin-visible error); the record
carries no truncation flag in M1 (**[Q]**).
The other ~54 SDK events (book opened/closed/finished, screen shown, wifi, status
changed, usb drive ended, plugins answered, menu events, ...) are not specified here.
Plugin-defined events (`Event::new("vendor.name")` and `Command::Emit(Id, Params)`)
use string ids today, so numeric tags alone cannot name them. **[Q2]** proposal: a later
`registration.event_named {name: str, token: u32}` (additive, tag from the
registration document's range) plus a `event.named {now_ms, name: str, payload: bytes}`.

### 1.4 Registration and dispatch rules

- [P] A plugin registers each kind with the existing `registration.event`; unknown
  `event_tag` returns `ERR_UNSUPPORTED` and poisons the construction transaction, as the
  tick check does (`registrations.rs:55-64`). The host admits tags 32-37 once their
  encoders exist.
- [P] One event, one `dispatch_event` call, in emission order. `STARTED` then `RESUME` stay
  two calls in that order.
- [P] The host encodes into a stack buffer (tags 32-36 are 8 body bytes plus framing,
  under 16 B; tag 37 at most 1,100 B) before the call; the plugin sees borrowed bytes valid
  for the call only (`abi/callbacks.md`).
- [P] Decoding follows `docs/abi/records.md`: exactly one record whose tag matches;
  trailing fields and unknown bitmap bits accepted; a decode error is `ERR_INVALID`
  returned from the plugin's callback, never a panic.

### 1.5 Failure semantics

A plugin callback that returns a negative status is logged against the plugin and the
event is not retried; remaining events still arrive. `ERR_BUSY` from the admission guard
(`abi7/calls.rs`) means the event is deferred by the host until the current dispatch ends
(**[Q]** drop vs defer when the queue is full; proposal: a queue of 8 events per plugin,
newest dropped, with a counter in plugin usage).

### 1.6 Memory

Host: no allocation per event (stack encode). Plugin: Battery keeps `State {pages: u32,
awake: bool, plugins_noted: Option<u64>}` (`lib.rs:41-49`), 24 B.

### 1.7 Open questions

Q1 screen input delivery (above). Q2 plugin-defined events. Q3 tag 37 now or later,
and `progress` precision. Q4 queue/drop policy. Q5 shell cadence and power-cut ordering
(not verified here).

## 2. Storage slots

(placeholder)

## 3. services.query, plugin names, usage and work notes

(placeholder)

## 4. Text, font, i18n and command vocabulary

(placeholder)

## 5. Summary, open questions, estimates

(placeholder)
