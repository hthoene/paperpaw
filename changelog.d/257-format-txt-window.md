### Changed

- **TXT books: the text read around the page is never held twice** (#248, #257). Where the plain-text reader needs a larger piece of the book, it lets go of the old piece and reads it again instead of copying it, so the reader's chapter scan stays within Beta 3's memory on normal books.
