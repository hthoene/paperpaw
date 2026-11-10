### Changed

- **FictionBooks run on interface 7 only** (#244). The interface-6 `format-fb2` plugin is gone: a FictionBook (`.fb2`, `.fb2.zip`) opens through the native plugin, with the same content. It is no longer registered by the app; until native plugins are switched on in the shells, these files do not open as books.
