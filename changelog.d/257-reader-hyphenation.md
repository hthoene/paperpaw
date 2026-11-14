### Fixed

- **The native reader breaks words at the end of a line again** (#257). With *Hyphenate words* on, long words are split with a hyphen where the language's table allows it (the host holds the tables), keeping the letters the *letters kept* settings ask for, as Beta 3 did. A language with no table, or the setting off, leaves words whole.
