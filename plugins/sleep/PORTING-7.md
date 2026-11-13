# Interface-7 port (slices 1 to 5: the decisions, the declarations, the native entry and screen, the pictures, the picker, the custom logo)

Version 1.0.4 (already above its Beta 3 version, 1.0.3; no further bump). Status: **not complete**: the native
entry (`NativeSleep`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin) shows the screen and, since
slice 3, its pictures (`src/native_screen/images.rs`); since slice 4 (#257) the picker (item 4,
`src/native_pick.rs`, `src/native_picker.rs`); since slice 5 (#257) the custom logo of `core.logo` (item 3); the steps
named below are open, so it is not in
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
  plugin's screen), `binding` (the screen `core.sleep` is bound to for a stored mode, item 5), the pictures folder (`is_picture`, `Pictures`: one after the other from where
  the last sleep left off, `shown_before`; a folder keeps the first 512 pictures by name within 24 KiB
  of names, so a folder of more cycles only through those, whatever order the card lists them in), and the geometry of a picture over the page (`placed`
  by position inside the margins and clear of the status lines, `box_size`, `fit` equal to the
  SDK's `fit_image`, `opacity`). Allocation is fallible and swept; sizes saturate.
- `declarations.rs`: the group (order 62, moon icon), the nine settings with their editors (the
  folder and widget choosers by slot, the picker by screen, `pick-overlay`, for both the picture and
  the widget over the page), units, steps and sliders, the screens (the picker and the sleep screen),
  the `core.sleep` offer, the `Sleep` need of the group and every setting, and the two places of
  widgets with their conditions. `tests/abi7_declarations.rs` compares each with the interface-6
  registration: they are equal, the picker's two extras and its screen included.
- The picker (`pick-overlay`, slice 4): see item 4 below.

## Not yet (ignored acceptance cases in `tests/acceptance_pending.rs`)

- **Hidden hints:** done (#2769): the sleep screen is declared `hints_hidden` (flag bit 8), as Beta 3 showed no
  row of button hints; pinned by `tests/abi7_declarations.rs`.
- **Another plugin's mode:** the declarations carry the plugin's own seven; another plugin's mode is a foreign
  `setting.choice` it declares itself, and sleep binds the slot for it (item 5 below).

(Done in this slice: the screen itself, the widgets composed with `ui.compose`, the presses taken while asleep, and
closing on wake: only a close, as interface 6 did.)

## What is missing to be native-only (state of #692, 2026-11-09)

The image facade is PR #692 (`native::image`, base `kernel/abi7-draft30-228`, in the draft-31 carrier after #610):
`CommandSender::image_prepare(PrepareImage::new(PrepareRoot::Data | Library, path, cookie).side(n))` decodes a
JPEG, PNG (not interlaced) or the first GIF frame of one of the plugin's **files** in the background; one
`event.image_ready` (`ImageReady`: status, `ImageId`, size) answers per cookie; `Canvas::draw_image(id, rect,
ImageStyle::new().fill().small().transparency(n))` draws it in bands inside a draw callback; `image_release` gives it
back; at most 8 prepared and 4 waiting per plugin. With it the items below become native:

1. **The cat and the logo: done (slice 3).** The cat (`pictures/sleeping-cat.png`) and the logo (`pictures/logo.png`: the
   glyph of the About plugin's logo, cropped square, since the SDK's logo is drawn from shapes and not a file) are written
   once into the data folder (`cat.png`, `logo.png`, as `DataStorage` does, rewritten only when the size differs) and
   asked for with `image_prepare` (`PrepareRoot::Data`; the cat at the screen's shorter side less the interface's two
   margins, the logo at a third of it). A draw can send no command (`commands.md`), so the request goes out when the
   screen opens (`ScreenOpened`, `Sleeping::begin`); the `ImageId` is kept in `Pics`, drawn with `draw_image` (the cat
   fitted in the middle inside the margins on white, the logo `small()` above the message, both as interface 6 placed
   them) and released at `ScreenClosed`. Until the answer (`event.image_ready`, token `IMAGE_READY`) the screen draws
   the message; the answer asks for a **full** refresh (the pictures are final, in grays) when what shows changed.
2. **The picture of the folder and the picture over the page: done as far as the contracts allow (slice 3).** The next
   file of `sleep.folder` (`core.files` listing with `Root::Files`, the plugin's `Pictures`, `shown_before` kept in
   `next.txt`) goes to `image_prepare` with `PrepareRoot::Library`; the plugin declares the optional `uses.storage`
   (and `uses.library`, `core.files`, `core.library`); without the reader's consent the listing and the prepare are
   refused and the cat shows (interface 6 read the card unasked: this is a consent to give). The picture over the page
   is the file `sleep.overlay-picture` names or the folder's next one, fitted and placed as `native_sleep::placed`
   says, drawn with `draw_image` and the see-through setting as `transparency`. Both are drawn **fitted and centred**
   (not `fill`): that is what interface 6's `fit_image` did. A file the host cannot decode, or none, clears the fact
   and `native_sleep::decide` falls back (the picture to the cat, the picture over the page to the cover, as before).
   **Named blocked steps of this item:**
   - *the clear parts of a PNG or GIF showing the page* (interface 6 decoded with alpha, `AlphaFile`): `image_prepare`
     decodes to the opaque gray/colour cache format and has no alpha flag; needs a prepare flag for alpha (Services);
     until then the picture over the page is opaque where it is not see-through;
   - *a picture a preset names under another plugin's folder* (`plugins/<id>/owl.png`): the card root (1) excludes
     `plugins/` and `.paperpaw/` and root 2 is the plugin's own install folder only; since #243 (`files.md` 1.1) root 1
     shows a **running data plugin's** folder read only to `image_prepare` (with `uses.storage`), so a data plugin's
     preset picture is read; a **code** plugin's folder, or a data plugin switched off or gone, stays refused (status 6,
     not found) and the fallback shows; that needs a root for another plugin's files (Host);
   - *why the picture over the page failed, for the picker* (`overlay-failed.txt`): done with the picker (item 4).
3. **The cover: done (slice 3).** `Library::cover` (op 7) for the first book of `Query::new().sort(RecentlyOpened)
   .descending(true)` that is `opened`, at the screen's longer side, held as the cover's `ImageId` (no prepare), released
   at close; also looked for as the spare of a picture over the page. No book read, a book without a cover, or a cover
   **still being made (`Busy`)** counts as no cover: the cat shows at that sleep (interface 6 made it in place; the
   device cannot wait for `library_changed` bit 6 while it falls asleep).
   **Other decisions that fall back at draw time:** a widget (or the list of widgets) that cannot be made is known only
   when the first draw lays it out, and a draw can ask for nothing, so the fallback picture of that case is the cover
   where one was held (looked for beforehand for the page mode) and else the message without a logo (interface 6: the
   cat or the logo); named step: a pre-asked cat and logo, or a command allowed in a draw (Host).
   **The custom logo of `core.logo`: done (slice 5, #257).** The message's logo is the card file the core setting
   `core.logo` names (a JPEG, PNG or GIF, e.g. `plugins/mine/logo.png` a data plugin's `settings.ini` sets, or any card
   path), read with `settings.read` as the other settings (a core setting is public: no feature, no metadata change)
   and trimmed of spaces and slashes at its ends as Beta 3's `Logo::read` did (`native_sleep::logo_file`). Where it is
   set, the request for the logo (from the events, `Sleeping::begin`, as before) is `image_prepare` with
   `PrepareRoot::Library` and that path at the logo's side (a third of the screen's shorter side); the answer is drawn
   `small()` and **fitted, not filled**, into that square: its fitted size (`images::fitted`, Beta 3's `Logo::fit`)
   lays out the message below it, as Beta 3 placed it. A refused request (no `uses.storage` consent, a path refused,
   one longer than 512 bytes) or an answer that is not `Ready` (not found, damaged, unsupported, too large, no memory,
   card failure) asks for the plugin's own `logo.png` (`PrepareRoot::Data`) instead, as Beta 3 fell back to its drawn
   logo; that one failing too leaves the message alone, as before. One logo picture is held at most (the card's is
   only asked for before the own one, never both held), released at `ScreenClosed` with the rest; the setting is read
   once per open into a 4 KiB buffer freed at once. **Differences to Beta 3:** a card picture now needs the reader's
   consent (`uses.storage`, optional; without it the plugin's logo shows); the host's picture cache decodes it (Beta 3
   wrote `logo.pixels` and `logo.stamp` into its data folder); a file under a **code** plugin's folder, or under a data
   plugin that is switched off, is refused by root 1 (`files.md` 1.1: only a running data plugin's folder is shown)
   and the plugin's logo shows: that needs a root for another plugin's files, as in item 2 (Host); the fallback is the
   plugin's own picture, not the logo drawn from shapes (as since slice 3). Pinned by `tests/sleep_logo.rs` (empty:
   own; set: the library picture, trimmed; failed: own; too long: own) and `images`' unit tests (a failed card logo
   asks for the own one; fitted sizes).
   **Not ported with the images:** the image description for accessibility (`Canvas::describe`: the native canvas
   has none). The gray-levels hint of `Screen::grays` is ported (#2769): every picture of the screen is drawn with
   `ImageStyle::grays()` (the panel's grays where it has them), as Beta 3 drew the sleep screen.
4. **The picker (`pick-overlay`): done (slice 4, #257).** The screen `sleep.pick-overlay` (token `PICK`) is declared
   and is the editor (`EditorDecl::Screen`) of both `sleep.overlay-picture` and `sleep.overlay-widget`, as in
   interface 6. Its rows are Beta 3's, in its order (`overlay::Picker`; the model is `src/native_pick.rs`, the screen
   `src/native_picker.rs`, the kit's `ItemList`): *From the pictures folder* (both settings empty), the presets of
   kind `sleep.overlay` (`core.registry` op presets, in the host's order, names as the host translates them), then
   every widget (*Widget: name*, `core.registry` op widgets). The row of what is chosen says *On* (`core.state.on`):
   a widget set marks only its row; else the folder where no picture is set, a preset whose values the settings hold
   (`core.registry` op values, compared with `settings.read`; a value the registry withholds is not compared).
   Choosing sets `sleep.overlay-widget` (empty but for a widget), then `sleep.overlay-picture` to empty (the folder)
   or applies the preset (`command.apply_preset`, `Apply`; one gone since is left), and closes the screen in the same
   callback; Back is the host's and closes. The rows are read when it opens, each registry list one page at a time
   with 4 KiB buffers freed before the screen shows; only the rows are held, and they go when it closes. A registry
   that refuses a list leaves its rows out (the folder row is always there); only a memory shortage fails the open.
   **Why the picture over the page did not show at the last sleep** is noted below the rows (*Not shown at the last
   sleep: …*, `Footer::Note`), from `overlay-failed.txt` in the data folder, which the sleep screen now keeps as
   interface 6 did (`native_picker::remember`, written only when it changed, removed when the picture showed): *There
   is no picture in the pictures folder* (no file of the folder, also a folder the reader gave no consent for),
   *{name} is not there* (`ImageStatus::NotFound`, or a prepare the host refused), *{name} cannot be shown
   ({reason})* (the other statuses, the reason in English as interface 6 gave its decoder's), *There is no widget
   {name}*. Picture failures are kept from the events they are known in (the open, `event.image_ready`); a widget
   that cannot be made is known only at the first draw, which writes no file, so it is kept when the screen closes
   (a device switched off with the screen up keeps the note of the sleep before). Pinned by `tests/native_pick.rs`
   (rows, order, the mark, choosing, a picker of the folder alone, the failures, refused allocations) and
   `tests/abi7_declarations.rs`.
   **Differences to Beta 3:** the reason of an unreadable picture comes from the host's status (Beta 3 gave its
   decoder's own words); a folder the reader gave no consent for reads as empty; the widget's note is kept at close,
   not at open; the widgets are those `core.registry` lists (it leaves out the widgets that only preview a settings
   group); the mark is decided when the picker opens (Beta 3 at each draw; a choice closes it either way).
5. **Another plugin's mode (reading-goal's `reading-goal.sleep`): settled (Kernel's ruling), done in sleep.** Nothing is
   embedded or opened by sleep: the sleep screen stays the slot `core.sleep`, shown by the host with its device-state
   rules. The offering plugin (reading-goal) declares a `slot.offer` of its screen `reading-goal.sleep` to the slot
   `core.sleep` (priority below sleep's own screen) and a foreign `setting.choice` (`registry.md` record 26,
   `Declarations::setting_choice("sleep.show", "reading-goal.sleep", title)`) whose value is that screen id. Sleep's
   part (`native_plugin.rs`, `bind_mode`; the decision is `native_sleep::binding`): at start and on `event.setting.changed`
   for `sleep.show` (or an empty key) it reads the setting and
   - for a **foreign value** (not one of the seven modes, not empty) that is a **current candidate** of `core.sleep`
     (the registry's slot offers, matched by screen id) it calls `command.bind_slot("core.sleep", Some(screen))`;
   - for its own modes, an empty value, its own screen's id, or a value no plugin offers (stale) it calls
     `bind_slot("core.sleep", None)`: back to the default, its own screen.
   The host refuses a screen not offered for the slot with `ERR_INVALID`; Invalid, NotFound, Denied and Unsupported
   are taken as "no binding" (never the plugin's failure), as is any other error but a memory shortage. The command
   needs the optional feature `uses.registry` (`registry.md` section 4.2; metadata declares it, `required: false`;
   without the user's consent the host answers Denied and the mode keeps the sleep screen). The registry query needs
   no feature (`core.registry` was declared already). **Meanwhile, and whenever the plugin that offered the mode is
   gone, a stored `sleep.show = reading-goal.sleep` (or any mode no plugin offers) falls back as interface 6 did: the
   cover where there is one, else the cat** (`native_sleep::decide`; the stored word is left as it is; pinned by
   `a_stored_mode_no_plugin_offers_falls_back_to_the_cover_or_the_cat` over all fact combinations).
   `Facts::other_screen` stays false and `Content::Other` is never answered: the other plugin's screen is shown by the
   host, not by sleep. **Binding and unbinding (Kernel's rulings; the host does not resolve slots from `sleep.show`):**
   - **At start and on `plugins-changed` (event 70):** a stored foreign value that is offered for `core.sleep` binds the
     slot, but only while the slot's `reg_slot` bit 0 (the reader bound it) is clear; **never an unbinding** there
     (`native_sleep::plan_stored`). So a Beta 3 stored `reading-goal.sleep` binds at the first start, and a binding the
     reader made survives every start. A stale value only shows the cover/cat fallback.
   - **On `setting.changed` for `sleep.show`, and for the empty key** (a bulk reset or import: `sleep.show` is read
     again and the same reconcile runs): a foreign value that is a current candidate binds; a change to one of its
     own modes (or an empty value) binds back to the default (empty screen) **only if the slot is bound now to one
     of `sleep.show`'s foreign choices** (registry ops `slots` and `choices`), so a screen the reader bound for another
     purpose stays; a stale value or the plugin's own screen's id leaves the slot alone (`native_sleep::plan`).
   Pinned by `a_beta_3_stored_value_binds_at_first_start`,
   `a_binding_of_a_non_sleep_screen_survives_start_event_70_and_a_reset`, `a_binding_the_reader_made_survives` and
   `a_change_unbinds_only_a_sleep_mode_and_binds_an_offered_one`. No consent is needed for a change (the reader's own
   setting change is the consent); the metadata still declares the optional `uses.registry` that `registry.md` 4.2
   names for `command.bind_slot`. Known limit: nothing binds when a plugin offering the mode appears later than the
   next start or event 70.
   **The registry handle** (`Registry::open`, for the slot offers) is opened the first time a foreign value needs the
   list and then kept for the plugin's lifetime (`NativeSleep::registry`), as `ui` is; it holds nothing the host has to
   give back before the plugin stops, and each query uses its own 4 KiB buffers that are freed after the call.
   **Two `hosted_construction` pins:** `tests/hosted_construction.rs` pins `make` as refused with `Factory(-1)`
   (`Unsupported`) because the batch subscribes to `event.setting.changed`, for which the host has no encoder yet;
   other plugins' tests still pin `DeclarationViewsUnavailable` (the commit refused, #944) because they do not
   subscribe to it. When Core adds the encoder, sleep's pin returns to `DeclarationViewsUnavailable` and then to the
   positive case with #944; the ignored positive test stays as it is.
6. **Hidden hints:** done (#2769, `ScreenDecl::hints_hidden`).

The order that makes the plugin native-only: #692 → 1 and 2 (one decode at a time, four waiting at most) → 3 with the
cover facade → 4 (5 is done); 1 to 3 are in slice 3, 4 in slice 4.

**Remaining gaps** (after slice 5 and #2769): the clear parts of a
PNG or GIF over the page (alpha, item 2); a picture (or a `core.logo`) under a code plugin's folder or a data plugin
that is not running (items 2 and 3, a root for another plugin's files); the image description
(item 3); a pre-asked cat and logo or a command allowed in a
draw for the fallbacks decided at draw time (item 3); the production loader and the qualification of screen commands,
composition and input (the acceptance cases in `tests/acceptance_pending.rs`, H1, H3, H4).

## Removal step (one sweep, with the reader port and Core #944)

1. `Sleep` and its `Plugin` impl, `SleepScreen`, the `cat` module's interface-6 drawing, `overlay`'s `Canvas` code
   (`native_sleep` and the decisions stay), the `HOW` table kept for the manifest;
2. the app's store-offer line and the other plugins' tests that register `Sleep`;
3. ~~list `NativeSleep` in `NATIVE_PLUGINS`~~ (done, #2769: under `native-linked` the app lists the native entry with
   its settings tables kept in the program, `fixed_tables`, and no longer installs the interface-6 `Sleep`); list it in
   `tools/plugin-files7/complete.txt` with step 1 (that list is for plugins with no interface-6 part left).
