# Screen lock on interface 7

Status: **ported, native only.** No interface-6 code is left (`code.rs`, `screen.rs`,
`touch.rs`, the interface-6 `Lock` plugin and its kernel-driven tests are gone). What the host
cannot take yet is listed under *What is pending*; each has an ignored case in
`tests/acceptance_pending.rs`. The crate is linked into the program
(`paperpaw-app`'s `NATIVE_PLUGINS`, feature `linked`) and builds as a native plugin file
(`native-entry`).

## Where things are

| Module | What it is |
| --- | --- |
| `declarations.rs` (+ `build.rs`) | the lock's page of settings (kind, lock after waking up, touch), the kind's editor (the lock's own screen), the screen `lock.screen` (secret, edges held), the action `lock.now`, the events started and resume |
| `native_model` | the rules, host-free: the code kept as `kind:salt:hash` (salted SHA-1 taken 2,000 times), the wrong tries and the 30 s wait after five, the PIN's keypad, the pattern's nine dots, `Code` (redacted `Debug`, wiped on drop) |
| `native_flow` | the steps of the screen (unlock, check the code in use, list the kinds, new code, again) as a list of effects; `native_input` is what an action, a tap or a finger does to them |
| `native_pad` | the layout (clock or header, title, dots, keypad, pattern, password button), 32-bit arithmetic in the order of interface 6, compared with it to the pixel in a test |
| `native_screen` | what the screen draws (prepared operations), `native_clock` the time and the date |
| `native_store` | the host calls: the code among the secrets (`core.secrets`, name `code`, not changeable, salt from `random`), the tries file `tries.txt` in the data folder, the two settings |
| `native_plugin` | `NativeLock`: carries the effects out, routes events and inputs |
| `native_touch` | the touch lock's way out (pure) |
| `native_touch_lock` | the touch lock on the host: the `core.input-filter` subscriber (takes every finger while `lock.touch` is on, answers run `lock.touch-toggle` for the way out and `lock.touch-hint` for the hint, passes keys and gestures), the actions `lock.touch-toggle`, `lock.touch-lock` (*Lock touch*, the entry in the quick panel and the reader's menu: an action, so the panel closes; a switch would leave it open on a screen no longer touchable, #234) and `lock.touch-hint`, and the status item `lock.touch-status` |

## Behaviour as interface 6

- Locks at start and, unless *Lock after waking up* is off, at wake; *Lock now* locks at once.
  The unlock screen is opened with `mode=unlock`, guards the device (`command.screen_policy`) and
  is opened again if something closes it while locked. With no code kept any more it lets the
  device be used.
- The same steps, messages and texts: PIN (at most 16 digits), pattern (nine dots, each once;
  without touch chosen with the buttons and *OK*), password on the keyboard (secret question; the
  whole text is hashed, at most 4,096 bytes taken), new code twice, at least 4, kinds listed with
  the one in use marked, *Off* removes the code. Codes kept by interface 6 open the lock
  (golden hashes in `src/native_parity_tests.rs`, made with an independent implementation).
- Five wrong tries: the next one waits 30 s; the tries are kept on the card (version 1 file read);
  **every input is gated by `Flow::waiting`**, only *Back* works while waiting, and the next input
  after the wait forgets the tries first (`Flow::wait_over`).
- What passes the lock: sleep, refresh the screen, night mode.
- Every callback allocates fallibly (refusal sweeps in `tests/native_alloc_refusal.rs`); a
  code, a hash and the typed password are wiped as soon as they are done with (buffers of the secrets
  and ui calls too); no `Debug` shows a code.

## The touch lock

Wired (#234). Ways out, none of which needs touch except the last: the power button held (the power plugin runs `lock.touch-toggle` at once while touch is locked, with no menu), the home key held (`long.home` while `lock.touch` is true), tapping twice and holding the second tap. The host does not call the filter while the screen lock guards, and a secret top screen is filtered by its own plugin only, so no overlay can take touch from the lock's own screen. Declared everywhere (`make` has no snapshot to tell touch); the entries carry the need for touch. Costs a filter callback per key press on devices without touch (decode and pass).

## Differences

- The delete and *OK* keys are drawn with lines, not icons (there is no icon font draw for plugins);
  the kinds list is the kit's (the kind in use is marked with a check mark in the value column, and
  a first press of *Confirm* chooses at once: the list has no "focus not shown yet" state).
- Where interface 6 drew the buttons with `ui::Button`, the password's and *OK* button are
  outlined rectangles with the style's corners (filled when chosen with the buttons).
- The clock shows the time and today as before; the zone is UTC until the time zone setting is
  read (draft 25).

## What is pending

- **The end of a wait redraws without an input**: needs `native::work` (core.work wrapper, Services).
  Until then the wait is counted from the clock when the screen is drawn, and the first input
  after it ends forgets the tries; `Effect::WakeAt` is carried out as nothing. (Ignored case
  `the-wait-after-five-wrong-tries-ends-on-time-without-an-input`.)
- **A kind changed without its code goes back at once**: the host refuses the subscription to a
  changed setting (`Factory(-1)`, found with `tests/hosted_construction.rs`). Until then the kind is
  put back at start and at wake, before the lock is asked for; the kind's editor is the lock's own
  screen, so the settings list does not change it. (Ignored case.)
- **Sealed *Lock now***: `ActionDecl` has no seal, so another plugin's handler could override
  `lock.now`. (Ignored case `lock-now-locks-from-any-button-gesture-menu-or-rule`.)
- **The page's icon (`lock`)** is declared, as is the `lock.now` icon. The Settings
  group registry reply still needs to carry the icon to the Settings plugin (#268).
- **The first-run step** (order 70, page = the kind setting) needs host op 22
  (Host todo 99), so it is not submitted.
- **Host acceptance** (draw, input, secrets, ui and storage through a scripted host, H1/H3/H4): the
  scripted host answers none of `core.secrets`, `core.ui` and storage yet.
- "How it works" (`how/`): the help plugin's text slot.

## Memory

The screen holds its `Flow` (a PIN of up to 16 digits reserved in one place, up to nine dots, the first
of two new codes), the kit's list of four kinds and the geometry of the last frame: well under 2 KiB.
A frame prepares at most a few hundred operations (about 60 for the keypad, 30 for the pattern), 
freed at the end of the draw. The secrets calls use two 2 KiB buffers, the password read two
4 KiB buffers, all wiped and freed in the callback. No static data.

## Fail-closed rules (review of the lock slice 2b)

- The code kept decides the lock, never the kind setting (`native_gate::active`): a reset or a plain
  list of choices may set the kind to `off` while the code stays; the kind is put back at the start
  and when the device wakes, after the lock.
- Starting, waking and *Lock now* lock first (`native_gate`). An error returned from a callback drops
  the commands it staged (screens-input.md 2.1), so once the unlock screen is staged the callback
  returns `Ok` and what failed after it is left for the next occasion (the kind, the tries). The
  device counts as locked before the screen is asked for; a locked device with no unlock screen gets
  one at *Lock now*, at waking and whenever a screen closes.
- Locked, an input that fails is taken (`Handled::Yes`), and so is one for an instance without a view.
- A handover that cannot be read, on a locked device, opens the screen *as the lock screen*: it asks
  for the code and releases nothing. The device is only ever unlocked by the right code
  (`Flow::unlock`) or by the code no longer being kept (`Flow::released`).
- A screen the host opened whose view could not be made (a refused allocation or read in `opened`)
  is made when it shows, draws or takes an input (`native_gate::Bare`, `recover`): it never stays
  blank.
- A code kept that cannot be read (the secret store refuses) locks nothing, as on interface 6: the
  start, *Lock now* and waking read it again, so it is retried there. The error is reported; no screen
  is staged for a device whose code is unknown.
- `draw` has no command stage: it only makes a missing view. The guard policy is set when the screen
  shows or takes an input (`native_gate::guard_due`), and again there where the host refused it.

## A code that cannot be kept says so (#459)

`store::keep` / `store::forget` fail where `core.secrets` cannot seal (the X4 with the radio off). The
effects after it (the kind, "done", Back) were dropped with the callback, so nothing was said. Now
`Flow::not_kept` drops them, says `lock.not-kept` and shows the kinds again; a renewal (a better salt
for a code that opens already) that cannot be kept is left for the next unlock, which goes on.
