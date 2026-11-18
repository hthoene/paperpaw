### Changed

- **Pictures as books are an interface-7 plugin** (#228). `format-images` declares its format
  (`jpg`, `jpeg`, `png`, `gif`; the pictures of the folder follow the opened one as pages) and
  answers the host's `core.format` calls from a native entry, with the same pages as before.
