### Fixed

- **EPUB, HTML, FB2, Markdown, CBZ, DOCX, images and MOBI books open in the native reader** (#243). The format plugins are linked native plugins now where the reader is (`native-linked-reading`; DOCX, images and MOBI where `native-linked-store` links the store's plugins), so `core.document` finds a format for them instead of answering "not found"; their interface-6 forms are left out of the same builds.
