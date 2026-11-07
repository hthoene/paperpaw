# Interface 7 family UIX: screens, dialogs, input, composition (draft)

Contract draft for #228 (assignment #1002). Items marked [D] are proposed decisions, [Q] open
questions for Plugin and Senior. Inputs: the plugin needs inventory (#984,
`plugin-needs-reg-uix-dev-lib-doc-228.md`, UIX section), Plugin's per-plugin call sites (#1003,
`plugin-uix-calls-1003.md`), `screens-input.md` (screens, actions, input, draw, commands 56-60),
`stages-services.md` (stages, services), `registry.md` (REG; plugin inputs listed by op 14, the
hold-Power escape) and `core-events.md`. Doc-only: records, schema and generator follow once this
is approved, as for EVT and REG. Tags below are proposed; the records step numbers them without
gaps (commands 1553-1555, 1559 and 1560 and events 83, 88 and 90 are free after the review rounds).

## 0. Scope and principles

- [D] **Plugin-side drawing stays (Q19).** Dialogs, the keyboard, lists, progress bars and
  widgets are plugin screens drawn by plugins. The host owns only what must be shared or trusted:
  the screen stack and its instances, routing of questions and answers, input filters and guards,
  composition of one plugin's drawing inside another's, and the few outputs a plugin cannot make
  itself (a message shown over any screen, the clipboard, speech).
- [D] **Answers go to the asker only.** A question gets a host request id that the asker learns
  at once (section 3); a speech request carries the asker's own `cookie` (a
  `u32`, like a declaration `token`). The host records who asked and delivers the answer to that
  plugin alone. No plugin can read or consume another
  plugin's answer; an answer is never dropped for lack of room (section 3.2) and never arrives
  after the asker stopped.
- [D] **Guards win.** A screen that guards the device (the lock) owns the input path and the
  stack above it, and hides everything below it from every other plugin (section 4.3).
