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

## What is missing to be native-only (state of #692, 2026-11-09)

The image facade is PR #692 (`native::image`, base `kernel/abi7-draft30-228`, in the draft-31 carrier after #610):
`CommandSender::image_prepare(PrepareImage::new(PrepareRoot::Data | Library, path, cookie).side(n))` decodes a
JPEG, PNG (not interlaced) or the first GIF frame of one of the plugin's **files** in the background; one
`event.image_ready` (`ImageReady`: status, `ImageId`, size) answers per cookie; `Canvas::draw_image(id, rect,
ImageStyle::new().fill().small().transparency(n))` draws it in bands inside a draw callback; `image_release` gives it
back; at most 8 prepared and 4 waiting per plugin. With it the items below become native:

1. **The cat and the logo.** Both are embedded (`pictures/sleeping-cat.png`, the SDK's logo): write each once into the
   data folder (`DataStorage`, as `format-images` and `translate` write their files), `image_prepare` it at the first
   draw's size (side of a third of the screen for the logo), keep the `ImageId` in `Laid`, draw it with `draw_image`
   (centred, `small()` for the logo), release it when the screen closes. The first draw has no picture yet: the
   screen draws the message and asks for a redraw on `image_ready` (a `Partial` refresh, then it shows).
2. **The picture of the folder** (`picture`) and the picture over the page: the next file of
   the folder setting (the plugin's `Pictures`, `shown_before` kept in `next.txt`) is a file of the user's
   card, not of the plugin's data folder: listing it needs `core.files` and `PrepareRoot::Library` needs
   `uses.storage`, which the plugin's metadata does not declare yet (interface 6 read the card without a consent:
   a new consent for the user to give), with the path inside the library; the answer's size gives the scale; `draw_image(..).fill()` is
   what interface 6's `fit` did for `cover`/`picture`.
3. **The cover** (`cover`): the cover of the book last read is a file the host's library keeps: it needs the library
   facade's cover path (`services/abi7-sdk-cover-228`) to name it for `image_prepare`; until that lands the cover
   mode falls back to the message as the decision table already says (`facts.cover` false).
4. **The picker (`pick-overlay`):** not an image matter: a list screen of the picture folder (`core.files` listing,
   as `picture-home`'s chooser) and of the widgets (`core.registry`): a slice of its own; the two settings have no
   editor until then.
5. **Another plugin's mode (reading-goal's `reading-goal.sleep`):** on interface 6 the plugin adds the choice
   with `Registrar::add_choice(sleep::SHOW, …)` and the sleep screen embeds the plugin's `Screen`. Natively the
   **offering side exists**: the offering plugin (reading-goal) declares the foreign `setting.choice` (`registry.md`
   record 26) with `Declarations::setting_choice("sleep.show", "reading-goal.sleep", title)`; sleep itself declares
   nothing for it. What is missing is the **showing side**: a mode's screen cannot be embedded in another plugin's
   draw, and the contract to settle (Kernel, asked) is that the sleep screen opens it with `command.open` in
   `replace` mode (the offered screen declared `overlay`, closing itself on `event.resume`), after checking the
   stored value among the `core.registry` choices (op 3). Until it exists, `native_sleep::decide` never answers
   `Content::Other` (`Facts::other_screen` is false).
   **Meanwhile a stored `sleep.show = reading-goal.sleep` (or any mode no plugin offers) falls back as interface 6
   did for a mode no plugin offers any more: the cover where there is one, else the cat** (`native_sleep::decide`;
   the stored word is left as it is; pinned by `a_stored_mode_no_plugin_offers_falls_back_to_the_cover_or_the_cat`
   over all fact combinations). `Facts::other_screen` is false until the builder and the opening contract exist.
6. **Hidden hints:** no declaration of a screen's hint visibility (kernel's).

The order that makes the plugin native-only: #692 → 1 and 2 (one stacked PR on #692's branch; one decode at a time,
four waiting at most) → 3 with the cover facade → 5 with the builder → 4.

## Removal step (one sweep, with the reader port and Core #944)

1. `Sleep` and its `Plugin` impl, `SleepScreen`, the `cat` module's interface-6 drawing, `overlay`'s `Canvas` code
   (`native_sleep` and the decisions stay), the `HOW` table kept for the manifest;
2. the app's store-offer line and the other plugins' tests that register `Sleep`;
3. list `NativeSleep` in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt` once the images are native.
