### Changed

- **Finding the page number reads the card far less** (#305). The reader found a position's page by reading the page list 8 bytes at a time, about a dozen small reads for every lookup and several lookups a page turn. It now reads the entries near the page at once and answers the following page turns from them.
