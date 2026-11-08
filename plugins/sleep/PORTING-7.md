# Interface-7 port (slice 1: the decisions and the declarations)

Version 1.0.4. Partial: what the screen decides and what the plugin declares, without the host. No
native screen and no `NATIVE_ENTRY` yet; the interface-6 `Sleep` and everything of it stays until
the host activates native plugins (then it goes together, no shim).

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

- Decoding and drawing the cat, the cover and the folder's pictures (the image slots 36/37 of
  PR 391 and `core.files` are not on integration); the cat is embedded
  (`pictures/sleeping-cat.png`) and decoded once per screen size.
- The screen itself (H1), the widgets composed with `ui.compose` (H3), buttons taken while asleep and
  the hidden hints (no declaration of a screen's hint visibility), closing on wake.
- The mode of another plugin (`Registrar::add_choice(sleep::SHOW, …)`) adds a choice to a setting at
  run time; the declarations carry the plugin's own seven.
