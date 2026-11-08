### Added

- **Library book details can be read without risking an abort** (#228).
  `library::try_book`, `try_book_sized`, `try_key`, `key_into` and
  `Format::try_load` return `data::Error::OutOfMemory` when the device is
  short of memory instead of stopping the program. `library::book`
  and `library::key` keep working as before.
