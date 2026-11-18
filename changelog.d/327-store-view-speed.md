### Changed

- The plugin store's list (`store.view`) is about twice as fast to draw: the one-line note of each row is cut to fit once, when the page of rows is made, and no longer again in every band of every frame (a frame estimated at 1.9 s on an X4 now about 0.9 s). Lists with notes (`WindowList`) no longer measure a cut note a second time.
