### Fixed

- **Words of your vocabulary list are marked in books again** (#257). The vocabulary plugin read the thickness of the mark into a buffer too small for it, so it never answered the reader and no word was marked. The buffer is now large enough.
