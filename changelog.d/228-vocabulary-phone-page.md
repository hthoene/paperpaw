### Added

- **Vocabulary on interface 7: Edit with your phone** (#2861, #257). The flash cards' *Edit with your phone* row is back on the native plugin: it serves the page for decks and cards from the device, shows its address as text and as a QR code, and guards every request with a key made for the screen. A wrong key is refused (and too many wrong ones lock it out), a request that is malformed or too long gets a clean refusal, and a device without a network says so.

### Fixed

- **Flash cards: a deck or card that is made or changed is not reported as failed when only its index could not follow.** Out of memory while the index was brought along left the deck or card done but answered an error, so a retry (the phone page's "try again") made it twice. The index is made anew when it is needed.
