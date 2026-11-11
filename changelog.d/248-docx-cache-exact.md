### Changed

- **A Word document converts in less memory** (#248). The cache's blocks were collected in a buffer that grew by doubling (old and new copy held together), and could wait for the card with several blocks in it. They are now sent to the card before a block would not fit with them, and each gets its room once, to the byte: the hostile Word document of the X4 memory test peaks at 211.8 KiB instead of 222.2. What is written is unchanged.
