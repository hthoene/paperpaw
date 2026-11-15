### Fixed

- **A converted-book index with billions of blocks can no longer overflow an offset** (#257). Two reads of the index still multiplied a block number by the entry size in 32 bits before widening it. They now widen first, so a number out of range reads nothing instead of panicking (debug) or reading the wrong place (release).
