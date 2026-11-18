### Added

- **format-epub answers `core.format@1` for FictionBook and comic book files** (#228). `fb2_provider()` and `cbz_provider()` serve FictionBooks (also zipped) and comic archives on the shared provider, with parity tests against the interface-6 readers (blocks, links, contents, pages, cover, picture). The parsers are unchanged; footnotes stay links of the note kind. The allocation-refusal acceptance cases are ignored until the shared parser is fallible (Core todo 208). Nothing is active until the host calls providers.
