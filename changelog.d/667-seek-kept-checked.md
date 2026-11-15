### Fixed

- **A damaged converted-book index can no longer overflow a read** (#667). Reading the index of a converted chapter multiplied and added block numbers without checking; a block number at the end of the range could panic in a debug build and read the wrong place in a release build. The numbers are now checked and a number out of range reads nothing.
