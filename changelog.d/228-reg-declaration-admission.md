### Added

- **The host admits plugins' registry declarations** (#228). Slot specs and
  offers, key bindings, setting defaults and extra choices for other plugins'
  settings, and presets with their values are now kept with the plugin's other
  declarations instead of refusing the whole batch. A bad record still refuses
  its whole batch, an offer must name a screen the same plugin declared, and
  all of a plugin's registry declarations go when it is unloaded.
