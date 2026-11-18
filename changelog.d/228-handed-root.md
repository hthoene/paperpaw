### Fixed

- **A file a plugin hands to the reader opens as the handed file** (#228). The reader read a hand-over with `root` `0` as its own data folder and `2` as a library book; it now opens `0` (the maker's data folder) and `2` (its install folder) as `doc_open` root 5, the read-only file the hand-over names (`document.md` 1). *Your own fonts* and the licences page of About can open their text in the reader this way (the plugin side follows in their own PRs).
