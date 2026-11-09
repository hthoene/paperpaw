### Changed

- **Hyphenation plugins are data only** (#228). The English pack and the twelve language packs no longer have an interface-6 code plugin; they are the data packs (`hyphenation/<lang>.pphp`) the reader already reads from the card. Nothing changes for a reader: the packs keep their ids and are offered as updates.
