### Added

- **Annotations in the native library SDK** (#228). `native::library::Library`
  reads a book's annotations in borrowed pages with their own cursor and counts,
  and reads the complete text or note of one annotation in bounded UTF-8
  chunks, so nothing is cut short.
