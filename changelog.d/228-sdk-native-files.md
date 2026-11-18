### Added

- **Native SDK: typed folder and file service** (#228). `native::files::Files` lists folders
  page by page, reads file information and free space, makes folders, moves entries and
  removes a folder tree step by step, all into buffers the plugin owns (`core.files@1`).
