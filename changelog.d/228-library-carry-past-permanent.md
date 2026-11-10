### Changed

- **Library moves and removals go on past errors no retry cures** (#228). `library::try_moved` and `library::try_removed` still stop at the first error a retry may cure (a card error, a full card, no memory) and return it. An error no retry changes (a path the card refuses, damaged or too large data) no longer stops the other parts: the cover, the recently opened and the favourites are still carried along or forgotten, and that error is returned at the end.
