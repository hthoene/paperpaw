### Fixed
- Less memory at the start with a large library: the host reads native books' details (an EPUB's title and author) only once the device has been left alone for a second after the start or an input, and not while the library is first walked, so an EPUB's open no longer adds to the start's peak (#248).
