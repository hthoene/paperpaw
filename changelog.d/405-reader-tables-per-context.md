### Changed

- **Less memory for the reader's letter tables** (#405). A book's page tables of letter widths and kerning pairs (about 1.5 KB per font and size) are now made when a font and size is first set, not all four at once: a plain-text book or one without italics holds 1.5 to 3 KB less while it is read. Page turns are as fast as before.
