### Fixed

- **A deflated book chapter of some sizes (about 27 to 28 KiB) was refused as "entry longer than its size"** (#4309). When the reader's window, sized to the entry, was filled to the last byte before the decoder had seen the end of the stream, the first "more output" answer was taken for an overlong entry. Only an answer that still wants to write after the window is full counts now.
