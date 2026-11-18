### Added

- **`core.registry` answers the settings and presets ops** (#228). The host
  side of `core.registry` lists the settings by group or plugin with their
  kind, flags, source and value (op 1), gives one setting with its editor
  fields and help text (op 2), lists a choice setting's own and added choices
  (op 3), and lists the presets, one preset and the values a preset or a
  widget placement sets (ops 16, 17 and 18). A value is shown to a plugin
  exactly when slot 5 would give it; otherwise the row is empty and flagged
  hidden. A secret is never shown, only that it has a value, and the grants
  only to the settings screen.
