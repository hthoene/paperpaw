# Interface 7: usage records and the clock (Battery pilot)

Status: contract for review (#228, assignment #808). It answers the Services Engineer's questions 1-11 of
section 2 in `assignment-788-plan.md` (attachment 774 on #228) and refines section 3 of
`interface-7-events-storage-services.md` (cited as **E7** below). Where the two differ, this document wins
for usage, plugin names, work notes and the clock. Base: `origin/abi7/integration` 5733ac47d.

Conventions as in E7: **[V]** is a verified fact with a `path:line` citation on the base, **[D]** is a
decision of this contract, **[Q]** is left open. Nothing here is implemented yet; slots 17-19 still answer
`ERR_UNSUPPORTED` (`crates/paperpaw-native/src/host_glue.rs`, default table).

## 0. Where it lands, and the meaning rule

- [D] Everything here is an **addition inside the open interface-7 draft**: four service keys, the record
  tags 48-52 from the `service.*` family (`docs/abi/records.md`, tag table), and a first meaning for slots
  17-19, which so far only answer `ERR_UNSUPPORTED`. No existing slot, record, field or status changes
  meaning, so the meaning rule (row 2's `docs/abi/raise.md`, PR #195) is kept: during a draft, existing
  entries only gain meaning. This document itself changes no generated file and needs no re-bless.
- [D] The `abi.def` rows for tags 48-52 and their codecs land with the generator work (row 4: singleton
  `records_decode_one`, plus the stream reader of section 2.4), re-blessed inside the draft that is open
  then. Slot 3 and the snapshot are not touched.

## 1. Facts this contract builds on

- [V] Slot 3 `clock.micros(ctx, out: out_u64) -> status`, effect query, since minor 1 (`abi/abi.def`, host
  entry 3). It is one of the two entries every shell installs itself (`host_glue.rs:1-3,76`): the shell's
  `Services::micros()`, documented as "microseconds since any fixed point, not going back"
  (`host_glue.rs:68-69`). It returns `ERR_INVALID` for a null `ctx` or `out` and `OK` otherwise
  (`host_glue.rs:207-224`).
- [V] Shell clocks: the X4 counts from boot (`esp_hal::time::Instant`, `shells/xteink/src/main.rs:322-327`);
  the desktop counts from the creation of its host services and saturates at `u64::MAX`
  (`shells/desktop/src/host_services.rs:76-78`).
- [V] The snapshot (slot 4) carries `monotonic_ms: u64` and `unix_seconds: i64` with presence bit 4 for the
  Unix time; an absent Unix time is encoded as 0 (`docs/abi/snapshot.md`, presence bits). Its milliseconds
  are the Env's monotonic `Instant`, and its Unix seconds use the SDK `UnixTime` mapping (`snapshot.md`,
  host integration). Every event's `now_ms` uses that same clock (E7 shared rule 2).
- [V] Global counters: `Usage` (`crates/paperpaw-sdk/src/usage.rs:49-67`), "since PaperPaw started (on a
  device whose sleep cuts the power, since it last woke)". They are kept by the kernel in `shared.usage`:
  inputs (`crates/paperpaw-core/src/lib.rs:454`), panel time (`lib.rs:874`), sleeps (`lib.rs:1153`),
  refreshes and pixels (`commands.rs:193`, `usage.rs:132-137`), and Bluetooth time
  (`bluetooth.rs:160`). All of them saturate except `bluetooth_ms`, which uses `+=`.
- [V] Per-plugin counters: `PluginUsage` (`usage.rs:69-100`), one row per plugin with `core` first
  (`crates/paperpaw-core/src/meter.rs:21-27`). Rows are only ever appended (`meter.rs:77-86`), and every
  counter saturates (`meter.rs:112-175`). PR #188 adds a per-plugin `storage.sync` count, attempts included,
  kept beside the table in the same order (`syncs`, branch `kernel/abi7-storage-adapter-228`,
  `meter.rs:31-33,91-99`).
- [V] Plugin ids are at most 40 bytes by the package rule (`tools/dev/src/package/folder.rs:623`); the
  `Id` type itself has no length limit (`crates/paperpaw-sdk/src/id.rs`).
- [V] What Battery uses today:
  - per sample: `env.now().millis()`, the battery, `env.usage()` and the Unix time
    (`plugins/battery/src/lib.rs:53-63`). The log columns are `kind, ms, millivolts, percent, charging,
    fast, partial, full, kilopixels, inputs, panel_ms, pages, unix`, and `unix` stays empty where the time
    is unknown (`log.rs:40,100-121`).
  - every 10 minutes: `env.plugin_usage()` for the plugin lines `ms, plugin, calls, busy_ms, shown_ms,
    fast, partial, full, kilopixels, KiB read, KiB written` (`lib.rs:116-127`, `log.rs:164-185`).
  - in its view: the same table for the developer rows (`shown_ms`, `busy_us`) and the network rows
    (`requests`, `refused`), plugin names, and work notes (`view.rs:226-303`).
  - a new time awake: a reading whose `ms` is smaller than the last one (`log.rs:255-271`).

## 2. Usage and plugin services

### 2.1 Keys, versions and metadata (answers Q1, Q3)

| Key | Argument | Version | Blob holds |
| --- | --- | --- | --- |
| `core.usage` | none | 1.0 | one tag 48 record |
| `core.plugin-usage` | none, or `from.<n>` | 1.0 | tag 49 rows, then exactly one tag 52 |
| `core.plugin` | a plugin id | 1.0 | one tag 50 record |
| `core.work-note` | a plugin id | 1.0 | one tag 51 record |

- [D] The key grammar is E7 D3.2 and stays ASCII: `key = service [ "/" arg ]`, at most 128 bytes.
  `from.<n>` takes `n` in decimal, from 0 to 4,294,967,295, with no sign and no leading zero (except
  `0` itself). `core.plugin-usage` without an argument is `from.0`.
- [D] The version is slot 17's `major`/`minor`, never part of the key. `major` must be 1 and `minor` at most
  0, otherwise the result is `ERR_UNSUPPORTED`.
- [D] A plugin's metadata declares the **base key** only (`core.usage`, never `core.plugin-usage/from.3`),
  as a service requirement with `major` 1 and the minor it needs (`abi/metadata.md`, service record:
  `key_bytes`, `major`, `semantic_revision`, `min_table_bytes`).
  - For a data service, `semantic_revision` is the required minor and `min_table_bytes` is 0: a data
    service has no call table.
  - A query for a key that is not declared returns `ERR_UNSUPPORTED`, so `needs_minor` and the store's
    admission see every service a plugin reads.
- [D] `core.plugin` gives only the name, in the UI language at the time of the query. `enabled` is
  dropped: Battery does not need it, and an optional field can still be appended later, whereas a field
  cannot be removed.

### 2.2 Records (answers Q1, Q2, Q8)

The wire format is the one in `docs/abi/records.md`: `tag, len, body`, an empty bitmap, positional scalars
in little-endian. All fields are required, in this order. Strings are UTF-8.

| Tag | Record | Fields in order |
| --- | --- | --- |
| 48 | `service.usage` | `fast: u32`, `partial: u32`, `full: u32`, `pixels: u64`, `inputs: u32`, `sleeps: u32`, `panel_ms: u64`, `bluetooth_ms: u64` |
| 49 | `service.plugin_usage` | `plugin: str 64`, `busy_us: u64`, `calls: u32`, `shown_ms: u32`, `fast: u32`, `partial: u32`, `full: u32`, `pixels: u64`, `read: u32`, `written: u32`, `syncs: u32`, `requests: u32`, `refused: u32`, `awake_ms: u32`, `awake_reason: str 64` |
| 50 | `service.plugin` | `name: str 128` |
| 51 | `service.work_note` | `at_ms: u64`, `text: str 256` |
| 52 | `service.end` | `flags: u32`, `total: u32`, `generation: u32` |

Sizes:
- A tag 48 record is 44 bytes of fields, 47 bytes with its framing (E7's "52 B body" overcounted).
- A tag 49 row is 60 bytes of numbers plus two strings of at most 65 bytes each (framed), plus framing:
  at most 194 bytes, so always below the 256 bytes of E7. A typical row (a short id, no hold) is about
  80 bytes.
- A tag 52 record is at most 15 bytes.

### 2.3 Meaning, units and overflow of every field

**Shared counter rules:**
- [D] Every counter counts since PaperPaw started; on a device whose sleep cuts the power, that is since
  it last woke. Counters never go down within one start, and they start again at 0 after a restart.
- [D] Every counter **saturates** at its type's largest value and stays there, and it is never cut down to
  a smaller type. A host that keeps a wider value converts it to the field's type by saturating.
  - Implementation item: `bluetooth_ms` is the only counter that adds with `+=` today
    (`bluetooth.rs:160`); it becomes `saturating_add` in the services PR.

**Tag 48, the device:**

| Field | Meaning | Unit |
| --- | --- | --- |
| `fast`, `partial`, `full` | screen refreshes asked for, by mode; modes between fast and full count as partial | refreshes |
| `pixels` | the sum of width × height of every refresh's area (a pixel refreshed twice counts twice) | pixels |
| `inputs` | button presses and releases, taps and swipes the kernel handled | events |
| `sleeps` | times the device went to sleep, or the app to the background | times |
| `panel_ms` | time the panel was busy with refreshes, where the shell reports it; 0 where it does not | ms |
| `bluetooth_ms` | time the Bluetooth radio was on; 0 on devices without one | ms |

**Tag 49, one row per plugin.** Row 0 is the kernel's own work, plugin `core`; the other rows follow in the
kernel table's order. The counters count while that plugin's code runs or while its screen is on top.

| Field | Meaning | Unit |
| --- | --- | --- |
| `plugin` | the plugin's id (never cut, see below) | - |
| `busy_us` | time its code ran; code it calls into (for example a nested plugin) pauses the count; 0 on a host without a clock for it | µs |
| `calls` | times its code was entered | calls |
| `shown_ms` | time one of its screens was on top (`u32`: saturates after about 49.7 days) | ms |
| `fast`, `partial`, `full`, `pixels` | as in tag 48, but only refreshes while its screen was on top | refreshes, pixels |
| `read`, `written` | bytes of storage its code read and wrote (`u32`: saturates at 4 GiB) | bytes |
| `syncs` | `storage.sync` calls of its code, **failed ones included** (S7; PR #188's per-plugin count) | calls |
| `requests` | network requests, servers and connections it opened | count |
| `refused` | those refused (offline, its network setting off, not declared) | count |
| `awake_ms` | time its holds kept the device from sleeping on its own (`u32`) | ms |
| `awake_reason` | the reason given with its last hold; empty if it never held | - |

Strings (answers Q8):
- [D] `awake_reason` and the tag 50 `name` are cut by the host to their caps, at a UTF-8 character
  boundary, and are never refused. An overlong name must not make a whole page fail.
- [D] `plugin` is an identity and is never cut. A row whose id is longer than 64 bytes is **left out** of
  the page. It still counts in `total`, and it sets `flags` bit 1. Ids at most 40 bytes long (the package
  rule) never hit this.
- [D] A work note's text is bounded **where it is stored**: the kernel cuts it to 256 bytes at a character
  boundary when it records the note. The record then carries what the kernel holds, with no second
  rule. This settles E7 Q14.

**Tag 50:** `name` is the plugin's display name, translated in the UI language at the time of the query. A
plugin without a translated name gets its id.

**Tag 51:**
- `at_ms` is when the note was set, on the same clock as `env.snapshot.monotonic_ms` and the events'
  `now_ms`.
- `text` is the note itself.
- A plugin without a note gets `ERR_NOT_FOUND` from the query. An empty note is a record with an empty
  `text`.

**Tag 52, the end of a table page:**
- `flags`:
  - bit 0: the page is **truncated** (more rows follow; ask again from `from.<n + rows read>`).
  - bit 1: at least one row was **left out** (an over-long id).
  - Other bits are 0; a reader ignores bits it does not know.
- `total`: the number of rows in the table, `core` included, when the page was built.
- `generation`: see 2.5.

### 2.4 Blob grammar and codecs (answers Q4)

- [D] A **scalar service** (`core.usage`, `core.plugin`, `core.work-note`) answers a blob that is exactly
  one record of its tag, with no count and no end record.
- [D] A **table service** (`core.plugin-usage`) answers a record stream with no count prefix: zero or more
  tag 49 rows, then **exactly one** tag 52 record as the last bytes of the blob. The end record is always
  there, also on the last page.
- [D] Readers refuse each of these with `ERR_INVALID`:
  - a blob that ends before the tag 52 record;
  - bytes after the tag 52 record;
  - a second tag 52 record;
  - any tag other than 49 before the end.

  A row with appended optional fields that the reader does not know is accepted
  (`records.md`, forward compatibility).
- [D] **Codec ownership:** the Kernel's generator emits the codecs from `abi.def`:
  - the records;
  - `records_decode_one(&[u8])` for a scalar blob (row 4);
  - a stream reader that takes one record at a time from a carry buffer of at most 256 bytes and reports
    the end record.

  Services builds the SDK facade on top and writes no codec by hand.

### 2.5 Pages, total and generation (answers Q7)

- [D] `from.<n>` starts at row index `n` of the kernel table, counted from 0 (`core`). The host fills the
  page with whole rows and **reserves the end record's bytes first**.
  - Every page whose `n` is below `total` holds at least one row: a row is at most 194 bytes, and the
    smallest page is 512 bytes (2.6).
  - If `n` is at or past `total`, the page holds only the end record, with `flags` 0.
- [D] `generation` changes whenever the table's **rows** change, that is when a row is added, removed or
  reordered: exactly the changes that shift offsets. Counters change all the time; the reader's own
  `busy_us` grows while it reads, so a generation that followed counters would differ on every page and
  tell the reader nothing.
  - Today rows are only appended, so `generation` changes exactly when a plugin first runs or shows a
    screen.
  - It starts at 0 when PaperPaw starts and adds 1 with wrapping. Readers only compare it for equality.
  - Pages are **not a consistent snapshot of counters** (S6). Two pages with the same generation have
    the same rows in the same order, but their counters may come from different moments.
- [D] When the generation changed between pages, Battery may see a row twice or miss one. That is
  acceptable: its 10-minute lines and its view are statistics. It keeps the last row it read for each id,
  so it never counts a plugin twice.

### 2.6 Lifetime, budget and statuses (answers Q5, Q6, Q11)

**Lifetime:**
- [D] A blob handle has kind `Blob` only, and it **expires at the end of the callback that created it**.
  Its storage is freed exactly once, on the first of three things:
  - `blob.release`;
  - the end of that callback, whatever it returns;
  - the owner's teardown (unload, stop, failure).

  Cleanup allocates nothing.
- [D] `handle.release` (slot 8) on a blob handle returns `ERR_STATE` and frees nothing: there is one release
  path per kind.

**X4 budget:**
- [D] On the X4 there is **one live blob per device, and a page is at most 512 bytes**. The blob is
  charged to the "stream chunk storage" row (512 bytes, one outstanding chunk) of ADR 0034's experimental
  M1 X4 resource profile (`origin/fw/adr-flat-boundary-228:docs/adr/0034-stable-plugin-boundary.md`,
  profile table). It is no second budget beside the 4,096-byte aggregate, so S6's "at most 4 KiB" is
  narrowed to 512 bytes on the X4.
  - What this costs Battery: 2 to 6 rows a page. With about 70 rows that is 12 to 35 pages every
    10 minutes, 3 crossings each, a few milliseconds in total.
  - The scalar services fit easily: tag 48 needs 47 bytes, tag 50 at most 133, tag 51 at most 270.
- [D] **Other hosts** may allow more live blobs and larger pages, up to E7 D3.4's two per owner, 16 KiB
  in total and 4 KiB a page. A plugin may rely only on the X4 floor: one live blob at a time and pages
  of at least 512 bytes. It must follow `flags` bit 0 rather than assume a page size. The desktop and
  emulator hosts use the X4 figures by default, so host tests show the X4's behaviour.
- [D] The plugin's carry buffer of one record (at most 256 bytes) is the plugin's own memory, read with
  `blob.read` into its own buffer. It is neither boundary storage nor the decoder workspace. Reconciling
  event 37's largest size with the 256-byte decoder workspace belongs to the events profile (PR #187),
  not to this contract.

**`services.query` (slot 17)** checks in this order. A failed query creates no handle and leaves
`out_handle` unchanged:

| Check | Status |
| --- | --- |
| no live frame for the caller, or the wrong owner | `ERR_STATE` |
| null or bad pointers, length over 128 | `ERR_INVALID` |
| key not UTF-8 | `ERR_UTF8` |
| key not in the grammar (2.1), including a bad `from.<n>` | `ERR_INVALID` |
| unknown service | `ERR_NOT_FOUND` |
| key not declared in the metadata, wrong major, or minor too high | `ERR_UNSUPPORTED` |
| unknown plugin id in `core.plugin` or `core.work-note`, or no note | `ERR_NOT_FOUND` |
| a live blob already exists (X4: device-wide), or no free handle (per owner or in the registry) | `ERR_BUSY` |
| allocation failed (everything allocated so far is freed) | `ERR_NO_MEMORY` |
| otherwise: build the blob, insert the handle, write `out_handle` | `OK` |

- [D] `ERR_BUSY` means a limit that frees itself when handles are released or the callback ends; it is
  never a quota failure. `ERR_NO_SPACE` stays reserved for "buffer or batch too small", as elsewhere.
- [D] **`blob.read` (slot 18):** a stale, foreign or wrong-kind handle gives `ERR_STATE`. An offset at or
  past the end returns 0, and a zero-capacity read copies nothing (`abi/README.md`). A read never
  returns more than the capacity or 4,096 bytes.
- [D] **`blob.release` (slot 19):** a stale, foreign or wrong-kind handle gives `ERR_STATE` and releases
  nothing. A blob that expired at the end of its callback is stale.

### 2.7 Where the host reads the tables (answers Q9)

- [D] The adapters read the kernel's `Usage`, the meter table, the `syncs` counts, plugin names and work
  notes through a **read-only, callback-scoped accessor on the existing frame** (`abi7/context.rs`). No
  borrow of a table survives the callback, and each page is encoded straight from the kernel table,
  never through `Env::plugin_usage()`, which clones the table (`env.rs:1036-1039`).
- [D] The Kernel supplies this frame extension in a small PR (about 1-2 h) after this contract is approved.
  Services then implements the adapters for slots 17-19 and the SDK facade on top of it.

## 3. The clock (answers Q10)

- [D] **No new slot or record.** Slot 3 and the snapshot cover what plugins need. A checked SDK wrapper
  is enough: `paperpaw_sdk::native::clock::micros() -> Result<u64, i32>`.
- [D] **Slot 3 is always present.** Every interface-7 host installs it: it has existed since minor 1, and
  the shell's `Services` must provide `micros`.
  - It never answers `ERR_UNSUPPORTED`. With a valid `ctx` and `out` it always answers `OK`, and it
    answers `ERR_INVALID` only for a null `ctx` or `out`.
  - A host without a fine clock still answers `OK`, with values from the best clock it has (for example
    milliseconds × 1000). The resolution is the host's and is not promised.
  - The value saturates at `u64::MAX` and never wraps.
- [D] There are **two monotonic clocks and no common epoch**:
  - `clock.micros`, for measuring durations within a callback or across callbacks;
  - the milliseconds of `env.snapshot.monotonic_ms`, every event's `now_ms` and tag 51's `at_ms`, which
    are one and the same clock.

  Each clock is monotonic (never going back) only within one start of PaperPaw, and both may start again
  from 0 after a restart or a power-cutting sleep. On the X4 both happen to count from boot; on the desktop
  they do not (1). A plugin must not derive one clock from the other or compare their values.
- [D] **Wall time.**
  - Only the snapshot gives it: `unix_seconds` with presence bit 4, present once the device knows the
    time. Unknown time is absent (bit clear, 0 seconds), never a guess.
  - No host and no SDK wrapper makes a wall time up from either monotonic clock. A plugin that shows
    or logs the time of day shows "unknown" or leaves the value out.
  - The time can become known in the middle of a run, so a plugin reads it per sample and does not cache
    it.

## 4. What Battery needs, keeping its behaviour

| Battery today | Interface 7 |
| --- | --- |
| `env.now().millis()` per reading (`lib.rs:57`) | `now_ms` of `event.battery_measured` (or `monotonic_ms` of the snapshot in a view) |
| `env.usage()` per sample (`lib.rs:61`) | `core.usage`: query, read, release in the same callback |
| `unix_seconds(env)` (`lib.rs:63`) | snapshot `unix_seconds` if bit 4 is set, else an empty `unix` column, as today |
| `env.plugin_usage()` every 10 minutes and in the view (`lib.rs:116-127`, `view.rs:226-300`) | `core.plugin-usage` pages until `flags` bit 0 is clear, one blob at a time |
| `plugin_name(env, id)` (`view.rs:89-96`) | `core.plugin/<id>`, only for the 5-10 rows the view shows |
| `env.work().note(&owner)` (`view.rs:303`) | `core.work-note/<id>`; `ERR_NOT_FOUND` means no note |

- [D] Its log stays byte-identical: the same columns, `kilopixels` = `pixels / 1000`, KiB = bytes / 1024,
  and `busy_ms` = `busy_us / 1000`. A new time awake is still detected by `ms` going back (1), which the
  restart rule in 3 keeps true.
- [D] `syncs` is new. Battery's log keeps its columns, and the developer view may show `syncs` later as
  its own change, with a format note in Battery's CHANGELOG.
- [D] Port note: Battery adds `p.requests + p.refused` with `+` (`view.rs:274-275`). With counters that
  saturate at `u32::MAX` that sum can overflow, so the port uses `saturating_add`.
- Per sample this costs 3 crossings and a 47-byte blob. Every 10 minutes it costs 12-35 pages, with no
  `Vec<PluginUsage>` held by the plugin and no clone of the table on the host.

## 5. Tests the implementation must bring

- Codecs: a round trip for each of tags 48-52, every field at 0 and at its largest value; strings at the
  cap and one byte over (refused by the encoder); truncated records; and an unknown appended field
  (accepted).
- Streams: no end record, bytes after the end, two ends, a foreign tag before the end, an empty table
  (only the end), and a carry buffer split at every byte of a row.
- Paging:
  - `from.0` up to the end, with every page ≤ 512 bytes and each holding at least one row;
  - `from.<total>` and `from.<total + 1>`, each giving only the end record;
  - `from.` with a leading zero, a sign, or a value above `u32::MAX`, each refused;
  - the generation stays equal while only counters change, and changes when a row is added;
  - an over-long id is left out and sets bit 1.
- Saturation: every counter at its largest value plus one stays there, `bluetooth_ms` included.
- Statuses: each row of the slot 17 table, with `out_handle` unchanged on every failure.
- Lifetime: release, callback end and teardown each free the blob exactly once. `handle.release` of a blob
  gives `ERR_STATE`. Stale, foreign and wrong-kind handles on slots 18 and 19 give `ERR_STATE`. A second
  live blob on the X4 gives `ERR_BUSY`, and so does a query from another owner.
- Allocation failure at each step frees everything and creates no handle.
- Clock:
  - slot 3 gives `OK` and does not go back across calls;
  - the wrapper maps `ERR_INVALID` to `Err`;
  - a snapshot without bit 4 makes the SDK give no Unix time;
  - a Battery log test with `unix` unknown, then known within one run.

## 6. Open

- [Q] Whether later consumers (not Battery) need a blob that outlives its callback (E7 Q15). Not in
  interface 7 unless a port needs it.
- [Q] The four persistent handles per plugin (ADR 0034 profile) are shared with fonts. Battery needs 1-2
  fonts plus one blob, which fits. A census across all ports stays open (E7 Q16).
