# Interface-7 port (complete: interface 6 is removed)

Version 1.0.4 (already above its Beta 3 version, 1.0.3; no further bump). Status: **complete**, with the named
steps below that wait for the host (the hidden hints of the screen first). The plugin is native only: the
native entry (`NativeSleep`, `NATIVE_ENTRY`, `native/` metadata, the `plugin-files7` bin) is the whole plugin, it
is in `NATIVE_PLUGINS` and in `tools/plugin-files7/complete.txt`, and the app links it and no interface-6 form.

## Removed (interface 6)

`Sleep` and its `Plugin` impl (manifest, registration, the screen's slot binding, the `HOW` table; the
catalogs carry the name, the description and `sleep.plugin.how`, the text of `how/`, which stays in the crate for
the store's page), `SleepScreen` and its drawing (the cat, the cover, the picture, the message with the logo,
the page, the picture over the page, another plugin's screen embedded), `overlay.rs` (the `Canvas` code, the
alpha decoding, the key of the decoded file and the interface-6 `Picker`), the `cat` module's decoding (only
`cat::PICTURE`, the file the native screen writes and has decoded, stays), the `.gray` and `.key` files it wrote
(earlier versions' leftovers are not read; `docs/data-files.md`), the interface-6 test suites that drove them
(`sleep_screen.rs`, `over_page.rs`: on a kernel with the real fonts) and the parity of the declarations with the
interface-6 registration (`tests/abi7_declarations.rs` now pins the declarations by what they say and by a reviewed
golden, `testdata/declarations.bin.v1`, `PAPERPAW_BLESS=1` writes it anew). The pure rules those suites also
tested are `native_sleep` and `native_stack` (with their tests); `next.txt` of version 1 is read
(`the_next_picture_of_version_1_is_read`). The app's store-offer line is gone and `paperpaw-plugin-sleep` is an
optional dependency of the app, switched on by `native-linked`; the dev-dependencies of `reading-goal` and
`vocabulary` and the interface-6 sleep in their tests are gone (vocabulary's `cards.rs` stands in for the slot's
screen with a few lines, and the app's flows, the desktop snapshots and `headless.rs` follow: no sleep screen
shows in the interface-6 programs, which do not run native plugins).

## Native entry and screen

`src/native_plugin.rs` (`NativeSleep`: `make` submits the settings, the screens (the overlay screen and the picker), the places and the
`core.sleep` offer and the event subscriptions; the picker's events and draws go to `native_picker`), `src/native_screen.rs` (`Sleeping`: what is read when the screen
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
  folder and widget choosers by slot, the picker by screen), units, steps and sliders, the screens,
  the `core.sleep` offer, the `Sleep` need of the group and every setting, and the two places of
  widgets with their conditions. `tests/abi7_declarations.rs` reads the batches back (checked by the host's
  rules) and pins them by what they say and by a reviewed golden.

## Waiting for the host (named steps; ignored acceptance cases in `tests/acceptance_pending.rs`)

- **Hidden hints** (kernel's H1b, #851, not merged): interface 6 hid the hints row of the screen while asleep
  (`set_screen_hints(…, Hidden)`); there is no declaration of a screen's hint visibility yet. Named step: declare
  it for `sleep.screen` when #851 lands (`acceptance_pending.rs`, `sleep-buttons-and-hints`).
- **Clear parts of a PNG or a GIF over the page**, **a picture named under another plugin's folder**, **a
  widget that cannot be made, known only at draw time**, the **custom logo**, the **image description** and the
  **gray-levels hint**: item 2 and 3 below, each with what it needs from the host.
- **The flash card as a sleep mode** (vocabulary's `vocabulary.sleep-card`): vocabulary's own port declares the
  offer and the `setting.choice` (as reading-goal's); interface 6 no longer adds the choice (see vocabulary's
  PORTING-7.md).
- **Another plugin's mode**: sleep binds the slot for it (item 5 below); the host does not resolve slots from
  `sleep.show`.

## The pictures and the other parts (state of #692, 2026-11-09)

The image facade is PR #692 (`native::image`, base `kernel/abi7-draft30-228`, in the draft-31 carrier after #610):
`CommandSender::image_prepare(PrepareImage::new(PrepareRoot::Data | Library, path, cookie).side(n))` decodes a
JPEG, PNG (not interlaced) or the first GIF frame of one of the plugin's **files** in the background; one
`event.image_ready` (`ImageReady`: status, `ImageId`, size) answers per cookie; `Canvas::draw_image(id, rect,
ImageStyle::new().fill().small().transparency(n))` draws it in bands inside a draw callback; `image_release` gives it
back; at most 8 prepared and 4 waiting per plugin. With it the items below are native:

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
     `plugins/` and `.paperpaw/` and root 2 is the plugin's own install folder only; needs a root for another plugin's
     files (Host); a path there fails and the fallback shows;
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
   **Not ported with the images:** the custom logo picture of `core.logo` (a card file named in `settings.ini`; the
   contracts allow it through `PrepareRoot::Library`, a small slice of its own), the image description for
   accessibility (`Canvas::describe`: the native canvas has none) and the gray-levels hint of `Screen::grays`.
4. **The picker (`pick-overlay`): done.** `src/native_picker.rs` (the screen: it reads what the rows need, draws
   the kit's `ItemList` and sets the settings), `src/native_pick.rs` (the rows, which one is chosen, what a choice
   sets: without the host, with tests), `src/native_folder.rs` (the pictures folder through `core.files`, shared
   with the folder picture). One screen for both settings (`EditorDecl::Screen("pick-overlay")` on
   `sleep.overlay-picture` and `sleep.overlay-widget`, as interface 6): *From the pictures folder* (the folder's
   pictures in turn: clears the picture and the widget), then each picture of the folder (the first 512 by name, as
   the screen's own listing; a path over 1 KiB is not offered; sets `sleep.overlay-picture` to `folder/name` and
   clears the widget), the presets of kind `sleep.overlay` (`Registry::presets`; applied with `apply_preset`, the
   widget cleared, marked where every value is what its setting holds: `Registry::values`), and the widgets
   (`Registry::widgets`, at most 256, as *Widget: {name}*; sets `sleep.overlay-widget`). The chosen row is marked
   with a bullet and the focus starts on it (a widget set overrides the pictures, which are then not marked). A
   choice sets the settings and closes the screen in **one callback** (`set_setting` and `close_screen` on one
   `CommandSender`: an error drops both). With the optional storage consent refused (or a folder that is not
   there) the folder has no pictures; with no registry the rows are the folder's only. **The footer** says why
   nothing over the page shows, as interface 6 did: for a widget that is no longer there (found when the picker
   opens, from the registry), else the reason the last sleep kept in `overlay-failed.txt` (written by the screen
   when the host answers a prepare request with a failure or refuses it, or the folder has no picture; removed
   when the picture shows; only when it changed; the text is in the reader's language at that time, with the
   reason keys `sleep.overlay.reason.*`). Differences from interface 6: the folder's pictures are rows (interface
   6 offered only the folder as one row and the presets); a widget that cannot be made at the sleep itself is
   noted only by the picker's own check (a draw sends nothing and writes nothing).
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
6. **Hidden hints:** no declaration of a screen's hint visibility (kernel's H1b, #851): the named step above.

