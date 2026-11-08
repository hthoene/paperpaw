### Changed

- **XTC and picture books get their interface-7 format providers** (#228).
  The `format-xtc` and `format-images` plugins can now answer the host's
  `core.format` calls (open, metadata, pages, contents, covers and pictures).
  They are not active yet: the host switches to them with the other
  interface-7 plugins.
