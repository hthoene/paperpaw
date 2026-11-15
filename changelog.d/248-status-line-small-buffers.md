### Changed

- **A page of a book takes about 8 KiB less memory to draw.** The status lines under and over the text held two reply buffers of 4 KiB and a third of 4 KiB for their request while the registry's answer was read. They now make one reply buffer (a second only for more than eight items) and a request buffer of the size of its items, after the registry's answer is read. On the X4 a drawn page peaks 16.9 KiB above the heap in use in the emulator, now 9.0 KiB. The pages and status lines are pixel for pixel the same.
