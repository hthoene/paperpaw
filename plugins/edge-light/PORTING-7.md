# Edge light on interface 7

Status: **complete.** A new plugin (#104), never published on interface 6, written as a native plugin,
version 1.0.0. No interface change (draft 36).

## What it uses

- **The input filter** (`core.input-filter`, `docs/abi/ui.md` 4.1): every finger goes to it first. It answers
  *pass* to all of them but one (below). `swipe::Tracker` follows a finger that goes down in a strip along the
  left or right edge; once it has moved up or down (more than the slop, twice as far as sideways) every sample
  asks for the light from how far the finger is above where it became a swipe, in steps of 5 %: the filter
  answers *run* `edge-light.step` (a filter's callback may send no command) and that action, an ordinary
  callback, sets the level and says it.
- **A swipe that is not ours is never touched:** a finger that starts elsewhere, one that moves sideways first
  and a tap are all *pass*. The vertical swipe the kernel recognises when a finger of ours lifts is *take*n
  (`Tracker::gesture`), so that no page turns or list scrolls by it.
- **The setting** `core.frontlight` (brightness) or `core.frontlight-warmth`, by `command.set_setting`, and the
  new level as a notice (`command.notify` with `announce`: the plugin has no screen).
- **Touch and a front light:** the settings page and the action are `menu.need` touch and front light (hidden
  elsewhere); the filter reads `core.device` once and does nothing without both.
- **The widget** *Front light* (`core.widget`): the level and a bar; a tap, and the action
  `edge-light.toggle`, switch the light off and on again at the level it had.

## Gaps against the idea (#104), on purpose

1. **Steps of 5 %, not stepless.** The light is previewed while the finger moves and set once when it lifts only
   by `command.preview_setting`, and a preview must belong to the submitter's own top screen (`registry.md`
   4.4, `ERR_STATE` otherwise): a filter has none, and a screen opened for the swipe would take the finger's
   up from the screen below. So each 5 % step is a setting change (and a write of the configuration file, at
   most twenty for a swipe over the whole range). A preview without a screen of the plugin's own would make
   it stepless.
2. **A finger cannot be taken half way:** *take* and *cancel* count only at a pointer down (`ui.md` 4.1), so
   the screens see the swipe along the edge as they would any vertical swipe; only the gesture it ends with
   is taken. Nothing is scrolled by a swipe on the screens of PaperPaw that act on the gesture alone.

## Tests

- `src/swipe.rs`: gesture recognition from sample sequences (a swipe in each strip, a vertical swipe elsewhere
  and a sideways one that turns vertical are not taken, taps, a diagonal, a second finger, a cancel, only the
  vertical swipe straight after is taken, a side that sets nothing) and the level from the finger's way.
- `tests/native_filter.rs`: the real plugin on the scripted host, as the kernel drives it (a stage call per
  finger): a swipe up the left edge answers *run* `edge-light.step` and the action sets the brightness and says
  it, then the swipe is taken once; the right edge sets the warmth; a swipe elsewhere, a tap and a sideways
  swipe pass; a device without touch or without a front light is left alone.
- `tools/pps/edge-light.pps`, run by `tools/pps/edge-light/run-emulator.sh`: the emulator with the X4 Pro's
  profile and a touch: swipes along each edge, a page swipe elsewhere, the widget.
