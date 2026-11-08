# Reading goal on interface 7 (partly ported)

Status: **not complete.** The interface-6 plugin (`lib.rs` registration, `widget.rs`,
`sleep.rs`, `pictures.rs`) is still in place, because the host services it needs
are not there. It is removed in the PR that finishes the port.

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
  Declares `uses.storage` (`today.txt`, root 0) and nothing else. Nothing in its paths allocates.
- Settings (`declarations::settings`, pinned in `testdata/declarations.bin.v1`): `content` (5 to 95,
  50), `happy` (50 to 200, 100) in percent with steps of 5 as sliders, and `pictures` (text, the
  folder chooser slot), all advanced, in the statistics' group `stats.group` as on interface 6.
- Events: a book opened, a book closed, suspend and stopping. The edges are `edge()`, which
  maps events to `Edge`s (tested without a host), and `Kept::apply`.
- **The figure comes by the stage `stats.read-today`** (schema 1, payload: day `i32` and seconds
  `u32`, little-endian; anything else is ignored). Reading goal *subscribes* to it. The contract
  (`docs/abi/stages-services.md`) lets only the stage's owner run it, so the statistics have to
  *run* it when the figure changes, with the figure as input; they do not pull. Stats still
  answers the stage as the asker's pull (stats `PORTING-7.md`): that change is on the stats side
  and is asked of Kernel (see the report). A host that does not admit stage subscriptions yet
  leaves the subscription out; `make` tolerates exactly that refusal (`ERR_UNSUPPORTED`).
- The write is best effort like interface 6's (`.ok()`): a failed write is not told; the figure is
  kept and written at the next edge.

### Differences from interface 6 (on purpose)

- The goal default `stats.goal.20` that interface 6 set through `set_setting_default` has no
  declaration yet: until the statistics declare `stats.goal` the cat goes by the 20 minutes
  without a goal, as it did with the goal off, and only the minutes show.
- Today is not compared with the clock here: that is the widget's, with the day from the
  snapshot (UTC until draft 25, as in stats).

## Not ported yet (each is an ignored test in `tests/acceptance_pending.rs`)

- The statistics running the stage `stats.read-today` with the figure (stats side, see above).
- Images (draft 22) and the widget host call (Core todo 76) for the widget and the
  sleep screen; the sleep plugin on interface 7.
- The plugin list and restart for the offer to switch the statistics on.
- The sleep screen's declaration (`declarations::screens`, token 10; `tests/abi7_declarations.rs`
  keeps its identity parity) joins the image when it can draw.
