### Fixed

- **Vocabulary no longer adds an empty line to a deck after a card error** (#248). When the last byte of a deck file could not be read, the next card was written after an extra blank line; a byte that cannot be read now counts as the end of a line, as before the host cleared failed reads.
