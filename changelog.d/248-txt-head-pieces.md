### Changed

- Opening a text book reads its head (encoding, Project Gutenberg header, the text's look) 4 KiB at a time instead of holding 16 KiB at once, and looks for the end marker in 4 KiB pieces (was 8 KiB): the library's details open of a text no longer sets the start memory peak (#248).
