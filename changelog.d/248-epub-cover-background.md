### Changed

- **A book's cover for the library is made without building the book** (#248). The library's cover open is a background open now (`document.md` 2.1), and a format skips what only a shown book needs: the EPUB family's open no longer builds the contents, the style cache and the first chapter for a cover (about 10 KiB less at the peak, once per book). A normal open still finds everything out. With both X4 document places held by other openers' documents the cover open is `ERR_FULL` (the library says busy) instead of taking a place.
