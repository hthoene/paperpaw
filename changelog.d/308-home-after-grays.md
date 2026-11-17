### Fixed

- **Home no longer draws its whole page again a few seconds after you moved
  the focus** (#308). Each minute Home laid the page out anew and redrew all of
  it (1.8 s on the X4, a key press waiting for it) whether anything had
  changed. Now a minute that shows nothing new draws nothing, and the clock,
  the date or the status line that changed draw only their own place.

### Changed

- **Pictures turn gray only after 30 s of rest** (#308, Decision 216). A
  screen with pictures (Home with its covers) is shown in black and white
  first; the grays now follow once there has been no key or touch for 30 s, not
  3 s. The gray pass ends in a 1.7 s clean to white on the X4, which the next
  move paid; moving around on Home no longer meets it. Every key press begins
  the 30 s again, and a pass that has started is still stopped by a key.
