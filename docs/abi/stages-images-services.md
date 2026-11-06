# Interface 7: stages (N4), images (N6) and `service.call` (D4)

Status: contract for the additive follow-up at draft 5 (#228, #241, #890). Everything here is
appended (new records, new `PpHost` slots, new service schemas); nothing in the blessed draft-5
surface changes. Items marked [D] are decided; [O] are open and named with their owner.

It builds on `screens-input.md`:
- the reply buffer (section 6);
- lifetime and cancellation (section 7);
- tag space (section 8);
- pages (section 9);
- nesting (section 11);
- grants (section 12);
- D1/D4 (section 13).

Senior's family notes (`c-families-228.md` sections 2.1, 2.6, D3, D4) are the background. Where
this file is more specific, it wins.

Slot ids below are the next free `PpHost` entries in this order (29-33); the generator assigns
them by append order. If another family is appended first, the ids move and nothing else does.

## 1. Stages (N4)

A stage is a named extension point owned by one plugin, for example the reader's `reader.insert`.
The host is the broker: it orders subscribers, admits them, bounds their replies and enforces
nesting. It never reads the payloads.

### 1.1 Records

| Tag | Record | Fields |
| --- | --- | --- |
| 224 | `stage.spec` (declaration) | `name: str 96`, `schema: u32`, `flags: u32`, `max_input: u32`, `max_reply: u32`, `max_records: u32` |
| 225 | `registration.stage` (declaration) | `stage: str 128` (full id), `token: u32`, `order: i32`, `signature: u64` |
| 512 | `stage.call` (input to a subscriber) | `stage: str 128`, `schema: u32`, `payload: bytes 4096` |
| 513 | `stage.reply` (output of `stage.run`) | `subscriber: str 64` (plugin id), `index: u32`, `status: i32`, `flags: u32`, `signature: u64`, `payload: bytes 4096` |

- [D] **`stage.spec`:**
  - `name` is relative; the stage's full id is `<plugin>.<name>`.
  - `flags`: bit 0 `changes_layout`, bit 1 `draws`, bit 2 `takes_taps`; other bits must be 0
    (`ERR_UNSUPPORTED`).
  - `max_input` and `max_reply` are each in 1..=4096, and `max_records` in 1..=64.
  - The name is unique among the plugin's stages (kind 7 in the declaration index).
- [D] **`registration.stage`** subscribes the plugin's `token` to a stage by full id.
  - A registration naming a stage that is not installed is kept and stays inert. Stages come and
    go with plugins, so this is never a load error.
  - At most 32 subscribers per stage, counted host-wide. The 33rd registration is refused at
    commit with `ERR_FULL`, naming the stage.
- [D] Tags 224/225 are in the declaration block, and 512/513 open the `stage` block (section 8).
  Payload tags are local to the stage's schema (`paperpaw-reader-api` for the reader's stages).

### 1.2 Running a stage

```
stage.run(ctx, stage: bytes, cursor: u32, input: bytes, out: buffer) -> i32   // slot 29
```

- [D] **Only the stage's owner runs it.** Any other caller gets `ERR_DENIED`. Host-defined stages
  (`core.*`) are run by the host only.
- [D] **One subscriber per call, by cursor.**
  - `cursor` 0 is the first subscriber in stage order: `(order, plugin id, registration index)`,
    ascending and stable while no plugin is added or removed.
  - The call runs exactly the subscriber at `cursor` and writes one `stage.reply` record into
    `out`. It returns the bytes written.
  - A cursor past the last subscriber returns `ERR_NOT_FOUND` and writes nothing.
  - The caller loops until it gets that. So the X4 needs exactly one reply buffer, never
    subscribers × `max_reply`.
  - `index` in the reply echoes `cursor`.
- [D] **What the subscriber gets.** It is called through `dispatch_event` with its token:
  - `input` is one `stage.call { stage, schema, payload = the caller's input }`.
  - The lent `out` has `cap = max_reply` of the stage (section 6 of `screens-input.md`).
  - Its result is framed by the host into the `stage.reply` payload, which is empty for a
    negative status.
- [D] **What the host checks.**
  - The caller's `input` must be at most `max_input`.
  - The subscriber's reply framing is checked only: a record batch, count at most
    `max_records`, length at most `max_reply`.
  - On a violation, that subscriber's reply has `status = ERR_INVALID` and an empty payload, a
    log line names the subscriber, and the stage goes on. The caller decodes payloads with the
    stage's own decoder.
- [D] **Status in `stage.reply`.**
  - 0 or more: OK, with `payload` holding the reply.
  - The subscriber's own negative status.
  - `ERR_CANCELLED`: the subscriber plugin is stopped or stopping.
  - `ERR_BUSY`: the subscriber is already in a callback (re-entry is refused, as for every
    `dispatch_event`).
  - `ERR_INVALID`: framing violation.

  An unreachable subscriber is never skipped silently. The caller sees its reply with a status
  and moves on.
