# Reading goal on interface 7

Status: **complete, with one named gap.** The plugin is the native plugin only: the interface-6
plugin (`lib.rs` registration, `widget.rs`, `sleep.rs`, `pictures.rs`, `mood.rs`), its integration tests
(`tests/goal.rs`, the parity tests against the interface-6 registration), its registration in
`paperpaw-app`, its store-file builder (`tools/plugin-files`, `plugins.txt` keeps a comment line) and the
feature `embedded-pictures` (the app no longer carries the plugin, so the cats are not built in) are gone.
**One thing of Beta 3 is not there until the host has it** (an ignored case, see *Blocked*):

1. **The cats installed with the plugin** (`pictures/<mood>.png` in the plugin's install folder) need
   `PrepareRoot::Bundle` (`image_prepare` root 2; Services todo 308, Host todo 309). Until it exists the
   cat shows only from one's own folder (`reading-goal.pictures`, any size, `.png`/`.jpg`/`.jpeg`, through
   the library root) and otherwise as the mood in words (what interface 6 did where the file was missing).

## Ported (no host needed): `src/model.rs`

- `Mood` (sad, meh, content, happy) by the share of the goal, thresholds as set
  and clamped; `mood.rs` re-exports it. Tests moved over unchanged, with more edges.
- The goal from the statistics' setting (`stats.goal.30` → 30, `.off` → none),
  `Today::of`: only today's seconds count, 20 minutes without a goal, thresholds
  clamped (content 1–99, happy 1–1000) as the settings are.
- `today.txt`: `parse_told` (exactly `<day> <seconds>`, else nothing) and `told_text`
  (into a caller buffer, no allocation, 32 bytes suffice).
- `Kept`: when what the statistics told is written (not while a book is open; at
  book close, suspend and stop; only if the file differs).
- Nothing allocates or can panic; texts go into caller-owned buffers.
- Settings, texts (en/de) and `how` are unchanged.

## Ported (over `Storage`, no allocation): `src/store.rs`

- `read_told` (a file of at most 64 bytes, exactly `<day> <seconds>`, else nothing) and
  `flush` (writes what `Kept` holds where the file differs, in one call; `Full`/`Io`
  told, the file as it was). The names are the short ones of the native data folder
  (root 0), `today.txt`. Interface 6's `told` also accepted what `parse_told` refuses
  (it split the trimmed text once the same way, so the accepted files are the same).

## Ported (the native shell): `src/native_plugin.rs`, `declarations.rs`

