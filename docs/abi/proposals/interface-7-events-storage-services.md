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

### 2.1 Facts

- [V] **No storage slot exists** in slots 0-19, and the default table answers
  `ERR_UNSUPPORTED` for everything but heap and clock
  (`crates/paperpaw-native/src/host_glue.rs:325-353`). Storage must be new slots.
  ADR34 section 3 says `Storage` becomes a plugin-side proxy over host entries
  ("open, read, write, list with a visitor callback, rename, sync, atomic replace",
  ADR34:216).
- [V] Battery uses six operations: `size`, `read_at`, `append`, `rename`, `remove`,
  `sync` (`plugins/battery/src/lib.rs:76-96,131`; `log.rs:159-190` on the 734 ref). It
  never lists, opens, truncates or overwrites in place. Its files are in
  `.paperpaw/plugins/battery/` (`Env::plugin_dir`,
  `crates/paperpaw-sdk/src/env.rs:1042`): `log.csv`, `log.old.csv`, `plugins.csv`,
  `plugins.old.csv`, each rotated at 128 KiB (`log.rs:37`), so at most about 512 KiB.
- [V] The SDK `Storage` trait is path based and stateless (`platform/storage.rs:118-`):
  `write` "atomically where the medium allows", `append` "may keep what is appended back
  until `sync`", `rename` replaces a file, `remove` of a missing file is fine, `sync` writes
  back everything.
- [V] `Env::storage()` hands a plugin the **whole device storage** (`env.rs:680-684`).
  Nothing confines a plugin to its folder today. ADR 0024 accepts this: "No sandbox", and
  manifests "say, they do not confine" (`docs/adr/0024-native-code-plugins.md:48,215-229`).
  `plain_path` only guards paths from outside (transfer servers) (`sdk/src/files.rs:120`).
