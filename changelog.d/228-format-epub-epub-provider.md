### Added

- **format-epub answers `core.format@1` for EPUB books** (#228). `epub_provider()` serves EPUB on the shared provider (sniffed by its `mimetype` entry), with a parity test against the interface-6 reader across chapters: blocks, header fields, links and contents. The parser is unchanged. Embedded fonts, ruby kinds and inline languages follow with draft 29; the allocation-refusal case is ignored until the shared parser is fallible (Core todo 208). Nothing is active until the host calls providers.
