### Fixed

- **The page numbers follow the reading font and the hyphenation.** The count of a book's pages was kept without them, so after another font was chosen (Literata from the store, for one) the old count was used and the page numbers were wrong. Each font and hyphenation has its count, as in Beta 3; the few counts kept stay bounded.
- **A reading font with a very long name no longer stops the book's page.** The reader falls back to the default font where the name does not fit its buffer.
