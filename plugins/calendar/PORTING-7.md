# Calendar on interface 7

Status: **complete, with the gaps named below.** A new store plugin (#133), never published on
interface 6, version 1.0.0, written for interface 7 with no interface change (draft 36). It shares
the crate `crates/paperpaw-caldav` with Tasks (#136): the content lines, the multistatus reader and
the addresses are Tasks'; this plugin added events (`event`), recurrence rules (`rrule`), time zones
(`tz`), contacts (`vcard`) and the `REPORT` bodies for a range of days and for contacts.

## Where things are

- `declarations.rs` (built into the image by `build.rs`): the settings group (order 92: *Calendars*, a
  row that opens the sources' screen, *Sync* and *Days in the agenda*), the screens `calendar.screen`,
  `calendar.sources` and `calendar.sleep`, the actions `calendar.open` and `calendar.sync`, the Tools
  entry (order 82), the sleep screen offered to `core.sleep` (and to `sleep.show`), the five widgets
  (`core.widgets.personal`) with the shared look and their settings, and the events subscribed.
- `src/model.rs`: the sources and the events in memory, their expansion for a range of days (repeats,
  exceptions, days an event goes on into, birthdays), the window, and the text of the file. No host,
  no clock, no card.
- `src/keep.rs`: `calendar.txt` in the data folder, read a line at a time and written whole or not at
  all (`docs/data-files.md`).
- `src/sync.rs`: a run over the sources, one request at a time, through the trait `Wire`.
- `src/native_plugin.rs` (`NativeCalendar`) is the glue; `native_screens.rs` the rows of the lists
  (agenda, day, week, birthdays, an event, the sources), `native_month.rs` the month grid (the Month
  screen and the Month widget paint the same), `native_asks.rs` the questions (the views, a source's
  menu, the kind, address, user name, password and name of a new source), `native_widgets.rs` the
  widgets, `native_sleep.rs` the sleep screen, `native_text.rs` days and times as words.

## The rules of a sync

1. **Which sources.** A run reads every source with an address. One that starts by itself leaves out a
   source whose name or password was refused until it is changed; *Sync now* and *Read now* do not.
2. **CalDAV and CardDAV.** `PROPFIND` (Depth 1) at the address set; an answer without a calendar of
   events (or address book) may name the account (`current-user-principal`) or its calendars
   (`calendar-home-set`, `addressbook-home-set`): those are followed, three steps at most, so the
   server's own address is enough. Every calendar that holds `VEVENT` (or does not say) is read with
   one `REPORT` for the window (`time-range`, sixty days either side), an address book with one
   `addressbook-query` for contacts that have a `BDAY` or `ANNIVERSARY`. The answer is saved to
   `sync.xml` and read as it streams: never in memory. Up to eight calendars an account.
3. **A subscription** (`.ics`, `webcal://` is `https://`) is one `GET` saved to `feed.ics` and read as it
   streams; **a calendar file** of the card is read in place.
4. **What a source gave before is replaced only once its answer is there**: a source that fails (no
   such address, a refused name, an answer that is no calendar) keeps its events, and says so on the
   sources' screen. No network ends the run and touches nothing.
5. **Events outside the window are not kept** (a subscription or a file holds years): an event that does
   not repeat must fall within sixty days of today (also where it begins before and goes on into it); one
   that repeats must have a day in it. At most 240 events; the rest are counted and said.
6. A time with a `TZID` of the table in `tz` is moved to the reader's clock when it is read (the offset of
   the reader's zone is found once for the window, with one change of daylight saving at most); one with
   a zone it does not know, or none, is the reader's own. The repeats of an event keep the wall-clock
   time it was read at: for a reader in another zone than the event's, they move by an hour with the
   other zone's change of daylight saving.
7. A changed time of an event that repeats (`RECURRENCE-ID`) leaves the day it stands in for out of the
   repeats and is an event of its own; a cancelled one only leaves the day out.

## Limits, so that memory is bounded

- 6 sources, 240 events in all; a title 56 bytes, a place 36, a name 32, an address 240, a user name 64.
  An event is one entry with a title, a place, its rule and at most 16 left-out days; nothing of a
  repeat is kept, they are expanded for what is shown (at most 400 at a time).
- An answer is at most 2 MiB (a file), read 2 KiB at a time; a content line 1,024 bytes.
- Rules it cannot read (`BYYEARDAY`, `BYWEEKNO`, a part that changes what the rule means) show their event
  once; so do `RDATE`s (not read).

## How it stays snappy and light on power

- A key press never waits for the network or reads the card: the events are in memory, nothing is
  written on a key press, and reading the sources is work. A source is read when the calendar is opened
  (at most every five minutes), on *Sync now*, and as *Sync* says: `When opened` (only then), `Also when
  the network is on anyway` (the default: an optional piece of work an hour on that runs when the radio
  is up for something else or the device wakes) or `Also when charging begins`.
- The widgets draw from memory; the host is told a widget changed when the events did, and at the next
  start or end of an event today or just after midnight (one work timer, only while a widget is placed).
- The sleep screen is the agenda widget given the whole screen, drawn in grays at once.

## Gaps, on purpose

1. The calendars are read, not changed: no events are made, edited or removed here, no reminders, no
   tasks (`VTODO`; Tasks has them). Writing back is left to a later version.
2. No year overview; the Month view shows a dot under days with events.
3. A birthday calendar a server makes of its own is read as any calendar; birthdays from contacts need a
   CardDAV address book (`BDAY`, `ANNIVERSARY`, `X-ANNIVERSARY`; a year of 1604 or
   `X-APPLE-OMIT-YEAR` is no year).
4. The password is sent as HTTP Basic (over `https://` unless the address says `http://`); no OAuth.
5. The switch-off screen has no calendar: the interface offers no place for it (#152); the sleep screen
   has.
6. The widgets show the time until an event as of the last time they were drawn.

## Tests

- `crates/paperpaw-caldav`: recurrence rules (daily, weekly, monthly by day and position, yearly, counts,
  until, the last weekday, skipping ahead changes nothing), time zones (Europe, America, the south, a name
  by its end), events (zones, exceptions, changed times, durations), contacts and every form of a birthday,
  and the request bodies.
- `src/model.rs`: expansion (repeats, exceptions, days an event goes on into, birthdays and 29 February),
  changed times, the window, the file's text.
- `src/keep.rs`: a power cut at every storage call of a save leaves the old file or the new one whole.
- `src/sync_tests.rs`: the engine against a CalDAV/CardDAV server in memory (the calendars found from the
  server's address, zones, repeats, exceptions, an all-day event of several days, an event cancelled and
  one outside the window), a subscription, a file of the card, contacts, a source that fails keeps its
  events, no network, a refused name, more events than the reader holds.
- `tests/native_declarations.rs`: the host's rules accept the declarations, every text is in both
  languages; `tests/native_flow.rs` (the scripted host), `tests/native_linked_entry.rs`;
  `tools/pps/calendar.pps` runs the screens, the views and the widgets on the emulator (it needs
  `tools/pps/fixtures/calendar-work.ics` on the card).
