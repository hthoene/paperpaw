### Fixed

- **Large pictures in books use less memory** (#316). A large PNG in a deflated entry of a book (a 1600 x 1000 picture) no longer holds two inflate windows at once: it is copied to a file first and decoded from there, one window at a time, so its page peaks at 214 KiB instead of 260 KiB on the X4. Long JPEGs in deflated entries go the same way (213 to 185 KiB).
