### Added

- **format-epub answers `core.format@1` for Markdown and HTML files** (#228). A shared provider serves any of the crate's document parsers (first Markdown and HTML) in the interface-7 way: blocks come in fragments, with text, runs, source maps, language, direction and writing mode, and bad calls get their refusals. The converter itself is unchanged; footnote and ruby kinds, inline languages and embedded fonts follow with draft 29. Nothing is active until the host calls providers.
