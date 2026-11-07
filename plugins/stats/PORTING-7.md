# Stats on interface 7 (partly ported)

Status: **not complete.** The interface-6 plugin (registration, screens, widgets, sync,
events) is still in place. It is removed in the PR that finishes the port.

## Ported: the data layer's files (`src/store.rs`, `src/files.rs`)

- File names are constant strings: logging a page allocates nothing.
- `days()` streams `days.bin` in 768 B chunks (no 1 MiB read), allocates fallibly, reads at
  most 4,096 kept days (48 KiB; a longer file is summed anew from the log, not refused),
  returns `Result` (`TooLarge` where the days do not fit) and writes the shortcut through
  `days.new` and a rename, so a cut leaves the old file, never a header covering days that
  are not there.
- `DataFolder` (`files.rs`) takes the plugin folder off the paths, so the layer runs on the
  host's root 0 (`native::storage::DataStorage`, PR #358) unchanged. `put_in_place` needs
  `rename`, which root 0 has. It goes when the layer's paths are the short ones.
- `books.ini` is loaded in id order with fallible allocation (no stable sort, which
  allocates) and written in pieces of at most 3.5 KiB, never held whole. Sections in a
  row of one id (also with empty sections between) still merge as in interface 6; a
  section of an id read before that is ignored, so the first book of an id is the book
  (interface 6 kept it too, by sorting stably and dropping later copies).
- Still infallible: `Value::as_str()` of `paperpaw-config` allocates for a quoted value with
  escapes. Its `quoted` flag is private, so it cannot be redone here cheaply; it wants a
  fallible variant in `paperpaw-config` (Kernel).
- Callers that read today's seconds (`read_today`) do not cache a failed read as 0.

## Not ported yet (follow-ups; each is an ignored case in `tests/acceptance_pending.rs`)

- The native shell: `Plugin` impl, declarations, `build.rs`, metadata, linked entry.
- Screens (overview, calendar, books, book) on `core.ui`.
- Widgets (today, week, ring, streak, calendar, year) and the status item (`left.rs`) on the
  widget and status stages (host call, Core todo 76).
- Events: page turned, book opened/closed/finished/moved, started/resume/suspend/stopping,
  and emitting `stats.read-today` as a stage for reading-goal.
- Sync (WebDAV conditional upload with ETag, credentials): `core.net` (#351) and secrets
  (Services todo 79).
- Allocation hygiene of `merge`, `extsort`, `koreader` (fallible).
- Folder listing (`sync` working-file sweep, `merge` tests): `core.files` (#339).
- The fixture `tools/dev/tests/fixtures/flows/stats.flow` (#321) points at the new screens.
