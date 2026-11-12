### Fixed

- **The power plugin's event handler no longer takes over 2 KiB of the X4's stack** (#228). It decoded the eight kinds of event it handles into one array, a frame of 2,064 bytes (the stack's band is 2 KiB high); it now tries one kind after the other, and the frame is 144 bytes. A check now makes sure no linked plugin's entry frame reaches 2,047 bytes.