- [D] **The hold-Power escape is untouched.** The host times the Power key on the raw device
  input, whatever any filter, guard, screen or dialog answers: Power key records never reach a
  filter (they go past the filter chain to the host's own handling), a filter's `replace` or
  `raise` can never produce a Power key record (such a verdict counts as pass, logged), and
  `raise_input` cannot name it (`ERR_DENIED`). An escape while a plugin callback hangs stays the
  REG device check (`registry.md` section 4.2).
- [D] **No interface-6 shapes.** `Params`, `ScreenKey`, `Emit`-based replies and `Env::stack` have
  no interface-7 copies; what they carried is in the records below.
- Out of 1.0 (no first-party plugin uses them; additive later): chrome bars (`ChromeSpec`),
  refresh filters (`RefreshFilterSpec`), sound playback, task switching (`SwitchTask`).
  [Q] Plugin: confirm none of these is needed by a port.

## 1. Screen instances and parameters

### 1.1 Instances

- [D] Each opening of a screen is an **instance**: a host-assigned `u32` from a checked counter,
  never 0 and never reused while the host runs (on the X4 until restart; at `u32::MAX` opens are
  `ERR_FULL` until restart). Request and hand-over ids (sections 3 and below) follow the same rule,
  from one counter, so a host text id (op 9, 10) names one request or one hand-over. The same screen
  may be open more than once with different parameters. An instance is what interface 6 called
  `ScreenKey`.
- [D] The `draw` and `input` callbacks keep their signatures. The host calls them only for the
  instance on top (and for composed instances, section 5), and it announces which instance is on
  top before the first `draw`: `event.screen_shown` (38) has the field `instance` (38 is not
  frozen, so it is a plain field of its interface-7 form). A plugin keeps per-instance state keyed
  by it.

| Tag | Record | Fields | To |
| --- | --- | --- | --- |
| 38 | `event.screen_shown` | its fields, among them `instance: u32` | the screen's token |
| 79 | `event.screen_opened` | `now_ms: u64`, `screen: str 128`, `instance: u32`, `slot: str 128`, `flags: u32`, `handover: u32`, `params: u32`, `text_bytes: u32` | the screen's token |
| 80 | `event.screen_closed` | `now_ms: u64`, `screen: str 128`, `instance: u32` | the screen's token |
| 81 | `event.stack_changed` | `now_ms: u64` | subscribers (hint) |

- [D] `screen_opened` comes once per instance, before its first `screen_shown`, and **again**
  whenever an open instance gets a new hand-over (`open` mode 1 or 3 shows it again; `flags` bit 0
  `again`), so a screen never misses a parameter update. `flags` bit 1 `restored`: reopened after
  a restart (#275), params kept, no text. `handover` names the instance's hand-over (0 none),
  `params` its param count and `text_bytes` its text's length. `screen_closed` comes once, after its
  last `draw`; a plugin frees the instance's state there. Like 38 and 39, 79 and 80 are not
  registrable (`ERR_UNSUPPORTED` in `registration.event`): they go to the token of the screen
  they name.
- [D] **Params are lossless** (interface 6 `Params`, Senior's answer to [Q] 3): an opener makes a
  **hand-over** with `core.ui` op 16 `handover` (input: `ui_param` records, reply `ui_handle`) and
  names it in `command.open.handover`. A `ui_param` (`key: str 32`, `value: str 1024`) is one
  `key=value` pair, a service row of the UIX block used only in `core.ui` inputs and replies (op 16
  in, op 17 out, `ui_status_ask.context`), never a declaration. At most 32 params per hand-over and
  at most 1,024 bytes of keys and values in all (the content bytes; encoded with fields and frames
  at most 1,313, section 8.1; tests at 1,024 `OK` and 1,025 `ERR_NO_SPACE`); keys
  `[a-z0-9_.-]{1,32}`, unique; a value is any UTF-8, newlines included. The host checks the form at
  op 16 (`ERR_INVALID`, over the cap `ERR_NO_SPACE`) and keeps them unchanged; their meaning belongs
  to the screen that reads them. Keys starting with `core.` are the host's (section 3:
  `core.request`, `core.kind`): op 16 naming one is `ERR_INVALID`.
- [D] **Long values** (the reader's selected text, interface 6 `params::TEXT`): a hand-over also has
  one host text (section 3.1 rules: at most 24,576 bytes, written in 3 KiB parts with op 9
  `text_write` on the hand-over id by its maker in the callback that made it, never cut; a begun but
  uncommitted text makes the `open` naming it `ERR_STATE`). The opened screen reads its params with
  op 17 `params` (`ui_handle` → `ui_param`*, `end`) and its text with op 10, at any time while it is
  open. So any text another plugin selected reaches the screen whole, without either plugin's Data.
  A hand-over the caller did not name in an applied `open` is freed when its callback returns; a
  refused batch frees it too.
- [D] **The host keeps each instance's hand-over** (fallibly, `ERR_NO_MEMORY` at op 16) for as
  long as the instance is open, or until a reopen gives it a new one: `open` modes 1 and 3
  compare the params, and reopening screens after a restart (#275) opens them with the same params
  (not the text). On the X4 the params of all instances share 8 KiB and texts are in the host
  text pool (section 8.2).
- [D] `stack_changed` is a hint: a screen opened, closed or came to the top. It carries nothing;
  the stack is read with `core.ui` op 1 (section 6).

### 1.2 Declaration flags

- [D] `screen.spec.flags` (11) gains, beside bit 0 `full_refresh_on_open`:
  - bit 1 `guards`: the screen guards the device (section 4.3) from its opening on;
  - bit 2 `overlay`: shown over the screen below (a menu, a dialog, the sleep screen over the
    page). For each frame the host first draws the topmost non-overlay instance below it and any
    overlays between, each as if on top (depth 1, with its widgets and status values, nothing
    lost), then the overlay over that. No nesting, so no extra stack. Under another plugin's
    overlay, an instance that is secret or at or below a guard is not drawn (the area stays
    blank): the sleep screen over a locked device never shows the page or the PIN screen;
  - bit 3 `waits_ready`: help for this screen waits until the screen says it is ready
    (`command.screen_ready`, section 7);
  - bit 4 `secret`: what the screen shows is secret from its opening on (a PIN, a password);
  - bit 5 `holds_edges`: edge swipes wait while the screen holds them.
  Other bits stay `ERR_UNSUPPORTED`.
- [D] **Policies change at run time** (the lock guards only while locked): `command.screen_policy`
  (`instance: u32`, 0 for the own top one; `flags: u32` with bits 1, 4 and 5 above) sets them for
  one of the submitter's own instances; other bits `ERR_INVALID`, another plugin's instance
  `ERR_DENIED`. **Turning `guards` on** is allowed only for the submitter's own top instance (the
  top of the whole stack, so no other plugin's screen is above it; the host's own `core.sleep`
  instance does not count, so a lock that locks while the sleep screen shows guards at once):
  anywhere else `ERR_STATE`, and the whole batch is refused. Turning it off, and bits 4 and 5 either
  way, work on any own instance. The declared flags are the state at opening; a screen declared
  `guards` that opens below a guard waits there (section 4.3) and never has screens of others above
  it. A change takes effect when the batch is applied, before the next input is routed. Core's tests
  cover both cases (own top: guards at once; not top: `ERR_STATE`, nothing changed).
- [D] **Secret** (interface 6 `Screen::secret`): an instance is secret when it says so, when it
  guards, and when it lies above a guarding instance. While the top instance is secret, input
  goes to the input filters of its own plugin and of the topmost guarding plugin only (others are
  skipped as if they passed), and no event tells where its focus is.

### 1.3 Navigation commands

Command records in the growth block (slot 15; `commands.md` rules: whole batch checked, applied
after the callback, chains bounded at 8 hops, `screens-input.md` section 3).

| Tag | Record | Fields |
| --- | --- | --- |
| 1547 | `command.open` | `target: str 128`, `mode: u32`, `slot: str 128`, `handover: u32` |
| 1548 | `command.close` | `instance: u32` |
| 1549 | `command.back_to` | `target: str 128` |
| 1550 | `command.set_key_layer` | `layer: str 128` |
| 1551 | `command.show_hints` | (none) |
| 1552 | `command.raise_input` | `input: str 64` |
| 1563 | `command.screen_policy` | `instance: u32`, `flags: u32` |

- [D] `open.mode`: 0 `screen` (open screen `target` on top), 1 `slot` (show the screen bound to slot
  `target`; if that slot is open already, close the screens above it and pass it the new hand-over),
  2 `replace` (open `target` in place of the submitter's own top instance, without showing the one
  below in between: the next step of a guide), 3 `in_slot` (open screen `target` for slot `slot`:
  shown again where that slot shows it with these params, else in place of what the slot shows).
  `slot` is empty except for mode 3 (`ERR_INVALID`). Other modes `ERR_INVALID`. `target` must be
  declared (`ERR_NOT_FOUND`), any plugin's (plugin freedom, as command 56). `handover` 0 is none;
  else one the submitter made in this callback and not yet named (`ERR_INVALID`). Command 56 stays:
  it is `open` mode 0 without a hand-over.
- [D] `close.instance`: 0 closes the submitter's own top instance (interface 6 `Back`; outside its
  own top screen `ERR_STATE`, as command 59). A non-zero instance closes that instance wherever it
  is in the stack, leaving the screens above it (interface 6 `Close(key)`): allowed for the
  submitter's own instances, and for another plugin's instance that is an `overlay` (help closes
  the overlay it covers). A guarding instance, and any instance above the topmost guarding one,
  only the guard's own plugin closes (`ERR_DENIED`, section 4.3). An instance that is no longer
  open is `ERR_NOT_FOUND`; the root screen is never closed (no effect, `OK`).
- [D] `back_to`: closes the screens above the topmost open instance of screen `target`, or of the
  screen of slot `target`; nothing where none is open (`OK`). Not across a guarding instance
  (`ERR_DENIED`).
- [D] `set_key_layer`: puts key-map layer `layer` in front of the slot's and the global layer for
  the submitter's own top instance (a book's own bindings); empty removes it. It goes with the
  instance. Outside its own top screen `ERR_STATE`.
- [D] `show_hints`: shows the row of button hints for a few seconds on a device with a hint row
  (`menu.need` 5); elsewhere `OK` and nothing.
- [D] `raise_input`: raises an input as written in key maps (`button.menu`, `double-tap.center`,
  or the submitter's own `input.<plugin>.<name>` / `gesture.<plugin>.<name>`): it runs what the
  layers shown bind it to, as if it came from the device. Another plugin's input or gesture, the
  Power key and the escape chord are `ERR_DENIED`; while an instance guards, only the guard's
  plugin may raise (`ERR_DENIED`, section 4.3). Raised inputs are a chain step (hop + 1) and pass
  no input filter.

## 2. Plugin inputs and gestures (declarations)

| Tag | Record | Fields |
| --- | --- | --- |
| 29 | `input.spec` | `name: str 64`, `title: str 96`, `kind: u32`, `need: u32`, `capability: u64`, `flags: u32` |

- [D] An input of the plugin's own, `input.<plugin>.<name>` in key maps (`kind` 0), or a gesture
  its recognizer makes, `gesture.<plugin>.<name>` (`kind` 1). `title` is a catalog key for
  binding lists. `flags` bit 0 `has_need`: `need`/`capability` as `setup.step` (`registry.md`
  section 6); without it both are 0 (`ERR_INVALID`). Other bits `ERR_INVALID`. The host stamps
  the owner; names are unique among the plugin's inputs (`ERR_INVALID`).
- [D] REG op 14 lists them with `reg_item.flags` bit 4 and `plugin` (`registry.md` section 10).
- [D] A plugin input is raised by its owner: by `command.raise_input` (its switch access, its
  chord) or by its recognizer's verdict (section 4.1). A gesture name never shadows a device
  gesture; the escape chord is never an input any plugin declares.

## 3. Questions and answers (dialogs)

Interface 6 `ask::text`, `ask::choice`, `ask::confirm`, `ask::folder` and their reply events.
The dialogs are plugin screens on the slots `core.keyboard`, `core.remote-keyboard`,
`core.choice`, `core.confirm` and `core.choose-folder` (the keyboard plugin and the ones that
replace it), as in interface 6. Questions and answers go through `core.ui` ops (section 6), so the
asker learns its request id at once and long texts move in pages, never cut.

### 3.1 Asking

- [D] Op 8 `ask` (input `ui_ask`: `kind`, `flags`, `slot`, `title`, `text`, `choices`; reply
  `ui_request`: `request`) records a question for the caller and returns its **request** id
  (`u32`, never 0). The dialog opens after the caller's callback returns (as a queued `open`, with
  the params `core.request` and `core.kind`), so the caller can still write the initial text.
- [D] `kind`: 0 text, 1 choice, 2 confirm (yes or no), 3 folder; others `ERR_INVALID`. `slot`:
  empty for the kind's slot; for kind 0 also `core.remote-keyboard` (the keyboard's "type on the
  phone" asks it this way, with its own text as the initial text); any other `ERR_INVALID`.
  `title` and `text` (the question) are display text the caller formatted itself (slot 27);
  `choices` (kind 1 only) at most 32 lines of at most 128 bytes and 2,048 bytes in all,
  non-empty, no trailing `\n` (`ERR_INVALID`, over the cap `ERR_NO_SPACE`).
- [D] `flags` (other bits `ERR_INVALID`; bits 0-7 for kind 0 only): bit 0 `live` (the asker hears
  the text while it is typed), bit 1 `multiline`, bit 2 `secret` (shown masked; the dialog instance
  is `secret`, section 1.2; never putting it on the clipboard is the dialog plugin's duty, the host
  cannot tell its text from other text, decision 44), bits 4-7 the text mode (0 plain, 1 number, 2
  e-mail, 3 URL, 4 file name).
- [D] **Request text.** Each request has one host text (its id is the request id), empty at
  first: for kind 0 the text to start from and then the answer, for kind 3 the folder to start in
  and then the chosen path. Op 9 `text_write` (`text` id, `offset`, `data` at most 3,072 bytes,
  `flags` bit 0 `last`: the text ends after this part) writes it in parts, append-only (`offset`
  the bytes written so far, else `ERR_INVALID`). **Replacing:** a write at `offset` 0 always starts
  the text over, also after `last`: the old text is gone, the new one is uncommitted until a part
  with `last` (an empty text is one write of no bytes with `last`). A failed part (`ERR_IO`,
  `ERR_NO_SPACE`, `ERR_NO_MEMORY`) leaves the text uncommitted at the parts before it; the writer
  goes on from there or starts over at 0; op 10 `text_read` (`text` id, `offset`) reads it in
  parts (`ui_text`: `total`, `offset`, `data`, cut only at UTF-8 boundaries). A host text is
  **committed** by the write with `last`; only a committed text is read or answered. Its cap
  (`ui_question.max_bytes`) is 24,576 bytes on every device: the 6,000 characters of interface 6
  in any script (at most 4 bytes each). Over it `ERR_NO_SPACE`; the text is never cut. On the X4
  the host keeps the first 512 bytes of a text in memory and the rest in a host file on the card
  (deleted when the text ends and at start). **Card failures** (Senior G): a full card is
  `ERR_NO_SPACE` for the part; with no card the request's question and the X4 host texts live in
  memory only, capped at what fits there: at most 2 KiB together (the secret text below aside),
  `ERR_IO` beyond; a missing or failing card is `ERR_IO`. A card pulled while a text has a card
  part makes every later read of that text `ERR_IO`, never a short text; a write at 0 starts it
  over. An answer committed before the card went is still delivered (outcome, choice and
  `length`), only its op 10 read fails (section 3.2).
  **Secret texts never touch the card**: a `secret` question's text stays in memory, at most
  8,192 bytes on every device, the 2,000 characters a password field takes in interface 6 in any
  script (`keyboard/src/lib.rs:116`; the lock hashes the whole code, so nothing is ever cut),
  allocated part by part as written (`ERR_NO_MEMORY` if the heap cannot, `ERR_NO_SPACE` beyond
  the cap); a real password costs its own length. **Live typing on the X4**: a `typing` answer
  (outcome 2) is accepted only while the text is in memory (at most 512 bytes, a secret text up
  to its cap), so typing never rewrites the card per key. A longer one is `ERR_NO_SPACE` to the
  dialog, and the asker gets one `typing` event with `flags` bit 2 `typing_stopped` (the live
  text stopped there, it is not complete); the final answer is always whole and may use the card
  once.
- [D] Who may write: the asker inside the callback that asked (the initial text), then only the
  dialog instance the host opened for the request, until its final answer. Who may read: that
  dialog while it is open, and the asker in the callback that delivers its answer. Anyone else
  `ERR_STATE`.
- [D] **A failed start cancels.** If the asking callback returns with an initial text begun but
  not committed (no `last`), or returns a failure status, the request is cancelled before any
  dialog opens: no partial text ever reaches a dialog, and the asker gets `event.answered`
  outcome 1 with `flags` bit 1 `not_started`. The same holds when the dialog's `open` cannot be
  applied (the stack is full).
- [D] Limits: at most 4 open requests device-wide, 1 on the X4 (Senior's answer to [Q] 4), and 1
  per asker (asking again cancels the open one: outcome 1); over that `ERR_FULL`. The request and
  the first part of its text are allocated fallibly at `ask` (`ERR_NO_MEMORY`). **One nested
  question:** the dialog instance of an open request may ask one more, of kind 0 on slot
  `core.remote-keyboard` only (the keyboard's "type on the phone", `keyboard/src/lib.rs:644`),
  also on the X4; the nested one is cancelled when its parent request ends, and it cannot nest
  again (`ERR_FULL`). It inherits the parent's `secret` flag (its dialog instance is secret too).
  **While any instance guards** a nested question is `ERR_DENIED`: the phone keyboard would carry
  the PIN over the network. Allowing it under a guard is the owner's decision (open, default
  refused). A request ends when answered (not `typing`), when its dialog closes without
  an answer (cancelled), or when the asker stops (no event). Asking from a screen ties the request
  to that instance: when it closes, the request is cancelled and its dialog closed. Asking from an
  action or an event is allowed.

### 3.2 Answering

| Tag | Record | Fields |
| --- | --- | --- |
| 82 | `event.answered` | `now_ms: u64`, `request: u32`, `kind: u32`, `outcome: u32`, `choice: u32`, `length: u32`, `flags: u32` |

- [D] The dialog reads the question with op 2 (`question`) and the text with op 10, writes its
  answer text with op 9 and commits with op 11 `answer` (`request`, `outcome`, `choice`). Only the
  instance the host opened for the request may answer (`ERR_STATE`); the first final answer wins.
  For kinds 0 and 3, an answer with outcome 0 or 2 needs a committed text (the last op 9 had
  `last`, else `ERR_STATE`).
- [D] `outcome`: 0 answered, 1 cancelled (no edit applies), 2 typing (a `live` text question: the
  text so far; the request stays open). Others `ERR_INVALID`. A consumer treats an outcome it does
  not know as cancelled.
- [D] What each kind answers: text, the request text (`length` its bytes); choice, `choice` (the
  line's index, `ERR_INVALID` past the list); confirm, `choice` 1 yes or 0 no; folder, the request
  text, a path on root 1 of at most 512 bytes (`ERR_INVALID` longer). Unused fields are 0.
- [D] **An answer is never lost.** `event.answered` is not a held-store edge: the answer stays in
  the request (its outcome, choice and text) and the request has its own pending mark, as a hint
  has (`core-events.md` 3.3), so the held-event store being full never drops it. The one
  exception is the X4 card: if it is pulled after the answer is committed, the event still comes
  with its outcome, choice and `length`, but the asker's op 10 read of a text with a card part is
  `ERR_IO` (section 3.1). The host encodes
  the event from the request when it delivers it to the asker (only the asker), in order with the
  asker's other events (the answer's place is when the dialog committed it). The asker reads the
  text with op 10 inside that callback. Memory is the request's own (section 8), bounded by the
  request limits.
- [D] **What the asker reads is what was answered.** Callbacks never overlap, so the text cannot
  change inside the delivering callback. Between an answer and its delivery, a `typing` update is
  withdrawn by the dialog's next op 9 (its pending mark is cleared; the next op 11 makes a new
  one), so a pending update always matches the committed text and its `length`. Typing updates
  replace each other, and a final answer replaces a pending typing update. After a final answer
  the dialog can no longer write (`ERR_STATE`), so the final text is fixed.
- [D] **Cleanup.** After the callback that delivered a final answer returns, the host frees the
  request and its text, whatever the callback's status (a failure is logged; the asker had the
  text in that callback). If the asker stops before delivery, the request is freed at its stop.
  A dialog still open after a final answer keeps no access to it (`ERR_STATE`); its own copy of
  the text is the dialog plugin's (it may keep a draft). `flags` bit 0: the dialog closed itself
  after this answer; bit 1 `not_started` (section 3.1); bit 2 `typing_stopped` (section 3.1).

