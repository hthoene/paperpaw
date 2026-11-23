### Fixed

- **Settings that change how an open page looks take effect at once again**
  (#483). A plugin could not set its screen's look from an event: the
  reader's *Smooth text* switched on or off while a book was open, and the
  home screen's *Buttons row*, applied only after the book or the screen
  was opened again. A preset a plugin applied from an event or an action
  was accepted and then left out.
