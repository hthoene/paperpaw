### Changed

- Statistics, battery and notes-overview, where linked (the emulator's store set), keep their settings, setting groups, ids and titles in tables built into the program instead of copies on the heap: about 6 KiB less heap with all three switched on (statistics 5.4 KiB, battery 0.4 KiB, notes-overview 0.2 KiB). No change on a device that loads them as plugin files.
