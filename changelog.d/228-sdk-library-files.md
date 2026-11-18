### Added

- **Deleting and importing books in the native SDK** (#228). `Library::can_delete`
  and `Library::delete` (up to 7 paths, `DeleteReport` with one `DeleteRow` per path,
  `with_notes`) and `Library::import` (`Import`, `Existing`) with the contract's name
  checks made before the host is called.