- [D] **`flags` in `stage.reply` are host-stamped, never from the subscriber.**
  - Bit 0 is `style_granted`: the user's `core.grant.style.<subscriber>` is yes (decision 131).
    This is the only way the stage owner learns a subscriber's grant, because grants are
    own-only (section 12).
  - The reader applies a subscriber's override of a user-protected value only when this bit is
    set (`paperpaw-reader-api` `may_override`).
  - The other bits are 0 today; consumers ignore bits they do not know.
- [D] **`signature`** echoes the subscriber's `registration.stage.signature`. For a
  `changes_layout` stage, the owner folds every subscriber's signature into its layout key
  (section 9). A plugin changes its signature by re-registering, which takes effect at its next
  start.
- [D] **Nesting (N8).** `stage.run` runs at depth 1 only. A subscriber (depth 2) that calls
  `stage.run` gets `ERR_STATE`. `service.call` is allowed at depth 2.
- [D] **Lifetime.** `stage.run` is synchronous; nothing about a call outlives it. A subscriber may
  not keep `stage.call` borrows (section 7).
- [O] **Async stage replies** (lookup providers that fetch from the network; Senior's 2.1) are a
  later additive step. A subscriber then answers with an operation handle under section 7
  instead of a payload. It is not needed for wave 2. Owner: Kernel, with the dictionary port.

### 1.3 Host-defined stages

- [D] `core.input-filter` and `core.widget` (Senior 2.1) are `core.*` stages with schemas in
  `abi/stages/*.def`, generated like the reader's (`tools/abi_stage_codec.py`). They come with
  their families (UIX, widgets), not with N4 itself.
- [D] Escape chord: holding Power for 8 s is never filterable. The host checks it before any
  `core.input-filter` subscriber.

### 1.4 Memory (X4)

- Per subscriber: one declaration-index entry, about 16 B resident.
- Per `stage.run`: the caller's `out` and one host-framed `stage.call` on the host scratch
  (4 KiB, already present). Nothing is retained between calls.

## 2. Images (N6)

### 2.1 Slots and records

```
image.info(ctx, path: bytes, out: buffer) -> i32                                    // slot 30
image.open(ctx, path: bytes, width: u32, height: u32, flags: u32, out: *u32) -> i32  // slot 31
canvas.draw_image(ctx, image: u32, x: i32, y: i32, width: u32, height: u32, flags: u32) -> i32 // slot 32
```

| Tag | Record | Fields |
| --- | --- | --- |
| 193 | `image.info` (reply of slot 30) | `width: u32`, `height: u32`, `format: u32` (1 JPEG, 2 PNG, 3 BMP, 4 GIF first frame) |
| 64 | `event.op_done` | `op: u32`, `status: i32` |
| 60 | `command.invalidate` | `x: i32`, `y: i32`, `width: u32`, `height: u32` |

- [D] **`path`** is a path under one of the plugin's storage roots (decision 125, `uses.storage`),
  or the current book's resource path as the reader gets it from the DOC family.
  - A path outside the plugin's roots is `ERR_DENIED`.
  - A missing file is `ERR_NOT_FOUND`.
  - A format `paperpaw-image` cannot decode is `ERR_UNSUPPORTED`.
- [D] **`image.info`** reads the header only, synchronously, bounded by the format's header size.
  It writes one `image.info` record (N1 rules for `out`). A placeholder uses it to reserve the
  exact size before layout.
- [D] **`image.open`** is an asynchronous operation (section 7, D3).
  - It returns `OK` and writes an image handle to `out` at once.
  - The host decodes in the background into the `paperpaw-image` gray format, scaled to fit
    `width` × `height` while keeping the aspect ratio (`flags` bit 0: fill and crop instead).
    The decoded image goes into the card cache.
  - Completion is `event.op_done { op = the handle, status }`, delivered to the plugin's event
    token for tag 64, which the plugin registers for with `registration.event`.
  - The event is a level-triggered hint; the state stays readable by drawing or by
    `handle.release`.
  - `handle.release` on a running decode cancels it.
  - Live image handles per plugin: at most 4 (shared cap of section 7), then `ERR_FULL`.
- [D] **`canvas.draw_image`** is valid only inside `draw`; outside it, it is `ERR_STATE`.
  - It draws the decoded image into the given rectangle, clipped to the current `draw.frame`
    clip. On the X4 it is called once per band, and the host reads only that band's rows from
    the cache.
  - A handle still decoding draws nothing and returns `ERR_BUSY`.
  - A failed decode returns that status.
  - `flags` must be 0 today.
- [D] **`command.invalidate`** asks for a redraw of the rectangle on the submitting plugin's own
  screen, or of a region it owns in the reader (insert, layer). The rectangle is clipped to the
  plugin's region. Several in one callback merge into their bounding box. Outside its own screen
  or region, it is `ERR_STATE` (whole batch refused).

  The typical flow:
  - on `event.op_done` for an image, invalidate the reserved rectangle;
  - the next `draw` then draws the image.
- [D] **Memory (X4):** decoding streams through the existing `paperpaw-image` band decoder into
  the card cache. Nothing stays resident per image beyond the handle entry. The cache entry may be
  evicted after `handle.release`.
- [O] Tag numbers 193/60/64 are proposals, checked against the tag table by the generator when the
  records are added (Kernel).

## 3. `service.call` (D4)

```
service.call(ctx, service: u32, op: u32, input: bytes, out: buffer) -> i32   // slot 33
```

- [D] **`service`** is a handle from `services.query` (slot 17). It is owned by the plugin and
  released through `handle.release`.
- [D] **`op`** is the service-local operation number.
  - An op the service schema does not define is `ERR_UNSUPPORTED`.
  - An op the plugin's declared features do not allow is `ERR_DENIED`.
- [D] **`input`** is one record batch with service-local tags, checked with the service's
  generated decoder (exact tags). **`out`** follows N1 (non-null, zeroed, never overlapping, one
  per depth); the result is the reply length, or a negative status.
- [D] **Synchronous and bounded.** A call never waits on the network or the card beyond the
  service's stated bound. Anything longer returns an operation handle (section 7, D3) whose
  completion is `event.op_done`, and whose result is read with a further op of the same service
  (`op.state`) or `blob.read`.
- [D] **Nesting:** allowed at depth 1 and 2. Re-entry into the same service instance from its own
  callback is `ERR_BUSY`.
- [D] **Service schemas** live in `abi/services/<key>.def`. Each has a major and minor, op numbers,
  input/reply records with local tags, and its bounds. They are generated with
  `tools/abi_stage_codec.py` into core (decoder) and SDK (encoder and typed wrapper).
  - The ABI lock pins them like records: append-only, never renumbered.
  - `services.query(key, major, minor)` finds the schema.
- [D] **First services** (from Senior's waves): `core.storage` metadata (FS), `core.registry`
  (REG), `core.library` (LIB), `core.secrets` (SEC), `core.net` (NET), `core.device` (DEV), each
  in its own PR, additive at draft 5.

## 4. Hand-offs

**Core** (host), one PR each, in this order. Each depends on the one before only where noted.

1. **N4 `stage.run`:**
   - `stage.spec` and `registration.stage` admission (index kind 7; the subscriber count host-wide);
   - slot 29;
   - subscriber ordering and cursor;
   - `stage.call` framing on the host scratch;
   - framing-only reply checks;
   - host-stamped `stage.reply` (subscriber, status, `style_granted`, signature);
   - N8 refusal at depth 2;
   - tests for every status row in 1.2, a 33rd subscriber, a foreign caller giving `ERR_DENIED`,
     and a style grant reaching the owner only as the flag.
2. **N6 images:**
   - slots 30-32 over `paperpaw-image`;
   - op handles in the shared registry (section 7, `Teardown` hooks);
   - `event.op_done`;
   - `command.invalidate`;
   - tests: info, open, op_done, draw, release cancels, the cap with `ERR_FULL`, and a path
     outside the roots giving `ERR_DENIED`.

   This is also the first family that creates handles, so it adds the device `Handles<Resource>`
   registry and wires `cancel` and `release`.
3. **D4 `service.call`:** slot 33 and the generator path for `abi/services/*.def`. Kernel writes
   the generator part (`tools/abi-gen.py`, lock section). Core writes the host dispatch and a
   `core.test-echo` service used only by tests.

**Services** (SDK):
1. a typed `Stage` caller (cursor loop, `StageReply` view with `style_granted()`) and a subscriber
   helper (decode `stage.call`, write the reply through `Reply`);
2. `Image` (info, open, draw, release on drop);
3. `ServiceHandle::call` with typed wrappers generated per service schema.

Each follows `screens-input.md` sections 6-7 and comes with the interface diff for Kernel's
review.

**Kernel:**
- records and slots in `abi.def` (tags above);
- the generator path for service schemas and its lock section;
- review of each PR's `abi.def`, lock and interface files.

Size [E]:
- N4: Core about 12-16 h, Services about 6 h.
- N6: Core about 14-18 h (with the registry), Services about 5 h.
- D4: Kernel about 6 h, Core about 8 h, Services about 6 h.
