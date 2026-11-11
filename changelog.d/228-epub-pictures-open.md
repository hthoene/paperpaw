### Fixed

- **A book with a picture before its first text opens in the native reader** (#243). The EPUB family decodes a picture when a page shows it, so the first block of a book that starts with a picture (a title page, a cover page) named a file that was not there yet; the host left the picture out and then refused the picture block, and with it the whole book ("could not be opened"). The picture is now decoded before its block is sent. A picture that cannot be served at all goes out as a paragraph with the block's text, so one broken picture costs only itself, never the book.
