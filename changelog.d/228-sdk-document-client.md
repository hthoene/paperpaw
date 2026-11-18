### Added

- **A document client in the native SDK** (#228). `native::document::Documents` opens
  `core.document` and opens a book through its format (`open_book`, with `OpenBook`:
  root, format, flags, picture sides, user CSS and the format's options), reads its
  preparation state (`state`, `DocState`, `Preparation`), closes it (`close`) and
  converts a position to a KOReader XPointer and back (`xpointer`, `locate`,
  `DocPosition`).
