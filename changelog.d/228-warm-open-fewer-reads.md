### Fixed

- **A book opened again, and every page turn, read the card a third as often.** A block of an EPUB, FictionBook or HTML book was read from the cache with three reads (index, header, text); now the record is read whole with one, and the index 32 entries at a time. A page of 25 blocks takes 25 reads instead of 81, and the whole of Alice 1,009 instead of 2,706. The files are unchanged.
