### Changed

- **Smaller plugin files** (#228). The event, draw and input entry points of
  a plugin are thin shims over code shared by all plugins, and the UI kit and
  the config parser slice text with `.get(..)`, so the `char`/`str` `Debug` and
  Unicode tables are no longer linked. Battery 89,640 → 82,944 bytes, End of
  book 244,620 → 228,912, Profiles 130,956 → 123,928. Behaviour is unchanged.