## 4. Input filters and guards

### 4.1 The `core.input-filter` stage

- [D] A host-defined stage (`stages-services.md` section 1.3). A plugin subscribes with
  `registration.stage` naming `core.input-filter`; at most 4 subscribers device-wide, in the order
  plugins registered (interface 6 order). Each input (`input.key`, `input.pointer`,
  `input.gesture`) goes to the filters first, as the stage call's single input record, before
  screens, gestures and key maps. The hold-Power escape is checked before any filter and never
  reaches one.
- [D] Each subscriber replies with one `ui.verdict` record (stage-local tag 1):
  `kind: u32` 0 pass, 1 take, 2 run (`action: str 128`), 3 cancel, 4 replace (`input: bytes 128`,
  one input record), 5 raise (`input: str 64`, one of the subscriber's own declared inputs or
  gestures). Anything else, an empty reply or a failed callback is pass (logged). 1-5 end the
  filter chain; replace passes the new input on to the filters after it and everything else.
- [D] A filter that takes or cancels a pointer down takes it whole, to its up; one that takes a
  key press, its release too (interface 6 rule). If the capturing plugin stops, or its filter
  callback fails while it holds a capture, the host releases the capture: the rest of that
  pointer or key goes nowhere (the up or release is swallowed), and the next input starts fresh
  with all filters.
