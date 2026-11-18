### Added

- **Fallible library moves and removals** (#228). `library::try_moved` and `library::try_removed` carry along or forget what the library knows of a book (its details, its cover and its place among the recently opened and the favourites) and return the first card error instead of dropping it. What is already done counts as done, so a host step that fails can run again and finish.
