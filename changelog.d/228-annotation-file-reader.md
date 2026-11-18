### Added

- **A book's annotation file can be read without holding it** (#228).
  `annotations::try_rows` reads the annotations of a book a line at a time,
  each text, note, client key and XPointer through a window of its own, and
  `try_head` its version and generation; neither allocates beyond the
  file's name, and a shortage of memory is `data::Error::OutOfMemory`.
