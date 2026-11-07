# Interface 7 family UIX: screens, dialogs, input, composition (draft)

Contract draft for #228 (assignment #1002). Items marked [D] are proposed decisions, [Q] open
questions for Plugin and Senior. Inputs: the plugin needs inventory (#984,
`plugin-needs-reg-uix-dev-lib-doc-228.md`, UIX section), Plugin's per-plugin call sites (#1003,
`plugin-uix-calls-1003.md`), `screens-input.md` (screens, actions, input, draw, commands 56-60),
`stages-services.md` (stages, services), `registry.md` (REG; plugin inputs listed by op 14, the
hold-Power escape) and `core-events.md`. Doc-only: records, schema and generator follow once this
is approved, as for EVT and REG. Tags below are proposed and checked again with the records.

## 0. Scope and principles

- [D] **Plugin-side drawing stays (Q19).** Dialogs, the keyboard, lists, progress bars and
  widgets are plugin screens drawn by plugins. The host owns only what must be shared or trusted:
  the screen stack and its instances, routing of questions and answers, input filters and guards,
  composition of one plugin's drawing inside another's, and the few outputs a plugin cannot make
  itself (a message shown over any screen, the clipboard, speech).
- [D] **Answers go to the asker only.** Every question, clipboard read and speech request carries
  the asker's own `cookie` (a `u32`, like a declaration `token`); the host records who asked and
  delivers the answer to that plugin alone, with that cookie. No plugin can read or consume another
  plugin's answer, and an answer never arrives after the asker stopped (it is dropped as an edge,
  `core-events.md` section 3.3).
- [D] **Guards win.** A screen that guards the device (the lock) hides everything below it from
  every plugin: no stack details, no composition, no input, no help, no message drawn over it.
- [D] **The hold-Power escape is untouched.** No filter, guard, plugin input or dialog sees or
  takes the 8-second Power hold (`registry.md` section 4.2, `stages-services.md` section 1.3).
- [D] **No interface-6 shapes.** `Params`, `ScreenKey`, `Emit`-based replies and `Env::stack` have
  no interface-7 copies; what they carried is in the records below.
- Out of 1.0 (no first-party plugin uses them; additive later): chrome bars (`ChromeSpec`),
  refresh filters (`RefreshFilterSpec`), sound playback, task switching (`SwitchTask`).
  [Q] Plugin: confirm none of these is needed by a port.

## 1. Screen instances and parameters

### 1.1 Instances

- [D] Each opening of a screen is an **instance**: a host-assigned `u32`, never 0, not reused while
  the host runs (on the X4 until restart). The same screen may be open more than once with
  different parameters. An instance is what interface 6 called `ScreenKey`.
- [D] The `draw` and `input` callbacks keep their signatures. The host calls them only for the
  instance on top (and for composed instances, section 5), and it announces which instance is on
  top before the first `draw`: `event.screen_shown` (38) gains the appended field `instance`.
  A plugin keeps per-instance state keyed by it.

| Tag | Record | Fields | To |
| --- | --- | --- | --- |
| 38 | `event.screen_shown` (grown) | existing fields, then `instance: u32` | the screen's token |
| 79 | `event.screen_opened` | `now_ms: u64`, `screen: str 128`, `instance: u32`, `slot: str 128`, `params: str 1024` | the screen's token |
| 80 | `event.screen_closed` | `now_ms: u64`, `screen: str 128`, `instance: u32` | the screen's token |
| 81 | `event.stack_changed` | `now_ms: u64` | subscribers (hint) |

- [D] `screen_opened` comes once per instance, before its first `screen_shown`; `screen_closed`
  once, after its last `draw`. A plugin frees the instance's state there. Like 38 and 39, 79 and
  80 are not registrable (`ERR_UNSUPPORTED` in `registration.event`): they go to the token of the
  screen they name.
- [D] `params` are text lines `key=value` (UTF-8, `\n`-separated, at most 32 lines, keys
  `[a-z0-9_.-]{1,32}`, no `\n` in a value), at most 1,024 bytes: interface 6 `Params` as text. The
  host checks the form at submit (`ERR_INVALID`) and passes them unchanged; their meaning belongs
  to the screen that reads them. Host-added keys start with `core.` (section 3: `core.request`).
