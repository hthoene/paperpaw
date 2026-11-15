### Fixed

- **A cover or picture finds room more often when memory is short** (#250). Unpacking a compressed picture asks for its largest piece of memory (32 KiB) first; asked for last, it could be refused although enough was free, and the picture's box (on a cover page, the whole page) stayed blank until the book was opened again. A picture that still finds no room no longer leaves an empty `image.tmp` in the book's cache folder.
