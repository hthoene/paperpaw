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
| 13 | `menu.entry` | `menu: str` (full id), `item: str` (full id), `order: i32`, `place: u32`, `anchor: str` (full id or empty) | 128, 128, 128 |
| 14 | `icon.spec` | `item: str` (full id), `icon: str` (icon name) | 128, 64 |
| 15 | `menu.need` | `item: str` (full id), `need: u32`, `capability: u64` | 128 |
| 16 | `menu.section` | `name: str` (relative), `title: str` (i18n key), `menu: str` (full id), `order: i32` | 96, 128, 128 |
| 17 | `menu.hide` | `menu: str` (full id), `item: str` (full id) | 128, 128 |
| 18 | `menu.describe` | `item: str` (relative), `key: str` (i18n key) | 96, 128 |

- [D] `token` names the plugin callback state the host passes to `draw`, `input` and
  `dispatch_event` for this screen or action. Tokens are the plugin's own numbers; one token may
  serve several screens.
- [D] `screen.spec.flags`: bit 0 `full_refresh_on_open`; other bits must be 0 (`ERR_UNSUPPORTED`).
  `action.spec.flags`: bit 0 `needs_screen` (the action only runs while one of this plugin's
  screens is on top); other bits 0.
- [D] Names follow the declaration name rules (`records.md`); full id = `<plugin>.<name>`. A
  screen's name is unique among the plugin's screens and an action's among its actions (a screen
  and an action may share a name); checked at commit, order-independent.
- [D] Running an action delivers `event.action_run` (tag 39) to its token through
  `dispatch_event`; the plugin answers with commands.
- [D] **Menu items are full ids** (`<plugin>.<name>`) in `menu.entry`, `icon.spec`, `menu.need` and
  `menu.hide`, as interface 6's `MenuEntry.item` is any `Id`: a plugin may put a core setting or
  another plugin's action into a menu (the reader puts `NIGHT_MODE` into its own menu). The first
  segment names the owner. An item of the calling plugin must resolve at commit to one of its
  actions, settings, setting groups or screens (`ERR_INVALID` otherwise); another plugin's item, or a
  menu or anchor that does not exist yet, is accepted and takes effect once it exists (plugins load
  in any order). `menu.section` and `menu.describe` only name the plugin's own (relative) items.
