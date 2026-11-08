### Added

- **A catalog line can be read without holding it** (#228).
  `catalog::try_view_at`, `try_offset_of`, `try_state` and `try_generation`
  read one book's line (titles cut at their caps, tags and fields counted),
  find a book's line by its path, and read the catalog's state and
  generation, all returning `data::Error::OutOfMemory` instead of stopping
  the program when the device is short of memory.
