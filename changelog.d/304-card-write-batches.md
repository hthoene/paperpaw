### Changed

- **Neighbouring changed card blocks are written in one command** (#304). The block cache writes changed blocks that were changed one after the other and lie one after the other on the card (up to 8) in a single write command; the order that keeps the card safe after a power cut is kept.
