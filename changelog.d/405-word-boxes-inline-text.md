### Changed

- **Less memory for the words of the page on show** (#405). The reader keeps each word's text inside the word's box record when it is short (up to ten bytes, nine words of ten in English), where each word had a small block of its own: about 1.5 KB less on a typical page of ninety words, and ninety fewer allocations at every page turn.
