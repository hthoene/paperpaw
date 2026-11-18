# Tasks on interface 7

Status: **complete, with the gaps named below.** A new store plugin (#136), never published on
interface 6, version 1.0.0, written for interface 7 with no interface change. The CalDAV and iCalendar
code is the crate `crates/paperpaw-caldav`, which the calendar plugin (#133) shares.

## Where things are

- `declarations.rs` (built into the image by `build.rs`): the settings group (order 91: *Server address*
  and *User name*, both edited on the account's screen, and *Keep done tasks (days)*), the screens
  `tasks.screen` and `tasks.account`, the actions `tasks.open` and `tasks.sync`, the Tools entry (order
  81), the four widgets (`core.widgets.personal`) with the shared look and their settings, and the
  events subscribed (start, wake, a setting, work due, the end of a request).
- `src/model.rs`: the lists and tasks in memory, their order, what waits for the server (marks on the
  task), and the text of the file. No host, no clock, no card.
- `src/keep.rs`: `tasks.txt` in the data folder, read a line at a time and written whole or not at all.
- `src/sync.rs`: a run against a server, one request at a time, through the trait `Wire`.
- `src/when.rs`: the day a typed text means.
- `src/native_plugin.rs` (`NativeTasks`), `native_screens.rs`, `native_asks.rs`, `native_widgets.rs`,
  `native_secret.rs`, `native_entry.rs`: the glue, the rows of the two screens, the questions and their
  answers, the widgets, the password among the secrets.

## The rules of a sync

1. **Where the lists are.** `PROPFIND` (Depth 1) at the address set. An answer without a calendar of
   tasks may name the account (`current-user-principal`) or its calendars (`calendar-home-set`): those
   are followed, three steps at most, so the server's own address is enough. The address found is kept.
   Every calendar that holds `VTODO` (or does not say what it holds) is a list.
2. **What changed there.** A list whose change tag (`getctag`) is the one kept is not read. Any other
   is read whole with one `REPORT` (`calendar-query` for `VTODO`, ETag and text), saved to `sync.xml`
   and read as it streams: never in memory.
3. **What changed here** is sent a task at a time: a new one with `PUT` and `If-None-Match: *`; a
   removed one with `DELETE` and `If-Match`; a changed one is fetched (`GET` to `item.ics`), changed in
   its own text (`paperpaw_caldav::patch`: only `SUMMARY`, `DUE`, `PRIORITY`, `STATUS`, `COMPLETED`,
   `PERCENT-COMPLETE` and the stamps are replaced, as far as they changed) and sent back with `PUT` and
   `If-Match`. So nothing on the server is written over unseen, and notes, alarms and what other
   programs wrote stay as they are.
4. **A task changed on both sides: the server's version wins, and the reader's is kept as a new task**
   in the same list (sent as such). The same where the server removed a task the reader changed. A
   task the reader removed and the server changed comes back. A list the server no longer has goes,
   but tasks of it that were changed here and never sent stay, in a list of the reader's own.
5. **The offline queue** is the marks on the tasks, in the file: whatever is not sent (no network, a
   refusal, the power) is sent in a later run. A run that sent a task and lost the power before it
   wrote that down finds the task on the server as it is on the reader and makes no second one.
6. An answer that ends before its end (`</multistatus>`) removes nothing.
7. A refused name or password (401, or 403 where the lists are looked up) stops runs that start by
   themselves until the account changes.

## Limits, so that memory is bounded

- 12 lists, 200 tasks in all; a title 160 bytes, a list's name 60; an id 120, a path 240, an ETag 96
  bytes (a server's task with more is not kept, and said). A task is one allocation.
- Done tasks stay for *Keep done tasks* days (7), then leave the reader; on a server they stay.
- An answer is at most 2 MiB (a file), one task's text 256 KiB (a file, streamed through the patcher
  96 bytes of a line at a time). A request body is inline up to 2,048 bytes, else the file `put.ics`.
- Tasks that repeat (`RRULE`, `RDATE`, `RECURRENCE-ID`) are shown and not changed here.
- Times with a `TZID` are read as the reader's own time; times in UTC are moved to it.

## How it stays snappy

- A key press never waits for the network and never reads the card: the tasks are in memory, a change
  is one write of the file (one storage call up to 4 KiB, which is about 25 tasks), and syncing is
  work: at once when the lists are opened (at most every five minutes) or on *Sync now*, within half an
  hour of a change (sooner when the network comes up), and an optional run an hour on when the radio is
  on anyway.
- The widgets draw from memory; the host is told a widget changed when a task did, and just after
  midnight (one work timer, only while a widget is placed).

## Gaps, on purpose

1. A task's notes (`DESCRIPTION`) are kept on the server but not shown.
2. Lists are made on the server elsewhere (no `MKCALENDAR`); a list made on the reader stays on the
   reader, also with an account.
3. A task is not moved between lists.
4. The password is sent as HTTP Basic (over `https://` unless the address says `http://`); no OAuth.

## Tests

- `crates/paperpaw-caldav`: lines unfolded in pieces of any size, a server's `VTODO` read, a to-do
  written and read back, the patcher (everything else byte for byte, folds, very long lines), the XML
  scanner, a multistatus answer, addresses and ETags.
- `src/model.rs`, `src/when.rs`: the marks, the order, the days done tasks stay, the file's text.
- `src/keep.rs`: a power cut at every storage call of a save leaves the old file or the new one whole.
- `src/sync_tests.rs`: the engine against a CalDAV server in memory (lists found from the server's
  address, both directions, the offline queue across a restart, a conflict both ways it can be seen,
  removed there and changed here, a power cut after a sending, an answer cut short, a server without
  ETags, a refused password).
- `tests/native_flow.rs` (the scripted host), `tests/native_linked_entry.rs`; `tools/pps/tasks.pps` runs
  the screens, the keyboard and the widgets on the emulator.
