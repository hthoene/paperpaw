### Fixed

- Format providers (XTC, Word, Mobipocket, picture folders and the EPUB family: EPUB, CBZ, FB2, HTML, Markdown): the end of a paged list (sections, contents, links, fonts) now carries the last-page bit on the final page and a generation that is never 0, as `document.md` requires. The host refused every such list, so a book could not be opened.
