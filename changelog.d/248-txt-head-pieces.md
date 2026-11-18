### Changed

- Opening a text book reads its 16 KiB head in one pass of 8 KiB pieces (encoding, Project Gutenberg header, the text's look) instead of holding it whole: the library's details open of a text no longer sets the start memory peak, with about as many card reads as before (#248). A Project Gutenberg header line of 8 KiB or more is now left out (no real header has one).
