### Changed

- **Widgets: no writable static data** (#228). The quote widget's count of showings is held by the plugin, not in a global, so the widget pack can be packed as a native plugin file. Nothing changes on the screen.
