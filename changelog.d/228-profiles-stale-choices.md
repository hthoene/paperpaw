### Fixed

- **Reading profiles skip a stale choice** (#228). A profile that keeps a choice the setting
  no longer offers (a font that was removed) applies the rest of its values instead of
  failing, as before. A profile that does not fit the command budget applies nothing.
