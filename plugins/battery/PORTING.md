# Battery: goldens the interface-7 port has to reproduce

These tests pin what the interface-6 Battery plugin does today (ticket #771,
umbrella #228). The port is accepted when the same goldens pass with the
interface-7 plugin, or when a difference is decided on purpose and the golden
is re-blessed in the same change with a note here. Section numbers refer to
`docs/abi/proposals/interface-7-events-storage-services.md` (the proposal);
everything marked "proposed" there can still change. See also
`tests/PORTING.md` for the older behavioural contracts these sit next to.

Re-bless (look at every changed file before committing it):

```sh
PAPERPAW_BLESS=1 cargo test -p paperpaw-plugin-battery --test screens --test storage_goldens
```

## 1. Screens (`tests/screens.rs`, `testdata/screens/*.png`)

16 pictures of the real screen on the 480x800 mono panel, English and German.
Compared pixel for pixel. The port needs the canvas, text, font and i18n
formatting contracts to draw them again (proposal 4.2, 4.3; slot 9 canvas).

| Golden | State | Needs (proposal) |
| --- | --- | --- |
| `empty`, `empty-known-en/de`, `empty-unknown-en/de` | empty log, battery known / unknown | 4.2 `tr`/`format`; services for battery snapshot (slot 4) |
| `few-en`, `few-de` | a few times awake | 2.2 storage read of the log; 4.2 plural, number, digits |
| `many-page-1`, `many-page-2`, `many-last-page`, `many-page-1-again` | many times awake, paging forward and back | 2.2 `read_at` (bounded history); 4.4 commands, input via screen callbacks (1.3 D1.3) |
| `charging`, `charging-de`, `charging-no-voltage` | charging, with and without voltage | slot 4 snapshot (charging, optional millivolts) |
| `low` | low battery | slot 4 snapshot |
| `developer-footer` | developer setting shows where the log is | settings read (slot 5); storage path of plugin root (2.2) |

## 2. Storage format (`tests/storage_goldens.rs`, `testdata/*.v1`)

| Golden / test | What it pins | Needs (proposal) |
| --- | --- | --- |
| `log-sequence.csv.v1` | exact bytes of `log.csv`: header, `start`, `sample`, `end` lines, a wall-clock column that is empty when unknown | 2.2 `storage.append`, `storage.size`; 1.2 events; 3 clock |
| `plugins-sequence-shape.csv.v1` | shape of `plugins.csv` (per-plugin usage rows; numbers masked as `…`) | 3 `services.query` usage; 2.2 append |
| rotation tests | at 128 KiB (`MAX_BYTES`) the log becomes `log.old.csv` (replacing the old one) and a new file with a header starts; 128 KiB - 1 does not rotate; same for `plugins.csv` | 2.2 `storage.rename`, `storage.remove` |
| `nothing_is_written...` | `battery.enabled = off` or an unreadable battery: no files at all | slot 5 settings; 1.2 |
| `a_tail_cut...` | the view survives a power-cut tail at every byte of the last hour | 2.2 `read_at`, bounded parse |
| `log-after-cut-tail.interface6.csv.v1` | **interface 6 only** (the bug below): a new line is joined to a cut tail | deleted with the interface-6 plugin |
| `log-after-cut-tail.interface7.csv.v1` | **what the port must write** (the one intended deviation): the partial line is terminated, the new `start` is its own record. Test is `#[ignore]`d until the port exists | 2.2 `storage.read_at` of the last byte, then `append` |
| settings test | two Bool specs `battery.enabled`, `battery.by-plugin`, default on, groups `power.group` / developer, titles and help keys | declarative registration (`declarative-registration.md`), settings value codec |

## 3. Events (`tests/events_table.rs`)

A table of 12 rows plus one extra test. Each row is a start-up state and a
sequence of host events; the result is the `kind` and `pages` columns of the log.

| Today | Port needs |
| --- | --- |
| start-up writes `start`; start after sleep is told twice and written once | lifecycle events (proposal 1.2 D1.2) |
| `battery_measured` writes `sample`; without a battery no event is sent, so only `start` (extra test) | 1.2 D1.1 (`event.battery_measured` is kept) |
| suspend / stopping write `end`; resume writes the next `start`; suspend twice writes two `end` (odd) | 1.2 D1.2, `storage.sync` (2.2) before power-off |
| pages turned are counted until the next line, across sleep | page-turn event (1.2 D1.2) |
| `enabled = off`: no log; switched off while awake and on again: the awake state is kept; off at start then on: first measurement writes `start` + `sample` (odd) | settings change events and slot 5 |

Input (buttons, touch, OPEN) is not an event (1.3 D1.3): it is covered by the
screen goldens and `tests/port_view_contract.rs`.

## 4. Settings

`battery.enabled` (default on): turns the whole log (`log.csv` and
`plugins.csv`) on or off at runtime; its effect is the events table above.
`battery.by-plugin` (default on, shown with the developer settings): together
with the core developer setting it shows the "by screen" and "by processor"
sections of the view (per-plugin time shown, time busy). It does not change
what is written to `plugins.csv`. The two specs, defaults, groups and keys are
pinned by the settings test. **Not pinned:** the effect of `battery.by-plugin`
on the view, because the sections need a plugin that was shown for 60 s or
busy for 100 ms, which the host fixtures cannot produce deterministically. The
port must check it by hand or with a Kernel-provided usage fixture (proposal
3, `services.query` usage).

## The one intended deviation

**A cut tail swallows the next `start` (bug in interface 6).** The log is
appended as it is. If a power cut left a last line without its newline, the
first line of the next run is written onto the end of it:
`sample,60000,40start,0,,,...`. Two records become one unreadable line; the
`start` that opens the new run is lost. The view survives it (every cut
position is tested).

Manager decision (#771): the interface-6 plugin is not patched. The
interface-7 port fixes it: before appending, if the log does not end in `\n`
(one `read_at` of the last byte, proposal 2.2), append `\n` first so the next
`start` is its own record. The partial line stays as a short unreadable record
(no rewrite, no delete). `log-after-cut-tail.interface7.csv.v1` pins this; run
it with `cargo test -p paperpaw-plugin-battery --test storage_goldens -- --ignored`.
Every other golden must pass unchanged. The same check applies to
`plugins.csv`, which is appended the same way.

## Odd, but pinned as it is

Suspend twice writes two `end` lines, and a plugin switched on after start-up
opens its awake state late. Both are `odd` rows in the events table; the port
keeps them.

## Not pinned

`battery.by-plugin`'s effect on the view (see section 4): the port needs a
deterministic clock/busy fixture to test it.
