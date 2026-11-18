### Changed

- **The first-run hints open in less memory** (#248). Opening the hints over a screen held the buffers of its hand-over while it read the hints with a second pair, 16 KiB at once; the first pair is now gone before the hints are read, and the requests of the screen list, the hints and the hand-over take 256 bytes instead of 4 KiB. What shows is unchanged.
