### Changed

- **Plugin interface 7, draft 33** (#228): storages tell an entry by its own identity (`Storage::entry_identity`: the first cluster on a memory card, the file id in a computer's folder), so the library can tell after a power cut whether a book's move was done; storages that wrap another (the card swap, the emulator's card and traces, the X4's trace, Android's private files) now pass this and `same_entry` on, and the SDK's `MemoryStorage` keeps an identity per file and folder, so moves work in the in-memory emulators too. Plugins built for draft 32 must be rebuilt.
