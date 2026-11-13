### Added

- **SDK: `native::text::hyphenate`** (#257). Native plugins ask the host where a word may break (host slot 38 `text.hyphenate`, `docs/abi/document.md` 6.1), with the tables of the installed hyphenation plugins, so the native reader can hyphenate again. Allocation-free, fallible; break places are checked before they are returned.