- [D] `menu.entry` puts `item` into `menu` (any plugin's menu, such as `core.tools` or `core.main`).
  The label is the item's title. `order` must fit an `i16` (lower comes first). `place`: 0 by
  `order`, 1 after the entry `anchor`, 2 before it, 3 last in the section `anchor` (a
  `menu.section`), 4 first, 5 last; `anchor` is a full id for 1-3 and empty for 0, 4 and 5
  (`ERR_INVALID`). **A place this host does not know is accepted and the entry is not shown** (a
  plugin built for a later host still loads; interface 6 did the same); its `anchor` is empty or a
  full id. An anchor that is not, or no longer, there places the entry by `order`. The same `menu`
  and `item` twice in one batch is `ERR_INVALID`. Where two plugins put the same item into the same
  menu, a placed entry (1-5) wins over one by order (it moves it), and between two of the same kind
  the plugin loaded later wins.
- [D] `icon.spec` gives an item an icon: one `[a-z0-9-]` segment naming an icon of the host's icon
  set (the SDK's icon names). **Any item may get one** (an icon theme); at most one per item in a
  batch (`ERR_INVALID`), and across plugins the one loaded later wins. A name the host's set lacks
  is accepted and shows no icon, so hosts may carry smaller sets.
- [D] `menu.need` offers an item only on devices that have what it needs; elsewhere it is left out
  of menus and settings pages, also where the user listed it (except `no-touch`, which only leaves
  it out of the menus plugins define). `need`: 0 `speech`, 1 `sleep` (the device shows the sleep
  screen), 2 `rotation`, 3 `no-touch`, 4 `touch`, 5 `button-row` (buttons below the screen with a
  row of hints), 6 `colors` (shows colours other than black and white), 7 `wifi`, 8 `firmware` (the
  device can update its firmware), 9 `black-and-white`, 10 `frontlight`, 11 `bluetooth`, 12 `has`.
  **A need this host does not know is accepted and not met** (the item is left out), so a later
  need needs no new record. For a known need `capability` is 0 except for `has`, where it is a
  nonzero set of capability bits the device must all have (`ERR_INVALID` otherwise): bit 0
  `audio`, 1 `warmth`, 2 `charge-limit`, 3 `keyboard`, 4 `hot-code`, 5 `page-keys`, 6 `buttons`,
  7 `usb-link`, 8 `render-hook`, 9 `render-regions`, 10 `look-extras`, 11 `usb-drive`, 12
  `chords`, 13 `page-turner`, 14 `hotspot`, 15 `pen-tilt`, 16 `pen-hover`, as the SDK's
  `Capability` (a test keeps this list, the SDK and the host's equal); a bit the host does not know
  is not met. At most one need per item in a batch (`ERR_INVALID`).
- [D] `menu.section` adds a heading `name` (the plugin's own; also usable in the user's menu
  definitions as `<plugin>.<name>`) with the translated `title` to `menu` at `order` (an `i16`);
  the entries ordered after it, up to the next heading, belong to it, and place 3 puts an entry last
  in it. Names are unique among the plugin's sections (`ERR_INVALID`).
- [D] `menu.hide` takes `item` out of `menu` as plugins contribute it, another plugin's entry
  included (an entry a plugin replaces with its own); a user who lists the item in their own menu
  keeps it, and the menu editor tells which plugin took it out. The same pair twice in a batch is
  `ERR_INVALID`.
- [D] `menu.describe` puts a line under one of the plugin's own items (`item`, relative) in menus,
  by its translation key `key`. At most one per item in a batch (`ERR_INVALID`).
- [D] An item whose label, switch, badge or availability changes while it is shown is not a
  declaration: the command `command.set_item_state` (section 3.1) sets it.
- [D] Until a host's menus and icon set take them, a host refuses these six records with
  `ERR_UNSUPPORTED` (after the syntax checks above, so a malformed record still says
  `ERR_INVALID`); it never accepts and drops one.
- [D] Translations need no record. A card plugin's catalogs are the files
  `plugins/<id>/i18n/<language>.ini` beside its `code.ppn` (the same folder layout as interface 6);
  a plugin built into the firmware brings them built in. `title` keys, `i18n.copy` (6),
  `i18n.format` (27) and plurals resolve in the calling plugin's catalogs through the host's language
  fallback chain, which ends in English; a key in none gives `ERR_NOT_FOUND`.

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
- [D] `event.screen_shown` (38) and `event.action_run` (39) cannot be registered for with
  `registration.event`: they go to the token of the screen or action declaration they name. A
  registration naming 38 or 39 refuses the batch (`ERR_UNSUPPORTED`, like every event tag the
  host has no encoder for).
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

- [D] The four records are part of the generated command codec (`commands_wire`, slot 15) beside
  `command.set_setting`: ids are nonempty UTF-8 of at most 128 bytes (`ERR_INVALID` empty,
  `ERR_NO_SPACE` longer, `ERR_UTF8`), `mode` above 3 is `ERR_INVALID`. A host that does not run
  them yet refuses a batch containing one with `ERR_UNSUPPORTED`, whole.
- [D] Authority (plugin freedom, no sandbox): `open_screen` and `run_action` may name any
  registered screen or action, own or foreign. A malformed id is `ERR_INVALID`, an id that is not
  registered `ERR_NOT_FOUND`; the whole batch is refused and nothing is queued. The owner is
  stamped by the host, never read from the record.
- [D] `redraw` and `close_screen` act on the submitting plugin's screen on top; outside its own
  screen they are `ERR_STATE` (whole batch refused).
- [D] Order: a batch is validated whole, then queued; queued commands run after the callback
  returns, in submit order, batches in order. Several `redraw` in one callback merge to the
  strongest mode.
- [D] **Chains are bounded (S6).** A callback the host runs because of a queued `open_screen` or
  `run_action` is one hop further in a chain than the callback that queued it; a user input or a
  host event starts a new chain at hop 0. A batch submitted at hop 8 that contains `open_screen` or
  `run_action` is `ERR_BUSY`, the whole batch is refused and the host logs it against the
  submitting plugin. Two plugins opening each other's screens therefore stop after 8 hops instead
  of looping, and no chain outlives the next user input.

### 3.1 Item state

| Tag | Record | Fields |
| --- | --- | --- |
| 60 | `command.set_item_state` | `item: str 96` (relative), `flags: u32`, `label: str 128`, `label_args: bytes 256`, `badge: str 128`, `badge_args: bytes 256` |

What a menu shows of an item now, where that changes while the plugin runs (interface 6
`set_item_state`): another label, a switch, faint, hidden or a badge. In interface 6 the host asked
a callback each time a menu showed the item; in interface 7 the plugin says it when it changes and
the host shows it from memory, so a menu never runs plugin code and a plugin that is not started
(lazy start) still shows its last kept state.

- [D] `item` is one of the submitting plugin's own actions or settings, relative to the plugin (the
  host prefixes the owner it knows, never a claim in the bytes). Another plugin's item cannot be
  named. A malformed id is `ERR_INVALID`; an id the plugin has not declared, `ERR_NOT_FOUND`.
- [D] `flags`, a closed set (other bits `ERR_INVALID`): bit 0 `hidden` (not shown in menus), bit 1
  `faint` (shown, cannot be chosen), bit 2 `switch` (shown with a switch), bit 3 `on` (the switch is
  on; without `switch` it is `ERR_INVALID`), bit 4 `keep` (below). `switch` on a setting is
  `ERR_INVALID`: a Boolean setting shows its own value, changed with `command.set_setting`.
- [D] `label` and `badge` are catalog keys of the plugin, empty for none: `label` is shown in place
  of the declared title, `badge` as a short text at the item's right (a count, "new"). Each key has
  its own argument batch, exactly the `args` of slot 27 (section 5: `i18n.arg_str`,
  `i18n.arg_int`, at most one `i18n.count`, which selects the plural form and binds `{count}`),
  empty for none; arguments without a key are `ERR_INVALID`. The host checks at submit that each
  key resolves with its arguments (statuses as slot 27, a missing key `ERR_NOT_FOUND`) and
  resolves them again, in the reader's language at that time, each time it shows the item. So a
  language change needs no new command. If a later resolution fails (catalogs changed), the item
  shows its title and no badge, never a raw key.
  A host may cache the resolved texts within the same budget, so a menu does not format every
  label and badge on each draw (16 items are up to 32 formattings, against the menu timing gate);
  it drops the cache on a language or catalog change and on the item's next state.
- [D] A record replaces the item's whole state; the default state (flags 0 or only `keep`, no
  label, no badge) removes it, the kept copy included. At most one record per item in a batch
  (`ERR_INVALID`); across batches the last one wins.
- [D] Rendering and the user's control are those of interface 6 `ItemState`: the user's menu
  edits decide where an item is listed; the state only changes how it is shown. `hidden` takes
  the item out of the places plugins give it, as `menu.hide` does: an item the user listed in a
  menu of their own stays shown there. The menu editor still lists a hidden item and says that
  its plugin hides it. An item whose `menu.need` is not met stays left out whatever its state.
- [D] Like the other commands it is validated with its whole batch and applied after the callback
  returns, in submit order. It runs no plugin callback, so it is accepted at any hop. A menu on
  screen that lists the item is redrawn once per drain, however many states changed.
- [D] Lifetime: without `keep`, a state ends at the plugin's teardown (section 7). With `keep`,
  it survives teardown (a stop, a lazy unload) and is shown whenever the item is; while the user
  has the plugin switched off its items are not in menus, and the kept state waits for it to be
  switched on. The host keeps kept states in a host-owned file and restores them at boot before
  any menu shows, without starting the plugin. Kept states are dropped when the plugin is updated
  or uninstalled (its keys may be gone) and when it sends the default state.
- [D] **Writing the kept file.** Only when the kept set changed, at most once per drain and at most
  once a minute per plugin (a newer kept state waits for the next allowed write), and once more
  before sleep or power-off if one is waiting; always through the host's atomic replace. A badge
  that changes often therefore wears the card no more than once a minute.
- [D] **Restoring the kept file is reading untrusted input.** The file names the code it was kept
  for (the plugin's version and its image's SHA-256, or the build for a linked plugin); a file for
  other code is dropped, so an update copied onto the card by a computer, which runs no update
  step, cannot restore states for keys that are gone. Every restored state is checked again with
  the command codec, the same limits and the declared-item check. A damaged, oversized or unknown
  file is deleted and logged once; boot never fails on it. If the kept states of all plugins
  together exceed the device limits, the host restores them in a fixed order (plugin id, then the
  order in the file) and drops the rest.
- [D] Limits per plugin: at most 16 items with a state, and at most 2,048 bytes of item ids, keys
  and arguments together (X4; a profile may allow more). Device-wide the host holds at most
  128 items with a state and 6,144 bytes of item state, its index included (about 24 bytes an
  item), in the shared `HostResources` (X4), whether a plugin runs or only its kept states were
  restored. Limits are checked on the batch's net result: a record that replaces an item's state
  counts the new state instead of the old, and one that removes it frees its share, so a plugin at
  its limit can still change or clear its items. A batch whose result would exceed a limit is
  `ERR_FULL`, whole. Storage is reserved fallibly (`ERR_NO_MEMORY`).
- [D] Until a host shows item states, it refuses a batch containing this record with
  `ERR_UNSUPPORTED`, whole (after the codec's checks).
- [D] The command codec checks the item (nonempty, at most 96 bytes, UTF-8), the flags (closed set,
  `on` needs `switch`), the keys (at most 128 bytes, UTF-8) and the argument batches (at most 256
  bytes, none without a key); the host decodes the argument batches with the records codec.

## 4. Fonts and text

- [D] `font.acquire(key)` keys: a registered font id (`Fonts::get` grammar) or a **role key**,
  one per role of `builtin::fonts` that interface 6 lets plugins draw in. No other role exists:
  an unknown role is `ERR_NOT_FOUND`.

  | Key | Resolves to (interface 6 call) |
  | --- | --- |
  | `role:ui` | `Fonts::role(fonts::UI)`, with the UI ink |
  | `role:reading` | `Fonts::role(fonts::READING)` |
  | `role:reading-bold` | `Fonts::bound(fonts::READING_BOLD)`, else `role:reading` |
  | `role:reading-italic` | `Fonts::bound(fonts::READING_ITALIC)`, else `role:reading` |
  | `role:reading-bold-italic` | `Fonts::bound(fonts::READING_BOLD_ITALIC)`, else `role:reading-bold` |
  | `role:status` | `Fonts::role(fonts::STATUS)` |
  | `role:icons` | `Fonts::bound(fonts::ICONS)`, no fallback |

  `role:icons` acquires even while no icon font is bound; a text call through it answers
  `ERR_NOT_FOUND` while none is (the plugin shows text only, as interface 6's menus do), checked
  again at each call.

  A role is resolved again at each text call, so it follows the user's choice and never goes
  stale. Bolder or monospaced text is a font the plugin acquires by id (a font pack), not a role.
- [D] Handles: at most 8 font handles per plugin, roles and font ids together (`ERR_FULL`
  beyond; a reading plugin holds five roles). This cap is the font family's own: blobs and
  operations count against their families' caps, not against it (section 7). X4 cost: a role
  handle is a few bytes; a handle by id keeps its id (<= 128 B) and its counter, so up to about
  1 KiB per plugin for 8; `handle.release` frees; teardown frees
  all. A handle acquired by font id answers `ERR_STATE` on `text.*` once that id is gone: the
  pack that provides it was removed or replaced (updated) after the acquire, even if the same
  id is registered again, or the id no longer resolves. The plugin acquires again (on
  `event.screen_shown`). How the host tells a replaced pack from the one acquired is its own
  (today: a removal counter per providing plugin, recorded in the handle; the counter never wraps
  to a value a handle holds: it saturates, and from then on every handle by id is stale); it
  never compares addresses or list positions.
- [D] Metrics, ink and nested clipping (Battery parity, #914):

  | Id | Entry | Arguments (after `ctx`) | Result |
  | --- | --- | --- | --- |
  | 29 | `text.metrics` | `font: handle`, `size: f32`, `ascent`, `descent`, `line_height: out_f32` | status |
  | 30 | `text.ink` | `font: handle`, `size: f32`, `text: bytes` (<= 4096), `top`, `bottom`, `left`, `right: out_i32` | status |
  | 31 | `canvas.clip` | `x: i32`, `y: i32`, `width: u32`, `height: u32` | status |

  - `text.metrics`: ascent and descent (both positive, from the baseline) and the line height, in
    pixels, of the size the text is drawn at. Works outside `draw`.
  - `text.ink`: the ink of `text` at `size` as this host draws it on its own screen, counting the
    pixels that screen shows as ink (coverage at or above its black threshold: the panel's
    `mono_coverage` on 1-bit screens, half on gray and colour): `top` and
    `bottom` relative to the baseline (above negative), `left` and `right` from the pen's start.
    Text without ink (spaces, empty) is `ERR_NOT_FOUND` with the outputs unchanged. Works
    outside `draw`. The numbers come from the host's own rasteriser (hinting, coverage, pack),
    so they can differ by a pixel or more between hosts (X4, desktop, Android); a plugin lays out
    by them on the host it runs on and never compares them with stored values.
  - Outputs of slots 29 and 30: a null or misaligned output pointer is `ERR_INVALID`. The host
    writes all outputs together and only on `OK`; on any error every output is unchanged.
  - `canvas.clip`: the clip of this draw call becomes the frame's clip intersected with the
    rectangle. Each call replaces the previous one (it can widen again up to the frame's clip,
    never beyond); an empty rectangle draws nothing. It resets when `draw` returns; outside `draw`
    it is `ERR_STATE`. Nesting is the SDK's: a nested canvas sets its parent's rectangle
    intersected with its own and restores the parent's when it ends.
  - A stale font handle is `ERR_STATE`, `size` must be finite and > 0, as for `text.measure`.
  - Host implementation notes: `text.ink` over up to 4096 bytes uses a bounded scratch buffer (no
    allocation per glyph) and is timed on the X4 before it is wired there; `canvas.clip` does
    its bounds arithmetic in 64 bits (`x + width` cannot overflow).
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
- [D] Host bounds the result to 4 KiB without result-sized stack storage; validation is complete
  before the first lookup. A counting pass precedes direct output writes.
- [D] The limits are generated from `abi.def` (`i18n_limits`, records codec): key 128 bytes
  (`I18N_FORMAT_MAX_KEY_BYTES`), argument batch 1024 bytes (`I18N_FORMAT_MAX_ARGUMENT_BYTES`) of
  at most 8 records (`I18N_FORMAT_MAX_RECORDS`), digits text 4096 bytes (`I18N_DIGITS_MAX_BYTES`).
  Host and SDK use these constants, never another family's cap.

## 6. Callback replies and draw input (N1, N2)

```
dispatch_event(state: pointer, token: u32, input: bytes, out: buffer) -> i32
draw(state: pointer, token: u32, input: bytes) -> i32
```

- [D] **Reply buffer (N1).** The host lends `out` for the duration of the call. A result >= 0 is
  the number of reply bytes written; a result < 0 is a status. A result greater than `out.cap` is
  a plugin fault: the host treats it as `ERR_INVALID` and reads no reply. The plugin may write
  only the first `out.cap` bytes. It must not keep `out` after returning.
- [D] **What `out` is (S2).** `out.ptr` is never null, also when `cap = 0` (then it is a dangling,
  aligned, non-null pointer), and its `cap` bytes are initialised (the host zeroes them before the
  first call), so the SDK may view it as `&mut [u8]`. `out` never overlaps `input`, any host scratch
  that a slot uses during the call, or the `out` of a nested call: the host keeps one reply buffer
  per nesting depth (on the X4 two depths, each sized for the largest reply of the families that
  can dispatch at that depth), never one shared buffer.
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
  `clip_bottom` give the rows this call may change, half-open `[clip_top, clip_bottom)` and always
  within the rectangle (`y <= clip_top <= clip_bottom <= y + height`): on the X4 these are the band
  being rendered, elsewhere the whole rectangle. An empty clip (`clip_top == clip_bottom`) is
  never sent.
- [D] `purpose`: 0 screen, 1 page layer, 2 inserted block, 3 status item, 4 widget. Other values
  are reserved: a plugin that does not know the purpose draws nothing and returns 0.
- [D] `id` and `payload` are defined by the owner of the purpose, for example the reader's insert
  id. A screen gets `id = 0` and an empty payload.
- [D] Drawing outside the clip is clipped by the canvas slots, not refused.

## 7. Lifetime and cancellation (all handles and async operations)

This rule binds every handle and every asynchronous operation of interface 7, present and future:
font handles, blobs, images, format and document handles, work, HTTP, sockets and stage
subscriptions. Families added later refer to it instead of defining their own.

- [D] **Handles are opaque and never reused (S3).** A handle is an opaque `u32`; 0 is never a
  valid handle, and its layout is the host's, never part of the contract. The host never hands
  out a released value again for the same plugin, so a stale or doubled release can never name a
  newer handle: it is `ERR_STATE` and changes nothing. (Today's host keeps a slot and a
  generation in the value and retires exhausted slots; hosts may change that freely.)
- [D] Every handle is released exactly once: by the plugin, or by the host when the plugin stops
  or its construction is torn down. A second release, or a release of a handle the plugin does not
  own, is `ERR_STATE` and changes nothing.
- [D] A provider stopped in the middle of an operation gets one cancel event for it and is never
  called back for that operation again. Requesters see the operation end with `ERR_CANCELLED`.
- [D] A late reply or completion (after cancel, stop or timeout) is refused with `ERR_STATE`, and
  the host drops it unread.
- [D] Stop order: the host cancels open operations, then releases handles, then drops the state.
  No callback runs after `drop`.
- [D] **Teardown bug path.** The shell always finishes teardown (cancel, release, `drop`) before it
  frees an owner. If it does not, and a cancel or release step fails then, the host reports it
  once, skips the step, still calls `drop` exactly once and unmaps the plugin. That is safe only
  because no host resource borrows the plugin's image or memory past a call: what a skipped step
  leaves behind is host-owned, bounded per plugin and unreachable under a never-reused owner id.
  A future family whose host resource does borrow the plugin's image or memory must make its
  release non-skippable and say so in its own spec.
- [D] **Async operations (D3, #890).** Every asynchronous operation is a handle. Its completion
  event is a level-triggered hint: it may be coalesced and never carries the only copy of the
  result. The result stays readable through the handle until `handle.release`. Stop or unload
  cancels every open operation, and nothing is called back afterwards; a late completion is
  refused (`ERR_STATE`). Releasing the handle of a running operation cancels it first. Live
  handles and operations per plugin are capped **per family** (fonts: section 4; each later family
  states its own); there is no cap shared across families. Today: font handles 8 (section 4); blobs
  2 live per plugin within the device's blob budget (events proposal D3.4, S6 on the X4); service
  handles one per declared service key (at most `metadata.max_services`). One past a cap is `ERR_FULL`. This
  replaces ADR 0034's X4 profile line "four persistent handles per plugin" (and its use in the
  proposals as a cap shared with fonts). SDK glue builds on
  exactly this.
- [D] **Dropped events (D6).** `event_dropped` applies only to families whose contract says they
  may drop (today: the lifecycle events of section 1.5 in the events proposal). Other families
  deliver or fail the request; the lifecycle drop rule does not generalise.

## 8. Tag space (N3)

| Family | Ranges |
| --- | --- |
| registration, tick | 1-3 |
| `command.set_setting` | 4 |
| declarations (`setting`, `resource`, `action`, `menu`, `screen`, `icon`, `format`, `registration`, `stage.spec`) | 5-31, 224-511 |
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
  the whole batch, for every plugin including the one the grant names.
- [D] **Own-grant reads (identity route).** A plugin reads its own grant with `settings.read` on the
  short key `core.grant.<kind>`, without a plugin id. The host takes the caller from `ctx`, the
  owner of the current dispatch frame, and answers a boolean: true only when the user granted that
  kind to this plugin. The plugin never names itself, so it cannot name another plugin either; there
  is no plugin id in the snapshot and no extra slot. Rules:
  - A read of any `core.grant.<kind>.<id>` key, the caller's own id included, is `ERR_DENIED`, so
    there is exactly one spelling.
  - An unknown `<kind>` is `ERR_UNSUPPORTED` (a newer plugin on an older host).
  - The value is a plain yes or no. A grant change, revoke included, shows on the next read; no
    event announces it, and nothing may be cached on the plugin side.
  - A read outside a dispatch frame (no callback running) is `ERR_STATE`, like every
    `settings.read`.
  - Another plugin's grant reaches a plugin only where the host stamps it into a host-built record
    (N4: `style_granted` in each stage reply).
  - The SDK offers one typed read, `settings::own_grant(host, GrantKind) -> Result<bool, Error>`,
    with a `#[non_exhaustive]` `GrantKind` (`Style`, `Install`).

## 13. Statuses, features and service families (D1, D2, D4, D5)

- [D] **New statuses (D2).** `ERR_DENIED` (-12): the plugin did not declare the feature, or the
  user refused (consent, a guarded grant). `ERR_TIMEOUT` (-13): a network or socket operation ran
  out of time. Both stay distinct from `ERR_UNSUPPORTED` (the host lacks the function) and
  `ERR_CANCELLED` (dismissed, stopped). SDK status types name both from 1.0 on.
- [D] **Feature records (D1).** A plugin states the families it uses as metadata feature records,
  `uses.<family>` (`uses.net`, `uses.secrets`, `uses.sockets`, `uses.library`, `uses.registry`,
  `uses.device`). Bit 0 of the record's `flags` means required. Feature revisions only add: a host
  at revision N serves every request for revision N or lower. A feature the host lacks, or has
  only at a lower revision than the record asks for, is unavailable: an optional one is left out
  and the plugin learns it is missing at run time, a required one refuses the load and names the
  key. So a feature can move to a new revision without breaking plugins built for an older one.
  Other bits are refused. See `abi/metadata.md`.
- [D] **Service families (D4).** Every later family (NET, LIB, REG, FS, SEC, DEV and the rest) is a
  versioned service schema, not a set of one-off slots. Slot 17 (`services.query`) stays the
  snapshot query over blobs; calls go through
  one generic slot, `service.call(ctx, service, op: u32, input, out) -> count` (synchronous and
  bounded); asynchronous work returns an operation handle under section 7. The slots
  (`service.open` 33, `service.call` 34) and the schema generator are in `stages-services.md`.
- [D] **Limits (D5).** A plugin may require up to 32 services (was 16) and 32 features.

## 14. Memory (X4)

- Input/screen records and `draw.frame` are encoded on the host stack (a framed `draw.frame` is at most about 300 bytes).
- Screens/actions: S2 index entries only (image path); about 16 bytes resident each.
- i18n: counting and direct output passes, no 4 KiB host stack scratch and nothing retained.
- Reply buffers are lent from the existing host scratch, nothing per plugin.
- Item state (section 3.1): at most 2,048 bytes of ids, keys and arguments per plugin plus about
  24 bytes of index per item (16 items: about 2.4 KiB); device-wide at most 128 items and 6 KiB,
  index included. Kept states live in one small host file per plugin, read at boot into the same
  budget.

## 15. Work after this contract

- Host adapters (Core):
  - screen/action apply (S2);
  - `input` dispatch and handled routing;
  - `dispatch_event` with the lent `out` and `draw` with `draw.frame`;
  - command execution for 56-60 (60: the item state store, its kept file and menu rendering);
  - font role resolution;
  - slots 27/28;
  - section 7 teardown order.
- SDK (Services/Plugin):
  - screen/action declaration builder entries;
  - `native::Plugin::input`;
  - `CommandSender::set_item_state` (section 3.1: flags, keys and `formatting::Arg` batches);
  - reply writing and `draw.frame` decoding;
  - input/event decoding;
  - i18n/fonts wrappers;
  - section 10 in the event glue.
- Follow-up PR (additive at draft 5): N4 stages and N6 images.
