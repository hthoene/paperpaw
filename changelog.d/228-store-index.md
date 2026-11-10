### Changed

- **The plugin store's index** (#228). The store keeps the library's plugins in an index on the card as before, now written and read by the new interface-7 code: the same files and format, a list of any length with little memory, and a refused allocation or a card that fails ends in an error instead of a crash.
