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

## Kernel Engineer position (review of this draft, 2026-11-06)

The draft was prepared with a helper and reviewed by the Kernel Engineer. Declarative registration
(manifest, translations, setting specs, resources) is in `declarative-registration.md` (PR #158) and takes
priority. It blocks every port; this document blocks Battery and the UI groups.

- **Events (1):** agreed. Keep `event.battery_measured` as its own event: tick has no fixed rate, and the
  host already gates sampling on a battery being present. Lifecycle and page turn are event kinds. Input
  stays with the screen callback, which must return whether it handled the input.
- **Storage (2):** agreed in shape: path-based slots 20–26, root 0 (the data folder, stamped by the host)
  only, other roots reserved. **No power-loss claim is final until cut tests exist.** These are: `write`
  old-or-new, rename over a file written by `append`, and the torn tail after `append` without `sync`.
  They are the first implementation item, before any slot is wired. A host-framed `append_record` stays out
  of interface 7. No hard quota; a free-space reserve.
- **Statuses:** one new `ERR_NOT_FOUND` (-10), shared with slot 6's missing key (registration proposal Q5).
  `ERR_FULL` (-11) stays distinct from `ERR_NO_SPACE`, which means a buffer or limit.
- **services.query (3):** snapshot-over-blob is my preference. Whether slot 17 is also the path to interface
  services with call tables is an owner/Senior question; it is not decided here.
- **Text, font, i18n, commands (4):** the facts are correct: only heap and clock are installed in a host table.
  The slot 13/14 adapters, font acquire/release and the i18n format/digits slots are the next text items
  after registration.

## Decided (Manager, 2026-11-06 05:27)

Recorded after Senior review; these supersede the corresponding open questions below. Caps that depend on a
measurement stay provisional until the X4 gate.

- **Quota:** no hard per-plugin quota. Usage is counted per plugin (bytes and syncs, S7) and a free-space
  reserve, the larger of 256 KiB and 1 % of the card, keeps the kernel's own saves possible.
- **Slot 17:** snapshot queries over blobs only. Calls into interface services such as `core.library` get their
  own design later.
- **Battery view:** the plugin draws over the canvas and text slots; host widget entries are a separate, later part.
- **`append_record`:** added only if the V1 cut tests show stale-byte tails (S2).
- **`storage.sync`:** no rate limit; per-plugin counting (S7).
- **Events:** `event.battery_measured` stays its own event; lifecycle and page turn are event kinds.
- **`service.end` (tag 52):** carries `flags: u32` (bit 0 truncated), `total: u32` (rows in the table) and
  `generation: u32` (changes whenever the table changes), in that order, as S6 requires.
- **Owner decisions (on #228, open):** storage root 1 for the user's card files (recommendation: yes, gated by a
  signed `uses.storage` feature; root 1 stays reserved in the wire meanwhile, root 0 is implemented), and what
  uninstall does with a plugin's data folder (decision 124).

## Binding amendments after Senior review (S1–S7, 2026-11-06)

These amendments override the sections below wherever they differ. The storage contract becomes final only
when each of them is in the implementation and closed by its test.

- **S1 Large writes (amends D2.5).** The recipe is: `remove(name.tmp)`, then `append` the chunks to
  `name.tmp`, then `sync`, then `rename(name.tmp, name)`. Removing first is required: a leftover temp file from
  an earlier cut or failure would otherwise receive the new chunks after its stale bytes, and the rename would
  publish a corrupt file that looks valid. Any error at any step abandons the attempt; the next attempt starts
  again with the remove. Test: start from a leftover `name.tmp` holding old data and show that the published
  file holds exactly the new bytes.
- **S2 Torn tails may hold stale bytes (amends D2.5 and V1).** The V1 cut tests check two cases: a tail that is
  short, and a file whose recorded size covers clusters whose new data was never written. The second would
  expose bytes of a deleted file, for example Battery's rotated `log.old.csv`, as rows that parse as valid. If
  V1 shows the second case can happen, per-record integrity becomes **required**: either a CRC in the plugin's
  record format or the host-framed `append_record` of Q8. Until V1 has run, the contract promises only "a prefix
  of the appended stream, possibly torn", and Battery must treat unverifiable rows as damaged.
- **S3 Lifecycle events are never dropped (amends 1.4 and 1.5).** `event.suspend` and `event.stopping` bypass the
  per-plugin event queue limit and are never deferred because of `ERR_BUSY`. The host dispatches them
  synchronously before it suspends or powers off. **V2 is a gate, not an open question:** power-off must wait
  until the suspend and stopping dispatch has returned, so that a handler's `append` and `sync` complete. The
  shell must show this before lifecycle events are activated.
- **S4 Failed appends (amends D2.6 and 2.3).** `append` and `write` check the free-space reserve **including the
  data length** before writing anything, so a full card normally writes nothing and returns `ERR_FULL`. After an
  `ERR_FULL` or `ERR_IO` from `append`, a partial line may still remain in the live file. The plugin must end
  that line (write a line break, or start the next record with a terminator) before it appends again; readers
  drop the unterminated fragment.
- **Two folders (terminology).** The **install folder** `plugins/<id>/` holds the plugin's code, `plugin.ini`,
  catalogs and resource files (registration proposal, PR #158); it is replaced by an update and removed by
  uninstall. The **data folder** `.paperpaw/plugins/<id>/` is storage root 0, where the plugin's own files live.
  An update keeps the data folder. In this document "plugin folder" means the data folder.
- **Uninstall and the data folder (owner decision 124, 2026-11-06).** On uninstall the user is **asked** whether
  the plugin's data folder is deleted as well. The default is **keep**. The prompt is a later UI task
  (Plugin Engineer). The kernel's part is the deletion, when the user chose it. It is a data-loss path, and the
  owner requires each of these points as a contract requirement with its own test (Senior reviews the
  implementation):
  1. Only the uninstalled plugin's own data folder is deleted, `.paperpaw/plugins/<id>/`, built by the kernel
     from the plugin's **validated id** (`Id`, non-empty, never `.` or `..`, no `/`). A path the plugin or
     any caller supplies is never used.
  2. The match is **exact**, never a prefix: removing `battery` leaves `battery-plus/` and `batteryx/`
     untouched.
  3. An empty, invalid or unresolvable id is a **refusal** that deletes nothing. It is never a deletion of
     `.paperpaw/plugins/` or of any other folder.
  4. A **power cut** in the middle leaves every other folder intact. The kernel first renames the folder
     into a kernel-owned folder for removals in progress, then deletes it there, and it finishes a removal
     found there at the next start. Nothing outside that folder is touched by the deletion. The test cuts
     power after every single write.
  - The user's choice reaches the kernel as an explicit "delete the data" request on the uninstall command.
    Its record shape belongs to the command vocabulary (section 4.4, tags 56-62), not to the storage slots.
  - Today `remove_plugin` removes only a data plugin's install folder and never a data folder
    (`crates/paperpaw-core/src/plugins.rs:852-861`).
  - On a card whose FAT is already damaged (two chains sharing clusters, the #238 class), deleting one file
    could free clusters another file uses. The implementation names this and checks whether the deletion
    should refuse when it finds such clusters.
- **S5 Root 0 cannot reach the install folder.** A root-0 path is validated as a relative path first and then
  joined under `.paperpaw/plugins/<id>/` by the host, so it can never name a file in the install folder
  `plugins/<id>/` (or anywhere outside the data folder).
- **S6 Blob budget on the X4 (amends D3.4).** The X4 profile allows **one live service blob per device, at most
  4 KiB**, within ADR 0034's 4,096-byte boundary profile. Larger hosts may keep the D3.4 figures. A result that
  does not fit is truncated at a record boundary and flagged in its `service.end` record. The plugin asks for
  the next part with a row offset as the key's argument, `core.plugin-usage/from.<n>` (D3.2 grammar; the
  service version is slot 17's `major`/`minor`, never part of the key). The host builds each page directly from
  the kernel's own table, never through a second copy such as `Env::plugin_usage()`, which already clones
  several KB per call. Pages are **not a consistent snapshot**: rows can shift between callbacks, so a reader
  may see a row twice or miss one. `service.end` carries the table's total row count and a generation counter
  that changes whenever the table changes, so the reader can tell that its pages came from different states.
- **S7 Sync accounting (amends Q9).** `Metered` counts `storage.sync` calls per plugin, next to bytes read and
  written, so a plugin that wears the card shows up in Battery's developer view. A rate limit stays an open
  question.

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
start point)` (`view.rs:434-466` on the 734 ref). **[Q1]** how an action id (a string) is
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

### 1.5 Failure semantics (Q4 decided)

- [D] No per-plugin event queue in 7.0 (0 extra bytes). The kernel already holds back
  events raised during a callback and delivers them after it returns, so a plugin is
  never re-entered by an event.
- [D] A plugin callback that returns a negative status is not retried; remaining events
  still arrive.
- [D] A non-lifecycle event that cannot be delivered (lease taken or scope busy, no
  environment, a producer parameter missing or unparsable, or a negative callback status)
  is reported to the shell through `FlatInstance::event_dropped(tag, why)` and dropped.
  The shell logs it; core keeps no counter, and the usage row's `refused` (network
  refusals) is not touched.
- [D] `suspend` and `stopping` are delivered synchronously when the plugin is not inside
  a callback. They are never retried, never wait, and never block power-off; a busy
  instance takes the same `event_dropped` path.
- [D] Flat plugins reach the kernel as ordinary kernel plugins through the code loader
  (`native_host::flat::FlatPlugin` over the shell's `FlatInstance`), so the kernel's
  recipient rules (`only`, password-mode events, guarded screens) apply unchanged. A
  batch registering anything the adapter cannot route yet (tick 3, draw) is refused at
  load with the tag named, never ignored.
- [D] `Reading.progress` (f32 percent) maps to `progress_ppm` as
  `((p.clamp(0.0, 100.0) * 10_000.0) + 0.5) as u32`, capped at 1,000,000 (NaN and
  -inf give 0, +inf gives 1,000,000); a bad producer value never drops the event.
- [D] Page counters (producer rule; the wire rule in `records.md` stays strict): a present
  0 in `page` or `pages`, or a `page` past `pages`, sends both counters absent; an
  unparsable optional counter is sent absent on its own, keeping the other when it is valid
  alone. The page turn is still delivered; only a missing or unparsable required parameter
  drops it (`DropReason::Producer`). Decoders on both sides refuse what the wire forbids.

### 1.6 Memory

Host: no allocation per event (stack encode). Plugin: Battery keeps `State {pages: u32,
awake: bool, plugins_noted: Option<u64>}` (`lib.rs:41-49`), 24 B.

### 1.7 Open questions

Q1 screen input delivery (above). Q2 plugin-defined events. Q3 tag 37 now or later,
and `progress` precision. Q4 decided (1.5). Q5 shell cadence and power-cut ordering
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

**D2.1 New slots, path based, no handles.** [D] (declared in `abi/abi.def` by the slots PR, all answering
`ERR_UNSUPPORTED` in the default table until they are wired). Append seven entries at the end of
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
[D] An entry's `max_bytes` is its **largest** `bytes` or `buffer` argument: 4096 for `read_at`, `write` and
`append` (the data), and 512 for `size`, `rename` and `remove` (the paths). Every path argument is at most 512
bytes on every entry (`MAX_PATH`, `crates/paperpaw-core/src/native_host/storage.rs`), and the host checks it
before anything else; for `rename` that holds for each of the two paths. The generator emits two `bytes`
arguments correctly; the C and Rust layout tests (`tools/tests/test_abi_layout.py`) compile and compare
them. A cap per argument in `abi.def` would change the lock format, so it is not done in interface 7.

**D2.2 `root` selects the namespace.** [D] Root 0 is implemented first; root 1 is decided (owner decision 125).
- `root = 0` (PLUGIN): the host prefixes `.paperpaw/plugins/<id>/` where `<id>` is
  stamped from the plugin's owner identity, never read from the call (ADR34:339
  "Namespaces are stamped, not claimed"). The plugin passes a relative path.
- `root = 1` (CARD, the user's files such as books and notes): **owner decision 125 (2026-11-06)**.
  - **The gate.** Root 1 is open only to a plugin whose **signed** metadata declares the semantic feature
    `uses.storage`. The kernel reads it when it loads the plugin and keeps it with the plugin's owner
    identity; it never trusts a runtime argument for it. A plugin without the declaration gets
    `ERR_UNSUPPORTED` for root 1 on every slot, and keeps root 0. The feature's entry in `abi.def` lands
    with the root-1 admission PR.
  - **What root 1 is.** Paths relative to the card's root, with the rules of D2.3.
  - **What it excludes.** The first path segment may not name `plugins` (the install folders) or `.paperpaw`
    (the kernel's settings, catalog and data, and every plugin's data folder, other plugins' included). Both
    are refused with `ERR_INVALID`, for both paths of `rename`.
    - The comparison is by **identity, not spelling**. FAT ignores case and can find an entry by its 8.3
      short name, so `Plugins`, `.PAPERPAW` and a short alias such as `PAPERP~1` must all be refused. The host
      compares the resolved first segment with the two folders (`paperpaw_sdk::files::same_path`), not the
      bytes of the path.
    - A plugin reaches its own data folder through root 0 only.
  - **Why other plugins' folders stay out**, although plugins may change other plugins' settings (owner
    decision 126): settings are changed through slot 15 (`command.set_setting`), which validates the value,
    keeps guarded and secret settings protected and announces the change. They are never changed by writing
    files, so decision 126 does not widen root 1.
  - **The same rules as root 0.** Root 1 has the same free-space reserve (D2.6), the same per-plugin
    counting (S7) and the same atomicity contract (D2.5).
  - Roots 2 and above stay reserved: `ERR_UNSUPPORTED`. The argument is in the signature now because slot
    signatures cannot change after release.
- This confinement is **hygiene and accounting, not a security boundary**: a native plugin
  can still write anywhere through the CPU (ADR 0024:48, 226-229). It gives a contract
  that works for emulator, wasm and Android hosts, per-plugin metering and clean uninstall.
  The owner wants it, with root 1 for the user's files (decision 125).

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
off. On the X4, `free_space()` is unknown (`None`) until `paperpaw-fat` has counted the FAT, a
128 KiB part at each ask, never trusting the card's FSInfo count (#238); until then the reserve
is skipped and the storage's own `Full` applies.

Gates before plugin storage goes live (Senior's remarks 1 and 2 on PR #188, Manager 2026-11-06):

- **G-free-1, the count is driven.** The kernel asks for one part in its idle ticks
  (`paperpaw-core/src/free_count.rs`): from the start, again on waking and after the card was a
  USB drive, a 50 ms pause between parts, never while a press waits to be told single or double
  nor within 2 s of sleep, not on a storage that cannot tell its size. The count stops while a
  computer holds the card. The shell must remount the FAT storage or reset its `Free` count before
  `usb_drive_ended`, since the computer may have changed it. No shell grants `USB_DRIVE` today.
  Done on host. Open on the X4
  (Firmware measures with the next device run, Kernel confirms; Notes PR #16 lists the same rows):
  (a) each part takes **at most 25 ms** (from the log); (b) on a FAT32 card of 64 GB or more, after a
  cold boot, the free space is known within **30 s** of idle; (c) over 10 sleeps, **no part** starts
  within 2 s before sleep; (d) a press during the count is handled no later than with the count off,
  and within 25 ms.
- **G-free-2, no drift after the count.** The free space follows the clusters the patched
  rust-fatfs marks taken and free in the FAT (`FileSystem::cluster_changes`), so a write that
  fails part of the way, a folder that grows and temporary files are followed exactly
  (`paperpaw-fat/src/tests/free_follow.rs`). Done on host. Open on the X4: once the count is done,
  20 writes, appends, renames and deletes; then the free space differs by **0 clusters** from a fresh
  count (after a restart) and from a PC's value for the card.

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

Q6 root/confinement policy (D2.2): settled by owner decision 125. Q7 reserve size and per-plugin quota. Q8 host-framed
append record. Q9 `sync` fairness. Q10 `ERR_NOT_FOUND`/`ERR_FULL` numbering and a media-gone
status. Q11 listing (`storage.list`) with the `list_from` cursor
(`platform/storage.rs` `list_from`) is not needed by Battery and is deferred; its
records would need the grammar of section 3.3 (stream). Q12 a cut test for append/sync
and for rename-over-an-appended-file must exist before the contract text is final (see
section 5, item V1).

## 3. services.query, plugin names, usage and work notes

Usage, plugin names, work notes and the clock are now contracted in `interface-7-usage-clock.md`, which
wins where it differs from this section (tag 48/49/50 fields, `next` in tag 52, page size on the X4,
`generation`).

### 3.1 Facts

- [V] Slot 17 `services.query(ctx, key: bytes, major: u16, minor: u16, out: out_handle)
  -> status`, effect mutate, ownership owner, `max_bytes` 128; slots 18/19
  `blob.read(ctx, blob: handle, offset: u64, out: buffer) -> count` (<= 4096) and
  `blob.release(ctx, blob: handle)` (`abi/abi.def`). Metadata allows up to 16 declared
  service keys of <= 128 bytes (`abi/metadata.md`).
- [V] Nothing implements them: the default table answers `ERR_UNSUPPORTED`
  (`host_glue.rs:309-353`); there is no core adapter. The generic handle registry exists
  with kinds `Font`, `Service`, `Blob`, 32 slots, **4 live handles per owner** and 27-bit
  generations (`paperpaw-core/src/abi7/handles.rs:7-8`); teardown drops an owner's handles
  (`take_owner`).
- [V] The scoped frame behind slots 4-6 exposes only settings, translator and snapshot
  (`abi7/context.rs:12-17`); usage, plugin list and work notes are not reachable from it.
- [V] ADR34:219 replaces `Services::get::<T>()` by "a key, a major.minor and a `#[repr(C)]`
  table per key (`core.library/1`)". Slot 17 as declared returns a `u32` handle, not a
  table pointer, and no call-through-handle slot exists. So **interface services such as
  `core.library` have no invocation path yet** (**[Q13]**, outside Battery).
- [V] What Battery reads (interface 6): global `Usage` per sample (`lib.rs:61`,
  `usage.rs:51-67`: 3 refresh counts, pixels u64, inputs, sleeps u32, `panel_ms`,
  `bluetooth_ms` u64); `plugin_usage()` every 10 minutes and for the developer view
  (`lib.rs:116-127`, `view.rs:226-300`): rows of `PluginUsage` (`usage.rs:72-100`: id,
  `busy_us` u64, calls, `shown_ms`, 3 refresh counts, pixels u64, read, written, requests,
  refused, `awake_ms` u32, `awake_reason` string); plugin display names via
  `env.plugins().get(id).name(i18n)` (`view.rs:89-96`); background work notes
  `env.work().note(&owner)` -> `Note {at: Instant, text: String}`
  (`view.rs:303`; `sdk/src/work.rs:703-706,757`); settings by dynamic key
  (`core.network.<plugin>`, `core.saving.<plugin>`, `view.rs:100-108`, covered by slot 5).
- [V] `Env::plugin_usage()` today **clones the whole table** on each call, "as the kernel
  keeps one table only (memory is short on small devices)" (`env.rs:1036-1039`).
  With about 70 plugins that is already several KB on the heap per call.

### 3.2 Decisions (proposed)

**D3.1 Slot 17 is a snapshot query; its handle is a Blob.** [P] For the keys below,
`services.query` builds an immutable byte snapshot in host memory and returns a handle of
kind `Blob`; the plugin reads it with `blob.read` and frees it with `blob.release`
(`handle.release`, slot 8, is refused for it with `ERR_STATE` so there is one release
path per kind). Interface services with call tables (ADR34:219) are a different class,
appended later with their own slot; this class is named **data services**.
**[Q13]** whether the owner wants snapshot semantics for slot 17 or reserves it for
interface services and appends a separate `data.query` slot. The handle registry
(`Kind::Service` next to `Kind::Blob`) suggests the second was anticipated; the declared
`blob.*` slots suggest the first.

**D3.2 Key grammar.** [P] `key = service [ "/" arg ]`, ASCII, at most 128 bytes (the
metadata key limit). `service = segment ("." segment)+`, segment `[a-z][a-z0-9-]*`
(<= 32 bytes). `arg = [A-Za-z0-9._-]{1,64}` (a plugin id, or `from.<n>` for a page offset). Anything else is `ERR_INVALID`;
non-UTF-8 is `ERR_UTF8`. Unknown service: `ERR_NOT_FOUND`. `major` must equal the
provider's major, `minor` must be <= the provider's, otherwise `ERR_UNSUPPORTED` (ADR34
compatibility rule: a plugin built at `7.m` runs on `7.n`, `n >= m`). The service key and
minor go into the plugin's declared service list (`metadata.services`) so `needs_minor`
accounts for it.

**D3.3 Blob content grammar.** [P] Single small records are one wire record
(`tag, len, body`, as in `docs/abi/records.md`) and the blob is exactly that record.
Tables are a **record stream**: records back to back, no count prefix, ending at the
blob's length. This differs from a batch (count-prefixed, <= 8 records,
<= 4096 B) on purpose: a 70-row table cannot be one batch. A reader keeps a carry buffer
of one record (<= 256 B for usage rows) between `blob.read` chunks. Total blob size cap
**8 KiB**; a table that would exceed it is cut at a row boundary and flagged by a final
`service.end {flags: u32, total: u32, generation: u32}` record (bit 0 of `flags` = truncated).

**D3.4 Blob lifetime and budget.** [P] A snapshot blob expires **at the end of the callback
that created it** (stale use returns `ERR_STATE`, the host frees it) unless the key class
declares otherwise; Battery reads and releases within one callback. Limits: at most 2 live
blobs per owner (the registry's 4 handles are shared with fonts) and **16 KiB of live
blob bytes across all plugins**; beyond that `services.query` returns `ERR_NO_MEMORY`
(allocation failure) or `ERR_BUSY` (budget used by another owner), with no handle
created.

### 3.3 Services and records

| Key | Arg | Response | Used by Battery for |
| --- | --- | --- | --- |
| `core.usage` | none | one record tag 48 | global counters per sample |
| `core.plugin-usage` | none | stream of tag 49 rows, then tag 52 | 10-minute plugin lines, developer view |
| `core.plugin` | plugin id | one record tag 50 | display name |
| `core.work-note` | plugin id | one record tag 51 (absent = `ERR_NOT_FOUND`) | note under a plugin in the view |

| Tag | Record | Fields in order (all required) | Caps |
| --- | --- | --- | --- |
| 48 | `service.usage` | `fast: u32`, `partial: u32`, `full: u32`, `pixels: u64`, `inputs: u32`, `sleeps: u32`, `panel_ms: u64`, `bluetooth_ms: u64` | 52 B body |
| 49 | `service.plugin_usage` | `plugin: str`, `busy_us: u64`, `calls: u32`, `shown_ms: u32`, `fast: u32`, `partial: u32`, `full: u32`, `pixels: u64`, `read: u32`, `written: u32`, `requests: u32`, `refused: u32`, `awake_ms: u32`, `awake_reason: str` | plugin 64, awake_reason 64; row <= 256 B |
| 50 | `service.plugin` | `name: str`, `enabled: u32` | name 128 |
| 51 | `service.work_note` | `at_ms: u64`, `text: str` | text 256 |
| 52 | `service.end` | `flags: u32`, `total: u32`, `generation: u32` | - |

Notes. Row size estimate: about 52 B of numbers plus two short strings, roughly 80-90 B,
so 70 plugins is about 6 KB (above one 4096 B read, hence chunking and the 8 KiB cap).
`Instant` and `Duration` fields are exported as monotonic milliseconds so the semantics of
`Instant` (an opaque kernel type) never cross. Counters stay `u32` saturating as in the
SDK (`usage.rs`). `service.work_note.text` is capped at 256 B by the **producer**
(the kernel, when the note is stored) with a UTF-8-safe cut; today `Note.text` has no
documented bound (`work.rs:703-706`) (**[Q14]** bound it at the `Command` that sets it
instead). `enabled` is a placeholder for the plugin manager's state: **[Q]** drop it if no
consumer needs it.

Alternative considered for `core.usage`: append the counters to the snapshot
(slot 4). Rejected: the snapshot has 128 B and uses 52 (`abi/abi.def` `snapshot_value`),
and every callback would pay for counters only Battery reads; that space is the scarce
extension room.
Alternative for plugin names: embed the localized name in every usage row. Rejected: 70
catalog lookups per query for the 5 to 10 names the view shows.

### 3.4 Validation, failure and per-sample cost

Order of checks in `services.query`: frame and owner (`ERR_STATE`), pointers and lengths
(`ERR_INVALID`), UTF-8 (`ERR_UTF8`), key grammar (`ERR_INVALID`), service known
(`ERR_NOT_FOUND`), version (`ERR_UNSUPPORTED`), per-owner and global blob budget
(`ERR_BUSY`), build snapshot (`ERR_NO_MEMORY` on allocation failure with everything freed),
only then insert the handle and write `out`. `blob.read` of a stale or foreign handle is
`ERR_STATE`; offset past the end returns 0; a zero-capacity read copies nothing
(`abi/README.md`). `blob.release` of a stale handle is `ERR_STATE` and releases nothing.
Teardown of the owner frees its blobs exactly once.
Per sample Battery adds: `services.query` + `blob.read` + `blob.release` for usage
(3 crossings, host snapshot 60 B), and every 10 minutes the same for the plugin table
(host transient about 6 KB, the same order as the clone `Env::plugin_usage()` makes today,
`env.rs:1036-1039`, while the plugin no longer holds a `Vec<PluginUsage>`).

### 3.5 Open questions

Q13 snapshot vs interface semantics for slot 17 (and an invocation slot for
`core.library`). Q14 where the note text is bounded. Q15 whether a blob may outlive its
callback for other consumers (ADR34 section 5 says "retained blobs use owner handles").
Q16 the per-owner handle limit of 4 is shared by fonts, services and blobs; Battery needs
at least 1-2 fonts plus up to 2 blobs: confirm the limit or raise it. Q17 `enabled` field.
Q18 the registry stores a `T` per entry; a `Blob` payload of up to 8 KiB x 2 owners must
fit the budget on X4 (not measured).

## 4. Text, font, i18n and command vocabulary

### 4.1 Facts: what is integrated, compiled, installed

- [V] **Installed in a combined host table: none of the text, font, i18n or command slots.** The only table
  constructor, `host_table()`, wires heap and clock; `font_acquire`, `handle_release`,
  `text_measure`, `text_draw`, `i18n_copy`, `commands_submit` and the rest answer
  `ERR_UNSUPPORTED` (`paperpaw-native/src/host_glue.rs:233-353`). `git grep` across
  `origin/abi7/integration`, `fw/abi7-loader-prefix-228`, `fw/abi7-foreign-fixture-228`,
  `kernel/abi7-scoped-settings-228` and the two `samples/*` refs finds no field
  initializer for these slots other than that stub table and the generated/baseline files.
  `abi/settings.md:29-31` says the settings/translation backends "compile in core but are
  not installed in a combined host table"; that remains true.
- [V] **Compiled in `paperpaw-core/src/abi7`:**
  - slot 5 and slot 6 backends: `queries::settings_read` and `queries::i18n_copy`
    (`abi7/queries.rs:21,49`), `unsafe`, scoped to a live frame; `i18n_copy` copies
    `Translator::tr(key)`, which **returns the key itself when the message is missing**
    (`paperpaw-i18n/src/lib.rs:273-277`), so a plugin cannot tell missing from present;
  - slot 4: `snapshot` + `snapshot_wire` (`abi7/snapshot.rs`);
  - text: **safe Rust functions** `graphics::text_measure(&FontHandle, size, &str)` and
    `graphics::text_draw(&mut Canvas, &FontHandle, size, x, baseline, &str, Color)`
    (`abi7/graphics.rs:118,133`). Their error contract is documented: input errors fail
    before drawing, a bad returned advance is reported **after** drawing, so slot 14 is not
    atomic (`graphics.rs:127-131`). These are not `extern "C"` adapters and have no frame
    or handle lookup in front of them;
  - the generic handle registry (`abi7/handles.rs`), no adapter uses it yet.
- [V] **Missing for fonts:** no adapter resolves a `font.acquire` key to a font, no
  `handle.release` adapter exists, and the font slots take a `u32` handle while the SDK's
  `FontHandle<'a>` is a **borrow** of the registry, cache and drawing state
  (`paperpaw-sdk/src/fonts.rs:796-810`); it cannot be stored in the registry past the call.
  A persistent handle must hold an owned key (the font id) and **re-resolve on each
  `text.*` call**, failing with `ERR_STATE` when the pack was replaced
  (ADR34:127-135 "a stale one fails with `ERR_STATE`"). Fonts are looked up by string id
  today (`Fonts::get(id)`, `fonts.rs:542`).
- [V] Canvas slots 9-12: only a typed `CanvasScope` on the Kernel branch
  `kernel/abi7-scoped-canvas-228` (head 2f8f99cc per the #228 ticket comments); it is not
  in this base and was not inspected here.
- [V] Battery's UI is built from SDK widgets (`ui::Row`, `ui::Label`, `heading`, paging
  header, `UiStyle`; `view.rs:140-466` on the 734 ref) that call fonts, i18n and style
  through the SDK's shared table today. How those widgets work in interface 7 (plugin-side
  code over slots 7, 13, 14, 9-12 and settings, or host widget entries, ADR34:~190 class B)
  is not defined anywhere I found. **[Q19]** This is the largest unknown on the view side.

### 4.2 i18n formatting, plural and digits

- [V] Battery calls `format(key, [("m", Arg::Int)])`, `plural(key, n, [])`, `number(i64)` and
  `localize_digits(text)` (`view.rs:55-73,81-82,117-129,189-193`), plus `tr(key)`.
  `Arg` is `Str(&str) | Int(i64)` (`paperpaw-i18n/src/format.rs:6-9`). Digits come from the
  catalog's `[meta] digits` (`paperpaw-i18n/src/lib.rs:195-199`); `Arg::Int` and `{count}` use
  them, `localize_digits` maps the Latin digits of a pre-formatted string, which can change
  its byte length (Arabic-Indic digits are 2 bytes).
- [P] **Do the work in the host, once** (catalog and plural rules live there; copying them
  into every plugin costs flash on the X4). Append two slots:

| Id | Entry | Arguments (after `ctx`) | Result | Effect | max_bytes |
| --- | --- | --- | --- | --- | --- |
| 27 | `i18n.format` | `key: bytes`, `args: bytes`, `out: buffer` | length | query | 4096 |
| 28 | `i18n.digits` | `text: bytes`, `out: buffer` | length | query | 4096 |

  `i18n.format` follows the snprintf rule used by slots 5 and 6 (returns the full length,
  writes at most `cap`, caller retries with a larger buffer; repeatable and read-only, so
  the schema's `length` result is allowed, `abi-gen.py:~140`). `args` is a record batch
  (<= 8 records, <= 1024 B total) of:

| Tag | Record | Fields |
| --- | --- | --- |
| 44 | `i18n.arg_str` | `name: str` (<= 32), `value: str` (<= 256) |
| 45 | `i18n.arg_int` | `name: str` (<= 32), `value: u64` (two's-complement `i64`; no `i64` wire type exists) |
| 46 | `i18n.count` | `value: u64` (an `i64`) |

  Semantics: no `i18n.count` record = `Translator::format`; with exactly one = plural
  selection by the catalog's rule (`key.<category>`, fallback `key.other`) with `{count}`
  bound, as `Translator::plural` (`paperpaw-i18n/src/lib.rs:281-300`). More than one count
  record, a duplicate name, an unknown tag or over-cap field: `ERR_INVALID` (unknown tag
  inside the args batch: `ERR_UNSUPPORTED`). A missing key returns the key (same as
  `tr`). Number localization is `i18n.digits(decimal text)`; `number(n)` becomes ASCII
  format in the plugin plus one `digits` call. `i18n.copy` (slot 6) stays for raw
  templates.
- [P] Memory: host builds the result in a 4 KiB stack/scratch buffer (today
  `Translator::format` returns a heap `String`, `lib.rs:281`); plugin stack holds only the
  caller's buffer. Validation completes before the first catalog lookup.
- [Q20] Whether a "key found" signal is wanted (`i18n.copy` and `format` return the key
  for a missing message); proposal: a status `ERR_NOT_FOUND` only for an explicit
  variant, not for the default.

### 4.3 Fonts and text slots (7, 8, 13, 14): what Battery needs

- [P] `font.acquire(key)`: `key` = the registered font id as `Fonts::get` takes it
  (<= 128 B, id grammar of section 3.2); returns a persistent handle, per-owner limit 4
  (registry). Battery needs one or two (regular, bold if headings use bold). **[Q21]** the
  key for "the user's UI font" and the reading font: reserved role keys
  (`role:ui`) versus ids; not defined in `abi.def`.
- [P] `handle.release(h)` for fonts; stale -> `ERR_STATE`; teardown frees all.
- [P] `text.measure`/`text.draw`: keep the slot signatures; the host wraps
  `graphics::text_measure/text_draw` behind frame + handle resolution. Needs: finite,
  non-negative `size`, `x` finite and in range, text <= 4096 B UTF-8, canvas present
  (slot 14, else `ERR_STATE`). `text.draw` is documented non-atomic on an advance error.
- Not needed by Battery now: wrapping/layout, hyphenation, glyph caches, reading faces
  (ADR34:202-206 text layout redesign); they stay out of this document.

### 4.4 Command vocabulary beyond `command.set_setting`

- [V] The commands Battery's interface-6 code issues: `Command::OpenScreen(SCREEN,
  Params::new())` and `Command::run(action)` = `RunAction(id, Params::new())`
  (`plugins/battery/src/lib.rs:165-166`, `view.rs:460` on the 734 ref;
  `sdk/src/command.rs:1141-1143`), plus screen outputs `request_redraw()` and
  `request_refresh(RefreshMode::Partial)` (`view.rs:~440-448`; `sdk/src/screen.rs:874-876,
  1003-1006`). `Command::Redraw(RefreshMode)` exists (`command.rs:743`). Battery sets no
  setting itself (no `SetSetting`), uses no `Notify`.
- [V] No `commands.submit` adapter exists in core (grep found none). SetSetting (tag 4,
  `key: str <= 128`, `value: settings_value` bytes, own-namespace registered non-secret
  settings only, whole-batch validation, <= 8 commands / 4096 B) is on Kernel's commands
  branch; not read by me, referenced only.
- [P] Records, tags 56-58 (tags 5-31 are the registration document's, 4 is SetSetting):

| Tag | Record | Fields | Caps |
| --- | --- | --- | --- |
| 56 | `command.open_screen` | `screen: str` | 128 (id grammar) |
| 57 | `command.run_action` | `action: str` | 128 (id grammar) |
| 58 | `command.redraw` | `mode: u32` (open set; 0 = request a redraw only, 1.. = `RefreshMode` ordinal + 1, so unknown modes are refused, not mapped) | - |

  Optional trailing field `params: bytes` (<= 256) is **not** added now: `Params` is a
  typed bag (`Params::carried::<T: Any>` cannot cross, ADR34:~230) and Battery passes none.
  An `open_screen` of a screen id the plugin did not register, or an action not owned by
  or exposed to it, is checked **by the host before queueing**: `ERR_INVALID` for a
  malformed id, `ERR_NOT_FOUND` for a missing target (whole batch refused, nothing queued).
  Owner is stamped by the host (ADR34:339), never read from the record. Redraw and refresh
  from within a screen callback are `command.redraw` in the same batch (ADR34's "batch
  header flags" idea would change the batch grammar; `docs/abi/records.md` has only a
  count). **[Q22]** header flags versus a record.
- Failure semantics follow commands.submit: whole batch validated first; a refused batch
  queues nothing; commands run after the callback returns (ADR34:279-293).

### 4.5 Open questions

Q19 widget/UI strategy for the Battery view. Q20 "key found" signal. Q21 font keys.
Q22 redraw as record or header flag. Q23 how a font handle survives a language/pack
change (re-resolve each call: cost per `text.draw` of a registry lookup, not measured).
Q24 the frame behind these slots must also carry `Canvas` and font registry; the
`Frame` trait offers only settings, translator, snapshot today (`abi7/context.rs:12-17`).

## 5. Summary, open questions, estimates

### 5.1 Decisions at a glance (all proposed)

| # | Item | Proposal | New ABI surface |
| --- | --- | --- | --- |
| D1.1 | Battery sample | Keep a dedicated `battery_measured` event; not tick-driven | record 36 |
| D1.2 | Lifecycle / page turn | Event kinds with `now_ms` header; not a query | records 32-35, 37 |
| D1.3 | Key/touch/action input | Not events: screen callback table (Handled needed) | none here |
| D2.1 | Storage | 7 path-based slots, no handles | slots 20-26 |
| D2.2 | Namespace | `root` argument; root 0 = stamped plugin folder; others reserved | in slot signatures |
| D2.5 | Power loss | `write` atomic <= 4096 B; `append` durable only after `sync`, torn tail possible | contract text |
| D2.6 | Quota | Metering + free-space reserve (`ERR_FULL`); no hard quota | statuses -10, -11 |
| D3.1-4 | services.query | Snapshot "data services", handle = Blob, 8 KiB cap, 16 KiB global budget, expires at callback end | records 48-52 |
| D4.2 | i18n | `i18n.format` (args batch, plural via count record) and `i18n.digits` | slots 27, 28; records 44-46 |
| D4.3 | Text/fonts | Keep slots 7, 8, 13, 14; build adapters; persistent handle = owned key, re-resolved per call | none |
| D4.4 | Commands | `open_screen`, `run_action`, `redraw` beside `set_setting` | records 56-58 |

Totals: 9 new slots (20-28), 2 new statuses, about 17 new record tags, all in 32-63.
Generator work is required for all of them (`tools/abi_records.py:10-14,50` accepts only
three shapes).

### 5.2 Consolidated open questions for the owner and Senior

Decisions the owner should make first:
1. **Q13** Is slot 17 a snapshot query (this proposal) or reserved for interface services
   with call tables, with a separate data query slot?
2. **Q6** Does the owner want root confinement at all (ADR 0024 "no sandbox")? If yes,
   which roots exist in M1 and how `Uses`/metadata gates the rest.
3. **Q19** Widget strategy: plugin-side drawing code over slots, or host widget entries
   (ADR34 class B/E)? Battery's view cannot be ported without an answer.
4. **Q1** Screen input callbacks and action delivery (not events).
5. **Q7** Free-space reserve size and whether any hard per-plugin quota exists.

Also open: Q2 plugin-defined events (string ids); Q3 page-turned payload now or later,
progress precision; Q4 decided (1.5); Q5 shell cadence of
`battery_measured` and power-cut ordering after `suspend`/`stopping`; Q8 host-framed append
record; Q9 `sync` fairness; Q10 status numbers and a media-gone status; Q11 `storage.list`;
Q14 where a work-note is bounded; Q15 blobs outliving a callback; Q16 handle limit 4 per
owner; Q17 `enabled` field; Q18 blob memory on X4; Q20 "key found" signal; Q21 font keys;
Q22 redraw record versus batch header flag; Q23 cost of re-resolving fonts; Q24 frame
contents; whether the generator copes with two `bytes` arguments in one entry.

### 5.3 Verification items not done (needed before the text is final)

- **V1** A cut test for `append`, `sync` and rename-over-an-appended-file in
  `paperpaw-fat` (the existing cut harness `tests.rs:354-398,446` supports it); until it
  exists section 2.1's torn-tail statements are inference.
- **V2** Shell facts: how often `Kernel::tick` and `set_status` run on the X4 and the
  desktop shell, and whether power-off waits for the suspend dispatch to return.
- **V3** `docs/abi/commands.md` on Kernel's commands branch (not reachable) and
  `docs/abi/proposals/declarative-registration.md` (not yet present): check for tag and
  vocabulary overlap.
- **V4** Heap numbers on the X4 (blob budget, args scratch, handle registry) are
  estimates; none was measured. PlainBoard `get_task` #228 was readable; the Plugin
  Engineer's Battery inventory (26-38 agent-hours) was used for the Battery-side facts.

### 5.4 Implementation estimate (agent-hours, Kernel side, rough)

Assumptions: includes generator changes, core adapters, unit tests, docs and one Senior
review round per item; excludes X4 device qualification, the SDK facade and the plugin port
itself (the Plugin Engineer's 26-38 h), and the declarative-registration and SetSetting
work. Hours are for one agent working with the usual build limits.

| Item | Work | Hours |
| --- | --- | --- |
| Generator groundwork | `str` fields, more record shapes, tag ranges, new statuses, lock/baseline updates, tests | 12-18 |
| 1 Events | records 32-37, host encode and admission, dispatch order, tests | 10-16 |
| 2 Storage | 7 slots, core adapter over `Storage` (root, path join/validate, reserve, metering), mapping table, FAT cut tests (V1) | 22-32 |
| 3 services | key parser, 4 services, blob registry integration with budget and expiry, stream grammar, frame extension (usage, plugins, notes) | 22-32 |
| 4 Text/fonts/i18n | font adapters and persistent handle with re-resolve, `handle.release`, extern wrappers for 13/14, `i18n.format`/`digits` | 18-26 |
| 4 Commands | records 56-58, owner and target checks (on top of the SetSetting path) | 8-12 |
| Total | | about 92-136 |

The widget/UI strategy (Q19) is not in these numbers and may dominate the port.
