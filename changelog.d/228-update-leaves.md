### Added

- **Update without the plugins the new firmware leaves out** (#228). When a firmware update leaves out
  plugins that are built into PaperPaw now, the update page names them and offers *Update without them*
  (for an update on the card, from the device's own check of the file) or *Download without them* (for an
  update the server named, from the plugins its answer lists), next to *Not now* to wait. No built-in
  plugin is dropped without being named; an update that waits for plugins and is chosen anyway names
  everything that is gone after it. The *New* mark is back on Update in the settings while an update the
  server named is kept, and goes when it is downloaded or no longer newer.
