### Changed

- **Smaller kernel rows for each plugin** (#248). The rows the kernel keeps for every linked plugin and its screens take less memory: a plugin whose declarations are read from the program keeps no room for an index, the set of events a plugin subscribed to holds the 128 event tags there are (not 256), a screen's row reads how many instances it may have from its flags, and a plugin's row keeps a waiting replacement as the bytes of its folder. About 2 KiB less at the start and while reading (emulator); nothing a plugin sees changes.
