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
| `native_touch` | the touch lock's way out (pure; see below) |

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
- **The page's icon (`lock`) and the first-run step** (order 70, page = the kind setting): the
  records exist but the host refuses icons until #517 and has no op 22 (Host todo 99). Not
  submitted, so the host admits the rest.
- **The touch lock**: the input filter that takes taps, swipes and the pen, the hint, the status
  item, the quick-panel and reader menu entries, the menu need for touch, the key map (`long.home`)
  and its actions. `native_touch` holds the pure part (the way out: tap twice and hold the second;
  the hint every 10 s) with its tests, ready for the filter (H4). Until it is wired the *Lock touch*
  setting is declared and does nothing.
- **Host acceptance** (draw, input, secrets, ui and storage through a scripted host, H1/H3/H4): the
  scripted host answers none of `core.secrets`, `core.ui` and storage yet.
- "How it works" (`how/`): the help plugin's text slot.

## Memory

The screen holds its `Flow` (a PIN of up to 16 digits reserved in one place, up to nine dots, the first
of two new codes), the kit's list of four kinds and the geometry of the last frame: well under 2 KiB.
A frame prepares at most a few hundred operations (about 60 for the keypad, 30 for the pattern), 
freed at the end of the draw. The secrets calls use two 2 KiB buffers, the password read two
4 KiB buffers, all wiped and freed in the callback. No static data.
