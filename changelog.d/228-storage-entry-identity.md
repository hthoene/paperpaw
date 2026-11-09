### Changed

- **Plugin interface 7, draft 33** (#228): storages tell an entry by its own identity (`Storage::entry_identity`: the first cluster on a memory card, the file id in a computer's folder), so the library can tell after a power cut whether a book's move was done; storages that wrap another (the card swap, the emulator's and the X4's traces, Android's private files) now pass this and `same_entry` on. Plugins built for draft 32 must be rebuilt.
