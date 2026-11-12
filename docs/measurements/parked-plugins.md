# Parked native plugins (#248, #2638): design

Status: design for Senior review, 2026-11-12. Kernel (parking, events, numbers) and Core (host
context, owner id, hooks). No interface change: plugins cannot tell a parked plugin from a running
one.

## Why

Idle home on the X4 profile (Services, #2633): 119.7 KiB live against Beta 3's 92.3. The gap is
per-plugin native state of the 32 linked plugins, and 26 of them are never called at idle. Per
plugin: instance, dispatch and leases about 405 B, host context (`PinnedOwner<Context>`,
`Construction`) about 380 B, the plugin's own state from `make` about 185 B. Parking those 26 saves
about 25 KiB in the emulator (X4 about 0.6 to 0.8 of it).

## What parking is

A **parked** plugin is admitted, registered and published exactly as today, so its menus, settings,
slots, declarations and owner-directory subscriptions stay. Only its runtime half is gone: the
`Retained` host (owner, context, `make` state, registration batch lease), the `NativeDispatch` and
the lease. Its `native_instances` row stays with the same key; `instance` is `None`.

Parked is not the ABI's "not started" (`core-events.md` 3.3, `stages-services.md`): for the plugin
and every other plugin a parked plugin is running. It gets every event it subscribed to, its screens
open, its stages and services answer. No `ERR_CANCELLED` or missed event because of it.

The row keeps an `Unpark` recipe (about 40 B, no copy of anything in the image): the linked entry,
its image, the admitted metadata view, the shell's services handle and report, and the hash of the
registration batch `make` submitted at start.

## When

- **Park:** once, right after `Kernel::start` admitted all linked plugins (`native_hosted`). Phase 1
  never parks a plugin again after it ran; dropping idle plugins again is phase 2 (after beta.1).
- **Not parked:** a plugin that can be called inside another plugin's callback, because unparking
  there would cost X4 stack on the nested route (no margin left, #2619): stage subscribers
  (`reader.filter`, `word-looked-up`, `core.widget`, `ui.compose`), `core.document` formats and
  service providers (`core.lookup`). The admission knows these from the registrations. Formats can
  follow in phase 1b if `doc_service` turns `ERR_BUSY` from `format_call` into "preparing" (Host). A
  plugin whose `make` does anything except submit registrations also stays (audit below).
- **Unpark:** between callbacks only (callback depth 0, as `NativeInstances::install` demands),
  synchronously where the kernel is at depth 0 (a command applied in `run_commands`, an action, a
  screen opened, an event emitted between callbacks). A request at depth > 0 uses the existing
  `request_start`/`take_start` path and answers `ERR_BUSY`: "call again on the next pass", as
  `firmware_tell` does already; the other pumps that reach a parked subscriber are made to keep the
  event the same way (Kernel). The event is not dropped.

## How (unpark)

1. Core: `Hosted::new` for the same owner id (the row's key): the owner id is not drawn again, so
   everything keyed by it (owner directory, settings scope, stores, files linked set) stays valid.
2. `make` runs again in a registration transaction; the batch must hash equal to the parked one,
   else the plugin is stopped with a log line (never a crash). The declarations are not staged or
   published again: they are still published.
3. `NativeDispatch::with_events` + lease, `NativeInstances::install(key, lease)`.

A failed unpark (no memory, refused batch) is that call's `ERR_NO_MEMORY` or the event's
`event_dropped`, logged; the plugin stays parked and is tried again on its next use. A trap in
`make` after start is a plugin trap as in any callback: the early-start/safe-mode counter is
untouched (it counts boots that did not reach the end of `start`, and parking and unparking come
after it).

## Audit of `make` (the 26 cold plugins)

All 26 only build `Self::default()` (or a unit) and submit fixed batches from their image
(`registrations::submit`). aids, news and notes-vault skip a batch the host answers `Unsupported`,
which comes out the same every time. None reads storage, settings or the clock, so `make` running
again gives the same plugin and the same batch. (fonts-browser, not cold, clears a folder in `make`:
it stays unparked.) Expected parkable after the nested-call rule (the admission decides from the
registrations): about, bluetooth, file-manager, files, keyboard, lock, mail, menu, opds, power,
reader, settings, setup, transfer, update, plus the 6 formats in 1b. Not parked: aids, vocabulary,
widgets, news, notes-vault (stage/format/widget roles).

## Measurement and gates

Same method as Beta 3 and #2633 (debug emulator, X4 profile, `--heap-budget none --heap-report`,
first `heap .*peak` line): idle, TXT, Alice EPUB and Verwandlung EPUB peaks before and after, plus
start time (`--script wait:3000` timestamps) and the X4 frames (`ppn5-frames`, 2,047 B). Expected:
idle about -14 KiB in phase 1 (15 plugins), about -20 KiB with the formats. Tests: a parked plugin
gets its event, opens its screen, runs its action; a changed batch at unpark stops it; no memory at
unpark keeps it parked and retries; depth > 0 answers `ERR_BUSY` and the event arrives next pass.

## Split

- Kernel: the `Unpark` row, park after start, unpark at depth 0, batch hash, event delivery to
  parked subscribers, the nested-call rule, tests, numbers.
- Core: `Hosted::new` for an existing owner id; host context made on unpark and dropped on park;
  hooks in screen open, slot open and action run that call `Kernel::unpark(key)` before the lease is
  needed.
