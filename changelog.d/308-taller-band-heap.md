### Fixed

- **The X4 no longer loses a third of its largest block of memory to the way a
  frame is drawn, and with it its fast frames** (#308). A frame of many bands is
  drawn in a taller band for the moment (#267). When the frame was done the
  usual band was asked for while the taller one's block was still held, and on
  the X4's heap (the first block that fits, lowest first) it could come to lie
  right behind it: the 24 KB then given back were a hole under the band, 35 KB
  of the largest block gone until the next restart. Whether it happened hung on
  how the start's allocations fell; since the reader's update of #235 it did at
  every start: the largest block on Home was 52 KB for 75 KB, under the 61 KB a
  taller band needs, so every larger frame was drawn in all its bands (a move
  on Home 1.2 s for 0.8 s). Now the taller band's block is given back first.
- **A largest block a little short of the room for a band of half the screen
  still draws a frame in fewer passes** (#308): the band is as tall as the
  block allows, not the usual one.
