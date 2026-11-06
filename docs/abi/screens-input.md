# Interface 7 draft 5: screens, input, callbacks and formatting

This is the draft-5 contract for #228 and #241. Each decision is marked [D] and names the `abi.def`
surface it adds or changes. It builds on `proposals/interface-7-events-storage-services.md`
sections 1.2 (D1.3) and 4.2-4.5, and replaces them where they differ. Draft 5 is the one breaking
batch before 1.0 (ADR 0034). Everything that comes after it is additive.

## 0. Strategy (Q19)

- [D] **Plugin-side drawing.** A plugin draws its screens itself over the canvas slots (9-12), the
  text slots (13, 14) with font handles (7, 8), and formats its texts through i18n (6, 27, 28). The
  SDK's widgets compile into the plugin. No host widget entries in 7.0; they can be added later as
  new slots without changing anything here. (Overridable by the owner; recorded for the freeze
  audit.)

## 1. Screens and actions are declarations

Records in the declaration range (5-31, growth block 224-511), applied by S2 like settings (image-borrowed where the
batch lies in the image, owned otherwise; refused at activation until the SDK views land).

| Tag | Record | Fields | Caps |
| --- | --- | --- | --- |
| 11 | `screen.spec` | `name: str` (relative), `title: str` (i18n key), `token: u32`, `flags: u32` | 96, 128 |
| 12 | `action.spec` | `name: str` (relative), `title: str` (i18n key), `token: u32`, `flags: u32` | 96, 128 |

- [D] `token` names the plugin callback state the host passes to `draw`, `input` and
  `dispatch_event` for this screen or action. Tokens are the plugin's own numbers; one token may
  serve several screens.
- [D] `screen.spec.flags`: bit 0 `full_refresh_on_open`; other bits must be 0 (`ERR_UNSUPPORTED`).
  `action.spec.flags`: bit 0 `needs_screen` (the action only runs while one of this plugin's
  screens is on top); other bits 0.
- [D] Names follow the declaration name rules (`records.md`); full id = `<plugin>.<name>`. Unique
  per plugin across screens and actions of the same kind; checked at commit, order-independent.
- [D] Running an action delivers `event.action_run` (tag 39) to its token through
  `dispatch_event`; the plugin answers with commands.

## 2. Input and screen events

### 2.1 Input callback (appended to `PpPlugin`)

```
input(state: pointer, token: u32, input: bytes) -> i32
```

- [D] Returns 1 = handled, 0 = not handled, negative = error (logged by the shell, treated as not
  handled). Any other positive value is an error. `nullable`: a plugin without screens leaves it
  null; input then goes to the next layer as not handled.
- [D] Called only for the token of the screen on top; never re-entered (same admission as
  `dispatch_event`); commands submitted inside run after it returns.
- [D] `input` is one batch of exactly one record (decode with `records_decode_single`).

### 2.2 Records

