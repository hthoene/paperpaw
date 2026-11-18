### Added

- **Whether a book was read for its title and cover can be asked without
  reading its details** (#228). `library::try_indexed` scans the details
  file a line at a time and holds none of it, and returns
  `data::Error::OutOfMemory` instead of stopping the program when the
  device is short of memory.
