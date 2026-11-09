### Changed

- **Hyphenation plugins are data only** (#228). The English pack and the twelve language packs no longer have an interface-6 code plugin; they are data packs (`hyphenation/<lang>.pphp`) the reader reads from the card. Hyphenation now comes only from the installed data packs: English is no longer built in, so a device without the English pack does not hyphenate English until it is installed. The packs keep their ids and are offered as updates.
