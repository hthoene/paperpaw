# Beta 3 memory parity: plan (#248, Decision 169)

Status: design note, 2026-11-13, Kernel. No code. Owner Decision 169: the flip reaches the Beta 3
start level before beta.1, even with a rework of weeks. This note says where the bytes are, which
changes close the gap, what each costs and who builds it. Builds on `parked-plugins.md` (parking
at admission, built) and `dormant-plugins.md` (D1-D6; D2 formats and D5 batch starts are built).

## Basis

Reference set (`~/shared/heap-reference-set.md`), debug emulator, X4 profile, start with
`--set reader.open-last=false --script wait:300`, `--heap-report` with 10 frames
(`PP_HEAP_DEPTH`), live heap by call site. Three binaries, verified distinct:

| Side | Tree | Live / peak KiB |
|---|---|---|
| Flip | integration `cd5266a91` + flip + static manifests (Firmware's patch) | 75.4 / 94.9 |
| Interface 6 on integration (no flip) | `cd5266a91` | 51.1 / 69.2 |
| Beta 3 | `2affa8d59` (v0.6.0-beta.3) | 59.7 / 79.8 |

The heap report's table sums to 75.4 / 50.0 / 58.5 (its rows are rounded); the memory test
gives 75.5 against 57.7. **Gap: about 17.8 KiB on the emulator** (X4 about 0.7 of it).

The interface-6 kernel on integration is already about 8.5 KiB under Beta 3, so the gap is all
native-host state. Flip minus interface 6, by area (KiB):

| Area | Flip | No flip | Difference |
|---|---|---|---|
| Instance and owner-directory rows, leases and lease slots | 5.3 | 0 | +5.3 |
| Running plugins' host context (`PinnedOwner<Context>`, `Construction`) | 4.5 | 0 | +4.5 |
| Running plugins' own state (`plugin_heap::Charged`) | 3.3 | 0 | +3.3 |
| Running plugins' dispatch (`NativeLease` with `NativeDispatch`) | 1.8 | 0 | +1.8 |
| Parked plugins' `Recipe` | 1.9 | 0 | +1.9 |
| Declaration store (owner table, batch starts) | 2.2 | 0 | +2.2 |
| Host services (documents, formats, resources, handle tables) | 6.9 | 1.6 | +5.3 |
| Host wiring per service (`shell_wiring::wire_*`, moves, secrets) | 2.6 | 0 | +2.6 |
| Publication (native action rows, setting reservations) | 3.1 | 0 | +3.1 |
| Emulator glue for linked plugins (`linked.rs`: sealer, lists) | 2.1 | 0 | +2.1 |
| Kernel tables, interface-6 plugin UI, plugin list | 29.5 | 36.2 | -6.7 |
| Framebuffer and rest | 12.2 | 12.2 | 0 |

Per plugin (64-bit emulator, measured, with the allocator's word), 26 linked plugins hosted, 12
running at start and 14 parked:

| Row | Every plugin | Running only | Parked only |
|---|---|---|---|
| instance table entry | 63 | | |
| owner directory row | 39 | | |
| declaration store owner + batch starts | 90 | | |
| host context (`PinnedOwner<Context>`) + `Construction` | | 384 | |
| plugin state from `make` (`Charged`, average) | | 247 | |
| dispatch lease (`NativeLease` + `NativeDispatch`) | | 93 | |
| `Recipe` + `Slot` + two `NativeLease`s (parked) | | | 306 |
| **Sum** | **~190** | **~725** | **~306** |

Running at start (12): home, help, reader, files, lock, power, setup, wifi, bluetooth, update,
plugin-manager, transfer. Parked (14): those of `SLEEPS` in the set (about, file-manager,
keyboard, menu, opds, settings, the formats); files, though in `SLEEPS`, is running at
`wait:300` (a call at start woke it).

## Changes

KiB are 64-bit emulator figures from the rows above; the X4 is about 0.7 of them (Firmware
measures each step on the activation branch). Days are for one engineer, including tests and
the before/after measurement.

### P1. Lazy host services (Host)

Make each host service at its first call, not at the start, except what the home screen uses at
start (library, ui). Today made at start and unused there: the document side (`wire_documents`
1.0, `DocState` 0.5, format places 1.1), host resources 0.8, and the wiring of net 0.4,
sockets 0.3, files 0.1, secrets 0.1, library moves 0.2 and seven small tables 1.0.
**About -4.0 KiB, 2 days, risk low to medium** (the first call of a service may now fail with
no memory where start would have failed: the call answers `ERR_NO_MEMORY` as its contract says).
No interface change. Host did `core.work` this way in #1376; same pattern.

### P2. Right-sized service handle table (Core)

One plugin's `ui.open` at start makes a handle table of 1.1 KiB (`abi7/handles.rs:248`,
`OwnerResources::insert_service`). Grow it as handles are opened (fallibly, as the resource
tables do). **About -1.0 KiB, 0.5 day, risk low.** No interface change.

### P3. Static glue for linked plugins (Firmware)

The emulator's `linked.rs` builds a sealer (1.0), the offered and feature lists (0.6) and
register closures (0.4) on the heap; all are known when it is built. Make them `static` tables,
and do the same in the X4 firmware's linked glue. **About -1.7 KiB emulator, 1 day, risk low.**
No interface change.

### P4. One row per plugin, handles by index (Kernel lead, Core)

`dormant-plugins.md` D4, carried further. Today a plugin sits in four tables (instance, owner
directory, declaration store owner, linked set) with its id copied, and every holder of a plugin
(screen factories, actions, the store, pumps) keeps a reference-counted `NativeLease` around a
`Slot`. Instead: one `NativeInstances` row per plugin holding the id (`&'static str` for linked
plugins), state bits, generation and incarnation, event tags and the batch starts; the other
tables become views on it. Holders keep a `PluginRef { index: u16, incarnation: u16 }` checked
against the row, not an `Rc`. A stale ref answers `Stopped`, as a dropped lease does now.
Every plugin goes from about 190 B to about 64 B, and the lease and slot allocations go
(counted in P5). **About -3.2 KiB (26 × 126 B), 4 days, risk high** (35 `live()` call sites,
the store's generation, re-entry). No `unsafe`.
No interface change. Senior reviews.

### P5. Parked = a static descriptor (Kernel, Core)

`dormant-plugins.md` D1. With P4, a parked plugin keeps only its row; what the `Recipe` copies
(entry, image, metadata, services, report, fixed tables) is `'static` in the shell's linked list
and is handed over as one `&'static LinkedPlugin`. The batch hash and retry state move into the
row. Parked plugins go from about 306 B to about 24 B. **About -3.9 KiB for the 14 parked today
(14 × 282 B), 1.5 days after P4, risk medium.** No interface change; Firmware's
shell list changes shape (paperpaw-app, not interface 7).

### P6. Park right after start (Kernel, parity tests by the plugin's owner)

Decision 169 changes Decision 166: a linked plugin that is running after the start event and
has nothing open (no screen on the stack, no open document of its formats, no handle, request,
socket, work or queued event) is parked at the first safe point after the start event. Every
event it subscribed to wakes it at the next safe point, as a parked plugin is woken today
(`NativeLease::live`, `ERR_BUSY` and retry inside a callback). Timers: the kernel keeps a parked
plugin's timer and wakes it when it fires (today a timer keeps a plugin running).

Candidates (9): reader, files, lock, power, setup, wifi, bluetooth, update, plugin-manager. Not
parked: home (on the stack), help (its overlay is on the stack at start and it is told of every
screen change), transfer (an installer: nested role). Each needs a case in
`crates/paperpaw-app/tests/sleep_parity.rs` (same flow asleep and awake, screens, replies and
card files) before it joins `SLEEPS`. The figures count 8 of the 9: one (likely wifi or power,
which keep a timer or a connection) may stay running after the checks.
**About -3.4 KiB with today's parked cost (8 plugins × 419 B), -5.5 KiB after P5 (8 × 701 B),
1.5 days for the kernel part plus 0.25 day per parity case (2 days for 8), risk medium**:
first-event latency on the X4 (`make` runs again: Firmware measures, owner sets the budget),
and plugins lose what they kept in memory after the start event (the parity case shows it;
storage is the contract). Interface: no ABI change; the docs say "an idle plugin may be stopped
and made again; keep what must survive in storage" (owner's text decision from
`dormant-plugins.md`).

### P7. Native action rows in the program (Kernel)

54 native actions take 32 B each in the kernel's list (`publish_native` 1.7 KiB) plus
reservations (0.6). Each linked plugin's build already makes a `NATIVE_TITLES` table; add the
key and token there and keep `(plugin, entry)` (8 B) per action. **About -1.3 KiB, 1.5 days,
risk medium. Interface change:** `NativeAction` and the private layout of SDK `Actions`
(`shape.txt`) change, a second interface-7 shape change after Decision 168 (owner's OK).

### Not proposed

- Host context per call depth (`dormant-plugins.md` D6): about -112 B per running plugin; after
  P6 only 4 run, so about -0.45 KiB for 2-3 days with `unsafe`. Reserve only.
- Declaration store owner row (`StoredOwner`): folded into P4.
- Settings specs index (4.2 KiB): Beta 3 holds the same (4.8), not part of the gap.

## Sum and order

| # | Change | KiB (emu) | Days | KiB/day | Who | Parallel with |
|---|---|---|---|---|---|---|
| P1 | lazy host services | -4.0 | 2 | 2.0 | Host | all |
| P2 | handle table | -1.0 | 0.5 | 2.0 | Core | all |
| P3 | static linked glue | -1.7 | 1 | 1.7 | Firmware | all |
| P4 | one row, index handles | -3.2 | 4 | 0.8 | Kernel + Core | P1-P3, P7 |
| P5 | parked descriptor | -3.9 | 1.5 | 2.6 | Kernel + Core | after P4 |
| P6 | park after start | -3.4 (-5.5 after P5) | 3.5 | 1.0-1.6 | Kernel + parity cases | with P4 |
| P7 | action rows in the program | -1.3 | 1.5 | 0.9 | Kernel | P1-P3 |
| | **Total P1-P6** | **-19.3** | **12.5** | | | |
| | **Total with P7** | **-20.6** | **14** | | | |

P4 and P5 are listed apart but are one rework: P5's saving needs P4's rows. P5 and P6 together
save -9.4 (14 × 282 + 8 × 701 B), which the total uses. P1-P6 reach about 1.5 KiB below the
17.8 needed; that margin is thin for estimates, so P7 (interface change) stays ready in case
the measured result after P1-P6 falls short.

Calendar with four people: days 1-2 P1 (Host), P2 (Core), P3 (Firmware) and the P6 kernel part
(Kernel); days 3-7 P4 (Kernel, Core on the store and directory views); days 8-9 P5; P6 parity
cases alongside (whoever owns the plugin, or Kernel). About 2 weeks to the end measurement.

## Gates (every step)

Before/after with separate binaries on the reference set: start EN and DE, TXT, EPUB (Alice),
pictures, store; memory test 17/17; callgrind start Ir (at most +2 %); X4 nested-route stack
(`gate.py`, 2 KB margin) for every new unpark route; `ppn5-frames` 2,047 B. P6 adds the
first-event latency of each newly parked plugin on the X4 (owner's budget).

## Decisions needed

- Owner: P7 as a second interface-7 shape change, if needed; the docs text for P6; an X4
  first-event latency budget for parked plugins.
- Senior: P4 (index handles in place of the lease) and P6 (park after start, kernel-held
  timers).
