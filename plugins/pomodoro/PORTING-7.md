# Pomodoro on interface 7

Status: **complete, with two named gaps.** A new plugin, never published on interface 6: it is the
interface-6 draft (PR #57, branch `plugin/pomodoro-107`, built on the owner's `hthoene/pomodoro`) written
as a native plugin, version 1.0.0. No interface-6 code is left or needed; no interface change.

Gaps against the draft, on purpose (no host call for them):

1. **Keep the screen awake during a round** (`pomodoro.awake`) is gone: interface 7 has no wake hold for a
   plugin's screen (`docs/abi/screens-input.md`, `work.md` 0). The device sleeps as the reader's *Sleep after*
   says; nothing runs while it sleeps, and the round is found again by the wall clock at the wake.
2. **The status item** (minutes left in a status line) is not ported: it needs the `core.status` stage and
   a status-item declaration, which this slice leaves out.

## Where things are

- `declarations.rs` (built into the image by `build.rs`): the settings group (order 90: *Focus*, *Short
  break*, *Long break* in minutes, *Rounds before the long break*, *Start the next phase by itself*, and
  under *More* the end signal and the daily goal), the screen, the actions `open`, `toggle` and `skip`, the
  Tools entry (order 80), the three widgets with the shared look and their parts (timer: bar, dots; today:
  week), and the events subscribed.
- `src/phase.rs`: the rules, with no clock and no screen (which phase follows, lengths, minutes left, how
  often to draw, the seven days' counts). Unchanged from the draft.
- `src/state.rs`: `Timer` (phase, running, end on the host's clock and by the wall clock, rounds, counts), its
  kept text `state.txt` (`phase|running|end|left|done|day|counts`, 160 bytes at most, a damaged or longer
  file is none) and how a running round goes on after a start (`restore`) or a wake (`wake`). It asks no
  host: the callers give the times. Nothing allocates.
- `src/native_shown.rs`: `Shown`, what the widgets and the screen draw of the timer, and the two count lines.
- `src/native_widgets.rs`, `src/native_screen.rs`: the widgets as drawing operations (by pixel room, not by
  cells) and the screen (the kit's header and chips).
- `src/native_plugin.rs` (`NativePomodoro`), `src/native_entry.rs`: the glue.

## How it keeps the e-ink calm

- The end of a phase is one work timer (`pomodoro.done`, `after_ms` the time left), whatever screen is open.
- A second timer (`pomodoro.tick`) exists only while the timer runs and a widget is placed or the screen is
  open: once a minute on the minute of what is left, every ten seconds in the last minute **on the screen
  only**. A tick tells a widget (`widget_changed`) only if its picture is another (the minutes, the bar); the
  screen draws only its clock block (phase, clock, bar) as a partial refresh. A button draws the whole
  screen; moving the focus draws the two buttons. Nothing draws per second.
- The end signal *Flash the screen* is one full refresh where the plugin's screen is on top (and nothing
  elsewhere: only the notice); *Blink the front light* sets `core.frontlight` to the other end for one second
  (the least a work timer waits) and back. A blink cut short by a restart leaves the light where it was set.
- Work does not run while the device sleeps and the host's clock may begin again after it: at the wake
  (`event.resume`) the end is worked out anew from the wall clock; a phase that ended meanwhile is finished
  and told then.

## Tests

- Unit tests beside the rules (`phase`, `state`: the kept text, a restart, a wake, rounds and the long break,
  a skipped round counts nothing), `native_shown` (what the widgets show in each state, the lines), the
  screen's clock text.
- `tests/native_flow.rs`: the real plugin on the scripted host: a start declares the end and (with a widget)
  the minute tick, the tick tells the widget, the end gives its notice and the break stands ready, a stale
  end does nothing, auto-start, the goal's notice, the light's blink and return, a wake by the wall clock.
- `tests/native_linked_entry.rs`; `tools/pps/pomodoro.pps` runs it on the emulator (switch on, the
  screen's buttons, the widgets, Back).
- The scripted host has no data folder: the file's write and read are covered at the unit level only.
