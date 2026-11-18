### Added

- **Named icons in the native SDK** (#228). `IconGlyph::named(host, name)` wraps host
  slot 40 `icon.glyph`: the character of an icon name in the bound icon font, drawn
  with `text.draw` through a `role:icons` font. Malformed names are refused before
  the host is asked, and `NotFound` means the plugin shows text instead.
