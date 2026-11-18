### Fixed

- **Emulator: downloads of a scripted run land on the card** (#228). On simulated time (tests, `--script`) the emulator handed `core.net` and `core.sockets` the secrets' store in memory, so a native plugin never found a file it had downloaded (the fonts browser's list said the library did not answer). They save on the card now, where the plugins read them.