- [D] `stack_changed` is a hint: a screen opened, closed or came to the top. It carries nothing;
  the stack is read with `core.ui` op 1 (section 6).

### 1.2 Declaration flags

- [D] `screen.spec.flags` (11) gains, beside bit 0 `full_refresh_on_open`:
  - bit 1 `guards`: the screen guards the device (section 4.3);
  - bit 2 `overlay`: shown over part of the screen below (a menu, a dialog); the host keeps
    drawing the screen below where the overlay does not cover it;
  - bit 3 `waits_ready`: help for this screen waits until the screen says it is ready
    (`command.screen_ready`, section 7).
  Other bits stay `ERR_UNSUPPORTED`.

### 1.3 Navigation commands

Command records in the growth block (slot 15; `commands.md` rules: whole batch checked, applied
after the callback, chains bounded at 8 hops, `screens-input.md` section 3).

| Tag | Record | Fields |
| --- | --- | --- |
| 1547 | `command.open` | `target: str 128`, `mode: u32`, `slot: str 128`, `params: str 1024` |
| 1548 | `command.close` | `instance: u32` |
| 1549 | `command.back_to` | `target: str 128` |
| 1550 | `command.set_key_layer` | `layer: str 128` |
| 1551 | `command.show_hints` | (none) |
| 1552 | `command.raise_input` | `input: str 64` |

- [D] `open.mode`: 0 `screen` (open screen `target` on top), 1 `slot` (show the screen bound to
  slot `target`; if that slot is open already, close the screens above it and pass `params`
  again), 2 `replace` (open `target` in place of the submitter's own top instance, without showing
  the one below in between: the next step of a guide), 3 `in_slot` (open screen `target` for slot
  `slot`: shown again where that slot shows it with these params, else in place of what the slot
  shows). `slot` is empty except for mode 3 (`ERR_INVALID`). Other modes `ERR_INVALID`. `target`
  must be declared (`ERR_NOT_FOUND`), any plugin's (plugin freedom, as command 56). Command 56
  stays: it is `open` mode 0 without params.
- [D] `close.instance`: 0 closes the submitter's own top instance (interface 6 `Back`; outside its
  own top screen `ERR_STATE`, as command 59). A non-zero instance closes that instance wherever it
  is in the stack, leaving the screens above it (interface 6 `Close(key)`): allowed for the
  submitter's own instances, and for another plugin's instance that is an `overlay` (help closes
  the overlay it covers). A guarding instance only its own plugin closes (`ERR_DENIED`). An
  instance that is no longer open is `ERR_NOT_FOUND`; the root screen is never closed (no effect,
  `OK`).
- [D] `back_to`: closes the screens above the topmost open instance of screen `target`, or of the
  screen of slot `target`; nothing where none is open (`OK`). Not across a guarding instance
  (`ERR_DENIED`).
- [D] `set_key_layer`: puts key-map layer `layer` in front of the slot's and the global layer for
  the submitter's own top instance (a book's own bindings); empty removes it. It goes with the
  instance. Outside its own top screen `ERR_STATE`.
- [D] `show_hints`: shows the row of button hints for a few seconds on a device with a hint row
  (`menu.need` 5); elsewhere `OK` and nothing.
