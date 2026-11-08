### Changed

- **The hyphenation plugins are data plugins in the release** (#228). English and the twelve store languages are built by `tools/hyphenation-packs` into code-free packages (`plugin.ini` with `[hyphenation]`, `hyphenation/<lang>.pphp`, format 5) and listed in `tools/plugin-files/data.toml` at 2.0.0, newer than Beta 3's 1.0.x, chosen at first like before. They leave `tools/plugin-files/plugins.txt`, so each id has one catalogue entry. The interface-6 hyphenation crates stay until their removal follows.
