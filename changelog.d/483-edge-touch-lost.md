### Fixed

- **A tap is not lost after a touch at the screen's edge** (#483). Where
  the lifting of a finger at an edge was not seen (a plugin took it, the
  device slept between), the next touch was swallowed or taken for a
  swipe from that edge.
