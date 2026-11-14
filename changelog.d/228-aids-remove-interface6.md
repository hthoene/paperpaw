### Changed

- **Reading aids are interface 7 only** (#228). The interface-6 plugin and its text filters (bold word beginnings, replaced words, pictures) are removed from the app and the plugin file build; the native plugin declares the same three settings and answers the reader's `reader.filter` stage. The rules apply again once the reader asks the stage (`BLOCKED READER-filter-fire`).
