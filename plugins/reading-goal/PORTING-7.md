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

## Not ported yet (each is an ignored test in `tests/acceptance_pending.rs`)

- The stage `stats.read-today` (from the stats port).
- The data folder handed to `store` by the shell (`core.files`/`DataStorage`, PR #339).
- Images (draft 22) and the widget host call (Core todo 76) for the widget and the
  sleep screen; the sleep plugin on interface 7.
- The plugin list and restart for the offer to switch the statistics on.
- Native registration and `declarations.rs` for interface 7 (`tests/abi7_declarations.rs`
  keeps the title encoding parity of the old preparation).
