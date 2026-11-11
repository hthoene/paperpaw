### Fixed

- **Text and XTC books open in the native reader** (#243). Formats TXT and XTC are linked native plugins now where the reader is (`native-linked-reading`), so `core.document` finds a format for them; their interface-6 forms are left out of the same builds. TXT's lists (sections, contents, links, fonts) end with the last-page flag and a generation, as the host checks them: before, the host refused them and the book would not open. XTC needs Services' list-end PR (`services/fmt-list-end-243`) for its own lists: merge both together.
