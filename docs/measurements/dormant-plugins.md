# Dormant native plugins (#248, item 5): design

Status: draft for Senior, 2026-11-12; the work comes after beta.1 (Decision 164). Kernel.
Builds on parking (`parked-plugins.md`, #1337): same lease slot, one accessor
(`NativeLease::live`), safe points, `ERR_BUSY` routes, back-off, `give_up`. No interface change:
a plugin still cannot tell dormant from running. Each step below is its own draft pull request.

## Why

Owner Decision 162: device-faithful measuring, target Beta 3. Firmware's X4-faithful start: live
94.3 KiB against Beta 3's 58.8 (+35.5); TXT 103.2 against 60.2, EPUB 102.7 against 68.1.

Measured on the parked build (park diagnosis, 32 linked plugins, 9 parked, 64-bit emulator,
X4 profile, idle home 110.9 KiB live), bytes per plugin including the allocator's word:

| Row | Every plugin | Running only | Parked only |
|---|---|---|---|
| instance row (`NativeInstances` entry) | 72 | | |
| owner directory row + its id | 80 | | |
| linked set entry + its id | 41 | | |
| lease slot (`Shared<Slot>`) | 48 | | |
| declaration store owner + batch starts | 184 | | |
| host context (`PinnedOwner<Context>`) | | 352 | |
| dispatch (`NativeDispatch`) | | 138 | |
| plugin heap (`make` state, charged) | | ~130 | |
| `Recipe` + `Parked` | | | 376 |
| **Sum** | **~425** | **~620** | **376** |

So a running plugin costs ~1.05 KiB and a parked one ~0.8 KiB. Parking a plugin saves only
~0.25 KiB, because the recipe is almost as large as what it replaces; and 425 B of every
plugin's cost is untouched by parking.

## What changes

**D1. The recipe becomes a static descriptor.** What `Recipe<S>` copies (entry, metadata, image,
id, `services: fn() -> S`, report, teardown, fixed tables) is `'static` or a function pointer
from the shell's linked list: the shell passes one `&'static LinkedPlugin` instead. What all
plugins share (callback depth, flat host, resource pool view, host profile) is held once by the
kernel. Per parked plugin only the batch hash, the event tags and the retry state stay (`Parked`,
~64 B). Saves ~310 B per parked plugin.

**D2. Formats may park.** Condition 4 of `parked-plugins.md` drops `FormatSpec`. Every route to
a format already keeps its request on `ERR_BUSY`: `format_call` requests the start and answers
`ERR_BUSY`; `doc_prepare` turns it into "later" (the document stays preparing); the details and
cover jobs count it as "not the book's fault" and retry. The first open of a book in that format
unparks it at the next safe point (prepare runs at depth 0). Saves ~0.55 KiB per format.

**D3. Dormant again when idle (re-park).** A plugin that was parked at admission is parked again
at a safe point when all hold:

1. its slot allows it: registered parked, running, no call holds the instance, no callback runs
   (condition 1 is re-checked on the `make` of its unpark);
2. it holds no blob handle, and nothing any host service would end with it
   (`HostServices::owner_holds`: documents it opened, pending opens too, and documents its
   formats read, a book of its folders, library transactions, image ids and prepares, UI
   requests, entries, widgets, status handles and taps, network requests and servers, sockets
   and casts, declared work and runs). Its font and service handles do not count: they live in
   the instance's memory, are released when it is dropped and opened again by the remade plugin
   (Senior on #1365); its library receipts are kept through the re-park, and so are a format
   plugin's formats in the format index (the next open of one of its books finds them);
3. no screen of it on the stack, no action or setting event queued for it, no hand-over pending
   and no staged batch waiting;
4. no open or pending document of its formats (a format re-parked under an open book would turn
   page reads inside the reader's callbacks into `ERR_BUSY`);
5. the device goes to standby (every idle plugin), or an unpark found no memory (one plugin at
   the next safe point). Decision 166: no idle timer.

**Soundness:** `Slot::live` hands out `&dyn NativeInstance` from a `OnceCell`, which can never be
emptied. D3 changes the slot to `RefCell<Option<Box<dyn NativeInstance>>>` and `live` returns a
`Ref` (`Ref::filter_map`). Shared borrows nest, so re-entry into the same plugin still works.
Re-parking takes `try_borrow_mut`: while any caller holds the instance it fails and the plugin
stays running. No `unsafe`. Cost: one borrow flag per plugin (8 B), and the 35 `live()` call sites
keep a `Ref` temporary instead of a reference (mechanical; sites that return it are
changed to take a closure).

Re-parking runs the plugin's `destroy` like parking at admission does, then fills the slot with
the descriptor's `Parked`. If that 64 B cannot be had, the plugin stays running.

**D4. One row per plugin.** The ~425 B every plugin pays is the same plugin kept in four tables,
with its id copied three times. `NativeInstances`' entry takes over the owner directory row (key,
tags), the linked set entry and the store's owner header (id, generation, incarnation, batch
starts); linked ids stay `&'static str`. Owner directory, linked set and declaration store
become views on that row. Target ~200 B per plugin, for all 32. No `unsafe`; Core co-owns the
owner directory and the store.

**D5 (item 4). Packed batch starts.** An image-only owner kept 12 B per batch (start and kind bits,
`declaration_scan.rs`). A plugin's batches are separate statics, so the image cannot be walked from
one batch to the next; but they lie close together. Built: an owner whose batches lie within 64 KiB
of the first keeps one start, one mask of the kinds it declares and a 2-byte offset per batch: 14 B
for the owner and 2 B per batch, in one allocation (`declaration_starts.rs`). An owner with one
batch, or with batches further apart, keeps a start and a mask per batch as before. Cost: a lookup
of a kind walks the frame headers of every batch of an owner that declares it, where the per-batch
mask skipped the batches without it (instruction gate: at most +2 %). 40 owners, 269 batches on the
emulator: about -2.2 KiB.

**D6 (parked lever, not built).** A running plugin's host context (280 B on the 64-bit emulator)
holds four scopes only a call uses (registrations 16, commands 16, environment 56, canvas 32 B).
One set per call depth instead of one per plugin saves ~112 B per running plugin: ~2.5 KiB on the
emulator, ~1.8 KiB on the X4 with ~23 plugins running. Cost: ~240 call sites, pinned scopes with
raw pointers, a per-call resource view, an owner check per depth set and changed re-entry rules
(`unsafe`, medium risk). Dropped from PR 1 (Manager, 2026-11-12); use it only if the end gap to
Beta 3 needs it.

## Which plugins (of 32)

- Dormant: the 9 parked at admission today (about, file-manager, files, help, keyboard, menu,
  opds, settings, transfer) and the 10 formats.
- Never dormant: start subscribers (reader, lock, setup, power, wifi, bluetooth, update,
  plugin-manager, vocabulary), widgets and status (home, news, widgets), stages (aids), and card
  plugins (their image is not `'static`; they keep today's path).
- Later (own decision): rare-event subscribers woken by a kept event tag (update,
  plugin-manager, vocabulary, setup, bluetooth), ~0.55 KiB each.

## Numbers (estimate, 64-bit emulator; X4-faithful about 0.7 of it, Firmware measures)

| Step | Saves (emulator) |
|---|---|
| D1 descriptor, 9 parked | -2.8 KiB |
| D2 formats, 10 | -5.5 KiB |
| D3 re-park | 0 at start; idle after use back to the start value |
| D4 one row, 32 | -7.0 KiB |
| D5 batch starts | -1.5 to -2.8 KiB |
| **Total** | **-17 to -18 KiB (X4-faithful about -12 to -13)** |
| later: rare-event subscribers, 5 | -2.8 KiB |

Against +35.5 KiB that is about a third; with #1337 (-3.1 emulator) and the later step, about
40 %. The rest is outside the per-plugin rows (host services, format scope, kernel tables).

## Failures

- No memory at unpark: as parking (stays parked, event kept, back-off 1 s to 60 s, give up after
  six tries with the no-memory notice, #555).
- No memory at re-park (the `Parked` row): stays running.
- Batch hash differs at an unpark after a re-park: withdraw, as parking.
- Re-park churn: only standby and memory pressure re-park (no idle timer), one plugin per
  pressure; 50 standby cycles leave live heap and handles unchanged (`tests/sleep_cycles.rs`).
- A pending open whose format gave up being made (no memory after its longest wait) fails the
  document with `ERR_NO_MEMORY`; it does not stay preparing (Senior, R2 on #1365).
- An unpark that meets something busy (the old owner not retired yet) waits like no memory
  but is no memory pressure (Senior, R3 on #1365).

## Tests and gates

Per step, before/after with separate binaries: EN/DE start (300 ms and 3 s), idle home, TXT, Alice
and Verwandlung peaks, first menu open at the Alice peak, idle after opening menu, settings, files
and one book and waiting 60 s, first-open latency of a dormant screen and of a dormant format's
book. Tests: each re-park condition keeps the plugin running; a held `Ref` blocks re-park; a
re-parked plugin gets its event, opens its screen, runs its action; a format opens a book from
parked, from re-parked, and is never re-parked under an open book. X4: `gate.py` on the unpark
paths of the open, prepare and pump routes, built from the activation branch (Firmware's start
cuts), not from integration; `ppn5-frames` 2,047 B.

## Steps

| # | Pull request | Days | Review |
|---|---|---|---|
| 0 | this note | 0.5 | Senior |
| 1 | D2 formats park | 1 | Senior (rule change), Host (routes) |
| 2 | D1 static descriptor | 0.5-1 | CR, Firmware (shell list) |
| 3 | D4 one row per plugin | 2-3 | Senior, Core |
| 4 | D3 re-park | 1.5 | Senior |
| 5 | D5 batch starts | 1 | Senior |
| 6 | numbers, X4 stack gate | 0.5 | Firmware |

Total 7-8.5 days.

## Decisions

Settled: Beta 3 parity is due before 1.0, not beta.1 (Decision 164); item 5 resumes after
beta.1.

Owner: (1) whether D3 is part of the 1.0 parity; (2) docs-only contract text:
"an idle plugin may be stopped and made again; keep what must survive in storage" (plugins lose
cursor or scroll kept in memory); (3) a first-open latency budget on the X4; (4) rare-event
subscribers before or after 1.0.

Senior: (a) formats park (D2); (b) `RefCell` slot and `Ref` accessor instead of `OnceCell` (D3);
(c) re-park conditions 1-5 and their triggers (standby, memory pressure); (d) one row per
plugin and the store's generation/incarnation in it (D4); (e) the stack gate of the unpark
paths.