- [D] `raise_input`: raises an input as written in key maps (`button.menu`, `double-tap.center`,
  a plugin's `input.<plugin>.<name>`): it runs what the layers shown bind it to, as if it came
  from the device. The escape chord cannot be raised (`ERR_DENIED`). Raised inputs are a chain
  step (hop + 1).

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
The dialogs are plugin screens on the slots `core.keyboard`, `core.choice`, `core.confirm` and
`core.choose-folder` (the keyboard plugin and the ones that replace it), as in interface 6.

### 3.1 Asking

| Tag | Record | Fields |
| --- | --- | --- |
| 1553 | `command.ask` | `cookie: u32`, `kind: u32`, `flags: u32`, `title: str 256`, `text: str 1024`, `initial: str 3072`, `choices: str 2048` |

- [D] `kind`: 0 text, 1 choice, 2 confirm (yes or no), 3 folder. Other values `ERR_INVALID`.
  `title`, `text` (the question or explanation), `initial` (text: the text to start from; folder:
  the folder to start in) are display text the asker formatted itself (slot 27); `choices` (kind 1
  only, else empty) is a list of at most 32 lines of at most 128 bytes, non-empty, no trailing
  `\n` (the REG list rule, enforced by the generated codec).
- [D] `flags` (other bits `ERR_INVALID`): bit 0 `live` (text: the asker gets the text so far
  while typing, section 3.2), bit 1 `multiline`, bit 2 `secret` (text: shown masked, never put on
  the clipboard), bit 3 `edit` (text: input-method mode, section 3.3), bits 4-7 the text mode (0
  plain, 1 number, 2 e-mail, 3 URL, 4 file name, others `ERR_INVALID`). Bits 0-7 only for kind 0.
- [D] The host records `(asker, cookie)` under a new **request** id (`u32`, never 0) and opens the
  slot's screen for `kind` with the params `core.request=<id>`, `core.kind=<kind>`. At most 4
  requests are open device-wide and 1 per asker (a new question from the same asker cancels its
  open one with outcome 1); over that `ERR_FULL`. A request ends when it is answered, when the
  dialog instance closes without an answer (outcome 1), or when the asker stops (no event).
- [D] Asking from a screen ties the request to that instance: when the asker's instance closes,
  the request is cancelled and the dialog closed. Asking from an action or event (no screen) is
  allowed; the request then lives until answered or cancelled.

### 3.2 Answering

| Tag | Record | Fields |
| --- | --- | --- |
| 1554 | `command.answer` | `request: u32`, `outcome: u32`, `choice: u32`, `text: str 3072` |
| 82 | `event.answered` | `now_ms: u64`, `cookie: u32`, `kind: u32`, `outcome: u32`, `choice: u32`, `text: str 3072`, `flags: u32` |

- [D] The dialog screen reads the question with `core.ui` op 2 (`question`, by its `request`).
  Only the instance the host opened for that request may answer it (another: `ERR_STATE`); the
  first answer wins (later: `ERR_STATE`). `outcome`: 0 answered, 1 cancelled (no edit applies),
  2 typing (only for a `live` text question: the text so far; the request stays open). Other
  values `ERR_INVALID`.
- [D] What each kind answers: text, `text` (at most 3,072 bytes); choice, `choice` (the line's
  index, `ERR_INVALID` past the list); confirm, `choice` 1 yes or 0 no; folder, `text` the path
  on root 1 (at most 512 bytes, `ERR_INVALID` longer). Unused fields are 0 and empty
  (`ERR_INVALID` otherwise).
- [D] The host delivers `event.answered` to the asker only, with its `cookie`, as an edge
  (`core-events.md` 3.3: kept in the held-event store, dropped and logged when it does not fit;
  for a dropped answer the asker gets nothing and sees its dialog closed). Typing updates
  (outcome 2) are coalesced per request: only the newest is kept. `flags` bit 0: the dialog
  closed itself after this answer.
- [D] A consumer treats an `outcome` it does not know as cancelled.
- [Q] Plugin: is 3,072 bytes of answered text enough for every port? The interface-6 keyboard
  allows 2,000 characters on one line and 6,000 multiline (up to 24 KB of UTF-8). Proposal: longer
  texts are not answers but edits (section 3.3).

### 3.3 Text editing (input method)

For an editor that owns its text (mail, notes) and wants the keyboard to type into it, without
handing the whole text back and forth:

| Tag | Record | Fields |
| --- | --- | --- |
| 1555 | `command.text_edit` | `request: u32`, `insert: str 256`, `delete_before: u32`, `delete_after: u32`, `flags: u32` |
| 83 | `event.text_edit` | `now_ms: u64`, `cookie: u32`, `insert: str 256`, `delete_before: u32`, `delete_after: u32`, `flags: u32` |

- [D] An `ask` text question with `flags` bit 3 `edit` opens the keyboard in edit mode: the
  keyboard sends what is typed as edits (delete so many bytes before and after the caret, then
  insert), the host forwards each one to the asker in order, and the editor applies it to its own
  text. `flags` bit 0 `done` ends the request (outcome as answered), bit 1 `cancel` ends it
  cancelled. Deletes count whole UTF-8 scalars in bytes; an edit that would cut a scalar is the
  editor's to refuse. `initial` gives the keyboard the text around the caret for display (at most
  3,072 bytes). Edits are edges and never coalesced; at most 16 wait per asker (`core-events.md`
  3.3).

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
  key press, its release too (interface 6 rule).
