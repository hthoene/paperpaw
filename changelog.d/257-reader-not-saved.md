### Fixed

- **Reader: "could not be saved" only says the memory may be full when it is** (#257). Opening the last book at the start, before the library had listed it, showed "Where you are in the book could not be saved. The memory may be full." The reader now tries such a place again at the next save without a notice, and says why a place, bookmark, highlight or note was not saved: a lack of memory, a full memory card, or just that it was not saved.
