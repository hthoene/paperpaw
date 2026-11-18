### Changed

- The Plugin store's setup pages (*Choose a setup*, *Add more*) and every list with notes draw faster: a note that fits one line is measured once, not word by word, in every band of a frame (frames estimated at about 0.99 s and 0.70 s on an X4 are now about 0.85 s and 0.57 s). The dev tool's `perf` no longer reports a request the store list leaves under way when it closes as a leak (the +628 B per opening was that request's state on the emulator's heap, ended later).