- [D] Cost: one callback per filter per input; on the X4 key presses only (no touch). Counts
  against the input-latency timing gate of the lock port.

### 4.2 Gesture recognizers

- [D] A gesture recognizer is a filter that answers 5 `raise` with its `gesture.<…>` input once it
  recognizes one; the key maps bind that name like any input. There is no separate recognizer
  callback.

### 4.3 Guards

- [D] An instance guards while its declared or current policy says so (section 1.2). While any
  instance guards, the **guard's plugin owns the input path and the stack above the topmost
  guarding instance**; everything else waits or is hidden. "The guard's plugin" below is the
  owner of the topmost guarding instance.
  - **Opens wait below.** An `open` (any mode), a dialog of another plugin's question, or a
    command 56 by any other plugin places the new instance directly below the topmost guarding
    instance: it gets no `draw` and no `input` until the guard ends (interface 6 rule). Only the
    guard's plugin opens above it (its own screens), and the dialogs of questions the guard's
    plugin asks open above it too, with their one nested question (section 3.1). The one other
    exception is the screen the host itself shows for a device state, the sleep screen of slot
    `core.sleep`: it shows above the guard, gets no input while the guard is up, and draws over a
    blank area (bit 2 `overlay`, section 1.2).
  - **Guard dialogs.** Such a dialog belongs to its plugin (the keyboard) but is authorized by
    the host for the guard's question: it is the top instance, so it gets keys and pointers, and
    its own plugin's filters run beside the guard's (section 1.2); it reads and writes its
    request's text and answers with ops 2, 9-11, and it closes itself (`close` 0) or is closed by
    the host when its request ends. It may close only its own instances above the guard and opens
    nothing else but its one nested question (`ERR_DENIED`); it is secret while it is above the
    guard. Its key layer applies as the guard's does (key maps below).
  - **Input.** Only the guard's plugin's input filters are called (the others are skipped, not
    called and ignored); keys and pointers then go to the top instance, which is the guard's
    plugin's. `raise_input` from any other plugin is `ERR_DENIED`.
  - **Key maps.** Only the own key layer (`set_key_layer`) of the guard instance or of the top
    guard dialog, and bindings to host actions or to the guard's plugin's own actions, apply; a
    binding in the slot or global layer that names another plugin's action or input does nothing
    while the guard is up (logged), so a key map can never run another plugin's code past the
    lock.
  - **Closing.** Only the guard's plugin closes the guarding instance or any instance above it
    (`close`, `back_to`: `ERR_DENIED` for others), except a guard dialog closing itself, so no
    other plugin closes the lock's keyboard.
  - **Stack.** For every other plugin, `core.ui` op 1 lists only the topmost guarding instance
    (`screen`, `owner`, flag guards) and nothing above or below it; `service.end.flags` bit 2
    says rows were left out. The guard's plugin sees the instances from the guard up.
  - **Drawing below.** Under another plugin's overlay nothing at or below the guard is drawn
    (section 1.2): the sleep screen never shows the page while the device is locked. Widget
    instances made by a screen below the guard are `ERR_DENIED` for ops 4-7 and 12-15.
  - **Output.** No `notify` is drawn over it (it waits, section 7); help shows nothing for any
    screen while the guard is up except the guard's plugin's own.
  - Core's tests cover each of these (hand-off, section 10).
- [D] **Private widgets and status** (Plugin: book titles, covers and highlights,
  `widgets/src/books.rs`). Widgets and status values stay usable under a guard (a clock on the
  sleep screen of a locked device), except those declared `private`: `widget.spec.flags` bit 1,
  `status.item.flags` bit 1, `status.line.flags` bit 1. While any instance guards, for every
  plugin but the guard's: a private widget is not made, measured, placed, targeted, sent input or
  composed (ops 4, 6, 7, 12, 13 and slot 35 `ERR_DENIED`; instances made before keep their handle
  but draw nothing); a private status item comes back from op 14 with `flags` bit 1 `hidden`, no
  text and no handle; a private status line is `ERR_DENIED` at op 15. The host drops cached
  private values when a guard starts. Undeclared means not private; the guard's plugin decides
  what its own screens show.
