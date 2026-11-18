### Added

- **Book key, fingerprint and reading position in the native SDK** (#228).
  `Library::book_key` (op 25) reads a book's key, KOReader fingerprint and size
  (`BookKey`), and `Library::position` / `Library::set_position` (ops 13, 14) read and
  write its reading position (`Position`, `PositionWrite`) with the sync and
  confirmed flags, so kosync can match other devices and keep a local place over a
  sync one.
