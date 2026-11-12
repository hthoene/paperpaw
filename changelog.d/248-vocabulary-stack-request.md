### Changed

- The vocabulary review screen asks the screen stack with a 256-byte request buffer instead of 4 KiB (the reply buffer stays at 4 KiB), 3.75 KiB less heap while it checks that it is on top.
