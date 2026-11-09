### Added

- **Provided metadata in the native SDK** (#228). `Library::provide` (LIB op 24, `uses.library`) with
  `Provided` (title, author, series, number, tags) lets a plugin such as calibre set what it knows
  better than a book's file; only the fields set are provided.
