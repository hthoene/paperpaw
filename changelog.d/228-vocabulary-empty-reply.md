### Fixed

- **Vocabulary answers a page with nothing to mark properly** (#257). With the marks off, on a page without a listed word, and for a tap on another plugin's mark, the plugin sent no bytes at all, which the host refuses as no answer: a refused call was logged for the plugin on every page. It now sends an empty batch.
