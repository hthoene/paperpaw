### Changed

- **The reader reads the screen stack with a small input buffer** (#248). Checking which screen is
  on top before reading aloud took two 4 KiB buffers; the query's input now takes 256 B.