- `NativeReadingGoal` (`native_plugin!`, features `linked` and `native-entry`, metadata in `native/`
  — regenerate with `python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json
  --bytes native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).
  Declares `uses.storage` (`today.txt`, root 0) and nothing else. The host calls the plugin makes (heap, registrations, storage, `stage.run`) are listed in `metadata.json`; the bytes only carry the lowest ABI minor they need, so adding a call of minor 1 leaves `.bin` and `.inspected.json` as they are (`tools/tests/test_abi_plugin_metadata.py` regenerates every plugin's and fails on a stale one). Nothing in its paths allocates.
- Settings (`declarations::settings`, pinned in `testdata/declarations.bin.v1`): `content` (5 to 95,
  50), `happy` (50 to 200, 100) in percent with steps of 5 as sliders, and `pictures` (text, the
  folder chooser slot), all advanced, in the statistics' group `stats.group` as on interface 6.
- Events: a book opened, a book closed, suspend and stopping. The edges are `edge()`, which
  maps events to `Edge`s (tested without a host), and `Kept::apply`.
- **The figure is asked for, by a stage Reading goal owns** (Kernel's ruling): `reading-goal.read-today`
  (schema 1; input: the day as `i32`, little-endian, empty meaning today; reply: the day `i32` and
  the seconds `u32`, little-endian; `max_input` 4, `max_reply` 8, one record). The statistics
  *subscribe* and answer from memory (no card read, `ERR_NOT_FOUND` for a day they do not know);
  nothing re-enters because the owner cannot subscribe to its own stage. Reading goal runs it (reading every subscriber's reply, hearing only `stats`) when it
  is about to write: at a book closed, suspend and stopping (the widget and the sleep screen will
  also on drawing and on the minute tick). No answer (`ERR_CANCELLED` when the statistics have not
  started yet, `ERR_NOT_FOUND`, busy, no stage, a malformed reply) leaves what was kept: `figure()`
  is tested for each. Not pushed by the statistics on every page turn (X4 stack and time). A host that
  does not admit stages yet leaves the stage out; `make` tolerates exactly that refusal
  (`ERR_UNSUPPORTED`). The statistics' side is in `plugins/stats` (see *Ported: the statistics' side*).
- The write is best effort like interface 6's (`.ok()`): a failed write is not told; the figure is
  kept and written at the next edge.

### Differences from interface 6 (on purpose)

- The settings are compared field by field with what interface 6 registered (`tests/settings_parity.rs`).
  The widget's own setting (its frame, `reading-goal.frame`) is not declared: it comes with the widget.

- The goal default `stats.goal.20` that interface 6 set through `set_setting_default` has no
  declaration yet: until the statistics declare `stats.goal` the cat goes by the 20 minutes
  without a goal, as it did with the goal off, and only the minutes show.
- Today is not compared with the clock here: that is the widget's, with the day from the
  snapshot (UTC until draft 25, as in stats).

## Ported (the widget): `src/native_widget.rs`, `declarations::widgets`

- *Reading cat* (`reading-goal.widget`, category *reading*, 2 x 2 cells, live) is declared with its
  group and the eight settings of interface 6 (frame without a default frame, padding, paper, ink,
  corners, frame line, shadow, and the part *minutes*); `tests/widget_declarations.rs` compares the
  widget and every setting with the interface-6 registration. The toolkit's `Cx` reads them and
  draws the frame (`paperpaw_ui_kit::widget`, as the widgets plugin does).
- The host asks the height (the frame, half the width for the picture, the line of minutes), where it
  sits and its one target (the whole widget) through `core.widget`; it draws (`DrawPurpose::Widget`)
  and sends the tap to the same token.
- It shows the picture (below), or where there is none the **mood in words in the picture's place**
  (what interface 6 did where the file was missing) and below it `12 / 20 min`, `12 min` without a
  goal, or *Statistics off* where they do not count (the part *minutes* switches the line off).
  The pixels need a host that composes widgets: the ignored case
  `the_widget_shows_the_cat_of_todays_mood_and_the_minutes` names that.
- The figure the widget shows is the one kept (what the statistics told, else `today.txt`) with
  today by the local time; the plugin asks the statistics' stage each minute (the tick, only while a
  widget is made), keeps and writes it as at a book closed, reads whether the statistics count
  (`core.registry`, the `stats` plugin running, off or not installed), and tells the host
  (`widget_changed`) only when what shows changed (a number of minutes, goal, mood and state, never
  drawn or measured for it).
- A tap: where the statistics count, the settings open on `stats.goal` (hand-over `focus`, slot
  `core.settings`); where they are off, a question offers *switch them on (PaperPaw starts anew)* or
  *not now*, where they are missing *get them from the store* or *not now*. The first answer is
  done (`set_plugin_enabled stats` and a restart, which the shell may refuse, `uses.power` is not
  required; or `store.open`); a second tap while the question is open asks nothing. Run over the
  scripted host in `tests/scripted_offer.rs` (the staged command is checked); the settings opening is
  a screen command the scripted host does not stage (BLOCKED H1: acceptance case
  `the_widget_is_drawn_and_a_tap_opens_the_goal`).
- Differences from interface 6, on purpose: the picture's description for screen readers (Role::Image)
  is not given while there is no picture; the picture setting `pictures` and the folder stay as
  declared. The widget reads the goal setting at each draw (no cache), as the widgets plugin does.
- The interface-6 `widget.rs`, `sleep.rs`, `pictures.rs` and the registration in `lib.rs` stay with
  the sleep screen's port; they go in the PR that finishes it (no shim).

## Ported: the cat's picture (`src/native_picture.rs`)

- Which file to ask the host for, and what its answer leads to, is decided without a host (`Pictures`,
  `candidate`, `steps`, `Next`; tests in `src/native_picture/tests.rs`). The order is interface 6's: one's own
  folder (`reading-goal.pictures`, `<mood>.png`, `.jpg`, `.jpeg` in either case, through
  `PrepareRoot::Library`, which needs `uses.storage`: declared), then the cats of the plugin's install
  folder (`Source::Bundle`, left out until `PrepareRoot::Bundle` exists: `root_of` in `native_plugin.rs`).
  The picture is asked for as large as the widest widget is drawn (its measured width; the screen's size
  before any is measured).
- One request is open at a time and one picture is held: the one of the mood the widget shows. The cookie is
  the mood and the step, so an answer to anything else is ignored. A refusal for memory or the card is asked
  again at the next minute; all files tried leaves the mood in words and is not asked again until the folder
  setting changes (`event.setting_changed` for `reading-goal.pictures`; the held picture is released then).
- Asked at the first measure of a widget and at each minute tick (never in a draw callback). When the picture
  is ready the plugin releases the one it held and tells every widget's maker (`widget_changed`). The held
  picture is also released when the last widget is dropped and when the folder setting changes (which also
  forgets the request open: its late answer is given back at once).
- The widget leaves the room above the minutes free and draws the picture over it with `canvas.draw_image`
  (`ImageStyle::new()`: scaled to fit, centred); a host that refuses it leaves the frame and the minutes.
  Metadata lists `canvas.draw_image`.

## Ported: the statistics' side

- The statistics (`plugins/stats`) subscribe to `reading-goal.read-today` (token 5) and answer it with the
  day and the seconds read on it, for nothing or today's day asked; another day is not found. Their own
  stage `stats.read-today` is gone.

## Ported: the sleep screen (`src/native_sleep.rs`, `declarations::{screens, navigation}`)

- *Today's reading cat* is a screen of its own (token 10, one instance), offered to the slot `core.sleep` at
  priority -1 (below the sleep plugin's own, priority 0; Kernel's ruling) with a `setting.choice`
  `reading-goal.sleep` on `sleep.show` (title: the same key, "Today's reading cat"). The native `sleep` (PR #649) binds
  `core.sleep` to this screen when `sleep.show` is `reading-goal.sleep` (at start and when it changes) if it is a
  current candidate, else it falls back to the cover or the cat. The screen gets no input while asleep and does
  not close itself: the host replaces it on wake (Kernel's ruling, as Port F put it).
- It draws (`DrawPurpose::Screen`): paper, the cat as large as the room leaves with its shape kept and centred,
  the minutes right under it as it is fitted (not under a box), or the mood in words above the minutes where
  there is no picture (yet). The layout is `native_sleep::layout` (tested without a host).
- **Asking early:** where `sleep.show` is this screen, the picture is asked for at the start (`event.started`),
  when a book closes, when the screen opens and at each tick while a widget is there, so the first sleep shows
  the cat as in Beta 3 once the picture is held. It is held while a widget lives **or** the screen is chosen;
  the last widget dropping with this screen chosen keeps it. **Limit:** a sleep screen that opens with no
  picture held yet (a first start straight into sleep) draws the words that time.
- **Known gap (gray view):** interface 6 drew white paper and black ink and said `grays()` so the panel showed the
  picture in grays. The native screen uses the theme's paper and ink (`Style`) and gives the host no grays hint,
  so on a panel with grays the picture may show in black and white only. Left for the host's screen flags to
  carry (no SDK flag for it yet).

## Blocked (each is an ignored case in `tests/acceptance_pending.rs` or `src/native_picture/tests.rs`)

- `PrepareRoot::Bundle`, Services todo 308 and Host todo 309: the installed cats.
- Hosting of the sleep screen: a host that admits screens, slot offers and foreign choices, with the native `sleep` (PR #649) binding it.
- A host that composes widgets and runs native plugins end to end: the pixels of the widget, and the tap
  that opens the settings on `stats.goal` (the scripted host stages no screen command).

## Heap gate and the first tick

- A widget drawn notes what it showed (`Item.shown`); the next tick that finds another figure tells
  the maker (`widget_changed`). Before, only a tick after a first tick could, so a widget drawn at
  12 minutes with 13 found at the first tick stayed stale. Tests: `native_plugin/tests.rs`
  (`settle`: told once, a refused notice tried again, a widget never drawn tells nothing).
- Each minute with a widget, `read_statistics` pages the registry for the statistics plugin (at most 16
  pages, each a 4 KiB request and a 4 KiB reply buffer, made once per call and reused, so 8 KiB at a time).
  This belongs in the heap gate's measurement of the plugin; it is not measured yet.

## Right-to-left (`ar`: a regression against Beta 3 until this is done)

Beta 3 mirrored the interface where its language is written right to left (Arabic). The UI kit now
does so for what it draws (`Style::rtl`, read from `core.language`: header, list rows, items, dialog,
chips, the sheet's pager; `Style::flip`, `flip_x` and `text::label_in` for the rest). What this plugin
draws itself is not mirrored yet, so its screens are left to right in those parts for `ar` until it
takes the kit's helpers (and, where it keeps its own keys, swaps left and right as `Chips::step` does).

## Widgets and the settings pages

The widgets are declared without `previews` (Kernel ruling, #228, as in Beta 3): they are offered in the Add picker, and a settings page does not show a sample of the widget. A preview-only widget would add one later.
