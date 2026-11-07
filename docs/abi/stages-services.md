# Interface 7: stages (N4) and services (D4)

Status: contract of family 1a (#228). It replaces the stage and `service.call` parts of PR #231
(`stages-images-services.md`, never merged); images (N6) come with their own family. Everything
here is appended (records, `PpHost` slots 32-34, service schemas), so it is the next draft (12): the
one-line `PpHost` shape and the SDK `Record` shape change. Items marked [D] are decided.

It builds on `screens-input.md`: reply buffers (section 6), lifetime and cancellation (section 7),
tag space (section 8), nesting (section 11), grants (section 12) and D1/D4 (section 13).

## 1. Stages (N4)

A stage is a named extension point owned by one plugin, for example the reader's `reader.insert`.
The host is the broker: it orders subscribers, admits them, bounds their replies and enforces
nesting. It never reads the payloads.

### 1.1 Records

| Tag | Record | Fields |
| --- | --- | --- |
| 19 | `stage.spec` (declaration) | `name: str 96`, `schema: u32`, `flags: u32`, `max_input: u32`, `max_reply: u32`, `max_records: u32` |
| 20 | `registration.stage` (declaration) | `stage: str 128` (full id), `token: u32`, `order: i32`, `signature: u64` |
| 512 | `stage.call` (input to a subscriber) | `stage: str 128`, `schema: u32`, `payload: bytes 3840` |
| 513 | `stage.reply` (output of `stage.run`) | `subscriber: str 64` (plugin id), `index: u32`, `status: i32`, `flags: u32`, `signature: u64`, `payload: bytes 3840` |

- [D] **`stage.spec`:**
  - `name` is relative; the stage's full id is `<plugin>.<name>`, unique among the plugin's stages
    (a second spec of the same name is `ERR_INVALID`). Specs count against the host's
    declaration-index budget (`ERR_FULL` past it).
  - `schema` is the stage's own payload schema version, set by its owner; subscribers see it in
    every `stage.call`.
  - `flags`: bit 0 `changes_layout`, bit 1 `draws`, bit 2 `takes_taps`. Other bits are
    `ERR_UNSUPPORTED` (a later host may know them).
  - `max_input` and `max_reply` are each in 1..=3840 (the generated
    `STAGE_CALL_PAYLOAD_MAX_BYTES` and `STAGE_REPLY_PAYLOAD_MAX_BYTES`: a whole `stage.call` or
    `stage.reply` then still fits one 4,096-byte batch), `max_records` in 1..=64. Outside them:
    `ERR_INVALID`.
- [D] **`registration.stage`** subscribes the plugin's `token` to a stage by full id, at `order`
  (an `i16`, as `menu.entry`).
  - A registration naming a stage that is not installed is kept and stays inert. Stages come and go
    with plugins, so this is never a load error.
  - At most 32 active subscribers per stage, counted host-wide. Over 32, only the extra
    registrations are refused, never the plugin's batch: the first 32 in stage order (section 1.2)
    are active, the rest stay inert with a log line naming the stage and the plugin. Load order
    never decides. The user can turn a subscriber off per stage (a host setting, decision 131,
    #241); a subscriber turned off is inert and does not count.
  - The same stage and token twice in a plugin's batch is `ERR_INVALID`.
  - At most 8 stage registrations per plugin; a 9th is refused with `ERR_FULL` for the whole batch
    (the plugin's own declarations, so the plugin can fix it).
  - A registration to the plugin's own stage is refused at commit (`ERR_INVALID`, the whole batch):
    the owner is inside its own callback whenever the stage runs, so it could only ever answer
    `ERR_BUSY`.
- [D] Tags 19 and 20 are in the declaration block; 512 and 513 open the `stage` block
  (`screens-input.md` section 8). Payload tags are local to the stage's schema (for the reader's
  stages, `paperpaw-reader-api`).

### 1.2 Running a stage (slot 32)

```
stage.run(ctx, stage: bytes, cursor: u32, input: bytes, out: buffer) -> count   // slot 32
```

- [D] **Only the stage's owner runs it.** Any other caller gets `ERR_DENIED`. A stage that is not
  declared is `ERR_NOT_FOUND`. Host-defined stages (`core.*`) are run by the host only.
- [D] **One subscriber per call, by cursor.**
  - `cursor` 0 is the first subscriber in stage order: `(order, plugin id, registration index)`,
    ascending, stable while no plugin is added or removed.
  - The call runs exactly the subscriber at `cursor` and writes one `stage.reply` record (a single
    framed record, as `record_encode_one`) into `out`, returning the bytes written.
  - A cursor past the last subscriber returns `ERR_NOT_FOUND` and writes nothing. The caller loops
    until it gets that, so the X4 needs exactly one reply buffer, never subscribers x `max_reply`.
  - `index` in the reply echoes `cursor`.
- [D] **Checks before any subscriber runs** (each refusal runs nothing and leaves `out`
  untouched): `ctx`, frame and owner (`ERR_INVALID`, `ERR_STATE`), the id (`ERR_INVALID`,
  `ERR_UTF8`), the stage exists (`ERR_NOT_FOUND`) and is the caller's (`ERR_DENIED`), `input` at
  most `max_input` (`ERR_NO_SPACE`), and `out` can hold the largest reply this stage can produce:
  `record_framed_len` of a `stage.reply` with a 64-byte subscriber id and `max_reply` payload
  bytes (`ERR_NO_SPACE`).
- [D] **Membership is frozen for the whole outermost callback** in which the owner runs the stage:
  plugins started, stopped, installed or turned off during it change the subscriber list only after
  that callback returns, so a cursor loop sees one list.
- [D] **A subscriber that is not running (lazy start, `screens-input.md` section 7)** is never
  started inside `stage.run`: its reply has status `ERR_CANCELLED` and an empty payload, and the host
  starts the plugin after the outermost callback returns, so it can answer the next run.
- [D] **What the subscriber gets.** It is called through `dispatch_event` with its token:
  - `input` is one `stage.call { stage, schema, payload = the caller's input }`;
  - the lent `out` has `cap = max_reply` (section 6 of `screens-input.md`);
  - its result is framed by the host into the `stage.reply` payload, which is empty for a negative
    status.
- [D] **What the host checks of the reply:** framing only: a record batch, count at most
  `max_records`, length at most `max_reply`. On a violation, that subscriber's reply has `status =
  ERR_INVALID` and an empty payload, a log line names the subscriber, and the stage goes on. The
  caller decodes payloads with the stage's own decoder.
- [D] **Status in `stage.reply`:**
  - 0 or more: OK, with `payload` holding the reply;
  - the subscriber's own negative status;
  - `ERR_CANCELLED`: the subscriber plugin is stopped, stopping or not started yet (lazy start, see
    above);
  - `ERR_BUSY`: the subscriber is already in a callback (re-entry is refused, as for every
    `dispatch_event`);
  - `ERR_INVALID`: framing violation.

  An unreachable subscriber is never skipped silently: the caller sees its reply with a status and
  moves on. `stage.run` itself returns OK (the byte count) whenever it wrote a reply.
- [D] **`flags` in `stage.reply` are host-stamped**, never from the subscriber. Bit 0 is
  `style_granted`: the user's `core.grant.style.<subscriber>` is yes (decision 131). This is the
  only way the stage owner learns a subscriber's grant, since grants are own-only
  (`screens-input.md` section 12); the reader applies a subscriber's override of a user-protected
  value only when it is set. Other bits are 0; consumers ignore bits they do not know.
- [D] **`signature`** echoes the subscriber's `registration.stage.signature`. For a
  `changes_layout` stage the owner folds every subscriber's signature into its layout key
  (`screens-input.md` section 9). A plugin changes its signature by declaring again, at its next
  start.
- [D] **Nesting (N8).** `stage.run` runs at depth 1 only; a subscriber (depth 2) that calls
  `stage.run` gets `ERR_STATE`. `service.call` is allowed at depth 2.
- [D] **Lifetime.** `stage.run` is synchronous; nothing about a call outlives it. A subscriber keeps
  no borrow of `stage.call` (section 7).
- [O] **Asynchronous replies** (lookup providers that fetch from the network) are a later additive
  step: a subscriber then answers with an operation handle (section 7) instead of a payload. Not
  needed for the first ports. Owner: Kernel, with the dictionary port.

### 1.3 Host-defined stages

- [D] `core.input-filter` and `core.widget` are `core.*` stages that come with their families (UIX,
  widgets), not with N4 itself. Their payload schemas are generated from stage-local records (section 2.4).
- [D] Escape chord: holding Power for 8 s is never filterable; the host checks it before any
  `core.input-filter` subscriber.

### 1.4 Memory (X4)

- Per subscriber: one declaration-index entry, about 16 bytes resident; per stage spec about 24.
- Per `stage.run`: the caller's `out` and one host-framed `stage.call` on the host scratch, both at
  most 4,096 bytes and released when the call returns. Nothing is retained between calls.
- Timing: a stage the reader runs on every page turn (for example `reader.insert`) costs one
  callback per subscriber; that time counts against the X4 page-turn budget and is part of the
  timing gate of the reader port.

## 2. Services (D4)

A service is a versioned call table the host offers, for example `core.device` or `core.registry`.
Slot 17 stays what it is (snapshot queries over blobs, `interface-7-usage-clock.md`); call services
have their own two slots.

### 2.1 Slots

```
service.open(ctx, key: bytes, major: u16, minor: u16, out: out_handle) -> status            // slot 33
service.call(ctx, service: handle, op: u32, input: bytes, out: buffer) -> count            // slot 34
```

- [D] **`service.open`** gives a handle of kind `Service` for one service key (at most 128 bytes,
  the D1 key grammar, no argument part). Checks, in order, each leaving `out` unchanged:

  | Check | Status |
  | --- | --- |
  | null `ctx`, bad key or `out` pointer, key over 128 bytes | `ERR_INVALID` |
  | no live frame for the caller | `ERR_STATE` |
  | key not UTF-8 / not in the grammar | `ERR_UTF8` / `ERR_INVALID` |
  | no such service on this host | `ERR_NOT_FOUND` |
  | key not declared in the plugin's metadata, other major, or `minor` above the declared revision or the host's | `ERR_UNSUPPORTED` |
  | the plugin already holds a handle for this key, or the registry is full | `ERR_FULL` |
  | allocation failed | `ERR_NO_MEMORY` |

  One handle per declared key, so at most `metadata.max_services` per plugin (`screens-input.md`
  section 7). The handle is released with `handle.release` (slot 8) and at teardown.
- [D] **`service.call`** runs operation `op` of the service behind the handle:

  | Check | Status |
  | --- | --- |
  | null `ctx`, bad pointers, `input` or `out` over 4,096 bytes | `ERR_INVALID` |
  | no live frame, or a stale, foreign or wrong-kind handle | `ERR_STATE` |
  | an op the schema does not define, or one newer than the revision the handle was opened with | `ERR_UNSUPPORTED` |
  | an op whose required feature the plugin did not declare (D1) or the user did not grant | `ERR_DENIED` |
  | `input` over the op's `max_input`, or `out` shorter than its `max_reply` | `ERR_NO_SPACE` |
  | `input` not one record batch of the op's input tags, exactly (generated decoder) | the decoder's status |
  | re-entry into the same service from its own callback | `ERR_BUSY` |
  | otherwise: the service runs and writes its reply batch into `out` | count, or the service's status |

  "Exactly" is about tags: every record's tag is one of the op's `input` tags, by exact tag. Order
  and multiplicity are the op's own rules (for example "one record"), checked by the service after
  the generic checks.

  Every check above runs before the service does anything, so a refused call has no effect and
  leaves `out` untouched. A service's own failure after that is its documented status, and `out`
  stays untouched then too (as slot 18, `interface-7-usage-clock.md`): a service writes `out` only
  once it will succeed, or builds its reply in host scratch of at most `max_reply` bytes and copies
  it on success.
- [D] **Synchronous and bounded.** A call never waits on the network or the card beyond its op's
  stated bound. Longer work returns an operation handle (section 7, D3) whose completion is
  `event.op_done`, read with a further op of the same service.
- [D] **Nesting:** allowed at depth 1 and 2.

### 2.2 Records

Service input and reply records are kernel records of the `service` family in the growth block
1024-1535 (`screens-input.md` section 8), declared in `abi/abi.def` like every record and part of
the generated records codec. A schema names them by tag; decoders classify by exact tag. The first
one exists for host tests only:

| Tag | Record | Fields |
| --- | --- | --- |
| 1024 | `service.echo` | `text: str 256` |

### 2.3 Schemas (`abi/services/<key>@<major>.def`)

Each major of a service has one schema file, `abi/services/<key>@<major>.def` (JSON):

```json
{
  "key": "core.test-echo", "major": 1, "revision": 0, "since": 1, "test_only": true,
  "ops": [
    {"id": 1, "name": "echo", "since": 0, "effect": "query", "feature": null,
     "input": ["service.echo"], "reply": ["service.echo"],
     "max_input": 512, "max_reply": 512, "bound": "none"}
  ]
}
```

- [D] `key`: the D1 service key grammar; the file name is `<key>@<major>`. `major` 1..=65535 and
  `revision` 0..=65535 are the D1 catalog's `major` and `semantic_revision`: every schema has a
  metadata catalog entry (`abi.def` `metadata.services`) with the same key, major and revision,
  which is what plugins declare. `since` is the interface minor that introduced it; a new schema
  takes a minor not yet released (as every record, setting and metadata addition). `test_only`
  marks a schema for host tests: the generated table carries it, and a device host never registers
  such a service.
- [D] Ops, in id order: `id` 1..=65535, unique, never renumbered; `name` unique
  (`[a-z][a-z0-9_]*`); `since` the service revision that introduced the op (at most `revision`); `effect` `query` or `mutate`;
  `feature` a D1 feature key that gates it, or null; `input` and `reply` lists of `service.*`
  record names (tags taken from `abi.def`), at most 8 each; `max_input` and `max_reply` in
  1..=4096; `bound` what the call may wait for: `none`, `card` (one bounded card access) or
  `radio` (never: that is an operation handle).
- [D] `tools/abi-gen.py` validates every schema and writes one generated module, `services_wire`,
  to `abi/generated/`, `crates/paperpaw-core/src/abi7/` and `crates/paperpaw-sdk/src/native/wire/`:
  per service its key, version and op constants (module `<key>_v<major>`), and a static table the host dispatcher and the SDK
  wrappers share (`SERVICES`, `ServiceSchema`, `ServiceOp`, `service_op`).
- [D] The ABI lock pins every schema (lock section `services`), by key and major: a published
  schema only grows. Its ops, their ids, names, inputs, replies, bounds, features, `max_input` and
  `max_reply` never change (so a reply never grows past what a caller sized for); a new op takes a
  new id and the service's next revision; a breaking change is a new major, a new schema file next
  to the old one.
- [D] **First services**, each with its family's PR: `core.device` (DEV), `core.registry` (REG),
  `core.storage` metadata (FS), `core.library` (LIB), `core.secrets` (SEC), `core.net` (NET).
  `core.test-echo` exists for host tests and is never offered on a device.

### 2.4 Stage schemas (`abi/stages/<key>@<major>.def`)

Each major of a host-defined stage has one JSON schema, `abi/stages/<key>@<major>.def`:

```json
{
  "key": "core.status", "major": 1, "revision": 0, "since": 1, "test_only": false,
  "max_subscribers": 32, "max_input": 3840, "max_reply": 3840,
  "input": ["status.ask"], "reply": ["status.value"], "reply_rows": 8,
  "records": [
    {"id": 1, "name": "status.ask", "since": 0, "fields": [
      {"name": "items", "type": "str", "since": 0, "optional": false,
       "ordinal": null, "default": null, "max_bytes": 1032,
       "list": {"separator": "\n", "max_items": 8, "item_max_bytes": 128}},
      {"name": "context", "type": "bytes", "since": 0, "optional": false,
       "ordinal": null, "default": null, "max_bytes": 1024}
    ]},
    {"id": 2, "name": "status.value", "since": 0, "fields": [
      {"name": "item", "type": "str", "since": 0, "optional": false,
       "ordinal": null, "default": null, "max_bytes": 128},
      {"name": "flags", "type": "u32", "since": 0, "optional": false,
       "ordinal": null, "default": null, "max_bytes": 0},
      {"name": "text", "type": "str", "since": 0, "optional": false,
       "ordinal": null, "default": null, "max_bytes": 128}
    ]}
  ]
}
```

- [D] `key` follows the service key grammar, at most 128 bytes, and must start with `core.`.
  The file name names the same key and major. `major` is 1..=65535, `revision` is 0..=65535,
  and `since` names an interface minor in 1..=the current minor. A new schema takes an unreleased
  minor. `test_only` marks a schema that device hosts never run.
- [D] Record tags are local to this stage schema, in 1..=511, unique and ascending. Record names
  have the same grammar as `abi.def` record names and are unique within the schema. A record's
  `since` and each field's `since` name a schema revision, at most `revision`. Fields use the
  same types, bounds, optional bitmap and list rules as `abi.def` records. Stage tags do not
  reserve or reuse kernel record tags. Plugin-defined stages publish their own payload schemas;
  the host checks their framing only.
- [D] `input` and `reply` each name 1..=8 distinct records from this schema; no record belongs to
  both. `max_subscribers` is 1..=32, `reply_rows` is 1..=64, and each of `max_input` and
  `max_reply` is 1..=3840 bytes. The generator refuses a schema if one framed input record at
  its field caps exceeds `max_input`, or `reply_rows` times the largest framed reply record
  exceeds `max_reply`.
- [D] `tools/abi-gen.py` emits `stages_wire.rs` to `abi/generated/`,
  `crates/paperpaw-core/src/abi7/` and `crates/paperpaw-sdk/src/native/wire/`. All copies are
  identical. Each `<key>_v<major>` module carries the key, version, bounds, input/reply tags and
  borrowed record codecs; `STAGES` lists the schemas for the host. `input_decode` checks
  exactly one input record and `max_input`; `reply_decode` checks reply tags, `reply_rows`
  and `max_reply`. Both validate the full batch before exposing a borrowed cursor. Unknown
  appended fields stay within the record length and are skipped; bytes after the batch
  are refused. The codecs share the kernel
  records emitter, framing and validation rules, without allocation.
- [D] The ABI lock pins the schema and generated codec. A published schema keeps every key,
  major, record and existing field. New records and appended fields take a newer schema
  revision; the bounds never shrink. A breaking change needs a new major alongside the old one.
- [D] Stages have no `metadata.services` catalog entry: plugins do not open them as services.
  Stage declarations select the stage by key and major. The initial schemas are
  `core.input-filter` (4 subscribers, one verdict), `core.widget` (32 subscribers, up to 32
  target rows) and `core.status` (32 subscribers, up to 8 value rows).

The stage codec is pinned as `emitted_stages`, the SHA-256 of the unformatted emitter
output, in ABI lock format 11. Formatting is checked separately, so a rustfmt update
does not change a frozen digest. Older lock
formats still validate their original emitted sections. This adds no wire draft.

## 3. Memory (X4)

- `service.open`: one `Service` handle (registry slot, about 40 bytes) per declared key in use.
- `service.call`: `input` and `out` are the caller's; the host decodes in place and allocates only
  what the op documents. The schema table is `static` (flash).

## 4. Hand-offs

**Core** (host), in this order:
1. N4: `stage.spec` and `registration.stage` admission (the subscriber count host-wide); slot 32,
   ordering and cursor, `stage.call` framing on the host scratch, framing-only reply checks, the
   host-stamped `stage.reply`, the N8 refusal at depth 2; tests for every status above, a 33rd
   subscriber, a foreign caller (`ERR_DENIED`) and a style grant reaching the owner only as the flag.
2. D4: slots 33 and 34 over the generated `SERVICES` table, the `Service` handle kind, the
   `core.test-echo` service for tests (never registered on a device), tests for every row of both
   tables.

**Services** (SDK): a typed `Stage` caller (cursor loop, `StageReply` view with
`style_granted()`), a subscriber helper (decode `stage.call`, write the reply through `Reply`), and
`Service::open`/`call` with typed wrappers per schema, using the generated table.

**Kernel:** this contract, the records and slots in `abi.def`, the schema generator and its lock
section, and review of each family's schema.
