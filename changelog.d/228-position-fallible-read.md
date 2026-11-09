### Added

- **A book's reading position can be read without risk to memory** (#228).
  `positions::try_load` reads where a book was left and returns
  `data::Error::OutOfMemory` instead of stopping the program when the
  device is short of memory.
