### Changed

- **Smaller declaration rows for linked plugins** (#248, D5). A plugin whose declaration batches lie close together in its image keeps one start, one kind mask and a 2-byte offset per batch instead of 12 bytes per batch (about 2 KiB less heap at start on the emulator). A plugin with one batch, or batches far apart, keeps one row per batch as before.
