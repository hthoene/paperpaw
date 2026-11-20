### Fixed

- Secure connections after reading a book in a script such as Devanagari: the font pack kept its substitution table (about 15 KB) and the advances of its sizes in memory through every other screen, so on the X3 and X4 the plugin store could fail with "Not enough memory for a secure connection". Before a secure connection (and where a screen asks for room) the packs now let go of them; they are read again from the card when the next page needs them (#410).