| Tag | Record | Fields |
| --- | --- | --- |
| 38 | `event.screen_shown` | `now_ms: u64`, `screen: str 128` (full id), `width: u32`, `height: u32` |
| 39 | `event.action_run` | `now_ms: u64`, `action: str 128` (full id) |
| 40 | `input.key` | `now_ms: u64`, `key: str 32`, `state: u32` (0 press, 1 release, 2 repeat) |
| 41 | `input.pointer` | `now_ms: u64`, `x: i32`, `y: i32`, `phase: u32` (0 down, 1 move, 2 up, 3 cancel) |
| 42 | `input.gesture` | `now_ms: u64`, `kind: u32` (0 tap, 1 long press, 2-5 swipe left/right/up/down), `x: i32`, `y: i32` |
| 43 | `input.action` | `now_ms: u64`, `action: str 128` (the kernel action the reader's key or touch mapped to, e.g. `core.page-next`) |

- [D] Tag families: `event` 32-39 and `input` 40-43, each with a growth block (section 8).
- [D] Key names are the kernel's keymap names (`docs/keymaps.md`); an unknown name is passed
  through, plugins ignore keys they do not know. Coordinates are screen pixels of the
  screen's own pane, origin top left.
- [D] `event.screen_shown` goes to the screen's token (through `dispatch_event`) each time it
  comes to the top or its pane size changes; a plugin re-acquires stale font handles there.
  There is no "hidden" event: a screen that is not on top gets no `draw` and no `input`.
- [D] Unknown enum values (state, phase, kind) are refused by decoders like every wire rule;
  producers never send them.

## 3. Commands beyond `command.set_setting`

| Tag | Record | Fields |
| --- | --- | --- |
| 56 | `command.open_screen` | `screen: str 128` (full id) |
| 57 | `command.run_action` | `action: str 128` (full id) |
| 58 | `command.redraw` | `mode: u32` (0 redraw only, 1 fast, 2 partial, 3 full) |
| 59 | `command.close_screen` | (no fields: closes the plugin's own top screen) |

- [D] Authority (plugin freedom, no sandbox): `open_screen` and `run_action` may name any
  registered screen or action, own or foreign. A malformed id is `ERR_INVALID`, an id that is not
  registered `ERR_NOT_FOUND`; the whole batch is refused and nothing is queued. The owner is
  stamped by the host, never read from the record.
- [D] `redraw` and `close_screen` act on the submitting plugin's screen on top; outside its own
  screen they are `ERR_STATE` (whole batch refused).
- [D] Order: a batch is validated whole, then queued; queued commands run after the callback
  returns, in submit order, batches in order. Several `redraw` in one callback merge to the
  strongest mode.

## 4. Fonts and text

- [D] `font.acquire(key)` keys: a registered font id (`Fonts::get` grammar) or a **role key**:
  `role:ui`, `role:ui-bold`, `role:reading`, `role:mono`. A role follows the reader's choice at
  the time of each text call (re-resolved per call); an unknown role is `ERR_NOT_FOUND`.
- [D] Handles: at most 4 per plugin (`ERR_FULL` beyond); `handle.release` frees; teardown frees
  all. A handle whose font id is gone (pack removed or replaced) answers `ERR_STATE` on
  `text.*`; the plugin acquires again (on `event.screen_shown`).
- [D] `text.measure`/`text.draw` keep their signatures; `size` finite and > 0, text at most 4096
  bytes UTF-8, `text.draw` needs a canvas (inside `draw`), else `ERR_STATE`. `text.draw` is not
  atomic on an advance error (documented).

## 5. i18n formatting, plurals and digits

| Id | Entry | Arguments (after `ctx`) | Result |
| --- | --- | --- | --- |
| 27 | `i18n.format` | `key: bytes` (<= 128), `args: bytes` (<= 1024), `out: buffer` | length |
| 28 | `i18n.digits` | `text: bytes` (<= 4096), `out: buffer` | length |

| Tag | Record | Fields |
| --- | --- | --- |
| 44 | `i18n.arg_str` | `name: str 32`, `value: str 256` |
| 45 | `i18n.arg_int` | `name: str 32`, `value: u64` (two's complement `i64`) |
| 46 | `i18n.count` | `value: u64` (two's complement `i64`) |

- [D] `args` is a record batch (<= 8 records). No `i18n.count`: `Translator::format`; exactly one:
  plural by the catalog's rule (`key.<category>`, fallback `key.other`) with `{count}` bound.
  Two counts, a duplicate name: `ERR_INVALID`; unknown tag: `ERR_UNSUPPORTED`.
- [D] A missing key is `ERR_NOT_FOUND` with the output unchanged, as slot 6 since D5.
- [D] Result as slots 5/6: the full length is returned, at most `cap` bytes written; the caller
  retries with a bigger buffer. `i18n.digits` maps ASCII digits to the UI language's digits
  (may grow the byte length).
- [D] Host builds into a bounded scratch; validation is complete before the first lookup.

## 6. Callback replies and draw input (N1, N2)

```
dispatch_event(state: pointer, token: u32, input: bytes, out: buffer) -> i32
draw(state: pointer, token: u32, input: bytes) -> i32
```

- [D] **Reply buffer (N1).** The host lends `out` for the duration of the call. A result >= 0 is
  the number of reply bytes written; a result < 0 is a status. A result greater than `out.cap` is
  a plugin fault: the host treats it as `ERR_INVALID` and reads no reply. The plugin may write
  only the first `out.cap` bytes. It must not keep `out` after returning.
- [D] Each dispatched record family defines its reply. Families without a reply get `cap = 0`,
  so their only valid results are 0 and a negative status. All tags 32-39 are such families.
  Stages (N4) are the first families with replies.
- [D] The reply is one record batch in the reply schema of the dispatched family. The host checks
  only the framing (count, lengths, caps); the receiving side decodes it with its own decoder.
- [D] `commands.submit` stays the path for side effects. A reply is an answer, never a command.
- [D] **Draw input (N2).** `draw` gets one batch of exactly one `draw.frame` record:

| Tag | Record | Fields |
| --- | --- | --- |
| 192 | `draw.frame` | `purpose: u32`, `x: i32`, `y: i32`, `width: u32`, `height: u32`, `clip_top: i32`, `clip_bottom: i32`, `id: u32`, `payload: bytes 256` |

- [D] `x`, `y`, `width` and `height` give the target rectangle in screen pixels. `clip_top` and
  `clip_bottom` give the rows this call may change: on the X4 these are the band being rendered,
  elsewhere the whole rectangle.
- [D] `purpose`: 0 screen, 1 page layer, 2 inserted block, 3 status item, 4 widget. Other values
  are reserved: a plugin that does not know the purpose draws nothing and returns 0.
- [D] `id` and `payload` are defined by the owner of the purpose, for example the reader's insert
  id. A screen gets `id = 0` and an empty payload.
- [D] Drawing outside the clip is clipped by the canvas slots, not refused.

## 7. Lifetime and cancellation (all handles and async operations)

This rule binds every handle and every asynchronous operation of interface 7, present and future:
font handles, blobs, images, format and document handles, work, HTTP, sockets and stage
subscriptions. Families added later refer to it instead of defining their own.

- [D] Every handle is released exactly once: by the plugin, or by the host when the plugin stops
  or its construction is torn down. A second release, or a release of a handle the plugin does not
  own, is `ERR_STATE` and changes nothing.
- [D] A provider stopped in the middle of an operation gets one cancel event for it and is never
  called back for that operation again. Requesters see the operation end with `ERR_CANCELLED`.
- [D] A late reply or completion (after cancel, stop or timeout) is refused with `ERR_STATE`, and
  the host drops it unread.
- [D] Stop order: the host cancels open operations, then releases handles, then drops the state.
  No callback runs after `drop`.
- [D] **Async operations (D3, #890).** Every asynchronous operation is a handle. Its completion
  event is a level-triggered hint: it may be coalesced and never carries the only copy of the
  result. The result stays readable through the handle until `handle.release`. Stop or unload
  cancels every open operation, and nothing is called back afterwards; a late completion is
  refused (`ERR_STATE`). Releasing the handle of a running operation cancels it first. Live
  handles and operations per plugin are capped; one past the cap is `ERR_FULL`. SDK glue builds on
  exactly this.
- [D] **Dropped events (D6).** `event_dropped` applies only to families whose contract says they
  may drop (today: the lifecycle events of section 1.5 in the events proposal). Other families
  deliver or fail the request; the lifecycle drop rule does not generalise.

## 8. Tag space (N3)

| Family | Ranges |
| --- | --- |
| registration, tick | 1-3 |
| `command.set_setting` | 4 |
| declarations (`setting`, `resource`, `action`, `menu`, `screen`, `icon`, `format`, `registration`) | 5-31, 224-511 |
| `event` | 32-39, 64-191 |
| `input` | 40-43, 192-223 (`draw` shares the block) |
| `i18n` | 44-47 |
| `service` | 48-55, 1024-1535 |
| `command` | 56-62, 1536-2047 |
| `stage` | 512-1023 |

- [D] **Decoders classify records by exact tag only, never by range.** A record that a 1.0 plugin
  does not know is unknown to it (`ERR_UNSUPPORTED` from the decoder), wherever its tag lies.
  `tools/tests/test_abi_tags.py` pins this for the generated codecs, the host and the SDK.
- [D] The ranges only check where a schema author places a new record (`TAG_RANGES` in
  `tools/abi_declarations.py`). Tags up to 65535 encode as LEB128.
- [D] Stage payloads use tags local to their stage schema, not kernel tags. The defining plugin
  publishes the schema; the host checks framing only.

## 9. Pages and progress (N5)

These rules bind every producer and consumer of `event.page_turned` and of a reading position.

- [D] `progress_ppm` measures the **source text** before the page's start position. Inserted
  content counts zero.
- [D] `pages` counts the laid-out pages, including pages of inserted content.
- [D] A layout's key includes the signature of every stage subscriber that may change layout. A
  signature change rebuilds the page index; the current page stays anchored at its text position.
- [D] A position is `(section, offset, insert)`. `insert` 0 is text; k is the k-th inserted item
  anchored at that offset. A page may start inside inserted content only at an item boundary, so
  two pages never share a position. `event.page_turned` carries `insert` as an optional field
  (absent = 0). Consumers that only follow text, such as sync and statistics, ignore it, which is
  correct because progress is text-based.

## 10. Input limits in SDK glue (N7)

- [D] SDK callback glue checks only null and length consistency of `input`. It never compiles in
  a fixed upper limit such as 4096. Decoders enforce each family's caps, so a later family with
  bigger records reaches old binaries through the decoder, which refuses only what it cannot read.

## 11. Nesting (N8)

- [D] Dispatch depth 2 is final: a host callback may cause one nested plugin callback, no more. A
  stage subscriber that calls `stage.run` gets `ERR_STATE`. This keeps the X4 stack bounded.

## 12. User grants: styles and installs (owner decisions 131, 132, 134)

- [D] A plugin may override the user's reading settings (font, size, colour, spacing and so on)
  only when the user has granted that plugin the right (decision 131). Without the grant the
  user's setting wins, and the host or reader drops the override. Style stages (N4) carry this
  rule.
- [D] Installs and updates that a plugin triggers always need a host confirmation, unless the user
  has exempted that plugin (decision 132). A firmware change a plugin triggers always needs the
  confirmation, even for an exempted plugin (decision 134). The confirmation flow belongs to the
  DEV family.
- [D] **Guarded grants.** User grants are host-held guarded settings under
  `core.grant.<kind>.<plugin id>` (`kind`: `style`, `install`). No plugin can ever write them:
  `command.set_setting`, a preview or a preset touching `core.grant.*` is `ERR_DENIED` and refuses
  the whole batch, for every plugin including the one the grant names. A plugin may read only its
  own grants, as yes/no; reading another plugin's grant is `ERR_DENIED`.

## 13. Statuses, features and service families (D1, D2, D4, D5)

- [D] **New statuses (D2).** `ERR_DENIED` (-12): the plugin did not declare the feature, or the
  user refused (consent, a guarded grant). `ERR_TIMEOUT` (-13): a network or socket operation ran
  out of time. Both stay distinct from `ERR_UNSUPPORTED` (the host lacks the function) and
  `ERR_CANCELLED` (dismissed, stopped). SDK status types name both from 1.0 on.
- [D] **Feature records (D1).** A plugin states the families it uses as metadata feature records,
  `uses.<family>` (`uses.net`, `uses.secrets`, `uses.sockets`, `uses.library`, `uses.registry`,
  `uses.device`). Bit 0 of the record's `flags` means required: a host that does not know a
  required key refuses the load and names the key; a host ignores an unknown key without the bit.
  Other bits are refused. See `abi/metadata.md`.
- [D] **Service families (D4).** Every later family (NET, LIB, REG, FS, SEC, DEV and the rest) is a
  versioned service schema behind `services.query`, not a set of one-off slots. Calls go through
  one generic slot, `service.call(ctx, service, op: u32, input, out) -> count` (synchronous and
  bounded); asynchronous work returns an operation handle under section 7. The slot and its
  generator path come in the additive follow-up to this draft.
- [D] **Limits (D5).** A plugin may require up to 32 services (was 16) and 32 features.

## 14. Memory (X4)

- Input/screen records and `draw.frame` are encoded on the host stack (a framed `draw.frame` is at most about 300 bytes).
- Screens/actions: S2 index entries only (image path); about 16 bytes resident each.
- i18n: one 4 KiB host scratch per call, none retained.
- Reply buffers are lent from the existing host scratch, nothing per plugin.

## 15. Work after this contract

- Host adapters (Core):
  - screen/action apply (S2);
  - `input` dispatch and handled routing;
  - `dispatch_event` with the lent `out` and `draw` with `draw.frame`;
  - command execution for 56-59;
  - font role resolution;
  - slots 27/28;
  - section 7 teardown order.
- SDK (Services/Plugin):
  - screen/action declaration builder entries;
  - `native::Plugin::input`;
  - reply writing and `draw.frame` decoding;
  - input/event decoding;
  - i18n/fonts wrappers;
  - section 10 in the event glue.
- Follow-up PR (additive at draft 5): N4 stages and N6 images.
