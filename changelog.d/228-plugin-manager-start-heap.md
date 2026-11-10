### Changed

- **Plugin manager (interface 7): lower start heap on the X4** (#248). The start reads only the names its notices need from the registry, and the *Screens* offers share one pair of read buffers, so the store flow peaks about 8 KiB lower.