- [D] **Sealed actions** keep `reg_item.flags` bit 1 (`registry.md`): no other plugin's handler
  overrides the action (the lock's unlock).

## 5. Composition: one plugin's drawing inside another's

The home screen shows widgets and status lines, the sleep screen shows the screen below and
widgets, settings pages show previews: one plugin draws a rectangle that another plugin fills. The
showing plugin never runs another plugin's code itself: it asks the host (`core.ui` ops, slot 35),
and the host runs the owner's callbacks and the host-defined stages (`stages-services.md` 1.3:
`core.*` stages are run by the host only).

### 5.1 The compose slot

```
ui.compose(ctx, kind: u32, handle: u32, x: i32, y: i32, width: u32, height: u32) -> i32
```

- [D] New host slot 35, appended to `PpHost`. Only inside the caller's own `draw` callback
  (else `ERR_STATE`). `kind`: 0 a widget instance (purpose 4), 1 a drawn status item (purpose 3),
  2 a status line (purpose 5, new); others `ERR_INVALID`. A screen is never composed into another:
  the screen below an overlay is drawn by the host as itself (section 1.2). `handle` the
  instance or the handle op 14 gave. The host calls the owner's `draw` with that purpose, `id` the
  handle, the rectangle given and the caller's clip intersected with it (empty: no call, `OK`).
  Drawing outside is clipped by the canvas slots.
- [D] Refusals: an unknown or closed handle `ERR_NOT_FOUND`; a widget made by a screen below the
  guard or a private one (section 4.3) `ERR_DENIED`; **a target owned by the
  caller's own plugin `ERR_BUSY`** (the host's per-plugin re-entry guard: a plugin is never
  re-entered while its `draw` runs; it draws its own widgets itself); a call from depth 2
  `ERR_BUSY`.
- [D] **Depth.** The caller's `draw` runs at depth 1, the child's at depth 2. A child cannot
  compose (`ERR_BUSY`) or run a stage (`stage.run` is depth 1 only), and ops 7 and 12-15 are
  depth 1 only. Only widgets and status values are children, and they hold no widgets
  themselves, so nothing visible is lost; screens under an overlay are drawn at depth 1 with
  everything (section 1.2).
- [D] **State.** The host saves the caller's clip and canvas state before the child and restores
  them after it, also when the child fails or faults. The child's failure is its status, logged;
  the caller's frame stays.
