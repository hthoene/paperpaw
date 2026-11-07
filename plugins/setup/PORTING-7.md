# Interface-7 port

Version 1.0.4. Native plugin: `src/native_plugin.rs` (`NativeSetup`: the start, the guide screen,
the skip question, the commands), the guide's state and what an input means in `src/native_guide.rs`
(`Guide`, `Act`, painting), the geometry in `src/native_layout.rs`, the registry reads in
`src/native_steps.rs` (the steps that apply, the setting of a page with its help and options), the
marks in `src/native_state.rs`. `NATIVE_ENTRY` (feature `linked`),
declarations in `declarations.rs` built into the image by `build.rs` (screen, slot offer for
`core.setup`, action and menu entry, start event), metadata in `native/` (regenerate with
`python3 tools/abi-metadata.py --schema abi/abi.def --input native/metadata.json --bytes
native/metadata.bin --json native/metadata.inspected.json`, likewise `metadata.host.*`).

## Ported

- The guide: dots for how far, the question large with its explanation, the answer in a card (a
  value, a switch, two options side by side, − and + with a sample of the text at that size), *Next*
  always right, *Skip* on the first page (by *Back*: a question, going on first), *Back* on the
  others; the last page tells where books come from. Laid out roomy, close where it does not fit
  (the largest text on a small screen).
- Buttons: up/down (previous/next) move among the card and the buttons; left/right change a number
  or pick one of two options, else move; Select chooses; page buttons turn pages; touch: tap.
- The steps are the registry's setup steps that apply now (the host evaluates applicability, not
  plugin code), in their order; a step whose setting is gone is left out. A setting step shows
  the setting's title (or the step's), its help, its value; a screen step opens the screen.
- Marks as before: `done` (finished or skipped) and `trial` (written before the guide opens: where
  it cannot be, the guide does not open at all) in the plugin's data; a notice when `done` cannot
  be written. A device where a book was opened already is marked done and gets no guide: the plugin notes
  the first book opened (`event.book_opened`, reason 0) in its own data (`opened`), without the
  library feature (least privilege, Kernel). A book read while setup was disabled re-offers the guide
  once, skippable (accepted difference from interface 6).
- The guide is the screen of the slot `core.setup` (another plugin can replace it); *Set up
  PaperPaw* in the settings (menu `core.settings-pages`, order 97, icon `wand`) opens the slot.
- Texts, ids, icon and en/de catalogs unchanged (two words added: on and off).

## Tests

Unit: what every input means on a laid-out page (`src/native_guide.rs`), the geometry
(`src/native_layout.rs`: roomy, close, no card, steppers, two options, tiny screens), setting values
(`src/native_steps.rs`: switch, number bounds, choice wrap, labels with allocator-failure sweeps).
The image holds what the builders make, and the host admits the screen
(`tests/abi7_declarations.rs`), linked entry, construction in the kernel's real registered scope
(`tests/hosted_construction.rs`: refused by the host today). The kernel tests that pinned the
interface-6 guide (`card.rs`, `guide.rs`, `guide_walk.rs`, `switch_step.rs`, `wander.rs`) went with
it.

## Acceptance pending on the host

The cases in `tests/acceptance_pending.rs` are refusals (never pass): first start (H1), first page,
text size and screen step (H1, H3, H4), skip (H1, H2, H4). The three desktop goldens of the
interface-6 guide (`x4-setup`, `x4-setup-size`, `boox-setup`) were deleted with it; no native
golden exists yet.

## Not ported yet (explicit)

- No hold-to-edit mode for a number (the guide changes it with left/right or a tap on − and +); a
  choice of many steps through its options (interface 6 could open the settings plugin's list).
- A screen step's badge ("Connected" on the card) is not read; the card says *Open*.
- A step's own help text (`setup.step` `help`) is not in the registry's reply row; the page shows
  the setting's help, and none for a screen step. Kernel adds `core.registry` op 22 `setup_step`
  (draft 24); until then an ignored acceptance case (`setup-screen-step-help`) names it.
- Right-to-left layout (the side buttons and the two options keep their order; the snapshot has no
  direction).
- The sample appears for the reader's text size only (interface 6 took any number whose unit was
  points; the registry's unit is translated text).
- The language setting lists its options through the registry's choices if it has any; unverified
  until the host serves them.
- `core.ui` opens in the skip question only; peak memory on the X4 is not measured.
- The interface-6 plugin is removed: `Setup`, `guide.rs`, the kernel tests, the app's
  `.plugin(Setup)` wiring and dependency, the three goldens. No first-run guide runs in the app until
  the host activates native plugins.
