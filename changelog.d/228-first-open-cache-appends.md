### Fixed

- **A book opened for the first time is ready sooner.** The cache of an EPUB, FictionBook, HTML or Markdown book is written in pieces of 2 KiB (1 KiB for the contents and indexes) instead of 256 B to 1 KiB, and a UTF-16 text file in steps of 2 KiB instead of 1 KiB. Each write is a call to the card (about 27 ms on the X4), so the first open of Alice makes 279 instead of 659. The cache files are byte for byte the same.
