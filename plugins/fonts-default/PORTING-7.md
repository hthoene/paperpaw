# Default fonts on interface 7 (a table of built-in fonts, no plugin code)

**Ruling (#228, Manager and Plugin Engineer, `docs/abi/proposals/declarative-registration.md` Q1 and Q12):**
`fonts-default` is **not** ported to a native `.ppn` wrapper and not made a data package like the
hyphenation packs (`plugins/hyphenation/PORTING-7.md`). Its faces are built into the firmware image
(`include_bytes!` of the Literata and Source Sans files, or the packs `build.rs` draws), so a card folder's
`resource.font_face` path cannot name them, there is no `builtin:` path scheme, and a built-in plugin is not
published on its own. It stays a registration of the firmware outside the flat boundary.

## What the code is now (#244)

- The crate implements no `Plugin` any more (the interface-6 trait): it is tables in the program,
  `DEFAULT_FONTS` (scalable fonts where the build has them, else packs), `DEFAULT_FONT_PACKS` and
  `LEAN_FONT_PACKS`, each a `paperpaw_sdk::BuiltInFonts` (kernel feature of the SDK, so nothing a
  plugin sees): who brings the fonts (id, name, version, description, *How it works*), the en/de
  translations, and per font its id, role, data, weight, family and style.
- The kernel registers a table with `KernelBuilder::built_in_fonts(&TABLE)`, in the place among the
  plugins where it is named. It is listed as before (`fonts-default`, "Default fonts").
- Same ids, roles, families, weights and order of registration as Beta 3 and as the `Plugin` it replaces;
  `tests/faces.rs` holds the same expectations as before, now against the tables.
- Version 1.0.3 (the code changed).

## Gates that keep it right (existing tests)

- `tests/faces.rs`: every role is bound to a face of the right family, weight and slant; the text faces
  and the packs hold the Latin set at every size; bold is bolder and the UI face a little above regular;
  the icon font holds every icon the first-party plugins ask for; what a lean build binds.
- `tests/style_reads.rs`: what reading a bold pack from the card costs against the firmware's copy.
- `translations_are_complete` (en/de).
- The data plugins that go with it keep their own versions (`tools/plugin-files/data.toml`:
  `literata-styles`, `literata-large`, `literata-bold-italic`); they read this crate's font files as inputs.

## Not measured

Flash size of the packs and of the scalable fonts are unchanged from Beta 3 (same inputs); nothing measured
on a device.
