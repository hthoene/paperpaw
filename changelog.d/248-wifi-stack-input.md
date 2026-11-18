### Changed

- **Wi-Fi reads the screen stack with a small input buffer** (#248). Checking whether home or a
  book is on top took two 4 KiB buffers; the query's input now takes 256 B, 3.75 KiB less at the
  peak of a store install.
