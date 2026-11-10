### Changed

- **Menu editor on the new plugin interface: refreshes when the registry changes** (#228). The plugin now
  runs in the host's real registered scope with its registry-change subscription admitted (#916),
  so its rows follow changes of the plugins, menus and presets without reopening the screen.
