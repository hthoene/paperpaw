### Changed

- **Less stack where a plugin lists or reads the card** (#250). The card's folders are looked up in a loop instead of one frame per folder level, and the one-time look for a half-finished move after the start takes a fraction of the frame it did, so the deepest route from a plugin call to a block read on the X4 is about 1.5 KB shorter (more where the folder is deeper). Nothing on the card is written differently.
