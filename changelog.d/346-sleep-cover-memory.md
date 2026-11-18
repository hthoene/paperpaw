### Fixed

- **The sleep screen in cover mode shows the book's cover on the X4, not the cat**
  (#346). The screen-sized cover is decoded inside the call and the X4's heap
  does not hold that; the screen gave up on "no memory" instead of asking for
  the list's cover Home shows. It asks for that one now.
