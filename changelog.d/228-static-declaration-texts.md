### Changed

- A linked native plugin's declared titles, descriptions and choices are read from the program
  instead of being copied to the heap, and its ids are taken from a table its build makes
  (`fixed_ids_source`, `FixedTables::with_ids`) where it has one (#248).