- [D] Cost: one callback per filter per input; on the X4 key presses only (no touch). Counts
  against the input-latency timing gate of the lock port.

### 4.2 Gesture recognizers

- [D] A gesture recognizer is a filter that answers 5 `raise` with its `gesture.<…>` input once it
  recognizes one; the key maps bind that name like any input. There is no separate recognizer
  callback.

### 4.3 Guards

- [D] While an instance of a `guards` screen (section 1.2) is open, everything below it is hidden:
  - `core.ui` op 1 lists the stack only from the topmost guarding instance up; rows below it are
    left out and `service.end.flags` bit 2 says so;
  - composition (section 5) of anything below it is `ERR_DENIED`, and composing a guarding
    instance itself is `ERR_DENIED` for every other plugin;
  - `command.close`/`back_to` cannot cross it (section 1.3), and no `notify` is drawn over it
    (it waits, section 7);
  - help shows nothing for screens below it.
- [D] **Sealed actions** keep `reg_item.flags` bit 1 (`registry.md`): no other plugin's handler
  overrides the action (the lock's unlock).

## 5. Composition: one plugin's drawing inside another's

The home screen shows widgets, the sleep screen shows the screen below and widgets, settings pages
show previews: one plugin draws a rectangle that another plugin fills.

### 5.1 The compose slot

```
ui.compose(ctx, kind: u32, instance: u32, x: i32, y: i32, width: u32, height: u32) -> i32
```

- [D] New host slot 35, appended to `PpHost`. Only inside the caller's own `draw` callback
  (else `ERR_STATE`). `kind` 0 a widget instance (section 5.2), 1 a screen instance (the
  stack's). The host calls the target owner's `draw` with a `draw.frame` of purpose 4 (widget) or
  0 (screen), `id` the instance, the rectangle given and the caller's clip intersected with it
  (an empty intersection: no call, `OK`). Drawing outside is clipped by the canvas slots.
- [D] Refusals: an unknown or closed instance `ERR_NOT_FOUND`; a guarding instance or any
  instance below the topmost guarding one `ERR_DENIED`; the caller's own instance or a cycle
  `ERR_INVALID`; nesting beyond depth 2 `ERR_BUSY`. The child's failure is its status, logged;
  the caller's frame stays.
- [D] The child never sees the caller's pixels or state, and the caller never sees the child's:
  the canvas is the screen's, shared only through the clip rectangle.

### 5.2 Widgets

| Tag | Record | Fields |
| --- | --- | --- |
| 224 | `widget.spec` | `name: str 96`, `title: str 96`, `category: str 96`, `previews: str 128`, `cells: u32`, `token: u32`, `flags: u32` |
| 225 | `widget.place` | `name: str 96`, `title: str 96`, `list_key: str 128`, `layout: u32` |
| 84 | `event.widget_made` | `now_ms: u64`, `widget: str 128`, `instance: u32`, `place: str 128` |
| 85 | `event.widget_dropped` | `now_ms: u64`, `instance: u32` |
| 86 | `event.widget_changed` | `now_ms: u64`, `instance: u32` |
| 1556 | `command.widget_changed` | `instance: u32` |
| 193 | `input.widget` | `now_ms: u64`, `instance: u32`, `target: u32`, `x: i32`, `y: i32`, `kind: u32` |

- [D] `widget.spec` declares a widget (interface 6 `WidgetSpec`): `category` one of the host's
  widget kinds or a catalog key of the plugin's own, `previews` the settings group it previews
  (empty for none), `cells` columns in bits 0-7 and rows in bits 8-15 (0: the grid's width and the
  rows it needs), `token` its callback state. `flags`: none yet (0). REG op 19 lists them.
- [D] `widget.place` declares where widgets show (interface 6 `WidgetPlace`): `list_key` the
  setting that lists them, `layout` 0 grid, 1 stack. Shown places are what `Widgets::shown` asked.
- [D] A screen that shows a widget makes an instance with `core.ui` op 4 (`widget_make`) and drops
  it with op 5; the host drops a screen instance's widgets when it closes. The widget's owner gets
  `widget_made`/`widget_dropped` at its token. It draws through `ui.compose` (purpose 4, `id` the
  instance).
- [D] The owner says its widget shows something new with `command.widget_changed` (own instances
  only); the host sends `event.widget_changed` (a hint, coalesced per instance) to the plugin
  whose screen made it, which redraws. The minute tick stays `event.tick`.
- [D] Taps on a widget: the showing screen forwards them with `core.ui` op 6 (`widget_input`); the
  owner gets `input.widget` at its token through `input` (`kind` 0 tap, 1 long press; `target`
  the widget target the showing screen resolved, 0 for none). Button navigation between targets
  stays the showing screen's (it reads targets with op 7).
- [Q] Senior: widget targets (interface 6 `WidgetTarget`, tap areas reached by buttons) as a
  stage reply of the owner (op 7 runs the `core.widget` stage `targets` call) or as declarations?
  Proposal: the stage, since targets follow live content.

### 5.3 Status items, lines and places

| Tag | Record | Fields |
| --- | --- | --- |
| 226 | `status.item` | `name: str 96`, `title: str 96`, `tap: str 96`, `priority: u32`, `min_width: u32`, `token: u32`, `flags: u32` |
| 227 | `status.line` | `name: str 96`, `title: str 96`, `height: u32`, `token: u32`, `flags: u32` |
| 228 | `status.place` | `name: str 96`, `title: str 96`, `left: str 128`, `center: str 128`, `right: str 128` |

- [D] A **host-defined stage `core.status`** gives an item's value: the screen that shows a line
  runs it with one `status.ask` record (stage-local: `item`, `context: str 1024` as `params`, the
  reader's `title`, `chapter`, `page`, `progress`); each item's owner replies one `status.value`
  (`text: str 128`, or `flags` bit 0 drawn: the value is drawn through `ui.compose` purpose 3 with
  `id` the item's declaration index). Values are cached per (item, context) until event 73
  (`status_changed`) or the minute tick.
- [D] `status.item.tap` is one of the owner's actions, run as the owner with the line's context;
  `priority` 0-255 (higher stays longer); `min_width` in thousandths of the text size (0: its own
  width). `status.line` is a whole line its owner draws (purpose 5, new), `height` in thousandths
  of the text size. `status.place` names the three settings that list a line's items (interface 6
  `Place`). REG op 19 lists items and lines with the widgets ([Q] Senior: or a separate op 21).
- [D] `draw.frame.purpose` gains 5 `status line`.

## 6. The service `core.ui` (major 1)

Bare reply streams and paging as `core.registry` (`registry.md` section 2.1, `page_rows` 32).

| Op | Name | Input | Reply | Notes |
| --- | --- | --- | --- | --- |
| 1 | `stack` | `reg_query` | `ui_screen`*, `end` | top first; guarded rows left out (section 4.3) |
| 2 | `question` | `ui_request` | `ui_question` | only the dialog instance the host opened for it |
| 3 | `hints` | `reg_query` (`scope`: screen id) | `ui_hint`*, `end` | for the help plugin |
| 4 | `widget_make` | `ui_widget_ref` | `ui_instance` | for the caller's top instance |
| 5 | `widget_drop` | `ui_instance` | (none) | own instances |
| 6 | `widget_input` | `ui_widget_input` | (none) | forwards a tap to the owner |
| 7 | `widget_targets` | `ui_instance` | `ui_target`*, `end` | the owner's current targets |

| Tag | Record | Fields |
| --- | --- | --- |
| 1041 | `service.ui_screen` | `screen: str 128`, `instance: u32`, `slot: str 128`, `owner: str 64`, `flags: u32` |
| 1042 | `service.ui_request` | `request: u32` |
| 1043 | `service.ui_question` | `kind: u32`, `flags: u32`, `title: str 256`, `text: str 1024`, `initial: str 3072`, `choices: str 2048` |
| 1044 | `service.ui_hint` | `input: u32`, `x: u32`, `y: u32`, `width: u32`, `height: u32`, `text: str 128`, `owner: str 64` |
| 1045 | `service.ui_widget_ref` | `widget: str 128`, `place: str 128` |
| 1046 | `service.ui_instance` | `instance: u32` |
| 1047 | `service.ui_widget_input` | `instance: u32`, `target: u32`, `x: i32`, `y: i32`, `kind: u32` |
| 1048 | `service.ui_target` | `target: u32`, `x: i32`, `y: i32`, `width: u32`, `height: u32`, `action: str 128` |

- [D] `ui_screen.flags`: bit 0 overlay, 1 guards, 2 covered (another instance is above it), 3
  waits ready and not yet ready. `owner` is host-stamped.
- [D] No feature is needed for ops 1, 3 and 7 (what is on screen is not private; guarded parts are
  left out). Ops 2 and 4-6 are tied to the caller's own instances, checked by the host.

## 7. Messages, help, clipboard, speech

| Tag | Record | Fields |
| --- | --- | --- |
| 30 | `help.hint` | `screen: str 128`, `input: u32`, `x: u32`, `y: u32`, `width: u32`, `height: u32`, `text: str 128` |
| 1557 | `command.screen_ready` | (none) |
| 87 | `event.screen_ready` | `now_ms: u64`, `screen: str 128`, `instance: u32` |
| 1558 | `command.notify` | `text: str 256`, `flags: u32` |
| 1559 | `command.set_clipboard` | `text: str 3072` |
| 1560 | `command.read_clipboard` | `cookie: u32` |
| 88 | `event.clipboard_read` | `now_ms: u64`, `cookie: u32`, `status: u32`, `text: str 3072` |
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
  over any screen for a few seconds, host-drawn, never over a guarding instance (it waits until
  the guard closes, at most 4 waiting, older ones dropped). `flags` bit 0 `announce`: also from
  background work and when no screen of the plugin shows (interface 6 `Announce`: device messages
  and rules the user set up; never sync results or offers). Without bit 0 a message from a plugin
  with no instance on the stack is dropped (`OK`, logged). Other bits `ERR_INVALID`.
- [D] **Clipboard.** Text only, at most 3,072 bytes, kept in host memory (not across restarts).
  `read_clipboard` answers with `event.clipboard_read` to the reader only (`status` 0 text, 1
  empty, 2 no clipboard on this device). A `secret` answer (section 3.1) is never put there.
  Whether a device has a clipboard: `menu.need` capability bit (new) [Q] or always (host memory)?
  Proposal: always; Android may back it with the system clipboard.
- [D] **Speech.** `speak` queues `text` (display text, at most 2,048 bytes) in `language` (a BCP
  47 tag, empty for the system's) at `rate` percent (50-300, else `ERR_INVALID`); `flags` bit 0
  `queue` (after what is being spoken, else replacing it). `spoken` comes when it was spoken or
  could not be (`status` 0 spoken, 1 replaced or stopped, 2 no voice for the language, 3 no speech
  on this device). Only `menu.need` 0 (`speech`) devices speak; elsewhere `spoken` status 3 at
  once. `stop_speaking` stops the submitter's own speech only.

## 8. Memory (X4)

- Instances: one entry per open instance (about 16 bytes; the stack is at most 16 deep, deeper
  opens are `ERR_FULL`); params are not kept by the host after `screen_opened` is delivered.
- Questions: at most 4 open requests device-wide, about 24 bytes each; the question text lives in
  the asker's batch until the dialog reads it, then in a host buffer of at most 6.5 KiB per open
  request, allocated fallibly (`ERR_NO_MEMORY` at submit). [Q] Senior: or keep only one open
  question device-wide on the X4 (6.5 KiB) to bound it.
- Answers and edits use the held-event store (`core-events.md` 3.3), no extra memory.
- Filters: at most 4 subscribers; one stage call per input on the host scratch.
- Composition: no host memory; nesting as `stages-services.md` (depth 2).
- Widgets: about 16 bytes per made instance, at most 32 device-wide.
- Status values: a cache of at most 2 KiB, dropped on event 73.
- Clipboard: at most 3 KiB host heap, fallible. Messages: at most 4 waiting, 1 KiB.
- New slot 35 and the stage/command arms are flash cost on the X4 ppn5 delta; measured with the
  records.

## 9. Interface-6 calls and their interface-7 form

| Interface 6 | Interface 7 |
| --- | --- |
| `Command::OpenScreen(id, params)`, `OpenSlot`, `Replace`, `OpenInSlot` | `command.open` modes 0-3 |
| `Command::Back`, `Close(key)`, `BackTo` | `command.close` (0 / instance), `command.back_to` |
| `ScreenKey`, `Cx::params` | `instance`, `event.screen_opened.params` |
| `Env::stack`, `StackEntry` (`guards`, `overlay`, `slot`) | `core.ui` op 1, `ui_screen`; event 81 |
| `Command::SetKeyLayer`, `ShowHints`, `Input` | `command.set_key_layer`, `show_hints`, `raise_input` |
| `Registrar::add_input`, `add_gesture`, `InputSpec`, `GestureSpec` | `input.spec`; recognizer = filter verdict 5 |
| `Registrar::add_input_filter`, `InputVerdict` | stage `core.input-filter`, `ui.verdict` |
| `Screen::guards`, sealed actions | `screen.spec.flags` bit 1; `reg_item.flags` bit 1 |
| `ask::{text, choice, confirm, folder}`, `Typed`, `Picked`, `Confirmed`, `FolderChosen` | `command.ask`, `event.answered`; dialog side `core.ui` op 2, `command.answer` |
| `Typed::while_typing` (live) | `ask` flag `live`, answer outcome 2 |
| `help::add`, `help::wait`, `help::ready`, `help::READY` | `help.hint`, `screen.spec` flag `waits_ready`, `command.screen_ready`, event 87, op 3 |
| `Registrar::add_widget`, `add_widget_place`, `Widgets::make`, `WidgetTarget` | `widget.spec`, `widget.place`, ops 4-7, `ui.compose`, `input.widget` |
| `Registrar::add_status_item`, `add_status_line`, `add_status_place` | `status.item`, `status.line`, `status.place`, stage `core.status` |
| `Command::Notify`, `Announce` | `command.notify` (`announce` bit) |
| `Command::SetClipboard`, `ReadClipboard`, `Env::clipboard` | `command.set_clipboard`, `read_clipboard`, event 88 |
| `Command::Speak`, `StopSpeaking`, `Spoken` | `command.speak`, `stop_speaking`, event 89 |
| `ChromeSpec`, `RefreshFilterSpec`, `SwitchTask`, `PlaySound` | not in 1.0 (section 0) |
| `Command::Emit` as a custom reply (keys' `PICKED`) | `ask` kind 1 (choice) or the defining plugin's stage (STG) |

## 10. Hand-offs (after approval)

- **Kernel:** records, `core.ui@1` schema, the three stage schemas (`core.input-filter`,
  `core.status`, `core.widget` targets), slot 35 in `abi.def`; generator and lock; this document
  final.
- **Core:** the stack with instances and params, commands 1547-1562, the request table, filter
  dispatch and guards, `ui.compose`, widget instances, status cache, clipboard and speech arms
  (speech through the shell), `core.ui` ops.
- **Firmware/shells:** speech and clipboard backends (Android), notify drawing, the X4 measurements.
- **Services:** SDK facade (navigation builders, `ask` helpers with typed answers, filter and
  status stage helpers, widget make/compose, help builders).
- **Plugin:** the ports, starting with keyboard, help, lock, sleep, home.