- [D] **Isolation.** The child gets no handle to the caller's state and the caller none to the
  child's; they share the canvas inside the clip rectangle. Whatever a plugin may read back from
  the framebuffer (the #251 probe, host-only today) it may read of a composed child too. Screens
  are never composed, and a secret instance is not drawn under another plugin's overlay, judged
  at each frame by section 1.2 (declared, set at run time, guarding, or above a guarding
  instance), so a password keyboard is never drawn inside or under another plugin's screen.
- [D] **X4 stack.** Two plugin frames and the host frame between them share the main stack, for slot
  35 and for ops 7 and 12-15 alike (each runs another plugin's callback inside the caller's): they
  join the frame budget of #254 (limit 2,048 bytes per frame), with a device high-water case (home
  with widgets under the sleep screen's widgets) before activation; until the stack gate passes, the
  X4 answers slot 35 and those ops with `ERR_UNSUPPORTED`.

### 5.2 Widgets

| Tag | Record | Fields |
| --- | --- | --- |
| 224 | `widget.spec` | `name: str 96`, `title: str 96`, `category: str 96`, `previews: str 128`, `cells: u32`, `token: u32`, `flags: u32` |
| 225 | `widget.place` | `name: str 96`, `title: str 96`, `list_keys: str 1032`, `layout: u32`, `shown_if_key: str 128`, `shown_if_value: str 128` |
| 84 | `event.widget_made` | `now_ms: u64`, `widget: str 128`, `instance: u32`, `place: str 128` |
| 85 | `event.widget_dropped` | `now_ms: u64`, `instance: u32` |
| 86 | `event.widget_changed` | `now_ms: u64`, `instance: u32` |
| 1556 | `command.widget_changed` | `instance: u32` |
| 193 | `input.widget` | `now_ms: u64`, `instance: u32`, `target: u32`, `x: i32`, `y: i32`, `kind: u32` |

- [D] `widget.spec` declares a widget (interface 6 `WidgetSpec`): `category` one of the host's
  widget kinds or a catalog key of the plugin's own, `previews` the settings group it previews
  (empty for none), `cells` columns in bits 0-7 and rows in bits 8-15 (0: the grid's width and the
  rows it needs), `token` its callback state; `flags` bit 0 `live` (read each minute; without it
  only after events). REG op 19 lists them.
- [D] `widget.place` declares where widgets show (interface 6 `WidgetPlace`): `list_keys` the
  settings that list them (full ids, one per line, at most 8: a grid's pages each one), `layout`
  0 grid, 1 stack; `shown_if_key` and `shown_if_value`: the place shows only while that setting
  has that value (interface 6 `shown_if`; both empty: always, one empty `ERR_INVALID`).
- [D] **Lifetime.** A screen that shows a widget makes an instance with op 4 and drops it with
  op 5; the host drops a screen instance's widgets when it closes. The owner gets `widget_made` /
  `widget_dropped` at its token and keeps the instance's data.
- [D] **Shown** (interface 6 `Widgets::shown`, used to fetch only then: weather) is not "has
  instances": a widget is shown when a place whose `shown_if` holds lists it (its base id, also a
  placement `x@2`) in one of its `list_keys`. The host works it out from the settings at the
  time of the query: REG op 19 `reg_widget.flags` bit 0 `shown` (`registry.md` section 3).
- [D] **Layout** (interface 6 `height`, `min_height`, `place`, `targets`): the showing screen asks
  through op 12 `widget_measure` (`instance`, `width` → `height`, `min_height`; 0 height: nothing
  to show), tells the place with op 13 `widget_place` (the rectangle, before the first draw there
  and when it changes), and reads targets with op 7 (`instance`, the rectangle → `ui_target` rows,
  reading order; Senior's answer to [Q] 1: targets follow live content, so the owner answers each
  time). The owner returns at most 32 targets per call (more `ERR_NO_SPACE` for the owner's
  reply, logged, no targets); op 7 pages them (8 rows a page) from the stage reply the host keeps
  for the cursor, and a `widget_changed` in between makes the cursor stale (`ERR_STATE`). The host
  answers each by one call of the `core.widget` stage to the owner only
  (stage-local records `widget.measure`, `widget.place`, `widget.targets` and their replies; the
  owner is subscribed by its `widget.spec`, no `registration.stage`).
- [D] **Updates.** The owner says its widget shows something new with `command.widget_changed`
  (own instances); the host sends `event.widget_changed` (a hint, coalesced per instance) to the
  plugin whose screen made it, which measures and draws again. The minute tick stays `event.tick`.
- [D] **Input.** The showing screen forwards a tap or long press with op 6 (`widget_input`); the
  owner gets `input.widget` through `input` (`kind` 0 tap, 1 long press; `target` from op 7, 0 for
  none). Moving between targets with buttons stays the showing screen's.

### 5.3 Status items, lines and places

| Tag | Record | Fields |
| --- | --- | --- |
| 226 | `status.item` | `name: str 96`, `title: str 96`, `tap: str 96`, `priority: u32`, `min_width: u32`, `token: u32`, `flags: u32` |
| 227 | `status.line` | `name: str 96`, `title: str 96`, `height: u32`, `token: u32`, `flags: u32` |
| 228 | `status.place` | `name: str 96`, `title: str 96`, `left: str 128`, `center: str 128`, `right: str 128` |

- [D] A screen that shows a status line asks for its items' values with op 14 `status_values`
  (`items`: full ids, one per line, at most 8, a longer line asks twice; `context`: a `ui_param`
  stream, the reader's
  `title`, `chapter`, `page`, `progress`) and gets one `ui_status_value` row per item (`item`,
  `flags`, `text`, `handle`), then `end`. The host runs the **host-defined stage `core.status`**
  once per owning plugin for its items (stage-local `status.ask` / `status.value`) and caches the
  values per (item, context) until event 73 (`status_changed`) or the minute tick. A value is text
  (at most 128 bytes) or, with `flags` bit 0, drawn: the showing screen composes it with
  `ui.compose` kind 1 and the row's `handle`. A handle is valid only inside the callback that got
  it, so a drawn item is asked for with op 14 inside `draw`, then composed in the same `draw`.
- [D] **Own items are not called back.** The host never re-enters the caller (section 5.1): an
  item of the caller's own plugin (the reader showing its own page number) comes back from op 14
  with `flags` bit 2 `own`, no text and no handle, without a `core.status` call; the caller fills
  it itself. Op 15 for an own line returns `ui_instance` with `flags` bit 0 `own` and instance 0.
  Likewise op 4 for an own widget returns `flags` bit 0 `own`; for such an instance ops 6, 7, 12
  and 13 and slot 35 are `ERR_BUSY` without a call, and the plugin measures, places and draws its
  own widget itself.
- [D] A status line (whole line drawn by its owner) gets its handle from op 15 `status_line`
  (`line`, `context` → `handle`) and is composed with kind 2.
- [D] `status.item.tap` is one of the owner's actions, run as the owner with the line's context;
  `priority` 0-255 (higher stays longer); `min_width` in thousandths of the text size (0: its own
  width). `status.line.height` in thousandths of the text size. `status.place` names the three
  settings that list a line's items (interface 6 `Place`). `status.item.flags` and
  `status.line.flags` bit 1 `private` (section 4.3); other bits `ERR_INVALID`. REG op 21 (new,
  Senior's answer to [Q] 2) lists items and lines, one row type per op; op 19 stays widgets only.
- [D] `draw.frame.purpose` gains 5 `status line`.

## 6. The service `core.ui` (major 1)

Bare reply streams and paging as `core.registry` (`registry.md` section 2.1) with `page_rows` 8
(a `ui_screen` row is 339 bytes: 8 rows and `end` fit 4,096, 12 would not). Paged ops: 1, 3, 7.
Ops that run another plugin's callback (7, 12-15) are allowed at depth 1 only (`ERR_BUSY` deeper).

| Op | Name | Input | Reply | Who |
| --- | --- | --- | --- | --- |
| 1 | `stack` | `reg_query` | `ui_screen`*, `end` | anyone; top first, guarded rows left out |
| 2 | `question` | `ui_request` | `ui_question` | the request's dialog instance |
| 3 | `hints` | `reg_query` (`scope`: screen id) | `ui_hint`*, `end` | anyone (the help plugin) |
| 4 | `widget_make` | `ui_widget_ref` | `ui_instance` | for the caller's top instance |
| 5 | `widget_drop` | `ui_instance` | (none) | the instance's maker |
| 6 | `widget_input` | `ui_widget_input` | (none) | the instance's maker |
| 7 | `widget_targets` | `ui_widget_area` | `ui_target`*, `end` | the instance's maker |
| 8 | `ask` | `ui_ask` | `ui_request` | anyone (section 3.1) |
| 9 | `text_write` | `ui_text` | (none) | sections 1.1, 3.1 |
| 10 | `text_read` | `ui_text_at` | `ui_text` | sections 1.1, 3.1 |
| 11 | `answer` | `ui_answer` | (none) | the request's dialog instance |
| 12 | `widget_measure` | `ui_widget_area` | `ui_size` | the instance's maker |
| 13 | `widget_place` | `ui_widget_area` | (none) | the instance's maker |
| 14 | `status_values` | `ui_status_ask` | `ui_status_value`*, `end` | anyone, inside its draw or event callback |
| 15 | `status_line` | `ui_status_ask` | `ui_instance` | anyone, inside its draw callback |
| 18 | `clipboard_write` | `ui_text` | (none) | anyone (section 7) |
| 19 | `clipboard_read` | `ui_text_at` | `ui_text` | anyone (section 7) |
| 16 | `handover` | `ui_param`* | `ui_handle` | anyone, for its own `open` (section 1.1) |
| 17 | `params` | `ui_handle` | `ui_param`*, `end` | the instance the hand-over was given to |

| Tag | Record | Fields |
| --- | --- | --- |
| 1041 | `service.ui_screen` | `screen: str 128`, `instance: u32`, `slot: str 128`, `owner: str 64`, `flags: u32` |
| 1042 | `service.ui_request` | `request: u32` |
| 1043 | `service.ui_question` | `kind: u32`, `flags: u32`, `max_bytes: u32`, `title: str 256`, `text: str 1024`, `choices: str 2048` |
| 1044 | `service.ui_hint` | `input: u32`, `x: u32`, `y: u32`, `width: u32`, `height: u32`, `text: str 128`, `owner: str 64` |
| 1045 | `service.ui_widget_ref` | `widget: str 128`, `place: str 128` |
| 1046 | `service.ui_instance` | `instance: u32`, `flags: u32` |
| 1047 | `service.ui_widget_input` | `instance: u32`, `target: u32`, `x: i32`, `y: i32`, `kind: u32` |
| 1048 | `service.ui_target` | `target: u32`, `x: i32`, `y: i32`, `width: u32`, `height: u32` |
| 1049 | `service.ui_ask` | `kind: u32`, `flags: u32`, `slot: str 128`, `title: str 256`, `text: str 1024`, `choices: str 2048` |
| 1050 | `service.ui_text` | `text: u32`, `offset: u32`, `total: u32`, `flags: u32`, `data: str 3072` |
| 1051 | `service.ui_text_at` | `text: u32`, `offset: u32` |
| 1052 | `service.ui_answer` | `request: u32`, `outcome: u32`, `choice: u32` |
| 1053 | `service.ui_widget_area` | `instance: u32`, `x: i32`, `y: i32`, `width: u32`, `height: u32` |
| 1054 | `service.ui_size` | `height: u32`, `min_height: u32` |
| 1055 | `service.ui_status_ask` | `items: str 1032`, `context: bytes 1024` |
| 1056 | `service.ui_status_value` | `item: str 128`, `flags: u32`, `handle: u32`, `text: str 128` |
| 1057 | `service.ui_param` | `key: str 32`, `value: str 1024` |
| 1058 | `service.ui_handle` | `handle: u32` |

- [D] `ui_screen.flags`: bit 0 overlay, 1 guards, 2 covered (another instance is above it), 3
  waits ready and not yet ready, 4 secret. `owner` is host-stamped.
- [D] No feature is needed: what is on screen is not private (guarded parts are left out), and
  every op that touches a request, an instance or a handle checks that it is the caller's.
- [D] `ui_param` (Senior's answer to [Q] 3) is a service row of this block, a neutral tag outside
  the event block: in op 16's input, op 17's reply and nested in `ui_status_ask.context`, nowhere
  else. **Two-way records** (sent by plugins and by the host, so never in the host-sent-only list of
  #270, `abi7_host_decode.rs` `HOST_ONLY`): `ui_param`, `ui_handle` (op 16 out, op 17 in), `ui_text`
  (ops 9 and 18 in, ops 10 and 19 out), `ui_instance` (ops 4 and 15 out, op 5 in) and `ui_request`
  (op 8 out, op 2 in). Host-sent only: `ui_screen`, `ui_question`, `ui_hint`, `ui_target`,
  `ui_size`, `ui_status_value`; the rest are plugin-sent. `ui_text.data` is cut only at UTF-8
  boundaries; `items` at most 8 lines of at most 128 bytes.

## 7. Messages, help, clipboard, speech

| Tag | Record | Fields |
| --- | --- | --- |
| 30 | `help.hint` | `screen: str 128`, `input: u32`, `x: u32`, `y: u32`, `width: u32`, `height: u32`, `text: str 128` |
| 1557 | `command.screen_ready` | (none) |
| 87 | `event.screen_ready` | `now_ms: u64`, `screen: str 128`, `instance: u32` |
| 1558 | `command.notify` | `text: str 256`, `flags: u32` |
| 1561 | `command.speak` | `cookie: u32`, `text: str 2048`, `language: str 35`, `rate: u32`, `flags: u32` |
| 1562 | `command.stop_speaking` | (none) |
| 89 | `event.spoken` | `now_ms: u64`, `cookie: u32`, `status: u32` |

- [D] **Help.** `help.hint` declares a hint for any screen (own or another's, full id): `input` 0
  any, 1 touch, 2 buttons; the zone in thousandths of the screen (all 0: no zone, listed in the
  box); `text` a catalog key of the declaring plugin (`owner` in op 3 says whose catalog). The help
  plugin reads them with op 3, subscribes to `event.screen_ready` (an edge, registrable) and to 81.
  A screen declared `waits_ready` sends `command.screen_ready` once its first page is drawn (own
  top instance, else `ERR_STATE`); for other screens the host raises 87 itself after the first
  `draw` of each instance. The help plugin closes its own overlay by instance (section 1.3) and
  resets "seen" in its own data.
- [D] **Messages.** `notify` shows `text` (display text the plugin formatted, at most 256 bytes)
  over any screen for a few seconds, host-drawn, never over a guarding instance (it waits until the
  guard closes, at most 4 waiting, 2 on the X4, older ones dropped). `flags` bit 0 `announce`: also
  from background work and when no screen of the plugin shows (interface 6 `Announce`: device
  messages and rules the user set up; never sync results or offers). Without bit 0 a message from a
  plugin with no instance on the stack is dropped (`OK`, logged). Other bits `ERR_INVALID`.
- [D] **Clipboard.** Text only, at most 4,096 bytes (interface 6 `MAX_CLIPBOARD`), not across
  restarts; every device has one (Senior's answer). It is a host text with the fixed id 0 (section
  3.1 rules: append-only parts, start over at 0, committed by `last`): op 18 `clipboard_write`
  (`ui_text`, `text` 0) writes it in parts, a write at 0 replaces the old clipboard only when its
  new text is committed (until then reads see the old one; an uncommitted new text is dropped when
  the writer's callback returns); op 19 `clipboard_read` (`ui_text_at`, `text` 0) reads it in parts
  (`total` 0: empty), any plugin, at once. Keeping a `secret` answer (section 3.1) off it is the
  dialog plugin's duty; the host cannot enforce it (decision 44). On the X4 it lives in a host file
  on the card (none in memory but the part being moved), deleted at start as host texts are; with no
  card, op 18 is `ERR_IO` and op 19 reads it empty; a pulled card makes op 19 `ERR_IO`. Android may
  back it with the system clipboard.
- [D] **Speech.** `speak` queues `text` (display text, at most 2,048 bytes) in `language` (a BCP
  47 tag, empty for the system's) at `rate` percent (50-300, else `ERR_INVALID`); `flags` bit 0
  `queue` (after what is being spoken, else replacing it). `spoken` comes when it was spoken or
  could not be (`status` 0 spoken, 1 replaced or stopped, 2 no voice for the language, 3 no speech
  on this device). Only `menu.need` 0 (`speech`) devices speak; elsewhere `spoken` status 3 at
  once. `stop_speaking` stops the submitter's own speech only.

## 8. Sizes and memory

### 8.1 Every record fits its limit

Encoded sizes at the caps (bitmap 1 byte; a `str N`/`bytes N` field takes a 2-byte length and N
bytes; scalars 4 or 8; a record frame adds its tag and a 2-byte length; a batch its count byte).
Limits: a command batch and a service input 4,096 bytes (`commands.md`, `stages-services.md`),
a service reply `max_reply` 4,096, a stage payload 3,840. Each case gets a test that encodes it at
its caps (hand-off).

| Record | At its caps | Limit |
| --- | --- | --- |
| `ui_ask` (op 8 input, batch) | 1 + 4 + 4 + 130 + 258 + 1,026 + 2,050 = 3,473, framed 3,478 | 4,096 |
| `ui_question` (op 2 reply) | 1 + 12 + 258 + 1,026 + 2,050 = 3,347, framed 3,351 | 4,096 |
| `ui_text` (ops 9, 10, 18, 19) | 1 + 16 + 3,074 = 3,091, framed 3,096 | 4,096 |
| `ui_status_ask` (ops 14, 15) | 1 + 1,034 + 1,026 = 2,061, framed 2,066 | 4,096 |
| `ui_status_value` × 8 + `end` (op 14) | 8 × 273 + about 30 = 2,214 | 4,096 |
| `ui_screen` page (op 1) | row 1 + 130 + 4 + 130 + 66 + 4 = 335, framed 339; 8 rows + `end` = 2,742 | 4,096 |
| `ui_target` (op 7; `core.widget` reply) | row 21, framed 25; a page 8 × 25 + `end` = 230; the owner's 32 = 800 | 4,096; 3,840 |
| `ui_param` × 32 (op 16 input, op 17 reply) | key and value bytes at most 1,024 in all, plus 32 × 9 of fields and frames, + 1 = 1,313 (+ `end` on op 17) | 4,096 |
| `command.open` | 1 + 130 + 4 + 130 + 4 = 269, framed 274 | 4,096 and the stage budget |
| `event.screen_opened` | 1 + 8 + 130 + 4 + 130 + 4 × 4 = 289, framed 293 | the 1.1 KB stack buffer of `core-events.md` 3.3 |
| `ui.verdict` (filter reply) | under 200 | 3,840 |

### 8.2 Memory (X4)

- Instances: one entry per open instance (about 16 bytes; the stack is at most 16 deep, deeper opens
  are `ERR_FULL`) plus its hand-over's params, kept while it is open, stored encoded (at most 1,313
  bytes each). On the X4 the params of all instances share 1,536 bytes (op 16 `ERR_FULL` beyond):
  the largest real hand-over is one long selected text, which goes in the hand-over's text, and real
  params are a few dozen bytes (Plugin's inventory #984). Allocated fallibly at op 16
  (`ERR_NO_MEMORY`).
- Host texts (request texts and hand-over texts, sections 1.1 and 3.1): up to 24,576 bytes each.
  On the X4 at most 4 at once (the question, its nested question, two hand-overs; `ERR_FULL`
  beyond), each with at most 512 bytes in memory and the rest in a host file on the card (a
  secret text up to 8,192 bytes, in memory only, as long as typed), so texts cost at most 9.5 KiB
  of heap, and 10 KiB with no card (2 KiB pool + the secret text); elsewhere at most 16, in
  memory.
- Questions: at most 4 open requests device-wide (on the X4 one and its nested one), about 24
  bytes each plus the question (title, text, choices: at most 3.3 KiB), allocated fallibly at
  `ask` (`ERR_NO_MEMORY`). On the X4 the question goes to the request's card file with its text
  and op 2 reads it from there, so questions cost about 50 bytes of heap, nothing when none is
  open.
- **X4 worst case: activation-gate item.** Instances 0.25 + params 1.5 + texts and questions 10 (no
  card; 9.55 with one; 8 of it only while an 8 KiB password is typed) + widgets 0.25 + status cache
  1 + op 7 target cache 0.8 (only while a callback pages) + clipboard 0 (card) + messages 0.5 =
  **about 14.3 KiB**, every part allocated fallibly (a refusal is `ERR_NO_MEMORY` or `ERR_FULL`,
  never a panic); typical use (a few instances with small params, one short question, a status line)
  about 2 KiB, under the 4 KiB heap gate. Gate before the UIX host is active on the X4: the worst
  case is a line of the X4 heap budget and the typical case is measured on the device under 4 KiB
  (hand-off: Firmware, R3 heap measurement).
- Answers are not held-store edges: each request carries its answer and a pending mark (section
  3.2), so they cost nothing beyond the request.
- Filters: at most 4 subscribers; one stage call per input on the host scratch.
- Composition: no host memory; nesting as `stages-services.md` (depth 2). Ops 7 and 12-15 run one
  stage call each on the host scratch and nest another plugin's callback inside the caller's just
  as slot 35 does: they share slot 35's frame-budget entry and X4 stack gate (section 5.1), and
  answer `ERR_UNSUPPORTED` on the X4 until it passes.
- Widgets: about 16 bytes per made instance, at most 32 device-wide (16 on the X4).
- Status values: a cache of at most 2 KiB (1 KiB on the X4; when full, values are asked again),
  dropped on event 73.
- Clipboard: at most 4 KiB, host heap (card file on the X4), fallible. Messages: at most 4
  waiting, 1 KiB (2 waiting, 512 bytes on the X4).
- New slot 35 and the stage/command arms are flash cost on the X4 ppn5 delta; measured with the
  records.

## 9. Interface-6 calls and their interface-7 form

| Interface 6 | Interface 7 |
| --- | --- |
| `Command::OpenScreen(id, params)`, `OpenSlot`, `Replace`, `OpenInSlot` | `command.open` modes 0-3 |
| `Command::Back`, `Close(key)`, `BackTo` | `command.close` (0 / instance), `command.back_to` |
| `ScreenKey`, `Cx::params`, `params::TEXT` | `instance`; hand-over (op 16, `command.open.handover`), read with op 17 and the hand-over text (op 10); event 79 again on update |
| `Screen::guards`, `secret`, `holds_edges` (dynamic) | `screen.spec.flags` bits 1, 4, 5; `command.screen_policy` |
| `Env::stack`, `StackEntry` (`guards`, `overlay`, `slot`) | `core.ui` op 1, `ui_screen`; event 81 |
| `Command::SetKeyLayer`, `ShowHints`, `Input` | `command.set_key_layer`, `show_hints`, `raise_input` |
| `Registrar::add_input`, `add_gesture`, `InputSpec`, `GestureSpec` | `input.spec`; recognizer = filter verdict 5 |
| `Registrar::add_input_filter`, `InputVerdict` | stage `core.input-filter`, `ui.verdict` |
| sealed actions | `reg_item.flags` bit 1 |
| `ask::{text, choice, confirm, folder}`, `Typed`, `Picked`, `Confirmed`, `FolderChosen` | `core.ui` ops 8-10, `event.answered`; dialog side ops 2, 9-11 |
| `Typed::while_typing` (live), `slots::REMOTE_KEYBOARD` | `ask` flag `live`, outcome 2; `ask.slot` `core.remote-keyboard` (also nested from the keyboard's own dialog) |
| `help::add`, `help::wait`, `help::ready`, `help::READY` | `help.hint`, `screen.spec` flag `waits_ready`, `command.screen_ready`, event 87, op 3 |
| `Registrar::add_widget`, `add_widget_place`, `Widgets::make`, `Widget::{height, min_height, place, targets, live}` | `widget.spec`, `widget.place`, ops 4-7, 12, 13, stage `core.widget`, `ui.compose`, `input.widget` |
| `Registrar::add_status_item`, `add_status_line`, `add_status_place` | `status.item`, `status.line`, `status.place`, ops 14, 15, stage `core.status`, `ui.compose` |
| `Command::Notify`, `Announce` | `command.notify` (`announce` bit) |
| `Command::SetClipboard`, `ReadClipboard`, `Env::clipboard`, `MAX_CLIPBOARD` | `core.ui` ops 18, 19 (4,096 bytes in parts) |
| `Command::Speak`, `StopSpeaking`, `Spoken` | `command.speak`, `stop_speaking`, event 89 |
| `ChromeSpec`, `RefreshFilterSpec`, `SwitchTask`, `PlaySound` | not in 1.0 (section 0) |
| `Command::Emit` as a custom reply (keys' `PICKED`) | `ask` kind 1 (choice) or the defining plugin's stage (STG) |

## 10. Hand-offs (after approval)

- **Kernel:** records, `core.ui@1` schema, the three stage schemas (`core.input-filter`,
  `core.status`, `core.widget` targets), slot 35 in `abi.def`; generator and lock; REG op 21
  (status items and lines) and the op 19 `shown` flag in `registry.md`; this document final.
- **Core:** the stack with instances, hand-overs and policies, commands 1547-1563, the request
  table with nested questions, the host text pool (X4 card part, secret texts in memory only),
  overlay drawing of the screen below, the clipboard text, filter capture release, filter
  dispatch and guards, `ui.compose`, widget instances, status cache, clipboard and speech arms
  (speech through the shell), `core.ui` ops.
- **Firmware/shells:** speech and clipboard backends (Android), notify drawing, the X4
  measurements: the nested-callback stack gate (slot 35, ops 7 and 12-15) and the UIX line of the
  heap budget with the typical-case device measure (section 8.2).
- **Services:** SDK facade (navigation builders, `ask` helpers with typed answers, filter and
  status stage helpers, widget make/compose, help builders).
- **Plugin:** the ports, starting with keyboard, help, lock, sleep, home.
