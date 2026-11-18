### Changed

- **Mobipocket books run on interface 7 only** (#228). The interface-6 `format-mobi` plugin is gone: a `.mobi`, `.azw` or `.azw3` file opens through the native plugin, with the same blocks, contents, links and pictures, and the converted book is kept in the host's document cache instead of a folder of the plugin. It is no longer registered by the app or offered as a store plugin there; until native plugins are switched on in the shells, these files do not open as books.
