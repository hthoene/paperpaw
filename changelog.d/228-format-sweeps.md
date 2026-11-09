### Fixed

- **Opening and reading an EPUB, a FictionBook or a comic book no longer aborts on a full heap** (#228). The package, navigation, metadata, cover and cache code of these formats answers a refused allocation with an out-of-memory refusal instead of aborting, as Markdown and HTML do; successful reads are unchanged. A sweep test refuses each allocation of an opened and fully read book in turn.
