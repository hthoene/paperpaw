### Added

- **Library reads can be refused for lack of memory instead of aborting** (#228).
  `library::try_book`, `try_book_sized`, `try_status`, `try_key`, `key_into`,
  `catalog::try_changes`, `try_count`, `try_path_at` and `Format::try_load`
  return `data::Error::OutOfMemory` when the device is short of memory.
  `library::book` and `library::key` keep working as before (a shortage
  still stops the program, never answers "no such book").

### Changed

- **`StorageError::OutOfMemory`** (#228). A read that the heap could not hold
  used to be reported as `TooLarge`, which means "never works"; it is now
  `OutOfMemory` ("try again"). This is a new variant in a full interface draft.
