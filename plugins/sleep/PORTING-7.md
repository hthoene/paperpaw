# Interface-7 port (slices 1 and 2: the decisions, the declarations, the native entry and screen)

Version 1.0.4 (already above its Beta 3 version, 1.0.3; no further bump). Status: **not complete**: the native
entry (`NativeSleep`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin) shows the screen, but the cat, the
cover and the pictures need the host's image drawing (named blocked steps below), so it is not in
`tools/plugin-files7/complete.txt`. The interface-6 `Sleep` and everything of it stays until the host activates native
plugins (then it goes together, no shim; see the removal step).

## Native entry and screen (slice 2)

`src/native_plugin.rs` (`NativeSleep`: `make` submits the settings, the overlay screen, the places and the
`core.sleep` offer and the `resume` subscription), `src/native_screen.rs` (`Sleeping`: what is read when the screen
opens, the widgets made and placed at the first draw, the message, the picture over the page's panel),
`src/native_stack.rs` (the page below, which the modes `page` and `overlay` leave in place; *when* conditions of the
picture over the page). The screen is an overlay (`ScreenDecl::overlay`): the host draws the page below first. The
device waking closes it (`event.resume`); a screen still there when the device is awake goes at the next press; while
asleep every press but *sleep* and *home* is taken. What cannot be read shows the message. A refresh is asked for each
time it shows (not for `page`).

## Ported

- `src/native_sleep.rs`: what the screen shows for the setting and the facts at hand (`decide`, with
  the two fallbacks: off, a page that stayed would look like a device that is on; an overlay with
  nothing to show is the cover; the cat where there is no cover or picture; a mode of another
  plugin's screen), the pictures folder (`is_picture`, `Pictures`: one after the other from where
  the last sleep left off, `shown_before`; a folder keeps the first 512 pictures by name within 24 KiB
  of names, so a folder of more cycles only through those, whatever order the card lists them in), and the geometry of a picture over the page (`placed`
  by position inside the margins and clear of the status lines, `box_size`, `fit` equal to the
  SDK's `fit_image`, `opacity`). Allocation is fallible and swept; sizes saturate.
- `declarations.rs`: the group (order 62, moon icon), the nine settings with their editors (the
  folder and widget choosers by slot, the picker by screen), units, steps and sliders, the screens,
  the `core.sleep` offer, the `Sleep` need of the group and every setting, and the two places of
  widgets with their conditions. `tests/abi7_declarations.rs` compares each with the interface-6
  registration.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- **Decoding and drawing the cat, the cover and the folder's pictures** (the image slots 36/37 of PR 391 and
  `core.files` are not on integration); the cat is embedded (`pictures/sleeping-cat.png`) and decoded once per screen
  size. Until then the screen shows the message where they would show.
- **The logo above the message:** interface 6 draws the PaperPaw logo (`paperpaw_sdk::logo`, a third of the screen
  across) above the message; the native screen draws the message alone (`native_screen.rs` leaves the logo's place
  empty): it needs an image the plugin can draw (the same host call as the cat).
- **The picture over the page** (the overlay's `Picture` mode, `sleep.overlay-picture`): its picture needs the image
  drawing too; the widget over the page is native.
- **The picker (`pick-overlay`):** interface 6 picks the picture and the widget over the page on a screen of its own.
  It is not ported (over ~150 lines: a list of the picture folder's files and of the widgets): the two settings have
  **no editor** until then (a text row), and the screen is not declared, so that no row opens a screen nobody answers.
  `tests/abi7_declarations.rs` pins the difference to interface 6 (two extras, one screen).
- **Hidden hints:** no declaration of a screen's hint visibility.
- **Another plugin's mode:** `Registrar::add_choice(sleep::SHOW, …)` adds a choice to a setting at run time; the
  declarations carry the plugin's own seven.

(Done in this slice: the screen itself, the widgets composed with `ui.compose`, the presses taken while asleep, and
closing on wake: only a close, as interface 6 did.)

## Removal step (one sweep, with the reader port and Core #944)

1. `Sleep` and its `Plugin` impl, `SleepScreen`, the `cat` module's interface-6 drawing, `overlay`'s `Canvas` code
   (`native_sleep` and the decisions stay), the `HOW` table kept for the manifest;
2. the app's store-offer line and the other plugins' tests that register `Sleep`;
3. list `NativeSleep` in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt` once the images are native.
