### Changed

- **Built-in texts no longer cost a card read for every installed store plugin** (#305). The text catalogs of store plugins on the card are asked before the built-in texts, and such a catalog could only say "not mine" by reading a block from the card: after every change of screen, each built-in text cost one card read per installed store plugin (5 ms a text with three of them). A compiled catalog now says the first and the last of its keys, and a text outside them is not looked for on the card. The catalogs in the card's cache are compiled anew once.
