### Added

- **A catalog line can be read without holding it** (#228).
  `catalog::try_view_at`, `try_offset_of`, `try_state` and `try_generation`
  read one book's path and numbers, count its tags and fields, find a
  book's line by its path, and read the catalog's state and generation.
  `catalog::try_text_at` reads a title, author, series, tag or field from
  a given byte on into the caller's buffer. All return
  `data::Error::OutOfMemory` instead of stopping the program when the
  device is short of memory.
