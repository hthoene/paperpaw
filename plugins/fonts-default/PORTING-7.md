# Default fonts on interface 7 (ruling: stays a built-in registration)

**Ruling (#228, Manager and Plugin Engineer, `docs/abi/proposals/declarative-registration.md` Q1 and Q12):**
`fonts-default` is **not** ported to a native `.ppn` wrapper and not made a data package like the
hyphenation packs (`plugins/hyphenation/PORTING-7.md`). Its faces are built into the firmware image
(`include_bytes!` of the Literata and Source Sans files, or the packs `build.rs` draws), so a card folder's
`resource.font_face` path cannot name them, there is no `builtin:` path scheme, and a built-in plugin is not
published on its own. It stays a Rust registration outside the flat boundary, like the other built-ins the
firmware links.

## What this means for the code

- `DefaultFonts` (scalable fonts) and `DefaultFontPacks` (bitmap packs) keep their `Plugin` implementation:
  a manifest that `uses` nothing, the en/de translations, and `add_font` + `bind_font` for the reading faces
  (regular, bold, italic, bold italic), the UI face and the icons face, at the same ids, roles, families and
  weights as Beta 3. Nothing is declared through the interface-7 declaration families and the plugin has no
  callbacks, no events, no settings and no screens, so there is no interface-6 behaviour to carry over and
  no native entry to add.
- **No change to the code, so no version bump:** it is at 1.0.2, its Beta 3 version, and a bump needs a code
  change. No fragment either (documentation only).
- No `no_std` or allocation finding on a callback path: the plugin runs once, at registration. The one
  `alloc::format!` builds the text of a refused font at start-up and sits outside any input, draw or event
  callback.

## Gates that keep it right (existing tests)

- `tests/faces.rs`: every role is bound to a face of the right family, weight and slant; the text faces
  and the packs hold the Latin set at every size; bold is bolder and the UI face a little above regular;
  the icon font holds every icon the first-party plugins ask for.
- `tests/style_reads.rs`: what reading a bold pack from the card costs against the firmware's copy.
- `translations_are_complete` (en/de).
- The data plugins that go with it keep their own versions (`tools/plugin-files/data.toml`:
  `literata-styles`, `literata-large`, `literata-bold-italic`); they read this plugin's font files as inputs.

## Open item for Kernel (not this plugin)

When the interface-6 `Plugin`/`Registrar` pair goes, the firmware needs a built-in registration entry for
fonts-default's faces and roles (`bind_font`, family, translations). The ruling says it stays; the kernel
decides the shape. Until then nothing changes here.

## Not measured

Flash size of the packs and of the scalable fonts are unchanged from Beta 3 (same inputs); nothing measured
on a device.