- [V] Path grammar: `is_valid_path` refuses empty, `.`, `..`, empty segments, `\`, `:`,
  control chars, trailing dot/space and Windows device names
  (`platform/storage.rs:451-465`); `FatStorage` also refuses its own temp names and journal
  (`paperpaw-fat/src/lib.rs:198-205`).
- [V] Metering already exists: `Metered` counts bytes read/written for the plugin that is
  running (`paperpaw-core/src/meter.rs:180-`, `io` at `:119-`), shown by Battery itself.
- [V] **Power loss on FAT32 (paperpaw-fat).**
  - `write` goes through a temp file written whole and synced, then renamed/replaced:
    "Cut off at any point, a file reads its old content or its new one, whole"
    (`paperpaw-fat/src/lib.rs:55-62`; code `:935-985`; tests `tests.rs` `cut_images`,
    `a_file_written_anew_is_found_after_any_cut` etc.).
  - `rename` adds the new entry before removing the old: cut between, the entry is at both
    places, never neither (`lib.rs:~64-72`); rename-over-existing is tested
    (`tests.rs:~591`).
  - `append` is **not atomic and not durable**: "kept in the open file until it is written
    back" and "written back when a call that changes the card returns (but
    `Storage::append`) and on `sync`" (`lib.rs:6-16,22-27,1226-1247`; `tests.rs:295-322`).
    So an unsynced appended tail can be lost by a cut. Up to `OPEN_FILES = 16` files are
    kept open (`lib.rs:120`).
  - On a full card an append applies "as far as it came" (`lib.rs:~1239`, comment "As far
    as it came, where the card is full"), i.e. **a failed append can leave a partial
    record**. `FatStorage` implements neither `truncate` nor `write_at` (default
    `Unsupported`), so the host cannot roll it back.
  - Block write order: changed blocks reach the card "in the order they were changed, and
    before whole blocks written past them: cut off at any point, the disk holds the changes
    up to one of them" (`paperpaw-fat/src/disk.rs:14-18`).
  - **Not verified:** no existing cut test targets an append or a `sync` in flight (the
    cut tests cover write, remove, rename, move; `after_cuts.rs:169` appends only *after*
    a cut). What a cut mid-append leaves in the last cluster (short tail, or stale bytes
    inside a size that already covers them) is therefore an inference, not a measured
    result.
- [V] `write_whole` needs room for a temp copy of the file before the old one is
  replaced (`lib.rs:955-985`), so a full card also stops the kernel's own atomic saves
  (settings use the same pattern).

### 2.2 Decisions (proposed)

**D2.1 New slots, path based, no handles.** [P] Append seven entries at the end of
the table (ids 20-26), all `since 1`, `reentry deny`:

| Id | Entry | Arguments (after `ctx`) | Result | Effect |
| --- | --- | --- | --- | --- |
| 20 | `storage.size` | `root: u32`, `path: bytes`, `out: out_u64` | status | query |
| 21 | `storage.read_at` | `root: u32`, `path: bytes`, `offset: u64`, `out: buffer` | count | query |
| 22 | `storage.write` | `root: u32`, `path: bytes`, `data: bytes` | status | mutate |
| 23 | `storage.append` | `root: u32`, `path: bytes`, `data: bytes` | status | mutate |
| 24 | `storage.rename` | `root: u32`, `from: bytes`, `to: bytes` | status | mutate |
| 25 | `storage.remove` | `root: u32`, `path: bytes` | status | mutate |
| 26 | `storage.sync` | (none) | status | mutate |

Why path based: it mirrors the SDK trait and Battery's six calls one to one, `FatStorage`
already caches open files by path (`lib.rs:849-890`), a handle would add lifetimes, a
per-plugin handle cap (the registry allows only 4 live handles per owner,
`abi7/handles.rs:8`, shared with fonts and blobs) and leak cleanup on unload, and no
existing consumer needs one. A handle-based open/read/write set can be appended later
without touching these. Why one entry per operation rather than a multiplexed
`storage.op`: per-entry `since` and `needs_minor` (`abi/metadata.md`), typed generated
stubs, per-entry `max_bytes`.
**[Q]** the generator accepts two `bytes` arguments but has one `max_bytes` per entry
(`abi-gen.py:123,133`); `rename`'s two paths share the cap (not verified that codegen
handles two `bytes` arguments; needs a test).

**D2.2 `root` selects the namespace; M1 implements root 0 only.** [P]
- `root = 0` (PLUGIN): the host prefixes `.paperpaw/plugins/<id>/` where `<id>` is
  stamped from the plugin's owner identity, never read from the call (ADR34:339
  "Namespaces are stamped, not claimed"). The plugin passes a relative path.
- `root = 1` (CARD, user files such as books, excluding `.paperpaw`) and later roots are
  **reserved**: `ERR_UNSUPPORTED` until the owner decides how `Uses`/metadata gate them
  (ADR 0024 lists "storage, other plugins' data" as declared uses). The argument is in the
  signature now because slot signatures cannot change after release.
- This confinement is **hygiene and accounting, not a security boundary**: a native plugin
  can still write anywhere through the CPU (ADR 0024:48, 226-229). It gives a contract
  that works for emulator, wasm and Android hosts, per-plugin metering and clean uninstall.
  **[Q]** whether the owner wants this at all, given "plugins should have as much room as
  possible" (ADR 0024:36-39).

**D2.3 Path rules.** [P] Relative, UTF-8, at most 512 bytes total and 255 per segment;
valid per `is_valid_path` (so `..`, absolute, `\`, `:` are `ERR_INVALID`), plus the FAT
temp-name/journal refusal (`fat lib.rs:198`). Validation happens on the plugin-relative
string **before** the prefix is added; the joined path is built in a 640-byte stack buffer
and re-checked. FAT ignores case, so `Log.csv` and `log.csv` name one file (document it).
Hidden names (`.x`) are allowed inside the plugin root (plugins keep hidden files).

**D2.4 Sizes.** [P] `path` <= 512; `data` for `write` and `append` <= 4096 per call;
`read_at` buffer <= 4096 per call (the entry's `max_bytes`); a longer write is chunked by
the plugin. Zero-length `append` creates the file; zero-length `write` creates an empty file.
`read_at` returns bytes copied, 0 at end of file, like `blob.read` (`abi.def` entry 18).

**D2.5 Atomicity and durability contract (what the doc must promise).** [P]
- `storage.write` (<= 4096 B): atomic replace; after a cut the file is old or new, whole
  (verified for FAT above). Parent folders under the plugin root are created.
- Atomic replace of **more** than 4096 B is composed by the plugin: `append` chunks to
  `name.tmp`, `storage.sync`, `storage.rename(name.tmp, name)`. This is power-safe given
  the rename property above. **Verify** rename-over-existing of a file written by
  `append` (not `write`) with a cut test before relying on it.
- `storage.append`: appended bytes are **in the file for reads at once but durable only
  after `storage.sync`**; one call is not atomic. After a cut the file holds a prefix of
  the appended stream, possibly with a torn last record (see the unverified item). The
  contract for plugin authors: records are newline- or length-framed, readers discard an
  unterminated or unparsable tail, and anything after the last `sync` may be lost.
- Battery fits: its log is CSV, line framed, and the reader already drops damaged lines
  one by one (`log.rs:159-190`; `src/log/hostile_tests.rs`). Syncs every 10 minutes and at
  sleep/stop are its data-loss bound (`lib.rs:128-131`: "an empty battery ends it all
  without warning"). The plugin facade should chunk `plugins.csv` appends **on line
  boundaries** (a full `plugin_lines` string can reach ~5 KB, over the 4096 cap).
- Framing for binary logs is not provided by the host. **[Q]** whether to offer a
  host-framed `storage.append_record` (length + CRC, torn-tail detection by the host).
- `storage.sync` flushes all kept-back data of the device (that is what `FatStorage::sync`
  does, `lib.rs:1249-1263`), not only the caller's. **[Q]** rate limit or accounting per
  plugin, since a plugin can force frequent card writes (Battery's comment on card wear,
  `lib.rs:128-130`).

**D2.6 Quota: accounting plus a free-space reserve, no hard per-plugin quota in M1.** [P]
`Metered` already counts bytes per plugin (`meter.rs:119-129`). Add a **reserve**: `append`
and `write` fail with `ERR_FULL` when `free_space()` would drop under a reserve (proposal
256 KiB or 1 % of the card, whichever is larger) so the kernel's own temp-and-replace saves
still succeed (`lib.rs:955`). Reason: otherwise one plugin can make settings saves fail.
A hard per-plugin byte quota would need a directory walk (FAT has no cheap folder size)
and contradicts ADR 0024's "as much room as possible"; offered as **[Q]** with default
off. `free_space()` is a progressive estimate on the X4 (`lib.rs:1325-1345`), so the
check is best effort.

### 2.3 Failure semantics

| Condition | Status | State left |
| --- | --- | --- |
| Bad pointer/length, path invalid, over caps | `ERR_INVALID` / `ERR_NO_SPACE` (too large) | nothing touched |
| Non-UTF-8 path | `ERR_UTF8` | nothing touched |
| Root unknown or unimplemented | `ERR_UNSUPPORTED` | nothing touched |
| No frame/owner (call outside a callback) | `ERR_STATE` | nothing touched |
| Missing file (`size`, `read_at`, `rename` source) | `ERR_NOT_FOUND` | nothing touched |
| `remove` of a missing file | `OK` (trait semantics) | - |
| Card full or reserve hit | `ERR_FULL` | `write`: old content intact (temp removed, `lib.rs:962-971`). `append`: **a partial tail may remain** (documented) |
| I/O error, card removed | `ERR_IO` | as the medium left it; `write` old-or-new |
| Re-entry from inside the same plugin's dispatch | `ERR_BUSY` before any effect | nothing touched |

### 2.4 Memory (256 KiB heap)

Host: no heap per call (stack path buffer <= 640 B, caller buffers). `FatStorage` open-file
cache is the existing ~100 B per open file x 16 (`lib.rs:117-125`) and is shared. A
`write` makes one temp copy on the card, not in RAM. Plugin: Battery's read loop uses a
2048 B buffer plus a carry `Vec` (`log.rs:164-185`); with a 4096 cap the stack buffer
should stay at 1-2 KiB on the X4.

### 2.5 Open questions

Q6 root/confinement policy (D2.2). Q7 reserve size and per-plugin quota. Q8 host-framed
append record. Q9 `sync` fairness. Q10 `ERR_NOT_FOUND`/`ERR_FULL` numbering and a media-gone
status. Q11 listing (`storage.list`) with the `list_from` cursor
(`platform/storage.rs` `list_from`) is not needed by Battery and is deferred; its
records would need the grammar of section 3.3 (stream). Q12 a cut test for append/sync
and for rename-over-an-appended-file must exist before the contract text is final (see
section 5, item V1).

## 3. services.query, plugin names, usage and work notes

(placeholder)

## 4. Text, font, i18n and command vocabulary

(placeholder)

## 5. Summary, open questions, estimates

(placeholder)
