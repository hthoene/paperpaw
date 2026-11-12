# Parked native plugins (#248, #2638): design

Status: rev 2 (Senior's M1-M5, #248), 2026-11-12. Kernel (lease accessor, parking, pumps, numbers)
and Core (lease slot, re-hosting, host context, `make` record, declaration store). No interface
change: plugins cannot tell a parked plugin from a running one.

## Why

Idle home on the X4 profile (Services, #2633): 119.7 KiB live against Beta 3's 92.3. The gap is
per-plugin native state of the 32 linked plugins, and 26 of them are never called at idle. Per
plugin: instance, dispatch and leases about 405 B, host context (`PinnedOwner<Context>`,
`Construction`) about 380 B, the plugin's own state from `make` about 185 B. Parking those 26 saves
about 25 KiB in the emulator (X4 about 0.6 to 0.8 of it).

## What parking is

A **parked** plugin is admitted, registered and published exactly as today, so its menus, settings,
slots, declarations and owner-directory subscriptions stay. Only its runtime half is gone: the
`Retained` host (owner, context, `make` state, registration batch lease) and the `NativeDispatch`.

Parked is not the ABI's "not started" (`core-events.md` 3.3, `stages-services.md`): for the plugin
and every other plugin a parked plugin is running. It gets every event it subscribed to, its screens
open, its actions run. No `ERR_CANCELLED` and no missed event because of it.

- **The lease stays; its inside is a slot.** Screen factories, actions, the declaration store and
  the pumps keep their `NativeLease` clones. Parking empties the slot inside the shared lease and
  unparking fills it again, so every holder stays valid. Empty is `Parked`, a state of its own and
  never `Stopped`.
- **The row keeps an `Unpark` recipe** (about 40 B, nothing copied from the image): the linked
  entry, its image, the admitted metadata view, the shell's services handle and report, and the
  hash of the registration batch `make` submitted at start. The owner id (the row's key) stays.
- **Declarations come from the row (M3).** The store's current/stale check, the generation and
  `with_image` read the instance row (enabled, not retiring; the linked image is `'static`), not the
  instance. Park and unpark leave the generation unchanged, so menus, settings, keys and hints stay.

## Which plugins are parked (M2, M3): checked in code

Right after its own `make`, inside its admission (`Kernel::register_linked_parked`, before any
callback or pump pass; #1337), a linked plugin is parked only if all of these hold, checked by the
host, not taken from an audit (another plugin's later `make` only registers and cannot reach it):

1. **`make` only registered.** While the start `make` runs, the host records every host call other
   than `registrations.submit` and the heap (Core, in the registration scope). One such call keeps
   the plugin running for good.
2. **No host resource.** The owner holds no handle, open operation, file, socket, secret, timer or
   work, and has no event queued (Core: `OwnerResources` and the pumps report empty).
3. **Image-only declarations.** Every declaration the store keeps for it is an offset into the
   image or an owned copy; none borrows the registration batch buffer, which is freed at park.
4. **No nested role.** None of its registrations can be called inside another plugin's callback:
   no stage subscription (`reader.filter`, `word-looked-up`, `core.widget`, `ui.compose`), no
   `core.document` format, no published service or lookup provider, no widget maker, and not an
   installer (`uses.install`, plugin requests). Formats can follow in phase 1b once `doc_service`
   turns `ERR_BUSY` from `format_call` into "preparing" (Host).

Phase 1 never parks a plugin again after it ran; dropping idle plugins again is phase 2 (after
beta.1). The `make` audit below says what to expect, but the checks decide.

## Unparking (M1, M5): one accessor

Every route that wants a plugin's runtime handle goes through one lease accessor
(`NativeLease::live`, used by actions, screens, dialogs and confirms, firmware answers,
hand-overs, timers, work and every event pump). It answers `Parked` itself:

- **At a safe point** (callback depth 0, no event batch being delivered, no registration transaction
  open) it unparks synchronously and goes on: a command applied in `run_commands`, an action run,
  a screen or slot opened (also another plugin's staged open and the dialog pump). The apply of
  another plugin's staged batch (`native_outbox_run`) at depth 0 is a safe point too: the batch is
  moved out of the outbox first, and a remake only builds the dispatch and checks the batch, it
  touches neither the registry nor the stack (Senior on #1337). A screen is made as it opens; one
  that cannot be made then (its wait after no memory) fails the open as without memory.
- **Anywhere else** it marks `request_start` and answers `ERR_BUSY`. An event pump keeps the event
  and calls again after its pass ends (the pending starts are taken at the pass boundary and at the
  top of `run_commands`, both safe points). This is how `firmware_tell` works today.
- **Fail safe:** a caller that sees `Parked` and does not handle it never treats it as stopped: it
  answers `ERR_BUSY` and keeps what it had. Drawing never unparks and never allocates: a parked
  plugin has no screen on the stack (opening one unparks it) and no widget instance (condition 4);
  a draw that meets one anyway draws nothing, logs and marks `request_start`.

**No route reaches a parked plugin inside another callback.** A callback's own calls into other
plugins are stage runs, `core.document`, services and lookups, widgets, compose and plugin requests:
condition 4 keeps every target of those running. Its screen opens, actions and dialogs are staged
and run after it returns, at a safe point. So `ERR_BUSY` at depth > 0 is a fail-safe only: no
plugin can spin on it inside one callback (a debug assertion and a log line flag it in tests).

Unparking: Core re-hosts with `Hosted::new` for the same owner id (nothing keyed by it changes),
`make` runs again in a registration transaction, the batch must hash equal to the parked one, then
`NativeDispatch::with_events` fills the lease slot. Nothing is staged or published again.

## Failures (M4)

- **No memory:** the plugin stays parked. An event of a family that may not be dropped stays
  queued in its pump (bounded as the pump's queue is) and is retried at the next safe point no
  sooner than 1 s later, doubling to 60 s; a retry never wakes the device. A family that allows
  drops reports `event_dropped` as its contract says. A command or action answers `ERR_NO_MEMORY`.
- **Batch mismatch:** the plugin is stopped through the normal withdraw path (declarations, slots,
  subscriptions go; the second instance is destroyed), with a log line. A test covers it.
- **Trap in `make` after start:** a plugin trap as in any callback. The early-start/safe-mode
  counter is untouched: it counts boots that did not reach the end of `start`, and parking and
  unparking come after it.

## Audit of `make` (the 26 cold plugins)

All 26 only build `Self::default()` (or a unit) and submit fixed batches from their image
(`registrations::submit`). aids, news and notes-vault skip a batch the host answers `Unsupported`,
which comes out the same every time. None reads storage, settings or the clock. (fonts-browser, not
cold, clears a folder in `make`: condition 1 keeps it running.) Expected parked: about, bluetooth,
file-manager, files, keyboard, lock, mail, menu, opds, power, reader, settings, setup, transfer,
update, plus the 6 formats in 1b. Expected running: aids, vocabulary, widgets, news, notes-vault.

## Measurement and gates

Same method as Beta 3 and #2633 (debug emulator, X4 profile, `--heap-budget none --heap-report`,
first `heap .*peak` line), before and after:

- steady idle, TXT, Alice EPUB and Verwandlung EPUB peaks; the first menu open at the Alice peak;
- the heap owner rows released at park (`owners.py`), and start time;
- the X4 unpark stack path from the main loop (`gate.py`, 2 KB margin) and `ppn5-frames` (2,047 B);
- first-open latency of a parked plugin's screen on the X4.

Expected: idle about -14 KiB in phase 1 (15 plugins), about -20 KiB with the formats. Tests: a
parked plugin gets its event, opens its screen, runs its action; each condition of 1-4 keeps a
plugin running; a changed batch withdraws it; no memory keeps it parked with the event queued and
the back-off; a request outside a safe point answers `ERR_BUSY` and the event arrives next pass.

## Split

- Kernel: the lease accessor and its `Parked` answer, the safe points, park after start and the
  condition checks, the `Unpark` recipe and batch hash, the pumps keeping events and the back-off,
  tests, numbers.
- Core: the slot inside `NativeLease`; `Hosted::new` for an existing owner id; host context made on
  unpark and dropped on park; the record of host calls during `make`; the empty-resources check;
  the declaration store's generation and image from the row. With the single accessor there are no
  per-route hooks.
