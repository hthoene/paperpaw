### Fixed

- **Home no longer draws its whole page again a few seconds after you moved
  the focus** (#308). Each minute Home laid the page out anew and redrew all of
  it (1.8 s on the X4, a key press waiting for it) whether anything had
  changed. Now a minute that shows nothing new draws nothing, and the clock,
  the date or the status line that changed draw only their own place.
